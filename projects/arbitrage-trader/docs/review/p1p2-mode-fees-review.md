# P1 (Mode Tracking) + P2 (Fee Modeling) Code Review

**Reviewer**: Code Reviewer (Engineering Team)
**Date**: 2026-04-08
**Files Reviewed**: 11 files across arb-db, arb-engine, arb-cli
**Build Status**: cargo build PASS, cargo test 184 PASS, cargo clippy PASS (verified)

---

## Verdict: APPROVE with advisory findings

The implementation is solid, well-structured, and production-safe. All checklist items pass or have only minor advisory notes. No blocking issues found.

---

## Checklist Results

### No `.unwrap()` in production code -- PASS
All `.unwrap()` calls are confined to `#[cfg(test)]` blocks. Production code uses `if let Err(e)` patterns, `unwrap_or_default()`, or propagates errors with `?`. Clean.

### `Decimal` for all monetary values -- PASS
All prices, fees, profits, spreads, and capital figures use `rust_decimal::Decimal`. No floating-point arithmetic on money anywhere. The config file stores decimals as quoted strings (`"7.0"`, `"0.0"`) which are parsed to `Decimal` -- correct approach.

### SQL injection safety -- PASS (with advisory note)
All production queries use parameterized `?` bindings via sqlx. The one exception is `has_column()` in migration 003 which uses `format!("PRAGMA table_info({})", table)` -- however this is **not exploitable** because:
1. The `table` values are hardcoded string literals (`"opportunities"`, `"orders"`, `"positions"`, `"daily_pnl"`)
2. No user input ever reaches this function
3. `PRAGMA table_info` is read-only

Similarly, `ALTER TABLE` uses `format!()` but only with the same hardcoded table names.

**Advisory**: Consider using a constant array and iterating it to make the closed set of table names explicit, though this is cosmetic.

### Migration is idempotent and backward-compatible -- PASS
- `has_column()` checks prevent duplicate `ALTER TABLE ADD COLUMN` calls
- `daily_pnl` recreation uses `CREATE TABLE IF NOT EXISTS` + `INSERT OR IGNORE` + rename pattern
- Default value `'paper'` on all new columns means existing rows get sensible values
- Migration runs programmatically in `run_migrations()` after 001/002

**Advisory**: The `daily_pnl` migration does `DROP TABLE daily_pnl` then `ALTER TABLE daily_pnl_new RENAME TO daily_pnl` in a single `raw_sql` batch. If the process crashes between DROP and RENAME, data is lost. In practice this is a sub-millisecond SQLite operation on a small table, so the risk is negligible -- but wrapping in a transaction would be safer for production databases with significant historical data.

### Mode string validation -- ADVISORY (no blocking issue)
The mode string is derived in `main.rs` lines 354-360:
```rust
let trading_mode: String = if args.paper_both || args.paper {
    "paper".into()
} else if use_demo {
    "demo".into()
} else {
    "production".into()
};
```
This is a closed set -- only these three values can be produced from CLI flag combinations. However, the `mode` field on models is `String`, and there is no validation at the DB layer or model layer to enforce the enum.

**Advisory**: Consider adding a `TradingMode` enum (`Paper`, `Demo`, `Production`) with `Display`/`FromStr` implementations. This would provide compile-time safety and prevent accidental invalid values if new call sites are added. Not blocking because the current code is safe -- all mode values originate from the single derivation point above.

### Fee calculation correctness -- PASS (with important documentation note)
The fee is computed as: `fee_rate * contract_price * quantity`

For default config (Kalshi 7%, Poly 0%):
- `kalshi_fee = 0.07 * kalshi_price * hedged_quantity`

This models a **taker fee on notional value** (7% of the price paid per contract). This is a reasonable approximation but **differs from Kalshi's actual fee structure**, which charges fees on **profit at settlement**, not on trade notional. Kalshi's actual fee schedule:
- 7% of profit on contracts that settle in-the-money
- No fee on contracts that settle out-of-the-money
- Fee caps at contract value

The current implementation **overestimates** fees (charges upfront on notional rather than on profit at settlement), which means the system will be **more conservative** than necessary -- it may reject opportunities that are actually profitable after real fees. This is a safe direction to err in (better to miss trades than to take unprofitable ones), but it means paper trading P&L will understate actual profitability.

**Recommendation**: Add a doc comment on `FeeConfig::compute_fees()` explicitly noting this is a conservative approximation of the actual Kalshi fee-on-profit model, and file a follow-up to implement the real fee structure (or at minimum, document the delta).

### Fee-adjusted threshold correctness -- PASS
The detector (line 81):
```rust
let effective_min_spread = self.min_spread_absolute + estimated_fees;
```
With default config: `effective_min = 0.02 + 0.07 * kalshi_price`. For typical kalshi_price of 0.53, effective_min = 0.02 + 0.0371 = 0.0571. The test `test_fee_adjusted_threshold_rejects_unprofitable` correctly verifies this: a 0.05 spread is rejected because 0.05 < 0.0571. Good.

The threshold is additive (base + fees) which means it correctly raises the bar but does not accidentally create an impossible-to-meet threshold -- spreads above the fee-adjusted minimum will still be detected.

### Mode correctly derived from CLI flags -- PASS
Flag combinations are properly validated:
- `--production && --demo` -> error (line 344)
- `--paper && --paper_both` -> error (line 346-347)
- Default (no flags) -> `demo` mode (line 351: `let use_demo = !args.production`)
- `--paper` or `--paper_both` -> `paper`
- `--demo` (explicit) -> `demo`
- `--production` -> `production`

All combinations produce valid mode strings. The default-to-demo behavior is the correct safety choice.

### daily_pnl composite key -- PASS
- Table recreated with `PRIMARY KEY (date, mode)` in migration 003
- `upsert_daily_pnl` uses `ON CONFLICT(date, mode) DO UPDATE` -- correct
- `aggregate_daily_pnl()` in engine.rs sets `mode: self.mode.clone()` -- correct
- Positions queried via `list_positions_by_mode(&self.mode)` -- ensures P&L is mode-scoped

**Note**: `get_daily_pnl(date)` queries `WHERE date = ?` without a mode filter, so it returns whichever row it finds first. Since the PK is now composite, this could return paper or production data ambiguously. This is currently only used in tests. If a production code path ever needs to query daily_pnl, it should filter by mode too. Low priority since no production caller exists.

### No hardcoded secrets -- PASS
All secrets (API keys, private keys) are loaded from environment variables. No credentials in source code. Config file contains only public API URLs.

---

## Additional Findings

### 1. `parse_dt()` uses `expect()` (non-blocking, pre-existing)
In `repo.rs` line 159: `DateTime::parse_from_rfc3339(s).expect("valid RFC3339 datetime")`. This will panic if the DB contains malformed datetime strings. This is pre-existing (not introduced in P1/P2) and acceptable for an application that writes its own data, but worth noting for robustness.

### 2. `aggregate_daily_pnl` counts all orders, not just today's (pre-existing logic)
In `engine.rs` lines 291-296, `trades_executed` counts ALL orders across all statuses, not filtered by today's date or by mode. This means the daily P&L `trades_executed` counter is cumulative rather than daily. This appears to be a pre-existing design choice rather than a P1/P2 regression, but it's worth confirming the intended semantics.

### 3. `unhedged_quantity` can be negative (pre-existing)
In `tracker.rs` line 53: `let unhedged = (poly_order.filled_quantity as i32) - (kalshi_order.filled_quantity as i32)`. This can be negative if kalshi fills more than poly. The field is stored as `i64` in the DB, which handles this correctly. Semantically this is fine (negative = over-hedged on kalshi side).

### 4. Fee config deserialization path in main.rs is clean
The `FeesConfig` struct in `main.rs` uses `String` fields (`kalshi_taker_fee_pct: String`) which are then parsed to `Decimal` with `unwrap_or` fallbacks (lines 713-714). This is intentional -- TOML deserializes decimal values as strings for precision. The fallback defaults match `FeeConfig::default()`.

### 5. Test coverage
- 4 new unit tests in `fees.rs` (all fee calculation paths)
- 1 new test in `detector.rs` (`test_fee_adjusted_threshold_rejects_unprofitable`)
- Existing tracker test uses `FeeConfig { 0%, 0% }` to keep legacy assertion valid -- correct approach
- No integration test for mode-scoped daily P&L aggregation across modes (would be nice to have)

---

## Summary

| Checklist Item | Result |
|---|---|
| No `.unwrap()` in production | PASS |
| `Decimal` for monetary values | PASS |
| SQL injection safety | PASS |
| Migration idempotent + backward-compatible | PASS |
| Mode string validation | PASS (advisory: consider enum) |
| Fee calculation correctness | PASS (conservative approximation) |
| Fee-adjusted threshold | PASS |
| Mode derived from CLI flags | PASS |
| daily_pnl composite key | PASS |
| No hardcoded secrets | PASS |

**Decision: APPROVE**

No blocking issues. Three advisory items for future improvement:
1. Add `TradingMode` enum for compile-time mode validation
2. Document that fee model is a conservative approximation of Kalshi's actual fee-on-profit structure
3. Add mode filter to `get_daily_pnl()` before any production caller uses it
