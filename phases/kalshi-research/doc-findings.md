# Kalshi API Documentation Findings

*Researched 2026-04-06 by Doc Researcher*

## Sources
- [Kalshi API Docs - Welcome](https://docs.kalshi.com/welcome)
- [WebSocket Connection](https://docs.kalshi.com/websockets/websocket-connection)
- [Orderbook Updates](https://docs.kalshi.com/websockets/orderbook-updates)
- [Market Ticker Channel](https://docs.kalshi.com/websockets/market-ticker)
- [User Fills Channel](https://docs.kalshi.com/websockets/user-fills)
- [Public Trades Channel](https://docs.kalshi.com/websockets/public-trades)
- [Quick Start: Authenticated Requests](https://docs.kalshi.com/getting_started/quick_start_authenticated_requests)
- [Quick Start: WebSockets](https://docs.kalshi.com/getting_started/quick_start_websockets)
- [Rate Limits](https://docs.kalshi.com/getting_started/rate_limits)
- [API Keys](https://docs.kalshi.com/getting_started/api_keys)
- [Orderbook Responses Guide](https://docs.kalshi.com/getting_started/orderbook_responses)
- [API Changelog](https://docs.kalshi.com/changelog)
- [Create Order](https://docs.kalshi.com/api-reference/orders/create-order)
- [Batch Create Orders](https://docs.kalshi.com/api-reference/orders/batch-create-orders)
- [Get Orders](https://docs.kalshi.com/api-reference/orders/get-orders)
- [Get Positions](https://docs.kalshi.com/api-reference/portfolio/get-positions)
- [Get Balance](https://docs.kalshi.com/api-reference/portfolio/get-balance)
- [Get Market Orderbook](https://docs.kalshi.com/api-reference/market/get-market-orderbook)
- [Python SDK - Markets](https://docs.kalshi.com/python-sdk/api/MarketsApi)
- [OpenAPI Spec](https://docs.kalshi.com/openapi.yaml)

---

## 1. Base URLs

| Environment | REST | WebSocket |
|---|---|---|
| **Production** | `https://api.elections.kalshi.com/trade-api/v2` | `wss://api.elections.kalshi.com/trade-api/ws/v2` |
| **Demo** | `https://demo-api.kalshi.co/trade-api/v2` | `wss://demo-api.kalshi.co/trade-api/ws/v2` |

**CRITICAL**: Our code uses `https://trading-api.kalshi.com/trade-api/v2` -- this is the OLD domain. The current production domain is `api.elections.kalshi.com`.

---

## 2. Authentication

### Signing Algorithm: RSA-PSS with SHA-256

The docs explicitly state RSA-PSS, not PKCS1v15. The official Python starter code uses:
```python
padding.PSS(
    mgf=padding.MGF1(hashes.SHA256()),
    salt_length=padding.PSS.DIGEST_LENGTH
)
```

### What Gets Signed
Message = `timestamp_ms_string + HTTP_METHOD + path` (no query parameters, no body)

Example: `1703123456789GET/trade-api/v2/portfolio/balance`

**Important**: Query parameters are explicitly EXCLUDED. `/portfolio/orders?limit=5` signs as `/trade-api/v2/portfolio/orders`.

### Headers (3 required)
| Header | Value |
|---|---|
| `KALSHI-ACCESS-KEY` | API key ID string |
| `KALSHI-ACCESS-SIGNATURE` | Base64-encoded RSA-PSS signature |
| `KALSHI-ACCESS-TIMESTAMP` | Millisecond Unix timestamp as string |

### WebSocket Authentication
Auth happens via headers during the WebSocket handshake (HTTP Upgrade), NOT via a JSON message after connection. The same three headers are included in the initial connection request.

### API Key Scopes (Dec 2025+)
Keys support `read` and `write` scopes. Existing keys default to full access.

---

## 3. Rate Limits

| Tier | Read (req/s) | Write (req/s) |
|---|---|---|
| Basic | 20 | 10 |
| Advanced | 30 | 30 |
| Premier | 100 | 100 |
| Prime | 400 | 400 |

**Write-limited endpoints only:**
- CreateOrder
- CancelOrder
- AmendOrder
- DecreaseOrder
- BatchCreateOrders (1 tx per item)
- BatchCancelOrders (0.2 tx per cancel)

All other endpoints are read-limited.

**Our code assumption**: 10 req/s trading, 100 req/s market data. This matches Basic tier write / Premier tier read. May need to be configurable per tier.

### Rate Limit Headers
The docs reference `Ratelimit-Remaining` and `Ratelimit-Reset` headers (our code already logs these).

### WebSocket Connection Limits
Sep 2025: WebSocket connections per user limited by tier (default 200).

---

## 4. REST Endpoints

### Market Data (Read-limited)

| Method | Path | Description |
|---|---|---|
| GET | `/markets` | List markets (paginated, filterable) |
| GET | `/markets/{ticker}` | Get single market |
| GET | `/markets/{ticker}/orderbook` | Get orderbook |
| GET | `/markets/orderbooks` | Get multiple orderbooks |
| GET | `/markets/trades` | Get trades |
| GET | `/events` | List events |
| GET | `/events/multivariate` | List multivariate events |
| GET | `/events/{event_ticker}` | Get single event |
| GET | `/events/{event_ticker}/metadata` | Get event metadata |
| GET | `/series/{series_ticker}/markets/{ticker}/candlesticks` | Market candlesticks |
| GET | `/exchange/status` | Exchange status |
| GET | `/exchange/schedule` | Exchange schedule |

### Portfolio/Trading (Write-limited for mutations)

| Method | Path | Description |
|---|---|---|
| GET | `/portfolio/orders` | List orders |
| POST | `/portfolio/orders` | Create order |
| GET | `/portfolio/orders/{order_id}` | Get order |
| DELETE | `/portfolio/orders/{order_id}` | Cancel order |
| POST | `/portfolio/orders/batched` | Batch create orders |
| DELETE | `/portfolio/orders/batched` | Batch cancel orders |
| POST | `/portfolio/orders/{order_id}/amend` | Amend order |
| POST | `/portfolio/orders/{order_id}/decrease` | Decrease order |
| GET | `/portfolio/orders/queue_positions` | Get queue positions |
| GET | `/portfolio/balance` | Get balance |
| GET | `/portfolio/positions` | Get positions |
| GET | `/portfolio/settlements` | Get settlements |
| GET | `/portfolio/fills` | Get fills |

### Missing from our crate
- Batch create/cancel orders
- Amend order
- Decrease order
- Queue positions
- Settlements
- Fills (REST)
- Events endpoints
- Candlesticks
- Exchange status/schedule
- Order groups
- Subaccounts
- Communications (RFQ/quotes)

---

## 5. Price Format - BREAKING CHANGE

### Current API (2026)
The API is migrating from integer cents to **fixed-point dollar strings**.

**REST orderbook response (current format):**
```json
{
  "orderbook_fp": {
    "yes_dollars": [["0.1500", "100.00"]],
    "no_dollars": [["0.2500", "50.00"]]
  }
}
```

**Create order accepts BOTH formats (transitional):**
- `yes_price` / `no_price` (integer, 1-99 cents) -- LEGACY
- `yes_price_dollars` / `no_price_dollars` (string, up to 6 decimals) -- NEW
- `count` (integer) -- LEGACY
- `count_fp` (string, 2 decimals) -- NEW

**Balance endpoint still returns integer cents:**
```json
{
  "balance": 150000,
  "portfolio_value": 0,
  "updated_ts": 0
}
```

### Changelog timeline
- Aug 2025: Subpenny pricing fields added alongside cent-based fields
- Jan 2026: Subaccount balance returns fixed-point dollars
- Mar 2026: Legacy integer cents fields removal from REST and WebSocket
- Mar 2026: `yes_price_fixed`/`no_price_fixed` removed from fills

**Our code uses integer cents everywhere. This needs migration to dollar strings.**

---

## 6. REST Response Formats

### Get Market Response
Key fields (from docs):
- `ticker`, `title`/`question`, `status`, `close_time`
- `yes_ask`, `yes_bid`, `no_ask`, `no_bid` (legacy cents)
- `volume`, `open_interest`, `liquidity`
- Plus new dollar-string variants

### Get Positions Response (Updated Nov 2025+)
```json
{
  "cursor": "string",
  "market_positions": [
    {
      "ticker": "string",
      "total_traded_dollars": "0.5600",
      "position_fp": "10.00",
      "market_exposure_dollars": "0.5600",
      "realized_pnl_dollars": "0.5600",
      "fees_paid_dollars": "0.5600",
      "last_updated_ts": "2024-01-01T00:00:00Z"
    }
  ]
}
```
**Removed fields**: `resting_orders_count` (deprecated), `position_cost`, `realized_pnl`, `fees_paid`, `position_fee_cost`

### Get Orders Response
```json
{
  "orders": [...],
  "cursor": "string"
}
```
Order fields: `order_id`, `user_id`, `client_order_id`, `ticker`, `side`, `action`, `type`, `status`, `yes_price_dollars`, `no_price_dollars`, `initial_count_fp`, `fill_count_fp`, `remaining_count_fp`, `taker_fees_dollars`, `maker_fees_dollars`, `taker_fill_cost_dollars`, `maker_fill_cost_dollars`, `expiration_time`, `created_time`, `last_update_time`, `self_trade_prevention_type`, `order_group_id`, `cancel_order_on_pause`, `subaccount_number`

### Get Balance Response
```json
{
  "balance": 150000,
  "portfolio_value": 0,
  "updated_ts": 0
}
```
Balance and portfolio_value are in **cents** (int64).

---

## 7. WebSocket Protocol

### Connection
URL: `wss://api.elections.kalshi.com/trade-api/ws/v2`
Auth: Via headers during HTTP upgrade handshake (same 3 headers as REST)

**NOT via JSON message after connection** -- this is different from what our code does.

### Available Channels (11)
| Channel | Type | Description |
|---|---|---|
| `orderbook_delta` | Private | Orderbook snapshots + deltas |
| `ticker` | Public | Market ticker updates |
| `trade` | Public | Public trade feed |
| `fill` | Private | User fill notifications |
| `market_positions` | Private | Position updates |
| `market_lifecycle_v2` | Public | Market lifecycle events |
| `multivariate_market_lifecycle` | Public | MVE lifecycle events |
| `multivariate` | Public | Multivariate events |
| `communications` | Private | RFQ/quote streams |
| `order_group_updates` | Private | Order group notifications |
| `user_orders` | Private | User order updates |

### Subscription Format
```json
{
  "id": 1,
  "cmd": "subscribe",
  "params": {
    "channels": ["orderbook_delta"],
    "market_ticker": "CPI-22DEC-TN0.1"
  }
}
```

Optional params: `send_initial_snapshot` (bool), `skip_ticker_ack` (bool), `shard_factor` (int), `shard_key` (int)

Market can be specified as:
- `market_ticker` (single string)
- `market_tickers` (array of strings)
- `market_id` (single UUID)
- `market_ids` (array of UUIDs)

### Subscription Confirmation
```json
{
  "id": 1,
  "type": "subscribed",
  "msg": {
    "channel": "orderbook_delta",
    "sid": 1
  }
}
```

### Orderbook Snapshot (sent first)
```json
{
  "type": "orderbook_snapshot",
  "sid": 2,
  "seq": 2,
  "msg": {
    "market_ticker": "FED-23DEC-T3.00",
    "market_id": "9b0f6b43-...",
    "yes_dollars_fp": [["0.0800", "300.00"]],
    "no_dollars_fp": [["0.5400", "20.00"]]
  }
}
```

### Orderbook Delta
```json
{
  "type": "orderbook_delta",
  "sid": 2,
  "seq": 3,
  "msg": {
    "market_ticker": "FED-23DEC-T3.00",
    "market_id": "...",
    "price_dollars": "0.960",
    "delta_fp": "-54.00",
    "side": "yes",
    "ts": "2022-11-22T20:44:01Z"
  }
}
```
Note: Delta is per-price-level, not the full book. `delta_fp` is the change in quantity.

### Ticker Message
```json
{
  "type": "ticker",
  "sid": 11,
  "msg": {
    "market_ticker": "FED-23DEC-T3.00",
    "market_id": "...",
    "price_dollars": "0.480",
    "yes_bid_dollars": "0.450",
    "yes_ask_dollars": "0.530",
    "volume_fp": "33896.00",
    "open_interest_fp": "20422.00",
    "dollar_volume": 16948,
    "dollar_open_interest": 10211,
    "yes_bid_size_fp": "300.00",
    "yes_ask_size_fp": "150.00",
    "last_trade_size_fp": "25.00",
    "ts": 1669149841,
    "time": "2022-11-22T20:44:01Z"
  }
}
```

### Fill Message
```json
{
  "type": "fill",
  "sid": 13,
  "msg": {
    "trade_id": "d91bc706-...",
    "order_id": "ee587a1c-...",
    "market_ticker": "HIGHNY-22DEC23-B53.5",
    "is_taker": true,
    "side": "yes",
    "yes_price_dollars": "0.750",
    "count_fp": "278.00",
    "fee_cost": "2.08",
    "action": "buy",
    "ts": 1671899397,
    "client_order_id": "optional-string",
    "post_position_fp": "500.00",
    "purchased_side": "yes",
    "subaccount": 3
  }
}
```

### Trade Message (Public)
```json
{
  "type": "trade",
  "sid": 11,
  "msg": {
    "trade_id": "d91bc706-...",
    "market_ticker": "HIGHNY-22DEC23-B53.5",
    "yes_price_dollars": "0.360",
    "no_price_dollars": "0.640",
    "count_fp": "136.00",
    "taker_side": "no",
    "ts": 1669149841
  }
}
```

### Heartbeat/Keepalive
Docs do not explicitly document a heartbeat mechanism. The Python `websockets` library "automatically handles WebSocket ping/pong frames." Our 30s ping approach is reasonable but not documented by Kalshi.

### Message Sequencing
Each message includes `sid` (subscription ID) and `seq` (sequence number). Clients should track `seq` for snapshot/delta consistency.

### Other Commands
- `unsubscribe`: `{"id": N, "cmd": "unsubscribe", "params": {"sids": [1, 2]}}`
- `list_subscriptions`: `{"id": N, "cmd": "list_subscriptions"}`
- `update_subscription`: add/remove markets from existing subscription

### Error Codes
22 documented codes (1-22) covering missing params, already subscribed, unknown subscription, market not found, etc.

---

## 8. Critical Breaking Changes (2025-2026)

1. **Dec 2025**: `GET /portfolio/positions` no longer returns settled positions -- use `GET /portfolio/settlements`
2. **Nov 2025**: `resting_orders_count` removed from positions
3. **Sep 2025**: `order_type` no longer required; only `limit` orders; `market` type deprecated
4. **Aug 2025+**: Migration from integer cents to fixed-point dollar strings
5. **Mar 2026**: Legacy integer fields removed from REST and WebSocket
6. **Mar 2026**: `resting_orders_count` deprecated in positions
