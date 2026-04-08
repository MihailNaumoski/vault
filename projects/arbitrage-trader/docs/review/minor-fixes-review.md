# Minor Fixes Code Review

**Reviewer**: Code Reviewer (automated)
**Date**: 2026-04-07
**Scope**: 2 targeted fixes from safety review feedback

---

## Fix 1: Cap price improvement relative to spread

**File**: `crates/arb-engine/src/executor.rs` (lines 75-87)

### Change Summary
Previously, `price_improve_amount` (default $0.01) was unconditionally added to both leg prices. On 50 contracts, this costs $1.00/trade. If the spread was near the 0.02 minimum threshold, improvement could make the trade net-negative.

Now the improvement is gated:
- **Skipped entirely** if `spread < 2 * price_improve_amount` (protects near-threshold trades)
- **Capped** at `min(configured_amount, spread * 0.25)` (max 25% of spread consumed by improvement)

### Review

| Check | Result | Notes |
|-------|--------|-------|
| Zero-spread edge case | Pass | `spread = 0` triggers `< 2 * improve`, improvement is `Decimal::ZERO` |
| Spread exactly at threshold (0.02) | Pass | `0.02 < 2 * 0.01 = 0.02` is false (not strictly less), so improvement applies. `min(0.01, 0.02 * 0.25 = 0.005) = 0.005`. Net cost is $0.50 on 50 contracts, spread revenue is $1.00 -- still profitable. |
| Spread barely above threshold (0.021) | Pass | `0.021 < 0.02` is false. `min(0.01, 0.021 * 0.25 = 0.00525) = 0.00525`. Conservative. |
| Large spread (0.10) | Pass | `min(0.01, 0.10 * 0.25 = 0.025) = 0.01`. Full configured improvement applies. |
| Negative spread (shouldn't happen) | N/A | Risk manager's `pre_trade_check` rejects spreads below `min_repost_spread` before this code runs. |
| Both legs get same improvement | Pass | Same `improve` variable used for both, which is correct -- each leg gets the same improvement. Total cost is `2 * improve * quantity`, which at the cap is `2 * 0.25 * spread * qty = 0.50 * spread * qty`, leaving 50% of spread as profit. |

**Verdict**: Pass. The logic correctly handles edge cases and ensures price improvement never exceeds what the spread can absorb.

**Note on the `spread == 2 * improve` boundary**: When spread is exactly 0.02 and improve is 0.01, the `<` comparison does NOT skip improvement. Instead, the 25% cap kicks in: `0.02 * 0.25 = 0.005` per leg, costing `2 * 0.005 * 50 = $0.50` total, preserving $0.50 of the $1.00 spread revenue. This is correct behavior.

---

## Fix 2: Fix position_id in unwind events

**Files**:
- `crates/arb-engine/src/unwinder.rs` (signature + row construction)
- `crates/arb-engine/src/engine.rs` (caller)
- `crates/arb-db/src/models.rs` (UnwindEventRow field type)
- `migrations/002_unwind_events.sql` (schema)

### Change Summary
Previously, `UnwindEventRow.position_id` was populated with `filled_order.order_id` -- an order ID, not a position ID. The `unwind()` method had no way to receive the actual position ID.

Now:
1. `unwind()` accepts `position_id: Option<String>` as a parameter
2. `UnwindEventRow.position_id` changed from `String` to `Option<String>`
3. DB schema changed from `TEXT NOT NULL` to `TEXT` (nullable)
4. Caller in `engine.rs` passes `None` (correct: unwinds happen when a position was NOT created)
5. The `order_id` field on the row continues to correctly hold the order ID

### Review

| Check | Result | Notes |
|-------|--------|-------|
| position_id no longer contains order_id | Pass | The old bug is fixed -- `position_id` field now receives the explicit parameter, not `filled_order.order_id` |
| Caller passes correct value | Pass | In `NeedsUnwind` handler, no position exists (position creation only happens in `BothFilled`), so `None` is correct |
| DB schema matches model | Pass | Both schema and model now allow NULL for `position_id` |
| sqlx binding handles Option | Pass | `sqlx::query().bind(&event.position_id)` correctly binds `None` as SQL NULL |
| Existing index still works | Pass | `idx_unwind_events_position` on `position_id` handles NULLs (SQLite indexes include NULLs) |
| Future callers can pass position_id | Pass | If unwind is later called for an existing position (e.g., during settlement), the caller can pass `Some(position_id)` |
| order_id field preserved | Pass | `order_id: Some(filled_order.order_id.clone())` unchanged -- correctly stores the order ID in its proper field |

**Verdict**: Pass. The fix correctly separates the position_id and order_id concerns, makes position_id nullable for the common case where no position exists, and is forward-compatible for future callers that do have a position.

---

## Build Verification

- `cargo build`: Clean
- `cargo test`: 175 tests pass (0 failures)
- `cargo clippy`: No warnings
