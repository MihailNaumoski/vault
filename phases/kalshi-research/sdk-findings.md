# Kalshi SDK Analysis Findings

*Researched 2026-04-06 by SDK Analyst*

## Sources Analyzed
- [Kalshi Official Starter Code (Python)](https://github.com/Kalshi/kalshi-starter-code-python/blob/main/clients.py)
- [kalshi-rs Rust SDK](https://github.com/arvchahal/kalshi-rs) -- community Rust SDK with 50+ endpoints
- [aiokalshi](https://github.com/the-odds-company/aiokalshi) -- async Python client
- [kalshi-python (lowgrind)](https://github.com/lowgrind/kalshi-python) -- official Python SDK
- [Kalshi OpenAPI spec](https://docs.kalshi.com/openapi.yaml)

---

## 1. Authentication Implementation (from Official Python Starter Code)

### Signing Algorithm: RSA-PSS (confirmed in code)
```python
def sign_pss_text(self, text: str) -> str:
    message = text.encode('utf-8')
    signature = self.private_key.sign(
        message,
        padding.PSS(
            mgf=padding.MGF1(hashes.SHA256()),
            salt_length=padding.PSS.DIGEST_LENGTH
        ),
        hashes.SHA256()
    )
    return base64.b64encode(signature).decode('utf-8')
```

### Header Construction (from Official Python Starter Code)
```python
def request_headers(self, method: str, path: str) -> Dict[str, Any]:
    current_time_milliseconds = int(time.time() * 1000)
    timestamp_str = str(current_time_milliseconds)
    path_parts = path.split('?')
    msg_string = timestamp_str + method + path_parts[0]
    signature = self.sign_pss_text(msg_string)
    
    headers = {
        "Content-Type": "application/json",
        "KALSHI-ACCESS-KEY": self.key_id,
        "KALSHI-ACCESS-SIGNATURE": signature,
        "KALSHI-ACCESS-TIMESTAMP": timestamp_str,
    }
    return headers
```

### WebSocket Connection (from Official Python Starter Code)
```python
async def connect(self):
    host = self.WS_BASE_URL + self.url_suffix  # wss://api.elections.kalshi.com/trade-api/ws/v2
    auth_headers = self.request_headers("GET", self.url_suffix)  # signs "GET" + "/trade-api/ws/v2"
    async with websockets.connect(host, additional_headers=auth_headers) as websocket:
        self.ws = websocket
        await self.on_open()
        await self.handler()
```

**Key finding**: WebSocket auth is via HTTP upgrade headers, NOT via a JSON message after connection.

---

## 2. Comparison: Our arb-kalshi vs Real API

### Finding 1: BREAKING -- Wrong Signing Algorithm
- **Our code** (auth.rs): Uses `rsa::pkcs1v15::SigningKey<Sha256>` (PKCS1v15)
- **Real API**: Uses RSA-PSS with SHA-256, MGF1(SHA-256), salt_length=DIGEST_LENGTH
- **Severity**: BREAKING -- signatures will be rejected by the server
- **Fix**: Switch from `rsa::pkcs1v15::SigningKey` to `rsa::pss::SigningKey` with appropriate parameters

### Finding 2: BREAKING -- Wrong Base URL
- **Our code** (types.rs): `https://trading-api.kalshi.com/trade-api/v2`
- **Real API**: `https://api.elections.kalshi.com/trade-api/v2`
- **Severity**: BREAKING -- connection will fail (DNS resolution or redirect)
- **Fix**: Update both `default_base_url()` and `default_ws_url()` in types.rs

### Finding 3: BREAKING -- Wrong WebSocket Auth Flow
- **Our code** (ws.rs): Connects to WS, then sends JSON auth message: `{"id":1,"cmd":"subscribe","params":{"channels":["auth"],...}}`
- **Real API**: Auth happens via HTTP headers during the WebSocket upgrade handshake
- **Severity**: BREAKING -- the "auth" channel subscription is not the correct auth mechanism
- **Fix**: Pass `KALSHI-ACCESS-KEY`, `KALSHI-ACCESS-SIGNATURE`, `KALSHI-ACCESS-TIMESTAMP` as HTTP headers in the `tokio_tungstenite::connect_async()` request

### Finding 4: BREAKING -- Wrong WebSocket URL  
- **Our code**: `wss://trading-api.kalshi.com/trade-api/ws/v2`
- **Real API**: `wss://api.elections.kalshi.com/trade-api/ws/v2`
- **Severity**: BREAKING
- **Fix**: Update default in types.rs

### Finding 5: SUBOPTIMAL -- Price Format Migration Needed
- **Our code**: Uses `u32` cents everywhere (prices, orderbook levels, order requests)
- **Real API**: Migrating to fixed-point dollar strings (`"0.4200"`, `"100.00"`)
- **Severity**: SUBOPTIMAL now, BREAKING after March 2026 (legacy fields being removed)
- **Fix**: Add `_dollars` and `_fp` string fields to all types, parse with appropriate deserialization

### Finding 6: SUBOPTIMAL -- Orderbook Response Format
- **Our code** (types.rs): `KalshiBookResponse { yes: Vec<Vec<u32>>, no: Vec<Vec<u32>> }` -- integer arrays
- **Real API**: `orderbook_fp { yes_dollars: [["0.1500", "100.00"]], no_dollars: [["0.2500", "50.00"]] }` -- string arrays
- **Severity**: SUBOPTIMAL (legacy format may still work during transition)
- **Fix**: Update to parse `orderbook_fp` with string price/quantity tuples

### Finding 7: SUBOPTIMAL -- WebSocket Message Envelope
- **Our code**: Expects flat messages like `{"type": "orderbook_delta", "market_ticker": "...", "yes": [...], "no": [...]}`
- **Real API**: Messages wrapped in envelope: `{"type": "orderbook_delta", "sid": 2, "seq": 3, "msg": { ... }}`
- **Severity**: SUBOPTIMAL/BREAKING -- serde deserialization will fail on the envelope
- **Fix**: Add outer envelope struct with `type`, `sid`, `seq`, `msg` fields

### Finding 8: SUBOPTIMAL -- Orderbook Delta Format
- **Our code**: Expects delta as full book levels `{"yes": [[42, 100]], "no": [[58, 100]]}`
- **Real API**: Delta is per-price-level: `{"price_dollars": "0.960", "delta_fp": "-54.00", "side": "yes"}`
- **Severity**: SUBOPTIMAL/BREAKING -- completely different delta structure
- **Fix**: Restructure delta handling to process single price-level updates

### Finding 9: SUBOPTIMAL -- Ticker Message Format
- **Our code**: `{"type":"ticker","market_ticker":"...","yes_price":55,"no_price":45,"volume":5000}`
- **Real API**: `{"type":"ticker","sid":11,"msg":{"market_ticker":"...","price_dollars":"0.480","yes_bid_dollars":"0.450","yes_ask_dollars":"0.530",...}}`
- **Severity**: SUBOPTIMAL/BREAKING -- different field names, string prices, envelope wrapping
- **Fix**: Update `KalshiWsMessage::Ticker` to match real field names and types

### Finding 10: SUBOPTIMAL -- Fill Message Format
- **Our code**: `{"type":"fill","order_id":"...","count":5,...,"yes_price":42,"no_price":58}`
- **Real API**: `{"type":"fill","sid":13,"msg":{"trade_id":"...","order_id":"...","yes_price_dollars":"0.750","count_fp":"278.00","fee_cost":"2.08",...}}`
- **Severity**: SUBOPTIMAL -- different field names, new fields (trade_id, fee_cost, is_taker, etc.)
- **Fix**: Update `KalshiWsMessage::Fill` variant

### Finding 11: SUBOPTIMAL -- Positions Response
- **Our code**: `KalshiPositionResponse { ticker, market_exposure, position: i32, resting_orders_count, total_cost }`
- **Real API**: `{ ticker, total_traded_dollars, position_fp, market_exposure_dollars, realized_pnl_dollars, fees_paid_dollars, last_updated_ts }`
- **Severity**: SUBOPTIMAL -- `resting_orders_count` was removed Nov 2025, fields are now dollar strings
- **Fix**: Update struct to match new field names and types

### Finding 12: SUBOPTIMAL -- Order Response
- **Our code**: `KalshiOrderResponse { order_id, status, remaining_count: u32, filled_count: u32, yes_price: u32, ... }`
- **Real API**: Uses `_fp` and `_dollars` suffixed fields: `remaining_count_fp`, `fill_count_fp`, `yes_price_dollars`, etc.
- **Severity**: SUBOPTIMAL -- legacy fields may still work during transition
- **Fix**: Add dollar-string fields

### Finding 13: COSMETIC -- Missing WS Sequence Tracking
- **Our code**: No `seq` tracking
- **Real API**: Each message has `seq` for consistency checking
- **Fix**: Track `seq` per subscription to detect missed messages

### Finding 14: COSMETIC -- Missing Snapshot Handling  
- **Our code**: No distinct handling for `orderbook_snapshot` messages
- **Real API**: Sends `orderbook_snapshot` first, then `orderbook_delta` updates
- **Fix**: Add `OrderbookSnapshot` variant to `KalshiWsMessage`

### Finding 15: MISSING -- Order Types
- **Sep 2025**: `market` order type deprecated; only `limit` orders supported
- **Sep 2025**: `order_type` field no longer required in create order
- Our code correctly uses "limit" type, which is fine

### Finding 16: MISSING -- Batch Operations
- Real API supports `POST /portfolio/orders/batched` (up to 20 orders)
- Real API supports `DELETE /portfolio/orders/batched` 
- Our code has no batch support

### Finding 17: MISSING -- Order Amend/Decrease
- Real API supports `POST /portfolio/orders/{id}/amend` and `/decrease`
- Our code has no amend/decrease support

### Finding 18: MISSING -- Additional WS Channels
We only subscribe to `orderbook_delta` and `ticker`. Missing:
- `trade` -- public trade feed
- `fill` -- user fills (we have the WS type but don't subscribe)
- `market_positions` -- position updates
- `user_orders` -- order updates
- `market_lifecycle_v2` -- market lifecycle events
- `communications` -- RFQ/quotes

---

## 3. WebSocket Auth: Our Code vs Reality

### Our Code (ws.rs lines 114-126)
```rust
pub(crate) fn build_auth_message(auth: &KalshiAuth) -> serde_json::Value {
    let (timestamp, signature) = auth.sign_request("GET", "/trade-api/ws/v2");
    serde_json::json!({
        "id": 1,
        "cmd": "subscribe",
        "params": {
            "channels": ["auth"],
            "key_id": auth.api_key_id(),
            "signature": signature,
            "timestamp": timestamp.parse::<i64>().unwrap_or(0)
        }
    })
}
```

### Real API (from official Python SDK)
```python
auth_headers = self.request_headers("GET", "/trade-api/ws/v2")
websockets.connect(host, additional_headers=auth_headers)
```

The auth happens in the HTTP headers during WebSocket upgrade. There is no `"auth"` channel. Our `build_auth_message` function is based on an incorrect understanding.

---

## 4. Rate Limit Comparison

| What | Our Code | Real API |
|---|---|---|
| Trading limit | 10 req/s | 10 req/s (Basic tier write) |
| Market data limit | 100 req/s | 20 req/s (Basic tier read) |
| Classification | `/portfolio/` = trading | Only order mutations = write |
| Read limit | Assumed 100 | 20 (Basic), 30 (Advanced), 100 (Premier), 400 (Prime) |

**Key difference**: Our 100 req/s for market data assumes Premier tier. Basic tier is only 20 req/s. Also, GET requests to `/portfolio/` endpoints are READ operations (20/s), not write operations (10/s). Our code incorrectly classifies all `/portfolio/` as trading.
