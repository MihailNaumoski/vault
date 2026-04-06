# Kalshi API Research Handoff

*Research Lead | 2026-04-06 | Session: multi-team/sessions/2026-04-06T21-03-54*

## Executive Summary

The existing `arb-kalshi` crate has **4 BREAKING issues** that will prevent it from working against the real Kalshi API, plus **8 SUBOPTIMAL issues** that will cause failures as the API completes its price format migration. The crate's core architecture (REST client, WebSocket, rate limiting, connector trait) is solid, but the protocol details are based on outdated or incorrect assumptions.

---

## BREAKING Issues (Must Fix Before Any Testing)

### B1. Wrong Signing Algorithm -- RSA-PSS required, not PKCS1v15
**File**: `arb-kalshi/src/auth.rs`
**Current**: `rsa::pkcs1v15::SigningKey<Sha256>`
**Required**: `rsa::pss::SigningKey<Sha256>` with `Pss::new_with_salt::<Sha256>(SHA256_DIGEST_LENGTH)`

The official Python starter code confirms RSA-PSS:
```python
padding.PSS(mgf=padding.MGF1(hashes.SHA256()), salt_length=padding.PSS.DIGEST_LENGTH)
```

**Impact**: Every authenticated request will return 401 Unauthorized.

**Fix**:
```rust
// Replace:
use rsa::pkcs1v15::SigningKey;
// With:
use rsa::pss::{BlindedSigningKey, Signature};

// In sign_with_timestamp:
let signing_key = BlindedSigningKey::<Sha256>::new(private_key);
let sig = signing_key.sign_with_rng(&mut rand::thread_rng(), message.as_bytes());
```

Note: RSA-PSS is non-deterministic (uses randomness in salt), so the same input will produce different signatures each time. Update tests accordingly -- signature verification tests still work, but determinism tests must be removed.

### B2. Wrong Base URLs
**File**: `arb-kalshi/src/types.rs`

| | Current (Wrong) | Correct |
|---|---|---|
| REST | `https://trading-api.kalshi.com/trade-api/v2` | `https://api.elections.kalshi.com/trade-api/v2` |
| WebSocket | `wss://trading-api.kalshi.com/trade-api/ws/v2` | `wss://api.elections.kalshi.com/trade-api/ws/v2` |

**Impact**: DNS resolution failure or HTTP redirect.

### B3. Wrong WebSocket Authentication Flow
**File**: `arb-kalshi/src/ws.rs`
**Current**: Connects to WS, then sends a JSON `{"cmd":"subscribe","params":{"channels":["auth"],...}}` message.
**Required**: Pass auth headers (`KALSHI-ACCESS-KEY`, `KALSHI-ACCESS-SIGNATURE`, `KALSHI-ACCESS-TIMESTAMP`) as HTTP headers during the WebSocket upgrade handshake.

From official Python SDK:
```python
auth_headers = self.request_headers("GET", "/trade-api/ws/v2")
websockets.connect(host, additional_headers=auth_headers)
```

**Fix** for `tokio-tungstenite`:
```rust
use tokio_tungstenite::tungstenite::http::Request;

let auth_headers = auth.headers("GET", "/trade-api/ws/v2")?;
let mut request = Request::builder()
    .uri(url)
    .header("KALSHI-ACCESS-KEY", auth_headers.get("KALSHI-ACCESS-KEY").unwrap())
    .header("KALSHI-ACCESS-SIGNATURE", auth_headers.get("KALSHI-ACCESS-SIGNATURE").unwrap())
    .header("KALSHI-ACCESS-TIMESTAMP", auth_headers.get("KALSHI-ACCESS-TIMESTAMP").unwrap())
    .body(())?;
let (ws_stream, _) = tokio_tungstenite::connect_async(request).await?;
```

Remove `build_auth_message()` function and the auth step in `connect_and_run()`.

### B4. Wrong WebSocket Message Envelope
**File**: `arb-kalshi/src/types.rs`, `arb-kalshi/src/ws.rs`
**Current**: Expects flat messages like `{"type":"orderbook_delta","market_ticker":"...","yes":[...]}`
**Required**: Messages are wrapped in an envelope:
```json
{"type": "orderbook_delta", "sid": 2, "seq": 3, "msg": { ... }}
```

**Fix**: Add envelope wrapper:
```rust
#[derive(Debug, Deserialize)]
pub struct KalshiWsEnvelope {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub sid: Option<u64>,
    pub seq: Option<u64>,
    pub msg: Option<serde_json::Value>,
    // For error responses
    pub id: Option<u64>,
}
```

Then parse `msg` based on `msg_type` into the appropriate inner type.

---

## SUBOPTIMAL Issues (Fix Before Production)

### S1. Price Format Migration (Integer Cents -> Dollar Strings)
**Files**: `types.rs`, `connector.rs`, `client.rs`, `ws.rs`

Legacy integer cents are being removed. Current API responses use both formats during transition:
- REST orderbook: `orderbook_fp.yes_dollars` (string arrays), not `orderbook.yes` (int arrays)
- Orders: `yes_price_dollars` / `no_price_dollars` (strings), not `yes_price` / `no_price` (ints)
- Positions: `position_fp` (string), `total_traded_dollars` (string)
- WS messages: All prices as dollar strings

**Priority**: High -- legacy fields being removed March 2026.

### S2. Orderbook Delta Structure
**File**: `ws.rs`
**Current**: Expects full-book delta `{"yes": [[42, 100]], "no": [[58, 100]]}`
**Required**: Per-price-level delta `{"price_dollars": "0.960", "delta_fp": "-54.00", "side": "yes"}`

Need to maintain local orderbook state and apply deltas incrementally.

### S3. Ticker Message Fields
**File**: `types.rs`
**Current**: `yes_price: u32, no_price: u32, volume: u64`
**Required**: `price_dollars: String, yes_bid_dollars: String, yes_ask_dollars: String, volume_fp: String, open_interest_fp: String, yes_bid_size_fp: String, yes_ask_size_fp: String, last_trade_size_fp: String, ts: u64, time: String`

### S4. Fill Message Fields
**File**: `types.rs`
**Current**: `order_id, count, remaining_count, side, yes_price, no_price`
**Required**: `trade_id, order_id, market_ticker, is_taker, side, yes_price_dollars, count_fp, fee_cost, action, ts, client_order_id, post_position_fp, purchased_side, subaccount`

### S5. Positions Response Fields
**File**: `types.rs`
**Current**: `ticker, market_exposure, position: i32, resting_orders_count, total_cost`
**Required**: `ticker, total_traded_dollars, position_fp, market_exposure_dollars, realized_pnl_dollars, fees_paid_dollars, last_updated_ts`
Note: `resting_orders_count` was removed Nov 2025.

### S6. Order Response Fields
**File**: `types.rs`
**Current**: `remaining_count: u32, filled_count: u32, yes_price: u32`
**Required**: `remaining_count_fp, fill_count_fp, initial_count_fp, yes_price_dollars, no_price_dollars, taker_fees_dollars, maker_fees_dollars, created_time, last_update_time`

### S7. Rate Limit Classification
**File**: `rate_limit.rs`
**Current**: All `/portfolio/` paths classified as trading (10 req/s)
**Required**: Only mutations (POST/DELETE to orders) are write-limited (10/s). GET requests to `/portfolio/` are read-limited (20/s for Basic tier).

### S8. Missing Orderbook Snapshot Handling
**File**: `ws.rs`
The WS sends an `orderbook_snapshot` message before deltas. Our code has no handling for this message type. Need to add `OrderbookSnapshot` variant.

---

## Missing Features (Nice to Have)

### M1. Batch Order Operations
- `POST /portfolio/orders/batched` (up to 20 orders per batch)
- `DELETE /portfolio/orders/batched`
- Each order in batch counts individually against rate limit

### M2. Order Amend/Decrease
- `POST /portfolio/orders/{id}/amend`
- `POST /portfolio/orders/{id}/decrease`

### M3. Additional WebSocket Channels
- `trade` -- public trade feed
- `market_positions` -- real-time position updates
- `user_orders` -- real-time order status updates
- `market_lifecycle_v2` -- market open/close/settle events

### M4. Sequence Number Tracking
WS messages include `seq` for gap detection. Should track per subscription.

### M5. Subscription Management
- `update_subscription` -- add/remove markets without resubscribing
- `list_subscriptions` -- query active subscriptions
- `unsubscribe` -- clean unsubscribe by `sid`

### M6. Events and Series Endpoints
- `GET /events`, `GET /events/{event_ticker}`
- Series and candlestick endpoints
- Useful for market discovery

---

## Complete WebSocket Protocol Reference

### Connection
```
URL: wss://api.elections.kalshi.com/trade-api/ws/v2
Auth: HTTP headers during upgrade
  KALSHI-ACCESS-KEY: <key_id>
  KALSHI-ACCESS-SIGNATURE: base64(RSA-PSS-SHA256(timestamp_ms + "GET" + "/trade-api/ws/v2"))
  KALSHI-ACCESS-TIMESTAMP: <timestamp_ms>
```

### Subscribe
```json
{
  "id": 1,
  "cmd": "subscribe",
  "params": {
    "channels": ["orderbook_delta"],
    "market_tickers": ["TICKER-1", "TICKER-2"]
  }
}
```
Optional: `send_initial_snapshot`, `skip_ticker_ack`, `shard_factor`, `shard_key`

### Subscribe Confirmation
```json
{"id": 1, "type": "subscribed", "msg": {"channel": "orderbook_delta", "sid": 1}}
```

### Unsubscribe
```json
{"id": 2, "cmd": "unsubscribe", "params": {"sids": [1]}}
```

### Error
```json
{"id": 123, "type": "error", "msg": {"code": 6, "msg": "Already subscribed"}}
```

### Available Channels (11)
| Channel | Auth Required | Description |
|---|---|---|
| `orderbook_delta` | Yes (connection) | Snapshots + deltas |
| `ticker` | Yes (connection) | Market price/volume updates |
| `trade` | Yes (connection) | Public trade feed |
| `fill` | Yes (connection) | User fill notifications |
| `market_positions` | Yes (connection) | Position updates |
| `user_orders` | Yes (connection) | Order status updates |
| `market_lifecycle_v2` | Yes (connection) | Lifecycle events |
| `multivariate_market_lifecycle` | Yes (connection) | MVE lifecycle |
| `multivariate` | Yes (connection) | MVE events |
| `communications` | Yes (connection) | RFQ/quotes |
| `order_group_updates` | Yes (connection) | Order group notifications |

All channels require an authenticated connection. Some carry public data but the connection itself must be authenticated.

---

## Complete REST Endpoint Reference

### Market Data (Read-limited)
| Method | Path | Our Crate | Notes |
|---|---|---|---|
| GET | `/markets` | `fetch_markets()` | Correct path |
| GET | `/markets/{ticker}` | `fetch_market()` | Correct path |
| GET | `/markets/{ticker}/orderbook` | `fetch_order_book()` | Response format changed |
| GET | `/markets/orderbooks` | Missing | Multiple orderbooks in one call |
| GET | `/markets/trades` | Missing | Public trade history |
| GET | `/events` | Missing | Event listing |
| GET | `/events/{event_ticker}` | Missing | Event detail |

### Trading (Write-limited for mutations)
| Method | Path | Our Crate | Notes |
|---|---|---|---|
| POST | `/portfolio/orders` | `post_order()` | Request format needs update |
| GET | `/portfolio/orders` | `fetch_open_orders()` | Response format needs update |
| GET | `/portfolio/orders/{id}` | `fetch_order()` | Response format needs update |
| DELETE | `/portfolio/orders/{id}` | `cancel_order()` | Correct |
| POST | `/portfolio/orders/batched` | Missing | Batch create |
| DELETE | `/portfolio/orders/batched` | Missing | Batch cancel |
| POST | `/portfolio/orders/{id}/amend` | Missing | Amend order |
| POST | `/portfolio/orders/{id}/decrease` | Missing | Decrease order |
| GET | `/portfolio/positions` | `fetch_positions()` | Fields changed |
| GET | `/portfolio/balance` | `fetch_balance()` | Correct (still cents) |
| GET | `/portfolio/settlements` | Missing | Settlement history |
| GET | `/portfolio/fills` | Missing | Fill history |

---

## Authentication Reference

### Algorithm
RSA-PSS with SHA-256 (MGF1-SHA256, salt_length = SHA256_DIGEST_LENGTH = 32)

### Signing Message
```
message = str(timestamp_ms) + HTTP_METHOD + path_without_query_params
```
Example: `"1703123456789GET/trade-api/v2/portfolio/balance"`

Important: Query parameters are EXCLUDED from the signed path.

### Headers
```
KALSHI-ACCESS-KEY: <api_key_id>
KALSHI-ACCESS-SIGNATURE: base64(<RSA-PSS signature>)
KALSHI-ACCESS-TIMESTAMP: <timestamp_ms as string>
Content-Type: application/json
```

### Private Key Format
PEM-encoded RSA private key. The docs say "RSA_PRIVATE_KEY format" which could be PKCS#1 (`-----BEGIN RSA PRIVATE KEY-----`) or PKCS#8 (`-----BEGIN PRIVATE KEY-----`). Our code assumes PKCS#8. The official Python SDK uses `serialization.load_pem_private_key()` which handles both formats. Consider supporting both.

---

## Rate Limits Reference

| Tier | Read/s | Write/s |
|---|---|---|
| Basic | 20 | 10 |
| Advanced | 30 | 30 |
| Premier | 100 | 100 |
| Prime | 400 | 400 |

**Write endpoints**: CreateOrder, CancelOrder, AmendOrder, DecreaseOrder, BatchCreateOrders, BatchCancelOrders
**All other endpoints**: Read-limited

**WebSocket**: Connection limit per user by tier (default 200).

Use `GET /account/limits` to check current tier programmatically.

---

## Implementation Checklist for Engineering

### Phase 1: Fix Breaking Issues (Required)
- [ ] **B1**: Replace PKCS1v15 signing with RSA-PSS in `auth.rs`
  - Use `rsa::pss::BlindedSigningKey<Sha256>` 
  - Update tests (signatures are now non-deterministic)
  - Verify with known test vectors if available
- [ ] **B2**: Update base URLs in `types.rs`
  - REST: `https://api.elections.kalshi.com/trade-api/v2`
  - WS: `wss://api.elections.kalshi.com/trade-api/ws/v2`
- [ ] **B3**: Fix WebSocket auth in `ws.rs`
  - Remove `build_auth_message()` and auth channel subscription
  - Pass auth headers in HTTP upgrade request
  - Sign with `"GET" + "/trade-api/ws/v2"`
- [ ] **B4**: Add WebSocket message envelope parsing
  - New `KalshiWsEnvelope` type with `type`, `sid`, `seq`, `msg`
  - Route inner `msg` to appropriate handler based on `type`

### Phase 2: Price Format Migration (High Priority)
- [ ] **S1**: Add dollar-string fields to all types
  - `yes_price_dollars: Option<String>` alongside `yes_price: Option<u32>`
  - Prefer dollar strings when available, fall back to cents
- [ ] **S2**: Update orderbook delta handling
  - Handle `orderbook_snapshot` messages
  - Handle per-level deltas (not full-book)
  - Maintain local orderbook state
- [ ] **S3**: Update ticker message parsing
- [ ] **S4**: Update fill message parsing
- [ ] **S5**: Update positions response parsing
- [ ] **S6**: Update order response parsing

### Phase 3: Rate Limit Fixes (Medium Priority)
- [ ] **S7**: Fix rate limit classification
  - Only POST/DELETE to order endpoints = write-limited
  - GET /portfolio/* = read-limited
  - Make limits configurable per tier

### Phase 4: Missing Features (Low Priority)
- [ ] **M1**: Batch order operations
- [ ] **M2**: Order amend/decrease
- [ ] **M3**: Additional WS channels (trade, user_orders, market_positions)
- [ ] **M4**: Sequence number tracking
- [ ] **M5**: Subscription management (update, list, unsubscribe)
- [ ] **M6**: Events/series endpoints

---

## Key Lessons Learned

1. **Docs + SDK cross-reference is essential**: The official Python starter code (`Kalshi/kalshi-starter-code-python/clients.py`) was the definitive source for auth implementation details.
2. **RSA-PSS vs PKCS1v15 is a critical distinction**: These produce incompatible signatures. Always verify the exact padding scheme.
3. **WebSocket auth varies by exchange**: Kalshi uses HTTP upgrade headers, not a JSON auth message. Polymarket uses a JSON auth message. Never assume.
4. **Price format migrations are in progress**: The API is mid-migration from integer cents to dollar strings. Code must handle both during the transition period.
5. **Rate limits are tier-dependent**: Hardcoding a single rate limit is insufficient. Need configurable limits or auto-detection via `GET /account/limits`.

---

*Sources: [Kalshi API Docs](https://docs.kalshi.com), [Official Python Starter](https://github.com/Kalshi/kalshi-starter-code-python), [kalshi-rs](https://github.com/arvchahal/kalshi-rs), [API Changelog](https://docs.kalshi.com/changelog)*
