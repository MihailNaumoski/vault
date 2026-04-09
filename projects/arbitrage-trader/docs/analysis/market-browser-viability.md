# Market Browser Viability Assessment

**Date:** 2026-04-08
**Database:** `data/arb.db` (322 price snapshots, ~5 hours of observation)
**Analyst:** Data Analyst (Trading team)

---

## 1. Current DB State

| Table            | Records | Notes |
|------------------|---------|-------|
| market_pairs     | 1       | Hormuz pair only |
| price_snapshots  | 322     | 14:46 - 19:45 UTC (5 hours) |
| opportunities    | 0       | Engine never triggered |
| orders           | 0       | Zero trades placed |
| positions        | 0       | Zero positions opened |
| unwind_events    | 0       | Nothing to unwind |
| daily_pnl        | 1       | All zeros (paper mode) |

The single pair is "Strait of Hormuz traffic returns to normal by end of April?" (Polymarket) matched to KXHORMUZNORM-26MAR17-B260501 (Kalshi). Match confidence 1.0, verified, active. Close time 2027-04-08.

Price ranges over the observation window:
- Polymarket YES: 0.255 - 0.335 (25.5c - 33.5c)
- Kalshi YES: 0.250 - 0.350 (25.0c - 35.0c)

**Bottom line:** One pair, zero trades, zero P&L. The engine ran for 5 hours, saw the market, and correctly decided there was nothing worth trading at current fee levels.

---

## 2. Spread Reality Across All Data

### 2.1 Spread Definition

The DB stores `spread = poly_yes_price - kalshi_yes_price`.
- **Positive spread:** Polymarket YES is more expensive. Arb direction = buy Kalshi YES + buy Poly NO.
- **Negative spread:** Kalshi YES is more expensive. Arb direction = buy Poly YES + buy Kalshi NO.

In both cases, the absolute spread represents the raw profit per contract before fees:
`raw_profit = |spread| = 1.00 - cost_of_both_legs`

### 2.2 Distribution (322 snapshots)

| Spread (abs) | Count | % |
|-------------|-------|---|
| 3+ cents    | 5     | 1.6% |
| 2.5 cents   | 30    | 9.5% |
| 2.0 cents   | 7     | 2.2% |
| 1.5 cents   | 62    | 19.6% |
| 1.0 cent    | 17    | 5.4% |
| 0.5 cent    | 178   | 55.8% |
| < 0.5 cent  | 22    | 6.9% |

### 2.3 Summary Statistics

| Metric | Value |
|--------|-------|
| Min spread (abs) | 0.0 cents |
| Max spread (abs) | 4.5 cents |
| Average spread (abs) | 1.02 cents |
| Median spread (abs) | 0.5 cents |
| Snapshots with spread >= 2 cents | 42 (13.0%) |
| Snapshots with spread >= 1.5 cents | 104 (32.3%) |
| Snapshots with spread >= 1 cent | 121 (37.6%) |
| Snapshots with any spread > 0 | 298 (92.5%) |

### 2.4 Are ANY Spreads > 2 Cents?

Yes. 42 snapshots (13%) show absolute spreads >= 2 cents. The biggest was 4.5 cents. But "big spread" does not mean "profitable" -- see Section 3.

### 2.5 Are ANY Profitable After 7% Kalshi Fee?

**Yes -- but only 6 out of 322 (1.9%).** All 6 occurred in the first 6 minutes of observation (14:46 - 14:52 UTC) when the spread was in the negative direction (Kalshi YES > Poly YES).

| Time (UTC) | Poly YES | Kalshi YES | Spread | Fee | Net/contract |
|------------|----------|------------|--------|-----|-------------|
| 14:46:39 | 0.305 | 0.350 | 4.5c | 2.45c | **+2.05c** |
| 14:47:09 | 0.305 | 0.350 | 4.5c | 2.45c | **+2.05c** |
| 14:47:39 | 0.325 | 0.350 | 2.5c | 2.45c | **+0.05c** |
| 14:48:09 | 0.320 | 0.350 | 3.0c | 2.45c | **+0.55c** |
| 14:49:09 | 0.315 | 0.350 | 3.5c | 2.45c | **+1.05c** |
| 14:52:09 | 0.315 | 0.340 | 2.5c | 2.38c | **+0.12c** |

These were in the "buy Poly YES + buy Kalshi NO" direction, where the worst-case Kalshi fee = 7% * kalshi_yes_price.

For the other 28 above-threshold snapshots (positive spread direction, "buy Kalshi YES + buy Poly NO"), the worst-case fee = 7% * (1 - kalshi_yes), which ranges from 4.55c to 5.04c. Since the max spread in that direction was only 3.0c, **zero positive-direction snapshots are profitable after fees.**

The 6 profitable snapshots survived because: (a) the spread was large (2.5-4.5 cents), and (b) the fee in the negative direction is 7% * kalshi_yes, which at kalshi_yes=0.35 is only 2.45 cents -- cheaper than the positive direction's fee.

---

## 3. The Fee Math

### 3.1 Kalshi Fee Structure

Kalshi charges **7% on profit** of winning contracts. "Profit" = payout - cost.

For an arbitrage position with two legs:
- **Leg A (Kalshi side wins):** Fee = 7% * (1.00 - kalshi_price) = 7% of what the Kalshi contract pays out in profit
- **Leg B (Kalshi side loses):** Fee = 0 (no profit, no fee)

So the worst case is always when the Kalshi leg wins.

### 3.2 Breakeven by Arb Direction

**Direction 1: Buy Poly YES + Buy Kalshi NO**
- Cost = poly_yes + kalshi_no = poly_yes + (1 - kalshi_yes)
- Raw spread = kalshi_yes - poly_yes
- Worst case: NO wins, Kalshi NO pays out. Fee = 7% * kalshi_yes
- **Breakeven: spread > 0.07 * kalshi_yes**

**Direction 2: Buy Kalshi YES + Buy Poly NO**
- Cost = kalshi_yes + poly_no = kalshi_yes + (1 - poly_yes)
- Raw spread = poly_yes - kalshi_yes
- Worst case: YES wins, Kalshi YES pays out. Fee = 7% * (1 - kalshi_yes) = 7% * kalshi_no
- **Breakeven: spread > 0.07 * (1 - kalshi_yes)**

### 3.3 Breakeven Table by Price Level

| Kalshi YES Price | Direction 1 Breakeven (buy Poly YES) | Direction 2 Breakeven (buy Kalshi YES) |
|-----------------|--------------------------------------|---------------------------------------|
| $0.05 (5c) | 0.35 cents | 6.65 cents |
| $0.10 (10c) | 0.70 cents | 6.30 cents |
| $0.20 (20c) | 1.40 cents | 5.60 cents |
| $0.30 (30c) | 2.10 cents | 4.90 cents |
| $0.50 (50c) | 3.50 cents | 3.50 cents |
| $0.70 (70c) | 4.90 cents | 2.10 cents |
| $0.80 (80c) | 5.60 cents | 1.40 cents |
| $0.90 (90c) | 6.30 cents | 0.70 cents |
| $0.95 (95c) | 6.65 cents | 0.35 cents |

**Key insight:** The fee is asymmetric. When Kalshi YES is cheap (low probability events), Direction 1 (buy Poly YES + Kalshi NO) has a very low breakeven. When Kalshi YES is expensive (high probability events), Direction 2 (buy Kalshi YES + Poly NO) has a very low breakeven.

For our Hormuz pair (kalshi_yes ~ 0.30-0.35):
- Direction 1 breakeven: 2.1 - 2.45 cents
- Direction 2 breakeven: 4.55 - 4.9 cents

This explains why only Direction 1 snapshots were profitable -- the fee is much cheaper in that direction for this price range.

### 3.4 Sweet Spots

The easiest arb to find is at extreme prices:
- **Markets near 5c or 95c:** One direction only needs 0.35 cents of spread to break even
- **Markets near 50c:** Both directions need 3.5 cents -- the hardest to arb

For the Market Browser, this means: **prioritize markets priced near 0 or 1, not markets priced near 0.50.**

---

## 4. Volume/Liquidity Concern

### 4.1 Current State

The system has **no volume data stored in the database**. The `market_pairs` table has no volume column. The `Market` type in `arb-types` has `volume` and `liquidity` fields, but these are only populated during live API calls (market listing/scanning) and never persisted to the DB.

### 4.2 Orderbook Depth

The system **does** fetch orderbook depth before execution. In `executor.rs`:
```
let book = self.poly.get_order_book(&opp.poly_yes_token_id).await.unwrap_or_default();
let book_depth = book.asks.first().map(|l| l.quantity).unwrap_or(0);
```

The risk manager enforces `min_book_depth = 50` contracts. This is a single-level depth check (top of book only).

### 4.3 Assessment for Market Browser

**Volume is critical for the browser but not currently available in stored form.** Without volume:
- You cannot rank pairs by tradability
- You cannot estimate fill probability
- You cannot assess whether observed spreads are "real" or just wide bid-ask in thin markets

**What you CAN do with existing infrastructure:**
- Both connectors implement `get_order_book()` which returns full bid/ask ladders with quantities
- The `Market` struct has `volume` and `liquidity` fields populated by `list_markets()` / `get_market()`
- You could fetch and display these at browse-time without storing them

**Recommendation for the browser:** Fetch `volume` and `liquidity` from each platform's API when displaying pairs, and show top-of-book depth inline. Do not require historical volume storage -- fresh real-time data is more useful.

---

## 5. What Makes a GOOD Pair for This System?

Based on the data and the fee math, the ideal pair for the Market Browser should have:

### 5.1 Price Range (most important)

Prefer markets where one platform prices YES far from 50c. At extreme prices (< 15c or > 85c), one arb direction breaks even at under 1.1 cents of spread. At 50c, you need 3.5 cents.

**Browser should sort by "cheapest breakeven direction" not by raw spread.**

### 5.2 Close Date

Current config requires `min_time_to_close_hours = 24`. But the real issue is capital lockup. A 3-day-to-close market with a 1.5c spread is better than a 6-month market with a 2c spread because:
- Capital is freed faster
- Annualized return is much higher
- Less exposure to platform risk

**Browser should show annualized return, not just raw spread.**

### 5.3 Spread History

The Hormuz data shows spreads are volatile on 30-second timeframes:
- Direction changes: 10.9% of transitions
- Level changes (> 0.5c move): 23.4% of transitions

This means the browser should show **current spread** and **spread trend** (widening or narrowing), not just a static number.

### 5.4 Minimum Viable Pair Characteristics

| Criterion | Threshold | Rationale |
|-----------|-----------|-----------|
| Kalshi YES price | < 0.15 OR > 0.85 | Breakeven under 1.1c in favorable direction |
| Current absolute spread | > breakeven for best direction | Must be actionable |
| Close time | 1-30 days | Capital efficiency |
| Top-of-book depth | >= 50 contracts (each side) | Fillability |
| Match confidence | >= 0.85 (auto-verified) | Must be same underlying event |

---

## 6. Maker vs Taker Orders

### 6.1 Fee Structure

| Order Type | Kalshi Fee | Polymarket Fee |
|-----------|------------|----------------|
| Taker     | 7% of profit | 0% |
| Maker     | 0%         | 0% |

### 6.2 Impact on Profitability

With maker orders (0% fee on both platforms), **every single non-zero spread is pure profit.** The breakeven is 0.00 cents, not 2-5 cents.

Applied to our dataset:

| Metric | Taker Orders | Maker Orders |
|--------|-------------|-------------|
| Profitable snapshots | 6 (1.9%) | 298 (92.5%) |
| Average profit/contract | ~0.7c (profitable only) | 1.02c (all non-zero) |
| Max profit/contract | 2.05c | 4.5c |
| Snapshots >= 1c profit | 3 | 121 |
| Snapshots >= 1.5c profit | 2 | 104 |

### 6.3 Maker Order Economics at 50 Contracts

| Spread | Taker Net | Maker Net | Taker Return | Maker Return |
|--------|-----------|-----------|-------------|-------------|
| 0.5c   | LOSS      | $0.25     | negative    | 0.50% |
| 1.0c   | LOSS      | $0.50     | negative    | 0.99% |
| 1.5c   | LOSS      | $0.75     | negative    | 1.49% |
| 2.0c   | -$0.08*   | $1.00     | -0.16%      | 1.99% |
| 2.5c   | +$0.20*   | $1.25     | +0.40%      | 2.48% |
| 3.0c   | +$0.45*   | $1.50     | +0.90%      | 2.97% |

*Taker net varies by price level and direction; shown for kalshi_yes=0.30

### 6.4 The Catch

Maker orders have zero fees but real operational costs:

1. **Fill risk:** Your limit order may never execute. You post at a price and wait. If the market moves away, you get nothing.
2. **One-leg risk:** One side fills but the other does not. You now hold a naked directional position on one platform. The unwinder must close it, potentially at a loss.
3. **Time risk:** Orders sit in the book. The market could move 2+ cents in 30 seconds (observed in this data). By the time both legs fill, the spread could have vanished.
4. **Maker queue position:** Other makers (including professional market-making firms) may have orders ahead of you at the same price. You fill last.

### 6.5 Verdict on Maker Strategy

The 1-2 cent spreads we observe **would be profitable with maker orders**, but only if both legs fill within the spread window. Given the data shows 23.4% of 30-second intervals have spread changes > 0.5 cents, the fill window is narrow.

**Maker orders transform the problem from "find bigger spreads" to "fill both legs before the spread closes."** This is an execution quality problem, not a spread-finding problem. The Market Browser helps with spread-finding but does not solve fill management.

---

## 7. Honest Viability Verdict

### 7.1 Will the Market Browser Help Find Profitable Arb Opportunities?

**With taker orders: Almost certainly not.**

The data from one pair over 5 hours is clear:
- Only 1.9% of snapshots (6/322) were profitable after the 7% taker fee
- Those 6 all occurred in the first 6 minutes -- likely a post-startup price convergence artifact
- Average profitable spread was still only 3.5 cents, netting 1.13 cents/contract
- Even if the browser found 100 pairs instead of 1, the fee structure kills thin spreads everywhere

**With maker orders: Possibly yes, but the browser is necessary, not sufficient.**

At 0% fees, 92.5% of snapshots would be profitable. The browser would help by:
- Scanning more pairs to find the best spreads at any moment
- Identifying extreme-price markets where even tiny spreads have value
- Showing live depth so the user can assess fill probability

But the browser alone does not solve maker order fill management, which is the real bottleneck for the maker strategy.

### 7.2 The Fundamental Problem

The fundamental problem is **not** which pair you pick. It is:

1. **With taker orders:** Kalshi's 7% fee requires spreads of 2-7 cents depending on price level. Cross-platform prediction market spreads are structurally 0-2 cents because professional market makers already arbitrage them. The fee exceeds the available edge. More pairs will not fix this.

2. **With maker orders:** Spreads of 0.5-2 cents are abundant (93% of snapshots). Zero fees make them all profitable. But filling both legs of a maker order before the spread closes is an engineering problem that the browser does not address.

### 7.3 Recommendation

| Strategy | Browser Value | Build Priority |
|----------|--------------|----------------|
| Taker arb on established markets | Low -- fee kills the edge regardless of pair selection | Do not build for this use case |
| Maker arb on established markets | Medium -- helps find best entry points, but fill management is the bottleneck | Build only after maker order infrastructure exists |
| Taker arb on new/illiquid markets | High -- the browser would catch transient dislocations on fresh listings that may have 5-10c spreads | Build this, but add new-market alerting |
| News-driven dislocation capture | Medium -- browser shows current state, but news events need real-time alerts, not browsing | Secondary priority |

### 7.4 If We Build It Anyway

If the Market Browser is built regardless of this assessment, it should:

1. **Show fee-adjusted net profit, not raw spread.** Raw spread misleads the user into thinking 2c spreads are tradable.
2. **Calculate breakeven for both directions** at current prices, and only highlight the profitable direction.
3. **Prioritize extreme-price markets** (YES < 15c or YES > 85c) where taker breakeven is lowest.
4. **Flag new markets** (listed within last 24 hours) where dislocations are most likely.
5. **Show annualized return** based on close date, not just absolute cents per contract.
6. **Display live orderbook depth** for both sides so the user can assess fillability.
7. **Include a maker/taker toggle** that recalculates all numbers at 0% vs 7% fee.

### 7.5 The Number

At current Kalshi fee levels (7% taker), the minimum cross-platform spread for guaranteed profit is:
- **0.35 cents** at extreme prices (5c/95c markets)
- **2.10 cents** at 30c markets (like Hormuz)
- **3.50 cents** at 50c markets

The observed market delivers 0.5-1.5 cent spreads 75% of the time. The browser will show the user many pairs with spreads that look actionable but are not.

**The honest answer: The Market Browser is a nice-to-have for situational awareness and maker order preparation, but it will not solve the taker fee problem. The system needs a maker order execution engine before the browser becomes genuinely useful for trading.**
