# Full Trading Data Assessment

**Date:** 2026-04-08  
**Database:** `data/arb.db`  
**Analyst:** Data Analyst (Trading team)

---

## 1. Database Overview

| Table            | Record Count |
|------------------|-------------|
| market_pairs     | 1           |
| opportunities    | 0           |
| orders           | 0           |
| positions        | 0           |
| price_snapshots  | 56          |
| daily_pnl        | 1           |
| unwind_events    | 0           |

**Summary:** The database is nearly empty. There is exactly 1 active market pair (Hormuz), 56 price snapshots covering a 27.5-minute window, and 1 daily_pnl row with all zeros. No orders were ever placed, no positions opened, no opportunities recorded, and no unwinds occurred.

---

## 2. Market Pairs

Only 1 pair exists in the database:

| Field | Value |
|-------|-------|
| Polymarket | "Strait of Hormuz traffic returns to normal by end of April?" |
| Kalshi | KXHORMUZNORM-26MAR17-B260501 |
| Active | Yes |
| Verified | Yes |
| Close time | 2027-04-08 |

The 3 dead presidential tickers referenced in the bug fix context have already been removed. Only the Hormuz pair remains.

---

## 3. Order Forensics

**Total orders: 0**

There are zero orders in the database. No trades were ever placed -- not on Polymarket, not on Kalshi, not paper, not live. The order table is completely empty.

This means:
- No filled orders, no cancelled orders, no pending orders
- No platform_order_id prefixes to check (no PAPER-*, SIM-*, or real IDs)
- Zero fill rate (undefined -- no attempts)
- The engine detected no opportunities that met its threshold

---

## 4. Position Analysis

**Total positions: 0**

No positions were ever opened. There is:
- Zero guaranteed_profit
- Zero hedged_quantity
- Zero unhedged_quantity
- No open or settled positions

---

## 5. P&L Reality Check

The single `daily_pnl` row:

| Field | Value |
|-------|-------|
| Date | 2026-04-08 |
| Mode | paper |
| Trades executed | 0 |
| Trades filled | 0 |
| Gross profit | 0 |
| Fees paid | 0 |
| Net profit | 0 |
| Capital deployed | 0 |

**Verdict:** The P&L is honestly zero. No phantom profits, no fake gains. The system ran but never traded.

---

## 6. Price Snapshot Analysis

### 6.1 Collection Window

| Metric | Value |
|--------|-------|
| Total snapshots | 56 |
| Start time | 2026-04-08 14:46:39 UTC |
| End time | 2026-04-08 15:14:39 UTC |
| Duration | 27.5 minutes |
| Frequency | ~2 snapshots/minute (30-second intervals) |

### 6.2 Price Ranges

| Platform | Min | Max | Average |
|----------|-----|-----|---------|
| Polymarket YES | 0.305 | 0.335 | 0.3173 |
| Kalshi YES | 0.310 | 0.350 | 0.3247 |

Both platforms price the Hormuz event at roughly 31-35 cents (31-35% implied probability). Prices are in a reasonable, tight range. **No zero-price contamination** -- every snapshot has non-zero prices on both sides.

### 6.3 Spread Distribution

| Spread (cents) | Count | % of total |
|----------------|-------|------------|
| -4.5           | 2     | 3.6%       |
| -3.5           | 1     | 1.8%       |
| -3.0           | 1     | 1.8%       |
| -2.5           | 2     | 3.6%       |
| -2.0           | 2     | 3.6%       |
| -1.5           | 10    | 17.9%      |
| -1.0           | 4     | 7.1%       |
| -0.5           | 11    | 19.6%      |
| 0.0            | 12    | 21.4%      |
| +0.5           | 5     | 8.9%       |
| +1.0           | 4     | 7.1%       |
| +1.5           | 1     | 1.8%       |

**Spread = Kalshi YES price - Polymarket YES price** (positive means Poly is cheaper = potential arb direction)

### 6.4 Key Statistics

| Metric | Value |
|--------|-------|
| Min spread | -4.5 cents |
| Max spread | +1.5 cents |
| Average spread | -0.75 cents |
| Median spread | -0.5 cents |
| Negative spreads | 33 (58.9%) |
| Zero spreads | 12 (21.4%) |
| Positive spreads | 10 (17.9%) |
| Spreads > 1 cent | 1 (1.8%) |

The spread distribution is **heavily negative**, meaning Kalshi is generally more expensive than Polymarket for this market. Only 18% of snapshots show a positive spread (Poly cheaper than Kalshi), and the single largest positive spread was just 1.5 cents.

### 6.5 Volatility

| Metric | Polymarket | Kalshi |
|--------|-----------|--------|
| Avg price move (30s) | 0.40 cents | 0.22 cents |
| Max price move (30s) | 2.5 cents | 2.0 cents |
| Avg spread change | 0.55 cents | - |
| Max spread change | 2.5 cents | - |

Polymarket is slightly more volatile than Kalshi on 30-second timeframes.

---

## 7. Opportunity Analysis

**Total opportunities recorded: 0**

The engine never recorded a single opportunity to the database. This is consistent with the engine configuration:

- `min_spread_absolute = "0.02"` (2.0 cents minimum)
- `min_spread_pct = "3.0"` (3.0% minimum)

The maximum observed positive spread was 1.5 cents (0.015), which is **below the 2.0 cent threshold**. The engine correctly declined to trade.

No phantom opportunities exist in the database -- the zero-price bug fixes are validated by the absence of any zero-price snapshots.

---

## 8. Unwind Analysis

**Total unwind events: 0**

No positions were opened, so no unwinds were needed. Zero slippage, zero unwind losses.

---

## 9. Viability Assessment

### 9.1 The Kalshi Fee Problem

Kalshi charges a **7% fee on the profit of winning trades**. This fundamentally changes the math:

**For a buy-Poly-YES / buy-Kalshi-NO arbitrage:**

- **If YES wins:** Poly YES pays out. No Kalshi fee (Kalshi NO lost). Net profit = spread. 
- **If NO wins:** Kalshi NO pays out. Kalshi fee = 7% of Kalshi YES price. Net profit = spread - 0.07 * kalshi_yes_price.

For a true arbitrage (guaranteed profit regardless of outcome), **both scenarios must be non-negative**:

```
Required: spread >= 0.07 * kalshi_yes_price
```

### 9.2 Breakeven Analysis

| Kalshi YES Price | Minimum Spread Needed | Best Observed Spread | Gap |
|------------------|-----------------------|---------------------|-----|
| 0.31 | 2.17 cents | 1.0 cents | -1.17 cents short |
| 0.32 | 2.24 cents | 1.5 cents | -0.74 cents short |
| 0.35 | 2.45 cents | N/A | N/A |
| 0.50 (hypothetical) | 3.50 cents | N/A | N/A |

**Number of snapshots with spread above breakeven threshold: 0 out of 56 (0%)**

Every single positive-spread snapshot is unprofitable after Kalshi fees.

### 9.3 Concrete Example: Best Observed Opportunity

The single best spread observed (1.5 cents at 14:54:09 UTC):

| Parameter | Value |
|-----------|-------|
| Poly YES price | 0.335 |
| Kalshi YES price | 0.320 |
| Raw spread | 1.5 cents |
| Default quantity | 50 contracts |
| Capital deployed | $50.75 |
| Gross profit (both outcomes) | $0.75 |
| Kalshi fee if NO wins | $1.12 |
| **Net if NO wins** | **-$0.37 (LOSS)** |
| Net if YES wins | +$0.75 (profit) |
| Expected value (50/50) | +$0.19 |

Even the best moment in the data would produce a **loss** in one outcome. This is not an arbitrage -- it is a directional bet with a slight positive expected value, contingent on a 50/50 outcome assumption.

### 9.4 Expected Value Across All Positive Spreads

| Metric | Per Contract |
|--------|-------------|
| Average EV (50/50 assumption) | -$0.0029 |
| Min EV | -$0.0059 |
| Max EV | +$0.0038 |

The average expected value across positive-spread snapshots is **negative** (-0.29 cents per contract). Only 1 out of 10 positive-spread snapshots has a marginally positive EV, and only under a naive 50/50 outcome assumption.

### 9.5 Capital Efficiency

For positive-spread snapshots:

| Metric | Value |
|--------|-------|
| Average capital per contract | $1.008 |
| Average raw spread | 0.8 cents |
| Average net profit (worst case) | -1.38 cents |
| Return on capital (worst case) | -1.37% |
| Return on capital (best case) | +0.79% |

### 9.6 What Would Make This Viable?

For this pair at current price levels (~0.32), the system needs spreads of at least **2.24 cents** to guarantee a profit. The data shows the maximum spread was 1.5 cents, and spreads above 1 cent occurred only once in 56 snapshots.

To be viable, one or more of these would need to change:
1. **Larger spreads** -- need ~50% larger than the maximum observed
2. **Lower Kalshi fees** -- at 3% instead of 7%, breakeven drops to ~1 cent
3. **Higher-probability markets** (kalshi_yes > 0.70) -- fee eats less of the spread in absolute terms... but spread is also typically tighter
4. **Asymmetric strategy** -- only trade when the profitable outcome is more likely (not a true arb)

---

## 10. Conclusions

### What the data actually shows:

1. **The system works correctly.** Price ingestion is clean (no zero prices), the engine correctly applies thresholds, and no phantom trades were generated. The bug fixes are validated.

2. **Zero trading activity.** Not a single order was placed, position opened, or opportunity recorded. The system observed the market and correctly concluded there was nothing to trade.

3. **The Hormuz pair is not arbitrageable.** Over 27.5 minutes of observation, the maximum spread was 1.5 cents. After Kalshi's 7% fee, this is a guaranteed loss in the worst-case outcome. No snapshot in the database represents a true risk-free arbitrage.

4. **Kalshi's 7% fee is the strategy-killer.** At current price levels (~32 cents), the fee alone requires 2.24 cents of spread just to break even. The market simply does not provide spreads that large. The typical spread is -0.5 to 0 cents (Kalshi actually cheaper or at parity).

5. **The observation window is tiny.** 27.5 minutes is not statistically meaningful for a market that runs for months. Larger spreads may exist during high-volatility events (news, market open/close). However, the structural fee problem does not go away.

6. **No evidence of profitable opportunities exists in this dataset.** Zero snapshots meet the breakeven threshold. The strategy is structurally unprofitable for this pair at these price levels given Kalshi's fee structure.

### Bottom line:

The arbitrage trader is a well-built system watching a market where the arbitrage does not exist. The spreads are real (not phantom), and they are too small to overcome the 7% Kalshi taker fee. The engine's 2-cent minimum threshold is actually *below* the true breakeven -- even if it fired, trades would lose money on the Kalshi-wins outcome.

**Recommendation:** Either find markets with structurally larger spreads (likely illiquid or newly-listed pairs), negotiate lower Kalshi fees, or pivot to a strategy that does not require guaranteed-profit arbitrage (e.g., statistical arbitrage with directional edge).
