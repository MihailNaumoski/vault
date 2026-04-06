# Phase 1 Context

**Generated:** 2026-04-06
**Phase:** 1 of 2
**Task:** Phase 6 — Polymarket API Integration Fix

---

## 1. Task Summary

Fix the Polymarket API integration across the arb-polymarket crate. Five sub-tasks:
token ID mapping (conditionId → clobTokenIds → tokenId), CLOB order book fetching
(must use tokenId, not conditionId), WebSocket price feeds (correct URL and subscription
format), market data parsing improvements, and removal of the HTTP polling workaround
from main.rs.

## 2. Phase Scope

Research Polymarket CLOB API docs. Determine correct token ID mapping flow, WebSocket URL
and subscription format, CLOB REST endpoints, and auth header requirements. Produce:
- `phases/phase-1/plan.md` — architecture plan
- `phases/phase-1/spec.md` — numbered acceptance criteria covering all 5 sub-tasks

## 3. Previous Phase Output

This is Phase 1 — no prior output.

## 4. Current Codebase State

### 4a. Changed Files (Since Last Phase)

8 files modified, 540 insertions / 160 deletions (all uncommitted on top of one-commit history):

```
Cargo.lock
config/default.toml
crates/arb-cli/Cargo.toml
crates/arb-cli/src/main.rs          (+270 lines — HTTP polling hack added here)
crates/arb-cli/src/tui.rs           (+200 lines — dashboard expansion)
crates/arb-polymarket/src/client.rs (+4 lines)
crates/arb-polymarket/src/connector.rs (+14 lines)
crates/arb-polymarket/src/types.rs  (+17 lines)
```

**Key hack in main.rs (lines 559–626):** A `tokio::spawn` loop polls
`gamma_url/markets?...` every 8 seconds via raw HTTP, extracts `outcomePrices`, and
sends `PriceUpdate` events manually. This bypasses `PolymarketConnector::subscribe_prices`
entirely. The loop uses condition IDs as `market_id` for price updates, but the price
cache and engine expect token IDs for CLOB operations. Token ID fields (`poly_yes_token_id`,
`poly_no_token_id`) are seeded as empty strings in `MarketPairRow`.

### 4b. Relevant Source Structure

```
crates/arb-polymarket/src/
  auth.rs          — HMAC-SHA256 auth header builder
  client.rs        — REST client (fetch_markets, fetch_order_book, fetch_price, post_order, …)
  connector.rs     — PredictionMarketConnector impl (delegates to client + ws)
  error.rs         — PolymarketError enum
  lib.rs           — crate re-exports
  mock.rs          — mock connector for tests
  rate_limit.rs    — token-bucket rate limiter (governor)
  signing.rs       — EIP-712 order signing (alloy)
  types.rs         — PolyConfig, PolyMarketResponse, PolyToken, PolyBookResponse,
                     PolyWsMessage, conversion helpers
  ws.rs            — WebSocket client with exponential backoff reconnect

crates/arb-cli/src/
  main.rs          — startup wiring, HTTP polling hack (lines 559–626)
  tui.rs           — terminal dashboard
```

### 4c. Dependencies and Integrations

**Key crates (arb-polymarket):**
- `reqwest` — HTTP client
- `tokio-tungstenite` — WebSocket
- `alloy-signer`, `alloy-signer-local`, `alloy-sol-types` — EIP-712 signing
- `hmac`, `sha2`, `base64`, `hex` — CLOB auth
- `governor`, `parking_lot` — rate limiting
- `rust_decimal`, `chrono`, `serde_json`, `uuid`, `rand`

**API URLs (config/default.toml):**
- CLOB REST: `https://clob.polymarket.com`
- Gamma API: `https://gamma-api.polymarket.com`
- WebSocket: `wss://ws-subscriptions-clob.polymarket.com/ws` ← **WRONG** (missing `/market` suffix)

**Default WS URL in types.rs:** `wss://ws-subscriptions-clob.polymarket.com/ws/market` (correct)
**Config file overrides it to:** `wss://ws-subscriptions-clob.polymarket.com/ws` (broken)

**WS subscription message format currently sent (ws.rs line 104–108):**
```json
{"type": "subscribe", "channel": "market", "assets_ids": ["<token_id>"]}
```
Needs verification against actual CLOB docs — key name may be `assets_ids` vs `asset_ids`.

**connector.rs `get_order_book`:** passes `id` (which callers supply as condition ID from
`PairInfo.poly_market_id`) directly to `fetch_order_book`. The CLOB `/book` endpoint
requires a token ID, not a condition ID — this will 404 or return wrong data.

## 5. Relevant Specs

No prior specs — Planning will produce them.

## 6. Risk Flags

1. **Token ID mapping gap (critical):** `poly_yes_token_id` / `poly_no_token_id` stored as
   empty strings in DB. Order book, price feed, and order placement all need token IDs.
   Gamma API `GET /markets/{conditionId}` returns `tokens[].tokenId` — mapping must be
   extracted and persisted on pair creation.

2. **WebSocket URL mismatch:** config.toml overrides the correct default in types.rs.
   Config value is missing the `/market` path segment.

3. **HTTP polling is condition-ID-keyed:** Price updates sent with `market_id = condition_id`
   but CLOB order books and order placement require `tokenId`. Cache/engine lookups may
   silently misfire.

4. **Secrets in .env — DO NOT READ:** `POLY_API_KEY`, `POLY_API_SECRET`, `POLY_PASSPHRASE`,
   `POLY_PRIVATE_KEY` are injected from environment. The `.env` file must not be read.

5. **Single-commit history:** Only one prior commit. All 540-line diff is uncommitted —
   no rollback point beyond the initial commit.

6. **connector.rs `place_limit_order`:** uses `req.market_id` as the token_id. Callers
   must ensure `market_id` is a token ID, not a condition ID — currently inconsistent.

## 7. Context Confidence

**HIGH** — All primary source files read directly. Git diff confirms exact changed files.
Config URLs confirmed. DB schema confirms token ID fields exist but are empty on insert.
One uncertainty: exact CLOB WebSocket subscription field name (`assets_ids` vs `asset_ids`)
requires docs verification which Planning must resolve.
