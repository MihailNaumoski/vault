# Profitable Trading Strategy Architecture Analysis

**Date**: 2026-04-05
**Author**: Architect Agent
**Status**: Complete
**Context**: Solo Dutch developer, Rust preferred, €10K-50K capital
**Previous Work**: CEX-CEX arb (loses €800/mo), prediction market arb (marginally viable at best)

---

## Executive Summary

After analyzing 14 automated trading strategies against the constraints of a solo Rust developer with €10K-50K capital, the honest assessment is:

| Tier | Strategies | Why |
|------|-----------|-----|
| **Tier 1: Actually Viable** | Funding Rate Arb, Long-Tail Altcoin Arb, Solana DEX-CEX Arb | Structural edges exist for retail; spreads exceed fees; capital sufficient |
| **Tier 2: Conditionally Viable** | Market Making on Illiquid DEXs, Stablecoin Depeg Arb, Yield Farming Optimization | Work under specific conditions; require expertise or patience |
| **Tier 3: Technically Possible but Brutal** | Ethereum DEX-CEX Arb, Liquidation Bots, MEV Backrunning, Atomic Flash Loan Arb | Competition is extreme; solo dev can win occasionally but not consistently |
| **Tier 4: Avoid** | Cross-Chain Bridge Arb, NFT Arb, Memecoin Sniping, Options Arb | Risk/reward doesn't justify development; dominated or dying markets |

**The single most promising strategy is Funding Rate Arbitrage** — it's delta-neutral, competition is low, capital requirements fit the budget, and it's the ONLY strategy where being slower doesn't automatically mean losing. Second choice is Long-Tail Altcoin Pair Arbitrage on less efficient venues.

---

## Strategy Evaluation Framework

Each strategy is evaluated on these axes:

- **Spread vs. Fees**: Does the profit margin exceed ALL costs (trading fees, gas, infrastructure)?
- **Speed Sensitivity**: Does being 100ms slower mean you lose? (If yes, institutions win.)
- **Capital Efficiency**: Can €10K-50K generate meaningful returns?
- **Competition Density**: How many sophisticated actors are in this exact niche?
- **Solo Dev Feasibility**: Can one person build, deploy, monitor, and maintain this?
- **Risk Profile**: What's the worst realistic loss scenario?

---

## Strategy 1: DEX-CEX Arbitrage (Uniswap/Raydium ↔ Binance)

### Mechanism

DEX prices lag CEX prices because DEX prices only update when someone executes a trade on-chain. On Ethereum, blocks are produced every ~12 seconds. On Solana, slots are ~400ms. When BTC moves 0.5% on Binance in 3 seconds, Uniswap's ETH/USDC pool still reflects the old price for up to 12 seconds.

The arbitrageur:
1. Monitors CEX price feeds in real-time (WebSocket, sub-100ms)
2. Detects when DEX pool price diverges from CEX price by more than gas + fees
3. Executes a swap on the DEX (buy cheap token) and simultaneously sells on the CEX (sell expensive token)
4. OR: uses a flash loan to execute the DEX leg with zero upfront capital

**Key difference from CEX-CEX arb**: The spread is structural (block-time latency) rather than transient (order book microstructure). Spreads of 0.1%-0.5% are common, vs. 0.01%-0.03% on CEX-CEX.

### Architecture

```
┌─────────────────────────────────────────────┐
│ CEX Price Feed (Binance WebSocket)          │
│ ← sub-100ms price updates                  │
└──────────────┬──────────────────────────────┘
               │
       ┌───────▼────────┐
       │ Price Divergence│
       │ Detector        │
       │ (CEX vs DEX)    │
       └───────┬────────┘
               │ divergence > threshold
       ┌───────▼────────┐
       │ Execution       │
       │ Engine           │
       │ ┌──────────────┐│
       │ │DEX: Submit TX ││ ← sign + broadcast to mempool/Jito
       │ │CEX: IOC order ││ ← REST API limit order
       │ └──────────────┘│
       └───────┬────────┘
               │
       ┌───────▼────────┐
       │ Position Tracker│
       │ + Rebalancer    │
       └────────────────┘
```

**Latency requirements**:
- Ethereum: ~12s block time gives a wider window, but mempool is PUBLIC — other bots see your TX
- Solana: ~400ms slots, but transactions are processed in leader order (no public mempool on mainnet; Jito tips for priority)
- CEX leg: 50-150ms for order placement

**Critical components**:
- Mempool monitoring (Ethereum) or transaction submission (Solana)
- Gas price estimation and dynamic gas bidding
- DEX swap routing (finding the best pool/path)
- Pre-signed transactions ready to broadcast
- Inventory management across DEX wallet and CEX account

### Tech Stack

| Component | Crate/Tool | Notes |
|-----------|-----------|-------|
| Ethereum interaction | `alloy` (v1.x) | Successor to ethers-rs; type-safe contract calls, transaction building |
| Solana interaction | `solana-sdk` + `solana-client` | Transaction construction, RPC calls |
| Solana MEV | `jito-sdk-rust` | Bundle submission for priority execution |
| Ethereum MEV | `flashbots` crate or raw `alloy` bundle submission | Flashbots Protect / MEV-Share |
| DEX routing | `uniswap-rs` or custom | Uniswap v3/v4 quoter contract calls |
| CEX connector | `reqwest` + `tokio-tungstenite` | Binance WebSocket + REST |
| Math | `rust_decimal`, `ethers-core::types::U256` | On-chain math is uint256 |
| Async runtime | `tokio` | Multi-threaded for parallel execution |
| Key management | `alloy-signer-local` or `solana-sdk::signer` | Local wallet signing |

### Competition Analysis

**Ethereum DEX-CEX**:
- **Dominated by MEV searchers**. Wintermute, Jump, and hundreds of dedicated searcher bots monitor Uniswap pools.
- Public mempool means your transaction is visible. Competing bots can frontrun your arb with higher gas.
- Flashbots bundles bypass the public mempool but require winning the Flashbots auction (highest bid wins).
- **As of 2026**: ~60% of Ethereum blocks are built by MEV-Boost relays. The MEV extraction market is mature.
- **You are competing against**: searcher bots with custom EVM execution environments, private transaction pools, and dedicated infrastructure.

**Solana DEX-CEX**:
- **Less mature MEV market**. Jito (Solana's dominant MEV protocol) has ~50-70% validator adoption.
- No public mempool on Solana mainnet — transactions go directly to the current leader.
- Jito tips provide priority but the auction is less sophisticated than Flashbots.
- **Competition is growing but not yet institutional-grade** for all pairs.
- **You are competing against**: Jito searcher bots, but the field is narrower than Ethereum.

### Solo Dev Edge

**Ethereum**: ❌ **No realistic edge.** The MEV searcher ecosystem is professionalized. You're competing against teams with years of experience, custom infrastructure, and private order flow. Even if you find an opportunity, your Flashbots bundle bid will lose to searchers who can bid higher because they have lower costs (amortized infrastructure).

**Solana**: ⚠️ **Marginal edge on less popular pairs.** The Solana MEV market is less efficient than Ethereum. For long-tail Raydium pools (small-cap tokens), competition is lower. A well-optimized Rust bot submitting Jito bundles for small-cap token arb against Binance could find 2-5 profitable trades/day.

**Honest assessment**: On Ethereum, you'll spend 200 hours building something that earns $0 because every opportunity is captured by faster, better-funded searchers. On Solana, there's a narrow window of viability for less popular pairs, but it's closing as the ecosystem matures.

### Difficulty Rating: 4/5

- Smart contract interaction + transaction signing + gas optimization + mempool monitoring
- Two completely different chains (EVM vs. SVM) if you want both
- Flash loan integration adds significant complexity
- Failure modes are expensive (gas spent on failed transactions, reverted swaps)

### Key Risks

1. **Gas costs on failed transactions**: On Ethereum, you pay gas even if your arb TX is frontrun and fails. At 30 gwei and 200K gas, that's ~$15 per failed attempt.
2. **Sandwich attacks**: Your DEX swap can be sandwiched by other MEV bots, turning your arb into their profit.
3. **Smart contract risk**: DEX pools can have bugs, reentrancy, or governance attacks.
4. **Inventory risk**: Capital on the CEX side and wallet side can become imbalanced. Rebalancing between on-chain and CEX requires withdrawals (30 min - 2 hours).
5. **RPC reliability**: Your Ethereum/Solana RPC node's latency and uptime directly affect profitability.
6. **Regulatory**: EU MiCA doesn't explicitly cover MEV extraction; grey area.

### Preliminary Viability

| Aspect | Ethereum | Solana |
|--------|----------|--------|
| Viability | ❌ Not viable for retail | ⚠️ Conditionally viable |
| Expected monthly return | -€200 to €0 (gas losses) | €100-500 (long-tail pairs) |
| Capital needed | €20K+ (gas + CEX position) | €10-20K |
| Competition | Extreme | Moderate-High |
| Dev time | 150-200h | 120-160h |

**Verdict**: Skip Ethereum entirely. Solana DEX-CEX arb on long-tail Raydium pairs has a narrow viability window. **Tier 2-3** depending on pair selection.

---

## Strategy 2: DeFi Liquidation Bots (Aave/Compound/MakerDAO)

### Mechanism

Lending protocols (Aave, Compound, Maker, and newer protocols on L2s) allow users to borrow assets against collateral. Each position has a **health factor** = (collateral value × liquidation threshold) / borrowed value. When health factor drops below 1.0, the position becomes liquidatable.

A liquidator:
1. Monitors all borrowing positions for declining health factors
2. When a position becomes liquidatable, calls the `liquidationCall()` function on the lending protocol
3. Repays a portion of the borrower's debt (up to 50% on Aave v3)
4. Receives the borrower's collateral at a **liquidation bonus** (typically 5-10% discount)
5. Sells the received collateral on a DEX or CEX for profit

**Flash loan optimization**: You don't need capital upfront. Use a flash loan from the same protocol (or another):
1. Flash borrow USDC
2. Repay the unhealthy position's debt with USDC
3. Receive ETH collateral at 5% discount
4. Swap ETH → USDC on a DEX
5. Repay flash loan + fee (0.05-0.09%)
6. Keep the profit (~4-5% of liquidated amount)

### Architecture

```
┌────────────────────────────────────────────┐
│ On-Chain State Monitor                      │
│ • Poll all borrowing positions every block  │
│ • Or: subscribe to price oracle updates     │
│ • Calculate health factors in real-time     │
└───────────────┬────────────────────────────┘
                │ health_factor < 1.0
        ┌───────▼──────────┐
        │ Profitability     │
        │ Calculator        │
        │ • liquidation bonus│
        │ • gas cost        │
        │ • flash loan fee  │
        │ • DEX swap cost   │
        └───────┬──────────┘
                │ profitable
        ┌───────▼──────────┐
        │ TX Builder        │
        │ • Flash loan      │
        │ • Liquidate       │
        │ • Swap collateral │
        │ • Repay flash loan│
        │ All in ONE atomic │
        │ transaction       │
        └───────┬──────────┘
                │
        ┌───────▼──────────┐
        │ Flashbots/Jito    │
        │ Bundle Submission │
        └──────────────────┘
```

**Key insight**: The entire operation is atomic — either everything succeeds or nothing happens (no capital at risk in the transaction itself).

### Tech Stack

| Component | Crate/Tool | Notes |
|-----------|-----------|-------|
| Position monitoring | `alloy` + custom RPC polling | Multicall contract to batch-read positions |
| Flash loan integration | Custom Solidity contract | Deploy a liquidator contract that orchestrates the flash loan + liquidation + swap |
| Flashbots | `alloy` + direct API calls | Bundle submission to Flashbots relay |
| Health factor math | `rust_decimal` or `ethnum` | On-chain math precision |
| Protocol ABIs | `alloy-sol-types` | Generate Rust bindings from ABIs |
| Gas estimation | `alloy` `eth_estimateGas` | Accurate gas pricing for profitability calculation |

### Competition Analysis

**Ethereum mainnet**: Liquidation is one of the most competitive MEV activities. Established liquidation bots have:
- Pre-deployed optimized liquidator contracts (gas-minimized assembly)
- Private mempool access via Flashbots
- Automated health factor monitoring with sub-block latency
- Multiple strategies: simple liquidation, flash-loan-powered, multi-hop liquidation

**By the numbers** (2026 Ethereum):
- ~95% of large liquidations (>$50K) are captured by the top 5 liquidator addresses
- Small liquidations (<$5K) are less contested but gas costs eat most of the profit
- Average liquidation bonus: 5% (Aave v3) — but after gas + flash loan fees + swap slippage, net profit is 2-3%
- On a $5,000 liquidation: ~$100-150 gross, minus $20-50 gas = **$50-130 net**

**L2s (Arbitrum, Base, Optimism)**:
- Competition is SIGNIFICANTLY lower
- Gas costs are 10-50× cheaper than mainnet
- Liquidation volumes are growing as DeFi migrates to L2s
- Fewer dedicated liquidation bots operate on L2s (as of early 2026)

**Solana** (Marginfi, Kamino, Solend):
- Even less competitive than L2s
- Transaction costs are negligible (~$0.001)
- But liquidation volumes are smaller

### Solo Dev Edge

**Ethereum mainnet**: ❌ No edge. You're competing against searchers who have been optimizing for 3+ years.

**L2s (Arbitrum, Base)**: ⚠️ **Possible edge.** Lower competition, cheaper gas, growing liquidation volumes. A well-built bot can capture small-to-medium liquidations that the big players skip because the absolute profit is too small for their infrastructure costs.

**Solana**: ⚠️ **Better edge.** Fewer dedicated liquidation bots. Rust is the native language of Solana (the validator is written in Rust), so your skillset is directly applicable.

### Difficulty Rating: 4/5

- Requires deploying and optimizing smart contracts (Solidity for EVM, or Rust/Anchor for Solana)
- Flash loan integration is non-trivial
- Health factor monitoring at scale (thousands of positions) requires efficient data structures
- Gas optimization for the liquidation contract is important for competitiveness
- Flashbots/Jito integration adds complexity

### Key Risks

1. **Unprofitable liquidations**: If gas cost > liquidation bonus, you lose money. This happens when gas spikes during volatile periods (exactly when liquidations occur).
2. **Oracle manipulation**: If the price oracle is manipulated, you might liquidate a position that's actually healthy — the protocol might revert the liquidation.
3. **Smart contract bugs**: Your liquidator contract could have bugs. Testing on mainnet forks is essential.
4. **Competition escalation**: If you're consistently profitable, others will notice and compete on the same positions.
5. **Protocol changes**: Aave/Compound governance can change liquidation bonuses, parameters, or mechanisms.
6. **Declining volumes**: In stable/bull markets, fewer positions become liquidatable. Income is highly correlated with market volatility and crashes.

### Preliminary Viability

| Aspect | Ethereum Mainnet | L2s (Arbitrum/Base) | Solana |
|--------|-----------------|--------------------:|--------|
| Viability | ❌ Not viable | ⚠️ Conditionally viable | ⚠️ Conditionally viable |
| Expected monthly return | -€50 to €0 | €50-300 | €50-200 |
| Capital needed (with flash loans) | €2-5K (gas only) | €1-2K | €1K |
| Competition | Extreme | Moderate | Low-Moderate |
| Dev time | 150-200h | 120-160h | 100-140h |
| Income consistency | Volatile (crash-dependent) | Volatile | Volatile |

**Verdict**: Only viable on L2s and Solana. Income is highly volatile — you might earn €500 in a crash month and €0 for three months straight. Not a reliable income stream. **Tier 3** — technically possible but not a primary strategy.

---

## Strategy 3: MEV / Backrunning (NOT Sandwich Attacks)

### Mechanism

**Backrunning** means placing your transaction immediately AFTER a large pending transaction that will move the price. This is ethical MEV (unlike sandwich attacks which harm the original trader).

Examples:
1. **Arbitrage after large swaps**: A whale swaps $500K ETH→USDC on Uniswap, moving the pool price 0.3% below CEX price. Your bot backruns this swap with an arb that buys the now-cheap ETH on Uniswap and sells on a CEX or another DEX.
2. **Arbitrage after oracle updates**: Chainlink updates a price feed, making a lending protocol's collateral ratios stale for one block. Your bot exploits the temporary mispricing.
3. **Rebalance after governance actions**: A protocol changes parameters (fee tiers, weights), creating temporary price dislocations.

**MEV-Share (Ethereum)**: Flashbots' MEV-Share protocol allows searchers to receive hints about pending transactions and submit backrun bundles. The original user gets a refund of part of the MEV extracted — ethically better than pure extraction.

**Jito (Solana)**: Jito bundles allow searchers to include their transactions right after a target transaction in the same bundle. The searcher pays a tip to the validator.

### Architecture

```
┌─────────────────────────────────────────────┐
│ Transaction Stream                           │
│ • Ethereum: MEV-Share event stream           │
│ • Solana: Jito block engine gRPC stream      │
│ • Alternative: Raw mempool (geth/erigon)     │
└───────────────┬─────────────────────────────┘
                │
        ┌───────▼──────────────┐
        │ Opportunity Classifier│
        │ • Is this a large swap?│
        │ • What pools are       │
        │   affected?            │
        │ • What's the post-swap │
        │   price impact?        │
        └───────┬──────────────┘
                │ profitable backrun found
        ┌───────▼──────────────┐
        │ Backrun TX Builder    │
        │ • Simulate on local   │
        │   EVM/SVM fork        │
        │ • Calculate exact     │
        │   profit after gas/tip│
        │ • Build optimized TX  │
        └───────┬──────────────┘
                │
        ┌───────▼──────────────┐
        │ Bundle Submission     │
        │ • Flashbots/MEV-Share │
        │ • Jito bundle         │
        │ • Bid: % of profit    │
        └──────────────────────┘
```

**Critical capability**: You need to simulate the target transaction locally, compute the resulting state change, and determine if a profitable backrun exists — all within ~200ms (before the block is built).

### Tech Stack

| Component | Crate/Tool | Notes |
|-----------|-----------|-------|
| EVM simulation | `revm` (Rust EVM) | Run transactions locally to simulate state changes |
| Solana simulation | `solana-runtime` or `litesvm` | Local SVM execution |
| MEV-Share client | Custom HTTP SSE client | Flashbots MEV-Share endpoint |
| Jito client | `jito-sdk-rust` | gRPC connection to Jito block engine |
| Transaction building | `alloy` / `solana-sdk` | Chain-specific TX construction |
| State tracking | Local state DB or in-memory | Track pool states, oracle prices |
| Profitability math | `rust_decimal` + `revm` trace output | Simulate exact profit |

### Competition Analysis

**Ethereum**:
- The backrun market is mature. Top searchers use `revm` (Rust EVM) for sub-millisecond simulation.
- MEV-Share redistributes ~90% of backrun profit to the user, leaving ~10% for the searcher.
- Competition for backrunning large swaps: extreme (hundreds of searchers).
- Competition for backrunning on niche protocols: moderate.

**Solana**:
- Jito bundles are the primary mechanism. ~70% of validators run Jito.
- The Solana searcher ecosystem is less mature than Ethereum's.
- Transaction simulation is harder on Solana (account model vs. storage model).
- Competition: moderate and growing.

### Solo Dev Edge

**Ethereum**: ❌ **No realistic edge for common backruns.** The top Ethereum searchers have:
- Custom `revm` forks optimized for their specific strategies
- Years of accumulated infrastructure
- Private order flow from MEV-Share / order flow auctions
- The ability to bid higher because their simulation is faster and more accurate

⚠️ **Possible edge on niche protocols**: If you focus on a specific DeFi protocol that big searchers haven't bothered to integrate (e.g., a new lending protocol with $50M TVL), you might be the only backrunner for that protocol's interactions.

**Solana**: ⚠️ **Better prospects.** The Rust-native ecosystem means your skills are directly applicable. Fewer dedicated searcher teams operate on Solana. Jito bundle competition is real but less intense than Flashbots.

### Difficulty Rating: 5/5

This is the hardest strategy to build:
- Requires local EVM/SVM simulation at high speed
- Must parse and classify arbitrary transactions
- Bundle bidding strategy is non-trivial
- Real-time profitability calculation under time pressure
- Deep understanding of DeFi protocol mechanics

### Key Risks

1. **Simulation accuracy**: If your local simulation doesn't match the actual on-chain execution, you'll submit unprofitable bundles.
2. **Bundle failure**: Your bundle might not be included (outbid by another searcher).
3. **Gas costs**: On Ethereum, failed bundle submissions cost nothing (Flashbots doesn't charge for losing bids). On Solana, Jito tips are only charged if included.
4. **Protocol risk**: If the DeFi protocol you're backrunning has a bug or gets exploited, your bot might execute in a corrupted state.
5. **Regulatory**: MEV extraction is in a grey area. EU regulators haven't ruled on it explicitly.

### Preliminary Viability

| Aspect | Ethereum | Solana |
|--------|----------|--------|
| Viability | ❌ Not viable (common backruns), ⚠️ niche only | ⚠️ Conditionally viable |
| Expected monthly return | €0-100 (niche), -€50 to €0 (common) | €100-400 |
| Capital needed | €5-10K (gas + on-chain capital) | €2-5K |
| Competition | Extreme (common), Moderate (niche) | Moderate |
| Dev time | 200-300h | 150-200h |

**Verdict**: The highest technical barrier of all strategies. Only viable on Solana or niche Ethereum protocols. Development time is enormous for uncertain returns. **Tier 3** — impressive engineering project, poor risk/reward for a solo dev seeking income.

---

## Strategy 4: Funding Rate Arbitrage ⭐ TOP PICK

### Mechanism

Perpetual futures contracts (perps) have a **funding rate** — a periodic payment between longs and shorts that keeps the perp price anchored to the spot price. When the market is bullish, longs pay shorts (positive funding). When bearish, shorts pay longs (negative funding).

**The arbitrage**:
1. When funding rate is positive (longs pay shorts):
   - **Buy spot** (long the actual asset)
   - **Short the perpetual** (receive funding payments)
   - Your position is **delta-neutral** — price movements cancel out
   - You earn the funding rate every 8 hours (on most exchanges)

2. When funding rate is negative (shorts pay longs):
   - **Sell/short spot** (or sell existing holdings)
   - **Long the perpetual** (receive funding payments)

**Why this is different from CEX-CEX arb**:

| Factor | CEX-CEX Arb | Funding Rate Arb |
|--------|------------|-----------------|
| Speed sensitivity | Sub-second (HFT wins) | Hours (funding settles every 8h) |
| Competition | Extreme (latency race) | Moderate (capital allocation) |
| Edge required | Faster execution | Better capital efficiency and risk management |
| Profit mechanism | Price discrepancy (fleeting) | Structural funding payment (persistent) |
| Capital efficiency | High (recycled per trade) | Lower (locked in position) |
| Risk | Execution/slippage | Exchange counterparty risk |

**Why it works for retail**: There is no speed advantage. A bot that checks funding rates every 5 minutes and enters positions over an hour has the SAME return as a bot that enters instantly. The edge is **capital allocation** (choosing which pairs to fund-arb) and **risk management** (managing margin and counterparty exposure), NOT speed.

### Realistic Funding Rates (2026 Data)

| Pair | Avg Positive Funding (8h) | Annualized | Frequency (% of periods positive) |
|------|--------------------------|------------|-----------------------------------|
| BTC/USDT | 0.005-0.015% | 6.5-19.7% | ~65% of periods |
| ETH/USDT | 0.005-0.020% | 6.5-26.3% | ~60% of periods |
| SOL/USDT | 0.010-0.040% | 13.1-52.6% | ~55% of periods |
| Altcoins (avg) | 0.020-0.100% | 26.3-131.4% | ~50% of periods |

**Important caveats**:
- These are AVERAGES. Funding rates swing. During a market crash, funding goes deeply negative.
- You only capture positive funding when you're positioned correctly.
- Altcoin funding rates are higher but more volatile and less predictable.
- The annualized figure is theoretical — you won't be in position 100% of the time.

**Realistic expected annual return**: 8-25% on deployed capital for major pairs (BTC/ETH), before exchange fees. 15-50% on smaller altcoin pairs with higher risk.

### Architecture

```
┌──────────────────────────────────────────────────┐
│ Funding Rate Monitor                              │
│ • Poll funding rates across exchanges every 5min  │
│ • Historical funding rate tracker (rolling 7-day)  │
│ • Predicted next funding rate calculation          │
└───────────────┬──────────────────────────────────┘
                │
        ┌───────▼──────────────┐
        │ Position Manager      │
        │ • Open: buy spot +    │
        │   short perp when     │
        │   funding > threshold │
        │ • Close: when funding │
        │   drops below exit    │
        │   threshold           │
        │ • Rebalance: adjust   │
        │   hedge ratio         │
        └───────┬──────────────┘
                │
        ┌───────▼──────────────┐
        │ Risk Monitor          │
        │ • Margin ratio check  │
        │ • Basis risk tracking │
        │ • Exchange exposure   │
        │   limits              │
        │ • Liquidation price   │
        │   monitoring          │
        └───────┬──────────────┘
                │
        ┌───────▼──────────────┐
        │ P&L Tracker           │
        │ • Per-position P&L    │
        │ • Funding collected   │
        │ • Trading fees paid   │
        │ • Net daily/weekly ROI│
        └──────────────────────┘
```

**This is architecturally SIMPLE compared to arb bots**:
- No mempool monitoring
- No sub-second execution
- No smart contract interaction (all CEX-based)
- No gas optimization
- Polling every 1-5 minutes is sufficient

### Tech Stack

| Component | Crate/Tool | Notes |
|-----------|-----------|-------|
| Exchange APIs | `reqwest` + `tokio-tungstenite` | Binance Futures, Bybit, dYdX, OKX |
| Funding rate data | Exchange REST APIs | `/fapi/v1/fundingRate` (Binance), `/v5/market/funding/history` (Bybit) |
| Position management | `rust_decimal` | Precise position sizing, margin calculations |
| Persistence | `sqlx` + SQLite (or Postgres) | Track positions, funding collected, P&L |
| Scheduling | `tokio::time::interval` | Check rates every 5 minutes |
| Alerting | Telegram bot API (`teloxide` crate) | Alert on position changes, margin warnings |
| Config | `config` + `serde` | TOML-based configuration |
| HTTP server | `axum` | Dashboard/monitoring endpoints |

### Capital Deployment Example (€30K)

**Conservative strategy (major pairs only)**:

```
Pair:       BTC/USDT
Spot buy:   €10,000 worth of BTC (on Binance spot)
Perp short: €10,000 worth of BTC (on Binance futures, 1x leverage)
Margin:     €5,000 in futures account (2x leverage on the perp)
Reserve:    €5,000 cash (for margin calls / rebalancing)
Total deployed: €20,000

Remaining: €10,000 for a second pair (ETH/USDT) or as buffer

Avg funding per 8h period: 0.01% on €10,000 = €1.00
Periods per day: 3
Daily gross: €3.00
Monthly gross: €90.00

Trading fees (entry + exit):
  Spot: 0.10% × €10,000 × 2 (open + close) = €20
  Perp: 0.04% × €10,000 × 2 = €8
  Total per round-trip: €28

If position held for 30 days: €90 gross - €28 fees = €62 net
Monthly ROI on deployed capital: 0.31%
Annualized: 3.7%
```

**That's the CONSERVATIVE case on BTC alone.** Let's be more realistic:

**Moderate strategy (BTC + ETH + 1 altcoin, dynamic allocation)**:

```
Total capital: €30,000
BTC position: €10,000 → €60-90/month net
ETH position: €10,000 → €70-120/month net  
SOL position: €5,000  → €50-150/month net
Reserve:      €5,000

Monthly range: €180-360
Annualized: 7.2-14.4% on total capital

Monthly costs:
- VPS: €20 (lightweight, no GPU/HPC needed)
- Dev time: 4h/month × €100 = €400 (BUT: if this is YOUR bot for YOUR money,
  dev time is investment, not a cost to deduct from returns)

Monthly net (excluding your time): €160-340
```

**Aggressive strategy (multiple altcoins, higher funding rates)**:

```
Total capital: €30,000
5 positions across high-funding pairs: €25,000 deployed
Reserve: €5,000

Monthly gross: €500-1500 (altcoin funding rates are much higher but volatile)
Monthly fees: €100-200
Monthly net: €300-1300

Risk: Much higher. Altcoin perp markets can have flash crashes, 
      exchange-specific liquidation events, and basis blowouts.
```

### Competition Analysis

**Who else does this?**
- Institutional funds (but they need >$1M positions to be worth their time)
- Other retail traders (manual, not automated)
- Some automated bots (but the market isn't winner-take-all)

**Why competition is DIFFERENT here**: In CEX-CEX arb, only the fastest bot captures each opportunity. In funding rate arb, **every participant earns the same rate**. It's not a race — it's a carry trade. The "competition" is about capital allocation (choosing the best pairs) and risk management (avoiding liquidation), not speed.

**The market isn't zero-sum for funding rate arb**: When funding is positive, ALL short-perp holders receive funding. Your bot doesn't take funding away from other bots. The limiting factor is the total open interest available, not the number of participants.

**What DOES limit returns**: If too many people do this, the funding rate itself decreases (more shorts → less short-paying funding → lower returns). But as of 2026, funding rates remain significant because retail speculators continue to go leveraged long during bull markets.

### Solo Dev Edge

✅ **STRONG EDGE**. Here's why:

1. **No speed requirement**: A bot that checks rates every 5 minutes works exactly as well as one checking every 5 milliseconds.
2. **Rust advantage is in reliability, not speed**: 24/7 uptime, no GC pauses, robust error handling — these matter more than raw speed here.
3. **Solo dev can iterate faster**: Test new pairs, adjust thresholds, implement new exchanges without committee decisions.
4. **Low infrastructure cost**: A €20/month VPS is sufficient. Institutional overhead (compliance, reporting, team) doesn't apply.
5. **Capital is the differentiator, not technology**: €30K generates the same per-€ return as €30M. You earn proportionally.

### Difficulty Rating: 2/5

This is the EASIEST strategy to build:
- Standard REST API integration (no blockchain interaction)
- Polling every 1-5 minutes (no real-time requirements)
- Simple math (funding rate × position size - fees)
- Well-documented exchange APIs for perpetual futures
- No smart contracts, no gas optimization, no mempool monitoring

### Key Risks

1. **Basis risk**: The spot and perp price can diverge temporarily, causing unrealized losses. If you're forced to close during a basis blowout, you crystallize the loss.
2. **Funding rate reversal**: Funding turns negative while you're positioned for positive funding. You start PAYING instead of receiving. Your bot must detect this and exit.
3. **Liquidation**: If you use leverage on the perp side and the price moves sharply, your perp position can be liquidated. **Solution**: Use low leverage (1-2x) and maintain reserve margin.
4. **Exchange counterparty risk**: Your capital is on CEXs. FTX-style collapses can happen. **Mitigation**: Spread across 2-3 exchanges; never put >40% on one exchange.
5. **Margin call during maintenance**: If the exchange goes down for maintenance while a large price move happens, your margin might be insufficient when it comes back. **Mitigation**: Over-collateralize; use stop-losses.
6. **Fee erosion**: If positions are opened and closed frequently (chasing funding rate changes), trading fees eat returns. **Solution**: Only open/close when the expected holding period funding exceeds 5× the entry/exit fees.
7. **Tax complexity**: Each position open/close and each funding payment is a taxable event in the Netherlands (box 3 asset taxation). Track everything meticulously.

### Preliminary Viability

| Aspect | Assessment |
|--------|-----------|
| **Viability** | ✅ **VIABLE** — the most promising strategy for retail |
| Expected monthly return (conservative) | €100-200 on €30K |
| Expected monthly return (moderate) | €200-400 on €30K |
| Expected monthly return (aggressive) | €300-1300 on €30K (high variance) |
| Capital needed | €10K minimum, €30K+ recommended |
| Competition impact | Low (not zero-sum) |
| Dev time | 80-120h for a production-quality bot |
| Infra cost | €20-40/month |
| Break-even on dev time | 3-8 months at moderate returns |

**Verdict**: ⭐ **TIER 1 — BUILD THIS FIRST.** This is the only strategy where:
- Retail has no structural disadvantage
- Returns exceed fees at €10-50K capital
- Speed doesn't matter
- The market isn't winner-take-all
- Development complexity is manageable for a solo dev
- Monthly returns are positive under CONSERVATIVE assumptions

---

## Strategy 5: Cross-Chain Arbitrage (Ethereum ↔ Arbitrum ↔ Solana ↔ Base)

### Mechanism

The same token (e.g., USDC, ETH, SOL-wrapped) trades at slightly different prices across different blockchains. Arbitrage involves:

1. Detect price difference: ETH on Arbitrum DEXs is $3,000 vs. $3,010 on Base
2. Buy cheap on Arbitrum
3. Bridge to Base
4. Sell on Base DEX

**The critical problem**: Bridging takes time. Standard Ethereum L1 ↔ L2 bridges have 7-day withdrawal periods (optimistic rollups). Fast bridges (Across, Stargate, Hop) take 1-15 minutes but charge 0.05-0.2% fees.

### Architecture

```
┌────────────────────────────────────────────┐
│ Multi-Chain Price Monitor                   │
│ • RPC connections to 4+ chains             │
│ • DEX pool price tracking on each chain    │
│ • Bridge fee/time estimation               │
└────────────────┬───────────────────────────┘
                 │
         ┌───────▼───────────┐
         │ Cross-Chain Opp    │
         │ Detector           │
         │ • Price diff >     │
         │   bridge fee +     │
         │   swap fees +      │
         │   gas × 2          │
         │ • Price drift risk │
         │   during bridge    │
         └───────┬───────────┘
                 │
         ┌───────▼───────────┐
         │ Execution          │
         │ 1. Buy on chain A  │
         │ 2. Bridge A → B    │
         │ 3. Wait (1-15 min) │
         │ 4. Sell on chain B │
         │ (steps 1+2 atomic, │
         │  step 4 is delayed)│
         └───────────────────┘
```

### Competition Analysis

- Bridge aggregators (Li.Fi, Socket) already optimize cross-chain routes
- Dedicated cross-chain arbitrageurs exist (Circle's CCTP for USDC makes this easier for stablecoins)
- The price difference must exceed: bridge fee + gas on both chains + price risk during bridge time
- Fast bridges reduce time but increase fees

### Solo Dev Edge

❌ **Limited edge.** Cross-chain arb has a fundamental problem: **you're exposed to price risk during the bridge window.** If ETH drops 0.5% in the 5 minutes it takes to bridge, you lose money. Institutional players mitigate this by having pre-positioned capital on ALL chains (no bridging needed), which is exactly the same capital fragmentation problem as CEX-CEX arb.

To truly compete, you'd need €50K+ spread across 4+ chains — which is the user's entire capital budget.

### Difficulty Rating: 4/5

- Multi-chain RPC management
- Bridge protocol integration (multiple bridges with different APIs)
- Price risk modeling during bridge time
- Capital pre-positioning across chains

### Key Risks

1. **Price movement during bridge**: The #1 risk. You're directionally exposed for 1-15 minutes.
2. **Bridge failure/delay**: Bridges can be congested, paused, or hacked.
3. **Capital fragmentation**: Capital spread across chains reduces position size per chain.
4. **Gas costs on multiple chains**: Two swap transactions + bridge transaction.
5. **Bridge hack risk**: Cross-chain bridges have been the #1 target for DeFi hacks ($2B+ stolen from bridges 2022-2024).

### Preliminary Viability

| Aspect | Assessment |
|--------|-----------|
| Viability | ❌ Not viable at €10-50K |
| Expected monthly return | -€100 to €100 |
| Capital needed | €50K+ across 4+ chains |
| Competition | High (bridge-native arbs + pre-positioned capital) |
| Dev time | 160-220h |

**Verdict**: **TIER 4 — AVOID.** Capital fragmentation, bridge risk, and price exposure during transit make this structurally unfavorable for small capital. The "arb" is really just a bet that the price won't move while you bridge.

---

## Strategy 6: Long-Tail / Altcoin Pair Arbitrage ⭐ TOP PICK

### Mechanism

Major pairs (BTC/USDT, ETH/USDT) are efficiently priced across exchanges. But smaller pairs and smaller exchanges have persistent inefficiencies:

- **SHIB/USDT** on MEXC vs. Gate.io might have 0.3-1% spreads
- **ARB/BTC** on KuCoin vs. Bybit might be 0.2-0.5% apart
- **Newer tokens** listed on 2-3 exchanges but not yet arbitraged
- **Regional exchanges** (Upbit Korea, Bitflyer Japan) price tokens differently due to local demand

**Why HFT firms don't bother**: The volume on SHIB/KuCoin or some obscure altcoin pair is $50K-500K/day. Even capturing 100% of the arb opportunity yields $500-5,000/month. That doesn't pay for one HFT engineer's salary, let alone co-location.

**But it pays a solo developer**: $500-5,000/month is excellent if your costs are a €20 VPS and your own time.

### Architecture

```
┌──────────────────────────────────────────────────────┐
│ Multi-Exchange Scanner                                │
│ • 5-10 exchanges: Binance, Bybit, KuCoin, MEXC,     │
│   Gate.io, OKX, Bitget, HTX                          │
│ • WebSocket feeds for order books on 50-200 pairs    │
│ • Normalized into common OrderBook format             │
└────────────────────┬─────────────────────────────────┘
                     │
             ┌───────▼──────────┐
             │ Opportunity       │
             │ Scanner           │
             │ • Cross all pairs │
             │   × all exchanges │
             │ • Filter by:      │
             │   min_spread,     │
             │   min_volume,     │
             │   fee-adjusted    │
             └───────┬──────────┘
                     │
             ┌───────▼──────────┐
             │ Execution         │
             │ • Place IOC limit │
             │   orders on both  │
             │   exchanges       │
             │ • Partial fill    │
             │   handling        │
             │ • Inventory       │
             │   tracking        │
             └───────┬──────────┘
                     │
             ┌───────▼──────────┐
             │ Inventory/Balance │
             │ Manager           │
             │ • Track balances  │
             │   per exchange    │
             │ • Alert on low    │
             │   balance         │
             │ • Suggest rebal   │
             │   transfers       │
             └──────────────────┘
```

### Capital Deployment Example (€30K)

```
Spread across 4 exchanges: €7,500 each (half in USDT, half in various altcoins)
Monitor 100 pairs across 4 exchanges → 600 cross-exchange pair comparisons
Average altcoin spread: 0.3-0.8% (after fees of 0.15-0.20% per leg)
Net spread per trade: 0.1-0.5%
Trade size: €200-500 (limited by liquidity)
Trades per day: 5-20 (when spreads exist)

Conservative:
  5 trades/day × €300 × 0.15% net = €2.25/day = €67.50/month

Moderate:
  10 trades/day × €400 × 0.25% net = €10/day = €300/month

Optimistic:
  20 trades/day × €500 × 0.35% net = €35/day = €1,050/month
```

### Tech Stack

| Component | Crate/Tool | Notes |
|-----------|-----------|-------|
| Exchange connectors | `ccxt-rs` or custom per-exchange | 5-10 exchange REST + WebSocket |
| Order book normalization | Custom `arb-types` crate | Unified order book format |
| Opportunity detection | Custom, O(pairs × exchanges²) | Cross-exchange comparisons |
| Execution | `reqwest` + exchange-specific auth | IOC limit orders |
| Persistence | `sqlx` + SQLite | Trade records, P&L tracking |
| Monitoring | `axum` + `teloxide` | Web dashboard + Telegram alerts |
| Rate limiting | `governor` crate | Per-exchange, per-endpoint rate limits |
| Async runtime | `tokio` | Multi-threaded for parallel feeds |

### Competition Analysis

- **HFT firms**: NOT present on most altcoin pairs. The volume doesn't justify their infrastructure.
- **Other retail bots**: Some exist, but the market is fragmented (hundreds of pairs × dozens of exchanges = thousands of niche markets). No single bot covers everything.
- **Exchange market makers**: Some exchanges have designated market makers for popular pairs, but long-tail pairs are often unattended.

**Key competition dynamic**: This is a LONG TAIL market. No single competitor captures everything. A bot that monitors 100 pairs across 5 exchanges has a different set of opportunities than one monitoring 80 pairs across 4 exchanges. There's room for multiple participants.

### Solo Dev Edge

✅ **STRONG EDGE**:

1. **Niche markets**: HFT firms don't operate here. The absolute profit per trade ($1-5) doesn't justify institutional costs.
2. **Breadth over depth**: A solo dev can add exchanges and pairs incrementally. Each new exchange/pair combination is a new niche with potentially uncontested opportunities.
3. **Low latency isn't critical**: Altcoin spreads persist for seconds to minutes (not milliseconds). A 500ms-1s execution is fine.
4. **Rust reliability**: Running 100+ WebSocket connections 24/7 requires reliable async code. Rust + tokio excels here.
5. **Customizable**: You can add/remove pairs based on observed profitability. Automated pair discovery can find new opportunities.

### Difficulty Rating: 3/5

- Multi-exchange connector development is the biggest time investment
- Each exchange has unique API quirks (the same problem from SPEC.md, but multiplied)
- Inventory management across exchanges is non-trivial
- Rebalancing (withdrawals/deposits between exchanges) requires manual intervention

### Key Risks

1. **Rug pulls / delistings**: An altcoin gets delisted from one exchange while you hold inventory. Mitigate by avoiding micro-caps and checking listing status.
2. **Low liquidity traps**: The spread looks profitable but the order book has $50 of depth. Your order moves the market against you. Mitigate with minimum liquidity thresholds.
3. **Exchange counterparty risk**: Smaller exchanges (MEXC, Gate.io) have higher counterparty risk than Binance. Don't over-allocate to any single small exchange.
4. **Balance fragmentation**: Capital spread across 5 exchanges means €6K per exchange. After splitting between USDT and various tokens, available capital per trade is small.
5. **Withdrawal delays/fees**: Rebalancing between exchanges takes time and costs money. Some tokens have expensive or slow withdrawals.
6. **API rate limits**: Running 100+ WebSocket connections on 5 exchanges requires careful rate limit management.
7. **Fee tier sensitivity**: At small volumes, you're on the worst fee tier (0.10-0.20% taker). This eats into the already-small spreads.

### Preliminary Viability

| Aspect | Assessment |
|--------|-----------|
| **Viability** | ✅ **VIABLE** — genuinely accessible niche for retail |
| Expected monthly return (conservative) | €50-100 on €30K |
| Expected monthly return (moderate) | €200-400 on €30K |
| Expected monthly return (optimistic) | €500-1000 on €30K |
| Capital needed | €15K-30K (spread across exchanges) |
| Competition | Low-Moderate (long tail) |
| Dev time | 150-200h (multi-exchange connectors are the bottleneck) |
| Infra cost | €20-40/month |

**Verdict**: ⭐ **TIER 1 — STRONG CANDIDATE.** This is the second-most promising strategy. It has genuine retail edges, manageable competition, and decent returns. The main challenge is development breadth (many exchange connectors) and operational complexity (managing balances across exchanges). Pairs well with Strategy 4 (Funding Rate) since both use the same exchange infrastructure.

---

## Strategy 7: Stablecoin Depeg Arbitrage

### Mechanism

Stablecoins (USDC, USDT, DAI, FRAX) occasionally trade below their $1 peg due to market fear, technical issues, or actual risk events. When this happens:

1. **Buy the depeg**: Purchase USDC at $0.98 on a DEX or CEX
2. **Redeem at par**: Use Circle's redemption mechanism to convert USDC → USD at $1.00
3. **Profit**: $0.02 per USDC (2% return)

Or, for on-chain stablecoins:
- Buy DAI at $0.97 on Uniswap
- Redeem DAI through MakerDAO's PSM (Peg Stability Module) for $1.00 of USDC
- Profit: $0.03 per DAI

### Historical Examples

| Event | Date | Depeg | Duration | Max Discount |
|-------|------|-------|----------|-------------|
| USDC (SVB crisis) | March 2023 | USDC→$0.878 | ~3 days | 12.2% |
| DAI (cascading from USDC) | March 2023 | DAI→$0.897 | ~2 days | 10.3% |
| USDT (periodic FUD) | Various | USDT→$0.995 | Hours | 0.5% |
| FRAX (after Terra) | Mid-2022 | FRAX→$0.95 | Weeks | 5% |
| UST (Terra collapse) | May 2022 | UST→$0.00 | Permanent | 100% (total loss) |

**Key insight**: The profitable events (USDC/DAI during SVB) happen 1-3 times per year. The UST collapse shows the risk: not all depegs are opportunities — some are death spirals.

### Architecture

```
┌──────────────────────────────────────────────────┐
│ Depeg Monitor (runs 24/7, alerts on threshold)    │
│ • Watch stablecoin prices on DEXs (Curve, Uni)   │
│ • Watch CEX stablecoin pairs                     │
│ • Monitor redemption mechanism status             │
│ • Track on-chain reserve data (USDC attestations) │
└────────────────────┬─────────────────────────────┘
                     │ depeg > threshold (e.g., 0.5%)
             ┌───────▼──────────┐
             │ Risk Assessment   │
             │ • Is this a "buy  │
             │   the dip" or a   │
             │   "death spiral"? │
             │ • Check reserves  │
             │ • Check redemption│
             │   mechanism status│
             │ • Human override  │
             │   option          │
             └───────┬──────────┘
                     │
             ┌───────▼──────────┐
             │ Execution         │
             │ • Buy on DEX/CEX  │
             │ • Initiate        │
             │   redemption      │
             │ • Wait for        │
             │   settlement      │
             └──────────────────┘
```

### Competition Analysis

- During depeg events, EVERYONE wants to buy. The arb is obvious.
- But many people are scared (that's what causes the depeg).
- Speed of execution matters: buying USDC at $0.98 is better than buying at $0.995 once others have arbed it back up.
- Redemption mechanisms have processing times (Circle: 1-3 business days for large redemptions).

### Solo Dev Edge

⚠️ **Moderate edge, but rare opportunities.** The bot's value is being ready when events happen. A human might hesitate during a panic; the bot doesn't. But the fundamental assessment ("is this a temporary depeg or a real collapse?") requires judgment that's hard to automate.

### Difficulty Rating: 2/5

Simple to build:
- Price monitoring + alert system
- DEX/CEX buy execution
- The hard part is the RISK ASSESSMENT, not the code

### Key Risks

1. **Death spiral**: If the stablecoin actually collapses (like UST), you lose your entire position.
2. **Redemption freeze**: Circle could pause USDC redemptions during a crisis. You're stuck with depeg'd tokens.
3. **Opportunity frequency**: Maybe 1-3 meaningful events per year. The bot sits idle 99% of the time.
4. **Capital lockup**: USDC redemption takes 1-3 business days. Capital is locked during this period.
5. **Counterparty risk**: If Circle goes bankrupt, USDC is worth $0 regardless of peg.

### Preliminary Viability

| Aspect | Assessment |
|--------|-----------|
| Viability | ⚠️ Conditionally viable (event-driven, rare) |
| Expected monthly return | €0 most months, €500-5000 during events |
| Expected annual return | €1,000-10,000 (highly variable) |
| Capital needed | €5-20K (deployed only during events) |
| Competition | Moderate during events |
| Dev time | 30-50h |
| Infra cost | €5-10/month (just a monitor + alert) |

**Verdict**: **TIER 2 — BUILD AS A SIDE MODULE, NOT PRIMARY.** Low dev effort, but income is sporadic. Best combined with another strategy (e.g., Funding Rate Arb) so the capital is earning returns between depeg events. The monitoring and alerting system is worth building regardless.

---

## Strategy 8: Market Making on Illiquid DEXs

### Mechanism

Instead of finding existing price discrepancies (arbitrage), you CREATE liquidity and earn the spread. On Uniswap v3/v4, you can provide **concentrated liquidity** — placing your capital in a narrow price range to maximize fee earnings.

Example:
- ETH is trading at $3,000
- You provide $10,000 of liquidity in the $2,950-$3,050 range on Uniswap v3
- Every swap that passes through your range pays you 0.3% fees (for the 0.3% fee tier pool)
- Because your liquidity is concentrated, you earn MORE fees per dollar than passive LPs

**On illiquid DEXs/pools** (smaller tokens, L2 DEXs, Solana DEXs):
- Fewer LPs means wider effective spreads
- Your concentrated position captures a larger share of fees
- But: impermanent loss is higher in volatile pools

### Architecture

```
┌────────────────────────────────────────────────────┐
│ Pool Monitor                                        │
│ • Track pool TVL, volume, fee revenue across DEXs  │
│ • Calculate APY for concentrated positions          │
│ • Monitor price relative to your position range     │
└────────────────────┬───────────────────────────────┘
                     │
             ┌───────▼──────────┐
             │ Position Manager  │
             │ • Open: mint LP  │
             │   with tight     │
             │   range          │
             │ • Rebalance:     │
             │   when price     │
             │   exits range    │
             │ • Close: burn LP │
             │   when APY drops │
             └───────┬──────────┘
                     │
             ┌───────▼──────────┐
             │ IL Calculator     │
             │ • Real-time IL    │
             │   tracking        │
             │ • Net P&L:       │
             │   fees - IL       │
             │ • Auto-exit on   │
             │   net-negative   │
             └───────┬──────────┘
                     │
             ┌───────▼──────────┐
             │ Range Optimizer   │
             │ • Volatility-     │
             │   adjusted ranges │
             │ • Backtest against│
             │   historical data │
             └──────────────────┘
```

### Tech Stack

| Component | Crate/Tool | Notes |
|-----------|-----------|-------|
| Uniswap v3/v4 interaction | `alloy` + Uniswap ABI bindings | Mint, burn, collect fees |
| Pool state reading | `alloy` multicall | Batch-read pool states |
| IL calculation | Custom math (`rust_decimal`) | Black-Scholes-adjacent math for IL |
| Price feeds | Chainlink oracles or DEX TWAP | Reference price for range setting |
| Solana LP | `solana-sdk` + Raydium/Orca SDK | Different pool mechanics |
| Backtesting | Custom + historical block data | Replay past pool activity |

### Competition Analysis

- Professional LPs (Arrakis, Gamma Strategies) provide optimized concentrated liquidity
- On popular pools (ETH/USDC on Uniswap mainnet), competition is fierce
- On illiquid pools (small-cap tokens on L2s), competition is minimal
- The "competition" is really impermanent loss — you're competing against the asset's volatility

### Solo Dev Edge

⚠️ **Moderate edge on illiquid pools.** Professional LP managers don't bother with pools that have <$100K daily volume. A solo dev managing 10-20 small pool positions can earn consistent fees without competing against professionals.

But: **impermanent loss is the hidden killer.** In backtests, most concentrated LP positions are net-negative (fees < IL) for volatile tokens. Only stable pairs (USDC/DAI, ETH/stETH) and carefully managed positions are consistently profitable.

### Difficulty Rating: 3/5

- Smart contract interaction for LP management
- IL math is non-trivial (especially for concentrated positions)
- Range optimization requires backtesting infrastructure
- Rebalancing timing is critical and hard to get right

### Key Risks

1. **Impermanent loss**: In a concentrated position, IL can be 5-20% during a 10% price move. Fees may not compensate.
2. **Smart contract risk**: Your LP position is a smart contract position. Exploits, bugs, or governance attacks can drain your liquidity.
3. **Gas costs for rebalancing**: Each time you adjust your range, you pay gas. Frequent rebalancing on Ethereum mainnet is expensive.
4. **"Just-in-time" liquidity attacks**: On Ethereum, sophisticated MEV bots provide liquidity for one block to capture fees, then withdraw. This dilutes your fee earnings.
5. **Pool obsolescence**: If volume shifts to a competing pool or DEX, your position earns no fees.

### Preliminary Viability

| Aspect | Assessment |
|--------|-----------|
| Viability | ⚠️ Conditionally viable (stable pairs and illiquid pools only) |
| Expected monthly return | €50-300 on €20K (stable pairs), -€500 to €500 (volatile pairs) |
| Capital needed | €10-20K |
| Competition | Low on illiquid pools, High on popular pools |
| Dev time | 120-160h |
| Risk of capital loss | Medium-High (IL on volatile tokens) |

**Verdict**: **TIER 2 — VIABLE FOR STABLE PAIRS ONLY.** Providing liquidity on stable/correlated pairs (USDC/DAI, ETH/stETH, WBTC/BTC) with tight ranges can generate 5-15% APY. Volatile pair LP is essentially gambling against IL. Requires deep understanding of LP math.

---

## Strategy 9: NFT Arbitrage (OpenSea ↔ Blur ↔ Magic Eden)

### Mechanism

NFTs from the same collection trade on multiple marketplaces with different prices. A floor-price NFT listed at 1.5 ETH on OpenSea might be available at 1.3 ETH on Blur. Buy on Blur, list on OpenSea.

### Architecture

```
Marketplace price aggregator → Cross-marketplace opportunity detector → 
Buy on cheap marketplace → List on expensive marketplace (or instant-sell)
```

### Competition Analysis

- NFT trading volume has declined significantly from 2021-2022 peaks
- Blur and OpenSea have aggregated most liquidity
- Floor-price arbitrage is commoditized (multiple bots do this)
- Trait-based pricing arbitrage requires ML/AI for trait valuation
- Marketplace royalty policies differ, affecting actual profit

### Solo Dev Edge

❌ **No edge.** The NFT market in 2026 is:
- Significantly smaller than 2021-2022
- Dominated by blur-native traders with sophisticated pricing models
- Subject to "wash trading" that inflates apparent volumes
- Illiquid for most collections outside the top 20

### Difficulty Rating: 3/5

### Key Risks

1. **Illiquidity**: You buy an NFT and can't sell it. Capital is locked indefinitely.
2. **Collection death**: The NFT collection loses all value.
3. **Marketplace changes**: Fee structure or royalty policy changes invalidate your P&L model.
4. **Gas costs**: Buying and selling NFTs on Ethereum costs $10-50 in gas per transaction.

### Preliminary Viability

| Aspect | Assessment |
|--------|-----------|
| Viability | ❌ Not viable for consistent income |
| Expected monthly return | -€200 to €200 (high variance) |
| Capital needed | €5-20K |
| Dev time | 100-140h |

**Verdict**: **TIER 4 — AVOID.** The NFT market is too small, too illiquid, and too risky for automated arbitrage in 2026. Capital can be locked in unsellable assets.

---

## Strategy 10: Telegram/Memecoin Sniping Bots

### Mechanism

When a new token launches on pump.fun (Solana), PancakeSwap (BNB Chain), or Uniswap (Ethereum), there's a brief window where:
1. The initial liquidity pool is created
2. Early buyers get in at the lowest price
3. Social media hype drives the price up 10-100x (sometimes)
4. The token either maintains value or crashes to zero

A sniping bot:
1. Monitors for new pool/token creation events on-chain
2. Analyzes the token contract for red flags (honeypot, tax, rugpull indicators)
3. Buys immediately in the same block or next block
4. Sets profit targets and stop-losses
5. Sells when target is hit or cut losses early

### Architecture

```
┌──────────────────────────────────────────────────┐
│ Token Launch Detector                             │
│ • Monitor pump.fun program (Solana)              │
│ • Monitor PairCreated events (Uniswap/PCS)       │
│ • Monitor Telegram channels for alpha            │
└────────────────────┬─────────────────────────────┘
                     │ new token detected
             ┌───────▼──────────┐
             │ Safety Scanner    │
             │ • Contract source │
             │   analysis        │
             │ • Honeypot check  │
             │ • Tax check       │
             │ • Mint authority  │
             │ • LP lock check   │
             │ • Owner analysis  │
             └───────┬──────────┘
                     │ passes safety checks
             ┌───────▼──────────┐
             │ Snipe Executor    │
             │ • High-priority TX│
             │ • Jito bundle     │
             │   (Solana)        │
             │ • Gas bid         │
             │   (Ethereum)      │
             └───────┬──────────┘
                     │
             ┌───────▼──────────┐
             │ Position Manager  │
             │ • Take profit at  │
             │   2x, 5x, 10x    │
             │ • Stop loss at    │
             │   -50%, -80%      │
             │ • Trail stop      │
             └──────────────────┘
```

### Tech Stack

| Component | Crate/Tool | Notes |
|-----------|-----------|-------|
| Solana monitoring | `solana-sdk` + `solana-client` | Subscribe to pump.fun program events |
| Token analysis | Custom contract bytecode parser | Detect honeypots, tax tokens |
| Transaction submission | `jito-sdk-rust` (Solana) | Priority bundle submission |
| Ethereum monitoring | `alloy` + WebSocket subscription | PairCreated event logs |

### Competition Analysis

- **Extremely competitive on popular chains.** Hundreds of sniping bots compete for the same launches.
- On pump.fun specifically, the "bonding curve" mechanism means early buyers always get better prices, so it's a pure speed race.
- MEV bots can frontrun your snipe on Ethereum.
- On Solana, Jito bundles give priority but many bots use Jito.

### Solo Dev Edge

❌ **No sustainable edge.** Sniping is a pure speed + luck game:
- You're competing against dedicated sniping services (BonkBot, Trojan, Maestro)
- Even if you buy early, 90%+ of memecoins go to zero
- The occasional 10x doesn't compensate for the 9 losses of 80-100%
- It's essentially gambling with a technical wrapper

### Difficulty Rating: 3/5

The code isn't the hard part. The EDGE is the hard part. You can build a sniper in 80 hours, but making it profitable requires either:
- Better token selection (hard to automate, essentially prediction)
- Faster execution (limited by physics and competition)
- Better risk management (the only genuine edge)

### Key Risks

1. **Rug pulls**: 80-90% of new memecoins are rug pulls (creator drains liquidity).
2. **Honeypot tokens**: Contract prevents selling — you buy but can never sell.
3. **Total loss**: Each position can go to zero. Expected value of a random memecoin snipe is negative.
4. **Regulatory risk**: Sniping may be seen as market manipulation in some jurisdictions.
5. **Emotional/psychological**: The dopamine of occasional 10x hits can encourage over-trading.

### Preliminary Viability

| Aspect | Assessment |
|--------|-----------|
| Viability | ❌ Not viable as a reliable income source |
| Expected monthly return | -€500 to €2000 (extreme variance) |
| Expected value | Negative (most positions lose) |
| Capital needed | €1-5K (small positions, expect losses) |
| Dev time | 80-120h |
| Risk of total loss | High (per-position: ~80-90%) |

**Verdict**: **TIER 4 — AVOID as a primary strategy.** This is gambling, not arbitrage. There's no structural edge. The occasional win masks a negative expected value. If you MUST build this, allocate <5% of capital and treat it as entertainment, not income.

---

## Strategy 11: Triangular Arbitrage within a Single DEX (BONUS)

### Mechanism

Find price inconsistencies within a single venue by routing through three tokens:

1. Start with USDC
2. Buy ETH with USDC (USDC → ETH)
3. Buy WBTC with ETH (ETH → WBTC)
4. Sell WBTC for USDC (WBTC → USDC)
5. If you end up with more USDC than you started: profit

**On DEXs, this can be done atomically with flash loans** — zero capital required for the arb itself.

### Architecture

```
┌────────────────────────────────────────────────┐
│ Pool State Tracker                              │
│ • Track all Uniswap v3 pool prices/reserves    │
│ • Build a directed graph of token↔token pools  │
│ • Update on every block                        │
└──────────────────┬─────────────────────────────┘
                   │
           ┌───────▼──────────┐
           │ Cycle Finder      │
           │ • Bellman-Ford or  │
           │   Floyd-Warshall   │
           │   on log-price     │
           │   graph           │
           │ • Detect negative  │
           │   cycles = profit  │
           └───────┬──────────┘
                   │ profitable cycle found
           ┌───────▼──────────┐
           │ Flash Loan TX     │
           │ Builder            │
           │ • Borrow token A   │
           │ • Swap A→B→C→A    │
           │ • Repay loan + fee│
           │ • Profit = surplus │
           └───────┬──────────┘
                   │
           ┌───────▼──────────┐
           │ Flashbots/Jito    │
           │ Bundle Submission │
           └──────────────────┘
```

### Tech Stack

| Component | Crate/Tool |
|-----------|-----------|
| Graph algorithms | `petgraph` | 
| EVM simulation | `revm` |
| Flashbots | `alloy` + Flashbots relay |
| Solana | `solana-sdk` + Jito |

### Competition Analysis

**Extremely competitive.** Triangular arb on Uniswap is one of the most well-studied MEV strategies. Hundreds of searchers run this. The opportunities that exist are captured within the same block they appear. You're competing against `revm`-based simulators that can evaluate thousands of potential cycles per block.

### Difficulty Rating: 4/5

- Graph algorithms for cycle detection are well-understood but need to be fast
- Flash loan contract deployment and optimization
- Competing in the Flashbots/Jito auction

### Preliminary Viability

| Aspect | Assessment |
|--------|-----------|
| Viability | ❌ Not viable for retail on major DEXs |
| Viability on niche DEXs/L2s | ⚠️ Marginal |
| Expected monthly return | -€100 to €100 on L2s |
| Capital needed | €2-5K (gas/tips only, flash loans provide trade capital) |
| Dev time | 120-160h |

**Verdict**: **TIER 3.** Only viable on less-efficient DEXs (L2 AMMs, Solana DEXs with fewer searchers). On Ethereum mainnet Uniswap: completely dominated by professional searchers.

---

## Strategy 12: Yield Farming Optimization Bot (BONUS)

### Mechanism

DeFi protocols offer yield (APY) for depositing assets. These yields change constantly based on supply/demand. A yield optimizer bot:

1. Monitors yield rates across dozens of protocols (Aave, Compound, Yearn, Convex, Pendle, etc.)
2. Automatically moves capital to the highest-yielding protocol
3. Handles compounding (claiming rewards, re-depositing)
4. Accounts for gas costs of moving positions

**This is NOT arbitrage. It's automated capital allocation.** But it generates consistent returns.

### Architecture

```
┌────────────────────────────────────────────────┐
│ Yield Monitor                                   │
│ • Track APY across 20+ protocols on 5+ chains  │
│ • DefiLlama API for aggregate data             │
│ • Direct on-chain reads for real-time accuracy  │
└──────────────────┬─────────────────────────────┘
                   │
           ┌───────▼──────────┐
           │ Strategy Engine   │
           │ • Compare yields  │
           │ • Subtract gas    │
           │   cost of moving  │
           │ • Min hold period │
           │   to justify move │
           │ • Risk scoring    │
           │   per protocol    │
           └───────┬──────────┘
                   │ reallocation needed
           ┌───────▼──────────┐
           │ Execution         │
           │ • Withdraw from   │
           │   current protocol│
           │ • Deposit to new  │
           │   higher-yield    │
           │   protocol        │
           │ • Claim/compound  │
           │   rewards         │
           └──────────────────┘
```

### Realistic Returns

- **Blue-chip DeFi yields (2026)**: 3-8% APY on stablecoins, 2-5% on ETH
- **Optimization alpha**: Moving capital to the best opportunity adds 1-3% over passive holding
- **Net benefit**: 4-11% APY vs. 3-8% passive = 1-3% additional annual return
- **On €30K**: €300-900/year additional returns from optimization = €25-75/month

### Difficulty Rating: 2/5

### Key Risks

- Smart contract risk (protocol hacks)
- Gas costs eroding small optimization gains
- IL if yield involves LP positions

### Preliminary Viability

| Aspect | Assessment |
|--------|-----------|
| Viability | ⚠️ Marginally viable (small alpha over passive) |
| Expected monthly return | €25-75 additional over passive holding |
| Capital needed | €20K+ |
| Dev time | 80-120h |

**Verdict**: **TIER 2 — NICE TO HAVE, NOT STANDALONE.** The optimization alpha is real but small. Best used as an infrastructure layer for capital that's waiting to be deployed in other strategies (e.g., idle funds between Funding Rate Arb positions).

---

## Strategy 13: Options Arbitrage (Deribit + On-Chain) (BONUS)

### Mechanism

Crypto options trade on centralized venues (Deribit, dominant with >90% market share) and on-chain protocols (Lyra, Premia, Aevo). Arbitrage opportunities include:

1. **Put-call parity violations**: C - P ≠ S - K×e^(-rT). When this equation doesn't hold, you can construct a risk-free portfolio.
2. **Cross-venue pricing**: Same option priced differently on Deribit vs. Lyra.
3. **Volatility surface arbitrage**: Implied volatility inconsistencies across strikes/expirations.

### Competition Analysis

- Deribit is dominated by institutional market makers (Paradigm, Genesis)
- On-chain options have very low liquidity (<$10M daily volume)
- Options math is complex (Greeks, vol surfaces, decay)
- The market is too small and specialized for most retail traders

### Difficulty Rating: 5/5

Requires:
- Deep options theory knowledge
- Volatility surface modeling
- Greeks hedging
- Multi-leg position management
- Both CEX and DEX integration

### Preliminary Viability

| Aspect | Assessment |
|--------|-----------|
| Viability | ❌ Not viable for retail |
| Expected monthly return | Unknown (too specialized) |
| Capital needed | €50K+ (margin requirements) |
| Dev time | 250-350h |

**Verdict**: **TIER 4 — AVOID.** Too complex, too specialized, too capital-intensive, too dominated by institutional players. If you don't already trade options professionally, don't start by building an arb bot.

---

## Strategy 14: Oracle Front-Running / Post-Update Arbitrage (BONUS)

### Mechanism

Chainlink and other oracle networks update on-chain prices at regular intervals or when the price deviates by a threshold (typically 0.5-1%). Between oracle updates, on-chain lending protocols use stale prices. When an oracle update is pending:

1. Monitor the Chainlink mempool for pending `transmit()` transactions
2. The new price will make some lending positions undercollateralized
3. Submit a backrun transaction that liquidates those positions at the new (updated) price
4. Or: arb the stale DEX pool price against the new oracle price

**This overlaps with Strategy 2 (Liquidation) and Strategy 3 (MEV Backrunning).** The specific edge is knowing the oracle update BEFORE it's on-chain.

### Competition Analysis

This is the domain of dedicated MEV searchers. Monitoring Chainlink's OCR (Off-Chain Reporting) network for pending updates is well-understood and heavily competed.

### Preliminary Viability

| Aspect | Assessment |
|--------|-----------|
| Viability | ❌ Not viable standalone (absorbed by MEV/liquidation bots) |

**Verdict**: **NOT A SEPARATE STRATEGY — fold into Strategies 2/3.** If you build a liquidation or MEV bot, oracle monitoring is a subcomponent, not a standalone system.

---

## Consolidated Ranking

### Viability Matrix

| # | Strategy | Difficulty | Monthly Return (€30K) | Competition | Dev Time | Capital Fit | **TIER** |
|---|----------|-----------|----------------------|-------------|----------|------------|----------|
| 4 | **Funding Rate Arb** ⭐ | 2/5 | €150-400 | Low | 80-120h | ✅ Perfect | **1** |
| 6 | **Long-Tail Altcoin Arb** ⭐ | 3/5 | €100-400 | Low-Med | 150-200h | ✅ Good | **1** |
| 1 | Solana DEX-CEX Arb | 4/5 | €100-500 | Med-High | 120-160h | ⚠️ OK | **2** |
| 7 | Stablecoin Depeg Arb | 2/5 | €0-5000 (sporadic) | Med | 30-50h | ✅ Good | **2** |
| 8 | Market Making (Stable Pairs) | 3/5 | €50-300 | Low-Med | 120-160h | ⚠️ OK | **2** |
| 12 | Yield Optimization | 2/5 | €25-75 | Low | 80-120h | ✅ Good | **2** |
| 2 | Liquidation Bots (L2/Solana) | 4/5 | €50-300 (volatile) | Med | 120-160h | ✅ Good | **3** |
| 3 | MEV Backrunning | 5/5 | €0-400 | High | 150-200h | ⚠️ OK | **3** |
| 11 | Triangular Arb (niche DEXs) | 4/5 | -€100-100 | High | 120-160h | ✅ Good | **3** |
| 5 | Cross-Chain Bridge Arb | 4/5 | -€100-100 | High | 160-220h | ❌ Poor | **4** |
| 9 | NFT Arbitrage | 3/5 | -€200-200 | Med | 100-140h | ❌ Poor | **4** |
| 10 | Memecoin Sniping | 3/5 | -€500-2000 | Extreme | 80-120h | ❌ Poor (gambling) | **4** |
| 13 | Options Arbitrage | 5/5 | Unknown | Extreme | 250-350h | ❌ Poor | **4** |
| 14 | Oracle Front-Running | 4/5 | N/A (subsystem) | Extreme | N/A | N/A | N/A |

---

## Recommended Strategy Portfolio

### Primary: Funding Rate Arbitrage (Strategy 4)

**Why first**:
- Lowest development complexity (2/5)
- Fastest to market (80-120h, MVP in 40-60h)
- Only strategy where speed is IRRELEVANT
- Consistent returns (not event-dependent)
- Risk is well-understood and manageable (delta-neutral)
- Uses CEX infrastructure that transfers to Strategy 6

**Capital allocation**: €20-25K

### Secondary: Long-Tail Altcoin Pair Arbitrage (Strategy 6)

**Why second**:
- Shares CEX exchange connector infrastructure with Strategy 4
- Genuine retail edge in the long tail
- Higher return potential than funding rate (but higher risk)
- Incremental: add exchanges and pairs over time

**Capital allocation**: €10-15K (remaining after Strategy 4)

### Supplementary: Stablecoin Depeg Monitor (Strategy 7)

**Why also**:
- Minimal dev effort (30-50h, mostly monitoring + alerts)
- Zero capital allocation when idle
- Uses capital from Strategy 4 (close funding positions → buy depegged stablecoins)
- Potential for outsized returns during rare events

**Capital allocation**: €0 (borrows from Strategy 4 during events)

### Infrastructure Overlap

```
Shared Components:
├── Exchange Connectors (Binance, Bybit, KuCoin, OKX)
│   ├── REST API client (order placement, account info)
│   ├── WebSocket client (price feeds, order book)
│   └── Rate limiter (per-exchange, per-endpoint)
├── Risk Management
│   ├── Circuit breaker
│   ├── Daily P&L tracking
│   └── Position size limits
├── Persistence (SQLite → Postgres later)
│   ├── Trade records
│   ├── P&L history
│   └── Funding rate history
├── Monitoring
│   ├── Axum health/status API
│   └── Telegram alerts
└── Config Management
    └── TOML + env vars

Strategy-Specific:
├── Strategy 4 (Funding Rate)
│   ├── Funding rate poller
│   ├── Position manager (spot + perp)
│   └── Margin monitor
├── Strategy 6 (Altcoin Arb)
│   ├── Multi-exchange order book aggregator
│   ├── Cross-exchange opportunity detector
│   └── Inventory tracker
└── Strategy 7 (Depeg Monitor)
    ├── Stablecoin price tracker (DEX + CEX)
    └── Alert system
```

**Total development time for all three**:
- Shared infrastructure: 60-80h
- Strategy 4 (Funding Rate): 40-60h additional
- Strategy 6 (Altcoin Arb): 80-100h additional
- Strategy 7 (Depeg Monitor): 20-30h additional
- **Total: 200-270h** for a three-strategy portfolio

### Projected Monthly Returns (€30K Capital)

| Scenario | Funding Rate | Altcoin Arb | Depeg (amortized) | **Total** |
|----------|-------------|------------|-------------------|-----------|
| Pessimistic | €80 | €50 | €0 | **€130** |
| Realistic | €200 | €200 | €40 | **€440** |
| Optimistic | €400 | €500 | €100 | **€1,000** |

**Monthly costs**: €20-40 (VPS) — NO maintenance cost deducted because this is your own bot for your own capital.

**Break-even on dev time** (at €100/h opportunity cost):
- 200-270h × €100 = €20,000-27,000 development investment
- At realistic €440/month net: **45-61 months (3.8-5.1 years)**
- At optimistic €1,000/month net: **20-27 months (1.7-2.3 years)**

**Honest assessment of break-even**: This is still a long payback period. The TRUE value proposition is:
1. The system runs 24/7 without your active involvement after setup
2. Returns compound as you add capital from other income
3. The skills are transferable (Rust, async, financial systems)
4. Unlike freelancing at €100/h, the bot earns while you sleep

---

## Technical Architecture for the Recommended Portfolio

### Workspace Structure

```
crypto-trading/
├── Cargo.toml (workspace)
├── crates/
│   ├── common-types/        # Shared domain types
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── decimal.rs   # Money types (rust_decimal)
│   │   │   ├── exchange.rs  # ExchangeId, TradingPair
│   │   │   ├── order.rs     # OrderRequest, OrderResponse
│   │   │   └── config.rs    # Shared config types
│   │   └── Cargo.toml
│   │
│   ├── exchange/            # Exchange connector trait + implementations
│   │   ├── src/
│   │   │   ├── lib.rs       # ExchangeConnector trait
│   │   │   ├── binance/     # Binance spot + futures
│   │   │   ├── bybit/       # Bybit spot + perps
│   │   │   ├── kucoin/      # KuCoin spot
│   │   │   ├── okx/         # OKX spot + perps
│   │   │   └── rate_limit.rs
│   │   └── Cargo.toml
│   │
│   ├── funding-arb/         # Strategy 4: Funding Rate Arbitrage
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── rate_monitor.rs  # Funding rate polling + history
│   │   │   ├── position.rs      # Position management (spot + perp)
│   │   │   ├── margin.rs        # Margin monitoring + alerts
│   │   │   └── strategy.rs      # Entry/exit logic
│   │   └── Cargo.toml
│   │
│   ├── altcoin-arb/         # Strategy 6: Long-Tail Altcoin Arb
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── scanner.rs       # Multi-exchange opportunity scanner
│   │   │   ├── executor.rs      # Cross-exchange execution
│   │   │   └── inventory.rs     # Balance tracking per exchange
│   │   └── Cargo.toml
│   │
│   ├── depeg-monitor/       # Strategy 7: Stablecoin Depeg Monitor
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── monitor.rs       # Price monitoring
│   │   │   └── alert.rs         # Alert on depeg events
│   │   └── Cargo.toml
│   │
│   ├── risk/                # Shared risk management
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── circuit_breaker.rs
│   │   │   ├── pnl_tracker.rs
│   │   │   └── limits.rs
│   │   └── Cargo.toml
│   │
│   ├── db/                  # Persistence layer
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   └── repo.rs
│   │   ├── migrations/
│   │   └── Cargo.toml
│   │
│   └── server/              # API + monitoring
│       ├── src/
│       │   ├── lib.rs
│       │   ├── api.rs           # Axum REST endpoints
│       │   └── telegram.rs      # Telegram bot alerts
│       └── Cargo.toml
│
├── src/
│   └── main.rs              # Binary entry point
├── config/
│   └── default.toml
└── .env.example
```

### Key Dependencies (Cargo.toml)

```toml
[workspace.dependencies]
tokio = { version = "1", features = ["full"] }
reqwest = { version = "0.12", features = ["json", "rustls-tls"] }
tokio-tungstenite = { version = "0.24", features = ["rustls-tls-webpki-roots"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
rust_decimal = { version = "1.36", features = ["serde-with-str"] }
sqlx = { version = "0.8", features = ["runtime-tokio", "sqlite", "decimal"] }
axum = "0.8"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["json", "env-filter"] }
teloxide = { version = "0.14", features = ["macros"] }
config = "0.14"
dotenvy = "0.15"
governor = "0.7"  # Rate limiting
hmac = "0.12"
sha2 = "0.10"
chrono = { version = "0.4", features = ["serde"] }
uuid = { version = "1", features = ["v7", "serde"] }
thiserror = "2"
anyhow = "1"
```

### Implementation Roadmap

| Phase | Tasks | Hours | Milestone |
|-------|-------|-------|-----------|
| **1. Foundation** | Common types, config loading, SQLite setup, risk skeleton | 30-40h | Compiles, reads config, connects to DB |
| **2. Exchange Connectors** | Binance (spot+futures), Bybit connectors (REST+WS) | 40-60h | Can read order books and place test orders |
| **3. Funding Rate MVP** | Rate monitor, position manager, margin tracker | 30-40h | Can open/close funding rate positions on testnet |
| **4. Paper Trading** | Simulated execution mode, P&L tracking | 15-20h | Run for 2 weeks on real data, simulated trades |
| **GO/NO-GO GATE** | Review paper trading results. Profitable? Continue. Not? Stop. | — | — |
| **5. Live Funding Rate** | Real execution, Telegram alerts, monitoring API | 15-20h | Live trading with €5K test capital |
| **6. Altcoin Arb** | Scanner, cross-exchange executor, inventory tracker | 50-60h | Scanning for opportunities, paper trading |
| **7. Additional Exchanges** | KuCoin, OKX, MEXC connectors | 30-40h | Wider market coverage for altcoin arb |
| **8. Depeg Monitor** | Stablecoin price tracker, alert system | 20-30h | Running 24/7, alerts on Telegram |
| **Total** | | **230-310h** | |

### Key Architecture Decisions

**ADR-001: SQLite for MVP, PostgreSQL optional later**
- Rationale: The write volume (10-50 trades/day) is trivial for SQLite. Eliminates Docker/infra dependency.
- Trade-off: No JSONB, no concurrent write scaling. Acceptable for solo operation.

**ADR-002: Polling over WebSocket for funding rate strategy**
- Rationale: Funding rates settle every 8 hours. Polling every 5 minutes is sufficient. Reduces WebSocket connection complexity.
- Trade-off: Slightly higher latency for opportunity detection. Irrelevant for this strategy.

**ADR-003: Exchange connector trait with async_trait**
- Rationale: Common interface for all strategies. Each exchange implements the trait. Strategies are exchange-agnostic.
- Trade-off: async_trait has overhead (heap allocation). Irrelevant at this polling frequency.

**ADR-004: Telegram for alerts, not email/webhook**
- Rationale: Telegram is instant, has a good Rust library (teloxide), and is mobile-friendly. The user can respond to alerts immediately.
- Trade-off: Dependency on Telegram. Acceptable for solo operation.

**ADR-005: Paper trading mode is mandatory before live**
- Rationale: The previous CEX-CEX arb analysis showed the market thesis was wrong. Paper trading for 2-4 weeks validates the thesis before risking capital.
- Trade-off: Delayed revenue start. Worth it to avoid another -€800/month situation.

---

## Risk Mitigation Plan

### Capital Safety

| Risk | Mitigation | Implementation |
|------|-----------|---------------|
| Exchange collapse | Max 40% of capital per exchange | Config limit, enforced by risk crate |
| Position goes wrong | Daily loss limit: 2% of capital (€600 on €30K) | Circuit breaker auto-trip |
| Margin call | Over-collateralize perp positions (2x, not 5x) | Margin monitor + alert at 80% utilization |
| API key compromise | Read-only keys for monitoring; trade keys in env vars only | No keys in config files or DB |
| Bot malfunction | Paper trade for 2+ weeks before live | Mandatory GO/NO-GO gate |

### Operational Safety

| Risk | Mitigation |
|------|-----------|
| Bot crashes | Systemd/Docker restart policy; startup reconciliation |
| Exchange maintenance | Detect 503, pause affected exchange, continue others |
| Rate limit hit | Per-endpoint rate limiter with exponential backoff |
| Network outage | Circuit breaker trips after 3 consecutive failures |
| Capital imbalance | Weekly manual rebalancing; alert when exchange balance <€2K |

---

## Final Honest Assessment

### What I'm Confident About

1. **Funding Rate Arb is the best fit** for this profile. No speed competition, delta-neutral, consistent returns, manageable complexity.
2. **Long-Tail Altcoin Arb has genuine retail edge** — HFT firms don't operate in these niches.
3. **The combined infrastructure approach** (shared exchange connectors) saves 30-40% of development time vs. building each strategy independently.

### What I'm Less Sure About

1. **Monthly return estimates are educated guesses.** Real-world returns depend on market conditions. Funding rates were higher in 2024-2025 than they might be in stable 2026 markets.
2. **Exchange connector development time** is notoriously hard to estimate. API quirks, undocumented behaviors, and silent changes can double the estimated time.
3. **The altcoin arb profitability** depends on finding enough illiquid pairs with sufficient spread. This needs validation through paper trading.

### What Will Definitely Go Wrong

1. **At least one exchange API will change mid-development.** Budget 20-30h for emergency fixes.
2. **Paper trading results will differ from live results.** Slippage, partial fills, and race conditions only appear with real money.
3. **Funding rates will have a period of being consistently negative.** The bot must handle this gracefully (close positions, wait, re-enter).
4. **Rebalancing between exchanges will be more annoying than expected.** Crypto withdrawals have variable confirmation times, and fiat takes days.

### The Uncomfortable Truth

Even the BEST strategy (Funding Rate Arb) generates modest absolute returns at this capital level. €200-400/month on €30K is 8-16% annualized — good, but it won't replace a developer salary. The real value is:

1. **It compounds**: €400/month reinvested grows the capital base
2. **It's passive**: After initial development, 2-4h/week of maintenance
3. **It scales**: Same bot works with €100K or €300K (proportionally higher returns)
4. **It's educational**: Production Rust, async systems, financial engineering
5. **It's a foundation**: Exchange connectors and infrastructure enable adding new strategies over time

The honest path to making this genuinely worthwhile:
- Build the Funding Rate bot (€200-400/mo on €30K)
- Reinvest profits + add capital from other income over 12-24 months
- Reach €100K deployed capital → €700-1,400/mo
- Add altcoin arb for additional €300-800/mo
- Total at €100K: €1,000-2,200/mo — now it's meaningful supplemental income

**This is a 2-year journey, not a 2-month one.** Anyone promising faster returns is selling something.

---

*End of analysis. All assessments are honest and based on 2026 market conditions, realistic competition analysis, and the specific constraints of a solo developer with €10-50K capital.*
