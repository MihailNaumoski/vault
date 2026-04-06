# Phase 4 — Arbitrage Engine — Build Prompt (v2 — Corrected)

**Goal:** Build the brain — detect opportunities from live prices, execute dual-leg orders, monitor fills, manage positions, handle unwinds, graceful shutdown.

**Project root:** `/Users/mihail/projects/vault/projects/arbitrage-trader`
**Depends on:** Phase 2.5 (97 tests, clippy clean) + Phase 3 (matcher + pair store)

---

## Context: EXACT Types From the Codebase

### ArbError (arb-types/src/error.rs) — USE THESE VARIANTS ONLY

```rust
pub enum ArbError {
    PlatformError { platform: Platform, message: String },
    AuthError { platform: Platform, message: String },
    RateLimited { platform: Platform, retry_after_ms: u64 },
    OrderRejected { platform: Platform, reason: String },
    InvalidPrice(String),
    MarketNotFound(String),
    PairNotVerified(String),
    Database(String),
    Config(String),
    WebSocket(String),
    Serialization(#[from] serde_json::Error),
    Other(String),
}
// Platform implements Display: "polymarket" / "kalshi"
```

**Error mapping cheat sheet:**
| Situation | Use this variant |
|-----------|-----------------|
| Risk check failed | `ArbError::Other(format!("risk: {e}"))` |
| Connector call failed (one leg) | `ArbError::PlatformError { platform, message }` |
| Both legs failed | `ArbError::Other(format!("both legs failed: ..."))` |
| No bids for unwind | `ArbError::Other("no bids for unwind".into())` |
| DB write failed | `ArbError::Database(e.to_string())` |

### PredictionMarketConnector trait (arb-types/src/lib.rs)
```rust
#[async_trait]
pub trait PredictionMarketConnector: Send + Sync + 'static {
    fn platform(&self) -> Platform;
    async fn list_markets(&self, status: MarketStatus) -> Result<Vec<Market>, ArbError>;
    async fn get_market(&self, id: &str) -> Result<Market, ArbError>;
    async fn get_order_book(&self, id: &str) -> Result<OrderBook, ArbError>;
    async fn subscribe_prices(&self, ids: &[String], tx: mpsc::Sender<PriceUpdate>) -> Result<SubHandle, ArbError>;
    async fn place_limit_order(&self, req: &LimitOrderRequest) -> Result<OrderResponse, ArbError>;
    async fn cancel_order(&self, order_id: &str) -> Result<(), ArbError>;
    async fn get_order(&self, order_id: &str) -> Result<OrderResponse, ArbError>;
    async fn list_open_orders(&self) -> Result<Vec<OrderResponse>, ArbError>;
    async fn get_balance(&self) -> Result<Decimal, ArbError>;
    async fn get_positions(&self) -> Result<Vec<PlatformPosition>, ArbError>;
}
```

### OrderResponse / LimitOrderRequest / OrderBook (arb-types/src/order.rs)
```rust
pub struct OrderResponse {
    pub order_id: String, pub status: OrderStatus, pub filled_quantity: u32,
    pub price: Decimal, pub side: Side, pub market_id: String,
}
pub struct LimitOrderRequest {
    pub market_id: String, pub side: Side, pub price: Decimal, pub quantity: u32,
}
// LimitOrderRequest derives: Debug, Clone, Serialize, Deserialize

pub struct OrderBook {
    pub market_id: String,
    pub bids: Vec<OrderBookLevel>,
    pub asks: Vec<OrderBookLevel>,
    pub timestamp: DateTime<Utc>,
}
// OrderBook does NOT derive Default — DateTime<Utc> has no Default
```

### Opportunity (arb-types/src/opportunity.rs) — WILL BE MODIFIED
```rust
pub struct Opportunity {
    pub id: Uuid, pub pair_id: Uuid,
    pub poly_side: Side, pub poly_price: Decimal,
    pub kalshi_side: Side, pub kalshi_price: Decimal,
    pub spread: Decimal, pub spread_pct: Decimal,
    pub max_quantity: u32,
    pub detected_at: DateTime<Utc>,
    pub status: OpportunityStatus, // Detected/Executing/Filled/Expired/Failed
}
```

### RiskManager (arb-risk/src/manager.rs)
```rust
pub fn pre_trade_check(&self, pair_id: Uuid, pair_verified: bool, spread: Decimal,
    min_spread: Decimal, close_time: DateTime<Utc>, quantity: u32,
    poly_price: Decimal, kalshi_price: Decimal,
    poly_balance: Decimal, kalshi_balance: Decimal, book_depth: u32,
) -> Result<(), RiskError>;
// RiskError is NOT convertible to ArbError via #[from] — use .map_err()
```

### Mock Connectors
```rust
// arb-polymarket (feature = "mock"):
//   MockPolymarketConnector::new() / ::with_state(Arc<parking_lot::Mutex<MockState>>)
//   MockState { markets, order_books, orders, positions, balance, placed_orders, cancelled_orders, should_fail: Option<ArbError>, price_updates, next_order_id }
//   Failure injection: state.should_fail = Some(ArbError::PlatformError { platform: Platform::Polymarket, message: "...".into() })

// arb-kalshi (feature = "mock"):
//   MockKalshiConnector::new() / ::with_state(Arc<parking_lot::Mutex<MockState>>)
//   MockState { markets, order_books, orders, positions, balance, placed_orders, cancelled_orders, should_fail: Option<String>, price_updates }
//   Failure injection: state.inject_failure("error message")

// Both mocks: place_limit_order() pushes to state.orders + state.placed_orders
// get_order() finds by order_id in state.orders — mutate status/filled_quantity to simulate fills
```

### DB Row Types (arb-db/src/models.rs)
```rust
pub struct OpportunityRow {
    pub id: String, pub pair_id: String,
    pub poly_side: String, pub poly_price: Decimal,
    pub kalshi_side: String, pub kalshi_price: Decimal,
    pub spread: Decimal, pub spread_pct: Decimal,
    pub max_quantity: i64, pub status: String,
    pub detected_at: DateTime<Utc>,
    pub executed_at: Option<DateTime<Utc>>,
    pub resolved_at: Option<DateTime<Utc>>,
}
pub struct OrderRow {
    pub id: String, pub opportunity_id: String,
    pub platform: String, pub platform_order_id: Option<String>,
    pub market_id: String, pub side: String,
    pub price: Decimal, pub quantity: i64, pub filled_quantity: i64,
    pub status: String, pub placed_at: DateTime<Utc>,
    pub filled_at: Option<DateTime<Utc>>,
    pub cancelled_at: Option<DateTime<Utc>>, pub cancel_reason: Option<String>,
}
pub struct PositionRow {
    pub id: String, pub pair_id: String,
    pub poly_side: String, pub poly_quantity: i64, pub poly_avg_price: Decimal,
    pub kalshi_side: String, pub kalshi_quantity: i64, pub kalshi_avg_price: Decimal,
    pub hedged_quantity: i64, pub unhedged_quantity: i64,
    pub guaranteed_profit: Decimal, pub status: String,
    pub opened_at: DateTime<Utc>, pub settled_at: Option<DateTime<Utc>>,
}
// DB exports: pub use repo::{Repository, SqliteRepository};
```

### Config (config/default.toml)
```toml
[engine]
scan_interval_ms = 1000, min_spread_pct = "3.0", min_spread_absolute = "0.02"

[orders]
max_order_age_secs = 30, max_hedge_wait_secs = 60, order_check_interval_ms = 500
min_repost_spread = "0.02", price_improve_amount = "0.01", default_quantity = 50
```

---

## Prompt 4-A: Modify Opportunity + Side::opposite + PriceCache + Detector

### Step 1: Add Side::opposite()

**File:** `crates/arb-types/src/order.rs` — add impl block after Side enum:

```rust
impl Side {
    pub fn opposite(&self) -> Self {
        match self {
            Side::Yes => Side::No,
            Side::No => Side::Yes,
        }
    }
}
```

### Step 2: Add Default for OrderBook

**File:** `crates/arb-types/src/order.rs` — add after OrderBook struct:

```rust
impl Default for OrderBook {
    fn default() -> Self {
        Self {
            market_id: String::new(),
            bids: Vec::new(),
            asks: Vec::new(),
            timestamp: Utc::now(),
        }
    }
}
```

### Step 3: Add market IDs + close_time to Opportunity

**File:** `crates/arb-types/src/opportunity.rs` — add three fields:

```rust
pub struct Opportunity {
    pub id: Uuid,
    pub pair_id: Uuid,
    pub poly_side: Side,
    pub poly_price: Decimal,
    pub poly_market_id: String,      // NEW: poly token_id (yes or no based on side)
    pub kalshi_side: Side,
    pub kalshi_price: Decimal,
    pub kalshi_market_id: String,    // NEW: kalshi ticker
    pub spread: Decimal,
    pub spread_pct: Decimal,
    pub max_quantity: u32,
    pub close_time: DateTime<Utc>,   // NEW: from pair data, for risk check
    pub detected_at: DateTime<Utc>,
    pub status: OpportunityStatus,
}
```

**Also update any existing code that constructs Opportunity** (grep for `Opportunity {` across the workspace — there should be none yet outside tests).

### Step 4: Create engine Cargo.toml

**File:** `crates/arb-engine/Cargo.toml` — replace entirely:

```toml
[package]
name = "arb-engine"
version = "0.1.0"
edition = "2021"

[features]
default = []
mock = ["arb-polymarket/mock", "arb-kalshi/mock"]

[dependencies]
arb-types = { workspace = true }
arb-db = { workspace = true }
arb-risk = { workspace = true }
arb-matcher = { workspace = true }
arb-polymarket = { workspace = true }
arb-kalshi = { workspace = true }
tokio = { workspace = true }
parking_lot = { workspace = true }
rust_decimal = { workspace = true }
rust_decimal_macros = { workspace = true }
chrono = { workspace = true }
uuid = { workspace = true }
tracing = { workspace = true }
anyhow = { workspace = true }
async-trait = { workspace = true }
serde = { workspace = true }

[dev-dependencies]
tokio = { workspace = true, features = ["test-util", "macros"] }
arb-polymarket = { workspace = true, features = ["mock"] }
arb-kalshi = { workspace = true, features = ["mock"] }
```

### Step 5: Create engine types

**File:** `crates/arb-engine/src/types.rs`

```rust
use arb_types::{OrderResponse, Platform};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
pub struct EngineConfig {
    pub scan_interval_ms: u64,
    pub min_spread_pct: Decimal,
    pub min_spread_absolute: Decimal,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OrderConfig {
    pub max_order_age_secs: u64,
    pub max_hedge_wait_secs: u64,
    pub order_check_interval_ms: u64,
    pub min_repost_spread: Decimal,
    pub price_improve_amount: Decimal,
    pub default_quantity: u32,
}

#[derive(Debug, Clone)]
pub struct ExecutionResult {
    pub opportunity_id: Uuid,
    pub poly_order: OrderResponse,
    pub kalshi_order: OrderResponse,
    pub executed_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub enum MonitorAction {
    BothFilled { poly_order: OrderResponse, kalshi_order: OrderResponse },
    Waiting,
    PartialHedge { filled_platform: Platform, filled_order: OrderResponse, open_order_id: String },
    NeedsUnwind { filled_platform: Platform, filled_order: OrderResponse, unfilled_order_id: String },
    BothCancelled,
}

/// Lightweight pair info passed to the detector and executor.
/// Built from MarketPairRow at engine startup.
#[derive(Debug, Clone)]
pub struct PairInfo {
    pub pair_id: Uuid,
    pub poly_market_id: String,   // condition_id or yes/no token_id
    pub kalshi_market_id: String,  // ticker
    pub close_time: DateTime<Utc>,
    pub verified: bool,
}
```

### Step 6: Create PriceCache

**File:** `crates/arb-engine/src/price_cache.rs`

```rust
use arb_types::{Platform, PriceUpdate};
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::time::Duration;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct PricePair {
    pub poly_yes: Decimal,
    pub poly_no: Decimal,
    pub kalshi_yes: Decimal,
    pub kalshi_no: Decimal,
    pub poly_updated: DateTime<Utc>,
    pub kalshi_updated: DateTime<Utc>,
}

pub struct PriceCache {
    prices: RwLock<HashMap<Uuid, PricePair>>,
    market_to_pair: RwLock<HashMap<(Platform, String), Uuid>>,
}

impl PriceCache {
    pub fn new() -> Self {
        Self {
            prices: RwLock::new(HashMap::new()),
            market_to_pair: RwLock::new(HashMap::new()),
        }
    }

    pub fn register_pair(&self, pair_id: Uuid, poly_market_id: &str, kalshi_market_id: &str) {
        let mut mapping = self.market_to_pair.write();
        mapping.insert((Platform::Polymarket, poly_market_id.to_string()), pair_id);
        mapping.insert((Platform::Kalshi, kalshi_market_id.to_string()), pair_id);
    }

    pub fn update(&self, update: &PriceUpdate) -> Option<Uuid> {
        let pair_id = {
            let mapping = self.market_to_pair.read();
            mapping.get(&(update.platform, update.market_id.clone())).copied()?
        };

        let mut prices = self.prices.write();
        let entry = prices.entry(pair_id).or_insert_with(|| PricePair {
            poly_yes: Decimal::ZERO, poly_no: Decimal::ZERO,
            kalshi_yes: Decimal::ZERO, kalshi_no: Decimal::ZERO,
            poly_updated: update.timestamp, kalshi_updated: update.timestamp,
        });

        match update.platform {
            Platform::Polymarket => {
                entry.poly_yes = update.yes_price;
                entry.poly_no = update.no_price;
                entry.poly_updated = update.timestamp;
            }
            Platform::Kalshi => {
                entry.kalshi_yes = update.yes_price;
                entry.kalshi_no = update.no_price;
                entry.kalshi_updated = update.timestamp;
            }
        }
        Some(pair_id)
    }

    pub fn get(&self, pair_id: &Uuid) -> Option<PricePair> {
        self.prices.read().get(pair_id).cloned()
    }

    pub fn is_fresh(&self, pair_id: &Uuid, max_age: Duration) -> bool {
        let prices = self.prices.read();
        let Some(pp) = prices.get(pair_id) else { return false };
        let now = Utc::now();
        let max_chrono = chrono::Duration::from_std(max_age).unwrap_or(chrono::Duration::seconds(60));
        (now - pp.poly_updated) < max_chrono && (now - pp.kalshi_updated) < max_chrono
    }
}

impl Default for PriceCache {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn make_update(platform: Platform, market_id: &str, yes: Decimal, no: Decimal) -> PriceUpdate {
        PriceUpdate { platform, market_id: market_id.into(), yes_price: yes, no_price: no, timestamp: Utc::now() }
    }

    #[test]
    fn test_register_and_update() {
        let cache = PriceCache::new();
        let pid = Uuid::now_v7();
        cache.register_pair(pid, "poly-1", "kalshi-1");
        let r = cache.update(&make_update(Platform::Polymarket, "poly-1", dec!(0.42), dec!(0.58)));
        assert_eq!(r, Some(pid));
        let pp = cache.get(&pid).unwrap();
        assert_eq!(pp.poly_yes, dec!(0.42));
    }

    #[test]
    fn test_unknown_market() {
        let cache = PriceCache::new();
        assert!(cache.update(&make_update(Platform::Polymarket, "unknown", dec!(0.5), dec!(0.5))).is_none());
    }

    #[test]
    fn test_both_platforms() {
        let cache = PriceCache::new();
        let pid = Uuid::now_v7();
        cache.register_pair(pid, "p1", "k1");
        cache.update(&make_update(Platform::Polymarket, "p1", dec!(0.42), dec!(0.58)));
        cache.update(&make_update(Platform::Kalshi, "k1", dec!(0.47), dec!(0.53)));
        let pp = cache.get(&pid).unwrap();
        assert_eq!(pp.poly_yes, dec!(0.42));
        assert_eq!(pp.kalshi_yes, dec!(0.47));
    }

    #[test]
    fn test_is_fresh() {
        let cache = PriceCache::new();
        let pid = Uuid::now_v7();
        cache.register_pair(pid, "p", "k");
        cache.update(&make_update(Platform::Polymarket, "p", dec!(0.5), dec!(0.5)));
        cache.update(&make_update(Platform::Kalshi, "k", dec!(0.5), dec!(0.5)));
        assert!(cache.is_fresh(&pid, Duration::from_secs(60)));
    }

    #[test]
    fn test_not_fresh_missing() {
        let cache = PriceCache::new();
        assert!(!cache.is_fresh(&Uuid::now_v7(), Duration::from_secs(60)));
    }
}
```

### Step 7: Create Detector (uses PairInfo, not bare UUIDs)

**File:** `crates/arb-engine/src/detector.rs`

```rust
use crate::price_cache::PriceCache;
use crate::types::{EngineConfig, PairInfo};
use arb_types::order::Side;
use arb_types::{Opportunity, OpportunityStatus};
use chrono::Utc;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

pub struct Detector {
    price_cache: Arc<PriceCache>,
    min_spread_pct: Decimal,
    min_spread_absolute: Decimal,
    max_staleness: Duration,
}

impl Detector {
    pub fn new(price_cache: Arc<PriceCache>, config: &EngineConfig) -> Self {
        Self {
            price_cache,
            min_spread_pct: config.min_spread_pct,
            min_spread_absolute: config.min_spread_absolute,
            max_staleness: Duration::from_secs(30),
        }
    }

    pub fn scan(&self, pairs: &[PairInfo]) -> Vec<Opportunity> {
        pairs.iter().filter(|p| p.verified).filter_map(|p| self.check_pair(p)).collect()
    }

    fn check_pair(&self, pair: &PairInfo) -> Option<Opportunity> {
        let prices = self.price_cache.get(&pair.pair_id)?;

        if !self.price_cache.is_fresh(&pair.pair_id, self.max_staleness) {
            return None;
        }

        // SPEC §8.3: spread = 1.00 - buy_A - buy_B
        let spread_a = dec!(1) - prices.poly_yes - prices.kalshi_no;
        let spread_b = dec!(1) - prices.poly_no - prices.kalshi_yes;

        let (spread, poly_side, kalshi_side, poly_price, kalshi_price) = if spread_a >= spread_b {
            (spread_a, Side::Yes, Side::No, prices.poly_yes, prices.kalshi_no)
        } else {
            (spread_b, Side::No, Side::Yes, prices.poly_no, prices.kalshi_yes)
        };

        if spread < self.min_spread_absolute { return None; }

        let combined = poly_price + kalshi_price;
        if combined <= Decimal::ZERO { return None; }
        let spread_pct = (spread / combined) * dec!(100);
        if spread_pct < self.min_spread_pct { return None; }

        Some(Opportunity {
            id: Uuid::now_v7(),
            pair_id: pair.pair_id,
            poly_side,
            poly_price,
            poly_market_id: pair.poly_market_id.clone(),
            kalshi_side,
            kalshi_price,
            kalshi_market_id: pair.kalshi_market_id.clone(),
            spread,
            spread_pct,
            max_quantity: 0,
            close_time: pair.close_time,
            detected_at: Utc::now(),
            status: OpportunityStatus::Detected,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arb_types::{Platform, PriceUpdate};
    use chrono::Duration as CDur;
    use rust_decimal_macros::dec;

    fn cfg(pct: Decimal, abs: Decimal) -> EngineConfig {
        EngineConfig { scan_interval_ms: 1000, min_spread_pct: pct, min_spread_absolute: abs }
    }

    fn pair_info(pair_id: Uuid) -> PairInfo {
        PairInfo {
            pair_id,
            poly_market_id: "poly-tok".into(),
            kalshi_market_id: "KALSHI-T".into(),
            close_time: Utc::now() + CDur::days(30),
            verified: true,
        }
    }

    fn fill_cache(cache: &PriceCache, pair_id: Uuid, py: Decimal, pn: Decimal, ky: Decimal, kn: Decimal) {
        cache.register_pair(pair_id, "poly-tok", "KALSHI-T");
        cache.update(&PriceUpdate { platform: Platform::Polymarket, market_id: "poly-tok".into(), yes_price: py, no_price: pn, timestamp: Utc::now() });
        cache.update(&PriceUpdate { platform: Platform::Kalshi, market_id: "KALSHI-T".into(), yes_price: ky, no_price: kn, timestamp: Utc::now() });
    }

    #[test]
    fn test_detects_above_threshold() {
        let pid = Uuid::now_v7();
        let cache = Arc::new(PriceCache::new());
        fill_cache(&cache, pid, dec!(0.42), dec!(0.58), dec!(0.47), dec!(0.53));
        let d = Detector::new(cache, &cfg(dec!(3.0), dec!(0.02)));
        let opps = d.scan(&[pair_info(pid)]);
        assert_eq!(opps.len(), 1);
        assert_eq!(opps[0].spread, dec!(0.05));
        assert_eq!(opps[0].poly_side, Side::Yes);
        assert_eq!(opps[0].poly_market_id, "poly-tok");
    }

    #[test]
    fn test_rejects_below_threshold() {
        let pid = Uuid::now_v7();
        let cache = Arc::new(PriceCache::new());
        fill_cache(&cache, pid, dec!(0.50), dec!(0.50), dec!(0.51), dec!(0.49));
        let d = Detector::new(cache, &cfg(dec!(3.0), dec!(0.02)));
        assert!(d.scan(&[pair_info(pid)]).is_empty());
    }

    #[test]
    fn test_picks_best_direction() {
        let pid = Uuid::now_v7();
        let cache = Arc::new(PriceCache::new());
        fill_cache(&cache, pid, dec!(0.42), dec!(0.58), dec!(0.47), dec!(0.53));
        let d = Detector::new(cache, &cfg(dec!(3.0), dec!(0.02)));
        let opps = d.scan(&[pair_info(pid)]);
        assert_eq!(opps[0].poly_side, Side::Yes);
    }

    #[test]
    fn test_skips_unverified() {
        let pid = Uuid::now_v7();
        let cache = Arc::new(PriceCache::new());
        fill_cache(&cache, pid, dec!(0.42), dec!(0.58), dec!(0.47), dec!(0.53));
        let d = Detector::new(cache, &cfg(dec!(3.0), dec!(0.02)));
        let mut pi = pair_info(pid);
        pi.verified = false;
        assert!(d.scan(&[pi]).is_empty());
    }

    #[test]
    fn test_empty_pairs() {
        let cache = Arc::new(PriceCache::new());
        let d = Detector::new(cache, &cfg(dec!(3.0), dec!(0.02)));
        assert!(d.scan(&[]).is_empty());
    }
}
```

### Step 8: lib.rs — declare all modules with stubs

**File:** `crates/arb-engine/src/lib.rs`

```rust
pub mod types;
pub mod price_cache;
pub mod detector;
pub mod executor;
pub mod monitor;
pub mod unwinder;
pub mod tracker;
pub mod engine;
```

Create stub files for modules built in later prompts:
- `executor.rs`: `// Built in Prompt 4-B`
- `monitor.rs`: `// Built in Prompt 4-C`
- `unwinder.rs`: `// Built in Prompt 4-C`
- `tracker.rs`: `// Built in Prompt 4-D`
- `engine.rs`: `// Built in Prompt 4-D`

### Verification

```bash
cargo test -p arb-engine
# Expected: 5 price_cache + 5 detector = 10 tests
cargo clippy -p arb-engine -- -D warnings
cargo test --workspace  # verify arb-types changes don't break others
```

---

## Prompt 4-B: Executor

**File:** `crates/arb-engine/src/executor.rs`

```rust
use crate::types::{ExecutionResult, OrderConfig};
use arb_db::SqliteRepository;
use arb_db::models::{OpportunityRow, OrderRow};
use arb_risk::RiskManager;
use arb_types::{
    ArbError, LimitOrderRequest, Opportunity, OpportunityStatus,
    OrderResponse, Platform, PredictionMarketConnector,
};
use chrono::Utc;
use parking_lot::RwLock;
use rust_decimal::Decimal;
use std::sync::Arc;
use tracing::{error, info, warn};
use uuid::Uuid;

pub struct Executor {
    pub(crate) poly: Arc<dyn PredictionMarketConnector>,
    pub(crate) kalshi: Arc<dyn PredictionMarketConnector>,
    risk_manager: Arc<RwLock<RiskManager>>,
    db: Arc<SqliteRepository>,
    config: OrderConfig,
}

impl Executor {
    pub fn new(
        poly: Arc<dyn PredictionMarketConnector>,
        kalshi: Arc<dyn PredictionMarketConnector>,
        risk_manager: Arc<RwLock<RiskManager>>,
        db: Arc<SqliteRepository>,
        config: OrderConfig,
    ) -> Self {
        Self { poly, kalshi, risk_manager, db, config }
    }

    pub async fn execute(&self, opp: &mut Opportunity) -> Result<ExecutionResult, ArbError> {
        let quantity = self.config.default_quantity;

        // 1. Pre-trade risk check
        {
            let rm = self.risk_manager.read();
            let poly_balance = self.poly.get_balance().await?;
            let kalshi_balance = self.kalshi.get_balance().await?;
            let book = self.poly.get_order_book(&opp.poly_market_id).await
                .unwrap_or_default();
            let book_depth = book.asks.first().map(|l| l.quantity).unwrap_or(0);

            rm.pre_trade_check(
                opp.pair_id, true, opp.spread, self.config.min_repost_spread,
                opp.close_time, quantity, opp.poly_price, opp.kalshi_price,
                poly_balance, kalshi_balance, book_depth,
            ).map_err(|e| ArbError::Other(format!("risk check failed: {e}")))?;
        }

        // 2. Build orders — using REAL market IDs from the opportunity
        opp.max_quantity = quantity;
        opp.status = OpportunityStatus::Executing;

        let poly_req = LimitOrderRequest {
            market_id: opp.poly_market_id.clone(),
            side: opp.poly_side,
            price: opp.poly_price,
            quantity,
        };
        let kalshi_req = LimitOrderRequest {
            market_id: opp.kalshi_market_id.clone(),
            side: opp.kalshi_side,
            price: opp.kalshi_price,
            quantity,
        };

        // 3. Place BOTH simultaneously
        info!(opp_id = %opp.id, spread = %opp.spread, "executing dual-leg order");
        let (poly_result, kalshi_result) = tokio::join!(
            self.poly.place_limit_order(&poly_req),
            self.kalshi.place_limit_order(&kalshi_req),
        );

        // 4. Handle all 4 outcomes
        match (poly_result, kalshi_result) {
            (Ok(poly_order), Ok(kalshi_order)) => {
                info!(opp_id = %opp.id, poly = %poly_order.order_id, kalshi = %kalshi_order.order_id, "both legs placed");
                self.persist_opportunity(opp).await;
                self.persist_order(opp.id, Platform::Polymarket, &poly_order, &poly_req).await;
                self.persist_order(opp.id, Platform::Kalshi, &kalshi_order, &kalshi_req).await;
                Ok(ExecutionResult { opportunity_id: opp.id, poly_order, kalshi_order, executed_at: Utc::now() })
            }
            (Ok(poly_order), Err(kalshi_err)) => {
                warn!(opp_id = %opp.id, err = %kalshi_err, "kalshi failed, cancelling poly");
                let _ = self.poly.cancel_order(&poly_order.order_id).await;
                opp.status = OpportunityStatus::Failed;
                self.persist_opportunity(opp).await;
                Err(ArbError::PlatformError { platform: Platform::Kalshi, message: format!("kalshi leg failed: {kalshi_err}") })
            }
            (Err(poly_err), Ok(kalshi_order)) => {
                warn!(opp_id = %opp.id, err = %poly_err, "poly failed, cancelling kalshi");
                let _ = self.kalshi.cancel_order(&kalshi_order.order_id).await;
                opp.status = OpportunityStatus::Failed;
                self.persist_opportunity(opp).await;
                Err(ArbError::PlatformError { platform: Platform::Polymarket, message: format!("poly leg failed: {poly_err}") })
            }
            (Err(poly_err), Err(kalshi_err)) => {
                error!(opp_id = %opp.id, "both legs failed");
                opp.status = OpportunityStatus::Failed;
                self.persist_opportunity(opp).await;
                Err(ArbError::Other(format!("both legs failed: poly={poly_err}, kalshi={kalshi_err}")))
            }
        }
    }

    async fn persist_opportunity(&self, opp: &Opportunity) {
        let row = OpportunityRow {
            id: opp.id.to_string(),
            pair_id: opp.pair_id.to_string(),
            poly_side: format!("{:?}", opp.poly_side).to_lowercase(),
            poly_price: opp.poly_price,
            kalshi_side: format!("{:?}", opp.kalshi_side).to_lowercase(),
            kalshi_price: opp.kalshi_price,
            spread: opp.spread,
            spread_pct: opp.spread_pct,
            max_quantity: opp.max_quantity as i64,
            status: format!("{:?}", opp.status).to_lowercase(),
            detected_at: opp.detected_at,
            executed_at: Some(Utc::now()),
            resolved_at: None,
        };
        if let Err(e) = self.db.insert_opportunity(&row).await {
            error!(opp_id = %opp.id, err = %e, "failed to persist opportunity");
        }
    }

    async fn persist_order(&self, opp_id: Uuid, platform: Platform, resp: &OrderResponse, req: &LimitOrderRequest) {
        let row = OrderRow {
            id: Uuid::now_v7().to_string(),
            opportunity_id: opp_id.to_string(),
            platform: platform.to_string(),
            platform_order_id: Some(resp.order_id.clone()),
            market_id: req.market_id.clone(),
            side: format!("{:?}", req.side).to_lowercase(),
            price: req.price,
            quantity: req.quantity as i64,
            filled_quantity: resp.filled_quantity as i64,
            status: format!("{:?}", resp.status).to_lowercase(),
            placed_at: Utc::now(),
            filled_at: None,
            cancelled_at: None,
            cancel_reason: None,
        };
        if let Err(e) = self.db.insert_order(&row).await {
            error!(opp_id = %opp_id, err = %e, "failed to persist order");
        }
    }
}
```

### Verification
```bash
cargo check -p arb-engine
cargo clippy -p arb-engine -- -D warnings
```

---

## Prompt 4-C: Monitor + Unwinder

**File:** `crates/arb-engine/src/monitor.rs`

```rust
use crate::types::{MonitorAction, OrderConfig};
use arb_types::{ArbError, OrderStatus, Platform, PredictionMarketConnector};
use chrono::Utc;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info, warn};

pub struct Monitor {
    poly: Arc<dyn PredictionMarketConnector>,
    kalshi: Arc<dyn PredictionMarketConnector>,
    config: OrderConfig,
}

impl Monitor {
    pub fn new(poly: Arc<dyn PredictionMarketConnector>, kalshi: Arc<dyn PredictionMarketConnector>, config: OrderConfig) -> Self {
        Self { poly, kalshi, config }
    }

    pub async fn watch_order_pair(&self, poly_order_id: &str, kalshi_order_id: &str) -> Result<MonitorAction, ArbError> {
        let check_interval = Duration::from_millis(self.config.order_check_interval_ms);
        let max_order_age = Duration::from_secs(self.config.max_order_age_secs);
        let max_hedge_wait = Duration::from_secs(self.config.max_hedge_wait_secs);
        let start = Utc::now();
        let mut interval = tokio::time::interval(check_interval);

        loop {
            interval.tick().await;

            // Batch-friendly: use list_open_orders to avoid per-order rate limit hits
            let poly_order = self.poly.get_order(poly_order_id).await?;
            let kalshi_order = self.kalshi.get_order(kalshi_order_id).await?;

            let poly_filled = poly_order.status == OrderStatus::Filled;
            let kalshi_filled = kalshi_order.status == OrderStatus::Filled;
            let poly_done = matches!(poly_order.status, OrderStatus::Cancelled | OrderStatus::Failed);
            let kalshi_done = matches!(kalshi_order.status, OrderStatus::Cancelled | OrderStatus::Failed);

            let elapsed = (Utc::now() - start).to_std().unwrap_or(Duration::ZERO);

            match (poly_filled, kalshi_filled) {
                (true, true) => {
                    info!(poly = poly_order_id, kalshi = kalshi_order_id, "both filled");
                    return Ok(MonitorAction::BothFilled { poly_order, kalshi_order });
                }
                (true, false) => {
                    if kalshi_done || elapsed > max_hedge_wait {
                        warn!("poly filled, kalshi timeout — need unwind");
                        return Ok(MonitorAction::NeedsUnwind {
                            filled_platform: Platform::Polymarket,
                            filled_order: poly_order,
                            unfilled_order_id: kalshi_order_id.to_string(),
                        });
                    }
                    debug!("poly filled, waiting for kalshi ({:.1}s)", elapsed.as_secs_f64());
                }
                (false, true) => {
                    if poly_done || elapsed > max_hedge_wait {
                        warn!("kalshi filled, poly timeout — need unwind");
                        return Ok(MonitorAction::NeedsUnwind {
                            filled_platform: Platform::Kalshi,
                            filled_order: kalshi_order,
                            unfilled_order_id: poly_order_id.to_string(),
                        });
                    }
                    debug!("kalshi filled, waiting for poly ({:.1}s)", elapsed.as_secs_f64());
                }
                (false, false) => {
                    if (poly_done && kalshi_done) || elapsed > max_order_age {
                        if !poly_done { let _ = self.poly.cancel_order(poly_order_id).await; }
                        if !kalshi_done { let _ = self.kalshi.cancel_order(kalshi_order_id).await; }
                        return Ok(MonitorAction::BothCancelled);
                    }
                    debug!("both open ({:.1}s)", elapsed.as_secs_f64());
                }
            }
        }
    }
}
```

**File:** `crates/arb-engine/src/unwinder.rs`

```rust
use arb_risk::RiskManager;
use arb_types::{ArbError, LimitOrderRequest, OrderResponse, Platform, PredictionMarketConnector};
use parking_lot::RwLock;
use rust_decimal::Decimal;
use std::sync::Arc;
use tracing::{info, warn};

pub struct Unwinder {
    poly: Arc<dyn PredictionMarketConnector>,
    kalshi: Arc<dyn PredictionMarketConnector>,
    risk_manager: Arc<RwLock<RiskManager>>,
}

impl Unwinder {
    pub fn new(poly: Arc<dyn PredictionMarketConnector>, kalshi: Arc<dyn PredictionMarketConnector>, risk_manager: Arc<RwLock<RiskManager>>) -> Self {
        Self { poly, kalshi, risk_manager }
    }

    pub async fn unwind(&self, filled_platform: Platform, filled_order: &OrderResponse, unfilled_order_id: &str) -> Result<Decimal, ArbError> {
        let (filled_conn, unfilled_conn): (&dyn PredictionMarketConnector, &dyn PredictionMarketConnector) = match filled_platform {
            Platform::Polymarket => (self.poly.as_ref(), self.kalshi.as_ref()),
            Platform::Kalshi => (self.kalshi.as_ref(), self.poly.as_ref()),
        };

        // 1. Cancel unfilled leg
        if let Err(e) = unfilled_conn.cancel_order(unfilled_order_id).await {
            warn!(order = unfilled_order_id, err = %e, "cancel failed (may already be cancelled)");
        }

        // 2. Exit filled position at best bid
        let book = filled_conn.get_order_book(&filled_order.market_id).await?;
        let best_bid = book.bids.first()
            .ok_or_else(|| ArbError::Other("no bids available for unwind".into()))?;

        let unwind_req = LimitOrderRequest {
            market_id: filled_order.market_id.clone(),
            side: filled_order.side.opposite(),
            price: best_bid.price,
            quantity: filled_order.filled_quantity,
        };

        info!(platform = %filled_platform, entry = %filled_order.price, exit = %best_bid.price, qty = filled_order.filled_quantity, "unwinding");
        let _ = filled_conn.place_limit_order(&unwind_req).await?;

        // 3. Calculate + record loss
        let entry_cost = filled_order.price * Decimal::from(filled_order.filled_quantity);
        let exit_value = best_bid.price * Decimal::from(filled_order.filled_quantity);
        let loss = (entry_cost - exit_value).max(Decimal::ZERO);

        self.risk_manager.write().exposure_mut().record_unwind_loss(loss);
        warn!(platform = %filled_platform, loss = %loss, "unwind complete");
        Ok(loss)
    }
}
```

### Verification
```bash
cargo check -p arb-engine
cargo clippy -p arb-engine -- -D warnings
```

---

## Prompt 4-D: Tracker + Engine + Integration Tests

**File:** `crates/arb-engine/src/tracker.rs`

```rust
use arb_db::SqliteRepository;
use arb_db::models::PositionRow;
use arb_risk::RiskManager;
use arb_types::{ArbError, Opportunity, OrderResponse, Position, PositionStatus};
use chrono::Utc;
use parking_lot::RwLock;
use rust_decimal::Decimal;
use std::sync::Arc;
use tracing::{error, info};
use uuid::Uuid;

pub struct Tracker {
    db: Arc<SqliteRepository>,
    risk_manager: Arc<RwLock<RiskManager>>,
}

impl Tracker {
    pub fn new(db: Arc<SqliteRepository>, risk_manager: Arc<RwLock<RiskManager>>) -> Self {
        Self { db, risk_manager }
    }

    pub async fn create_position(&self, opp: &Opportunity, poly_order: &OrderResponse, kalshi_order: &OrderResponse) -> Result<Position, ArbError> {
        let hedged = poly_order.filled_quantity.min(kalshi_order.filled_quantity);
        let unhedged = (poly_order.filled_quantity as i32) - (kalshi_order.filled_quantity as i32);
        let profit = opp.spread * Decimal::from(hedged);

        let position = Position {
            id: Uuid::now_v7(), pair_id: opp.pair_id,
            poly_side: opp.poly_side, poly_quantity: poly_order.filled_quantity, poly_avg_price: poly_order.price,
            kalshi_side: opp.kalshi_side, kalshi_quantity: kalshi_order.filled_quantity, kalshi_avg_price: kalshi_order.price,
            hedged_quantity: hedged, unhedged_quantity: unhedged,
            guaranteed_profit: profit, status: PositionStatus::Open,
            opened_at: Utc::now(), settled_at: None,
        };

        let row = PositionRow {
            id: position.id.to_string(), pair_id: position.pair_id.to_string(),
            poly_side: format!("{:?}", position.poly_side).to_lowercase(),
            poly_quantity: position.poly_quantity as i64, poly_avg_price: position.poly_avg_price,
            kalshi_side: format!("{:?}", position.kalshi_side).to_lowercase(),
            kalshi_quantity: position.kalshi_quantity as i64, kalshi_avg_price: position.kalshi_avg_price,
            hedged_quantity: position.hedged_quantity as i64, unhedged_quantity: position.unhedged_quantity as i64,
            guaranteed_profit: position.guaranteed_profit, status: "open".into(),
            opened_at: position.opened_at, settled_at: None,
        };
        if let Err(e) = self.db.insert_position(&row).await {
            error!(pos = %position.id, err = %e, "persist position failed");
        }

        let capital = poly_order.price * Decimal::from(poly_order.filled_quantity)
            + kalshi_order.price * Decimal::from(kalshi_order.filled_quantity);
        self.risk_manager.write().exposure_mut().add_position(opp.pair_id, capital);

        info!(pos = %position.id, profit = %profit, hedged, "position created");
        Ok(position)
    }
}
```

**File:** `crates/arb-engine/src/engine.rs`

```rust
use crate::detector::Detector;
use crate::executor::Executor;
use crate::monitor::Monitor;
use crate::price_cache::PriceCache;
use crate::tracker::Tracker;
use crate::types::{EngineConfig, MonitorAction, PairInfo};
use crate::unwinder::Unwinder;
use arb_types::{ArbError, PredictionMarketConnector, PriceUpdate};
use parking_lot::Mutex;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, Semaphore};
use tracing::{error, info, warn};
use uuid::Uuid;

pub struct Engine {
    poly: Arc<dyn PredictionMarketConnector>,
    kalshi: Arc<dyn PredictionMarketConnector>,
    pub price_cache: Arc<PriceCache>,
    detector: Detector,
    executor: Arc<Executor>,
    monitor: Arc<Monitor>,
    tracker: Arc<Tracker>,
    unwinder: Arc<Unwinder>,
    config: EngineConfig,
    shutdown_tx: broadcast::Sender<()>,
    execution_semaphore: Arc<Semaphore>,
    executing_pairs: Arc<Mutex<HashSet<Uuid>>>,
}

impl Engine {
    pub fn new(
        poly: Arc<dyn PredictionMarketConnector>,
        kalshi: Arc<dyn PredictionMarketConnector>,
        price_cache: Arc<PriceCache>,
        executor: Executor, monitor: Monitor, tracker: Tracker, unwinder: Unwinder,
        config: EngineConfig,
    ) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        let detector = Detector::new(price_cache.clone(), &config);
        Self {
            poly, kalshi, price_cache, detector,
            executor: Arc::new(executor), monitor: Arc::new(monitor),
            tracker: Arc::new(tracker), unwinder: Arc::new(unwinder),
            config, shutdown_tx,
            execution_semaphore: Arc::new(Semaphore::new(2)),
            executing_pairs: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub async fn run(self: Arc<Self>, mut price_rx: mpsc::Receiver<PriceUpdate>, pairs: Vec<PairInfo>) -> Result<(), ArbError> {
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        let mut scan_timer = tokio::time::interval(std::time::Duration::from_millis(self.config.scan_interval_ms));
        info!(pairs = pairs.len(), "engine started");

        loop {
            tokio::select! {
                Some(update) = price_rx.recv() => { self.price_cache.update(&update); }
                _ = scan_timer.tick() => {
                    let opps = self.detector.scan(&pairs);
                    for mut opp in opps {
                        // Prevent duplicate execution for same pair
                        {
                            let executing = self.executing_pairs.lock();
                            if executing.contains(&opp.pair_id) { continue; }
                        }
                        self.executing_pairs.lock().insert(opp.pair_id);

                        let engine = self.clone();
                        let sem = self.execution_semaphore.clone();
                        let pairs_lock = self.executing_pairs.clone();
                        let pid = opp.pair_id;

                        tokio::spawn(async move {
                            let _permit = match sem.acquire().await { Ok(p) => p, Err(_) => { pairs_lock.lock().remove(&pid); return; } };
                            engine.handle_opportunity(&mut opp).await;
                            pairs_lock.lock().remove(&pid);
                        });
                    }
                }
                _ = shutdown_rx.recv() => { info!("shutdown signal"); break; }
            }
        }
        Ok(())
    }

    async fn handle_opportunity(self: &Arc<Self>, opp: &mut arb_types::Opportunity) {
        match self.executor.execute(opp).await {
            Ok(result) => {
                match self.monitor.watch_order_pair(&result.poly_order.order_id, &result.kalshi_order.order_id).await {
                    Ok(MonitorAction::BothFilled { poly_order, kalshi_order }) => {
                        if let Err(e) = self.tracker.create_position(opp, &poly_order, &kalshi_order).await {
                            error!(opp = %opp.id, err = %e, "position creation failed");
                        }
                    }
                    Ok(MonitorAction::NeedsUnwind { filled_platform, filled_order, unfilled_order_id }) => {
                        if let Err(e) = self.unwinder.unwind(filled_platform, &filled_order, &unfilled_order_id).await {
                            error!(opp = %opp.id, err = %e, "unwind failed");
                        }
                    }
                    Ok(MonitorAction::BothCancelled) => info!(opp = %opp.id, "both expired"),
                    Ok(_) => {} // Waiting/PartialHedge not returned as terminal
                    Err(e) => error!(opp = %opp.id, err = %e, "monitor error"),
                }
            }
            Err(e) => warn!(opp = %opp.id, err = %e, "execution failed"),
        }
    }

    pub async fn shutdown(&self) {
        info!("shutting down — cancelling open orders");
        let _ = self.shutdown_tx.send(());
        for conn in [&self.poly, &self.kalshi] {
            if let Ok(orders) = conn.list_open_orders().await {
                for o in &orders { let _ = conn.cancel_order(&o.order_id).await; }
                info!(platform = %conn.platform(), cancelled = orders.len());
            }
        }
        info!("shutdown complete");
    }
}
```

### Integration Tests

**File:** Add to bottom of `crates/arb-engine/src/engine.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::price_cache::PriceCache;
    use crate::types::{OrderConfig, PairInfo};
    use arb_db::SqliteRepository;
    use arb_risk::{RiskConfig, RiskManager};
    use arb_types::*;
    use arb_types::order::{OrderBookLevel, Side};
    use chrono::Duration as CDur;
    use parking_lot::RwLock;
    use rust_decimal_macros::dec;

    fn order_config() -> OrderConfig {
        OrderConfig {
            max_order_age_secs: 2, max_hedge_wait_secs: 3,
            order_check_interval_ms: 100, min_repost_spread: dec!(0.02),
            price_improve_amount: dec!(0.01), default_quantity: 50,
        }
    }

    fn engine_config() -> EngineConfig {
        EngineConfig { scan_interval_ms: 100, min_spread_pct: dec!(3.0), min_spread_absolute: dec!(0.02) }
    }

    #[cfg(feature = "mock")]
    mod with_mocks {
        use super::*;
        use arb_polymarket::MockPolymarketConnector;
        use arb_polymarket::MockState as PolyState;
        use arb_kalshi::MockKalshiConnector;
        use arb_kalshi::MockState as KalshiState;

        #[tokio::test]
        async fn test_executor_both_legs_succeed() {
            let db = Arc::new(SqliteRepository::new("sqlite::memory:").await.unwrap());
            db.run_migrations().await.unwrap();

            let poly_state = Arc::new(parking_lot::Mutex::new(PolyState::default()));
            poly_state.lock().balance = dec!(5000);
            poly_state.lock().order_books.insert("poly-tok".into(), OrderBook {
                market_id: "poly-tok".into(),
                bids: vec![OrderBookLevel { price: dec!(0.40), quantity: 100 }],
                asks: vec![OrderBookLevel { price: dec!(0.42), quantity: 100 }],
                timestamp: Utc::now(),
            });
            let poly: Arc<dyn PredictionMarketConnector> = Arc::new(MockPolymarketConnector::with_state(poly_state.clone()));

            let kalshi_state = Arc::new(parking_lot::Mutex::new(KalshiState::default()));
            kalshi_state.lock().balance = dec!(5000);
            let kalshi: Arc<dyn PredictionMarketConnector> = Arc::new(MockKalshiConnector::with_state(kalshi_state.clone()));

            let rm = Arc::new(RwLock::new(RiskManager::new(RiskConfig::default())));
            rm.write().set_engine_running(true);

            let executor = Executor::new(poly, kalshi, rm, db, order_config());

            let mut opp = Opportunity {
                id: Uuid::now_v7(), pair_id: Uuid::now_v7(),
                poly_side: Side::Yes, poly_price: dec!(0.42), poly_market_id: "poly-tok".into(),
                kalshi_side: Side::No, kalshi_price: dec!(0.53), kalshi_market_id: "kalshi-t".into(),
                spread: dec!(0.05), spread_pct: dec!(5.26), max_quantity: 0,
                close_time: Utc::now() + CDur::days(30),
                detected_at: Utc::now(), status: OpportunityStatus::Detected,
            };

            let result = executor.execute(&mut opp).await;
            assert!(result.is_ok(), "execution should succeed: {:?}", result.err());

            let r = result.unwrap();
            assert!(!r.poly_order.order_id.is_empty());
            assert!(!r.kalshi_order.order_id.is_empty());
            assert_eq!(poly_state.lock().placed_orders.len(), 1);
            assert_eq!(kalshi_state.lock().placed_orders.len(), 1);
        }

        #[tokio::test]
        async fn test_executor_kalshi_fails_cancels_poly() {
            let db = Arc::new(SqliteRepository::new("sqlite::memory:").await.unwrap());
            db.run_migrations().await.unwrap();

            let poly_state = Arc::new(parking_lot::Mutex::new(PolyState::default()));
            poly_state.lock().balance = dec!(5000);
            poly_state.lock().order_books.insert("poly-tok".into(), OrderBook::default());
            let poly: Arc<dyn PredictionMarketConnector> = Arc::new(MockPolymarketConnector::with_state(poly_state.clone()));

            let kalshi_state = Arc::new(parking_lot::Mutex::new(KalshiState::default()));
            kalshi_state.lock().balance = dec!(5000);
            kalshi_state.lock().inject_failure("connection refused");
            let kalshi: Arc<dyn PredictionMarketConnector> = Arc::new(MockKalshiConnector::with_state(kalshi_state.clone()));

            let rm = Arc::new(RwLock::new(RiskManager::new(RiskConfig::default())));
            rm.write().set_engine_running(true);

            let executor = Executor::new(poly, kalshi, rm, db, order_config());

            let mut opp = Opportunity {
                id: Uuid::now_v7(), pair_id: Uuid::now_v7(),
                poly_side: Side::Yes, poly_price: dec!(0.42), poly_market_id: "poly-tok".into(),
                kalshi_side: Side::No, kalshi_price: dec!(0.53), kalshi_market_id: "kalshi-t".into(),
                spread: dec!(0.05), spread_pct: dec!(5.26), max_quantity: 0,
                close_time: Utc::now() + CDur::days(30),
                detected_at: Utc::now(), status: OpportunityStatus::Detected,
            };

            let result = executor.execute(&mut opp).await;
            assert!(result.is_err());
            // Poly order was placed then cancelled
            assert_eq!(poly_state.lock().placed_orders.len(), 1);
            assert_eq!(poly_state.lock().cancelled_orders.len(), 1);
        }

        #[tokio::test]
        async fn test_tracker_creates_position() {
            let db = Arc::new(SqliteRepository::new("sqlite::memory:").await.unwrap());
            db.run_migrations().await.unwrap();
            let rm = Arc::new(RwLock::new(RiskManager::new(RiskConfig::default())));
            rm.write().set_engine_running(true);

            let tracker = Tracker::new(db, rm.clone());

            let opp = Opportunity {
                id: Uuid::now_v7(), pair_id: Uuid::now_v7(),
                poly_side: Side::Yes, poly_price: dec!(0.42), poly_market_id: "p".into(),
                kalshi_side: Side::No, kalshi_price: dec!(0.53), kalshi_market_id: "k".into(),
                spread: dec!(0.05), spread_pct: dec!(5.26), max_quantity: 50,
                close_time: Utc::now() + CDur::days(30),
                detected_at: Utc::now(), status: OpportunityStatus::Executing,
            };

            let poly_order = OrderResponse { order_id: "p1".into(), status: OrderStatus::Filled, filled_quantity: 50, price: dec!(0.42), side: Side::Yes, market_id: "p".into() };
            let kalshi_order = OrderResponse { order_id: "k1".into(), status: OrderStatus::Filled, filled_quantity: 50, price: dec!(0.53), side: Side::No, market_id: "k".into() };

            let pos = tracker.create_position(&opp, &poly_order, &kalshi_order).await.unwrap();
            assert_eq!(pos.hedged_quantity, 50);
            assert_eq!(pos.guaranteed_profit, dec!(2.50));
            assert_eq!(rm.read().exposure().total_exposure(), dec!(47.50)); // 0.42*50 + 0.53*50
        }
    }
}
```

### Verification

```bash
cargo test -p arb-engine
cargo test -p arb-engine --features mock
# Expected: 10 unit (cache+detector) + 3 integration = 13+
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

---

## Phase 4 Acceptance Criteria

- [ ] `cargo test --workspace` passes (all previous + engine = 130+)
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] `Opportunity` carries `poly_market_id`, `kalshi_market_id`, `close_time`
- [ ] Detector receives `PairInfo` (not bare UUIDs), only scans verified pairs
- [ ] Executor uses real market IDs from Opportunity (not price strings)
- [ ] All error handling uses actual `ArbError` variants (`PlatformError`, `Other`)
- [ ] Executor handles all 4 outcome combos, cancels successful leg on failure
- [ ] Monitor polls until terminal state, respects both timeouts
- [ ] Unwinder exits at best bid, records loss in exposure tracker
- [ ] Tracker creates position with correct hedged qty and profit math
- [ ] Engine prevents duplicate execution of same pair via `executing_pairs` lock
- [ ] Engine limits to 2 concurrent executions via semaphore
- [ ] Engine shutdown cancels all open orders on both platforms
- [ ] Integration tests pass with mock connectors (both-succeed + one-fails + tracker)
- [ ] `Side::opposite()` works, `OrderBook::default()` works

## Execution Order

```
4-A first  → Modify arb-types (Side, OrderBook, Opportunity) + PriceCache + Detector + types
4-B second → Executor (uses modified Opportunity + correct ArbError)
4-C third  → Monitor + Unwinder (uses Side::opposite)
4-D last   → Tracker + Engine orchestrator + integration tests
```
