# SPEC.md — Rust Cross-Platform Prediction Market Arbitrage System

**Version**: 2.0.0  
**Date**: 2026-04-05  
**Status**: Draft — Ready for Implementation

---

## Table of Contents

1. [Project Overview & PRD](#1-project-overview--prd)
2. [Technical Specification](#2-technical-specification)
3. [API Specification](#3-api-specification)
4. [Data Models](#4-data-models)
5. [Database Schema](#5-database-schema)
6. [Platform Connectors](#6-platform-connectors)
7. [Market Matching Engine](#7-market-matching-engine)
8. [Arbitrage Engine](#8-arbitrage-engine)
9. [Order Management](#9-order-management)
10. [Risk Management](#10-risk-management)
11. [Configuration](#11-configuration)
12. [Testing Strategy](#12-testing-strategy)
13. [Deployment](#13-deployment)
14. [Implementation Roadmap](#14-implementation-roadmap)

---

## 1. Project Overview & PRD

### 1.1 Product Vision

A production-grade cross-platform prediction market arbitrage system built in Rust. The system detects price discrepancies for equivalent binary-outcome markets across Polymarket and Kalshi, then places limit orders on both platforms to capture the spread with zero maker fees.

### 1.2 Problem Statement

Prediction markets for the same real-world event are priced differently across platforms. Polymarket (crypto-native, unregulated) and Kalshi (CFTC-regulated, USD-based) attract different user bases with different information, risk appetites, and capital constraints. This creates persistent price discrepancies of 2%–10% on equivalent markets, lasting minutes to hours.

Unlike crypto arbitrage (dominated by HFT firms, sub-second windows, 0.4%+ fees eating margins), prediction market arbitrage has:
- **Wider spreads**: 2%–10% vs 0.05%–0.5%
- **Longer windows**: minutes/hours vs milliseconds
- **Lower fees**: 0% maker on both platforms
- **Less competition**: retail-dominated, few professional arbitrageurs
- **Guaranteed profit**: binary outcomes mean perfect hedging is possible

### 1.3 How It Works

```
Polymarket:  "Will X happen?" YES = $0.40  (implies 40% probability)
Kalshi:      "Will X happen?" YES = $0.48  (implies 48% probability)

Action: Buy YES on Polymarket at $0.40, Buy NO on Kalshi at $0.52

If X happens:   Polymarket pays $1.00, Kalshi pays $0.00 → net = $1.00 - $0.40 - $0.52 = +$0.08
If X doesn't:   Polymarket pays $0.00, Kalshi pays $1.00 → net = $0.00 + $1.00 - $0.40 - $0.52 = +$0.08

Guaranteed $0.08 profit per share regardless of outcome.
```

### 1.4 Goals

**Primary Goals (MVP)**
- G1: Match equivalent markets across Polymarket and Kalshi automatically with human verification
- G2: Detect cross-platform arbitrage opportunities with < 1s detection latency
- G3: Place limit orders on both platforms to capture the spread at 0% maker fees
- G4: Manage order lifecycle — post, monitor, cancel stale, replace
- G5: Track positions, P&L, and settlement across both platforms
- G6: Persist all trades, opportunities, and market pairs to SQLite

**Secondary Goals (Phase 2+)**
- G7: Add Manifold Markets and PredictIt connectors
- G8: Web dashboard for monitoring
- G9: Automated market matching via NLP/fuzzy matching
- G10: Multi-outcome market arbitrage (not just binary)

### 1.5 Non-Goals

- **NOT** a market-making system — we don't provide liquidity, we exploit mispricings
- **NOT** a betting/prediction system — we are direction-neutral
- **NOT** multi-user SaaS — single-operator deployment
- **NOT** a high-frequency system — opportunities last minutes, not milliseconds

### 1.6 Success Metrics

| Metric | MVP Target | Phase 2 Target |
|--------|-----------|----------------|
| Market pair matching accuracy | > 95% (with human review) | > 99% (automated) |
| Opportunity detection latency | < 1s | < 500ms |
| Limit order fill rate | > 60% | > 75% |
| Trade win rate | 100% (hedged) | 100% |
| Net monthly return on capital | 3%–8% | 5%–15% |
| System uptime | 99% | 99.9% |
| Max capital at risk (unhedged) | < $500 | < $200 |

### 1.7 MVP Scope

**In scope:**
- `arb-types`: Shared domain types (markets, orders, positions, prices)
- `arb-polymarket`: Polymarket CLOB connector (REST + WebSocket + EIP-712 signing)
- `arb-kalshi`: Kalshi connector (REST + WebSocket + RSA signing)
- `arb-matcher`: Market matching engine (fuzzy + manual mapping)
- `arb-engine`: Opportunity detection, order placement, position tracking
- `arb-risk`: Risk limits, exposure tracking, unhedged position alerts
- `arb-db`: SQLite persistence
- `arb-cli`: Binary entry point, config loading, TUI status display

**Out of scope for MVP:**
- Web dashboard
- Additional platforms (Manifold, PredictIt)
- Automated NLP market matching
- Multi-outcome markets
- REST API server (CLI-only for MVP)

### 1.8 Assumptions & Constraints

- **A1**: Operator has funded accounts on both Polymarket and Kalshi
- **A2**: Operator has API credentials for both platforms
- **A3**: Market pairs are manually verified before trading (automated matching proposes, human confirms)
- **A4**: Both platforms support limit orders with 0% maker fees
- **A5**: Operator handles fund transfers between platforms manually
- **C1**: Rust stable toolchain 1.85+
- **C2**: Single-binary deployment
- **C3**: All secrets in environment variables or `.env`
- **C4**: Kalshi is US-only; operator must have US access

---

## 2. Technical Specification

### 2.1 Workspace Structure

```
prediction-arb/
├── Cargo.toml                          # Workspace root
├── Cargo.lock
├── config/
│   ├── default.toml
│   └── pairs.toml                      # Verified market pairs
├── migrations/
│   └── 001_initial_schema.sql
├── .env.example
├── README.md
├── SPEC.md
└── crates/
    ├── arb-types/
    │   └── src/ (lib.rs, market.rs, order.rs, position.rs, opportunity.rs, price.rs, error.rs, event.rs)
    ├── arb-polymarket/
    │   └── src/ (lib.rs, client.rs, auth.rs, signing.rs, ws.rs, types.rs)
    ├── arb-kalshi/
    │   └── src/ (lib.rs, client.rs, auth.rs, ws.rs, types.rs)
    ├── arb-matcher/
    │   └── src/ (lib.rs, fuzzy.rs, store.rs)
    ├── arb-engine/
    │   └── src/ (lib.rs, detector.rs, executor.rs, tracker.rs)
    ├── arb-risk/
    │   └── src/ (lib.rs, manager.rs, limits.rs, exposure.rs)
    ├── arb-db/
    │   └── src/ (lib.rs, repo.rs, models.rs)
    └── arb-cli/
        └── src/ (main.rs, tui.rs)
```

### 2.2 Workspace Cargo.toml

```toml
[workspace]
resolver = "2"
members = [
    "crates/arb-types",
    "crates/arb-polymarket",
    "crates/arb-kalshi",
    "crates/arb-matcher",
    "crates/arb-engine",
    "crates/arb-risk",
    "crates/arb-db",
    "crates/arb-cli",
]

[workspace.dependencies]
tokio = { version = "1.44", features = ["full"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
rust_decimal = { version = "1.36", features = ["serde-with-str"] }
rust_decimal_macros = "1.36"
chrono = { version = "0.4", features = ["serde"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
thiserror = "2.0"
anyhow = "1.0"
uuid = { version = "1.11", features = ["v7", "serde"] }
reqwest = { version = "0.12", features = ["json", "rustls-tls"] }
tokio-tungstenite = { version = "0.26", features = ["rustls-tls-native-roots"] }
sqlx = { version = "0.8", features = ["runtime-tokio", "sqlite", "chrono", "uuid", "json"] }
hmac = "0.12"
sha2 = "0.10"
hex = "0.4"
base64 = "0.22"
rsa = "0.9"
alloy-signer = "0.9"
alloy-signer-local = "0.9"
alloy-primitives = "0.8"
alloy-sol-types = "0.8"
dashmap = "6.1"
config = { version = "0.15", features = ["toml"] }
dotenvy = "0.15"
futures = "0.3"
futures-util = "0.3"
async-trait = "0.1"
parking_lot = "0.12"
strsim = "0.11"
ratatui = "0.29"
crossterm = "0.28"

arb-types       = { path = "crates/arb-types" }
arb-polymarket  = { path = "crates/arb-polymarket" }
arb-kalshi      = { path = "crates/arb-kalshi" }
arb-matcher     = { path = "crates/arb-matcher" }
arb-engine      = { path = "crates/arb-engine" }
arb-risk        = { path = "crates/arb-risk" }
arb-db          = { path = "crates/arb-db" }

[profile.release]
opt-level = 3
lto = "thin"
codegen-units = 1
panic = "abort"
```

### 2.3 Dependency Graph

```
arb-cli (binary: arb)
  ├── arb-engine
  │     ├── arb-polymarket ──► arb-types
  │     ├── arb-kalshi     ──► arb-types
  │     ├── arb-matcher    ──► arb-types
  │     ├── arb-risk       ──► arb-types
  │     ├── arb-db         ──► arb-types
  │     └── arb-types
  ├── arb-db       ──► arb-types
  └── arb-types
```

No circular dependencies. `arb-types` has zero internal crate dependencies.

---

## 3. API Specification

No REST API server in MVP. The system is CLI-only with a TUI (terminal UI) for monitoring.

### 3.1 TUI Dashboard

```
┌─ Prediction Market Arbitrage ──────────────────────────────────┐
│ Status: RUNNING    Capital: $12,340    Today P&L: +$47.20      │
├─ Active Pairs (12) ────────────────────────────────────────────┤
│ Pair                    Poly    Kalshi   Spread   Status        │
│ Trump wins 2026         0.42    0.48     6.0%     ARBITRAGE     │
│ Fed rate cut July       0.65    0.63     2.0%     WATCHING      │
│ BTC > 100k Dec          0.71    0.72     1.0%     BELOW_THRESH  │
├─ Open Orders (3) ──────────────────────────────────────────────┤
│ Platform    Market              Side   Price   Qty   Age        │
│ Polymarket  Trump wins 2026     YES    0.41    50    12s        │
│ Kalshi      Trump wins 2026     NO     0.53    50    12s        │
│ Polymarket  Fed rate cut July   YES    0.64    30    3s         │
├─ Positions (2) ────────────────────────────────────────────────┤
│ Market                  Poly Pos    Kalshi Pos   Hedged   P&L   │
│ Trump wins 2026         50 YES      50 NO        YES      +$3   │
│ Bitcoin ETF approved    100 YES     100 NO       YES      +$8   │
├─ Recent Trades (5) ────────────────────────────────────────────┤
│ Time     Market                  Action            Profit       │
│ 14:32    Trump wins 2026         Both legs filled   +$4.00      │
│ 14:28    Fed rate cut July       Cancelled (stale)  $0.00       │
│ 13:55    GDP > 3%                Both legs filled   +$6.50      │
└────────────────────────────────────────────────────────────────┘
  [q]uit  [p]ause  [r]esume  [m]arkets  [o]rders
```

---

## 4. Data Models

### 4.1 Core Types (arb-types)

#### Platform

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Polymarket,
    Kalshi,
}
```

#### Market

```rust
/// A single binary-outcome market on one platform.
pub struct Market {
    pub id: MarketId,
    pub platform: Platform,
    pub platform_id: String,          // condition_id (Poly) or ticker (Kalshi)
    pub question: String,
    pub yes_price: Decimal,           // 0.00–1.00
    pub no_price: Decimal,            // 0.00–1.00 (should be ~1 - yes_price)
    pub volume: Decimal,
    pub liquidity: Decimal,
    pub status: MarketStatus,         // Open, Closed, Settled
    pub close_time: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

#### MarketPair

```rust
/// Two markets on different platforms representing the same real-world event.
pub struct MarketPair {
    pub id: Uuid,
    pub polymarket: MarketRef,        // condition_id + yes_token_id + no_token_id
    pub kalshi: MarketRef,            // market ticker
    pub match_confidence: f64,        // 0.0–1.0
    pub verified: bool,               // human-verified as equivalent
    pub active: bool,
    pub created_at: DateTime<Utc>,
}

pub struct MarketRef {
    pub platform_id: String,
    pub question: String,
    pub close_time: DateTime<Utc>,
}
```

#### Opportunity

```rust
/// A detected arbitrage opportunity across a market pair.
pub struct Opportunity {
    pub id: Uuid,
    pub pair_id: Uuid,
    pub poly_side: Side,              // YES or NO — what to buy on Polymarket
    pub poly_price: Decimal,          // price to buy at on Polymarket
    pub kalshi_side: Side,            // opposite side — what to buy on Kalshi
    pub kalshi_price: Decimal,        // price to buy at on Kalshi
    pub spread: Decimal,              // guaranteed profit per share (e.g., 0.08)
    pub spread_pct: Decimal,          // spread as percentage
    pub max_quantity: u32,            // limited by book depth and risk
    pub detected_at: DateTime<Utc>,
    pub status: OpportunityStatus,    // Detected, Executing, Filled, Expired, Failed
}
```

#### Order

```rust
pub struct Order {
    pub id: Uuid,
    pub opportunity_id: Uuid,
    pub platform: Platform,
    pub platform_order_id: Option<String>,
    pub market_id: String,            // platform-specific market ID
    pub side: Side,                   // YES or NO
    pub price: Decimal,               // limit price (0.00–1.00)
    pub quantity: u32,                // number of contracts/shares
    pub filled_quantity: u32,
    pub order_type: OrderType,        // Limit (always limit for MVP)
    pub status: OrderStatus,          // Pending, Open, PartialFill, Filled, Cancelled, Failed
    pub placed_at: DateTime<Utc>,
    pub filled_at: Option<DateTime<Utc>>,
    pub cancelled_at: Option<DateTime<Utc>>,
    pub cancel_reason: Option<String>,
}
```

#### Position

```rust
/// A hedged position across both platforms for one market pair.
pub struct Position {
    pub id: Uuid,
    pub pair_id: Uuid,
    pub poly_side: Side,
    pub poly_quantity: u32,
    pub poly_avg_price: Decimal,
    pub kalshi_side: Side,
    pub kalshi_quantity: u32,
    pub kalshi_avg_price: Decimal,
    pub hedged_quantity: u32,         // min(poly_qty, kalshi_qty)
    pub unhedged_quantity: i32,       // positive = excess poly, negative = excess kalshi
    pub guaranteed_profit: Decimal,   // hedged_qty * spread
    pub status: PositionStatus,       // Open, SettledPoly, SettledKalshi, FullySettled
    pub opened_at: DateTime<Utc>,
    pub settled_at: Option<DateTime<Utc>>,
}
```

### 4.2 Key Invariants

- All prices are `Decimal` in the range `[0.00, 1.00]` — normalized across platforms
- Kalshi prices (1–99 cents) are converted to decimal on ingestion: `cents / 100`
- All IDs are UUID v7 (time-ordered)
- `Side` is always `YES` or `NO` — never `Buy`/`Sell` (clarity for prediction markets)
- A hedged position always has one YES leg and one NO leg on opposite platforms
- `spread = 1.00 - poly_price - kalshi_price` (must be > 0 for arbitrage)

---

## 5. Database Schema

### 5.1 SQLite Schema

```sql
-- Verified market pairs across platforms
CREATE TABLE market_pairs (
    id TEXT PRIMARY KEY,                    -- UUID v7
    poly_condition_id TEXT NOT NULL,
    poly_yes_token_id TEXT NOT NULL,
    poly_no_token_id TEXT NOT NULL,
    poly_question TEXT NOT NULL,
    kalshi_ticker TEXT NOT NULL,
    kalshi_question TEXT NOT NULL,
    match_confidence REAL NOT NULL,
    verified INTEGER NOT NULL DEFAULT 0,    -- boolean
    active INTEGER NOT NULL DEFAULT 1,
    close_time TEXT NOT NULL,               -- ISO 8601
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- Detected arbitrage opportunities
CREATE TABLE opportunities (
    id TEXT PRIMARY KEY,
    pair_id TEXT NOT NULL REFERENCES market_pairs(id),
    poly_side TEXT NOT NULL,                -- 'yes' or 'no'
    poly_price TEXT NOT NULL,               -- decimal as string
    kalshi_side TEXT NOT NULL,
    kalshi_price TEXT NOT NULL,
    spread TEXT NOT NULL,
    spread_pct TEXT NOT NULL,
    max_quantity INTEGER NOT NULL,
    status TEXT NOT NULL DEFAULT 'detected',
    detected_at TEXT NOT NULL,
    executed_at TEXT,
    resolved_at TEXT
);

-- Orders placed on platforms
CREATE TABLE orders (
    id TEXT PRIMARY KEY,
    opportunity_id TEXT NOT NULL REFERENCES opportunities(id),
    platform TEXT NOT NULL,                 -- 'polymarket' or 'kalshi'
    platform_order_id TEXT,
    market_id TEXT NOT NULL,
    side TEXT NOT NULL,
    price TEXT NOT NULL,
    quantity INTEGER NOT NULL,
    filled_quantity INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL DEFAULT 'pending',
    placed_at TEXT NOT NULL,
    filled_at TEXT,
    cancelled_at TEXT,
    cancel_reason TEXT
);

-- Hedged positions
CREATE TABLE positions (
    id TEXT PRIMARY KEY,
    pair_id TEXT NOT NULL REFERENCES market_pairs(id),
    poly_side TEXT NOT NULL,
    poly_quantity INTEGER NOT NULL,
    poly_avg_price TEXT NOT NULL,
    kalshi_side TEXT NOT NULL,
    kalshi_quantity INTEGER NOT NULL,
    kalshi_avg_price TEXT NOT NULL,
    hedged_quantity INTEGER NOT NULL,
    unhedged_quantity INTEGER NOT NULL DEFAULT 0,
    guaranteed_profit TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'open',
    opened_at TEXT NOT NULL,
    settled_at TEXT
);

-- Price snapshots for analysis
CREATE TABLE price_snapshots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    pair_id TEXT NOT NULL REFERENCES market_pairs(id),
    poly_yes_price TEXT NOT NULL,
    kalshi_yes_price TEXT NOT NULL,
    spread TEXT NOT NULL,
    captured_at TEXT NOT NULL
);

-- Daily P&L summary
CREATE TABLE daily_pnl (
    date TEXT PRIMARY KEY,                  -- YYYY-MM-DD
    trades_executed INTEGER NOT NULL DEFAULT 0,
    trades_filled INTEGER NOT NULL DEFAULT 0,
    gross_profit TEXT NOT NULL DEFAULT '0',
    fees_paid TEXT NOT NULL DEFAULT '0',
    net_profit TEXT NOT NULL DEFAULT '0',
    capital_deployed TEXT NOT NULL DEFAULT '0'
);

CREATE INDEX idx_opportunities_pair ON opportunities(pair_id, status);
CREATE INDEX idx_orders_opportunity ON orders(opportunity_id);
CREATE INDEX idx_orders_status ON orders(status) WHERE status IN ('pending', 'open');
CREATE INDEX idx_positions_status ON positions(status) WHERE status = 'open';
CREATE INDEX idx_snapshots_pair_time ON price_snapshots(pair_id, captured_at);
```

---

## 6. Platform Connectors

### 6.1 Common Trait

```rust
#[async_trait]
pub trait PredictionMarketConnector: Send + Sync + 'static {
    fn platform(&self) -> Platform;

    // Market data
    async fn list_markets(&self, status: MarketStatus) -> Result<Vec<Market>>;
    async fn get_market(&self, id: &str) -> Result<Market>;
    async fn get_order_book(&self, id: &str) -> Result<OrderBook>;
    async fn subscribe_prices(&self, ids: &[String], tx: mpsc::Sender<PriceUpdate>) -> Result<SubHandle>;

    // Trading
    async fn place_limit_order(&self, req: &LimitOrderRequest) -> Result<OrderResponse>;
    async fn cancel_order(&self, order_id: &str) -> Result<()>;
    async fn get_order(&self, order_id: &str) -> Result<OrderResponse>;
    async fn list_open_orders(&self) -> Result<Vec<OrderResponse>>;

    // Account
    async fn get_balance(&self) -> Result<Decimal>;
    async fn get_positions(&self) -> Result<Vec<PlatformPosition>>;
}
```

### 6.2 Polymarket Connector (arb-polymarket)

**Authentication (two layers):**

1. **API auth** — HMAC-SHA256
   - Register API key: sign a message with Polygon wallet private key → `POST /auth/api-key` → receive `apiKey`, `secret`, `passphrase`
   - Request signing: `HMAC-SHA256(timestamp + method + path + body, base64_decode(secret))`
   - Headers: `POLY_API_KEY`, `POLY_SIGNATURE`, `POLY_TIMESTAMP`, `POLY_PASSPHRASE`

2. **Order signing** — EIP-712
   - Orders are signed typed data (EIP-712) with the Polygon wallet private key
   - Order struct: `maker`, `taker`, `tokenId`, `makerAmount`, `takerAmount`, `nonce`, `expiration`, `feeRateBps`, `side`, `signatureType`
   - Use `alloy-signer-local` for signing

**REST API** — Base: `https://clob.polymarket.com`

| Endpoint | Method | Use |
|---|---|---|
| `/markets` | GET | List markets (paginated) |
| `/book` | GET | Order book for a token_id |
| `/price` | GET | Mid-market price |
| `/order` | POST | Place signed limit order |
| `/order/{id}` | DELETE | Cancel order |
| `/orders` | GET | Open orders |
| `/positions` | GET | Current positions |

**WebSocket** — `wss://ws-subscriptions-clob.polymarket.com/ws`
- Subscribe: `{"type": "subscribe", "channel": "market", "assets_ids": ["<token_id>"]}`
- Receives order book deltas and last trade prices

**Market metadata** — Gamma API: `https://gamma-api.polymarket.com/markets`
- Richer metadata: descriptions, tags, resolution sources, slugs

**Fees**: 0% maker, ~2% taker. We always use limit orders → 0%.

**Rate limits**: ~100 req/s authenticated.

**Price format**: `0.00–1.00` in USDC.e per share.

### 6.3 Kalshi Connector (arb-kalshi)

**Authentication:**
- API keys: RSA key pair generated in Kalshi dashboard
- Request signing: `RSA-SHA256(timestamp + method + path)` with private key (PEM)
- Headers: `KALSHI-ACCESS-KEY`, `KALSHI-ACCESS-SIGNATURE`, `KALSHI-ACCESS-TIMESTAMP`

**REST API** — Base: `https://trading-api.kalshi.com/trade-api/v2`

| Endpoint | Method | Use |
|---|---|---|
| `/markets` | GET | List markets (filter by event_ticker, status) |
| `/markets/{ticker}` | GET | Single market details |
| `/markets/{ticker}/orderbook` | GET | Order book |
| `/portfolio/orders` | POST | Place order |
| `/portfolio/orders/{id}` | DELETE | Cancel order |
| `/portfolio/orders` | GET | List open orders |
| `/portfolio/positions` | GET | Current positions |
| `/portfolio/balance` | GET | Account balance |

**WebSocket** — `wss://trading-api.kalshi.com/trade-api/ws/v2`
- Channels: `orderbook_delta`, `ticker`, `fill`
- Auth via session token on connection

**Fees**: 0% maker (with incentive programs), ~2 cents/contract taker. We always use limit orders.

**Rate limits**: 10 req/s trading, 100 req/s market data. Headers: `Ratelimit-Remaining`, `Ratelimit-Reset`.

**Price format**: 1–99 cents. Normalize on ingestion: `price_decimal = cents / 100`.

---

## 7. Market Matching Engine

### 7.1 Overview

The matcher identifies equivalent markets across Polymarket and Kalshi. Since there is no shared identifier, matching uses fuzzy text similarity + human verification.

### 7.2 Matching Pipeline

```
1. Pull all active markets from both platforms
2. For each Polymarket market, score against all Kalshi markets:
   a. Fuzzy string similarity on question text (Jaro-Winkler)
   b. Close-time proximity (must be within 48 hours)
   c. Category/tag overlap (if available)
3. Filter candidates with score > 0.7
4. Present top candidates to operator for verification
5. Verified pairs are stored in pairs.toml and the database
6. Re-scan periodically for new markets
```

### 7.3 Pair Configuration (pairs.toml)

```toml
[[pair]]
poly_condition_id = "0xabc123..."
poly_yes_token_id = "12345..."
poly_no_token_id = "67890..."
kalshi_ticker = "PRES-2026-DEM"
label = "Democrat wins 2026 presidential election"
verified = true
active = true

[[pair]]
poly_condition_id = "0xdef456..."
poly_yes_token_id = "11111..."
poly_no_token_id = "22222..."
kalshi_ticker = "FED-RATE-2026-JUL"
label = "Fed cuts rate in July 2026"
verified = true
active = true
```

### 7.4 Resolution Equivalence

Two markets are equivalent **only if**:
- They ask the same question about the same event
- They resolve using the same or compatible criteria
- They have the same or very close resolution dates
- YES on one platform means YES on the other (no inversion)

The operator must verify resolution criteria manually. A wrong match = guaranteed loss.

---

## 8. Arbitrage Engine

### 8.1 Detection Algorithm

For each verified market pair, on every price update:

```rust
fn detect_opportunity(pair: &MarketPair, poly: &OrderBook, kalshi: &OrderBook) -> Option<Opportunity> {
    // Strategy 1: Buy YES on Poly, buy NO on Kalshi
    let poly_yes_ask = poly.best_ask(Side::Yes)?;     // cheapest YES on Poly
    let kalshi_no_ask = kalshi.best_ask(Side::No)?;    // cheapest NO on Kalshi
    let spread_1 = Decimal::ONE - poly_yes_ask - kalshi_no_ask;

    // Strategy 2: Buy NO on Poly, buy YES on Kalshi
    let poly_no_ask = poly.best_ask(Side::No)?;
    let kalshi_yes_ask = kalshi.best_ask(Side::Yes)?;
    let spread_2 = Decimal::ONE - poly_no_ask - kalshi_yes_ask;

    // Pick the better spread
    let (poly_side, kalshi_side, spread) = if spread_1 > spread_2 {
        (Side::Yes, Side::No, spread_1)
    } else {
        (Side::No, Side::Yes, spread_2)
    };

    if spread > min_spread_threshold {
        Some(Opportunity { spread, poly_side, kalshi_side, ... })
    } else {
        None
    }
}
```

### 8.2 Main Loop

```
loop:
    price_update = rx.recv()
    pair = find_pair(price_update.market_id)
    
    if !engine.is_running():
        continue
    
    update_order_book(pair, price_update)
    
    if let Some(opp) = detect_opportunity(pair):
        if risk_manager.check(&opp).is_ok():
            executor.execute(opp)
```

### 8.3 Spread Calculation

```
spread = 1.00 - buy_price_platform_A - buy_price_platform_B

Example:
  Buy YES on Polymarket at $0.42
  Buy NO on Kalshi at $0.53
  spread = 1.00 - 0.42 - 0.53 = $0.05 per share (5%)

If outcome is YES:  +$1.00 - $0.42 - $0.53 = +$0.05
If outcome is NO:   -$0.42 + $1.00 - $0.53 = +$0.05
```

The spread must account for:
- Maker fees (0% on both — no adjustment needed)
- Potential slippage if our limit order fills at a worse price (mitigated by limit orders)
- The cost of capital being locked until settlement

---

## 9. Order Management

### 9.1 Limit Order Lifecycle

This is the critical section. Unlike taker orders (fire-and-forget), limit orders require active management.

```
                ┌──────────┐
                │ DETECTED │  opportunity found
                └────┬─────┘
                     │
              ┌──────▼──────┐
              │  POST BOTH  │  place limit orders on both platforms
              │   ORDERS    │  simultaneously
              └──────┬──────┘
                     │
         ┌───────────┼───────────┐
         ▼           ▼           ▼
    ┌─────────┐ ┌─────────┐ ┌─────────┐
    │ BOTH    │ │ ONE     │ │ NEITHER │
    │ FILLED  │ │ FILLED  │ │ FILLED  │
    └────┬────┘ └────┬────┘ └────┬────┘
         │           │           │
         ▼           ▼           ▼
    ┌─────────┐ ┌─────────┐ ┌─────────┐
    │ HEDGED  │ │ UNWIND  │ │ CANCEL  │
    │ PROFIT  │ │ EXPOSED │ │ BOTH    │
    └─────────┘ └─────────┘ └─────────┘
```

### 9.2 Order Placement Strategy

1. **Post both orders simultaneously** — `tokio::join!()` on both platforms
2. **Price**: Post at the current best ask (or slightly better to be first in queue)
3. **Quantity**: Min of available depth on both sides, capped by risk limits
4. **Time-in-force**: GTC (Good-Till-Cancelled) with our own TTL management

### 9.3 Order Monitoring Loop

Every 500ms for each active order pair:

```rust
async fn monitor_orders(poly_order: &Order, kalshi_order: &Order) -> Action {
    let poly_status = poly_client.get_order(&poly_order.id).await;
    let kalshi_status = kalshi_client.get_order(&kalshi_order.id).await;

    match (poly_status.filled(), kalshi_status.filled()) {
        // Both filled — perfect hedge, record profit
        (true, true) => Action::RecordHedgedPosition,

        // One filled, other still open — wait up to MAX_HEDGE_WAIT
        (true, false) | (false, true) => {
            if order_age > MAX_HEDGE_WAIT {
                Action::UnwindExposedLeg
            } else {
                Action::Wait
            }
        }

        // Neither filled — check if opportunity still exists
        (false, false) => {
            if spread_still_valid() {
                if order_age > MAX_ORDER_AGE {
                    Action::CancelAndRepost  // price may have moved
                } else {
                    Action::Wait
                }
            } else {
                Action::CancelBoth
            }
        }
    }
}
```

### 9.4 Timing Parameters

| Parameter | Default | Description |
|---|---|---|
| `max_order_age_secs` | 30 | Cancel unfilled orders after this |
| `max_hedge_wait_secs` | 60 | Max time to wait for second leg after first fills |
| `order_check_interval_ms` | 500 | How often to poll order status |
| `min_repost_spread` | 0.02 | Don't repost if spread dropped below this |
| `price_improve_amount` | 0.01 | How much to improve price when reposting |

### 9.5 Unwind Strategy

When only one leg fills and the second doesn't fill within `max_hedge_wait_secs`:

1. Cancel the unfilled order
2. Attempt to sell the filled position at market (taker order) to exit
3. Accept the ~2% taker fee as the cost of the failed hedge
4. Log the loss and update risk counters

This is the primary risk of the system. Mitigation:
- Only trade markets with sufficient liquidity on both platforms
- Keep `max_hedge_wait_secs` short
- Track unwind rate — if > 20%, pause and investigate

---

## 10. Risk Management

### 10.1 Pre-Trade Checks

```rust
fn pre_trade_check(opp: &Opportunity) -> Result<(), RiskError> {
    // 1. System is running
    check_engine_running()?;
    
    // 2. Market pair is verified
    check_pair_verified(opp.pair_id)?;
    
    // 3. Spread exceeds minimum threshold
    check_min_spread(opp.spread)?;
    
    // 4. Market closes in > 24 hours (no last-minute trades)
    check_time_to_close(opp.close_time)?;
    
    // 5. Sufficient balance on both platforms
    check_balances(opp.quantity, opp.poly_price, opp.kalshi_price)?;
    
    // 6. Position size within per-market limit
    check_position_limit(opp.pair_id, opp.quantity)?;
    
    // 7. Total exposure within global limit
    check_total_exposure(opp.quantity)?;
    
    // 8. Max unhedged exposure not exceeded
    check_unhedged_limit()?;
    
    // 9. Daily loss limit not exceeded
    check_daily_loss()?;
    
    // 10. Order book has sufficient depth
    check_liquidity(opp)?;
    
    Ok(())
}
```

### 10.2 Risk Limits

| Limit | Default | Description |
|---|---|---|
| `min_spread_pct` | 3.0% | Minimum spread to trigger a trade |
| `max_position_per_market` | $1,000 | Max capital in one market pair |
| `max_total_exposure` | $10,000 | Max capital across all positions |
| `max_unhedged_exposure` | $500 | Max capital in one-legged positions |
| `max_daily_loss` | $200 | Pause trading if daily losses exceed this |
| `min_time_to_close_hours` | 24 | Don't trade markets closing within 24h |
| `min_book_depth` | 50 | Min contracts available at best price |
| `max_unwind_rate_pct` | 20% | Pause if > 20% of trades need unwinding |

### 10.3 Capital Lock-Up Awareness

Prediction market positions lock capital until settlement (which can be weeks/months away). The system must:
- Track total locked capital vs available capital
- Prefer markets with nearer resolution dates (faster capital turnover)
- Alert when locked capital exceeds a configurable threshold
- Calculate annualized return accounting for lock-up duration

```
Annualized return = (spread / cost) * (365 / days_to_settlement) * 100

Example:
  Spread: $0.05, Cost: $0.95, Settlement: 30 days
  Annualized: (0.05 / 0.95) * (365 / 30) * 100 = 64% APY
```

---

## 11. Configuration

### 11.1 default.toml

```toml
[engine]
enabled = true
scan_interval_ms = 1000
min_spread_pct = "3.0"
min_spread_absolute = "0.02"

[orders]
max_order_age_secs = 30
max_hedge_wait_secs = 60
order_check_interval_ms = 500
min_repost_spread = "0.02"
price_improve_amount = "0.01"
default_quantity = 50

[risk]
max_position_per_market = "1000.00"
max_total_exposure = "10000.00"
max_unhedged_exposure = "500.00"
max_daily_loss = "200.00"
min_time_to_close_hours = 24
min_book_depth = 50
max_unwind_rate_pct = "20.0"

[polymarket]
clob_url = "https://clob.polymarket.com"
gamma_url = "https://gamma-api.polymarket.com"
ws_url = "wss://ws-subscriptions-clob.polymarket.com/ws"

[kalshi]
api_url = "https://trading-api.kalshi.com/trade-api/v2"
ws_url = "wss://trading-api.kalshi.com/trade-api/ws/v2"

[database]
path = "data/arb.db"

[logging]
level = "info"
format = "pretty"
```

### 11.2 Environment Variables

```bash
# .env.example

# Polymarket (Polygon wallet)
POLY_PRIVATE_KEY=0x...          # Polygon wallet private key (for EIP-712 order signing)
POLY_API_KEY=...                # CLOB API key
POLY_API_SECRET=...             # CLOB API secret
POLY_PASSPHRASE=...             # CLOB API passphrase

# Kalshi (RSA key pair)
KALSHI_API_KEY_ID=...           # API key ID from dashboard
KALSHI_PRIVATE_KEY_PATH=./kalshi_private.pem   # RSA private key file

# General
RUST_LOG=info,arb_engine=debug,arb_polymarket=debug,arb_kalshi=debug
```

---

## 12. Testing Strategy

### 12.1 Unit Tests

- `arb-types`: All type conversions, price normalization, spread calculation
- `arb-matcher`: Fuzzy matching accuracy with known market pairs
- `arb-engine`: Detector tested with synthetic price data
- `arb-risk`: All pre-trade checks individually and in combination

### 12.2 Integration Tests

- **Mock connectors**: Both platform connectors have mock implementations for testing
- **Replay mode**: Feed recorded price data through the pipeline
- **Paper trading**: Connect to real APIs but skip order placement; log what would have been traded

### 12.3 Paper Trading Mode

Before deploying with real capital:

```toml
[engine]
paper_trading = true   # connects to real APIs, detects real opportunities, simulates orders
```

Paper trading mode:
- Pulls real prices from both platforms
- Runs the full detection pipeline
- Simulates order placement (assumes fill at limit price after random delay)
- Tracks simulated P&L
- Logs everything to the database

This validates the matching, detection, and risk logic without risking capital.

### 12.4 Test Commands

```bash
cargo test --workspace                     # All tests
cargo test -p arb-engine                   # Single crate
cargo run -- --paper                       # Paper trading mode
cargo run -- --match                       # Run matcher only, show proposed pairs
```

---

## 13. Deployment

### 13.1 Build & Run

```bash
# Build
cargo build --release

# Run
./target/release/arb                       # Normal mode
./target/release/arb --paper               # Paper trading
./target/release/arb --match               # Match markets only
./target/release/arb --tui                 # With terminal UI (default)
./target/release/arb --headless            # No TUI, log-only
```

### 13.2 System Requirements

- Any Linux/macOS machine with internet access
- ~50MB RAM
- ~100MB disk for SQLite
- Stable internet (both APIs are tolerant of moderate latency)
- No co-location needed — opportunities last minutes

### 13.3 Observability

- **Logging**: `tracing` crate, JSON in production, pretty in dev
- **TUI**: Real-time dashboard showing opportunities, orders, positions, P&L
- **SQLite**: Full audit trail queryable with any SQLite tool
- **Alerts**: Log warnings for unhedged positions, unwind events, risk limit hits

---

## 14. Implementation Roadmap

### Phase 1: Foundation (Week 1–2)

| Task | Crate | Effort |
|------|-------|--------|
| Workspace setup | root | 2h |
| All shared types + price normalization | arb-types | 6h |
| SQLite schema + migrations | arb-db | 4h |
| Repository trait + SQLite impl | arb-db | 6h |
| Risk config + manager skeleton | arb-risk | 4h |
| Config loading + CLI skeleton | arb-cli | 3h |

### Phase 2: Platform Connectors (Week 3–4)

| Task | Crate | Effort |
|------|-------|--------|
| Polymarket REST client + auth | arb-polymarket | 10h |
| Polymarket EIP-712 order signing | arb-polymarket | 6h |
| Polymarket WebSocket feed | arb-polymarket | 4h |
| Kalshi REST client + RSA auth | arb-kalshi | 8h |
| Kalshi WebSocket feed | arb-kalshi | 4h |
| Mock connectors for testing | both | 4h |

### Phase 3: Matching + Detection (Week 5–6)

| Task | Crate | Effort |
|------|-------|--------|
| Fuzzy market matcher | arb-matcher | 6h |
| Manual pair verification flow (CLI) | arb-matcher | 4h |
| Pair config file loading | arb-matcher | 2h |
| Arbitrage detector | arb-engine | 6h |
| Spread calculator | arb-engine | 2h |
| Price snapshot recording | arb-db | 2h |

### Phase 4: Order Management + Execution (Week 7–8)

| Task | Crate | Effort |
|------|-------|--------|
| Limit order placement (both legs) | arb-engine | 8h |
| Order monitoring loop | arb-engine | 6h |
| Cancel + repost logic | arb-engine | 4h |
| Unwind strategy | arb-engine | 6h |
| Position tracker | arb-engine | 4h |
| Full risk pre-trade checks | arb-risk | 6h |

### Phase 5: TUI + Paper Trading (Week 9–10)

| Task | Crate | Effort |
|------|-------|--------|
| Paper trading mode | arb-engine | 6h |
| TUI dashboard (ratatui) | arb-cli | 10h |
| P&L tracking + daily summaries | arb-db | 4h |
| Unit tests (all crates) | all | 12h |
| Integration tests with mocks | all | 6h |
| 1 week paper trading validation | — | — |

**Total estimated effort: ~160 hours (10 weeks)**

---

### Revenue Projection (Conservative)

| Parameter | Value |
|---|---|
| Starting capital | $10,000 ($5k per platform) |
| Average spread captured | 4% |
| Average position size | $200 |
| Profit per trade | $8 |
| Trades per day | 5–10 |
| Daily profit | $40–$80 |
| Monthly profit | $1,200–$2,400 |
| Monthly return | 12%–24% |
| Unwind loss rate | ~10% of trades, ~$2 avg loss |
| Net monthly after unwinds | $1,000–$2,000 |

Capital turnover is the main constraint — positions lock capital until settlement. With 30-day average settlement, $10k capital supports ~$10k in positions, turning over ~once per month. Shorter-term markets (1–7 day resolution) dramatically improve turnover.

---

*End of specification. This document is the single source of truth for implementation.*
