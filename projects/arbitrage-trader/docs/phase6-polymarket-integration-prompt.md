# Phase 6 — Polymarket API Integration Fix — Research & Build Prompt

**Goal:** Fix the Polymarket integration to properly use their CLOB API and WebSocket for real-time price feeds, order book data, and correct market identification. Currently the system uses HTTP polling with condition IDs but Polymarket's CLOB uses token IDs for everything.

**Project root:** `/Users/mihail/projects/vault/projects/arbitrage-trader`
**Depends on:** Phase 5 complete (engine, paper trading, TUI all working)

---

## IMPORTANT: Code State

The last **git commit** (`d34f8ee`) contains the clean Phase 1-5 core: 8-crate workspace, 154 tests, paper trading, TUI, engine — all solid.

The **uncommitted changes** are quick-and-dirty integration wiring done WITHOUT reading the Polymarket docs. They work but are hacky:
- `main.rs` — has raw `reqwest` HTTP polling to Gamma API, hardcoded market seeding, synthetic price feeds
- `client.rs` — patched `fetch_markets()` with `active=true&closed=false` query params
- `connector.rs` — added debug logging
- `types.rs` — added `outcome_prices` fallback parsing
- `Cargo.toml` — added `arb-polymarket`, `arb-kalshi` (mock), `reqwest`, `uuid` deps to arb-cli
- `tui.rs` — redesigned with colors, market name lookup
- `config/default.toml` — lowered `min_book_depth` to 0 and `min_time_to_close_hours` to 1 (workarounds)
- `migrations/001_initial_schema.sql` — added `IF NOT EXISTS` to all CREATE statements

**Your job is to replace these hacks with proper implementations that follow the actual Polymarket API documentation.** Read the docs first, understand the correct API patterns (token IDs, CLOB endpoints, WebSocket format), then rewrite the integration properly. The uncommitted changes show what we TRIED to do — use them to understand intent, not as correct implementations.

---

## Research Required

### 1. Polymarket API Documentation

Fetch and analyze the full Polymarket documentation:
- **Documentation index:** `https://docs.polymarket.com/llms.txt`
- Use this index to discover all available API pages
- Focus on: CLOB API, WebSocket, authentication, market data, order placement

### 2. Key Questions to Answer

**Market Identification:**
- What is the relationship between `conditionId`, `tokenId`, and `clobTokenIds`?
- The Gamma API returns `conditionId` (0x hash) and `clobTokenIds` (JSON string of large numbers)
- The CLOB API and WebSocket use `tokenId` — which is the CLOB token ID
- How do you map from conditionId → tokenId? Is it from the Gamma API `clobTokenIds` field?
- Each market has TWO token IDs (Yes and No outcomes) — how to identify which is Yes vs No?

**CLOB REST API:**
- Base URL: `https://clob.polymarket.com`
- What endpoints are available for market data without auth? (order book, prices, midpoint)
- What is the correct endpoint for getting an order book by token ID?
- What does the order book response look like? (bids/asks format)
- Rate limits?

**WebSocket:**
- What is the correct WebSocket URL? (current config: `wss://ws-subscriptions-clob.polymarket.com/ws` — gets 404)
- Is it `wss://ws-subscriptions-clob.polymarket.com/ws/market`?
- What is the correct subscription message format?
- What message types are received? (book updates, price changes, trades)
- Does it use `assets_id` or `market` or `token_id` for subscription?

**Authentication:**
- L1 auth: EIP-712 signing with private key — for creating API keys and signing orders
- L2 auth: HMAC-SHA256 with apiKey/secret/passphrase — for CLOB trading endpoints
- Which endpoints need auth? Which are public?
- Our current auth implementation is in `crates/arb-polymarket/src/auth.rs` — verify it matches the docs

**Order Placement (for future live trading):**
- What is the order format? (signed order with EIP-712)
- How does the CLOB handle limit orders?
- What is the tick size system?
- How are orders matched?

### 3. Gamma API vs CLOB API

Currently we fetch markets from Gamma API (`gamma-api.polymarket.com/markets`). Understand:
- Gamma API = high-level market metadata (questions, categories, conditions)
- CLOB API = trading data (order books, prices, orders)
- Which should we use for what?
- Can we get real-time prices from CLOB without WebSocket? (REST polling)

---

## Current State — What Exists

### Working:
- `crates/arb-polymarket/src/connector.rs` — PolymarketConnector with all 11 trait methods
- `crates/arb-polymarket/src/client.rs` — REST client for Gamma + CLOB APIs
- `crates/arb-polymarket/src/ws.rs` — WebSocket client with reconnection logic
- `crates/arb-polymarket/src/auth.rs` — HMAC-SHA256 L2 auth
- `crates/arb-polymarket/src/signing.rs` — EIP-712 L1 signing
- `crates/arb-polymarket/src/types.rs` — Response types for API deserialization

### Broken / Incomplete:
1. **`client.rs:fetch_markets()`** — fetches from Gamma API but `outcomePrices` field wasn't being parsed (fixed with fallback, but `tokens` array is empty from Gamma)
2. **`ws.rs` WebSocket** — connects to wrong URL (404), subscribes with wrong ID format (uses condition IDs, needs token IDs)
3. **`connector.rs:get_order_book()`** — calls CLOB API with condition ID, but CLOB uses token IDs
4. **`connector.rs:subscribe_prices()`** — passes condition IDs to WebSocket, needs token IDs
5. **Token ID mapping** — nowhere in the code do we extract `clobTokenIds` from Gamma API and map them for CLOB/WS use
6. **`types.rs:PolyMarketResponse`** — missing `clobTokenIds` field for token ID extraction

### Current Workaround in main.rs:
- HTTP polling every 8s from Gamma API to get prices (works but slow, not real-time)
- Kalshi is fully mocked
- Engine works with condition IDs as market identifiers

---

## What Needs to Be Built

### Phase 6-A: Token ID Mapping

**Research:** Read the Polymarket docs to understand conditionId → tokenId mapping.

**Build:**
1. Add `clob_token_ids` field to `PolyMarketResponse` in `types.rs`
2. Parse the `clobTokenIds` JSON string field from Gamma API (`"[\"123...\", \"456...\"]"`)
3. Create a `TokenMap` struct that maps: `conditionId → (yes_token_id, no_token_id)`
4. Populate this map when fetching markets from Gamma API
5. Store token IDs in the DB (`poly_yes_token_id`, `poly_no_token_id` fields already exist but are empty)

### Phase 6-B: Fix CLOB Order Book

**Research:** Read the CLOB API docs for the order book endpoint.

**Build:**
1. Fix `client.rs` to fetch order book using token ID (not condition ID)
2. The order book endpoint is likely: `GET https://clob.polymarket.com/book?token_id={token_id}`
3. Parse the CLOB order book response format (may differ from current `OrderBook` type)
4. Update `connector.rs:get_order_book()` to use the token ID from the map

### Phase 6-C: Fix WebSocket Price Feed

**Research:** Read the WebSocket docs for correct URL and subscription format.

**Build:**
1. Fix the WebSocket URL (likely needs `/market` suffix or different path)
2. Fix the subscription message to use correct field names and token IDs
3. Test that price updates flow through: WS → parse → PriceUpdate → PriceCache → Detector
4. Remove the HTTP polling workaround from main.rs once WS works

### Phase 6-D: Fix Market Fetching

**Research:** Understand the best way to discover active, liquid markets.

**Build:**
1. Fix `fetch_markets()` to properly parse all fields including `clobTokenIds`
2. Add filtering for liquid markets (volume > X, liquidity > Y)
3. Store token IDs when inserting market pairs into DB
4. Update `connector.rs:list_markets()` to return fully populated Market structs

### Phase 6-E: Startup Wiring Cleanup

**Build:**
1. Remove the direct HTTP polling from `main.rs` — should go through the connector
2. Use proper `poly.subscribe_prices()` with token IDs
3. Remove the `reqwest` direct dependency from `arb-cli` (all API calls go through `arb-polymarket`)
4. Clean up the market seeding — use connector methods instead of raw HTTP

---

## Key Files to Read

| File | What It Does | What's Wrong |
|------|-------------|-------------|
| `crates/arb-polymarket/src/client.rs` | REST client for Gamma + CLOB | Missing token ID support, order book uses wrong IDs |
| `crates/arb-polymarket/src/ws.rs` | WebSocket client | Wrong URL, wrong subscription format |
| `crates/arb-polymarket/src/connector.rs` | Trait implementation | Passes condition IDs where token IDs needed |
| `crates/arb-polymarket/src/types.rs` | API response types | Missing `clobTokenIds`, `outcomePrices` fallback is hacky |
| `crates/arb-polymarket/src/auth.rs` | HMAC auth | Probably correct but verify against docs |
| `crates/arb-polymarket/src/signing.rs` | EIP-712 signing | Verify against docs |
| `crates/arb-cli/src/main.rs` | Startup wiring | Has workaround HTTP polling that should be removed |
| `config/default.toml` | Config | WebSocket URL may be wrong |

## Environment

- `.env` has: `POLY_PRIVATE_KEY`, `POLY_API_KEY`, `POLY_API_SECRET`, `POLY_PASSPHRASE` (DO NOT READ THIS FILE)
- `config/default.toml` has API URLs
- Kalshi is mocked (approval pending) — don't touch Kalshi code

## Acceptance Criteria

- [ ] Token IDs extracted from Gamma API and stored in DB
- [ ] Order book fetched from CLOB using correct token IDs
- [ ] WebSocket connects and receives real-time price updates
- [ ] Prices in TUI update in real-time (not just every 8s)
- [ ] No direct HTTP calls in main.rs — all through connector
- [ ] `cargo test --workspace` still passes
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] Paper trading works with real-time Polymarket prices

## Verification

```bash
# After building:
cargo run -- --paper --headless  # Should show real-time price updates in logs
cargo run -- --paper --tui       # TUI should show prices updating live
cargo test --workspace
cargo clippy --workspace -- -D warnings
```
