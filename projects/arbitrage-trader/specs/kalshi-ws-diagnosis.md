# Kalshi WebSocket Price Feed Diagnosis

## Summary

The Kalshi WebSocket connects and subscribes successfully, but **orderbook snapshot messages are silently dropped** because our deserialization uses the wrong field names. The ticker channel likely works for active markets but may not fire for low-volume markets like `KXGREENTERRITORY-29-27`. Two bugs identified, one critical.

---

## Bug #1 (CRITICAL): Orderbook Snapshot Field Name Mismatch

### The Problem

Our `OrderbookSnapshotMsg` struct deserializes fields named `yes` and `no`:

```rust
// crates/arb-kalshi/src/ws.rs, line 148
pub struct OrderbookSnapshotMsg {
    pub market_ticker: String,
    #[serde(default)]
    pub yes: Vec<Vec<serde_json::Value>>,  // WRONG FIELD NAME
    #[serde(default)]
    pub no: Vec<Vec<serde_json::Value>>,   // WRONG FIELD NAME
}
```

But the **actual Kalshi API** (confirmed from AsyncAPI spec at `docs.kalshi.com/asyncapi.yaml`) sends:

```json
{
  "type": "orderbook_snapshot",
  "sid": 2,
  "msg": {
    "market_ticker": "KXGREENTERRITORY-29-27",
    "yes_dollars_fp": [["0.27", "150.00"], ["0.26", "300.00"]],
    "no_dollars_fp": [["0.73", "200.00"], ["0.74", "100.00"]]
  }
}
```

The correct field names are **`yes_dollars_fp`** and **`no_dollars_fp`**, not `yes` and `no`.

### Why This Is Silent

Because both fields have `#[serde(default)]`, when the real API sends `yes_dollars_fp`/`no_dollars_fp`, serde sees no `yes`/`no` fields and defaults both to empty `Vec`. The `LocalOrderbook::from_snapshot()` creates an empty book, and `to_price_update()` returns `None` (both sides empty). No error is logged -- the snapshot is silently eaten.

### The Fix

```rust
#[derive(Deserialize)]
pub struct OrderbookSnapshotMsg {
    pub market_ticker: String,
    #[serde(default, alias = "yes")]
    pub yes_dollars_fp: Vec<Vec<serde_json::Value>>,
    #[serde(default, alias = "no")]
    pub no_dollars_fp: Vec<Vec<serde_json::Value>>,
}
```

And update all references from `inner.yes` / `inner.no` to `inner.yes_dollars_fp` / `inner.no_dollars_fp` (or keep the field names as `yes`/`no` internally but add `#[serde(rename = "yes_dollars_fp", alias = "yes")]`).

**Simplest fix** -- just rename with alias for backward compat:

```rust
#[derive(Deserialize)]
pub struct OrderbookSnapshotMsg {
    pub market_ticker: String,
    #[serde(default, rename = "yes_dollars_fp", alias = "yes")]
    pub yes: Vec<Vec<serde_json::Value>>,
    #[serde(default, rename = "no_dollars_fp", alias = "no")]
    pub no: Vec<Vec<serde_json::Value>>,
}
```

This way the struct field stays `yes`/`no` internally (no downstream changes) but serde looks for `yes_dollars_fp` first and falls back to `yes`.

### Files to Change

- `crates/arb-kalshi/src/ws.rs` lines 148-153: `OrderbookSnapshotMsg` struct
- `crates/arb-kalshi/src/ws.rs` test at line 736: Update test JSON to use `yes_dollars_fp`/`no_dollars_fp` (or keep both tests for backward compat)

---

## Bug #2 (MODERATE): Ticker Channel for Low-Volume Markets

### The Problem

The ticker channel is event-driven -- it sends updates "whenever any ticker field changes" (price, volume, bid/ask). For low-volume markets like `KXGREENTERRITORY-29-27`, the ticker may simply not fire because nothing is trading.

### Why This Matters

Even if Bug #1 is fixed, the orderbook snapshot only fires once on subscription. After that, updates come via `orderbook_delta` messages (which DO use `price_dollars`/`delta_fp`/`side` -- our code handles these correctly). But if no one is trading, there are no deltas either.

### The Fix

The Kalshi ticker subscription supports an optional `send_initial_snapshot` parameter (confirmed from AsyncAPI spec). We should use it:

```rust
// In build_subscribe_message or when subscribing to ticker channel
let ticker_sub = serde_json::json!({
    "id": 2,
    "cmd": "subscribe",
    "params": {
        "channels": ["ticker"],
        "market_tickers": tickers,
        "send_initial_snapshot": true  // ADD THIS
    }
});
```

This ensures we get at least one ticker message immediately with current prices, even for dormant markets.

### File to Change

- `crates/arb-kalshi/src/ws.rs` line 607: Add `send_initial_snapshot: true` to the ticker subscription, either by modifying `build_subscribe_message` to accept extra params or by constructing the ticker sub inline.

---

## Non-Issues (Verified Working)

### Authentication
Auth via HTTP upgrade headers is correct. The code at line 561-592 properly:
- Signs `GET /trade-api/ws/v2` with RSA-PSS-SHA256
- Includes `KALSHI-ACCESS-KEY`, `KALSHI-ACCESS-SIGNATURE`, `KALSHI-ACCESS-TIMESTAMP` headers
- Uses `tokio_tungstenite::connect_async(request)` which passes custom headers (v0.26 supports this)

### WebSocket URL
`config/default.toml` uses `wss://api.elections.kalshi.com/trade-api/ws/v2` which matches the production endpoint. (Note: `types.rs` defaults to `wss://trading-api.kalshi.com/...` but the config override is correct.)

### Subscription Message Format
`build_subscribe_message()` at line 114 produces the correct format:
```json
{"id": 1, "cmd": "subscribe", "params": {"channels": ["orderbook_delta"], "market_tickers": ["TICKER"]}}
```
This matches the Kalshi API docs exactly.

### PaperConnector Delegation
`PaperConnector::subscribe_prices()` at line 101-106 correctly delegates to `self.inner.subscribe_prices(ids, tx)`, so the real Kalshi WS is used even in paper mode.

### Orderbook Delta Parsing
Delta messages use `price_dollars`, `delta_fp`, `side` -- which match the API. This path works correctly once a snapshot has been received.

### Ping/Pong
The code sends WebSocket-level pings (binary ping frames) every 30s, and responds to server pings. Kalshi sends heartbeat pings every 10s. This is handled correctly.

---

## Polymarket Comparison (Why It Works)

Polymarket WS works because:
1. It uses a simpler subscription: `{"type": "market", "assets_ids": [...]}` (no auth needed for WS)
2. It receives `best_bid_ask` and `price_change` messages that are event-driven and frequent
3. The message parsing handles multiple message types (`BestBidAsk`, `PriceChange`, `LastTradePrice`)
4. Polymarket markets tend to be more active with more frequent price updates

---

## Root Cause Summary

**Primary**: Orderbook snapshot field names are wrong (`yes`/`no` vs `yes_dollars_fp`/`no_dollars_fp`). This means the initial price data from the orderbook subscription is silently dropped, and the local orderbook stays empty. Subsequent deltas apply to an empty book, which may still produce `None` prices if only one side has data.

**Secondary**: No `send_initial_snapshot` on the ticker subscription means we rely on organic ticker events, which may never come for dormant markets.

**Together**: Both the orderbook and ticker paths fail to deliver the initial price for a quiet market. The REST polling fallback at 8s intervals does work (confirmed by user), which is why prices eventually appear but only from REST, not WS.

---

## Recommended Fix Order

1. Fix `OrderbookSnapshotMsg` field names (Bug #1) -- this is the critical fix
2. Add `send_initial_snapshot: true` to ticker subscription (Bug #2)
3. Add debug logging for raw WS messages (at least first N per session) to catch similar issues faster
4. Consider adding a test that validates against real Kalshi API response shapes
