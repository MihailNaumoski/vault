# Database Health Check - 2026-04-09

## 1. Market Pairs

| Metric | Value |
|--------|-------|
| Total pairs | **1** |
| Active | 1 |
| Verified | 1 |
| Unverified (auto-discovered) | 0 |

### The Single Pair

| Field | Value |
|-------|-------|
| ID | `019d6d8e-5ba1-7c53-808a-af67f99e1b3a` |
| Poly question | "Strait of Hormuz traffic returns to normal by end of April?" |
| Kalshi ticker | `KXHORMUZNORM-26MAR17-B260501` |
| Match confidence | 1.0 (manually configured) |
| Verified | Yes |
| Active | Yes |
| Close time | 2027-04-08 (1 year from creation - default) |
| Created | 2026-04-08T14:45:39 |

**Source**: This pair was loaded from `config/pairs.toml` (manual configuration). It is the only pair defined in that file.

## 2. Price Data

| Metric | Value |
|--------|-------|
| Total snapshots | **468** |
| Distinct pairs with data | 1 |
| Earliest snapshot | 2026-04-08T14:46:39 |
| Latest snapshot | 2026-04-09T07:39:16 |
| Snapshots on 2026-04-08 | 421 |
| Snapshots on 2026-04-09 | 47 |

### Spread Statistics

| Metric | Value |
|--------|-------|
| Min spread | -0.045 (Kalshi higher) |
| Max spread | +0.035 (Poly higher) |
| Avg spread | +0.0071 |
| Snapshots with spread > 5% | 0 |

### Spread Distribution

| Spread | Count | Note |
|--------|-------|------|
| -0.045 to -0.015 | ~21 | Kalshi significantly more expensive |
| -0.010 to -0.005 | ~78 | Kalshi slightly more expensive |
| 0.000 | 28 | No spread |
| +0.005 to +0.010 | ~205 | Poly slightly more expensive |
| +0.015 to +0.035 | ~136 | Poly more expensive |

Current prices (latest snapshot): Poly YES = 0.2650, Kalshi YES = 0.2300, spread = +0.035.

## 3. Opportunities

**Zero opportunities detected.** The `opportunities` table is empty.

The engine has never identified an actionable arbitrage opportunity from its price monitoring.

## 4. Orders & Positions

- **Orders**: 0 (table empty)
- **Positions**: 0 (table empty)
- **Unwind events**: 0 (table empty)

The engine has never placed a trade, in paper or live mode.

## 5. Daily P&L

| Date | Mode | Trades Executed | Trades Filled | Net Profit | Capital Deployed |
|------|------|----------------|---------------|------------|-----------------|
| 2026-04-08 | paper | 0 | 0 | $0 | $0 |
| 2026-04-09 | paper | 0 | 0 | $0 | $0 |

Two P&L records exist but both show zero activity.

## 6. Overall Assessment

### Why the TUI Shows Only 1 Item

The TUI shows 1 pair because **there is exactly 1 market pair in the database**. This single pair was manually configured in `config/pairs.toml`. The TUI queries `list_active_market_pairs()` which runs `SELECT ... FROM market_pairs WHERE active = 1`, returning this one row.

### Has Auto-Discovery Ever Run?

**No evidence of auto-discovery producing results.** There are zero unverified pairs in the database. The matcher (`arb-matcher`) has a pipeline that fetches Polymarket markets from Gamma API and Kalshi markets, then runs semantic matching. However, either:
1. The `--match` subcommand has never been run, or
2. It ran but found no matches meeting the confidence threshold, or
3. Matches were found but none survived the filters (volume > $10k, price between 0.10-0.90, close time > 6 hours)

### Has the Engine Ever Traded?

**No.** Zero opportunities, zero orders, zero positions. The engine has only been collecting price data.

### Data Health Summary

| Aspect | Status | Notes |
|--------|--------|-------|
| Schema | OK | All 7 tables exist with proper indexes |
| Price ingestion | Working | 468 snapshots over ~17 hours at ~30s intervals |
| Pair coverage | Minimal | Only 1 manually-configured pair |
| Opportunity detection | Inactive | No opportunities ever detected |
| Trading activity | None | No orders, positions, or P&L |
| Auto-discovery | Not producing | No auto-discovered pairs in DB |

### Root Cause

The TUI shows 1 item because the system has exactly 1 pair — the manually-seeded Hormuz pair. To see more pairs, either:
1. Add more entries to `config/pairs.toml`
2. Run the matcher (`--match` flag) to auto-discover cross-platform pairs
3. Both — seed some manual pairs while also enabling auto-discovery
