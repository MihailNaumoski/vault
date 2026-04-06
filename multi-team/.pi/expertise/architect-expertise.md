# Architect Expertise

*This file is maintained by the architect agent. Do not edit manually.*

## Session: 2026-04-05 — Crypto Arbitrage Architecture Analysis

### Patterns Observed
- **Rust workspace decomposition**: 7-crate pattern (types → connectors → engine → risk → db → server → cli) is a solid template for any multi-venue trading system. Types crate with zero deps as foundation is correct.
- **Trait-based connector abstraction**: `ExchangeConnector` trait with `Send + Sync + 'static` bounds is the right pattern for multi-venue systems in async Rust. Take `mpsc::Sender` for push-based data flow.
- **Risk as a separate crate**: Safety-critical code (risk management, circuit breakers) must be independently testable. Never bundle with execution logic.
- **Channel topology for trading systems**: mpsc for hot-path data flow (books → engine), broadcast for fan-out (events → UI), mpsc for fire-and-forget persistence (trades → DB writer).

### Risks & Anti-Patterns
- **Static fee assumption**: Real exchanges have tiered fees based on 30-day volume. Any trading system spec that treats fees as constant is fundamentally flawed.
- **No rebalancing mechanism**: Cross-exchange arb creates directional capital flow. Without rebalancing, the system self-limits within hours/days.
- **Sequential engine loop**: Processing detection and execution in the same loop blocks new opportunities during trade execution. Spawn execution into separate tasks.
- **Latency claims without co-location**: <300ms p95 execution is unrealistic for REST-based order placement to exchanges 80-150ms away. Be honest about network physics.

### Market Viability Insights (2026)
- **Crypto spot arb on major venues is dead for retail**: HFT firms with co-location dominate BTC/USDT, ETH/USDT. Spreads < fees at retail tier.
- **Prediction market arb is marginally viable**: Wider spreads, less competition, but capital lockup kills annualized returns. Market matching (NLP) is the hard problem.
- **Where alpha exists**: DEX-CEX arb, exotic altcoin pairs, triangular arb within single exchanges, MEV-aware strategies.

### Mistakes to Avoid
- Don't conflate detection (pure computation) and execution (side-effectful I/O) in the same crate. Split them.
- Don't drop critical writes (executed trades) under backpressure. Only drop informational events.
- Don't forget post-restart reconciliation — the connector trait needs `get_open_orders()` for crash recovery.

## Session: 2026-04-05 — Profitable Strategy Analysis (14 strategies evaluated)

### Strategy Viability Framework
- **Speed sensitivity is the #1 filter**: If a strategy requires sub-second execution to be profitable, retail CANNOT compete. Period.
- **Funding Rate Arb is the rare exception**: It's the only crypto trading strategy where speed is completely irrelevant (8h funding periods). Competition is about capital allocation, not latency.
- **Long-tail markets are where retail wins**: HFT firms need $1M+ monthly profit per strategy to justify infrastructure costs. Markets generating $500-5000/mo are invisible to institutions but perfect for solo devs.
- **Flash loan strategies (triangular arb, liquidation) are attractive on paper but dominated on practice**: The zero-capital-required framing masks the extreme competition from professional MEV searchers.

### Chain-Specific MEV Landscape (2026)
- **Ethereum MEV**: Professionalized, mature, ~60% of blocks via MEV-Boost. Retail cannot compete on common strategies.
- **Solana MEV**: Less mature, Jito has ~50-70% validator adoption. Better opportunity for retail on less popular pairs. Rust-native ecosystem is an advantage for Rust developers.
- **L2 MEV (Arbitrum, Base)**: Growing but less competitive than mainnet. Good entry point for liquidation bots and simple arb.

### Architecture Patterns for Multi-Strategy Bots
- **Shared exchange connector infrastructure saves 30-40% dev time** when running multiple strategies on the same exchanges.
- **Strategy-as-crate pattern**: Each strategy in its own crate, sharing common types and exchange connectors. Clean separation of concerns.
- **Polling > WebSocket for slow strategies**: Funding rate arb needs 5-minute polling, not real-time feeds. Over-engineering the data pipeline wastes time.
- **Paper trading is NON-NEGOTIABLE**: Previous CEX-CEX analysis proved the market thesis was wrong. Always paper-trade for 2+ weeks before live capital.

### Key Insight: Revenue vs. Speed Spectrum
```
High Speed Required                     Low Speed Required
(retail loses)                          (retail can compete)
│                                                        │
├── CEX-CEX Arb (ms)                                     │
├── Ethereum MEV (block-time, ~12s)                      │
├── DEX-CEX Arb Ethereum (seconds)                       │
├── Liquidation (seconds)                                │
├── Triangular DEX Arb (seconds)                         │
├── Solana DEX-CEX (seconds, less competitive)           │
├── Long-tail Altcoin Arb (seconds-minutes)              │
├── Market Making (minutes)                              │
├── Stablecoin Depeg (minutes-hours)                     │
├── Yield Optimization (hours-days)                      │
└── Funding Rate Arb (8 hours) ← BEST FIT FOR RETAIL    │
```

### Profitability Reality Check
- At €30K capital, even the BEST strategy generates €200-400/month (8-16% annualized).
- This is supplemental income, not a salary replacement, until capital reaches €100K+.
- Break-even on development investment: 2-5 years at realistic returns.
- The honest value proposition: passive income that compounds + scales with capital over time.

### Mistakes from This Analysis
- None yet (first pass). Will update after implementation results are available.

### Next Steps If Building
1. Start with Funding Rate Arb (lowest risk, fastest MVP: 40-60h)
2. Paper trade 2-4 weeks before any live capital
3. GO/NO-GO gate at 2-week mark — if paper returns are negative, STOP
4. Add Altcoin Arb only AFTER Funding Rate proves profitable
5. Depeg Monitor is a cheap add-on (30-50h) — build as secondary
