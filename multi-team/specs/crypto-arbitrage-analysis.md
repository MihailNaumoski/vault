# Architectural Analysis: Crypto Arbitrage Systems

**Date**: 2026-04-05  
**Author**: Architect Agent  
**Status**: Complete  
**Artifacts Analyzed**:
1. Prediction Market Arbitrage Bot (Polymarket/Kalshi) — conceptual flowchart
2. `SPEC.md` — Rust cross-exchange crypto arbitrage system (Binance/Coinbase/Kraken)

---

## 1. Architecture Quality Assessment — SPEC.md

### 1.1 Workspace & Crate Structure — Grade: A-

The 7-crate workspace is well-decomposed:

| Crate | Role | Assessment |
|-------|------|------------|
| `arb-types` | Shared domain types, zero internal deps | ✅ Correct. Foundation crate with no coupling. |
| `arb-exchange` | Connector trait + implementations | ✅ Good separation. Each exchange is a module. |
| `arb-engine` | Detection + execution | ⚠️ Does too much. Detection and execution have different change rates. |
| `arb-risk` | Risk controls, circuit breaker, PnL | ✅ Proper isolation of safety-critical code. |
| `arb-db` | Persistence layer | ✅ Clean repository pattern. |
| `arb-server` | REST + WebSocket API | ✅ Thin layer, appropriate scope. |
| `arb-cli` | Binary entry point | ✅ Minimal — config loading and shutdown only. |

**What works:**
- `arb-types` as a zero-dependency foundation is textbook correct. All money types use `rust_decimal`, all timestamps are `DateTime<Utc>`, all IDs are UUID v7. These are non-negotiable for a financial system and the spec gets them right.
- Separating `arb-risk` from `arb-engine` is critical. Risk controls must be independently testable and auditable. Good call.
- `arb-db` behind a repository trait means you can test the engine without Postgres. Correct.

**What could improve:**
- `arb-engine` conflates detection (pure computation) with execution (side-effectful I/O). These should be separate crates: `arb-detector` (pure, stateless, trivially testable) and `arb-executor` (stateful, manages order lifecycle). The spec even acknowledges the detector is a "pure function — no side effects" but then bundles it with the executor, which retries, unwinds, and writes to channels.
- No `arb-metrics` crate. Prometheus is listed as a Phase 2 feature but metrics should be baked into every component from day one. Retrofitting metrics into hot paths is painful.

### 1.2 Dependency Graph — Grade: A

```
arb-cli → arb-server → arb-engine → arb-exchange → arb-types
                     → arb-risk   → arb-types
                     → arb-db     → arb-types
```

**Strengths:**
- Strict DAG — no cycles, no unnecessary cross-dependencies
- `arb-types` is a leaf. Nothing can pull in the engine or server transitively.
- `arb-exchange` doesn't depend on `arb-engine` — connectors are unaware of arbitrage logic. This is correct and enables reuse.

**Weakness:**
- `arb-cli` directly depends on `arb-exchange` AND `arb-server`. The stated reason isn't clear. If the CLI needs to initialize connectors before passing them to the server, that's a wiring concern — but it means changes to exchange connectors force a recompile of the binary even if the server crate already re-exports everything. Minor but worth noting for build times in a Rust workspace.

### 1.3 Trait-Based Connector Abstraction — Grade: B+

The `ExchangeConnector` trait is well-designed:

```rust
#[async_trait]
pub trait ExchangeConnector: Send + Sync + 'static {
    fn exchange_id(&self) -> ExchangeId;
    async fn fetch_order_book(...) -> Result<OrderBook, ExchangeError>;
    async fn subscribe_order_book(..., tx: mpsc::Sender<OrderBookUpdate>) -> ...;
    async fn place_order(...) -> Result<OrderResponse, ExchangeError>;
    // ...
}
```

**What works:**
- `Send + Sync + 'static` bounds — correct for sharing across tokio tasks.
- Takes `mpsc::Sender` for order book subscriptions — clean push model.
- Returns exchange-specific error type, not `anyhow::Error` — enables pattern matching on failure modes.
- `fee_schedule()` is synchronous and returns a reference — implies fees are loaded once at startup.

**Concerns:**
- **`fee_schedule()` is a snapshot, not live.** This is assumption A4 in the spec: "Exchange fees are known at startup and treated as constant." This is wrong. All three exchanges (Binance, Coinbase, Kraken) have tiered fee schedules based on 30-day volume. As the bot trades, its fee tier changes. Starting at Binance maker/taker 0.10%/0.10%, but at >$50M/month volume you're at 0.02%/0.04%. More importantly, **competitors at higher tiers pay less in fees and can profitably execute opportunities that are unprofitable for you**. A static fee schedule means the bot will slowly miscalculate profitability as its 30-day volume accumulates.
- **No `get_trade_history()` or `get_open_orders()` method.** Post-restart reconciliation is impossible. If the bot crashes between placing leg 1 and leg 2, it has no way to discover the dangling order on restart. This is a critical gap for a system handling real money.
- **`cancel_order` takes a string ID** — but order IDs have different formats per exchange. Binance uses numeric IDs, Coinbase uses UUIDs, Kraken uses alphanumeric. This works but loses type safety.

### 1.4 Async Architecture — Grade: B+

**Channel topology:**
- `mpsc::Sender<OrderBookUpdate>` — exchange connectors → engine aggregator
- `broadcast::Sender<SystemEvent>` — engine → server (WebSocket) + risk manager
- `mpsc::Sender<DbCommand>` — engine → DB writer (fire-and-forget persistence)

**What works:**
- The hot path (book update → detect → execute) stays in a single task, minimizing context switches. Good.
- DB writes are decoupled via mpsc with a batching writer (100 items or 50ms window). This is correct — you never want a database round-trip in the trading critical path.
- `tokio::join!()` for concurrent leg execution. The right primitive — both legs fire simultaneously, you wait for both.

**Concerns:**
- **Single-threaded engine loop.** The main loop is `loop { rx.recv(); detect; execute; }`. This is sequential — while executing a trade (up to 5 seconds with retries), no new opportunities are processed. The spec doesn't discuss spawning execution into separate tasks. At 500ms execution time and even moderate opportunity frequency, this is a bottleneck.
- **Backpressure story is incomplete.** The DB writer "drops writes under backpressure" — but which writes? Dropping trade records means the PnL tracker drifts from reality. The spec should distinguish between droppable events (opportunity detections) and critical writes (executed trades).
- **No explicit channel buffer sizes.** `mpsc::channel(?)` — the capacity matters. A buffer of 1 means the exchange connector blocks when the engine is executing a trade. A buffer of 10,000 means 10,000 stale order books queue up during a slow period and all get processed as a burst.
- **WebSocket broadcast** — `broadcast::Sender` drops messages when subscribers are slow. This is fine for a monitoring dashboard, but the spec doesn't mention this behavior to the operator.

### 1.5 Risk Management Layer — Grade: A-

The risk module is the strongest part of the spec. The 8-step pre-trade check sequence is correctly ordered for fail-fast:

1. Circuit breaker (cheapest check — one boolean)
2. Daily trade count
3. Daily PnL
4. Opportunity expiry
5. Order book staleness
6. Position size
7. Total exposure
8. Open position count

**What works:**
- Circuit breaker auto-trips on 3 consecutive failures or daily loss limit. Both are correct triggers.
- Daily drawdown limit with automatic halt. Non-negotiable for capital safety. ✅
- Position tracking per-exchange. Essential for monitoring imbalance.
- PnL tracking with per-trade loss limit.

**Concerns:**
- **No post-trade risk checks.** Pre-trade checks gate entry, but there's no mechanism for "we're now in a worse position than expected, force-unwind everything." If leg 1 fills at a bad price and leg 2 fills at a worse price, the trade is technically "successful" but unprofitable. The risk manager should track cumulative slippage and escalate.
- **No cross-exchange rebalancing.** If all profitable arb opportunities are "buy on Binance, sell on Kraken" for a period, capital accumulates on Kraken and depletes on Binance. Eventually, the bot can't execute. The spec has no rebalancing mechanism and doesn't even acknowledge this as a risk.
- **Stale data threshold of 1 second is generous.** At the timescales this system operates (100ms–2s opportunity windows), a 1-second-old order book is ancient. By the time you act on it, the opportunity is gone. 200–300ms would be more appropriate for the claimed latency targets.

### 1.6 API Design — Grade: A-

**REST API:**
- Clean resource-oriented routes under `/api/v1/`
- Proper error envelope with structured codes
- Engine control endpoints (start/pause/stop) — essential for operator safety
- Hot-reloadable config via `PUT /api/v1/config`
- Single API key auth with rate limiting

**WebSocket:**
- Channel-based subscription model
- Heartbeat with 30s/10s timeout
- Order book throttling at 100ms — prevents flooding dashboards

**What works:**
- The API is comprehensive for an operator tool. You can monitor, control, and debug everything.
- Versioned endpoints (`/api/v1/`) for future compatibility.
- Paginated and filterable list endpoints — important when you have thousands of trades.

**Minor issues:**
- `POST /api/v1/risk/circuit-breaker` to reset — should require a body with explicit confirmation or reason. Accidentally hitting this endpoint shouldn't silently re-enable trading after a circuit break.
- No audit trail in the API response for who reset the circuit breaker (though the spec mentions audit_logs in the DB).

### 1.7 Technology Choices — Grade: A

| Choice | Verdict |
|--------|---------|
| **Rust** | ✅ Correct for latency-sensitive financial system. No GC pauses, predictable performance, compile-time safety. |
| **tokio** | ✅ The async runtime for Rust. No other realistic option. |
| **axum** | ✅ Best Rust web framework. Tower middleware, extractors, WebSocket support. |
| **sqlx** | ✅ Compile-time query verification is a killer feature for financial data. |
| **rust_decimal** | ✅ Mandatory. Using f64 for money is a disqualifying bug. |
| **UUID v7** | ✅ Time-ordered for index locality. Smart choice for append-heavy workloads. |
| **PostgreSQL** | ✅ ACID compliance for trade records. Correct. |
| **reqwest + tokio-tungstenite** | ✅ Standard HTTP client + WS. No issues. |
| **dashmap** | ⚠️ Concurrent hashmap for order books. Fine, but `parking_lot::RwLock<HashMap>` might be simpler given the access pattern (few writers, many readers). |
| **tracing** | ✅ The structured logging crate for Rust. |

**One notable absence:** No mention of `moka` or `mini-moka` for caching. If the system is querying exchange balances frequently, an in-memory cache with TTL would reduce API calls.

---

## 2. Relationship Between Artifacts

### 2.1 These Are Fundamentally Different Systems

| Dimension | Prediction Market Arb (Artifact 1) | Crypto Exchange Arb (Artifact 2) |
|-----------|-----------------------------------|---------------------------------|
| **Asset type** | Binary outcome contracts (YES/NO) | Continuous-price spot crypto |
| **Profit mechanism** | Resolution: cheapest YES + cheapest NO < 100¢ | Spread: buy low exchange, sell high exchange simultaneously |
| **Holding period** | Days to months (until market resolves) | Seconds (immediate leg execution) |
| **Time pressure** | Low — prediction market prices move slowly | Extreme — sub-second opportunity windows |
| **Latency requirement** | Seconds are fine | Milliseconds matter |
| **Capital lockup** | High — capital locked until resolution | Low — in and out within seconds |
| **Risk profile** | Near-zero if both legs fill (guaranteed profit at resolution) | Leg risk, slippage, partial fills |
| **Competition** | Moderate — niche, fewer participants | Extreme — HFT firms with co-location |
| **Regulatory** | US prediction markets have strict rules (Kalshi is CFTC-regulated) | Crypto exchanges vary by jurisdiction |

### 2.2 Shared Architectural Patterns

Despite being different systems, they share structural DNA:

1. **Multi-venue connector pattern**: Both need to connect to 2+ venues, normalize data formats, and maintain live connections. The `ExchangeConnector` trait from Artifact 2 would work for prediction markets with different method signatures.

2. **Price comparison engine**: Both compare prices across venues. The crypto system compares order books; the prediction market system compares YES/NO contract prices. The logic is structurally similar — "find the cheapest X across all venues."

3. **Simultaneous execution**: Both must execute on two platforms at once. The `tokio::join!()` pattern from Artifact 2 applies directly to Artifact 1.

4. **Risk management**: Both need position tracking, loss limits, and circuit breakers. The risk crate from Artifact 2 could be adapted.

5. **Monitoring dashboard**: Both need real-time visibility. The REST + WebSocket API from Artifact 2 serves both use cases.

### 2.3 Fundamental Divergences

1. **Profit timing**: Crypto arb is immediate — profit realized in seconds. Prediction market arb is deferred — profit locked until the market resolves (could be months). This means:
   - Crypto arb has capital efficiency. Prediction market arb ties up capital.
   - Prediction market arb has certainty. Crypto arb has execution risk.

2. **Market matching**: Artifact 1 requires matching *identical events* across Polymarket and Kalshi (e.g., "Will Bitcoin exceed $100K by July 2026?" on both). This is a fuzzy NLP problem — market titles differ, resolution criteria may differ subtly, and mismatched markets mean total loss. Artifact 2 has no matching problem: BTC/USDT is BTC/USDT everywhere.

3. **Execution model**: Crypto arb needs limit orders, partial fill handling, and unwind logic. Prediction market arb needs simple contract purchases — fill or no fill. The execution layer is simpler for prediction markets.

4. **Fee structure**: Crypto exchanges charge percentage-based trading fees. Prediction markets charge fees on winnings (Polymarket: 0% trading fee but 2% on profits; Kalshi: variable by contract).

### 2.4 Should They Share Infrastructure?

**Verdict: Partial sharing is reasonable, full sharing is overengineering.**

| Component | Shareable? | Notes |
|-----------|-----------|-------|
| `arb-types` (domain types) | ❌ No | Completely different domains. Order books vs. contract prices. |
| Connector trait pattern | ✅ Yes, pattern only | Both need multi-venue connectors, but the trait signatures differ. |
| Risk management framework | ⚠️ Partially | Circuit breaker and loss limits generalize. Position tracking doesn't. |
| DB schema | ❌ No | Different entities (trades vs. contract purchases). |
| API server skeleton | ✅ Yes | Axum + WebSocket + auth is reusable. |
| Monitoring dashboard | ✅ Yes | Generic real-time event display works for both. |
| Execution engine | ❌ No | Fundamentally different execution semantics. |

**Recommendation:** If building both, extract a shared `arb-infra` crate with the API server skeleton, WebSocket broadcaster, circuit breaker, and config loading. Keep everything else separate. Don't force shared abstractions where the domains diverge.

---

## 3. Key Architectural Risks

### 3.1 Latency Claims — UNREALISTIC

**Spec claims: <10ms detection, <300ms execution.**

Let's decompose:

| Component | Realistic Latency | Notes |
|-----------|------------------|-------|
| Exchange WS → rust parser | 1–5ms | Fast in Rust, but depends on message format |
| Network round-trip to exchange (no co-location) | 50–150ms | AWS us-east-1 to Binance: ~80ms. To Kraken: ~120ms. |
| Order placement (API → exchange → ack) | 100–300ms per leg | Best case. Under load, 500ms+. |
| Total detection-to-fill | 200–600ms | Realistic range without co-location |

**<10ms detection** is plausible — it's in-process computation once the order book update arrives in memory. But it's misleading because the bottleneck is not detection, it's the network round-trip for execution.

**<300ms execution** requires:
- Both exchanges respond to REST order placement in <300ms
- No retries, no rate limiting delays
- Low network latency to both exchanges simultaneously

This is achievable as a p50 but not as a p95. The spec claims p95. In practice, p95 execution will be 500–800ms without co-location, by which time most crypto arb opportunities have evaporated.

**The spec explicitly says it's NOT an HFT system requiring co-location** — but then sets latency targets that implicitly require it. This is a contradiction.

### 3.2 Profitability — HIGHLY QUESTIONABLE

**Spec claim: 0.05%–0.5% spreads on BTC/USDT across Binance, Coinbase, Kraken.**

This was realistic in 2020–2022. In 2026:

- **Binance-Coinbase BTC/USDT spread** is typically 0.01%–0.03% during normal conditions. The 0.05%+ spreads occur during volatility events (maybe 5–10 minutes per day).
- **Taker fees** at the lowest retail tier: Binance 0.10%, Coinbase 0.08%, Kraken 0.26%. Round-trip cost for buy on one + sell on another: **0.18%–0.36%**.
- **The fee floor exceeds the typical spread.** At retail fee tiers, this system would lose money on nearly every trade.
- To be profitable, you need **maker fees** (limit orders, not IOC) or **VIP tier fees** from high volume. The spec uses IOC (taker) orders exclusively.

**Break-even analysis:**
- Minimum spread needed: ~0.20% (sum of taker fees on both legs)
- Average available spread in 2026: ~0.02%–0.05% (normal), ~0.10%–0.50% (volatile moments)
- **Result: unprofitable 90%+ of the time at retail fee tiers**

### 3.3 Competition — YOU LOSE

The crypto arb space in 2026 is dominated by:

1. **Jump Crypto, Wintermute, Alameda successors** — co-located servers, sub-millisecond execution, custom kernel-bypass networking (DPDK, io_uring).
2. **Exchange-native market makers** — they get rebates (negative fees) and see order flow first.
3. **Proprietary HFT desks** — dedicated infrastructure, direct exchange connectivity.

Your edge with this system: **None.** You're running on a cloud VM with 100ms+ network latency, paying retail fees, and using REST APIs while competitors use co-located servers with FIX protocol connections. By the time your order reaches the exchange, the opportunity has been captured by someone faster. You are the slow fish.

The spec's non-goal — "NOT a high-frequency trading system requiring co-location" — is an honest acknowledgment, but it also means the system cannot compete in the market it's designed for.

### 3.4 Fee Assumption — INCORRECT

**Spec assumption A4: "Exchange fees are known at startup and treated as constant."**

This is wrong in three ways:

1. **Tiered fees**: All three exchanges adjust fees based on 30-day trailing volume. As the bot trades, its tier changes, and so does the break-even threshold.
2. **Promotional rates**: Exchanges frequently run zero-fee promotions on specific pairs. Not tracking these means missing opportunities or miscalculating costs.
3. **BNB discount**: Binance offers 25% fee discount when paying fees in BNB. The spec doesn't account for this.

**Impact**: Profitability calculations will be systematically wrong. The bot may execute trades it thinks are profitable but aren't, or skip trades that are actually profitable.

### 3.5 Balance Fragmentation — CRITICAL GAP

If arb opportunities are directionally biased (e.g., BTC is consistently cheaper on Binance), the bot will:
- Accumulate BTC on Binance (buying cheap)
- Accumulate USDT on Coinbase/Kraken (selling expensive)
- Eventually run out of USDT on Binance → can't buy → halts

**The spec has no rebalancing mechanism.** Options:
1. **Periodic withdrawal/deposit**: Transfer funds between exchanges. Takes 10–30 minutes for crypto, 1–3 days for fiat. During this time, capital is locked.
2. **Internal netting**: Wait for the bias to reverse. May never happen.
3. **Third-party service**: Use a settlement network. Adds complexity and cost.

This is not a minor issue — it's a fundamental constraint on how long the system can operate.

### 3.6 Partial Fills — DANGEROUS

The spec mentions IOC limit orders and "automatic unwind on partial failure." Let's trace the worst case:

1. Opportunity detected: buy 1 BTC on Binance at $60,000, sell 1 BTC on Coinbase at $60,100.
2. Leg 1 (Binance buy): fills 0.8 BTC.
3. Leg 2 (Coinbase sell): fills 1.0 BTC. Now you've sold 0.2 BTC you don't have — effectively a short position on Coinbase.
4. Unwind: need to buy 0.2 BTC on Coinbase at market price. If price has moved against you, this could wipe out the entire trade's profit and more.

**Wait — selling what you don't have isn't possible in spot markets.** So the real scenario is:
1. Leg 1 fills 0.8 BTC. Leg 2 sells 0.8 BTC (if the order was also for 1 BTC, 0.2 BTC remains unfilled).
2. Actually, the legs are simultaneous via `tokio::join!()` — you don't know leg 1's fill before submitting leg 2.
3. So leg 2 is submitted for 1 BTC but the exchange only has 0.8 BTC available in the account.
4. Leg 2 either fails entirely (insufficient balance) or partially fills to available balance.

**The spec doesn't address the coordination problem**: you can't sell BTC on Coinbase that's still sitting on Binance. This means you need **pre-positioned inventory** on both exchanges. The spec's config has `max_trade_quantity_usd = $5,000` but doesn't discuss how much capital must be pre-positioned per exchange.

### 3.7 Prediction Market Arb — Different (Better?) Risk Profile

The prediction market arb (Artifact 1) has a fundamentally different risk profile:

**Advantages over crypto exchange arb:**
- ✅ **Guaranteed profit if both legs fill**: Buy YES at 52¢ + NO at 47¢ = 99¢ cost, guaranteed 100¢ payout. 1¢ profit per contract regardless of outcome.
- ✅ **No execution speed race**: Prices move slowly (minutes/hours, not milliseconds).
- ✅ **Less competition**: Fewer sophisticated players in prediction markets.
- ✅ **Larger spreads**: Cross-platform prediction market spreads can be 2–5%.
- ✅ **Simpler execution**: No partial fills, no slippage (usually).

**Disadvantages:**
- ❌ **Capital lockup**: Money is locked until market resolution (days to months). Annual return may look mediocre even with high per-trade profit.
- ❌ **Market matching risk**: If "Will BTC hit $100K by July?" on Polymarket has subtly different resolution criteria than Kalshi's version, your "arb" becomes a bet.
- ❌ **Liquidity**: Prediction markets are thin. Can't deploy large capital without moving the market.
- ❌ **Regulatory risk**: Polymarket has had CFTC issues. Kalshi is CFTC-regulated. Operating across both may have legal implications.
- ❌ **Platform risk**: If either platform has solvency issues, your locked capital is at risk.
- ❌ **Fee on winnings**: Polymarket charges ~2% on profits. On a 1¢ spread per contract, that's meaningful.

---

## 4. Profitability Reality Check

### 4.1 Crypto Exchange Arb (Artifact 2)

**Capital requirements:**
- 3 exchanges × $50,000 pre-positioned = $150,000 minimum
- The spec's `max_total_exposure_usd = $50,000` suggests awareness of this, but that's total, not per-exchange

**Revenue model (optimistic):**
- Average profitable spread: 0.10% (after fees)
- Trade frequency: 10 profitable trades/day (aggressive)
- Average trade size: $5,000
- Daily gross profit: 10 × $5,000 × 0.10% = **$50/day**
- Monthly: **~$1,500**

**Revenue model (realistic for 2026):**
- Average profitable spread after fees: 0.02% (most are negative after fees)
- Profitable trades: 2–3/day (most opportunities gone before you can execute)
- Daily gross profit: 3 × $5,000 × 0.02% = **$3/day**
- Monthly: **~$90**

**Costs:**
- Cloud VM (c5.xlarge or equivalent): ~$150/month
- PostgreSQL (RDS): ~$50/month
- Developer time (maintenance): 10h/month × $100/h = $1,000/month

**Verdict: The system costs more to run than it earns.** At retail fee tiers, without co-location, on well-arbitraged pairs like BTC/USDT, this is a money-losing operation. The 200-hour development investment (~$20,000 at $100/h) would take years to recoup under optimistic assumptions and never under realistic ones.

**The only path to profitability:**
1. Trade exotic pairs with wider spreads (but lower liquidity)
2. Achieve VIP fee tiers (requires high existing volume — chicken-and-egg)
3. Add more exchanges (spec's Phase 2: OKX, Bybit) to find more opportunities
4. Co-locate or use faster connectivity
5. Become a market maker (explicitly a non-goal)

### 4.2 Prediction Market Arb (Artifact 1)

**Capital requirements:**
- Polymarket: $10,000 (USDC on Polygon)
- Kalshi: $10,000 (USD)
- Total: $20,000

**Revenue model (optimistic):**
- Cross-platform spread: 3% average (buy YES at 52¢ + NO at 45¢ = 97¢ for guaranteed $1)
- Available markets with genuine cross-platform matches: 20–30 active at any time
- Capital per opportunity: $500 (limited by liquidity)
- Average lockup: 30 days
- Monthly capital deployed: $10,000 across 20 markets
- Monthly gross profit: $10,000 × 3% = **$300/month**
- Annual ROI: ~18% on $20,000 deployed

**Revenue model (realistic):**
- Cross-platform spread after fees: 1.5% (Polymarket 2% winner fee, Kalshi fees)
- Available matching markets: 10–15 (many don't match precisely)
- Capital per opportunity: $200 (thin order books)
- Monthly capital deployed: $3,000
- Monthly gross profit: $3,000 × 1.5% = **$45/month**
- Annual ROI: ~2.7% on $20,000 (worse than a savings account)

**The market matching problem is the killer:**
- Polymarket: "Will Donald Trump win the 2028 presidential election?"
- Kalshi: "Will the Republican nominee win the 2028 presidential election?"
- These are NOT the same market. Trump could lose the primary. Treating them as identical = total loss on one leg.
- Automated matching via NLP is fragile. Manual verification is slow and doesn't scale.

**Costs:**
- Lower than crypto arb (simpler system, less infrastructure)
- Cloud VM: ~$30/month (lightweight)
- Development: ~80 hours ($8,000 at $100/h)

**Verdict: Better than crypto exchange arb, but barely.** The risk-adjusted return is uncompelling. The market matching problem introduces hidden risk that undermines the "guaranteed profit" promise. Capital lockup kills annualized returns. Realistic returns don't justify the development effort.

### 4.3 Capital Efficiency Comparison

| Factor | Crypto Exchange Arb | Prediction Market Arb |
|--------|--------------------|-----------------------|
| Capital required | $150,000+ | $20,000 |
| Capital utilization | High (recycled every trade) | Low (locked for weeks/months) |
| Annualized ROI (optimistic) | ~12% | ~18% |
| Annualized ROI (realistic) | **Negative** | ~3% |
| Risk of total loss | Low (but steady bleed) | Low (if matching is correct) |
| Break-even on dev cost | Never (realistic) / 13 months (optimistic) | 14 months (optimistic) |

---

## 5. Verdict

### On the SPEC.md (Artifact 2) as Architecture

**Architecture grade: B+**

The spec is genuinely well-written. The crate decomposition, trait abstractions, async patterns, risk management, and technology choices are all sound. This is a competent Rust systems design. Specific praise:

- Rust + `rust_decimal` for financial computation: correct and non-negotiable
- Risk management as a separate, fail-fast gating layer: exactly right
- Repository pattern with compile-time-checked SQL: excellent
- Event-driven architecture with appropriate channel topologies: solid
- The spec is thorough — 200 hours of planning for 200 hours of implementation

If the market conditions were right, this architecture would serve well. The engineering is not the problem.

### On Viability

**Crypto exchange arb (Artifact 2): NOT VIABLE as specified.**

The fundamental issue isn't the code — it's the market. In 2026, BTC/USDT cross-exchange arb on major venues is a solved problem dominated by firms with co-located infrastructure, negative-fee market maker agreements, and sub-millisecond execution. A cloud-hosted Rust system with REST API order placement cannot compete. The spec honestly admits it's not an HFT system but targets a market that only HFT can profitably serve.

**Prediction market arb (Artifact 1): MARGINALLY VIABLE.**

The market is less efficient, competition is weaker, and spreads are wider. But capital lockup devastates returns, the market matching problem is genuinely hard to solve correctly, and the total addressable market (number of matching markets × available liquidity) is small. It could work as a side project generating $50–200/month with $20K capital, but it's not a business.

### What Would Make These Viable?

**For crypto arb:**
1. Target **emerging DEX-CEX arbitrage** (Uniswap ↔ Binance) — wider spreads, less competition, MEV-aware
2. Target **less efficient pairs** (altcoin/altcoin) where HFT firms don't bother
3. Add **triangular arbitrage** within a single exchange (no cross-exchange latency)
4. Accept it as an **educational project** (the architecture is genuinely instructive)

**For prediction market arb:**
1. Build **robust market matching** with human-in-the-loop verification
2. Target **multi-platform markets** (add Metaculus, Manifold, PredictIt if legal)
3. Focus on **high-conviction, high-spread markets** rather than automation of everything
4. Accept **manual operation** — the speed advantage of automation is unnecessary here

### Final Assessment

The SPEC.md is an A-grade engineering plan for a D-grade business proposition. The architecture is sound; the market thesis is not. Build it to learn Rust systems programming, not to make money.

---

*End of analysis.*
