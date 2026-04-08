# P0 Price Ingestion Fixes — Code Review (Rework 1)

**Reviewer:** Code Reviewer (Engineering)
**Date:** 2026-04-08
**Rework cycle:** 1 (verifying F1 fix from initial review)
**Files reviewed:**
- `crates/arb-kalshi/src/ws.rs` (full file — focus on lines 469-498, 640-654, 960-995)
- `crates/arb-kalshi/src/connector.rs` (line 405)
- `crates/arb-kalshi/src/auth.rs` (clippy fix)

**Build verification:** cargo clippy PASS (0 errors, 1 pre-existing warning in arb-risk), cargo test 179/179 PASS

---

## Verdict: APPROVE

F1 is resolved. No new issues found. All previous blocking findings are addressed.

---

## F1 Resolution Verification

### What changed

1. **`best_bid()` (line 469):** Now returns `Option<Decimal>` instead of `Decimal`. Uses `.max()` on the iterator, which naturally returns `None` when `yes_levels` is empty. No `.unwrap_or_default()`.

2. **`best_ask()` (line 478):** Same pattern — returns `Option<Decimal>` via `.min()`. No `.unwrap_or_default()`.

3. **`to_price_update()` (line 488):** Now returns `Option<PriceUpdate>`. Uses the `?` operator on both `self.best_bid()?` and `self.best_ask()?`, so if either side is empty, the whole method returns `None`. No zero leakage path.

4. **Delta processing in `connect_and_run()` (lines 647-654):** `book.to_price_update(market_ticker)` now returns `Option<PriceUpdate>` directly, which is assigned to `price_update`. When either book side is drained by deltas, `None` propagates and no update is sent to the price cache. This is correct.

5. **Snapshot processing in `connect_and_run()` (lines 641-645):** `book.to_price_update(market_ticker)` returns `Option<PriceUpdate>`, assigned directly to `update`. The original F3 finding (divergence between live path and dead code in `ws_message_to_price_update`) is now moot — both paths use Option-based None propagation.

### Zero leakage audit

Searched for `unwrap_or_default` in `ws.rs`: **zero occurrences**. The remaining `unwrap_or_default` calls in `client.rs` are on HTTP response body text (`.text().await.unwrap_or_default()`) for error messages, not price values — these are safe.

### F1 status: **RESOLVED**

---

## New Test Coverage

### `test_local_orderbook_one_side_empty_returns_none` (lines 982-995)

This test covers exactly the scenario identified in the original review:

1. Adds only YES-side levels, asserts `to_price_update()` returns `None` (no NO side)
2. Adds NO-side levels, asserts `to_price_update()` returns `Some` (both sides present)
3. Removes all NO-side levels via negative delta, asserts `to_price_update()` returns `None` again

This exercises the full drain-via-delta lifecycle. Good coverage.

### Updated existing tests

- `test_local_orderbook_from_snapshot` (line 936-937): Uses `Some(dec!(...))` for `best_bid`/`best_ask` assertions
- `test_local_orderbook_apply_delta_add` (line 945-946): Same
- `test_local_orderbook_apply_delta_remove` (line 957): Same
- `test_local_orderbook_empty_returns_none` (line 961-966): Asserts `None` for both methods and `to_price_update`

All updated tests correctly reflect the `Option` return types.

---

## F5 Resolution Verification

**File:** `crates/arb-kalshi/src/connector.rs` line 405

Comment now reads:
```
// no_price should be 0.45 (from 1 - yes_ask cents fallback), not 45
```

This accurately describes the derivation: `1 - 0.55 = 0.45`. **Resolved.**

---

## Auth.rs Clippy Fix

The `auth.rs` file was noted as having a collapsible `str_replace` clippy warning. Reviewed the file — no clippy warnings remain for this crate. The only remaining clippy warning is the pre-existing `type_complexity` in `arb-risk/src/manager.rs`, which is unrelated to these changes.

---

## Outstanding Items from Initial Review (Non-Blocking)

These were non-blocking in the initial review and remain non-blocking:

| Finding | Severity | Status | Notes |
|---------|----------|--------|-------|
| F1 | P1 / BUG | **RESOLVED** | `Option` return types prevent zero leakage |
| F2 | P2 / CORRECTNESS | Acknowledged | Ticker cents fallback uses raw `no_price` rather than `1 - yes_ask` in cents. Acceptable as legacy fallback since dollar fields are present in modern messages. |
| F3 | P2 / DEAD CODE | **Mitigated** | Live snapshot path now uses same `Option`-based `to_price_update()` as the dead code path in `ws_message_to_price_update()`. Paths are convergent. Dead code remains for test coverage. |
| F4 | P3 / HARDENING | Open | WS price path still does not validate [0, 1] range. Not a bug (detector rejects negative spreads) but would improve data integrity. |
| F5 | P3 / STYLE | **RESOLVED** | Comment updated |
| F6 | P3 / PRECISION | Open | `f64` for quantities in `LocalOrderbook`. Low risk, theoretical. |

---

## Checklist

| Item | Status | Notes |
|------|--------|-------|
| No `.unwrap_or_default()` on prices | PASS | Zero occurrences in ws.rs |
| `Decimal` for monetary values | PASS | All prices use `rust_decimal::Decimal` |
| `Option` propagation prevents zero emission | PASS | `best_bid()`, `best_ask()`, `to_price_update()` all return `Option` |
| Delta drain scenario handled | PASS | New test confirms `None` on drain |
| Snapshot empty-side scenario handled | PASS | `?` operator in both `ws_message_to_price_update` and `to_price_update` |
| `cargo clippy` clean | PASS | 0 errors, 1 pre-existing warning (unrelated) |
| `cargo test` all pass | PASS | 179/179 |
| No regressions | PASS | All 4 updated tests pass with `Option` types |

---

## Summary

The F1 fix is clean and correct. Changing `best_bid()` and `best_ask()` to return `Option<Decimal>` and `to_price_update()` to return `Option<PriceUpdate>` eliminates the zero-leakage path at the source. The `?` operator in `to_price_update()` ensures that a `PriceUpdate` is only emitted when both sides of the orderbook have at least one level. The new test covers the exact drain-via-delta scenario that was the core concern.

All three layers of the price pipeline now correctly handle missing data:
1. **Ingestion (ws.rs):** Returns `None` when prices are unavailable
2. **Ticker handler:** Requires both `yes_p` and `no_p` to be `Some`
3. **Detector (defense-in-depth):** Rejects zero prices at the detection layer

No further rework required.
