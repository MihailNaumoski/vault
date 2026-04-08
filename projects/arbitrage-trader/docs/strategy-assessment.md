# Arbitrage Trader — Strategy Assessment

**Date**: 2026-04-08
**Status**: Pre-live analysis after fixing price ingestion bugs

## Business Model

Buy YES on Polymarket + buy NO on Kalshi (or vice versa) when `yes_price + no_price < $1.00`. Guaranteed $1.00 at settlement regardless of outcome. Profit = `$1.00 - cost_of_both_sides`.

**Example**: If Poly YES = $0.45 and Kalshi NO = $0.50, you pay $0.95 total and receive $1.00 at settlement = $0.05 guaranteed profit per contract.

## Current Reality (from data forensics)

| Metric | Value |
|--------|-------|
| Real cross-platform spreads | -10.5 to +1.3 cents |
| Kalshi taker fee | 7% of profit |
| Polymarket fee | ~0% currently |
| Best observed spread | +1.3 cents |
| Fee on 1.3-cent spread | ~0.7 cents |
| Net profit after fees | ~0.6 cents per contract |
| Contracts needed for $1 profit | ~167 contracts |

**Finding**: Zero profitable arbitrage opportunities existed in the data. All reported profit ($49.30) was phantom — caused by zero-default prices in the Kalshi WebSocket feed.

## Why It's Hard

1. **Markets are already efficient** — Professional market makers bridge Kalshi and Polymarket in milliseconds
2. **Fees eat the edge** — Kalshi's 7% taker fee destroys thin spreads
3. **Latency disadvantage** — System polls every 1s; HFT bots operate in milliseconds
4. **Low liquidity** — Presidential 2028 and Hormuz pairs have thin books, wide bid-ask spreads
5. **Capital lockup** — Money locked until settlement (could be months), terrible capital efficiency

## When Prediction Market Arb Works

Profitable in specific conditions:

- **New market launches** — When a market first opens on one platform but already exists on another, prices can diverge for minutes/hours
- **Breaking news events** — One platform reprices faster than the other (information asymmetry window)
- **High-volume, short-duration markets** — Weekly/daily binary events with enough volume for meaningful fills
- **Maker fees** — Posting limit orders (maker) instead of taking (taker), fees are lower or zero
- **Cross-exchange settlement timing** — Some events settle on different schedules

## Potential Strategy Pivots

### 1. Maker Order Strategy
- Post limit orders on both sides instead of taking
- Kalshi maker fee: 0% (vs 7% taker)
- Polymarket maker fee: 0%
- Requires orderbook management, fill monitoring, and cancel-replace logic
- Risk: orders may not fill, or fill asymmetrically (one side fills, other doesn't)

### 2. News-Driven Scanning
- Monitor breaking news feeds (RSS, Twitter/X, news APIs)
- Detect events that will move prediction market prices
- Race to arbitrage the price dislocation before market makers reprice
- Requires: NLP/sentiment analysis, low-latency news ingestion, fast execution

### 3. New Market Launch Sniping
- Monitor both platforms for new market listings
- When a market appears on one platform that already exists on the other, check for price dislocations
- Time window: minutes to hours after listing
- Requires: continuous market scanning, Gamma API pagination, fast pair matching

### 4. Multi-Platform Expansion
- Add more prediction market platforms (Manifold, PredictIt, Metaculus)
- More platforms = more price dislocation opportunities
- Different fee structures may offer better edges
- Requires: new connector implementations per platform

### 5. Statistical Arbitrage (Not Pure Arb)
- Instead of guaranteed profit, trade correlated but not identical markets
- Example: "Will Democrats win 2028?" on Poly vs individual candidate markets on Kalshi
- Higher risk, higher potential reward
- Requires: correlation modeling, risk management for directional exposure

## Infrastructure Status (Post-Fix)

| Component | Status |
|-----------|--------|
| Price ingestion (WS + REST) | Fixed — consistent `1 - yes_ask` semantics |
| Zero-price guard | Active — detector rejects zero prices |
| Orderbook path | Fixed — consistent with ticker path |
| Fee modeling | Active — 7% Kalshi, 0% Poly, fee-adjusted thresholds |
| Mode tracking | Active — paper/demo/production tagged in DB |
| Dead ticker detection | Active — 30s warning for silent tickers |
| TUI display | Fixed — shows market names for all orders |
| Paper trading | Functional — simulates fills with real price data |

## Recommendation

The current taker-based binary arb strategy on established markets is **not viable** with current fee structures and market efficiency. To proceed profitably, either:

1. **Pivot to maker orders** (zero fees, but requires fill management)
2. **Focus on transient dislocations** (news events, new listings)
3. **Expand to more platforms** (more inefficiency to exploit)

The infrastructure is solid — the execution pipeline, risk management, and monitoring work correctly now. The bottleneck is finding actual edge, not engineering.
