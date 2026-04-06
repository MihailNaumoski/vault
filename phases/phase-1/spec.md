# Phase 1: Implementation Specification — Polymarket API Integration Fix

**Author:** Spec Writer (Planning Team)
**Date:** 2026-04-06
**Based on:** `phases/phase-1/plan.md` (Architect)
**Status:** Complete

---

## Overview

This spec covers five sub-tasks for fixing the Polymarket API integration:
1. Token ID Mapping
2. CLOB Order Book Fix
3. WebSocket Price Feed Fix
4. Market Data Parsing Improvements
5. HTTP Polling Hack Removal

Each acceptance criterion (AC) is numbered, testable, and grouped by sub-task.

---

## Sub-Task 1: Token ID Mapping

### AC-1: Add `clob_token_ids` field to `PolyMarketResponse`

**File:** `crates/arb-polymarket/src/types.rs`
**Function/Struct:** `PolyMarketResponse`

Add a new optional field:
```rust
#[serde(default)]
pub clob_token_ids: Option<String>,
```

**Acceptance:**
- Field deserializes from Gamma API JSON where `clobTokenIds` is a JSON-encoded string (e.g., `"[\"71321045...\",\"71321046...\"]"`)
- Field deserializes as `None` when absent or null
- Existing tests in `types.rs` continue to pass

---

### AC-2: Implement `extract_token_ids()` method on `PolyMarketResponse`

**File:** `crates/arb-polymarket/src/types.rs`
**Function:** `PolyMarketResponse::extract_token_ids() -> Option<(String, String)>`

Returns `Some((yes_token_id, no_token_id))` or `None`.

**Logic (in order of priority):**
1. If `self.tokens` is non-empty, find the token with `outcome == "Yes"` (case-insensitive) and the token with `outcome == "No"` (case-insensitive). Return their `token_id` values.
2. Else if `self.clob_token_ids` is `Some`, parse it as a JSON array of strings. If the array has at least 2 elements, return `(array[0], array[1])` — index 0 is Yes, index 1 is No (positionally aligned with standard Polymarket outcomes ordering).
3. Else return `None`.

**Acceptance:**
- Returns correct IDs when `tokens[]` has Yes and No entries
- Returns correct IDs when only `clob_token_ids` is populated (tokens empty)
- Returns `None` when both are empty
- Returns `None` when `clob_token_ids` is a malformed string
- Returns `None` when `tokens[]` has only one entry (missing Yes or No)
- Case-insensitive matching on outcome ("YES", "yes", "Yes" all work)
- Unit test with all six scenarios above

---

### AC-3: Implement `resolve_token_ids()` on `PolymarketClient`

**File:** `crates/arb-polymarket/src/client.rs`
**Function:** `PolymarketClient::resolve_token_ids(condition_id: &str) -> Result<(String, String), PolymarketError>`

**Logic:**
1. Call `self.fetch_market(condition_id)`
2. Call `.extract_token_ids()` on the response
3. If `None`, return `Err(PolymarketError::Api { status: 0, message: "no token IDs found for condition {condition_id}" })`
4. Return `Ok((yes_id, no_id))`

**Acceptance:**
- Calls `fetch_market` with the provided condition_id
- Returns error with descriptive message when token IDs cannot be extracted
- Respects rate limiter (inherited from `fetch_market`)

---

### AC-4: Add token ID fields to `PairInfo`

**File:** `crates/arb-engine/src/types.rs` (or wherever `PairInfo` is defined)
**Struct:** `PairInfo`

Add fields:
```rust
pub poly_yes_token_id: String,
pub poly_no_token_id: String,
```

**Acceptance:**
- Fields default to empty string `""` for backwards compatibility
- All existing `PairInfo` construction sites are updated (main.rs has two paths: fresh seeding and DB-loaded)
- No compilation errors in engine crate or dependent crates

---

### AC-5: Populate token IDs during pair seeding in `main.rs`

**File:** `crates/arb-cli/src/main.rs`
**Location:** The pair seeding block (around lines 388-494)

**Logic for fresh seeding path (when DB is empty):**
1. After extracting `condition_id` from Gamma response, call `poly_real` (cast or new client) to resolve token IDs
2. Call `resolve_token_ids(condition_id)` for each market
3. If successful, populate `MarketPairRow.poly_yes_token_id` and `poly_no_token_id`
4. If resolution fails, log a warning and skip this market (do not seed with empty token IDs)

**Logic for DB-loaded path:**
1. When loading pairs from DB, check if `poly_yes_token_id` is empty
2. If empty, attempt to resolve token IDs and update the DB row
3. If resolution fails, log a warning but still use the pair (graceful degradation)

**Acceptance:**
- Fresh seeded pairs have non-empty `poly_yes_token_id` and `poly_no_token_id` in DB
- DB-loaded pairs with missing token IDs get backfilled on startup
- Failed resolution does not crash the application
- PairInfo structs passed to engine contain the resolved token IDs

---

## Sub-Task 2: CLOB Order Book Fix

### AC-6: Engine passes token ID (not condition ID) to `get_order_book()`

**File:** Engine crate — wherever `get_order_book()` is called with a Polymarket pair

**Current bug:** The engine calls `get_order_book(pair.poly_market_id)` where `poly_market_id` is a condition ID.

**Fix:** The engine must call `get_order_book(pair.poly_yes_token_id)` for Polymarket order books.

**Acceptance:**
- For Polymarket markets, `get_order_book()` receives a token ID (starts with a large decimal number or hex string, NOT a condition ID format like `0x...` with 66 chars)
- Order book requests to `/book?token_id=<token_id>` return valid data (non-empty bids/asks for active markets)
- If `poly_yes_token_id` is empty, the call should be skipped or return an error rather than sending an empty token_id

---

### AC-7: `PolyBookResponse` includes additional fields from API

**File:** `crates/arb-polymarket/src/types.rs`
**Struct:** `PolyBookResponse`

Add optional fields matching the CLOB `/book` response:
```rust
#[serde(default)]
pub min_order_size: Option<String>,
#[serde(default)]
pub tick_size: Option<String>,
#[serde(default)]
pub neg_risk: Option<bool>,
#[serde(default)]
pub last_trade_price: Option<String>,
#[serde(default)]
pub hash: Option<String>,
```

**Acceptance:**
- New fields deserialize from CLOB response without breaking existing tests
- Existing `PolyBookResponse` deserialization tests pass (new fields are all optional with defaults)
- `to_order_book()` continues to work unchanged

---

## Sub-Task 3: WebSocket Price Feed Fix

### AC-8: Fix WebSocket URL in `config/default.toml`

**File:** `config/default.toml`
**Line:** 27

**Change:**
```toml
# Before:
ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws"
# After:
ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/market"
```

**Acceptance:**
- Config value matches the documented WebSocket URL exactly
- Matches the default in `types.rs::default_ws_url()`

---

### AC-9: Update WebSocket subscription message format

**File:** `crates/arb-polymarket/src/ws.rs`
**Location:** Inside `subscribe()`, the subscription message construction (around line 104)

**Current (incorrect):**
```json
{"type": "subscribe", "channel": "market", "assets_ids": ["<id>"]}
```
Sent in a loop, one message per token ID.

**Required (from docs):**
```json
{
  "assets_ids": ["<token_id_1>", "<token_id_2>", ...],
  "type": "market",
  "initial_dump": true,
  "level": 2,
  "custom_feature_enabled": false
}
```
Sent as a single message with all token IDs.

**Acceptance:**
- Subscription message uses `"type": "market"` (not `"subscribe"`)
- No `"channel"` field present
- Includes `"initial_dump": true`
- Includes `"level": 2`
- Includes `"custom_feature_enabled": false`
- All token IDs are sent in a single `"assets_ids"` array (not one per message)
- Field name is `"assets_ids"` (confirmed correct from docs)
- Unit test verifies the message JSON structure

---

### AC-10: Fix WebSocket heartbeat to use text PING/PONG

**File:** `crates/arb-polymarket/src/ws.rs`
**Location:** The ping/pong section of the message loop (around lines 156-169)

**Current:** Sends binary `Message::Ping(vec![])` every 30 seconds.
**Required:** Send text `Message::Text("PING")` every 10 seconds. Handle text `"PONG"` responses.

**Acceptance:**
- Ping interval reduced from 30 seconds to 10 seconds
- Sends `Message::Text("PING".into())` instead of `Message::Ping(...)`
- `"PONG"` text messages are recognized and update `last_message_time`
- Stale timeout remains at 90 seconds (or adjusted proportionally)
- Binary pong handling (`Message::Pong`) is retained as fallback

---

### AC-11: Update `PolyWsMessage::PriceChange` variant

**File:** `crates/arb-polymarket/src/types.rs`
**Enum variant:** `PolyWsMessage::PriceChange`

The actual WebSocket `price_change` event has a `price_changes[]` array, not flat fields.

**Option A (recommended):** Change the variant to:
```rust
PriceChange {
    market: Option<String>,
    #[serde(default)]
    price_changes: Vec<PriceChangeEntry>,
    timestamp: Option<String>,
}
```
Where:
```rust
pub struct PriceChangeEntry {
    pub asset_id: String,
    pub price: String,
    pub side: Option<String>,
}
```

**Option B:** Keep the current flat structure and only handle it in `parse_ws_message()` with manual JSON parsing.

**Acceptance:**
- `price_change` events from the WebSocket are parsed without error
- Each `PriceChangeEntry` in the array produces a separate `PriceUpdate` (or the first matching entry is used)
- The `parse_ws_message()` function in `ws.rs` is updated accordingly
- Unit test with a `price_change` message containing a `price_changes` array

---

### AC-12: Handle new WebSocket event types gracefully

**File:** `crates/arb-polymarket/src/types.rs`
**Enum:** `PolyWsMessage`

The WebSocket may send additional event types: `tick_size_change`, `best_bid_ask`, `new_market`, `market_resolved`.

**Acceptance:**
- The `Unknown` variant catches all unrecognized event types without panic
- `best_bid_ask` events could optionally be parsed for price updates (nice-to-have, not required)
- No deserialization errors logged for known-but-ignored event types

---

## Sub-Task 4: Market Data Parsing Improvements

### AC-13: `PolyMarketResponse` handles `clobTokenIds` as JSON-encoded string

**File:** `crates/arb-polymarket/src/types.rs`

The Gamma API returns `clobTokenIds` as a JSON-encoded string like `"[\"71321045...\",\"71321046...\"]"`, not as a native JSON array.

**Acceptance:**
- `clob_token_ids` field correctly deserializes the string value
- `extract_token_ids()` correctly parses the inner JSON array
- Does not break when `clobTokenIds` is null, absent, or an empty string `""`

---

### AC-14: `PolyMarketResponse::to_market()` preserves token IDs

**File:** `crates/arb-polymarket/src/types.rs`

Currently `to_market()` creates an `arb_types::Market` but discards token IDs. While token IDs are stored separately in `MarketPairRow`, the conversion should not actively lose information.

**Acceptance:**
- No changes needed to `to_market()` itself (token IDs are extracted separately via `extract_token_ids()`)
- Verify that `to_market()` still works correctly after adding `clob_token_ids` field
- Existing `test_market_response_deserialize` test passes

---

## Sub-Task 5: HTTP Polling Hack Removal (`main.rs`)

### AC-15: Remove the HTTP polling `tokio::spawn` block

**File:** `crates/arb-cli/src/main.rs`
**Location:** Lines 559-626 (the `tokio::spawn(async move { ... })` loop)

**Acceptance:**
- The entire polling block is removed
- No remaining references to `gamma_url_for_feed`, `feed_pairs`, or the `poly_prices` HashMap used in the polling loop
- The `"Price feed started (Polymarket API polling every 8s + Kalshi mock)"` log line is removed
- Compilation succeeds with no dead code warnings related to removed variables

---

### AC-16: Replace with proper WebSocket price subscription

**File:** `crates/arb-cli/src/main.rs`
**Location:** After pair seeding, before engine start

**Logic:**
1. Collect all unique `poly_yes_token_id` values from the seeded pairs
2. Filter out any empty strings
3. If the list is non-empty, call `poly.subscribe_prices(&token_ids, price_tx.clone())` through the connector
4. Store the returned `SubHandle` to keep the subscription alive
5. Also start the Kalshi mock price feed (the jitter loop for mock Kalshi prices) separately if still needed

**Acceptance:**
- WebSocket subscription is established through the connector's `subscribe_prices` method
- Token IDs passed to `subscribe_prices` are actual token IDs (not condition IDs)
- The `SubHandle` is stored (not dropped) so the WS connection stays alive
- `PriceUpdate` events from the WS use `asset_id` as `market_id` (which are token IDs)
- Price cache registration uses the correct IDs (token IDs match what PriceUpdate.market_id contains)
- Engine receives price updates via the existing `price_rx` channel

---

### AC-17: Price cache registration uses token IDs

**File:** `crates/arb-cli/src/main.rs`
**Location:** Around line 532 — `price_cache.register_pair(...)`

**Current:**
```rust
price_cache.register_pair(p.pair_id, &p.poly_market_id, &p.kalshi_market_id);
```
Where `poly_market_id` is a condition ID.

**Required:** The price cache must map `pair_id` to the ID that will appear in `PriceUpdate.market_id`. Since WS sends `asset_id` (token ID), the cache must be keyed by token ID.

**Fix:**
```rust
price_cache.register_pair(p.pair_id, &p.poly_yes_token_id, &p.kalshi_market_id);
```

**Acceptance:**
- Price cache is registered with the Polymarket YES token ID (not condition ID)
- When a `PriceUpdate` arrives from the WS with `market_id = <token_id>`, the price cache correctly maps it to the right pair
- Engine can look up the correct cached price for each pair

---

### AC-18: Kalshi mock price feed preserved

**File:** `crates/arb-cli/src/main.rs`

The removed polling block also generates mock Kalshi price updates with jitter. This functionality must be preserved.

**Acceptance:**
- Mock Kalshi price updates continue to be generated on a timer
- Kalshi `PriceUpdate` events are sent to the same `price_tx` channel
- The jitter pattern can be simplified but must still produce price variation

---

## Edge Cases and Error Handling

### AC-19: Empty token IDs are handled gracefully

**All affected files**

**Acceptance:**
- If `poly_yes_token_id` is empty string after resolution attempt, the pair is either:
  - Skipped during seeding (with warning log), OR
  - Included but order book / WS subscription calls are skipped for that pair (with warning)
- No panics or crashes from empty token_id in `/book`, WS subscription, or order placement
- `connector.rs::place_limit_order()` returns error if `req.market_id` is empty (already implemented)

---

### AC-20: WebSocket disconnection and reconnection

**File:** `crates/arb-polymarket/src/ws.rs`

**Acceptance:**
- Reconnection logic with exponential backoff continues to work after subscription format change
- On reconnect, the new subscription message format is re-sent with all stored token IDs
- Max consecutive failures (10) triggers permanent stop (existing behavior preserved)
- `PING`/`PONG` timeout causes reconnect, not crash

---

### AC-21: Gamma API unavailability during token ID resolution

**File:** `crates/arb-cli/src/main.rs`

**Acceptance:**
- If `resolve_token_ids()` fails for a market (network error, 404, etc.), a warning is logged
- The market pair is skipped (fresh seeding) or used without token IDs (DB-loaded, with warning)
- The application continues to start with whatever pairs it can resolve
- At least one pair must resolve successfully for the engine to start (or log a clear error)

---

### AC-22: `clobTokenIds` has fewer than 2 entries

**File:** `crates/arb-polymarket/src/types.rs`

**Acceptance:**
- If `clobTokenIds` parses to an array with 0 or 1 elements, `extract_token_ids()` returns `None`
- No index-out-of-bounds panic
- Unit test covers this case

---

## Summary

| Sub-Task | ACs | Coverage |
|---|---|---|
| Token ID Mapping | AC-1 through AC-5 | Type changes, extraction, resolution, storage, population |
| CLOB Order Book | AC-6, AC-7 | Correct ID passing, response fields |
| WebSocket Fix | AC-8 through AC-12 | URL, subscription format, heartbeat, message types |
| Market Parsing | AC-13, AC-14 | clobTokenIds parsing, backward compat |
| Polling Removal | AC-15 through AC-18 | Remove hack, add WS, fix cache, preserve Kalshi |
| Edge Cases | AC-19 through AC-22 | Empty IDs, reconnect, API failure, malformed data |

**Total: 22 acceptance criteria across 5 sub-tasks + edge cases**

---

## Files Modified (Summary)

| File | Changes |
|---|---|
| `crates/arb-polymarket/src/types.rs` | AC-1, AC-2, AC-7, AC-11, AC-12, AC-13, AC-14, AC-22 |
| `crates/arb-polymarket/src/client.rs` | AC-3 |
| `crates/arb-polymarket/src/ws.rs` | AC-9, AC-10, AC-20 |
| `crates/arb-polymarket/src/connector.rs` | Minor (expose resolve if needed) |
| `crates/arb-cli/src/main.rs` | AC-5, AC-15, AC-16, AC-17, AC-18, AC-21 |
| `config/default.toml` | AC-8 |
| `crates/arb-engine/src/types.rs` | AC-4 |
| Engine order book call site | AC-6 |
