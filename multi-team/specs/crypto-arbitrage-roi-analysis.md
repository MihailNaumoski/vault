# ROI Analysis: Crypto Arbitrage Systems

**Date**: 2026-04-05  
**Author**: Spec Writer Agent  
**Audience**: Dutch developer evaluating build vs. don't-build  
**Sources**: `SPEC.md` (crypto exchange arb), `specs/crypto-arbitrage-analysis.md`, `specs/crypto-arbitrage-readiness.md`

---

## TL;DR — Read This Before Anything Else

| Question | Answer |
|----------|--------|
| How long to build crypto exchange arb? | **240–325 hours** (realistic), not the spec's 200h |
| How long to build prediction market arb? | **40–80h** (semi-auto), **150–200h** (full-auto) |
| Will either system ever pay for itself? | **Almost certainly not** at realistic assumptions |
| What's the closest thing to break-even? | Prediction market arb, full-auto, at 2× optimistic revenue: 32 months |
| What's the honest alternative? | Spend those 280 hours freelancing at €100/hr: **€28,000 guaranteed** |
| Should you build it? | For learning: yes. For profit: no. |

> **The core problem**: Crypto exchange arbitrage on BTC/USDT is dominated by HFT firms with co-located servers and sub-millisecond execution. At retail fee tiers (0.10%–0.26% taker), the fee floor *exceeds* the typical available spread. You will lose money on most trades, before infrastructure costs. Prediction market arbitrage is better, but capital lockup, thin liquidity, and maintenance overhead make even the optimistic scenario barely break-even.

---

## 1. Development Time Investment

### 1.1 Strategy 1 — Crypto Exchange Arbitrage (Binance / Coinbase / Kraken)

This is the 7-crate Rust system: `arb-types`, `arb-exchange`, `arb-engine`, `arb-risk`, `arb-db`, `arb-server`, `arb-cli`.

| Phase | SPEC Estimate | Realistic Estimate¹ | Reason for Gap |
|-------|--------------|---------------------|----------------|
| Phase 1: Foundation (types, DB, config, risk skeleton) | 34h | 40–50h | SQL DDL must be written from scratch; `arb-types` has more edge cases than estimated; SQLx compile-time verification is slow in CI |
| Phase 2: Core Engine (detection, execution, channel topology) | 37h | 50–70h | VWAP algorithm has unspecified edge cases; unwind logic is genuinely hard; partial fill reconciliation adds ~15h not in spec; async Rust borrow-checker friction |
| Phase 3: Exchange Connectors (Binance, Coinbase, Kraken REST + WS) | 44h | 70–100h | **This is where estimates die.** Each exchange has undocumented quirks, silent API changes, rate-limit edge cases, auth signature traps, and production debugging that takes 2× estimated time |
| Phase 4: API Server (17 endpoints, WebSocket) | 33h | 30–45h | Axum is well-documented; closest phase to spec estimate; WebSocket broadcaster is the complexity spike |
| Phase 5: Testing (unit, integration, load) | 38h | 50–60h | Good tests take longer than the code; "100% line coverage on arb-types" is a vanity metric; chaos tests not in scope |
| **TOTAL** | **186h ≈ 200h** | **240–325h (midpoint: ~280h)** | |

¹ *Source: readiness assessment §4.1, based on realistic exchange API connector complexity*

**True MVP** (2 exchanges + SQLite + 4 REST endpoints + basic detection): **~88 hours**  
*Source: readiness assessment §4.2 — proves detection and execution work before building full infrastructure*

---

### 1.2 Strategy 2 — Prediction Market Arbitrage (Polymarket ↔ Kalshi)

This system matches equivalent binary-outcome markets across two platforms and captures the guaranteed spread.

#### Sub-variant A: Semi-Automated (alert + manual execution)

| Component | Hours |
|-----------|-------|
| API connectors (Polymarket REST + Kalshi REST, auth, data normalization) | 15–25h |
| Market matching engine (fuzzy title match + manual verification CLI) | 10–20h |
| Spread calculator (YES/NO price pairs, fee-adjusted profit) | 5–10h |
| Alert system (Telegram/webhook on spread > threshold) | 5–10h |
| Testing (mock connectors, synthetic market data) | 5–15h |
| **TOTAL** | **40–80h (midpoint: 60h)** |

#### Sub-variant B: Full-Auto (end-to-end automated execution)

| Component | Hours |
|-----------|-------|
| Everything in Semi-Auto | 40–80h |
| Order execution (limit orders on both platforms, EIP-712 + RSA signing) | 30–40h |
| Order monitoring loop (fill tracking, cancel stale, repost) | 20–30h |
| Settlement monitoring (track resolution, confirm payout) | 15–25h |
| Risk management (circuit breaker, max exposure, unhedged position limits) | 15–20h |
| Additional integration testing | 25–35h |
| **TOTAL** | **150–200h (midpoint: 175h)** |

---

## 2. Opportunity Cost Calculator

Before building anything, ask: **what would I earn if I spent these hours freelancing?**

| Variant | Hours | @ €50/hr | @ €100/hr |
|---------|-------|----------|-----------|
| Crypto Arb (SPEC estimate) | 200h | **€10,000** | **€20,000** |
| Crypto Arb (Realistic) | 280h (midpoint) | **€14,000** | **€28,000** |
| Crypto Arb (True MVP) | 88h | **€4,400** | **€8,800** |
| Prediction Mkt (Semi-Auto) | 60h (midpoint) | **€3,000** | **€6,000** |
| Prediction Mkt (Full-Auto) | 175h (midpoint) | **€8,750** | **€17,500** |

> This is your **guaranteed alternative**. Every row above represents income you could have instead. The systems must earn this back *plus* cover ongoing costs before the investment pays off.

---

## 3. Monthly Profit & Loss

### 3.1 Strategy 1 — Crypto Exchange Arb

**Revenue assumptions** *(source: analysis §4.1)*:
- **Pessimistic**: €75/month — 2–3 profitable trades/day, 0.02% spread after fees
- **Realistic**: €150/month — midpoint between pessimistic and optimistic
- **Optimistic**: €300/month — 10 profitable trades/day, 0.10% spread after fees, €5,000 trade size
- **Super optimistic (2×)**: €600/month — assumes spreads double (unlikely in 2026)

Note: at retail taker fees (0.10% Binance + 0.26% Kraken = 0.36% round-trip), the **fee floor exceeds the typical 0.02%–0.05% spread**. Most trades will be *negative* after fees. The "realistic" revenue above is already highly optimistic.

**Monthly costs** *(source: analysis §4.1)*:
- Cloud VM (c5.xlarge or equivalent): **€140/month**
- PostgreSQL (managed): **€45/month**
- Monitoring/logging: **€15/month**
- **Total infrastructure: €200/month**
- Maintenance dev time: 10h/month × €50–100/hr: **€500–1,000/month**
- **Total monthly burn: €700–1,200/month**

**Capital required: €150,000** (€50,000 pre-positioned on each of 3 exchanges)

| Scenario | Monthly Revenue | Monthly Costs | **Monthly Net** |
|----------|----------------|---------------|-----------------|
| Pessimistic | €75 | €1,200 | **-€1,125** |
| Realistic | €150 | €950 | **-€800** |
| Optimistic | €300 | €700 | **-€400** |
| Super optimistic (2×) | €600 | €700 | **-€100** |

**Even at 2× optimistic revenue, this system is cash-flow negative every single month.**

---

### 3.2 Strategy 2 — Prediction Market Arb

**Revenue assumptions** *(source: analysis §4.2)*:
- **Pessimistic**: €50/month — thin liquidity, few matching markets found
- **Realistic**: €100/month — 10–15 matching markets, €200 per position, 1.5% spread after Polymarket's 2% winner fee and Kalshi's contract fees
- **Optimistic**: €250/month — 20 matching markets, €500 per position, 3% avg spread

Note: capital is **locked until market resolution** (days to months). Annual turnover is far lower than it appears. The "guaranteed profit" promise only holds if markets are truly equivalent — a fuzzy NLP match that is *wrong* means a total loss on one leg.

**Monthly costs**:
- Lightweight VM (t3.micro or equivalent): **€30/month**
- Maintenance: 4h/month × €50–100/hr: **€200–400/month** (API changes, market matching updates, Polymarket smart contract updates)
- **Total monthly burn: €230–430/month**

**Capital required: €20,000** (€10,000 per platform, locked for weeks/months)

| Scenario | Monthly Revenue | Monthly Costs | **Monthly Net** |
|----------|----------------|---------------|-----------------|
| Pessimistic | €50 | €430 | **-€380** |
| Realistic | €100 | €330 | **-€230** |
| Optimistic | €250 | €230 | **+€20** |
| Super optimistic (2×) | €500 | €230 | **+€270** |

---

## 4. P&L Projections

**Assumptions for all tables:**
- Crypto Arb: dev happens months 1–4 (280h total ÷ 4 = 70h/month); revenue starts month 4; infra €200/month from month 1; maintenance €750/month (midpoint) from month 4
- Prediction Mkt: dev happens month 1 only (60h); revenue starts month 2; infra €30/month from month 1; maintenance €300/month (midpoint) from month 2
- Net P&L = Cumulative Revenue − Cumulative Dev − Cumulative Infra − Cumulative Maintenance

---

### 4.1 Crypto Exchange Arb — 12 Months

| Mo | Cum Dev (€50/hr) | Cum Dev (€100/hr) | Cum Infra | Cum Revenue | Cum Maint | Net P&L @ €50/hr | Net P&L @ €100/hr |
|----|-----------------|------------------|-----------|-------------|-----------|-------------------|-------------------|
| 1  | €3,500          | €7,000           | €200      | €0          | €0        | **-€3,700**       | **-€7,200**       |
| 2  | €7,000          | €14,000          | €400      | €0          | €0        | **-€7,400**       | **-€14,400**      |
| 3  | €10,500         | €21,000          | €600      | €0          | €0        | **-€11,100**      | **-€21,600**      |
| 4  | €14,000         | €28,000          | €800      | €150        | €750      | **-€15,400**      | **-€29,400**      |
| 5  | €14,000         | €28,000          | €1,000    | €300        | €1,500    | **-€16,200**      | **-€30,200**      |
| 6  | €14,000         | €28,000          | €1,200    | €450        | €2,250    | **-€17,000**      | **-€31,000**      |
| 7  | €14,000         | €28,000          | €1,400    | €600        | €3,000    | **-€17,800**      | **-€31,800**      |
| 8  | €14,000         | €28,000          | €1,600    | €750        | €3,750    | **-€18,600**      | **-€32,600**      |
| 9  | €14,000         | €28,000          | €1,800    | €900        | €4,500    | **-€19,400**      | **-€33,400**      |
| 10 | €14,000         | €28,000          | €2,000    | €1,050      | €5,250    | **-€20,200**      | **-€34,200**      |
| 11 | €14,000         | €28,000          | €2,200    | €1,200      | €6,000    | **-€21,000**      | **-€35,000**      |
| 12 | €14,000         | €28,000          | €2,400    | €1,350      | €6,750    | **-€21,800**      | **-€35,800**      |

*After month 4: system bleeds €800/month regardless of rate (€150 revenue − €200 infra − €750 maintenance = −€800). The hole only gets deeper.*

---

### 4.2 Crypto Exchange Arb — 24 Months

| Mo | Cum Dev (€50/hr) | Cum Dev (€100/hr) | Cum Infra | Cum Revenue | Cum Maint | Net P&L @ €50/hr | Net P&L @ €100/hr |
|----|-----------------|------------------|-----------|-------------|-----------|-------------------|-------------------|
| 12 | €14,000         | €28,000          | €2,400    | €1,350      | €6,750    | **-€21,800**      | **-€35,800**      |
| 13 | €14,000         | €28,000          | €2,600    | €1,500      | €7,500    | **-€22,600**      | **-€36,600**      |
| 14 | €14,000         | €28,000          | €2,800    | €1,650      | €8,250    | **-€23,400**      | **-€37,400**      |
| 15 | €14,000         | €28,000          | €3,000    | €1,800      | €9,000    | **-€24,200**      | **-€38,200**      |
| 16 | €14,000         | €28,000          | €3,200    | €1,950      | €9,750    | **-€25,000**      | **-€39,000**      |
| 17 | €14,000         | €28,000          | €3,400    | €2,100      | €10,500   | **-€25,800**      | **-€39,800**      |
| 18 | €14,000         | €28,000          | €3,600    | €2,250      | €11,250   | **-€26,600**      | **-€40,600**      |
| 19 | €14,000         | €28,000          | €3,800    | €2,400      | €12,000   | **-€27,400**      | **-€41,400**      |
| 20 | €14,000         | €28,000          | €4,000    | €2,550      | €12,750   | **-€28,200**      | **-€42,200**      |
| 21 | €14,000         | €28,000          | €4,200    | €2,700      | €13,500   | **-€29,000**      | **-€43,000**      |
| 22 | €14,000         | €28,000          | €4,400    | €2,850      | €14,250   | **-€29,800**      | **-€43,800**      |
| 23 | €14,000         | €28,000          | €4,600    | €3,000      | €15,000   | **-€30,600**      | **-€44,600**      |
| 24 | €14,000         | €28,000          | €4,800    | €3,150      | €15,750   | **-€31,400**      | **-€45,400**      |

*At month 24, you've spent €31,400–€45,400 and earned €3,150 in trading revenue. The gap widens forever.*

---

### 4.3 Prediction Market Arb (Semi-Auto) — 12 Months

| Mo | Cum Dev (€50/hr) | Cum Dev (€100/hr) | Cum Infra | Cum Revenue | Cum Maint | Net P&L @ €50/hr | Net P&L @ €100/hr |
|----|-----------------|------------------|-----------|-------------|-----------|-------------------|-------------------|
| 1  | €3,000          | €6,000           | €30       | €0          | €0        | **-€3,030**       | **-€6,030**       |
| 2  | €3,000          | €6,000           | €60       | €100        | €300      | **-€3,260**       | **-€6,260**       |
| 3  | €3,000          | €6,000           | €90       | €200        | €600      | **-€3,490**       | **-€6,490**       |
| 4  | €3,000          | €6,000           | €120      | €300        | €900      | **-€3,720**       | **-€6,720**       |
| 5  | €3,000          | €6,000           | €150      | €400        | €1,200    | **-€3,950**       | **-€6,950**       |
| 6  | €3,000          | €6,000           | €180      | €500        | €1,500    | **-€4,180**       | **-€7,180**       |
| 7  | €3,000          | €6,000           | €210      | €600        | €1,800    | **-€4,410**       | **-€7,410**       |
| 8  | €3,000          | €6,000           | €240      | €700        | €2,100    | **-€4,640**       | **-€7,640**       |
| 9  | €3,000          | €6,000           | €270      | €800        | €2,400    | **-€4,870**       | **-€7,870**       |
| 10 | €3,000          | €6,000           | €300      | €900        | €2,700    | **-€5,100**       | **-€8,100**       |
| 11 | €3,000          | €6,000           | €330      | €1,000      | €3,000    | **-€5,330**       | **-€8,330**       |
| 12 | €3,000          | €6,000           | €360      | €1,100      | €3,300    | **-€5,560**       | **-€8,560**       |

*After dev: system bleeds €230/month (€100 revenue − €30 infra − €300 maintenance). Cheaper than crypto arb, but still red.*

---

### 4.4 Prediction Market Arb (Semi-Auto) — 24 Months

| Mo | Cum Dev (€50/hr) | Cum Dev (€100/hr) | Cum Infra | Cum Revenue | Cum Maint | Net P&L @ €50/hr | Net P&L @ €100/hr |
|----|-----------------|------------------|-----------|-------------|-----------|-------------------|-------------------|
| 12 | €3,000          | €6,000           | €360      | €1,100      | €3,300    | **-€5,560**       | **-€8,560**       |
| 13 | €3,000          | €6,000           | €390      | €1,200      | €3,600    | **-€5,790**       | **-€8,790**       |
| 14 | €3,000          | €6,000           | €420      | €1,300      | €3,900    | **-€6,020**       | **-€9,020**       |
| 15 | €3,000          | €6,000           | €450      | €1,400      | €4,200    | **-€6,250**       | **-€9,250**       |
| 16 | €3,000          | €6,000           | €480      | €1,500      | €4,500    | **-€6,480**       | **-€9,480**       |
| 17 | €3,000          | €6,000           | €510      | €1,600      | €4,800    | **-€6,710**       | **-€9,710**       |
| 18 | €3,000          | €6,000           | €540      | €1,700      | €5,100    | **-€6,940**       | **-€9,940**       |
| 19 | €3,000          | €6,000           | €570      | €1,800      | €5,400    | **-€7,170**       | **-€10,170**      |
| 20 | €3,000          | €6,000           | €600      | €1,900      | €5,700    | **-€7,400**       | **-€10,400**      |
| 21 | €3,000          | €6,000           | €630      | €2,000      | €6,000    | **-€7,630**       | **-€10,630**      |
| 22 | €3,000          | €6,000           | €660      | €2,100      | €6,300    | **-€7,860**       | **-€10,860**      |
| 23 | €3,000          | €6,000           | €690      | €2,200      | €6,600    | **-€8,090**       | **-€11,090**      |
| 24 | €3,000          | €6,000           | €720      | €2,300      | €6,900    | **-€8,320**       | **-€11,320**      |

*€8,320–€11,320 spent over 2 years, €2,300 earned in trading revenue. You'd have been better off in a savings account.*

---

## 5. Break-Even Analysis

"Break-even" = months until cumulative trading profit covers total cumulative cost.

| Strategy | Dev Cost (€50/hr) | Dev Cost (€100/hr) | Monthly Net (realistic) | Break-Even @ €50/hr | Break-Even @ €100/hr |
|----------|------------------|------------------|------------------------|---------------------|----------------------|
| Crypto Arb (full, 280h) | €14,000 | €28,000 | **-€800/month** | **NEVER** | **NEVER** |
| Crypto Arb (MVP, 88h) | €4,400 | €8,800 | **-€800/month** | **NEVER** | **NEVER** |
| Prediction Mkt (semi, realistic) | €3,000 | €6,000 | **-€230/month** | **NEVER** | **NEVER** |
| Prediction Mkt (semi, optimistic) | €3,000 | €6,000 | +€20/month | **150 months (12.5 yrs)** | **300 months (25 yrs)** |
| Prediction Mkt (full, 2× optimistic) | €8,750 | €17,500 | +€270/month | **32 months (2.7 yrs)** | **65 months (5.4 yrs)** |

**Key observations:**

1. **Crypto arb is cash-flow negative in every scenario.** Even under 2× optimistic revenue (€600/month), the system loses €100/month after infrastructure. Break-even is mathematically impossible — you'd need ~3× the realistic revenue estimate just to cover costs, with zero return on development investment.

2. **The MVP doesn't help.** Cutting dev time from 280h to 88h saves on dev cost, but the *operating* losses are the problem. A cheaper system to build is still a money-losing system to run.

3. **Prediction market arb has a sliver of hope.** The full-auto variant, in a 2× optimistic scenario (which requires roughly 20 matching markets × €500 per position × 3% spread), breaks even in 32–65 months. That's 2.7–5.4 years of hope hanging on a scenario that may not materialise.

4. **The optimistic scenarios are genuinely optimistic.** The 2× optimistic figure assumes you can find 20+ well-matched markets with €500 of available liquidity each, consistently, for years. In practice, matching markets are sparse and liquidity is thin (€50–200 per market is more realistic).

---

## 6. Hidden Costs & Risk Factors

The P&L tables above **exclude** these costs entirely. They make the picture worse.

### 6.1 Capital Opportunity Cost

You're locking capital at zero (or worse) return:

| Capital | Risk-Free Rate | Annual Opportunity Cost | Monthly |
|---------|---------------|------------------------|---------|
| €150,000 (crypto arb) | 4% (Dutch savings/bonds) | **€6,000/year** | **€500/month** |
| €20,000 (prediction mkt) | 4% | **€800/year** | **€67/month** |

For crypto arb: add €500/month to the already-negative figures. Monthly net becomes -€1,300 realistic.

### 6.2 Exchange Counterparty Risk

- **Crypto exchanges**: one exchange hack or insolvency = potential total loss of €50,000 pre-positioned capital. This has happened (FTX 2022, Celsius, etc.)
- **Prediction markets**: Polymarket is unregulated (US persons blocked); Kalshi is CFTC-regulated but small
- **Probability of a platform event in 3 years**: not negligible — estimate at least 5–10%
- **Expected loss**: €150,000 × 7.5% probability = €11,250 expected value drag (crypto arb)

### 6.3 API Breakage Cost

Exchanges change APIs without notice or with minimal deprecation warning:
- Estimate: 2–3 breaking changes per year across 3 exchanges
- Emergency fix time: 10–20h per incident
- Cost at €75/hr: €750–€1,500 per incident × 2–3/year = **€1,500–€4,500/year**
- This is not in the maintenance budget — it's unplanned downtime with real capital at risk

### 6.4 Regulatory Risk

- **Dutch MiCA regulation** (EU Markets in Crypto-Assets): fully in effect, compliance requirements evolving
- **Polymarket geo-restriction**: already blocked for US persons; Dutch access may change
- **Tax**: Dutch *box 3* asset taxation applies to crypto holdings; each trade leg may be a taxable event; accounting costs €500–€2,000/year for a professional
- **Kalshi**: CFTC-regulated; verify Dutch residency is permissible for cross-border prediction market trading

### 6.5 Psychological Cost

This is real and not priced into any table:
- A system handling €150,000 in real money is **not a side project** — it demands attention
- Circuit breaker trips at 3am require a response
- Unexplained P&L drifts require investigation
- Maintenance windows on exchanges require availability
- Estimated: 5–10 hours/month of **cognitive overhead** beyond the 10h maintenance estimate, at zero additional hourly rate (it's anxiety, not productive work)

---

## 7. The Verdict

| Factor | Crypto Arb | Prediction Mkt (Semi-Auto) | Prediction Mkt (Full-Auto) |
|--------|-----------|--------------------------|--------------------------|
| **Dev hours** | 240–325h | 40–80h | 150–200h |
| **Capital needed** | €150,000 | €20,000 | €20,000 |
| **Monthly profit (realistic)** | **-€800** | **-€230** | **-€230** |
| **Monthly profit (2× optimistic)** | **-€100** | **+€20** | **+€270** |
| **Break-even** | **NEVER** | NEVER (realistic) | 32 months (2× optimistic only) |
| **Risk of total loss** | Low (steady bleed) | Medium (market mismatch = total loss on leg) | Medium |
| **Competition** | Extreme (HFT firms, co-located, sub-ms) | Moderate (retail-dominated) | Moderate |
| **Capital locked** | No (seconds per trade) | Yes (days to months per market) | Yes (days to months per market) |
| **Learning value** | **HIGH** (Rust, async, financial systems, WebSocket) | Medium | Medium |
| **Recommendation** | ❌ **Don't build for profit** | ⚠️ Only as a learning project | ⚠️ Only if 2× scenario is credible |

**Crypto arb summary**: An A-grade engineering plan for a D-grade business proposition. The architecture is genuinely well-designed. The market is not. In 2026, BTC/USDT cross-exchange arbitrage on major venues is dominated by firms with co-located servers, negative-fee market maker agreements, and FIX protocol connectivity. A cloud VM paying retail taker fees cannot compete. The system will lose money on most trades before infrastructure costs.

**Prediction market arb summary**: Better odds, but the math is still unfavourable at realistic assumptions. The "guaranteed profit" framing is true only if your market matching is correct — a wrong match causes a total loss on one leg. The capital lockup problem means your €20,000 is earning 6% annualised (optimistic) before costs, and negative after costs.

---

## 8. What Would Make This Profitable?

If you want to build *something* in this space that actually earns, here are alternatives with better unit economics:

### 8.1 DEX-CEX Arbitrage (Better Spreads, Less Competition)

Buy on Uniswap (or another DEX), sell on Binance (or vice versa). Spreads are wider because DEX prices lag CEX prices by block time (12 seconds on Ethereum). Competition exists (MEV bots) but is less extreme than pure CEX-CEX. Key requirement: understand MEV and gas costs.

### 8.2 Altcoin Pairs (Inefficient Markets)

Major HFT firms don't bother with SHIB/USDT or obscure altcoin pairs — the volume doesn't justify co-location costs. Spreads on thinly traded pairs can be 0.5%–2%. The catch: liquidity is thin (can't deploy large capital), and the risk of a coin rug-pull or exchange delisting is non-trivial.

### 8.3 Triangular Arbitrage (No Cross-Exchange Latency)

Find price inconsistencies within a single exchange: BTC/USDT → ETH/BTC → ETH/USDT → back to USDT. No withdrawal/rebalancing issues, no cross-exchange latency, one fee schedule. Narrower spreads, but execution is entirely within one API.

### 8.4 Market Making (Different Strategy Entirely)

Instead of arbitrage (waiting for someone else to misprice), provide liquidity as a market maker. Post limit orders on both sides of the spread, earn the bid-ask spread passively. More complex risk management (inventory management, adverse selection), but a fundamentally different and more sustainable edge. Kraken and Coinbase offer rebates for market makers.

### 8.5 The Honest Alternative

> **280 hours × €100/hr = €28,000 — guaranteed, taxed as income, zero capital at risk.**

This is not a dismissal of the learning value. Building the crypto arb system is genuinely excellent technical education: production Rust, async architecture, financial data pipelines, API design. **Build it for the learning. Do not build it expecting it to pay for itself.** If you want to run it with real money, start with paper trading (fake money, real data) for at least 3 months before touching live capital.

---

*Sources: `SPEC.md` (prediction market arb, Phase breakdown), `specs/crypto-arbitrage-analysis.md` (profitability analysis §4.1–4.2), `specs/crypto-arbitrage-readiness.md` (time estimate reality check §4.1–4.2). All costs in EUR. Exchange rates, fee schedules, and market conditions as of 2026-04-05.*
