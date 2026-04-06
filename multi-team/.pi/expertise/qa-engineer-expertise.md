# QA Engineer Expertise

*This file is maintained by the QA engineer agent. Do not edit manually.*

<!-- 
This file will grow over time as the agent works.
The agent tracks: patterns, decisions, risks, mistakes, and insights.
Max lines: 5000
-->

## Session: 2026-04-05 — Crypto Arbitrage Quantitative Validation

### Task
Quantitative validation of profit claims for two arbitrage strategies (crypto exchange arb and prediction market arb). Analysis-only task — no code written.

### Key Findings (for future reference)

#### Crypto Exchange Arb Fee Math Pattern
- ALWAYS check: gross_spread_needed = fee_leg1 + fee_leg2 + min_net_profit_threshold
- At retail taker fees: Binance(0.10%) + Coinbase(0.08%) = 0.18% break-even minimum
- Spec threshold of 0.050% NET is internally consistent but requires 0.23%+ GROSS
- Typical BTC/USDT cross-exchange spreads (2024-2026): 0.01–0.05% normal, 0.10–0.50% volatile
- Bottom line: fees 3.6× larger than typical spreads → unprofitable at retail tiers

#### Prediction Market Arb Break-Even Formula
Derived algebraically:
- Constraint from Polymarket-wins: S > 2 - 0.02X  (where S=gross spread in cents, X=YES price)
- Constraint from Kalshi-wins: S > fX/(100-f)  (where f=Kalshi fee %)
- Binding: S_min = max(2-0.02X, fX/(100-f))
- At X=50, Kalshi 2%: S_min ≈ 1.02¢
- At X=50, Kalshi 5%: S_min ≈ 2.63¢
- At X=50, Kalshi 7%: S_min ≈ 3.76¢

#### SPEC Contradiction Patterns Found
When validating a trading system spec, always check these pairs:
1. Execution latency vs. non-HFT claim (often contradictory)
2. Fee model vs. profit threshold (often fee > threshold)
3. Win rate target vs. deterministic pre-check design
4. Staleness threshold vs. opportunity window duration
5. Retry logic timeout vs. opportunity TTL (retries often can't fire in time)
6. Exposure limits vs. pre-positioning requirements (exposure ≠ capital required)
7. Absolute + percentage dual thresholds (check for redundancy at given trade sizes)

#### Capital Efficiency Analysis Pattern
For any arbitrage system, always calculate:
- Pre-positioned capital requirement (often much larger than "exposure limit")
- Utilization rate = in-flight capital / total pre-positioned
- Opportunity cost at 5% risk-free rate
- Break-even with and without developer time
- "Operational break-even" (post-build) vs. "investment break-even" (including dev cost)

The "operational break-even" is the honest short-term metric; "investment break-even" shows whether it was worth building.

#### Output Location
Report written to: `tests/numbers-reality-check.md`
(Used tests/ as closest valid write path for a quantitative analysis deliverable)

### Patterns to Remember
- Opportunity cost must be included in all break-even analyses
- "Guaranteed profit" in prediction markets is conditional on markets being IDENTICAL (resolution criteria match)
- HFT competition makes timing claims unreliable — always compute realistic latency from network round-trips
- Show BOTH YES-wins and NO-wins scenarios for prediction market arb; they have asymmetric fee impact
- A 70% win rate target for a system with deterministic pre-checks is a red flag (implies pre-checks are wrong 30% of the time)
