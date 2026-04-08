# TUI Bug Fixes - Code Review

**Reviewer:** Engineering / Code Reviewer
**Date:** 2026-04-07
**Verdict:** APPROVE

## Files Reviewed
1. `crates/arb-cli/src/tui.rs`
2. `crates/arb-engine/src/engine.rs`

## Bug 1: Open Orders panel only showing "open" status (tui.rs:94-99)

**Fix:** Replaced single `list_orders_by_status("open")` with a loop over `["open", "pending", "partial_fill"]`.

**Assessment: Correct.**
- The three statuses cover all active (non-terminal) order states. `Filled`, `Cancelled`, and `Failed` are terminal and should not appear in "Open Orders."
- Uses `if let Ok(mut orders)` so a DB error on one status does not block the others.
- Appends via `all_active.append(&mut orders)` which is idiomatic and avoids extra allocation.
- Edge case (all queries fail): `all_active` stays empty, panel shows 0 orders. Correct behavior.

**Severity:** None - no issues.

## Bug 2A: market_names missing pair.id key (tui.rs:109-115)

**Fix:** Added `state.market_names.insert(p.id.clone(), short)` after inserting `poly_condition_id` and `kalshi_ticker` keys.

**Assessment: Correct.**
- `PositionRow.pair_id` stores `MarketPairRow.id`, not a platform-specific ID. Without this key, positions could never resolve a human-readable name.
- The `.clone()` ordering is correct: `short.clone()` for the first two inserts, moved `short` (no clone) for the third. This is optimal.
- The `if state.market_names.is_empty()` guard means this only runs once, so the extra insert has negligible cost.
- Edge case (no active pairs): map stays empty, fallback truncation logic in `draw_positions` handles it.

**Severity:** None - no issues.

## Bug 2B: Position market name lookup using wrong key (tui.rs:422)

**Fix:** Replaced `.values().next()` (which grabbed an arbitrary name) with `.get(&p.pair_id)` (direct lookup by position's pair_id).

**Assessment: Correct.**
- `PositionRow.pair_id` is a `String` matching `MarketPairRow.id`. The `market_names` map now has this key (Bug 2A fix).
- The fallback `unwrap_or_else` truncates `p.pair_id` if no name found, which is the right defensive behavior.
- Edge case (pair_id not in map): falls back to displaying the raw pair_id (truncated at 25 chars). Acceptable.

**Severity:** None - no issues.

## Bug 3: aggregate_daily_pnl only counting 3 of 6 statuses (engine.rs:282-287)

**Fix:** Replaced three individual `list_orders_by_status` calls with a loop over all 6 statuses: `["open", "pending", "partial_fill", "filled", "cancelled", "failed"]`.

**Assessment: Correct.**
- `trades_executed` is meant to reflect total order activity for the day. Excluding `filled`, `cancelled`, and `failed` would undercount.
- The `OrderStatus` enum has exactly 6 variants: `Pending`, `Open`, `PartialFill`, `Filled`, `Cancelled`, `Failed`. All are now covered.
- Uses `if let Ok(orders)` so individual status query failures are silently skipped (consistent with existing error handling in this method).
- `filled_orders` is still queried separately on line 288 for `trades_filled`, which is correct - that's a different metric.
- Edge case (all queries fail): `all_orders_count` stays 0, P&L row shows 0 trades. Acceptable degraded behavior.

**Note:** `list_orders_by_status` does not filter by date - it returns ALL orders with that status, not just today's. This is a pre-existing design issue (not introduced by this fix) and is out of scope for this review. The fix correctly makes the query comprehensive across all statuses, matching the intended semantics of the existing code.

## Clippy / Build Verification

```
cargo clippy: 0 warnings, 0 errors
```

Clean build confirmed.

## Summary

All four fixes are correct, minimal, and well-targeted. Each fix addresses exactly the reported bug without introducing new issues. Error handling is consistent with the existing codebase patterns (silent `if let Ok` for non-critical TUI refreshes). No regressions identified.
