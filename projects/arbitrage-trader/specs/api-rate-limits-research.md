# API Rate Limits Research

Research date: 2026-04-09

## Polymarket REST Limits

### Gamma API (`https://gamma-api.polymarket.com`)

Rate limits are enforced per 10-second sliding windows via Cloudflare. When exceeded, requests are **throttled (delayed/queued)** rather than immediately rejected.

| Endpoint | Limit |
|----------|-------|
| General | 4,000 req/10s (400/s) |
| `/markets` | 300 req/10s (30/s) |
| `/events` | 500 req/10s (50/s) |
| `/markets` + `/events` listing combined | 900 req/10s |
| `/comments` | 200 req/10s |
| `/tags` | 200 req/10s |
| `/public-search` | 350 req/10s |

**Pagination:** Offset-based with `limit` and `offset` parameters. No documented maximum for `limit`, but examples only show up to `limit=100`. The `/events` endpoint is recommended for bulk fetching since events contain their associated markets, reducing API calls.

**Current implementation note:** Our code uses `limit=200` in main.rs and `limit=50` in the client library. These appear safe given the 300 req/10s allowance for `/markets`.

### CLOB API (`https://clob.polymarket.com`)

| Endpoint | Limit |
|----------|-------|
| General | 9,000 req/10s (900/s) |
| Market data (`/book`, `/price`, `/midpoint`) | 1,500 req/10s (150/s) |
| Bulk endpoints (`/books`, `/prices`, `/midpoints`) | 500 req/10s (50/s) |
| Balance allowance (GET) | 200 req/10s |
| Balance allowance (UPDATE) | 50 req/10s |
| API key endpoints | 100 req/10s |

**Trading endpoints (dual burst + sustained limits):**

| Endpoint | Burst | Sustained |
|----------|-------|-----------|
| `POST /order` | 3,500 req/10s | 36,000 req/10 min |
| `DELETE /order` | 3,000 req/10s | 30,000 req/10 min |
| `POST/DELETE /orders` (batch) | 1,000 req/10s | 15,000 req/10 min |
| `DELETE /cancel-all` | 250 req/10s | 6,000 req/10 min |

### Data API (`https://data-api.polymarket.com`)

| Endpoint | Limit |
|----------|-------|
| General | 1,000 req/10s |
| `/trades` | 200 req/10s |
| `/positions` | 150 req/10s |

**No tier system documented.** No authenticated vs. unauthenticated rate differences. No rate limit headers mentioned in responses.

---

## Polymarket WebSocket Limits

**URL:** `wss://ws-subscriptions-clob.polymarket.com/ws/market`

**Authentication:** Not required for market channel.

**Heartbeat:** Send `PING` (text) every 10 seconds. Server responds with `PONG`. Failure to maintain heartbeat may result in disconnection.

**Subscription management:** Dynamic subscribe/unsubscribe without reconnecting via `updateSubscription` operation.

**Event types available:**
- `book` -- full orderbook snapshot
- `price_change` -- new orders/cancellations
- `best_bid_ask` -- BBO changes (requires `custom_feature_enabled: true`)
- `last_trade_price` -- executed trades
- `tick_size_change` -- min tick adjustments
- `new_market`, `market_resolved` (requires `custom_feature_enabled: true`)

**Undocumented limits:**
- Max concurrent connections: NOT SPECIFIED
- Max subscriptions per connection: NOT SPECIFIED
- Max asset IDs per subscription message: NOT SPECIFIED
- Message rate limits: NOT SPECIFIED

**Current implementation:** Our code sends `PING` every 5 seconds with a 30-second stale timeout. The docs recommend 10-second ping interval -- our 5-second interval is more aggressive but safe.

---

## Kalshi REST Limits

### Rate Limit Tiers

| Tier | Read | Write | Qualification |
|------|------|-------|--------------|
| **Basic** | 20 req/s | 10 req/s | Account signup |
| **Advanced** | 30 req/s | 30 req/s | Form submission at kalshi.typeform.com/advanced-api |
| **Premier** | 100 req/s | 100 req/s | 3.75% of monthly exchange volume |
| **Prime** | 400 req/s | 400 req/s | 7.5% of monthly exchange volume |

**Write-limited endpoints:**
- `CreateOrder`, `CancelOrder`, `AmendOrder`, `DecreaseOrder` -- 1 transaction each
- `BatchCreateOrders` -- 1 transaction per item
- `BatchCancelOrders` -- 0.2 transactions per cancel

**Tier downgrades:** Exchange will downgrade from Prime/Premier for lack of activity.

### GET /markets Endpoint

- **URL:** `https://api.elections.kalshi.com/trade-api/v2/markets`
- **`limit` parameter:** 0-1000 (default: 100)
- **Pagination:** Cursor-based. Response includes `cursor` field; pass as query parameter for next page. `cursor=null` means no more pages.
- **Query parameters:** `cursor`, `limit`, `status`, `event_ticker`, `series_ticker`, `tickers` (comma-separated), `min_close_ts`, `max_close_ts`, timestamp filters, `mve_filter`

**Rate limit headers:** Responses include `Ratelimit-Remaining` and `Ratelimit-Reset` headers (our code already logs these).

**429 handling:** Our code checks for HTTP 429 and returns `KalshiError::RateLimited`.

**Note on pagination docs contradiction:** The general pagination guide says max 100 items/page, but the GET /markets endpoint schema explicitly allows `limit` 0-1000. The endpoint-specific documentation is likely authoritative -- test with limit=200 first.

---

## Kalshi WebSocket Limits

**URLs:**
- Production: `wss://api.elections.kalshi.com/trade-api/ws/v2`
- Demo: `wss://demo-api.kalshi.co/trade-api/ws/v2`

**Authentication:** Required. Three headers during handshake:
- `KALSHI-ACCESS-KEY`
- `KALSHI-ACCESS-SIGNATURE` (RSA-PSS signed)
- `KALSHI-ACCESS-TIMESTAMP`

**Heartbeat:** Server sends Ping frames (0x9) every 10 seconds with body `heartbeat`. Clients must respond with Pong frames (0xA).

**Available channels:**

| Channel | Auth Required | Description |
|---------|--------------|-------------|
| `orderbook_delta` | Yes | Orderbook price level changes |
| `ticker` | No (public) | Price, volume, OI updates |
| `trade` | No (public) | Public trade notifications |
| `fill` | Yes | Order fill notifications |
| `market_positions` | Yes | Position updates |
| `market_lifecycle_v2` | No (public) | Market state changes |
| `user_orders` | Yes | Order creation/update |

**Subscription format:**
```json
{
  "id": 1,
  "cmd": "subscribe",
  "params": {
    "channels": ["orderbook_delta", "ticker"],
    "market_ticker": "TICKER-HERE"
  }
}
```

**Undocumented limits:**
- Max concurrent connections: NOT SPECIFIED
- Max subscriptions per connection: NOT SPECIFIED
- Message rate limits: NOT SPECIFIED

**Subscription management:** `list_subscriptions` and `update_subscription` commands supported.

---

## Current Implementation Gaps

### 1. Polymarket rate limiter is over-conservative for Gamma, under-specified for CLOB

Our `PolyRateLimiter` enforces a flat 100 req/s for all endpoints. But Gamma `/markets` is only 30 req/s (300/10s), while CLOB general is 900 req/s. A single limiter cannot properly enforce both.

**File:** `crates/arb-polymarket/src/rate_limit.rs`

### 2. Kalshi rate limiter matches Basic tier only

Our `KalshiRateLimiter` hardcodes Basic tier (10 write/s, 20 read/s). If we upgrade to Advanced (30/30) or Premier (100/100), the limiter needs to be configurable.

**File:** `crates/arb-kalshi/src/rate_limit.rs`

### 3. Kalshi market fetching does not paginate

`KalshiClient::fetch_markets()` sends a single request with `limit=200`. It discards the cursor from the response and returns only the first page. With ~1000+ open markets on Kalshi, this misses most markets.

**File:** `crates/arb-kalshi/src/client.rs` (line 73: `req = req.query(&[("limit", "200")])`)

### 4. Polymarket Gamma fetch does not paginate

`PolymarketClient::fetch_markets()` uses `limit=50` and never follows `next_cursor`. The connector comments confirm: "MVP: fetch the first page only."

**File:** `crates/arb-polymarket/src/client.rs` (line 52)
**File:** `crates/arb-polymarket/src/connector.rs` (line 43: comment "MVP: fetch the first page only")

### 5. main.rs duplicates Gamma fetching outside the client

The CLI's match-only and live modes fetch from Gamma directly via raw `reqwest::Client` with `limit=200`, bypassing the `PolymarketClient` and its rate limiter entirely.

**File:** `crates/arb-cli/src/main.rs` (lines 207-208, 651)

### 6. No retry-after logic for 429 responses

Both clients detect 429 but return an error immediately. Neither implements backoff-and-retry when rate limited.

### 7. Polymarket WebSocket ping interval mismatch

Our code pings every 5 seconds; docs recommend 10 seconds. Harmless but unnecessary extra traffic.

**File:** `crates/arb-polymarket/src/ws.rs` (line 121: `let ping_interval = Duration::from_secs(5)`)

---

## Recommendations for Safe Fetch Limits

### Polymarket (Gamma API)

| Strategy | Details |
|----------|---------|
| **Safe single request** | `limit=100` (well within 300 req/10s for `/markets`) |
| **Aggressive single request** | `limit=500` -- likely works but undocumented max; test empirically |
| **Recommended for 500+ markets** | Paginate with `limit=100`, `offset=0,100,200,...`. At 300 req/10s you can do 5 pages/second safely |
| **Alternative** | Use `/events?active=true` endpoint (500 req/10s) -- events embed markets, fewer requests needed |

### Kalshi

| Strategy | Details |
|----------|---------|
| **Safe single request** | `limit=200` (current) or `limit=500` -- both within 0-1000 range |
| **Maximum single request** | `limit=1000` (documented max for GET /markets) |
| **Recommended for all markets** | Use `limit=1000` + cursor pagination. At 20 read req/s (Basic), you get 1000 markets/request, ~20k markets/second throughput |
| **Rate budget** | A full pagination cycle (e.g., 3 pages of 1000) costs 3 read requests = 0.15s of rate budget at Basic tier |

### Priority Implementation Changes

1. **Kalshi: Increase limit to 1000 and add cursor pagination** -- highest impact, simplest change. Modify `fetch_markets()` to loop with cursor.
2. **Polymarket: Add offset pagination to Gamma fetch** -- loop with `offset` increments of 100.
3. **Split Polymarket rate limiter** -- separate Gamma (30/s for `/markets`) from CLOB (150/s for `/book`).
4. **Make Kalshi rate limiter tier-configurable** -- accept tier as config parameter.
5. **Add 429 retry logic** -- exponential backoff when `RateLimited` error is returned.
6. **Route main.rs Gamma calls through the client** -- stop bypassing the rate limiter.
