# Safety Fixes Code Review

**Reviewer:** Code Reviewer (cross-team review)  
**Date:** 2026-04-07  
**Scope:** 3 safety fixes — unwind rate enforcement, config reconciliation, unwind persistence  
**Rework Cycle:** 0  

## Decision: APPROVE

> All three safety fixes are correctly implemented with proper error handling, parameterized queries, Decimal for monetary values, and no `.unwrap()` in production code. No blocking findings.

## Summary

| Severity | Count |
|----------|-------|
| CRITICAL | 0 |
| MAJOR    | 0 |
| MINOR    | 2 |
| NIT      | 2 |
| **Total** | **4** |

**Blocking findings:** 0 (CRITICAL + MAJOR)

## Findings

### [MINOR] Price improvement always adds, never subtracts — `crates/arb-engine/src/executor.rs:79,85`

Both legs apply `+ improve` to the price:
```rust
price: opp.poly_price + improve,   // line 79
price: opp.kalshi_price + improve, // line 85
```

In this prediction market arbitrage model, both legs are *buys* (buy YES on one platform, buy complementary on the other; spread = 1.0 - price_a - price_b), so adding to both prices is correct for increasing fill probability. However, this unconditionally increases cost by `2 * price_improve_amount` per trade ($0.02 total on a 50-contract order = $1.00 per trade), which directly eats into the spread. If the spread is close to the minimum threshold (e.g., 0.02), the improvement could make the trade unprofitable after fees. Consider guarding against `price_improve_amount` exceeding some fraction of the spread.

**Suggested fix:** Add a check like `let improve = improve.min(opp.spread / Decimal::from(4))` to cap price improvement at 25% of spread, or at minimum log a warning when improvement exceeds a threshold relative to spread.

---

### [MINOR] Unwind event uses `order_id` for both `position_id` and `order_id` fields — `crates/arb-engine/src/unwinder.rs:95-98`

```rust
position_id: filled_order.order_id.clone(),
// ...
order_id: Some(filled_order.order_id.clone()),
```

The `position_id` field in `unwind_events` is meant to track which *position* was unwound, but here it is populated with the filled order's ID, not the actual position ID. This doesn't cause a runtime error, but it makes the `idx_unwind_events_position` index less useful for querying "all unwinds for a given position" and creates a data modeling inconsistency. The `Unwinder::unwind()` method doesn't receive a position ID parameter, so the caller would need to pass it.

**Suggested fix:** Add an `opportunity_id` or `position_id` parameter to `Unwinder::unwind()` and use it for the `position_id` field. This can be deferred as a non-blocking improvement.

---

### [NIT] Comment in test incorrectly describes trade counting — `crates/arb-risk/src/manager.rs:350-352`

```rust
// unwind_count=2, total_trades=7 (5 add_position + 2 record_unwind_loss)
// Actually: add_position increments total_trades_today, record_unwind_loss increments unwind_count
// So: total_trades_today=5, unwind_count=2 => rate=40%
```

The first comment line says "total_trades=7" then immediately corrects itself. The self-correction is good but the stale line should be removed to avoid confusion for future readers.

---

### [NIT] `unwrap_or_default()` on order book fetch — `crates/arb-engine/src/executor.rs:50`

```rust
.unwrap_or_default();
```

This silently treats a failed order book fetch as an empty book (depth=0), which would then be caught by the `InsufficientLiquidity` risk check. The behavior is safe, but a `warn!` log when the order book fetch fails would aid debugging.

---

## Spec Compliance Audit

| Acceptance Criterion | Status | Notes |
|---------------------|--------|-------|
| AC-1: `RiskError::UnwindRateTooHigh` properly integrated | Pass | Check #11 in `pre_trade_check()` includes current + max values, returns meaningful error via `thiserror` |
| AC-2: Config defaults reconciled and sane | Pass | `min_book_depth` 50, `min_time_to_close_hours` 24, `max_unwind_rate_pct` 20.0 — all match between `default.toml` and `RiskConfig::default()` |
| AC-3: `price_improve_amount` applied to both legs | Pass | Applied to both poly and kalshi legs (lines 79, 85). Both are buy-side in this arb model, so adding is correct |
| AC-4: `UnwindEventRow` has correct types | Pass | All monetary fields use `Decimal`, timestamps use `DateTime<Utc>`, no sqlx derive attributes needed (manual row mapping) |
| AC-5: `insert_unwind_event()` uses parameterized queries | Pass | 10 `?` placeholders with 10 `.bind()` calls — no string interpolation in SQL |
| AC-6: Migration SQL correct | Pass | Correct types (TEXT for IDs/decimals, INTEGER for quantity), sensible default for `unwound_at`, proper indexes on `position_id` and `unwound_at` |
| AC-7: Unwinder persists events with accurate values | Pass | `entry_price`, `exit_price`, `slippage`, `loss` all correctly computed from `filled_order.price` and `best_bid.price`. Loss is floored at zero with `.max(Decimal::ZERO)` |
| AC-8: No `.unwrap()` in production code | Pass | All `.unwrap()` calls are in `#[cfg(test)]` modules. Production code uses `?`, `unwrap_or`, `unwrap_or_default`, or `if let Err` |
| AC-9: All monetary values use `Decimal` | Pass | `entry_price`, `exit_price`, `slippage`, `loss` in `UnwindEventRow` are all `Decimal`. The only `f64` in models is `match_confidence` (a 0-1 score, not money, per SPEC) |
| AC-10: No hardcoded credentials | Pass | API keys/private keys loaded from config at runtime. Test keys are in `#[cfg(test)]` only. `private_key` field in `Debug` impl is `"<redacted>"` |

## Test Coverage Assessment

The new `test_unwind_rate_too_high` test in `manager.rs` correctly validates that the unwind rate check fires when the rate exceeds the configured maximum. The test sets up a realistic scenario (5 trades, 2 unwinds = 40% > 20% max). The `test_risk_config_defaults` test was updated to verify the new defaults match. The `insert_unwind_event` method is exercised through the existing integration test infrastructure (`setup()` runs all migrations including the new 002). The unwinder's DB persistence is covered at the unit level through the repo tests. There is no direct integration test for the full `Unwinder::unwind() -> DB persist` flow, but this is acceptable given the mock-based architecture.

## Linter / Type-Check Results

Per the Trading team's report: `cargo clippy` clean, `cargo test` passes all 178 tests. Not re-run in this cross-team review (no write access to source code).
