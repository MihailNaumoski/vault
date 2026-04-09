# TUI Bug Fixes — 2026-04-07

## Summary

Fixed 3 bugs in the TUI and engine where order display was incomplete and positions showed wrong market names.

## Bugs Fixed

### Bug 1: Missing Orders in Open Orders Panel

**File:** `crates/arb-cli/src/tui.rs:94`

**Problem:** `refresh_state` only fetched orders with status `"open"`, but orders start as `"pending"` and can be `"partial_fill"`. These active orders were invisible in the TUI.

**Fix:** Loop over all active statuses and merge results:

```rust
// Before (line 94):
if let Ok(orders) = db.list_orders_by_status("open").await {
    state.open_orders = orders;
}

// After:
let mut all_active = Vec::new();
for status in &["open", "pending", "partial_fill"] {
    if let Ok(mut orders) = db.list_orders_by_status(status).await {
        all_active.append(&mut orders);
    }
}
state.open_orders = all_active;
```

### Bug 2a: Market Names Map Missing Pair ID Key

**File:** `crates/arb-cli/src/tui.rs:109-115`

**Problem:** The `market_names` HashMap was keyed by `poly_condition_id` and `kalshi_ticker`, but positions reference markets via `pair_id` (which maps to `MarketPairRow.id`). No key existed for pair ID lookups.

**Fix:** Added a third insert for the pair's own ID:

```rust
state.market_names.insert(p.poly_condition_id.clone(), short.clone());
state.market_names.insert(p.kalshi_ticker.clone(), short.clone());
state.market_names.insert(p.id.clone(), short);  // <-- NEW
```

### Bug 2b: Positions Show Wrong Market Name

**File:** `crates/arb-cli/src/tui.rs:422`

**Problem:** `draw_positions` used `state.market_names.values().next()` which grabs an arbitrary HashMap entry. All positions displayed the same (random) market name.

**Fix:** Use the same `.get()` lookup pattern as `draw_orders`:

```rust
// Before (line 417):
let market_name = state.market_names.values().next()
    .cloned().unwrap_or_else(|| p.pair_id[..8].to_string());

// After:
let market_name = state.market_names.get(&p.pair_id).cloned()
    .unwrap_or_else(|| if p.pair_id.len() > 25 {
        format!("{}...", &p.pair_id[..25])
    } else {
        p.pair_id.clone()
    });
```

### Bug 3: P&L Aggregation Misses Order Statuses

**File:** `crates/arb-engine/src/engine.rs:282-290`

**Problem:** `aggregate_daily_pnl` only counted `"open"`, `"filled"`, and `"cancelled"` orders for `trades_executed`. Missing `"pending"`, `"partial_fill"`, and `"failed"`.

**Fix:** Loop over all 6 `OrderStatus` variants:

```rust
// Before:
let all_orders = self.db.list_orders_by_status("open").await.unwrap_or_default();
let filled_orders = self.db.list_orders_by_status("filled").await.unwrap_or_default();
let cancelled_orders = self.db.list_orders_by_status("cancelled").await.unwrap_or_default();
let trades_executed = (all_orders.len() + filled_orders.len() + cancelled_orders.len()) as i64;

// After:
let mut all_orders_count = 0i64;
for status in &["open", "pending", "partial_fill", "filled", "cancelled", "failed"] {
    if let Ok(orders) = self.db.list_orders_by_status(status).await {
        all_orders_count += orders.len() as i64;
    }
}
let filled_orders = self.db.list_orders_by_status("filled").await.unwrap_or_default();
let trades_executed = all_orders_count;
let trades_filled = filled_orders.len() as i64;
```

## Code Review

**Decision:** APPROVE — no blocking findings.

**Observation (pre-existing, not introduced):** `list_orders_by_status` has no date filter, so `trades_executed` in daily P&L may count orders from previous days.

**Build:** `cargo check` + `cargo clippy` both clean.

## Files Changed

| File | Changes |
|------|---------|
| `crates/arb-cli/src/tui.rs` | Bugs 1, 2a, 2b |
| `crates/arb-engine/src/engine.rs` | Bug 3 |

## Teams Involved

| Team | Agent | Role |
|------|-------|------|
| Trading | Trading Lead | Coordinated bug fix delegation |
| Trading | Rust Engine Dev | Implemented all 3 fixes |
| Engineering | Engineering Lead | Coordinated code review |
| Engineering | Code Reviewer | Reviewed and approved |

Session: `multi-team/sessions/2026-04-07T21-51-50/`
