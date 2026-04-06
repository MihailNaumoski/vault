# Phase 2 -- Build Prompts: Platform Connectors

Generated: 2026-04-05
Phase: 2 of 5
Depends on: Phase 1 (arb-types, arb-db, arb-risk, arb-cli -- all compiling)

---

## Design Decisions

### D1: Connector struct design (Auto-selected: trait object pattern from arb-types)
Both connectors implement `PredictionMarketConnector` from `arb-types`. Each connector is a struct holding an HTTP client, auth state, rate limiter, and optional WebSocket handle. Constructed via a `Config` struct, not builder pattern (simpler for MVP).

### D2: Mock location (Auto-selected: same crate, feature-gated)
Mock connectors live inside their respective crates behind `#[cfg(feature = "mock")]` rather than in a separate crate. This keeps test utilities close to the real implementation and avoids circular dependencies. Both crates expose a `MockConnector` when the `mock` feature is enabled.

### D3: Rate limiter design (Auto-selected: token bucket in-process)
Use `governor` crate for token-bucket rate limiting (already battle-tested, async-native). Polymarket gets 100 req/s bucket. Kalshi gets two buckets: 10 req/s for trading endpoints, 100 req/s for market data.

### D4: WebSocket reconnection strategy (Addresses gap G3)
Exponential backoff with jitter: initial 1s, max 30s, jitter +/- 25%. On reconnect, re-subscribe to all previously subscribed market IDs. Max 10 consecutive failures before emitting a fatal error. The reconnecting wrapper is a background `tokio::task` that owns the connection and forwards messages to an `mpsc` channel.

### D5: Kalshi rate limit batching (Addresses gap G5)
Order monitoring uses `list_open_orders()` (single request returning all orders) instead of per-order polling. Combined with WebSocket `fill` channel for real-time fill notifications, this keeps trading requests well under 10 req/s.

### D6: Missing types needed (Addresses gap G1)
All required types already exist in arb-types from Phase 1: `OrderBook`, `OrderBookLevel`, `PriceUpdate`, `SubHandle`, `LimitOrderRequest`, `OrderResponse`, `PlatformPosition`. No new shared types needed. Connector-internal types (API response structs, auth state) are defined per-crate in `src/types.rs`.

### D7: New workspace dependency -- `governor` for rate limiting
Add `governor = "0.8"` to `[workspace.dependencies]` in root `Cargo.toml`. This replaces hand-rolled token buckets with a proven async-compatible rate limiter.

---

## Prompt Execution Order

```
Prompt 1:  Workspace Setup (Cargo.toml updates)
Prompt 2A: arb-polymarket -- Auth + Signing (HMAC + EIP-712)
Prompt 2B: arb-polymarket -- REST Client
Prompt 2C: arb-polymarket -- WebSocket Client
Prompt 2D: arb-polymarket -- Trait Impl + Mock + Tests
Prompt 3A: arb-kalshi -- Auth (RSA-SHA256)
Prompt 3B: arb-kalshi -- REST Client
Prompt 3C: arb-kalshi -- WebSocket Client
Prompt 3D: arb-kalshi -- Trait Impl + Mock + Tests
```

Dependencies:
- Prompt 1 must complete first (all others depend on it)
- Prompts 2A-2D are sequential within arb-polymarket
- Prompts 3A-3D are sequential within arb-kalshi
- The 2x and 3x chains can run in parallel after Prompt 1

---

## Prompt 1: Workspace Setup

### Task
Update Cargo.toml files for arb-polymarket and arb-kalshi with all required dependencies. Add `governor` to workspace dependencies.

### Context Files
- `/Users/mihail/projects/vault/projects/arbitrage-trader/Cargo.toml` (workspace root)
- `/Users/mihail/projects/vault/projects/arbitrage-trader/crates/arb-polymarket/Cargo.toml`
- `/Users/mihail/projects/vault/projects/arbitrage-trader/crates/arb-kalshi/Cargo.toml`

### Deliverables

#### Root Cargo.toml changes
Add to `[workspace.dependencies]`:
```toml
governor = "0.8"
```

#### arb-polymarket/Cargo.toml
```toml
[package]
name = "arb-polymarket"
version = "0.1.0"
edition = "2021"

[features]
default = []
mock = []

[dependencies]
arb-types = { workspace = true }
tokio = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
rust_decimal = { workspace = true }
chrono = { workspace = true }
tracing = { workspace = true }
thiserror = { workspace = true }
anyhow = { workspace = true }
reqwest = { workspace = true }
tokio-tungstenite = { workspace = true }
futures-util = { workspace = true }
async-trait = { workspace = true }
hmac = { workspace = true }
sha2 = { workspace = true }
hex = { workspace = true }
base64 = { workspace = true }
alloy-signer = { workspace = true }
alloy-signer-local = { workspace = true }
alloy-primitives = { workspace = true }
alloy-sol-types = { workspace = true }
governor = { workspace = true }
parking_lot = { workspace = true }
uuid = { workspace = true }

[dev-dependencies]
tokio = { workspace = true, features = ["test-util", "macros"] }
```

#### arb-kalshi/Cargo.toml
```toml
[package]
name = "arb-kalshi"
version = "0.1.0"
edition = "2021"

[features]
default = []
mock = []

[dependencies]
arb-types = { workspace = true }
tokio = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
rust_decimal = { workspace = true }
chrono = { workspace = true }
tracing = { workspace = true }
thiserror = { workspace = true }
anyhow = { workspace = true }
reqwest = { workspace = true }
tokio-tungstenite = { workspace = true }
futures-util = { workspace = true }
async-trait = { workspace = true }
rsa = { workspace = true }
sha2 = { workspace = true }
base64 = { workspace = true }
governor = { workspace = true }
parking_lot = { workspace = true }
uuid = { workspace = true }

[dev-dependencies]
tokio = { workspace = true, features = ["test-util", "macros"] }
```

### Build Check
```bash
cd /Users/mihail/projects/vault/projects/arbitrage-trader && cargo check --workspace
```

### Acceptance Criteria
- [ ] `cargo check --workspace` passes
- [ ] Both connector crates have all dependencies listed
- [ ] `mock` feature is declared in both crates
- [ ] No unused dependencies (each one is justified below)

### Output Contract
- **Files modified:** `Cargo.toml`, `crates/arb-polymarket/Cargo.toml`, `crates/arb-kalshi/Cargo.toml`
- **Exports:** None (config only)
- **Build status:** Must pass `cargo check`

---

## Prompt 2A: arb-polymarket -- Auth + Signing

### Task
Implement Polymarket authentication (HMAC-SHA256 request signing) and EIP-712 order signing. These are the two hardest pieces of the Polymarket connector.

### Context Files
- `/Users/mihail/projects/vault/projects/arbitrage-trader/SPEC.md` (Section 6.2)
- `/Users/mihail/projects/vault/projects/arbitrage-trader/crates/arb-types/src/order.rs` (Side, LimitOrderRequest)
- `/Users/mihail/projects/vault/projects/arbitrage-trader/crates/arb-types/src/error.rs` (ArbError)

### Files to Create
- `crates/arb-polymarket/src/auth.rs`
- `crates/arb-polymarket/src/signing.rs`
- `crates/arb-polymarket/src/types.rs`
- `crates/arb-polymarket/src/error.rs`

### Module Structure
```
arb-polymarket/
  src/
    lib.rs          -- module declarations, re-exports
    auth.rs         -- HMAC-SHA256 request signing
    signing.rs      -- EIP-712 typed data order signing
    types.rs        -- Polymarket-specific API types
    error.rs        -- Connector-specific error type
```

### Deliverables

#### src/error.rs -- Connector Error Type
```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PolymarketError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("auth error: {0}")]
    Auth(String),
    #[error("signing error: {0}")]
    Signing(String),
    #[error("websocket error: {0}")]
    WebSocket(String),
    #[error("rate limited")]
    RateLimited,
    #[error("api error {status}: {message}")]
    Api { status: u16, message: String },
    #[error("deserialization error: {0}")]
    Deserialize(#[from] serde_json::Error),
}

impl From<PolymarketError> for arb_types::ArbError {
    fn from(e: PolymarketError) -> Self {
        // Map to appropriate ArbError variants
    }
}
```

#### src/types.rs -- Polymarket API Types
Define Polymarket-specific request/response structs that map to/from arb-types:
- `PolyConfig` -- credentials struct: `api_key`, `secret`, `passphrase`, `private_key` (hex-encoded Polygon wallet key)
- `PolyApiOrder` -- the JSON body for POST /order matching Polymarket CLOB schema
- `PolyOrderResponse` -- API response from order placement
- `PolyMarketResponse` -- API response from GET /markets
- `PolyBookResponse` -- API response from GET /book
- `PolyPositionResponse` -- API response from GET /positions
- `PolyBalanceResponse` -- API response from portfolio balance
- `PolyWsMessage` -- WebSocket incoming message enum (book_delta, last_trade, etc.)

All structs derive `Serialize, Deserialize` with appropriate `#[serde(rename_all = "camelCase")]` as Polymarket uses camelCase JSON.

#### src/auth.rs -- HMAC-SHA256 Request Signing
```rust
use hmac::{Hmac, Mac};
use sha2::Sha256;
use base64::{Engine as _, engine::general_purpose::STANDARD};

pub struct PolyAuth {
    api_key: String,
    secret: Vec<u8>,   // base64-decoded secret
    passphrase: String,
}

impl PolyAuth {
    pub fn new(api_key: String, secret_b64: String, passphrase: String) -> Result<Self, PolymarketError>;

    /// Sign a request. Returns (timestamp, signature) to set as headers.
    /// Signature = Base64(HMAC-SHA256(timestamp + method + path + body, secret))
    pub fn sign_request(&self, method: &str, path: &str, body: &str) -> (String, String);

    /// Build the auth headers for a request.
    /// Returns: POLY_API_KEY, POLY_SIGNATURE, POLY_TIMESTAMP, POLY_PASSPHRASE
    pub fn headers(&self, method: &str, path: &str, body: &str) -> reqwest::header::HeaderMap;
}
```

Key implementation details:
- `timestamp` is current Unix epoch in seconds as string
- `message = timestamp + method_uppercase + path + body`
- `signature = Base64(HMAC-SHA256(message, base64_decode(secret)))`
- Method must be uppercase (GET, POST, DELETE)
- Path includes query string if present
- Body is empty string for GET/DELETE

#### src/signing.rs -- EIP-712 Order Signing
```rust
use alloy_primitives::{Address, U256};
use alloy_signer::Signer;
use alloy_signer_local::PrivateKeySigner;
use alloy_sol_types::{sol, SolStruct};

// Define the EIP-712 typed data struct matching Polymarket's contract
sol! {
    #[derive(Debug)]
    struct Order {
        uint256 salt;
        address maker;
        address signer;
        address taker;
        uint256 tokenId;
        uint256 makerAmount;
        uint256 takerAmount;
        uint256 expiration;
        uint256 nonce;
        uint256 feeRateBps;
        uint8 side;
        uint8 signatureType;
    }
}

pub struct OrderSigner {
    signer: PrivateKeySigner,
    chain_id: u64,           // 137 for Polygon mainnet
    verifying_contract: Address,  // Polymarket CTF Exchange address
}

impl OrderSigner {
    pub fn new(private_key_hex: &str, chain_id: u64) -> Result<Self, PolymarketError>;

    /// Sign a limit order request, returning the signed order body for the API.
    /// Converts arb-types LimitOrderRequest into Polymarket's order format,
    /// signs with EIP-712, and returns the complete POST body as JSON.
    pub async fn sign_order(
        &self,
        req: &arb_types::LimitOrderRequest,
        token_id: &str,
    ) -> Result<serde_json::Value, PolymarketError>;

    /// Get the wallet address (maker address).
    pub fn address(&self) -> Address;
}
```

Key implementation details:
- The Polymarket CTF Exchange verifying contract address is `0x4bFb41d5B3570DeFd03C39a9A4D8dE6Bd8B8982E` on Polygon (hardcode as constant, configurable via PolyConfig)
- Domain separator: `{ name: "Polymarket CTF Exchange", version: "1", chainId: 137, verifyingContract: <address> }`
- `makerAmount` and `takerAmount` are in USDC units (6 decimals). Convert from `LimitOrderRequest.price` and `quantity`: for a BUY, `makerAmount = price * quantity * 1e6`, `takerAmount = quantity * 1e6`
- `side`: 0 = BUY, 1 = SELL
- `signatureType`: 0 = EOA
- `nonce`: random u256
- `salt`: random u256
- `expiration`: current time + TTL (e.g., 300 seconds)
- `feeRateBps`: 0 (maker)
- `taker`: zero address (any taker)
- The token_id maps YES/NO side to the correct Polymarket outcome token

### Unit Tests
```
tests in auth.rs:
  - test_sign_request_known_vector: sign a known (timestamp, method, path, body) and verify against expected HMAC
  - test_sign_request_empty_body: GET request with no body
  - test_headers_correct_keys: verify all 4 header names are correct

tests in signing.rs:
  - test_sign_order_produces_valid_json: sign an order, deserialize result, verify all fields present
  - test_sign_order_side_mapping: Side::Yes -> 0, Side::No -> 1
  - test_sign_order_amount_calculation: verify makerAmount/takerAmount math for known price+quantity
  - test_signer_address: verify address derivation from known private key
```

### Build Check
```bash
cd /Users/mihail/projects/vault/projects/arbitrage-trader && cargo test -p arb-polymarket
```

### Acceptance Criteria
- [ ] `cargo check -p arb-polymarket` passes
- [ ] All unit tests pass
- [ ] HMAC signing produces correct output for known test vectors
- [ ] EIP-712 signing produces a recoverable signature
- [ ] Error types convert to `ArbError` correctly

### Output Contract
- **Files created:** `src/auth.rs`, `src/signing.rs`, `src/types.rs`, `src/error.rs`
- **Files modified:** `src/lib.rs` (module declarations)
- **Exports:** `PolyAuth`, `OrderSigner`, `PolyConfig`, `PolymarketError`
- **Build status:** Must pass `cargo check` and `cargo test`

### Out of Scope
- REST client (Prompt 2B)
- WebSocket client (Prompt 2C)
- Full trait implementation (Prompt 2D)

---

## Prompt 2B: arb-polymarket -- REST Client

### Task
Implement the Polymarket REST client for market data and trading endpoints. Uses `PolyAuth` from Prompt 2A for request signing.

### Context Files
- `/Users/mihail/projects/vault/projects/arbitrage-trader/SPEC.md` (Section 6.2, endpoint table)
- `/Users/mihail/projects/vault/projects/arbitrage-trader/crates/arb-polymarket/src/auth.rs` (from 2A)
- `/Users/mihail/projects/vault/projects/arbitrage-trader/crates/arb-polymarket/src/signing.rs` (from 2A)
- `/Users/mihail/projects/vault/projects/arbitrage-trader/crates/arb-polymarket/src/types.rs` (from 2A)
- `/Users/mihail/projects/vault/projects/arbitrage-trader/crates/arb-types/src/market.rs`
- `/Users/mihail/projects/vault/projects/arbitrage-trader/crates/arb-types/src/order.rs`

### Files to Create
- `crates/arb-polymarket/src/client.rs`
- `crates/arb-polymarket/src/rate_limit.rs`

### Module Structure Addition
```
arb-polymarket/src/
    client.rs       -- REST client (all HTTP calls)
    rate_limit.rs   -- Token bucket rate limiter wrapper
```

### Deliverables

#### src/rate_limit.rs -- Rate Limiter
```rust
use governor::{Quota, RateLimiter, clock::DefaultClock, state::{InMemoryState, NotKeyed}};
use std::num::NonZeroU32;

pub struct PolyRateLimiter {
    limiter: RateLimiter<NotKeyed, InMemoryState, DefaultClock>,
}

impl PolyRateLimiter {
    /// Create a rate limiter for Polymarket (100 req/s).
    pub fn new() -> Self;

    /// Wait until a request is permitted.
    pub async fn acquire(&self);
}
```

#### src/client.rs -- REST Client
```rust
pub struct PolymarketClient {
    http: reqwest::Client,
    auth: PolyAuth,
    signer: OrderSigner,
    rate_limiter: PolyRateLimiter,
    base_url: String,        // https://clob.polymarket.com
    gamma_url: String,       // https://gamma-api.polymarket.com
}

impl PolymarketClient {
    pub fn new(config: PolyConfig) -> Result<Self, PolymarketError>;

    // Market data (uses Gamma API for metadata, CLOB API for prices/books)
    pub async fn fetch_markets(&self, next_cursor: Option<&str>) -> Result<Vec<PolyMarketResponse>, PolymarketError>;
    pub async fn fetch_market(&self, condition_id: &str) -> Result<PolyMarketResponse, PolymarketError>;
    pub async fn fetch_order_book(&self, token_id: &str) -> Result<PolyBookResponse, PolymarketError>;
    pub async fn fetch_price(&self, token_id: &str) -> Result<rust_decimal::Decimal, PolymarketError>;

    // Trading
    pub async fn post_order(&self, req: &arb_types::LimitOrderRequest, token_id: &str) -> Result<PolyOrderResponse, PolymarketError>;
    pub async fn cancel_order(&self, order_id: &str) -> Result<(), PolymarketError>;
    pub async fn fetch_open_orders(&self) -> Result<Vec<PolyOrderResponse>, PolymarketError>;
    pub async fn fetch_order(&self, order_id: &str) -> Result<PolyOrderResponse, PolymarketError>;

    // Account
    pub async fn fetch_positions(&self) -> Result<Vec<PolyPositionResponse>, PolymarketError>;
    pub async fn fetch_balance(&self) -> Result<rust_decimal::Decimal, PolymarketError>;
}
```

Key implementation details:
- Every HTTP call goes through `self.rate_limiter.acquire()` before sending
- Every authenticated request uses `self.auth.headers(method, path, body)` to add HMAC headers
- Order placement uses `self.signer.sign_order()` then POSTs the signed body
- Responses are deserialized into `Poly*Response` types, then the trait impl (Prompt 2D) converts to arb-types
- Pagination: Polymarket uses cursor-based pagination for `/markets`. The `fetch_markets` method handles one page; the trait impl loops.
- Error handling: non-2xx responses parsed into `PolymarketError::Api { status, message }`
- Rate limit responses (HTTP 429) mapped to `PolymarketError::RateLimited`
- All requests use `rustls-tls` (already configured in workspace reqwest features)

Endpoint mapping:
| Method | Path | Auth | Use |
|--------|------|------|-----|
| GET | `/markets` | No | List markets (paginated, via Gamma API) |
| GET | `/book?token_id={id}` | No | Order book |
| GET | `/price?token_id={id}` | No | Mid-market price |
| POST | `/order` | HMAC + EIP-712 signed body | Place order |
| DELETE | `/order/{id}` | HMAC | Cancel order |
| GET | `/orders?market={id}` | HMAC | Open orders |
| GET | `/positions` | HMAC | Current positions |

### Unit Tests
```
tests in client.rs (unit tests with mock HTTP, no real API calls):
  - test_fetch_order_book_deserialize: mock a known JSON response, verify OrderBook conversion
  - test_post_order_request_format: verify the HTTP request body matches expected schema
  - test_cancel_order_request_format: verify DELETE request is correctly formed
  - test_rate_limiter_throttles: spawn 200 requests, verify they complete in >= 2 seconds
  - test_api_error_handling: mock a 400 response, verify PolymarketError::Api is returned
  - test_429_handling: mock a 429 response, verify PolymarketError::RateLimited is returned
```

### Build Check
```bash
cd /Users/mihail/projects/vault/projects/arbitrage-trader && cargo test -p arb-polymarket
```

### Acceptance Criteria
- [ ] All REST methods implemented and compile
- [ ] Rate limiter wraps every request
- [ ] Error responses are properly mapped
- [ ] Unit tests pass with mocked HTTP responses
- [ ] No hardcoded URLs (configurable via PolyConfig, with sensible defaults)

### Output Contract
- **Files created:** `src/client.rs`, `src/rate_limit.rs`
- **Files modified:** `src/lib.rs` (add module declarations)
- **Exports:** `PolymarketClient`, `PolyRateLimiter`
- **Build status:** Must pass `cargo check` and `cargo test`

### Out of Scope
- WebSocket (Prompt 2C)
- Trait implementation (Prompt 2D)
- Integration tests against real API

---

## Prompt 2C: arb-polymarket -- WebSocket Client

### Task
Implement the Polymarket WebSocket client for real-time price feeds. Includes reconnection logic with exponential backoff (addresses gap G3).

### Context Files
- `/Users/mihail/projects/vault/projects/arbitrage-trader/SPEC.md` (Section 6.2, WebSocket details)
- `/Users/mihail/projects/vault/projects/arbitrage-trader/crates/arb-types/src/event.rs` (PriceUpdate, SubHandle)
- `/Users/mihail/projects/vault/projects/arbitrage-trader/crates/arb-polymarket/src/types.rs` (PolyWsMessage)

### Files to Create
- `crates/arb-polymarket/src/ws.rs`

### Deliverables

#### src/ws.rs -- WebSocket Client with Reconnection
```rust
use tokio::sync::mpsc;
use arb_types::{PriceUpdate, SubHandle};

pub struct PolyWebSocket {
    url: String,
    subscribed_ids: parking_lot::RwLock<Vec<String>>,
}

impl PolyWebSocket {
    pub fn new(url: String) -> Self;

    /// Subscribe to price updates for the given token IDs.
    /// Spawns a background task that:
    /// 1. Connects to wss://ws-subscriptions-clob.polymarket.com/ws
    /// 2. Sends subscribe messages for each token ID
    /// 3. Parses incoming messages into PriceUpdate
    /// 4. Forwards to the provided mpsc::Sender
    /// 5. On disconnect, reconnects with exponential backoff
    /// Returns a SubHandle whose cancel_tx stops the background task.
    pub async fn subscribe(
        &self,
        token_ids: &[String],
        tx: mpsc::Sender<PriceUpdate>,
    ) -> Result<SubHandle, PolymarketError>;
}
```

Key implementation details:

**Connection lifecycle:**
1. Connect via `tokio_tungstenite::connect_async(url)`
2. For each token_id, send: `{"type": "subscribe", "channel": "market", "assets_ids": ["<token_id>"]}`
3. Read messages in a loop, parse `PolyWsMessage`
4. Convert book deltas / last trade prices into `PriceUpdate` structs
5. Send each `PriceUpdate` through the `tx` channel

**Reconnection strategy (D4):**
```rust
struct ReconnectPolicy {
    initial_delay: Duration,     // 1 second
    max_delay: Duration,         // 30 seconds
    jitter_factor: f64,          // 0.25 (+/- 25%)
    max_consecutive_failures: u32, // 10
}
```
- On disconnect: increment failure count, compute delay = min(initial * 2^failures, max) +/- jitter
- On successful connect: reset failure count
- After max_consecutive_failures: send error through channel, stop task
- On reconnect: re-subscribe to all IDs stored in `subscribed_ids`

**Message parsing:**
Polymarket WS sends JSON messages. The key message types are:
- `book` -- full order book snapshot for a token
- `book_delta` -- incremental update (price level changed)
- `last_trade_price` -- last trade price update
- `tick_size_change` -- minimum tick size changed (can ignore for MVP)

Extract `yes_price` and `no_price` from the best bid/ask of the book or from last trade. Map to `PriceUpdate { platform: Polymarket, market_id, yes_price, no_price, timestamp }`.

**Cancellation:**
The `SubHandle.cancel_tx` sends `()` to a `oneshot::Receiver` held by the background task. The task's select loop checks this alongside message reads:
```rust
tokio::select! {
    msg = ws_stream.next() => { /* handle message */ }
    _ = cancel_rx => { /* clean shutdown, break */ }
}
```

### Unit Tests
```
tests in ws.rs:
  - test_parse_book_message: parse a known Polymarket book JSON into PriceUpdate
  - test_parse_book_delta: parse an incremental update
  - test_reconnect_policy_backoff: verify delay doubles each failure, caps at max
  - test_reconnect_policy_jitter: verify jitter stays within bounds
  - test_cancel_stops_task: create subscription, cancel it, verify task exits
```

### Build Check
```bash
cd /Users/mihail/projects/vault/projects/arbitrage-trader && cargo test -p arb-polymarket
```

### Acceptance Criteria
- [ ] WebSocket connects and parses price messages
- [ ] Reconnection with exponential backoff works
- [ ] SubHandle cancellation cleanly shuts down the background task
- [ ] PriceUpdate structs have correct platform and market_id
- [ ] All unit tests pass

### Output Contract
- **Files created:** `src/ws.rs`
- **Files modified:** `src/lib.rs` (add module declaration)
- **Exports:** `PolyWebSocket`
- **Build status:** Must pass `cargo check` and `cargo test`

### Out of Scope
- Integration testing against real WebSocket (manual test)
- Order book maintenance (that is arb-engine's job)

---

## Prompt 2D: arb-polymarket -- Trait Implementation + Mock + Tests

### Task
Implement `PredictionMarketConnector` for Polymarket by wiring together the auth, client, and WebSocket components. Also implement the mock connector for testing.

### Context Files
- `/Users/mihail/projects/vault/projects/arbitrage-trader/crates/arb-types/src/lib.rs` (PredictionMarketConnector trait)
- `/Users/mihail/projects/vault/projects/arbitrage-trader/crates/arb-polymarket/src/client.rs` (from 2B)
- `/Users/mihail/projects/vault/projects/arbitrage-trader/crates/arb-polymarket/src/ws.rs` (from 2C)
- `/Users/mihail/projects/vault/projects/arbitrage-trader/crates/arb-polymarket/src/types.rs` (from 2A)

### Files to Create
- `crates/arb-polymarket/src/connector.rs`
- `crates/arb-polymarket/src/mock.rs`

### Deliverables

#### src/connector.rs -- Trait Implementation
```rust
use arb_types::*;
use async_trait::async_trait;

pub struct PolymarketConnector {
    client: PolymarketClient,
    ws: PolyWebSocket,
}

impl PolymarketConnector {
    pub fn new(config: PolyConfig) -> Result<Self, PolymarketError>;
}

#[async_trait]
impl PredictionMarketConnector for PolymarketConnector {
    fn platform(&self) -> Platform { Platform::Polymarket }

    async fn list_markets(&self, status: MarketStatus) -> Result<Vec<Market>, ArbError> {
        // Paginate through client.fetch_markets(), filter by status
        // Convert PolyMarketResponse -> Market (assign MarketId, map fields)
    }

    async fn get_market(&self, id: &str) -> Result<Market, ArbError> {
        // id is the Polymarket condition_id
        // client.fetch_market(id) -> convert
    }

    async fn get_order_book(&self, id: &str) -> Result<OrderBook, ArbError> {
        // id is the token_id for a specific outcome
        // client.fetch_order_book(id) -> convert PolyBookResponse to OrderBook
    }

    async fn subscribe_prices(
        &self,
        ids: &[String],
        tx: tokio::sync::mpsc::Sender<PriceUpdate>,
    ) -> Result<SubHandle, ArbError> {
        // ids are token_ids
        // ws.subscribe(ids, tx)
    }

    async fn place_limit_order(&self, req: &LimitOrderRequest) -> Result<OrderResponse, ArbError> {
        // req.market_id is the token_id
        // client.post_order(req, token_id) -> convert PolyOrderResponse to OrderResponse
    }

    async fn cancel_order(&self, order_id: &str) -> Result<(), ArbError> {
        // client.cancel_order(order_id)
    }

    async fn get_order(&self, order_id: &str) -> Result<OrderResponse, ArbError> {
        // client.fetch_order(order_id) -> convert
    }

    async fn list_open_orders(&self) -> Result<Vec<OrderResponse>, ArbError> {
        // client.fetch_open_orders() -> convert each
    }

    async fn get_balance(&self) -> Result<rust_decimal::Decimal, ArbError> {
        // client.fetch_balance()
    }

    async fn get_positions(&self) -> Result<Vec<PlatformPosition>, ArbError> {
        // client.fetch_positions() -> convert each
    }
}
```

Key implementation details:
- The `market_id` field in arb-types maps to different Polymarket concepts:
  - For `get_market`: `condition_id` (the event)
  - For `get_order_book`, `place_limit_order`: `token_id` (the specific YES/NO outcome)
  - For `subscribe_prices`: `token_id`
- The connector does NOT maintain order book state -- it fetches on demand or subscribes
- Conversion functions (PolyMarketResponse -> Market, etc.) should be impl methods on the response types or standalone functions in `types.rs`

#### src/mock.rs -- Mock Connector (feature-gated)
```rust
#[cfg(feature = "mock")]
pub mod mock {
    use arb_types::*;
    use async_trait::async_trait;
    use std::sync::Arc;
    use parking_lot::Mutex;

    #[derive(Debug, Default)]
    pub struct MockState {
        pub markets: Vec<Market>,
        pub order_books: std::collections::HashMap<String, OrderBook>,
        pub orders: Vec<OrderResponse>,
        pub positions: Vec<PlatformPosition>,
        pub balance: rust_decimal::Decimal,
        pub placed_orders: Vec<LimitOrderRequest>,  // records what was placed
        pub cancelled_orders: Vec<String>,           // records what was cancelled
        pub should_fail: Option<ArbError>,           // inject failures
        pub price_updates: Vec<PriceUpdate>,         // queued for subscribe_prices
    }

    pub struct MockPolymarketConnector {
        pub state: Arc<Mutex<MockState>>,
    }

    impl MockPolymarketConnector {
        pub fn new() -> Self;
        pub fn with_state(state: Arc<Mutex<MockState>>) -> Self;
    }

    #[async_trait]
    impl PredictionMarketConnector for MockPolymarketConnector {
        fn platform(&self) -> Platform { Platform::Polymarket }

        async fn list_markets(&self, status: MarketStatus) -> Result<Vec<Market>, ArbError> {
            // Return state.markets filtered by status
            // If should_fail is set, return Err
        }
        // ... all other methods read from / write to MockState
    }
}
```

Key mock design:
- `Arc<Mutex<MockState>>` allows tests to inject data and verify calls
- `placed_orders` records all order placements for assertion
- `cancelled_orders` records all cancellations
- `should_fail` causes the next call to return an error (then resets)
- `price_updates` is drained by `subscribe_prices` -- sends all queued updates then holds the channel open
- For `subscribe_prices`: spawn a task that sends `state.price_updates` through the channel, then waits on cancel

### Update src/lib.rs
```rust
pub mod auth;
pub mod signing;
pub mod types;
pub mod error;
pub mod client;
pub mod rate_limit;
pub mod ws;
pub mod connector;

#[cfg(feature = "mock")]
pub mod mock;

pub use connector::PolymarketConnector;
pub use types::PolyConfig;
pub use error::PolymarketError;

#[cfg(feature = "mock")]
pub use mock::MockPolymarketConnector;
```

### Unit Tests
```
tests in connector.rs:
  (These use MockPolymarketConnector internally or test conversion logic)
  - test_market_conversion: PolyMarketResponse -> Market, verify all fields
  - test_order_book_conversion: PolyBookResponse -> OrderBook
  - test_order_response_conversion: PolyOrderResponse -> OrderResponse

tests in mock.rs:
  - test_mock_list_markets: inject markets, call list_markets, verify returned
  - test_mock_place_order: place an order, verify it appears in state.placed_orders
  - test_mock_cancel_order: cancel, verify in state.cancelled_orders
  - test_mock_failure_injection: set should_fail, verify error returned
  - test_mock_subscribe_prices: inject price updates, subscribe, verify all received
  - test_mock_get_balance: set balance, verify returned correctly
  - test_mock_trait_object: verify MockPolymarketConnector can be used as Box<dyn PredictionMarketConnector>
```

### Build Check
```bash
cd /Users/mihail/projects/vault/projects/arbitrage-trader && cargo test -p arb-polymarket --features mock
```

### Acceptance Criteria
- [ ] `PolymarketConnector` implements all 11 methods of `PredictionMarketConnector`
- [ ] `MockPolymarketConnector` implements all 11 methods
- [ ] Mock records placed/cancelled orders for test assertions
- [ ] Mock supports failure injection
- [ ] All unit tests pass
- [ ] `cargo test -p arb-polymarket --features mock` passes
- [ ] Both connector types are usable as `Box<dyn PredictionMarketConnector>`

### Output Contract
- **Files created:** `src/connector.rs`, `src/mock.rs`
- **Files modified:** `src/lib.rs` (final module structure)
- **Exports:** `PolymarketConnector`, `MockPolymarketConnector` (mock feature), `PolyConfig`, `PolymarketError`
- **Build status:** Must pass `cargo test --features mock`

---

## Prompt 3A: arb-kalshi -- Auth (RSA-SHA256)

### Task
Implement Kalshi authentication with RSA-SHA256 request signing. This is simpler than Polymarket's two-layer auth but still needs careful implementation.

### Context Files
- `/Users/mihail/projects/vault/projects/arbitrage-trader/SPEC.md` (Section 6.3)
- `/Users/mihail/projects/vault/projects/arbitrage-trader/crates/arb-types/src/error.rs` (ArbError)

### Files to Create
- `crates/arb-kalshi/src/auth.rs`
- `crates/arb-kalshi/src/types.rs`
- `crates/arb-kalshi/src/error.rs`

### Module Structure
```
arb-kalshi/
  src/
    lib.rs          -- module declarations, re-exports
    auth.rs         -- RSA-SHA256 request signing
    types.rs        -- Kalshi-specific API types
    error.rs        -- Connector-specific error type
```

### Deliverables

#### src/error.rs -- Connector Error Type
```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum KalshiError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("auth error: {0}")]
    Auth(String),
    #[error("websocket error: {0}")]
    WebSocket(String),
    #[error("rate limited")]
    RateLimited,
    #[error("api error {status}: {message}")]
    Api { status: u16, message: String },
    #[error("deserialization error: {0}")]
    Deserialize(#[from] serde_json::Error),
    #[error("rsa error: {0}")]
    Rsa(String),
}

impl From<KalshiError> for arb_types::ArbError {
    fn from(e: KalshiError) -> Self {
        // Map to appropriate ArbError variants
    }
}
```

#### src/types.rs -- Kalshi API Types
Define Kalshi-specific request/response structs:
- `KalshiConfig` -- credentials struct: `api_key_id`, `private_key_pem` (RSA private key as PEM string)
- `KalshiOrderRequest` -- JSON body for POST /portfolio/orders
- `KalshiOrderResponse` -- API response from order operations
- `KalshiMarketResponse` -- API response from GET /markets
- `KalshiBookResponse` -- API response from GET /markets/{ticker}/orderbook
- `KalshiPositionResponse` -- API response from GET /portfolio/positions
- `KalshiBalanceResponse` -- API response from GET /portfolio/balance
- `KalshiWsMessage` -- WebSocket incoming message enum (orderbook_delta, ticker, fill)

All structs derive `Serialize, Deserialize` with `#[serde(rename_all = "snake_case")]` (Kalshi uses snake_case JSON).

Important: Kalshi prices are in cents (1-99). Store raw in Kalshi types, convert to Decimal (0.01-0.99) in the trait impl using `arb_types::price::kalshi_cents_to_decimal()`.

#### src/auth.rs -- RSA-SHA256 Request Signing
```rust
use rsa::{RsaPrivateKey, pkcs8::DecodePrivateKey};
use rsa::pkcs1v15::SigningKey;
use sha2::Sha256;
use base64::{Engine as _, engine::general_purpose::STANDARD};

pub struct KalshiAuth {
    api_key_id: String,
    signing_key: SigningKey<Sha256>,
}

impl KalshiAuth {
    /// Create from API key ID and PEM-encoded RSA private key.
    pub fn new(api_key_id: String, private_key_pem: &str) -> Result<Self, KalshiError>;

    /// Sign a request. Returns (timestamp, signature) for headers.
    /// Message = timestamp_ms + method + path
    /// Signature = Base64(RSA-SHA256(message, private_key))
    pub fn sign_request(&self, method: &str, path: &str) -> (String, String);

    /// Build auth headers for a request.
    /// Returns: KALSHI-ACCESS-KEY, KALSHI-ACCESS-SIGNATURE, KALSHI-ACCESS-TIMESTAMP
    pub fn headers(&self, method: &str, path: &str) -> reqwest::header::HeaderMap;
}
```

Key implementation details:
- `timestamp` is current Unix epoch in **milliseconds** as string
- `message = timestamp_ms + method_uppercase + path` (no body, unlike Polymarket)
- `signature = Base64(RSA_PKCS1v15_SHA256(message, private_key))`
- PEM key loaded via `RsaPrivateKey::from_pkcs8_pem(pem_str)`
- The `rsa` crate's `SigningKey<Sha256>` with PKCS1v15 padding
- Method must be uppercase
- Path is the full path including `/trade-api/v2` prefix

### Unit Tests
```
tests in auth.rs:
  - test_sign_request_deterministic: sign same input twice (with mocked timestamp), verify same output
  - test_sign_request_verifiable: sign a message, verify with corresponding public key
  - test_headers_correct_keys: verify all 3 header names match Kalshi docs
  - test_new_from_pem: load a test PEM key, verify no error
  - test_new_invalid_pem: pass garbage, verify KalshiError::Auth returned
```

Note: Generate a test RSA key pair in the test module (do not use real credentials):
```rust
#[cfg(test)]
fn test_key_pair() -> (String, RsaPrivateKey) {
    use rsa::pkcs8::EncodePrivateKey;
    let mut rng = rand::thread_rng();
    let key = RsaPrivateKey::new(&mut rng, 2048).unwrap();
    let pem = key.to_pkcs8_pem(rsa::pkcs8::LineEnding::LF).unwrap();
    (pem.to_string(), key)
}
```

### Build Check
```bash
cd /Users/mihail/projects/vault/projects/arbitrage-trader && cargo test -p arb-kalshi
```

### Acceptance Criteria
- [ ] RSA key loading from PEM works
- [ ] Request signing produces verifiable signatures
- [ ] Headers use correct Kalshi header names
- [ ] Error handling for invalid keys works
- [ ] All unit tests pass

### Output Contract
- **Files created:** `src/auth.rs`, `src/types.rs`, `src/error.rs`
- **Files modified:** `src/lib.rs` (module declarations)
- **Exports:** `KalshiAuth`, `KalshiConfig`, `KalshiError`
- **Build status:** Must pass `cargo check` and `cargo test`

### Out of Scope
- REST client (Prompt 3B)
- WebSocket client (Prompt 3C)

---

## Prompt 3B: arb-kalshi -- REST Client

### Task
Implement the Kalshi REST client for market data and trading endpoints. Uses `KalshiAuth` from Prompt 3A. Includes dual rate limiters (D3, D5) for trading vs market data.

### Context Files
- `/Users/mihail/projects/vault/projects/arbitrage-trader/SPEC.md` (Section 6.3, endpoint table)
- `/Users/mihail/projects/vault/projects/arbitrage-trader/crates/arb-kalshi/src/auth.rs` (from 3A)
- `/Users/mihail/projects/vault/projects/arbitrage-trader/crates/arb-kalshi/src/types.rs` (from 3A)
- `/Users/mihail/projects/vault/projects/arbitrage-trader/crates/arb-types/src/market.rs`
- `/Users/mihail/projects/vault/projects/arbitrage-trader/crates/arb-types/src/order.rs`

### Files to Create
- `crates/arb-kalshi/src/client.rs`
- `crates/arb-kalshi/src/rate_limit.rs`

### Deliverables

#### src/rate_limit.rs -- Dual Rate Limiter
```rust
use governor::{Quota, RateLimiter, clock::DefaultClock, state::{InMemoryState, NotKeyed}};
use std::num::NonZeroU32;

pub struct KalshiRateLimiter {
    trading: RateLimiter<NotKeyed, InMemoryState, DefaultClock>,   // 10 req/s
    market_data: RateLimiter<NotKeyed, InMemoryState, DefaultClock>, // 100 req/s
}

impl KalshiRateLimiter {
    pub fn new() -> Self;

    /// Wait for trading rate limit (10 req/s). Used for order operations.
    pub async fn acquire_trading(&self);

    /// Wait for market data rate limit (100 req/s). Used for market/book queries.
    pub async fn acquire_market_data(&self);
}
```

This directly addresses gap G5. The tight 10 req/s trading limit means:
- Order monitoring MUST use `list_open_orders()` (1 request) not per-order polling
- Order placement + cancellation share the 10 req/s budget
- Market data queries have a separate 100 req/s budget

#### src/client.rs -- REST Client
```rust
pub struct KalshiClient {
    http: reqwest::Client,
    auth: KalshiAuth,
    rate_limiter: KalshiRateLimiter,
    base_url: String,  // https://trading-api.kalshi.com/trade-api/v2
}

impl KalshiClient {
    pub fn new(config: KalshiConfig) -> Result<Self, KalshiError>;

    // Market data (uses market_data rate limiter)
    pub async fn fetch_markets(&self, cursor: Option<&str>, status: Option<&str>) -> Result<Vec<KalshiMarketResponse>, KalshiError>;
    pub async fn fetch_market(&self, ticker: &str) -> Result<KalshiMarketResponse, KalshiError>;
    pub async fn fetch_order_book(&self, ticker: &str) -> Result<KalshiBookResponse, KalshiError>;

    // Trading (uses trading rate limiter -- 10 req/s!)
    pub async fn post_order(&self, req: &KalshiOrderRequest) -> Result<KalshiOrderResponse, KalshiError>;
    pub async fn cancel_order(&self, order_id: &str) -> Result<(), KalshiError>;
    pub async fn fetch_open_orders(&self) -> Result<Vec<KalshiOrderResponse>, KalshiError>;
    pub async fn fetch_order(&self, order_id: &str) -> Result<KalshiOrderResponse, KalshiError>;

    // Account (uses trading rate limiter)
    pub async fn fetch_positions(&self) -> Result<Vec<KalshiPositionResponse>, KalshiError>;
    pub async fn fetch_balance(&self) -> Result<KalshiBalanceResponse, KalshiError>;
}
```

Key implementation details:
- Market data endpoints use `rate_limiter.acquire_market_data()`
- Trading + account endpoints use `rate_limiter.acquire_trading()`
- Kalshi returns rate limit info in headers: `Ratelimit-Remaining`, `Ratelimit-Reset`. Log these via `tracing::debug!` for monitoring but rely on governor for enforcement.
- Prices in responses are cents (1-99). Keep as cents in Kalshi types; conversion to Decimal happens in the trait impl.
- HTTP 429 responses: parse `Ratelimit-Reset` header, map to `KalshiError::RateLimited`
- Kalshi uses cursor pagination for list endpoints

Endpoint mapping:
| Method | Path | Rate Limit | Use |
|--------|------|-----------|-----|
| GET | `/markets` | market_data | List markets |
| GET | `/markets/{ticker}` | market_data | Single market |
| GET | `/markets/{ticker}/orderbook` | market_data | Order book |
| POST | `/portfolio/orders` | trading | Place order |
| DELETE | `/portfolio/orders/{id}` | trading | Cancel order |
| GET | `/portfolio/orders` | trading | List open orders |
| GET | `/portfolio/positions` | trading | Positions |
| GET | `/portfolio/balance` | trading | Balance |

### Unit Tests
```
tests in client.rs:
  - test_fetch_market_deserialize: mock known JSON, verify correct KalshiMarketResponse
  - test_fetch_order_book_deserialize: mock book JSON with cents, verify struct
  - test_post_order_request_format: verify request body schema matches Kalshi API
  - test_dual_rate_limiter: verify trading and market_data have independent budgets
  - test_api_error_handling: mock 400, verify KalshiError::Api
  - test_429_handling: mock 429 with Ratelimit-Reset header
```

### Build Check
```bash
cd /Users/mihail/projects/vault/projects/arbitrage-trader && cargo test -p arb-kalshi
```

### Acceptance Criteria
- [ ] All REST methods implemented and compile
- [ ] Dual rate limiter enforces 10 req/s for trading, 100 req/s for market data
- [ ] Correct rate limiter used for each endpoint
- [ ] Prices stored as cents in Kalshi types (not yet converted)
- [ ] Unit tests pass

### Output Contract
- **Files created:** `src/client.rs`, `src/rate_limit.rs`
- **Files modified:** `src/lib.rs` (add module declarations)
- **Exports:** `KalshiClient`, `KalshiRateLimiter`
- **Build status:** Must pass `cargo check` and `cargo test`

---

## Prompt 3C: arb-kalshi -- WebSocket Client

### Task
Implement the Kalshi WebSocket client for real-time price feeds. Includes reconnection logic (reuses pattern from Prompt 2C) and session-token auth on connection.

### Context Files
- `/Users/mihail/projects/vault/projects/arbitrage-trader/SPEC.md` (Section 6.3, WebSocket details)
- `/Users/mihail/projects/vault/projects/arbitrage-trader/crates/arb-types/src/event.rs` (PriceUpdate, SubHandle)
- `/Users/mihail/projects/vault/projects/arbitrage-trader/crates/arb-kalshi/src/types.rs` (KalshiWsMessage)
- `/Users/mihail/projects/vault/projects/arbitrage-trader/crates/arb-polymarket/src/ws.rs` (reference implementation for reconnection pattern)

### Files to Create
- `crates/arb-kalshi/src/ws.rs`

### Deliverables

#### src/ws.rs -- WebSocket Client with Auth + Reconnection
```rust
use tokio::sync::mpsc;
use arb_types::{PriceUpdate, SubHandle};

pub struct KalshiWebSocket {
    url: String,
    auth: KalshiAuth,  // needed for auth on each new connection
    subscribed_ids: parking_lot::RwLock<Vec<String>>,
}

impl KalshiWebSocket {
    pub fn new(url: String, auth: KalshiAuth) -> Self;

    /// Subscribe to price updates for the given market tickers.
    /// Connection flow:
    /// 1. Connect to wss://trading-api.kalshi.com/trade-api/ws/v2
    /// 2. Authenticate: send signed auth message with API key + timestamp + signature
    /// 3. Subscribe to channels: orderbook_delta, ticker for each market
    /// 4. Parse incoming messages into PriceUpdate
    /// 5. Forward to mpsc::Sender
    /// 6. Reconnect with exponential backoff on disconnect (re-auth required)
    pub async fn subscribe(
        &self,
        tickers: &[String],
        tx: mpsc::Sender<PriceUpdate>,
    ) -> Result<SubHandle, KalshiError>;
}
```

Key implementation details:

**Authentication on connection:**
Unlike Polymarket (unauthenticated WS), Kalshi requires auth on each WebSocket connection:
1. Connect to WS endpoint
2. Send auth message: `{"id": 1, "cmd": "subscribe", "params": {"channels": ["auth"], "key_id": "<api_key>", "signature": "<rsa_sig>", "timestamp": "<ts_ms>"}}`
3. Wait for auth confirmation before subscribing to data channels

**Channel subscriptions:**
```json
{"id": 2, "cmd": "subscribe", "params": {"channels": ["orderbook_delta"], "market_tickers": ["TICKER1", "TICKER2"]}}
{"id": 3, "cmd": "subscribe", "params": {"channels": ["ticker"], "market_tickers": ["TICKER1", "TICKER2"]}}
```

**Message parsing:**
- `orderbook_delta` -- order book changes (price level updates in cents)
- `ticker` -- last trade price and volume for a market
- `fill` -- order fill notifications (useful for order monitoring, but primarily used by arb-engine)

Convert prices from cents to Decimal using `kalshi_cents_to_decimal()` before creating `PriceUpdate`.

**Reconnection strategy:**
Same as Polymarket (D4): exponential backoff, 1s initial, 30s max, 25% jitter, 10 max failures. On reconnect, must re-authenticate then re-subscribe.

**Cancellation:**
Same pattern as Polymarket: `oneshot::Sender` in SubHandle, `select!` in background task.

### Unit Tests
```
tests in ws.rs:
  - test_parse_orderbook_delta: parse known Kalshi orderbook_delta JSON, verify PriceUpdate with correct Decimal prices
  - test_parse_ticker_message: parse ticker JSON, verify prices converted from cents
  - test_auth_message_format: verify the auth subscription message matches expected schema
  - test_subscribe_message_format: verify channel subscription messages are correct
  - test_reconnect_reauths: verify that reconnection includes auth step (test the state machine)
```

### Build Check
```bash
cd /Users/mihail/projects/vault/projects/arbitrage-trader && cargo test -p arb-kalshi
```

### Acceptance Criteria
- [ ] WebSocket authenticates on connection
- [ ] Subscribes to orderbook_delta and ticker channels
- [ ] Prices correctly converted from cents to Decimal
- [ ] Reconnection re-authenticates before re-subscribing
- [ ] SubHandle cancellation works cleanly
- [ ] All unit tests pass

### Output Contract
- **Files created:** `src/ws.rs`
- **Files modified:** `src/lib.rs` (add module declaration)
- **Exports:** `KalshiWebSocket`
- **Build status:** Must pass `cargo check` and `cargo test`

---

## Prompt 3D: arb-kalshi -- Trait Implementation + Mock + Tests

### Task
Implement `PredictionMarketConnector` for Kalshi by wiring together auth, client, and WebSocket. Implement mock connector. This mirrors Prompt 2D's structure.

### Context Files
- `/Users/mihail/projects/vault/projects/arbitrage-trader/crates/arb-types/src/lib.rs` (PredictionMarketConnector trait)
- `/Users/mihail/projects/vault/projects/arbitrage-trader/crates/arb-types/src/price.rs` (kalshi_cents_to_decimal)
- `/Users/mihail/projects/vault/projects/arbitrage-trader/crates/arb-kalshi/src/client.rs` (from 3B)
- `/Users/mihail/projects/vault/projects/arbitrage-trader/crates/arb-kalshi/src/ws.rs` (from 3C)
- `/Users/mihail/projects/vault/projects/arbitrage-trader/crates/arb-kalshi/src/types.rs` (from 3A)
- `/Users/mihail/projects/vault/projects/arbitrage-trader/crates/arb-polymarket/src/mock.rs` (reference for mock design)

### Files to Create
- `crates/arb-kalshi/src/connector.rs`
- `crates/arb-kalshi/src/mock.rs`

### Deliverables

#### src/connector.rs -- Trait Implementation
```rust
use arb_types::*;
use async_trait::async_trait;

pub struct KalshiConnector {
    client: KalshiClient,
    ws: KalshiWebSocket,
}

impl KalshiConnector {
    pub fn new(config: KalshiConfig) -> Result<Self, KalshiError>;
}

#[async_trait]
impl PredictionMarketConnector for KalshiConnector {
    fn platform(&self) -> Platform { Platform::Kalshi }

    async fn list_markets(&self, status: MarketStatus) -> Result<Vec<Market>, ArbError> {
        // Paginate through client.fetch_markets(), filter by status
        // Convert KalshiMarketResponse -> Market
        // IMPORTANT: Convert prices from cents to Decimal here
    }

    async fn get_market(&self, id: &str) -> Result<Market, ArbError> {
        // id is the Kalshi ticker
        // client.fetch_market(id) -> convert, including price normalization
    }

    async fn get_order_book(&self, id: &str) -> Result<OrderBook, ArbError> {
        // id is the Kalshi ticker
        // client.fetch_order_book(id) -> convert
        // IMPORTANT: OrderBookLevel prices must be Decimal (0.01-0.99), not cents
        // Use kalshi_cents_to_decimal() for each level
    }

    async fn subscribe_prices(
        &self,
        ids: &[String],
        tx: tokio::sync::mpsc::Sender<PriceUpdate>,
    ) -> Result<SubHandle, ArbError> {
        // ids are Kalshi tickers
        // ws.subscribe(ids, tx)
        // Price conversion happens inside ws.rs already
    }

    async fn place_limit_order(&self, req: &LimitOrderRequest) -> Result<OrderResponse, ArbError> {
        // Convert LimitOrderRequest to KalshiOrderRequest
        // IMPORTANT: Convert Decimal price to cents for Kalshi API
        // price_cents = (req.price * 100).to_u32()
        // client.post_order(kalshi_req) -> convert response back
    }

    async fn cancel_order(&self, order_id: &str) -> Result<(), ArbError> {
        // client.cancel_order(order_id)
    }

    async fn get_order(&self, order_id: &str) -> Result<OrderResponse, ArbError> {
        // client.fetch_order(order_id) -> convert, normalize prices
    }

    async fn list_open_orders(&self) -> Result<Vec<OrderResponse>, ArbError> {
        // client.fetch_open_orders() -> convert each
        // This is a SINGLE request (addresses G5 batching)
    }

    async fn get_balance(&self) -> Result<rust_decimal::Decimal, ArbError> {
        // client.fetch_balance() -> extract balance as Decimal
        // Kalshi balance is in dollars, no conversion needed
    }

    async fn get_positions(&self) -> Result<Vec<PlatformPosition>, ArbError> {
        // client.fetch_positions() -> convert each
        // Normalize prices from cents to Decimal
    }
}
```

Critical price conversion notes (Kalshi-specific):
- **Inbound** (API -> arb-types): multiply cents by 0.01 via `kalshi_cents_to_decimal()`
- **Outbound** (arb-types -> API): multiply Decimal by 100, truncate to u32
- This conversion happens in the connector layer, NOT in arb-types or the client layer
- The client layer (`KalshiClient`) works with raw Kalshi types (cents)
- The connector layer (`KalshiConnector`) converts at the trait boundary

#### src/mock.rs -- Mock Connector (feature-gated)
```rust
#[cfg(feature = "mock")]
pub mod mock {
    use arb_types::*;
    use async_trait::async_trait;
    use std::sync::Arc;
    use parking_lot::Mutex;

    #[derive(Debug, Default)]
    pub struct MockState {
        pub markets: Vec<Market>,
        pub order_books: std::collections::HashMap<String, OrderBook>,
        pub orders: Vec<OrderResponse>,
        pub positions: Vec<PlatformPosition>,
        pub balance: rust_decimal::Decimal,
        pub placed_orders: Vec<LimitOrderRequest>,
        pub cancelled_orders: Vec<String>,
        pub should_fail: Option<ArbError>,
        pub price_updates: Vec<PriceUpdate>,
    }

    pub struct MockKalshiConnector {
        pub state: Arc<Mutex<MockState>>,
    }

    impl MockKalshiConnector {
        pub fn new() -> Self;
        pub fn with_state(state: Arc<Mutex<MockState>>) -> Self;
    }

    #[async_trait]
    impl PredictionMarketConnector for MockKalshiConnector {
        fn platform(&self) -> Platform { Platform::Kalshi }
        // Same mock pattern as MockPolymarketConnector
    }
}
```

### Update src/lib.rs
```rust
pub mod auth;
pub mod types;
pub mod error;
pub mod client;
pub mod rate_limit;
pub mod ws;
pub mod connector;

#[cfg(feature = "mock")]
pub mod mock;

pub use connector::KalshiConnector;
pub use types::KalshiConfig;
pub use error::KalshiError;

#[cfg(feature = "mock")]
pub use mock::MockKalshiConnector;
```

### Unit Tests
```
tests in connector.rs:
  - test_market_conversion_price_normalization: KalshiMarketResponse with cents -> Market with Decimal
  - test_order_book_cents_to_decimal: verify each OrderBookLevel price is 0.xx not xx
  - test_order_request_decimal_to_cents: LimitOrderRequest with 0.42 -> KalshiOrderRequest with 42
  - test_order_response_cents_to_decimal: verify OrderResponse prices are Decimal

tests in mock.rs:
  - test_mock_list_markets: inject, retrieve, verify
  - test_mock_place_order: place, verify recorded
  - test_mock_cancel_order: cancel, verify recorded
  - test_mock_failure_injection: set should_fail, verify error
  - test_mock_subscribe_prices: inject updates, verify delivery
  - test_mock_trait_object: verify Box<dyn PredictionMarketConnector> works
```

### Build Check
```bash
cd /Users/mihail/projects/vault/projects/arbitrage-trader && cargo test -p arb-kalshi --features mock
```

### Acceptance Criteria
- [ ] `KalshiConnector` implements all 11 methods of `PredictionMarketConnector`
- [ ] All price conversions correct (cents <-> Decimal at trait boundary)
- [ ] `MockKalshiConnector` implements all 11 methods
- [ ] Mock records placed/cancelled orders
- [ ] Mock supports failure injection
- [ ] All unit tests pass, especially price conversion tests
- [ ] `cargo test -p arb-kalshi --features mock` passes
- [ ] Both types usable as `Box<dyn PredictionMarketConnector>`

### Output Contract
- **Files created:** `src/connector.rs`, `src/mock.rs`
- **Files modified:** `src/lib.rs` (final module structure)
- **Exports:** `KalshiConnector`, `MockKalshiConnector` (mock feature), `KalshiConfig`, `KalshiError`
- **Build status:** Must pass `cargo test --features mock`

---

## Phase 2 Completion Criteria (All Prompts)

- [ ] `cargo check --workspace` passes
- [ ] `cargo test -p arb-polymarket --features mock` -- all tests pass
- [ ] `cargo test -p arb-kalshi --features mock` -- all tests pass
- [ ] Both `PolymarketConnector` and `KalshiConnector` implement `PredictionMarketConnector`
- [ ] Both `MockPolymarketConnector` and `MockKalshiConnector` implement `PredictionMarketConnector`
- [ ] All four types are usable as `Box<dyn PredictionMarketConnector>`
- [ ] HMAC-SHA256 auth works for Polymarket (unit tests with known vectors)
- [ ] EIP-712 signing produces valid typed data signatures (unit tests)
- [ ] RSA-SHA256 auth works for Kalshi (unit tests with generated key pair)
- [ ] WebSocket clients include reconnection with exponential backoff
- [ ] Rate limiters enforce: Polymarket 100 req/s, Kalshi 10 req/s trading + 100 req/s data
- [ ] Kalshi order monitoring uses batched `list_open_orders()` (gap G5 resolved)
- [ ] No new types needed in arb-types (gap G1 confirmed resolved from Phase 1)
- [ ] WebSocket reconnection strategy implemented (gap G3 resolved)

## Gap Resolutions Summary

| Gap | Resolution | Prompt |
|-----|-----------|--------|
| G1 (undefined types) | All types already defined in Phase 1 arb-types. No new shared types needed. | N/A |
| G3 (WebSocket reconnection) | Exponential backoff: 1s initial, 30s max, 25% jitter, 10 max failures. Re-subscribe on reconnect. | 2C, 3C |
| G5 (Kalshi rate batching) | Dual rate limiter (10 trading / 100 data). Order monitoring via `list_open_orders()`. WebSocket fill channel for real-time status. | 3B, 3D |

---

## File Summary

### arb-polymarket (8 files)
```
crates/arb-polymarket/
  Cargo.toml          -- (modified) full dependency list
  src/
    lib.rs            -- (modified) module declarations + re-exports
    error.rs          -- (new) PolymarketError type
    types.rs          -- (new) Polymarket API request/response types + PolyConfig
    auth.rs           -- (new) HMAC-SHA256 request signing
    signing.rs        -- (new) EIP-712 typed data order signing
    client.rs         -- (new) REST client for all HTTP endpoints
    rate_limit.rs     -- (new) Token bucket rate limiter (100 req/s)
    ws.rs             -- (new) WebSocket client with reconnection
    connector.rs      -- (new) PredictionMarketConnector trait impl
    mock.rs           -- (new) MockPolymarketConnector (feature-gated)
```

### arb-kalshi (8 files)
```
crates/arb-kalshi/
  Cargo.toml          -- (modified) full dependency list
  src/
    lib.rs            -- (modified) module declarations + re-exports
    error.rs          -- (new) KalshiError type
    types.rs          -- (new) Kalshi API request/response types + KalshiConfig
    auth.rs           -- (new) RSA-SHA256 request signing
    client.rs         -- (new) REST client for all HTTP endpoints
    rate_limit.rs     -- (new) Dual token bucket (10 trading / 100 data)
    ws.rs             -- (new) WebSocket client with auth + reconnection
    connector.rs      -- (new) PredictionMarketConnector trait impl
    mock.rs           -- (new) MockKalshiConnector (feature-gated)
```

### Root workspace (1 file modified)
```
Cargo.toml            -- (modified) add governor dependency
```

Total: 3 modified files, 18 new files across 9 prompts.
