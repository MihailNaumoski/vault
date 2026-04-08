# Phantom Profit Forensics Report

**Date**: 2026-04-08
**Analyst**: Data Analyst (Trading Team)
**Status**: Complete

## Executive Summary

**100% of the $49.30 reported profit is fictitious**, caused by phantom spreads from zero-default Kalshi prices. No legitimate arbitrage opportunities existed in the dataset.

## Findings

### 1. All Orders Are Paper Trades

All 8 orders in the database use `paper-` prefixed IDs. No real money was at risk.

| Prefix     | Count |
|-----------|-------|
| `paper-`  | 8     |
| Real      | 0     |

### 2. 100% of Opportunities Have Zero Kalshi Price

| Total Opportunities | kalshi_price = 0 | kalshi_price > 0 | % Phantom |
|--------------------|-----------------|-----------------|-----------|
| 4                  | 4               | 0               | **100%**  |

Every single opportunity was a phantom. There are **zero** legitimate arbitrage opportunities in the database.

### 3. Phantom Spread Analysis

When `kalshi_price = 0`, the spread formula `1 - poly_price - kalshi_price` produces:

| Opportunity | poly_price | kalshi_price | Phantom Spread | Spread % |
|-------------|-----------|-------------|---------------|----------|
| ...686b6    | 0.163     | 0           | 0.837         | 513%     |
| ...8a5c     | 0.405     | 0           | 0.595         | 147%     |
| ...d305     | 0.016     | 0           | 0.984         | 6,150%   |
| ...b0a0     | 0.014     | 0           | 0.986         | 7,043%   |

Average phantom spread: **0.8505** (85 cents). Real spreads in prediction markets are typically 1-5 cents.

### 4. Fictitious Profit Calculation

The reported daily PnL:
- **Date**: 2026-04-08
- **Trades Executed**: 8
- **Trades Filled**: 5
- **Gross Profit**: $49.30
- **Net Profit**: $49.30 (no fees modeled)
- **Capital Deployed**: $1.70

Filled orders breakdown:

| Order ID | Platform | Side | Price | Qty | Status |
|----------|----------|------|-------|-----|--------|
| paper-kalshi-1 | kalshi | no | $0.01 | 50 | filled |
| paper-polymarket-2 | polymarket | yes | $0.415 | 50 | filled |
| paper-kalshi-3 | kalshi | no | $0.01 | 50 | filled |
| paper-polymarket-5 | polymarket | yes | $0.024 | 50 | filled |
| paper-kalshi-4 | kalshi | no | $0.01 | 50 | filled |

The paper executor fills Kalshi orders at $0.01 (minimum tick), which is the mock fill price, not a real market price. The "profit" comes from: `spread * quantity` where spread is 60-98 cents from the phantom zero prices.

### 5. Price Snapshots Show Real Kalshi Prices Exist

The 23 price snapshots (captured post-detection, during monitoring) show **non-zero** Kalshi prices:

| Market | Poly Yes | Kalshi Yes | Spread |
|--------|----------|-----------|--------|
| Market A | 0.40-0.43 | 0.46-0.50 | -0.035 to -0.105 |
| Market B | 0.163 | 0.15 | +0.013 |
| Market C | 0.016 | 0.01 | +0.006 |

Key finding: The **real** spreads are -10.5 to +1.3 cents. Most are **negative** (no opportunity). The tiny positive spreads (0.6-1.3 cents) are well below transaction costs and would not be profitable.

### 6. Root Cause Chain

1. Kalshi WebSocket connection drops or ticker message arrives with `yes_ask_dollars = None`
2. `ws_message_to_price_update()` calls `.unwrap_or_default()` which sets price to `0`
3. Price cache stores `kalshi_no = 0`
4. Detector computes `spread = 1 - poly_yes - 0 = ~0.98` (phantom 98-cent spread)
5. Risk manager and executor see a "huge opportunity" and execute
6. Paper executor fills at $0.01 mock price
7. PnL calculation records the phantom spread as real profit

## Conclusion

- **Total fictitious profit**: $49.30 (100% of reported)
- **Legitimate profit**: $0.00
- **Legitimate opportunities**: 0 of 4
- Real spreads in the data are -10 to +1 cent, not actionable after fees
- The three bugs (zero-default in WS, bid/ask confusion in REST, no zero guard in detector) created a perfect storm where every detected "opportunity" was a data error
