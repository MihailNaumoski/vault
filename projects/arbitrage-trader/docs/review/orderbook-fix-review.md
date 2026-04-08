# Code Review: Orderbook Price Fix + Dead Ticker Cleanup

**Reviewer:** Code Reviewer (Engineering)
**Date:** 2026-04-08
**Verdict:** APPROVE

---

## Summary

Two changes in `crates/arb-kalshi/src/ws.rs` plus a config cleanup in `config/pairs.toml`.

1. **Orderbook price derivation fix** -- `best_ask()` renamed to `best_no_bid()`, changed from `min(no_levels)` to `max(no_levels)`, and all paths (ticker, snapshot, delta) now produce consistent `no_price` semantics. Zero-price defaults replaced with `Option`-based propagation that returns `None` instead of emitting corrupt data.

2. **Dead ticker detection** -- After subscribing, a `HashSet` tracks tickers that have received at least one price update. After 30 seconds, any silent tickers are logged at `warn` level.

3. **Config cleanup** -- Removed 3 placeholder presidential/economic tickers from `pairs.toml`, keeping only the real Hormuz pair.

---

## Checklist

- [x] No `.unwrap()` in production code -- all `.unwrap()` / `.expect()` calls are in `#[cfg(test)]` only. The `unwrap_or(0.0)` in `apply_delta` is a safe default for f64 parse, not a panic path.
- [x] `Decimal` for monetary values -- `PriceUpdate` fields are `rust_decimal::Decimal`. The `f64` in `LocalOrderbook` is only for quantity tracking (delta arithmetic), not price representation. Prices are parsed to `Decimal` at extraction time.
- [x] `best_no_bid()` semantics are correct (see analysis below).
- [x] Snapshot and delta paths produce identical prices (both delegate to `LocalOrderbook`).
- [x] Ticker and orderbook paths are now consistent (both derive `no_price` as `1 - yes_ask`, which equals `best_no_bid`).
- [x] Silent ticker logging works correctly (see analysis below).
- [x] No timing/concurrency issues (see analysis below).
- [x] Tests cover the new behavior -- 5 new/updated tests for empty book, one-side-empty, and updated snapshot assertions.

---

## Detailed Analysis

### 1. `best_no_bid()` -- Is `max(no_levels)` correct?

**Yes, this is semantically correct.**

In Kalshi's binary market model:
- The YES orderbook has bids (people wanting to buy YES). Best bid = highest price = `max(yes_levels)`.
- The NO orderbook has bids (people wanting to buy NO). Best NO bid = highest price = `max(no_levels)`.
- A NO bid at price P implies a YES ask at price (1 - P). So: `yes_ask = 1 - best_no_bid`.

The old code used `min(no_levels)`, which would return the *worst* NO bid -- the lowest price someone is willing to pay for NO. This was wrong because it gave the worst execution price rather than the best.

The ticker path computes: `no_price = 1 - yes_ask_dollars`.
The orderbook path now computes: `no_price = best_no_bid = max(no_levels)`.
Since `yes_ask = 1 - best_no_bid`, we get `no_price = 1 - yes_ask = 1 - (1 - best_no_bid) = best_no_bid`.

The two paths are algebraically identical.

### 2. Snapshot vs Delta Path Consistency

Both the snapshot and delta handlers now delegate to `LocalOrderbook`:

- **Snapshot path** (in `ws_message_to_price_update`): Constructs `LocalOrderbook::from_snapshot()` then calls `to_price_update()`.
- **Snapshot path** (in `connect_and_run`): Same -- constructs `from_snapshot()`, calls `to_price_update()`, stores the book for future deltas.
- **Delta path** (in `connect_and_run`): Applies delta to the stored book, then calls `to_price_update()`.

All three converge on the same `to_price_update()` method, which uses `best_bid()` for YES and `best_no_bid()` for NO. Previously, the snapshot path in `ws_message_to_price_update` had its own inline extraction logic (taking `first()` element) that disagreed with the delta path.

### 3. Zero-Price Elimination

The old code used `.unwrap_or_default()` (i.e., `Decimal::ZERO`) when prices were missing. This is dangerous because:
- A zero Kalshi price creates phantom spreads: `spread = 1 - poly_yes - 0 = ~0.5` (huge fake opportunity).
- The detector already had a zero-guard (lines 50-55 of `detector.rs`), but it's defense-in-depth -- better to never emit zeros upstream.

The new code:
- Ticker path: Returns `None` if either `yes_p` or `no_p` cannot be determined.
- Orderbook path: `best_bid()` and `best_no_bid()` return `Option`, and `to_price_update()` uses `?` to bail on empty sides.

This is the correct approach. The comment in the ticker handler ("CRITICAL: Never default to zero") accurately documents the reasoning.

### 4. Dead Ticker Detection

**Implementation is correct and well-scoped.**

- `tickers_with_data: HashSet<String>` -- inserted on successful price update (line 659), so only real data counts.
- Check fires once after 30 seconds via the `silent_check_done` flag.
- The check runs inside the `ping_interval` sleep arm (every 30s), so worst case it fires at ~60s (if the first ping cycle completes just before the timeout). This is acceptable for a diagnostic warning.
- No concurrency issues: the `HashSet` is local to the `connect_and_run` async function, not shared across threads. The entire function runs in a single task.

**30-second timeout is reasonable.** Kalshi sends an orderbook snapshot immediately upon subscription for active markets, so any market that hasn't sent data within 30s is very likely dead or delisted.

**Minor note:** On reconnection (the outer `ws_task` loop), `connect_and_run` is called fresh, so `tickers_with_data` resets. This means the dead-ticker warning fires again after each reconnect, which is actually useful -- it re-validates liveness after network disruptions.

### 5. Race Between Ticker and Orderbook Updates

The ticker channel and orderbook_delta channel can both fire for the same market in rapid succession. Could they produce different prices?

**Yes, transiently, but this is acceptable.** The ticker is a periodic summary while the orderbook is real-time. They will naturally have slight timing differences. The price cache overwrites on each update (last-write-wins), so the most recent data always prevails. This is the standard approach for exchange data feeds and is not a bug.

### 6. Test Coverage

New/updated tests:
- `test_local_orderbook_empty_returns_none` -- verifies `None` for empty book and `to_price_update`.
- `test_local_orderbook_one_side_empty_returns_none` -- verifies partial books return `None`, then `Some` when complete, then `None` again when a side is removed.
- `test_parse_orderbook_snapshot` -- updated assertion: `no_price = 0.59` (was `0.58`), confirming `max` semantics.
- `test_local_orderbook_from_snapshot` -- asserts `best_no_bid() = Some(0.59)`.
- `test_local_orderbook_apply_delta_add` / `_remove` -- updated to `Option` return types.

Tests adequately cover the core logic changes. No additional tests needed.

---

## Clippy / Build

- `cargo clippy`: PASS (one pre-existing warning in `arb-risk` about type complexity, unrelated).
- `cargo test -p arb-kalshi`: 50/50 pass.

---

## Config Review (`pairs.toml`)

Replaced 4 placeholder pairs (with fake IDs and a "NOTE" warning about them) with 1 real Hormuz pair using actual Polymarket condition/token IDs and a real Kalshi ticker. This is a straightforward cleanup. The token IDs are 78 and 56 digits long, consistent with Polymarket's uint256 token IDs.

---

## Verdict: APPROVE

The price derivation fix is mathematically sound, the zero-price elimination adds a critical safety layer, the snapshot/delta/ticker paths are now provably consistent, and the dead ticker detection is a clean diagnostic addition. Code quality is high -- good comments, proper `Option` propagation, no production panics, and thorough test coverage.
