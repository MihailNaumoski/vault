# Architecture Plan — Rust Cross-Platform Prediction Market Arbitrage System

**Based on**: `SPEC.md` v2.0.0 (2026-04-05)
**Produced by**: Architect (Planning Team)
**Date**: 2026-04-05

---

## 1. Architecture Analysis

### 1.1 Workspace Decomposition Assessment

The spec defines 8 crates in a Cargo workspace. The decomposition is **well-structured** with clean boundaries:

| Crate | Responsibility | Assessment |
|-------|---------------|------------|
| `arb-types` | Shared domain types, enums, price normalization | **Good** — zero internal dependencies, true leaf crate. All other crates depend on it. |
| `arb-db` | SQLite persistence via sqlx | **Good** — depends only on `arb-types`. Clean repository pattern. |
| `arb-polymarket` | Polymarket REST + WS connector | **Good** — isolated platform concern. Depends only on `arb-types`. |
| `arb-kalshi` | Kalshi REST + WS connector | **Good** — isolated platform concern. Depends only on `arb-types`. |
| `arb-matcher` | Fuzzy market matching | **Good** — depends only on `arb-types`. |
| `arb-risk` | Risk limits and pre-trade checks | **Good** — depends only on `arb-types`. |
| `arb-engine` | Detection, execution, position tracking | **Concern** — depends on 5 other crates. This is the "fat middle" of the system. See below. |
| `arb-cli` | Binary entry point, config, TUI | **Good** — top of the tree, orchestrates everything. |

**Granularity verdict**: The granularity is appropriate. Eight crates for this system size provides fast incremental compilation and clean test isolation without over-fragmentation.

**Concern with `arb-engine`**: This crate has the highest fan-in (depends on `arb-polymarket`, `arb-kalshi`, `arb-matcher`, `arb-risk`, `arb-db`, and `arb-types`). It handles detection, execution, order monitoring, AND position tracking. Consider whether `arb-engine` should be split into:
- `arb-engine` (detection + orchestration only)
- `arb-executor` (order placement, monitoring, unwinding)
- `arb-tracker` (position tracking, P&L)

**Recommendation**: Keep as-is for MVP. The spec's file-level decomposition within `arb-engine` (`detector.rs`, `executor.rs`, `tracker.rs`) achieves the same separation at the module level. Split into separate crates only if compilation times become an issue or if teams need to work on these independently.

### 1.2 Dependency Graph Validation

The spec's dependency graph (Section 2.3):

```
arb-cli
  ├── arb-engine
  │     ├── arb-polymarket ──► arb-types
  │     ├── arb-kalshi     ──► arb-types
  │     ├── arb-matcher    ──► arb-types
  │     ├── arb-risk       ──► arb-types
  │     ├── arb-db         ──► arb-types
  │     └── arb-types
  ├── arb-db       ──► arb-types
  ├── arb-risk     ──► arb-types
  └── arb-types
```

**Validation**: No circular dependencies. The DAG is correct. The `arb-cli` crate directly depends on `arb-db`, `arb-risk`, and `arb-types` in addition to `arb-engine` — this is appropriate because the CLI needs to:
- Load config and create the risk manager before passing it to the engine
- Initialize the DB connection and run migrations before the engine starts
- Access types for config parsing

**Note**: `arb-matcher` depends only on `arb-types`, but the engine needs to call it. This is correct — the matcher provides a pure function (text similarity scoring) plus a store for verified pairs, with no need for platform connectors.

### 1.3 Data Flow Analysis

The primary data flow is:

```
[Polymarket WS] ──► PriceUpdate ──┐
                                   ├──► Engine.detector ──► Opportunity
[Kalshi WS]     ──► PriceUpdate ──┘         │
                                            ▼
                                    RiskManager.pre_trade_check()
                                            │
                                            ▼
                              Engine.executor ──► tokio::join!(
                                                   poly_client.place_limit_order(),
                                                   kalshi_client.place_limit_order()
                                                 )
                                            │
                                            ▼
                              Engine.monitor_orders() ──► Action
                                            │
                                  ┌─────────┼─────────┐
                                  ▼         ▼         ▼
                             RecordPos   Unwind    CancelBoth
                                  │         │         │
                                  ▼         ▼         ▼
                              DB.persist() / Engine.tracker.update()
```

**Channel architecture**: The spec uses `mpsc::Sender<PriceUpdate>` for the WebSocket-to-engine feed. This is correct for multiple producers (two connectors) and a single consumer (the engine's main loop).

**Concern**: The spec's main loop (Section 8.2) processes price updates sequentially. If the engine is busy executing an opportunity (placing orders, monitoring), it will block processing of new price updates. The engine needs internal concurrency:
- Price updates should be processed in the main loop (fast path: detect opportunity)
- Order execution should be spawned as a separate `tokio::task`
- Order monitoring should run as a background loop, not inline

The spec hints at this with the monitoring loop (Section 9.3, "Every 500ms for each active order pair") but does not explicitly show how these concurrent tasks are structured.

### 1.4 PredictionMarketConnector Trait Assessment

The trait (Section 6.1) is well-designed as a common interface:

```rust
#[async_trait]
pub trait PredictionMarketConnector: Send + Sync + 'static {
    fn platform(&self) -> Platform;
    async fn list_markets(&self, status: MarketStatus) -> Result<Vec<Market>>;
    async fn get_market(&self, id: &str) -> Result<Market>;
    async fn get_order_book(&self, id: &str) -> Result<OrderBook>;
    async fn subscribe_prices(&self, ids: &[String], tx: mpsc::Sender<PriceUpdate>) -> Result<SubHandle>;
    async fn place_limit_order(&self, req: &LimitOrderRequest) -> Result<OrderResponse>;
    async fn cancel_order(&self, order_id: &str) -> Result<()>;
    async fn get_order(&self, order_id: &str) -> Result<OrderResponse>;
    async fn list_open_orders(&self) -> Result<Vec<OrderResponse>>;
    async fn get_balance(&self) -> Result<Decimal>;
    async fn get_positions(&self) -> Result<Vec<PlatformPosition>>;
}
```

**Strengths**:
- Clean abstraction over both platforms despite different auth models
- Auth complexity is hidden inside each connector implementation
- `subscribe_prices` takes a channel sender — good for decoupled price feeds

**Gaps**:
1. **Missing `get_trades` / `get_fills`** — needed to reconcile actual fill prices vs expected prices
2. **No rate-limiting awareness in the trait** — Kalshi's 10 req/s trading limit needs explicit handling. Options: (a) rate limiter inside each connector, (b) expose rate limit info in the trait. Recommend (a) — internal rate limiter using `tokio::time::Interval` or a semaphore.
3. **Error types** — `Result<T>` is unqualified. Should be `Result<T, ConnectorError>` with a typed error enum per connector, or `anyhow::Result` for simplicity. The spec uses both `thiserror` and `anyhow` in dependencies but doesn't show which is used where.
4. **`SubHandle` not defined** — this is the WebSocket subscription handle for unsubscribing / detecting disconnection. Needs definition.
5. **Reconnection** — `subscribe_prices` returns once, but WebSockets disconnect. The trait should either: (a) handle reconnection internally and re-emit from where it left off, or (b) expose connection state so the caller can resubscribe.

### 1.5 Concurrency Model Assessment

**Tokio async runtime**: Correct choice. The system is I/O-bound (HTTP requests, WebSocket streams, SQLite queries). No CPU-heavy computation.

**`tokio::join!` for dual-leg orders**: Correct — both legs must be posted as simultaneously as possible to minimize price movement risk.

**Missing concurrency design**:
- How many concurrent order pairs can the engine manage? The monitoring loop polls every 500ms per pair — with 10 active pairs, that is 20 API calls/second just for monitoring, which hits Kalshi's 10 req/s limit.
- The spec needs a batching strategy: poll all Kalshi orders in a single `list_open_orders()` call rather than per-order `get_order()` calls.

### 1.6 SQLite + sqlx Assessment

**Correct for single-operator**: SQLite with sqlx is the right choice. No need for PostgreSQL for a single-user system.

**Strengths**:
- Zero ops overhead (no database server)
- File-based backup (just copy `arb.db`)
- sqlx compile-time query checking (if using `sqlx::query!` macros)

**Concerns**:
- **WAL mode not specified** — SQLite defaults to rollback journal. For concurrent reads (TUI) and writes (engine), WAL mode (`PRAGMA journal_mode=WAL`) should be set at connection time.
- **Connection pooling** — sqlx provides pooling, but with SQLite it is limited by the single-writer lock. The spec should specify: one write connection, multiple read connections.
- **Migration strategy** — only `001_initial_schema.sql` is shown. How will schema changes be handled as the system evolves? sqlx has built-in migration support via `sqlx::migrate!()`.

---

## 2. Phased Build Plan

### Phase 1: Foundation (Estimated: 2-3 sessions)

**Goal**: Workspace compiles, core types exist, DB is functional, config loads, binary runs and prints "Hello, arb".

**Acceptance criteria**: `cargo build --workspace` succeeds, `cargo test --workspace` passes, `cargo run` prints a startup message with loaded config.

#### Step 1.1 — Workspace Setup

Create the workspace root:

```
projects/arbitrage-trader/prediction-arb/
├── Cargo.toml          (workspace definition from spec Section 2.2)
├── Cargo.lock           (generated)
├── .env.example         (from spec Section 11.2)
├── config/
│   ├── default.toml     (from spec Section 11.1)
│   └── pairs.toml       (empty initial, with example comment)
├── migrations/
│   └── 001_initial_schema.sql  (from spec Section 5.1)
└── crates/
    ├── arb-types/Cargo.toml
    ├── arb-db/Cargo.toml
    ├── arb-risk/Cargo.toml
    └── arb-cli/Cargo.toml
```

**Workspace Cargo.toml**: Copy from spec Section 2.2. For Phase 1, only include `arb-types`, `arb-db`, `arb-risk`, and `arb-cli` in the `members` list. Comment out the rest. This prevents compilation errors from missing crates.

```toml
[workspace]
resolver = "2"
members = [
    "crates/arb-types",
    "crates/arb-db",
    "crates/arb-risk",
    "crates/arb-cli",
    # Phase 2:
    # "crates/arb-polymarket",
    # "crates/arb-kalshi",
    # Phase 3:
    # "crates/arb-matcher",
    # Phase 4:
    # "crates/arb-engine",
]
```

Only include workspace dependencies needed by Phase 1 crates. Others can remain but will produce warnings, not errors.

#### Step 1.2 — arb-types Crate

**Dependencies**: `serde`, `serde_json`, `rust_decimal`, `rust_decimal_macros`, `chrono`, `thiserror`, `uuid`

**Files to create**:

- `crates/arb-types/Cargo.toml`
- `crates/arb-types/src/lib.rs` — re-exports all modules
- `crates/arb-types/src/platform.rs` — `Platform` enum
- `crates/arb-types/src/market.rs` — `Market`, `MarketStatus`, `MarketRef`, `MarketPair`, `MarketId` (newtype over `Uuid`)
- `crates/arb-types/src/order.rs` — `Order`, `OrderStatus`, `OrderType`, `Side`, `LimitOrderRequest`, `OrderResponse`
- `crates/arb-types/src/position.rs` — `Position`, `PositionStatus`, `PlatformPosition`
- `crates/arb-types/src/opportunity.rs` — `Opportunity`, `OpportunityStatus`
- `crates/arb-types/src/price.rs` — `PriceUpdate`, `OrderBook`, `OrderBookEntry`, price normalization functions (`cents_to_decimal`, `validate_price_range`)
- `crates/arb-types/src/event.rs` — `EngineEvent` enum for internal event bus (price updates, order fills, risk alerts)
- `crates/arb-types/src/error.rs` — `ArbError` enum using `thiserror`

**Key implementation details**:

1. All `Decimal` fields use `rust_decimal::Decimal` with `serde(with = "rust_decimal::serde::str")` for JSON serialization as strings
2. `Side` enum: `YES` / `NO` (not Buy/Sell — spec Section 4.2 invariant)
3. `Platform` enum: `Polymarket` / `Kalshi` with `serde(rename_all = "lowercase")`
4. `MarketId` should be a newtype: `pub struct MarketId(pub Uuid);`
5. Price validation: all prices in `[0.00, 1.00]` range, enforced at construction
6. UUIDs are v7 (time-ordered) — use `uuid::Uuid::now_v7()`
7. `OrderBook` type (not defined in spec, needed by trait): `pub struct OrderBook { pub bids: Vec<OrderBookEntry>, pub asks: Vec<OrderBookEntry> }` with `best_ask(side: Side) -> Option<Decimal>` method
8. `PriceUpdate` type: `pub struct PriceUpdate { pub platform: Platform, pub market_id: String, pub yes_price: Decimal, pub no_price: Decimal, pub timestamp: DateTime<Utc> }`
9. `SubHandle` type: A handle for cancelling a WebSocket subscription. `pub struct SubHandle { cancel_tx: oneshot::Sender<()> }` or wrap a `tokio::task::JoinHandle`

**Error hierarchy**:

```rust
#[derive(Debug, thiserror::Error)]
pub enum ArbError {
    #[error("connector error: {platform}: {message}")]
    Connector { platform: Platform, message: String, source: Option<Box<dyn std::error::Error + Send + Sync>> },

    #[error("risk check failed: {0}")]
    Risk(#[from] RiskError),

    #[error("database error: {0}")]
    Database(String),

    #[error("config error: {0}")]
    Config(String),

    #[error("price out of range: {0}")]
    PriceOutOfRange(Decimal),

    #[error("order error: {0}")]
    Order(String),
}

#[derive(Debug, thiserror::Error)]
pub enum RiskError {
    #[error("spread {spread} below minimum {min}")]
    SpreadTooLow { spread: Decimal, min: Decimal },

    #[error("position limit exceeded for pair {pair_id}")]
    PositionLimitExceeded { pair_id: Uuid },

    #[error("total exposure {current} would exceed limit {limit}")]
    ExposureLimitExceeded { current: Decimal, limit: Decimal },

    #[error("daily loss limit exceeded")]
    DailyLossLimitExceeded,

    #[error("insufficient balance on {platform}: have {available}, need {required}")]
    InsufficientBalance { platform: Platform, available: Decimal, required: Decimal },

    #[error("market closes in {hours}h, minimum is {min_hours}h")]
    TooCloseToExpiry { hours: i64, min_hours: i64 },

    #[error("engine is paused")]
    EnginePaused,

    #[error("pair {pair_id} not verified")]
    PairNotVerified { pair_id: Uuid },

    #[error("insufficient book depth: {available} < {required}")]
    InsufficientDepth { available: u32, required: u32 },

    #[error("unwind rate {rate}% exceeds limit {limit}%")]
    UnwindRateExceeded { rate: Decimal, limit: Decimal },
}
```

**Tests for arb-types**:
- Price normalization: `cents_to_decimal(42)` == `Decimal::new(42, 2)`
- Price validation: reject < 0 and > 1
- Spread calculation: `1.00 - 0.42 - 0.53 == 0.05`
- `OrderBook::best_ask()` returns correct value
- Serialization round-trips for all types

#### Step 1.3 — arb-db Crate

**Dependencies**: `arb-types`, `sqlx` (with SQLite + chrono + uuid + json features), `tokio`, `tracing`, `thiserror`, `anyhow`

**Files to create**:

- `crates/arb-db/Cargo.toml`
- `crates/arb-db/src/lib.rs` — `Database` struct, `init()`, migration runner
- `crates/arb-db/src/repo.rs` — `Repository` trait + `SqliteRepository` implementation
- `crates/arb-db/src/models.rs` — DB row types (may differ slightly from domain types for serialization)

**Key implementation details**:

1. `Database` struct wraps `sqlx::SqlitePool`
2. Initialization: `Database::new(path: &str) -> Result<Self>` — creates file if missing, sets `PRAGMA journal_mode=WAL`, `PRAGMA foreign_keys=ON`, `PRAGMA busy_timeout=5000`, runs migrations
3. Use `sqlx::migrate!("../../migrations")` macro for compile-time migration embedding
4. `Repository` trait with methods:

```rust
#[async_trait]
pub trait Repository: Send + Sync {
    // Market pairs
    async fn upsert_pair(&self, pair: &MarketPair) -> Result<()>;
    async fn get_pair(&self, id: Uuid) -> Result<Option<MarketPair>>;
    async fn list_active_pairs(&self) -> Result<Vec<MarketPair>>;

    // Opportunities
    async fn insert_opportunity(&self, opp: &Opportunity) -> Result<()>;
    async fn update_opportunity_status(&self, id: Uuid, status: OpportunityStatus) -> Result<()>;
    async fn list_recent_opportunities(&self, limit: u32) -> Result<Vec<Opportunity>>;

    // Orders
    async fn insert_order(&self, order: &Order) -> Result<()>;
    async fn update_order(&self, order: &Order) -> Result<()>;
    async fn list_open_orders(&self) -> Result<Vec<Order>>;

    // Positions
    async fn insert_position(&self, pos: &Position) -> Result<()>;
    async fn update_position(&self, pos: &Position) -> Result<()>;
    async fn list_open_positions(&self) -> Result<Vec<Position>>;

    // Price snapshots
    async fn insert_snapshot(&self, pair_id: Uuid, poly_yes: Decimal, kalshi_yes: Decimal, spread: Decimal) -> Result<()>;

    // P&L
    async fn get_daily_pnl(&self, date: &str) -> Result<Option<DailyPnl>>;
    async fn upsert_daily_pnl(&self, pnl: &DailyPnl) -> Result<()>;
}
```

5. Schema: Use the SQL from spec Section 5.1 verbatim in `migrations/001_initial_schema.sql`

**Tests for arb-db**:
- Create in-memory DB (`sqlite::memory:`), run migrations
- CRUD for each table
- Foreign key constraints work (inserting order with invalid opportunity_id fails)
- WAL mode is active after init

#### Step 1.4 — arb-risk Skeleton

**Dependencies**: `arb-types`, `rust_decimal`, `chrono`, `serde`, `tracing`, `thiserror`

**Files to create**:

- `crates/arb-risk/Cargo.toml`
- `crates/arb-risk/src/lib.rs` — re-exports
- `crates/arb-risk/src/limits.rs` — `RiskLimits` config struct (deserialized from TOML)
- `crates/arb-risk/src/manager.rs` — `RiskManager` struct with stub `pre_trade_check()`
- `crates/arb-risk/src/exposure.rs` — `ExposureTracker` struct (tracks current capital at risk)

**Key implementation details**:

1. `RiskLimits` struct matches spec Section 10.2 config:

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct RiskLimits {
    pub min_spread_pct: Decimal,
    pub max_position_per_market: Decimal,
    pub max_total_exposure: Decimal,
    pub max_unhedged_exposure: Decimal,
    pub max_daily_loss: Decimal,
    pub min_time_to_close_hours: i64,
    pub min_book_depth: u32,
    pub max_unwind_rate_pct: Decimal,
}
```

2. `RiskManager::pre_trade_check(&self, opp: &Opportunity) -> Result<(), RiskError>` — implement the 10-point checklist from spec Section 10.1. For Phase 1, implement checks 1-5 (spread, time-to-close, balance, position limit, exposure). Checks 6-10 (book depth, unwind rate, etc.) require real data and can be stubbed.

3. `ExposureTracker` — in-memory tracker using `parking_lot::RwLock<ExposureState>`:

```rust
pub struct ExposureState {
    pub total_exposure: Decimal,
    pub per_pair_exposure: HashMap<Uuid, Decimal>,
    pub unhedged_exposure: Decimal,
    pub daily_loss: Decimal,
    pub unwind_count: u32,
    pub total_trades: u32,
}
```

**Tests for arb-risk**:
- `RiskLimits` deserializes from TOML
- Pre-trade check rejects below-minimum spread
- Pre-trade check rejects when exposure limit would be exceeded
- Exposure tracker updates correctly

#### Step 1.5 — arb-cli Skeleton

**Dependencies**: `arb-types`, `arb-db`, `arb-risk`, `config`, `dotenvy`, `tokio`, `tracing`, `tracing-subscriber`, `anyhow`, `clap` (not in spec — add it for CLI arg parsing, or use manual parsing)

**Note on clap**: The spec does not include `clap` in workspace dependencies but shows CLI flags (`--paper`, `--match`, `--tui`, `--headless`). Either add `clap` or parse args manually. Recommendation: add `clap = { version = "4", features = ["derive"] }` to workspace dependencies.

**Files to create**:

- `crates/arb-cli/Cargo.toml`
- `crates/arb-cli/src/main.rs` — entry point: load env, load config, init logging, init DB, print startup info
- `crates/arb-cli/src/tui.rs` — empty stub for Phase 5

**Key implementation details**:

1. `main.rs` structure:

```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Load .env
    dotenvy::dotenv().ok();

    // 2. Init tracing
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    // 3. Load config from config/default.toml
    let settings = config::Config::builder()
        .add_source(config::File::with_name("config/default"))
        .build()?;

    // 4. Parse CLI args (--paper, --match, --tui, --headless)

    // 5. Init database
    let db = arb_db::Database::new(&settings.get_string("database.path")?).await?;

    // 6. Init risk manager
    let risk_limits: RiskLimits = settings.get("risk")?;
    let risk_manager = RiskManager::new(risk_limits);

    // 7. Print startup info
    tracing::info!("Prediction Market Arbitrage System starting");
    tracing::info!("Database: {}", settings.get_string("database.path")?);
    tracing::info!("Paper trading: {}", paper_mode);

    // 8. (Phase 2+) Init connectors, engine, start main loop

    Ok(())
}
```

**Tests for arb-cli**: Minimal in Phase 1 — config loading with a test TOML file.

#### Phase 1 Completion Criteria

- [ ] `cargo build --workspace` compiles with zero errors
- [ ] `cargo test --workspace` passes all tests
- [ ] `cargo run` loads config, inits DB, prints startup message, and exits cleanly
- [ ] `arb-types` has all domain types from spec Section 4 with serde and basic validation
- [ ] `arb-db` creates SQLite, runs migrations, and passes CRUD tests
- [ ] `arb-risk` loads limits from config and validates basic risk checks
- [ ] `cargo clippy --workspace` has no warnings

---

### Phase 2: Platform Connectors (Depends on Phase 1)

**Goal**: Both Polymarket and Kalshi connectors implement the `PredictionMarketConnector` trait. Mock connectors exist for testing.

**Crates**: `arb-polymarket`, `arb-kalshi`

#### Step 2.1 — Connector Trait in arb-types

Move the `PredictionMarketConnector` trait into `arb-types` so both connector crates and the engine can reference it without circular dependencies. Add `async-trait` and `tokio` (for `mpsc`) as dependencies of `arb-types`.

Alternatively, create a thin `arb-connector` trait crate. Recommendation: keep it in `arb-types` for simplicity in MVP.

#### Step 2.2 — arb-polymarket

1. REST client with HMAC-SHA256 auth (spec Section 6.2)
2. EIP-712 order signing using `alloy-signer-local` (spec Section 6.2)
3. WebSocket price feed
4. Internal rate limiter (100 req/s is generous, but still implement a token bucket)
5. Mock implementation for tests

**Critical complexity**: EIP-712 signing is the hardest part. The Polymarket CLOB requires typed data signatures for orders. This must exactly match Polymarket's contract ABI.

#### Step 2.3 — arb-kalshi

1. REST client with RSA-SHA256 auth (spec Section 6.3)
2. WebSocket price feed
3. Internal rate limiter (**critical**: 10 req/s trading limit)
4. Mock implementation for tests

**Critical complexity**: RSA signing with PEM key loading using the `rsa` crate. Kalshi's rate limits (10 req/s trading) are tight — the monitoring loop must batch requests.

#### Step 2.4 — Mock Connectors

Create `MockConnector` implementing `PredictionMarketConnector` for both platforms. Use `Arc<Mutex<MockState>>` to allow tests to inject price data and verify order placement.

**Phase 2 Completion Criteria**:
- [ ] Both connectors compile and implement the full trait
- [ ] Auth works against real APIs (manual test with sandbox/testnet if available)
- [ ] WebSocket connects and receives price updates
- [ ] Mock connectors pass unit tests
- [ ] Rate limiters enforce platform-specific limits

---

### Phase 3: Matching + Detection (Depends on Phase 1, partially on Phase 2)

**Goal**: Market matcher proposes pair candidates, config file loads verified pairs, arbitrage detector identifies opportunities from price data.

**Crates**: `arb-matcher`, `arb-engine` (detection subset)

#### Step 3.1 — arb-matcher

1. Fuzzy string matching using Jaro-Winkler (`strsim` crate)
2. Close-time proximity scoring
3. Pair store: load from `pairs.toml`, save new matches to DB
4. CLI command: `arb --match` pulls markets from both platforms, runs matching, presents candidates

#### Step 3.2 — arb-engine (detector only)

1. `Detector` struct: takes price updates, maintains current order books per pair, runs `detect_opportunity()` on each update
2. Spread calculation per spec Section 8.3
3. Output: `Vec<Opportunity>` emitted to a channel

**Phase 3 Completion Criteria**:
- [ ] Fuzzy matcher scores known market pairs > 0.9
- [ ] `pairs.toml` loads and deserializes correctly
- [ ] Detector finds opportunities in synthetic price data
- [ ] Spread calculation matches spec examples

---

### Phase 4: Order Management + Execution (Depends on Phases 2 + 3)

**Goal**: Full order lifecycle — place both legs, monitor fills, handle partial fills, unwind exposed legs.

**Crates**: `arb-engine` (executor + tracker), `arb-risk` (full implementation)

#### Step 4.1 — Executor

1. `Executor::execute(opp: Opportunity)` — places both legs via `tokio::join!`
2. Returns `ExecutionResult` with both order IDs

#### Step 4.2 — Order Monitor

1. Background task polling order status every 500ms
2. State machine: spec Section 9.1 lifecycle
3. Cancel + repost logic: spec Section 9.3

#### Step 4.3 — Position Tracker

1. Create `Position` when both legs fill
2. Track hedged vs unhedged quantities
3. Persist to DB

#### Step 4.4 — Unwind Strategy

1. Cancel unfilled leg
2. Place taker order to exit filled leg
3. Record loss

#### Step 4.5 — Full Risk Manager

Complete all 10 pre-trade checks from spec Section 10.1.

**Phase 4 Completion Criteria**:
- [ ] Full order lifecycle works with mock connectors
- [ ] Unwind strategy executes correctly
- [ ] Risk checks block trades that exceed limits
- [ ] Positions are correctly tracked in DB

---

### Phase 5: TUI + Paper Trading (Depends on Phase 4)

**Goal**: Terminal dashboard, paper trading mode, production readiness.

**Crates**: `arb-cli` (TUI), `arb-engine` (paper trading mode)

#### Step 5.1 — Paper Trading

1. `PaperConnector` wrapping real connectors: pulls real prices, simulates order fills
2. Configurable fill delay and fill probability
3. Logs simulated trades to DB

#### Step 5.2 — TUI Dashboard

1. Using `ratatui` + `crossterm`
2. Layout from spec Section 3.1
3. Panels: active pairs, open orders, positions, recent trades
4. Key bindings: quit, pause, resume, markets, orders

#### Step 5.3 — Integration Testing

1. End-to-end test with mock connectors
2. Replay mode: feed recorded price data
3. Verify P&L tracking accuracy

**Phase 5 Completion Criteria**:
- [ ] Paper trading runs against real price feeds
- [ ] TUI displays all required panels
- [ ] P&L tracking matches manual calculation
- [ ] 1 week of stable paper trading

---

## 3. Spec Gaps and Risks

### Critical (could block implementation)

| # | Gap | Spec Section | Severity | Details |
|---|-----|-------------|----------|---------|
| G1 | **Undefined types** | 4, 6 | **BLOCKER** | `OrderBook`, `PriceUpdate`, `SubHandle`, `LimitOrderRequest`, `OrderResponse`, `PlatformPosition`, `OrderBookEntry` are referenced but never defined. Implementation must define these. Proposed definitions included in Phase 1 plan above. |
| G2 | **Error type hierarchy** | 2.2 | **BLOCKER** | Spec lists `thiserror` and `anyhow` as dependencies but never shows error types. No guidance on when to use `thiserror` (library crates) vs `anyhow` (application crate). Must define `ArbError`, `RiskError`, `ConnectorError`. Proposed hierarchy included above. |
| G3 | **WebSocket reconnection** | 6.2, 6.3 | **HIGH** | No reconnection strategy defined. WebSocket connections will drop. Options: exponential backoff, jitter, max retries. Must also handle re-subscribing to all price feeds after reconnect. Implementation should use a reconnecting wrapper. |
| G4 | **CLI argument parser missing** | 13.1 | **MEDIUM** | Spec shows `--paper`, `--match`, `--tui`, `--headless` flags but `clap` is not in workspace dependencies. Must add `clap` or implement manual arg parsing. |

### High (could cause production issues)

| # | Gap | Spec Section | Severity | Details |
|---|-----|-------------|----------|---------|
| G5 | **Kalshi rate limit vs monitoring loop** | 6.3, 9.3 | **HIGH** | Kalshi allows 10 req/s for trading. Order monitoring polls every 500ms per pair. With 5 active pairs (10 orders), that is 20 polls/second — double the limit. Must batch via `list_open_orders()` instead of per-order polling, or use WebSocket `fill` channel for order status. |
| G6 | **Unwind taker fee economics** | 9.5, 10 | **HIGH** | Unwind requires taker orders at ~2% fee. On a 3% spread trade of $200, the guaranteed profit is $6. An unwind costs ~$4 in taker fees. Net profit from unwind is only $2, but if the market moved against you, loss could exceed $4. The `min_spread_pct` of 3% may be too low given unwind risk. Consider raising to 4-5% or tracking unwind-adjusted expected value. |
| G7 | **Concurrent order execution blocking price processing** | 8.2 | **HIGH** | The main loop is sequential: receive price update, detect, execute. If execution takes 2+ seconds (two API calls), price updates queue up and opportunities go stale. Must spawn execution as separate `tokio::task` and continue processing prices. |
| G8 | **SQLite WAL mode and connection handling** | 5 | **HIGH** | Spec does not mention WAL mode, `PRAGMA` settings, or connection pool config. SQLite without WAL will serialize all reads behind writes, causing TUI lag. Must set `journal_mode=WAL`, `busy_timeout=5000`, `foreign_keys=ON` at connection time. |

### Medium (design gaps, won't block but need decisions)

| # | Gap | Spec Section | Severity | Details |
|---|-----|-------------|----------|---------|
| G9 | **Paper trading fill simulation** | 12.3 | **MEDIUM** | Spec says "assumes fill at limit price after random delay" but this is unrealistically optimistic. Real limit orders often don't fill. Paper trading should simulate fill probability based on book depth and order position in queue. Otherwise paper P&L will be much higher than live P&L, giving false confidence. |
| G10 | **No graceful shutdown** | 13 | **MEDIUM** | What happens when the operator presses Ctrl+C? Open orders should be cancelled. Positions should not be affected (they're already hedged). Need `tokio::signal::ctrl_c()` handler that: (1) cancels all open unfilled orders, (2) saves state, (3) exits. |
| G11 | **Market pair staleness** | 7 | **MEDIUM** | Markets close. Pairs become inactive. The spec mentions `close_time` and `active` flag but no automatic deactivation logic. Need a periodic task that deactivates pairs whose markets have closed. |
| G12 | **Polymarket token ID complexity** | 6.2 | **MEDIUM** | Polymarket uses `condition_id` for the market but `token_id` for each outcome (YES token, NO token). Orders must reference `token_id`, not `condition_id`. The `MarketPair` struct stores `poly_yes_token_id` and `poly_no_token_id` separately. The connector must correctly map `Side::YES` to the right token ID. This mapping is easy to get wrong. |
| G13 | **No health check or heartbeat** | 13 | **MEDIUM** | No way to know if the system is alive and healthy from outside. Consider adding a simple health file (`data/health.json`) updated every 30s with: last price update time, open orders, unhedged exposure. |
| G14 | **Database migration beyond initial schema** | 5 | **LOW** | Only one migration shown. As the system evolves, need a strategy. sqlx supports `sqlx::migrate!()` with numbered SQL files. This is sufficient — just document the convention. |
| G15 | **Decimal string storage in SQLite** | 5.1 | **LOW** | All `Decimal` values are stored as `TEXT` in SQLite. This prevents SQL aggregation queries (`SUM`, `AVG`). Acceptable for MVP, but if analytics queries are needed later, consider storing as `REAL` with known precision loss, or using application-level aggregation. |

### Risk Register

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Polymarket API changes (unversioned CLOB API) | Medium | High | Pin to known working endpoints, add response validation, monitor for breaking changes |
| Kalshi rate limit causes order monitoring failures | High | High | Batch polling, use WebSocket fill channel, implement backoff |
| One-legged fills leading to unhedged exposure | Medium | High | Short TTLs, aggressive cancellation, daily unwind rate monitoring |
| WebSocket disconnection during active trades | Medium | Medium | Reconnection with exponential backoff, fallback to REST polling |
| SQLite corruption on crash | Low | High | WAL mode, `PRAGMA synchronous=NORMAL`, periodic `.backup` |
| EIP-712 signing incompatibility with Polymarket | Medium | High | Test against Polymarket testnet first, validate signature format |
| Capital lockup reduces effective returns | High | Medium | Prefer short-duration markets, track annualized returns, alert on high lockup |

---

## Appendix A: Dependency Matrix for Phase 1

```
arb-cli ──► arb-db ──► arb-types
   │                       ▲
   ├──► arb-risk ──────────┘
   │                       ▲
   └──► arb-types ─────────┘
```

Build order (topological):
1. `arb-types` (no dependencies)
2. `arb-db` (depends on arb-types)
3. `arb-risk` (depends on arb-types)
4. `arb-cli` (depends on arb-types, arb-db, arb-risk)

Steps 2 and 3 can be built in parallel after Step 1.

## Appendix B: Workspace Dependencies for Phase 1

Only these workspace dependencies are needed for Phase 1 compilation:

```
tokio, serde, serde_json, rust_decimal, rust_decimal_macros, chrono,
tracing, tracing-subscriber, thiserror, anyhow, uuid, sqlx,
config, dotenvy, parking_lot, async-trait
```

Platform-specific dependencies (`reqwest`, `tokio-tungstenite`, `hmac`, `sha2`, `rsa`, `alloy-*`, `strsim`, `ratatui`, `crossterm`) are not needed until Phase 2+.

---

*End of architecture plan. This document should be treated as the implementation guide for the coding phase.*
