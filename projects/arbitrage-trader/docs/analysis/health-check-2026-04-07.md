# Health Check Report - 2026-04-07

## Database: data/arb.db

### 1. Positions Summary
- **Total positions**: 120
- **All positions status**: `open` (none settled, none closed)
- **Hedged quantity**: 50 per position (uniform)
- **Unhedged quantity**: 0 across all positions (good -- no one-legged exposure)

### 2. Win Rate
- **Settled positions**: 0
- **Win rate**: N/A (no positions have settled yet)
- **Guaranteed profit range**: $1.50 - $47.50 per position
- **Average guaranteed profit**: $9.62 per position
- **Total guaranteed profit (unrealized)**: $1,154.53

### 3. Fill Rate
- **Daily P&L records**: 0 (table is empty)
- **Orders**: 274 total, all status `open`
- **Filled quantity**: 0 across all orders
- **Total requested quantity**: 13,700 contracts
- **Fill rate**: 0% (no orders have filled yet)

### 4. Spread Analysis (from opportunities)
- **Total opportunities**: 137
- **Average spread**: $0.1839 (18.39 cents)
- **Average spread %**: 111.68% (very wide spreads, likely paper trading or market making test)
- **Min spread**: $0.03
- **Max spread**: $0.95
- **All opportunity statuses**: `executing` (none resolved)

### 5. Unwind Damage Evidence
- **Orders with cancel_reason**: 0
- **Positions with unhedged_quantity > 0**: 0
- **Failed opportunities**: 0
- **Assessment**: No evidence of past unwind damage. However, there is also no unwind history because unwind events were not persisted (this is exactly the gap Fix #3 addresses).

### 6. Daily P&L Trend
- No daily P&L records exist. The `daily_pnl` table is empty, meaning the P&L aggregation job has not run or the system has not completed a full trading day cycle.

### 7. Price Snapshots
- **Total snapshots**: 0
- **Assessment**: Price snapshot capture appears disabled or not yet running. This limits historical spread analysis capability.

### 8. Platform Distribution
- **Market pairs**: 12 total, all verified, all active
- **Orders**: 137 polymarket + 137 kalshi (perfectly matched)
- **Positions**: 120 (17 opportunities did not result in positions -- likely still executing)

### 9. Anomalies and Concerns

1. **Zero fills**: All 274 orders show `filled_quantity = 0` despite 120 positions being created with `hedged_quantity = 50`. This is likely a paper trading artifact where position creation happens before order fill tracking updates.

2. **No settled positions**: All 120 positions are `open`. Either the system is very new (started ~18 hours ago on 2026-04-06T18:39) or settlement logic is not running.

3. **No price snapshots**: Historical price data is not being captured, which limits post-hoc analysis.

4. **No daily P&L**: Aggregation appears non-functional.

5. **Spread distribution is extremely wide**: Average spread of 111.68% suggests either paper trading with synthetic prices or very illiquid markets. Real prediction markets rarely have spreads above 10%.

6. **17 opportunities without positions**: 137 opportunities vs 120 positions. The gap could be from timing (still executing) or from the 17 extra opportunity rows being older duplicates.

### 10. Unwind Events Table Schema Review

Proposed schema:
```
id, position_id, platform, order_id, entry_price, exit_price, quantity, slippage, loss, unwound_at
```

**Assessment**: Sufficient for core unwind analytics. Suggestions for future iterations:
- Add `opportunity_id` for joining back to the original opportunity
- Add `market_id` for per-market unwind analysis
- Add `book_depth_at_unwind` to correlate liquidity conditions with slippage
- Consider adding `trigger_reason` (timeout, cancel, manual) for operational analysis
- The current schema covers the critical path: what was unwound, at what loss, and when

---
*Generated: 2026-04-07 by automated health check*
