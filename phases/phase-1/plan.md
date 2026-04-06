# Phase 1: Architecture Plan — Polymarket API Integration Fix

**Author:** Architect (Planning Team)
**Date:** 2026-04-06
**Status:** Complete

---

## 1. API Research Findings

### 1.1 Token ID Mapping (conditionId -> clobTokenIds -> tokenId)

**The Problem:** The codebase stores `poly_yes_token_id` and `poly_no_token_id` as empty strings in the DB. Every CLOB operation (order book, price, order placement) requires a **token ID**, not a condition ID.

**The Solution — Two mapping sources:**

**Source A: Gamma API `GET /markets/{conditionId}`**
- Returns `clobTokenIds` as a JSON-encoded string (e.g., `"[\"71321045...\",\"71321046...\"]"`)
- Also returns `outcomes` as a JSON-encoded string (e.g., `"[\"Yes\",\"No\"]"`)
- The `clobTokenIds` array is positionally aligned with `outcomes` — index 0 corresponds to "Yes", index 1 to "No"
- Base URL: `https://gamma-api.polymarket.com`
- Public endpoint, no authentication required

**Source B: CLOB API `GET /simplified-markets`**
- Returns paginated results with `data[]` containing `SimplifiedMarket` objects
- Each market has `condition_id` and a `tokens[]` array
- Each token: `{ token_id: string, outcome: string, price: number, winner: boolean }`
- The `outcome` field directly maps "Yes" / "No" to the `token_id`
- Base URL: `https://clob.polymarket.com`
- Public endpoint, no authentication required

**Chosen Strategy:** Use the Gamma API `GET /markets/{conditionId}` response to extract token IDs, because:
1. We already call this endpoint in `client.rs::fetch_market()`
2. The existing `PolyMarketResponse` already has a `tokens: Vec<PolyToken>` field
3. The Gamma API response includes `clobTokenIds` which we can parse as a fallback
4. We avoid adding a new CLOB endpoint call

**Mapping Flow:**
1. On pair creation/seeding, call `fetch_market(condition_id)` 
2. If `tokens[]` is populated, extract `token_id` for "Yes" and "No" outcomes
3. If `tokens[]` is empty, parse `clobTokenIds` string as JSON array, align with `outcomes` array
4. Store extracted IDs in `MarketPairRow.poly_yes_token_id` and `poly_no_token_id`
5. All downstream CLOB calls use these stored token IDs

### 1.2 CLOB REST Endpoints (Verified from OpenAPI Spec)

**Order Book — `GET /book`**
- Base: `https://clob.polymarket.com`
- Parameter: `token_id` (query, required) — must be a token ID, NOT a condition ID
- Public: yes (no auth headers)
- Response schema:
  ```json
  {
    "market": "string (conditionId/market address)",
    "asset_id": "string (tokenId)",
    "timestamp": "string",
    "hash": "string",
    "bids": [{"price": "string", "size": "string"}],
    "asks": [{"price": "string", "size": "string"}],
    "min_order_size": "string",
    "tick_size": "string",
    "neg_risk": false,
    "last_trade_price": "string"
  }
  ```

**Price — `GET /price`**
- Parameters: `token_id` (required), `side` (required: "BUY" or "SELL")
- Public: yes
- Response: `{"price": 0.45}` (number, not string)

**Midpoint — `GET /midpoint`**
- Parameter: `token_id` (required)
- Public: yes
- Response: `{"mid_price": "string"}`

**Spread — `GET /spread`**
- Parameter: `token_id` (required)
- Public: yes
- Response: `{"spread": "string"}`

**Tick Size — `GET /tick-size`**
- Parameter: `token_id` (optional)
- Public: yes
- Response: `{"minimum_tick_size": number}` (commonly 0.01)

**Prices (batch) — `GET /prices`**
- Parameters: `token_ids` (comma-separated), `sides` (comma-separated "BUY"/"SELL")
- Public: yes
- Response: map of token_id to side-price object

### 1.3 WebSocket Specification (Verified from Docs)

**Exact URL:** `wss://ws-subscriptions-clob.polymarket.com/ws/market`

**Current config value (WRONG):** `wss://ws-subscriptions-clob.polymarket.com/ws`
**Default in types.rs (CORRECT):** `wss://ws-subscriptions-clob.polymarket.com/ws/market`

**Subscription Message Format:**
```json
{
  "assets_ids": ["<token_id_1>", "<token_id_2>"],
  "type": "market",
  "initial_dump": true,
  "level": 2,
  "custom_feature_enabled": false
}
```

**CONFIRMED: The field name is `assets_ids` (with the leading 's' — plural "assets").**
The current code uses `assets_ids` which is correct.

**Current code sends:**
```json
{"type": "subscribe", "channel": "market", "assets_ids": ["<id>"]}
```
**Issues with current format:**
- Missing `"initial_dump": true` — won't receive initial book snapshot
- Missing `"level": 2` — may not get full book depth
- Uses `"type": "subscribe"` — docs show `"type": "market"`
- Uses `"channel": "market"` — not in the docs format

**Update/Unsubscribe format:**
```json
{
  "operation": "subscribe|unsubscribe",
  "assets_ids": ["<token_id>"],
  "level": 2,
  "custom_feature_enabled": false
}
```

**Heartbeat:** Client must send `"PING"` (string) every 10 seconds; server responds `"PONG"`.

**Event types received:**

| Event Type | Fields | Description |
|---|---|---|
| `book` | event_type, asset_id, market, bids[], asks[], timestamp, hash | Full book snapshot |
| `price_change` | event_type, market, price_changes[], timestamp | Price level changes |
| `last_trade_price` | event_type, asset_id, market, price, size, side, timestamp | Last executed trade |
| `tick_size_change` | event_type, asset_id, market, old_tick_size, new_tick_size, timestamp | Tick size updated |
| `best_bid_ask` | event_type, asset_id, market, best_bid, best_ask, spread, timestamp | Top-of-book update |

**Note:** `price_change` has a `price_changes[]` array, not flat `price`/`asset_id` fields. The current `PolyWsMessage::PriceChange` struct is incorrect — it expects flat fields.

**Authentication:** None required. The market channel is public.

### 1.4 Authentication Summary

| Endpoint Category | Auth Required | Type |
|---|---|---|
| Gamma API (all) | No | Public |
| CLOB read: /book, /price, /midpoint, /spread, /tick-size, /prices | No | Public |
| CLOB write: /order, /order/{id} DELETE | Yes | L2 (HMAC) |
| CLOB user: /orders, /positions, /balance | Yes | L2 (HMAC) |
| WebSocket /ws/market | No | Public |

**L2 Auth Headers (current code matches):**
- `POLY_API_KEY`
- `POLY_SIGNATURE` (HMAC-SHA256)
- `POLY_TIMESTAMP`
- `POLY_PASSPHRASE`

**Note from docs:** There is also a `POLY_ADDRESS` header required for authenticated endpoints. The current `auth.rs` does NOT send `POLY_ADDRESS`. This is a latent bug for order placement but is out of scope for Phase 1 token mapping/order book fix (order placement uses L2 auth but is not broken by the token ID issue specifically).

### 1.5 Order Placement Format (Reference)

- `POST /order` with L2 auth
- Request body includes signed `order` object with `tokenId` (string, decimal representation of the uint256)
- `owner` field is a UUID (the API key ID)
- `orderType`: GTC, FOK, GTD, FAK
- The current `signing.rs` produces the correct format
- The issue is that `connector.rs::place_limit_order` uses `req.market_id` as token_id — callers must ensure this is a token ID

---

## 2. Architecture Decisions

### 2.1 Token ID Storage and Lifecycle

**Decision:** Extract token IDs during market pair seeding and store them persistently in the DB.

**Rationale:** Token IDs are static per market — they don't change. Extracting once and storing avoids repeated lookups.

**Implementation:**
- `MarketPairRow` already has `poly_yes_token_id` and `poly_no_token_id` fields
- During pair creation (both the seeding path in main.rs and any future automated pairing), populate these fields
- Add a `resolve_token_ids()` method to `PolymarketClient` that takes a condition ID and returns `(yes_token_id, no_token_id)`

### 2.2 PairInfo Extension

**Decision:** Add `poly_yes_token_id` and `poly_no_token_id` to the `PairInfo` struct.

**Rationale:** `PairInfo` is passed to the engine and all downstream components. They need token IDs to call CLOB endpoints correctly.

**Impact:** `PairInfo` is defined in `arb_engine::types`. This requires a change to the engine crate.

### 2.3 Connector `get_order_book` Fix

**Decision:** The caller must pass a token ID (not condition ID) to `get_order_book()`.

**Current bug:** `connector.rs::get_order_book(id)` receives `id` from the engine, which passes `PairInfo.poly_market_id` (a condition ID). The CLOB `/book` endpoint requires a token ID.

**Fix:** The engine must pass `pair.poly_yes_token_id` instead of `pair.poly_market_id` when calling `get_order_book()`. This is an engine-side change, not a connector change. The connector already correctly forwards to `fetch_order_book(token_id)`.

### 2.4 WebSocket Subscription Format Update

**Decision:** Update the subscription message in `ws.rs` to match the documented format exactly.

**Changes:**
1. Use `"type": "market"` instead of `"type": "subscribe"`
2. Remove `"channel": "market"` 
3. Add `"initial_dump": true` for initial book snapshot
4. Add `"level": 2` for full book depth
5. Add `"custom_feature_enabled": false`
6. Send all token IDs in a single subscription message instead of one-per-ID loop

### 2.5 WebSocket Heartbeat

**Decision:** Add PING/PONG heartbeat to the WebSocket client.

**Current behavior:** The code sends binary ping frames (`Message::Ping`). The docs require sending the string `"PING"` and expecting string `"PONG"`.

**Fix:** Change the ping mechanism to send `Message::Text("PING")` and handle `"PONG"` text responses.

### 2.6 WebSocket URL Fix

**Decision:** Fix the config file value.

**Change:** `config/default.toml` line 27: change `ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws"` to `ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/market"`

### 2.7 HTTP Polling Removal

**Decision:** Remove the `tokio::spawn` HTTP polling loop in `main.rs` (lines 559-626) and replace with proper WebSocket subscription through the connector.

**Why:** The polling hack:
- Bypasses the connector entirely
- Uses condition IDs as market_id in PriceUpdate (wrong for CLOB operations)
- Polls every 8 seconds instead of receiving real-time WS updates
- Doesn't use token IDs at all

**Replacement:** After populating token IDs during pair seeding, call `poly.subscribe_prices(token_ids, price_tx)` through the connector. The WS subscription will emit `PriceUpdate` events with `asset_id` (token ID) as the `market_id`, which is correct for CLOB operations.

### 2.8 PolyWsMessage::PriceChange Update

**Decision:** Update the `PriceChange` variant to match actual WS message format.

**Current:** expects flat `asset_id`, `price` fields
**Actual from docs:** has `price_changes[]` array with multiple changes per message
**Fix:** Update the struct or add a new variant for the array format

---

## 3. Component Changes Needed

### 3.1 `types.rs` — Add clobTokenIds parsing

**File:** `crates/arb-polymarket/src/types.rs`

Changes:
- Add `clob_token_ids: Option<String>` field to `PolyMarketResponse` (Gamma API returns this as JSON string)
- Add method `PolyMarketResponse::extract_token_ids() -> Option<(String, String)>` that:
  1. Tries `tokens[]` array first (match on outcome "Yes"/"No" to get token_id)
  2. Falls back to parsing `clob_token_ids` JSON string aligned with `outcomes`
  3. Returns `(yes_token_id, no_token_id)` or None
- Update `PolyWsMessage::PriceChange` to handle `price_changes[]` array format
- Add `PolyBookResponse` fields: `min_order_size`, `tick_size`, `neg_risk`, `last_trade_price` (optional, for completeness)

### 3.2 `ws.rs` — Fix subscription format and heartbeat

**File:** `crates/arb-polymarket/src/ws.rs`

Changes:
- Update subscription message (line 104-108) to match documented format
- Send all IDs in one subscription instead of looping
- Change ping from binary `Message::Ping` to text `"PING"` 
- Handle `"PONG"` text responses in the message loop
- Reduce ping interval from 30s to 10s per docs

### 3.3 `connector.rs` — No structural changes needed

**File:** `crates/arb-polymarket/src/connector.rs`

The connector correctly delegates to `client.fetch_order_book(id)`. The bug is that callers pass condition IDs. This is fixed by:
1. Populating token IDs during seeding (main.rs)
2. Passing token IDs from engine (engine crate change)

One small change: add a `resolve_token_ids` method that delegates to `client.fetch_market()` + extract.

### 3.4 `client.rs` — Add resolve_token_ids method

**File:** `crates/arb-polymarket/src/client.rs`

Changes:
- Add `resolve_token_ids(condition_id) -> Result<(String, String)>` that calls `fetch_market()` and extracts the yes/no token IDs
- No changes to existing methods (they already take token_id correctly)

### 3.5 `config/default.toml` — Fix WebSocket URL

**File:** `config/default.toml`

Change: `ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws/market"`

### 3.6 `main.rs` — Token ID population + polling removal

**File:** `crates/arb-cli/src/main.rs`

Changes:
- During pair seeding (both fresh and DB-loaded paths), call `resolve_token_ids()` to populate `poly_yes_token_id` and `poly_no_token_id` in the DB rows
- Remove the HTTP polling `tokio::spawn` block (lines 559-626)
- Replace with proper WS subscription: `poly.subscribe_prices(&token_ids, price_tx)`
- Update `PairInfo` construction to include token IDs
- Ensure `PriceUpdate.market_id` from WS uses token_id (which the WS naturally provides as `asset_id`)

### 3.7 `arb_engine::types::PairInfo` — Add token ID fields

**File:** Location in engine crate (outside arb-polymarket)

Changes:
- Add `poly_yes_token_id: String` and `poly_no_token_id: String` to `PairInfo`
- Engine uses `pair.poly_yes_token_id` when calling `get_order_book()` for Polymarket

---

## 4. Implementation Sequence

### Step 1: Types + Config (no runtime behavior change)
1. Add `clob_token_ids` field to `PolyMarketResponse`
2. Add `extract_token_ids()` method
3. Fix `ws_url` in `config/default.toml`
4. Add token ID fields to `PairInfo` (engine crate)

### Step 2: Client + Connector (add capabilities)
1. Add `resolve_token_ids()` to `PolymarketClient`
2. Expose via connector if needed

### Step 3: WebSocket fix (fix subscription format + heartbeat)
1. Update subscription message format in `ws.rs`
2. Fix heartbeat to use text PING/PONG
3. Update `PolyWsMessage::PriceChange` struct

### Step 4: Main.rs integration (wire it all together)
1. Add token ID resolution during pair seeding
2. Update DB rows with resolved token IDs
3. Remove HTTP polling hack
4. Connect WS subscription through connector
5. Ensure engine passes token IDs to order book calls

### Step 5: Validation
1. Unit tests for `extract_token_ids()`
2. Unit tests for new WS subscription format
3. Integration test: condition_id -> token_ids -> order_book pipeline

---

## 5. Risks and Mitigations

| Risk | Severity | Mitigation |
|---|---|---|
| Gamma API rate limiting during token ID resolution | Medium | Resolve token IDs once during seeding, cache in DB. Rate limiter already exists. |
| clobTokenIds field empty or null for some markets | Medium | Fallback to `tokens[]` array. If both empty, skip the market pair with a warning. |
| WS subscription format change breaks connection | High | Keep exponential backoff reconnect. Log subscription messages at debug level. |
| Engine crate PairInfo change has wide blast radius | Medium | PairInfo is only constructed in main.rs. Default empty strings for backwards compat. |
| PING/PONG format mismatch | Low | Easy to test. Server will disconnect if wrong, triggering reconnect. |
| HTTP polling removal removes only working price source | High | Implement WS fix in same PR. Test WS connection before removing polling. Consider feature flag. |

---

## 6. Out of Scope (Phase 2)

- Missing `POLY_ADDRESS` header in `auth.rs` (needed for live order placement)
- Multi-page market fetching (currently first page only)
- Automated market pair matching (currently manual seeding)
- Kalshi real connector (currently mock)
- Database migration for token ID fields (if schema change needed)
