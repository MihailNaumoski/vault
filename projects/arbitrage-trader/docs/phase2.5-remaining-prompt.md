# Phase 2.5 Remaining — Single Execution Prompt

**Goal:** Fix all remaining issues so the Phase 2.5 gate passes:
- `cargo test --workspace` ≥ 90 tests
- `cargo clippy --workspace -- -D warnings` passes (zero errors, zero warnings)
- All M1/M2/M3 fixes applied

**Project root:** `/Users/mihail/projects/vault/projects/arbitrage-trader`

---

## Fix 1: Clippy Error — Loop That Never Loops

**File:** `crates/arb-polymarket/src/connector.rs` lines 39-62

The `for _ in 0..5` loop has an unconditional `break` at the end, so it only runs once. Clippy rightly errors on this.

**Fix:** Remove the loop since we're only taking the first page for MVP:

```rust
// REPLACE the entire list_markets method body (lines 35-64) with:
async fn list_markets(&self, status: MarketStatus) -> Result<Vec<Market>, ArbError> {
    let mut all_markets = Vec::new();

    // MVP: fetch first page only. Pagination will be added when needed.
    let page = self
        .client
        .fetch_markets(None)
        .await
        .map_err(ArbError::from)?;

    for m in &page {
        let market = m.to_market();
        if market.status == status {
            all_markets.push(market);
        }
    }

    Ok(all_markets)
}
```

---

## Fix 2: Clippy Warnings — Unused Fields + Redundant Closure

**File:** `crates/arb-cli/src/main.rs`

The config structs `EngineConfig`, `OrdersConfig`, `PlatformConfig`, `KalshiConfig` have fields that are parsed but never read yet. Suppress with `#[allow(dead_code)]` since they'll be used in Phase 4:

```rust
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct EngineConfig { ... }

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct OrdersConfig { ... }

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct PlatformConfig { ... }

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct KalshiConfig { ... }
```

**Also fix:** Any `redundant closure` warnings — replace `.map(|x| foo(x))` with `.map(foo)`.

**Also fix:** Any `manual implementation of .is_multiple_of()` in arb-kalshi — replace the manual modulo check with `.is_multiple_of()` or suppress if the method doesn't exist in the version used.

---

## Fix 3: M1 — Price Bounds Validation on API Responses

**File:** `crates/arb-polymarket/src/connector.rs`

Add price validation when converting API responses to domain types. In the `to_market()`, `to_order_response()`, and order book conversion methods, validate that prices are within [0.00, 1.00].

Use the existing `validate_price()` function from `arb-types::price`:

```rust
use arb_types::price::validate_price;

// In to_order_book() or wherever prices are converted from API response:
fn validate_api_price(price: Decimal, context: &str) -> Result<Decimal, ArbError> {
    if !validate_price(price) {
        return Err(ArbError::Connector {
            platform: Platform::Polymarket,
            message: format!("price {} out of range [0.00, 1.00] in {}", price, context),
            source: None,
        });
    }
    Ok(price)
}
```

Apply the same pattern in `crates/arb-kalshi/src/connector.rs`.

---

## Fix 4: M2 — Token ID / Market ID Validation

**File:** `crates/arb-polymarket/src/connector.rs`

At the start of `place_limit_order()`:

```rust
async fn place_limit_order(&self, req: &LimitOrderRequest) -> Result<OrderResponse, ArbError> {
    if req.market_id.is_empty() {
        return Err(ArbError::Connector {
            platform: Platform::Polymarket,
            message: "market_id (token_id) is empty — cannot place order".into(),
            source: None,
        });
    }
    // ... existing code
}
```

Apply the same in `crates/arb-kalshi/src/connector.rs` `place_limit_order()`:

```rust
async fn place_limit_order(&self, req: &LimitOrderRequest) -> Result<OrderResponse, ArbError> {
    if req.market_id.is_empty() {
        return Err(ArbError::Connector {
            platform: Platform::Kalshi,
            message: "market_id (ticker) is empty — cannot place order".into(),
            source: None,
        });
    }
    // ... existing code
}
```

---

## Fix 5: M3 — WebSocket Active Ping for Silent Disconnect Detection

**File:** `crates/arb-polymarket/src/ws.rs`

In the WebSocket read loop, add a ping interval. Find the main `loop` that reads messages and wrap it with `tokio::select!`:

```rust
use tokio::time::{interval, Instant, Duration};

// Inside the connection handler, before or at the start of the read loop:
let mut ping_interval = interval(Duration::from_secs(30));
let mut last_message_time = Instant::now();

// Then in the loop, use select:
loop {
    tokio::select! {
        msg = ws_read.next() => {
            match msg {
                Some(Ok(message)) => {
                    last_message_time = Instant::now();
                    // ... existing message handling ...
                }
                Some(Err(e)) => { /* existing reconnect logic */ }
                None => { /* existing stream-ended logic */ }
            }
        }
        _ = ping_interval.tick() => {
            if last_message_time.elapsed() > Duration::from_secs(90) {
                tracing::warn!("polymarket WS: no messages for 90s, triggering reconnect");
                break; // break inner loop to trigger reconnection
            }
            // Optionally send a ping frame if the WS supports it
            // let _ = ws_write.send(Message::Ping(vec![])).await;
        }
    }
}
```

Apply the same pattern in `crates/arb-kalshi/src/ws.rs` (Kalshi already handles incoming Pings at line 317 — add the active staleness check).

---

## Fix 6: arb-db Test Suite

**File:** `crates/arb-db/src/repo.rs` — add at the bottom:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, Utc};
    use rust_decimal_macros::dec;
    use uuid::Uuid;

    async fn setup_db() -> SqliteRepository {
        let repo = SqliteRepository::new("sqlite::memory:").await.unwrap();
        repo.run_migrations().await.unwrap();
        repo
    }

    fn make_pair() -> MarketPairRow {
        MarketPairRow {
            id: Uuid::now_v7(),
            poly_condition_id: "cond-123".into(),
            poly_yes_token_id: "tok-yes-123".into(),
            poly_no_token_id: "tok-no-123".into(),
            poly_question: "Will X happen?".into(),
            kalshi_ticker: "X-HAPPEN-2026".into(),
            kalshi_question: "Will X happen by end of 2026?".into(),
            match_confidence: 0.92,
            verified: true,
            active: true,
            close_time: Utc::now() + chrono::Duration::days(30),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn make_opportunity(pair_id: Uuid) -> OpportunityRow {
        OpportunityRow {
            id: Uuid::now_v7(),
            pair_id,
            poly_side: "yes".into(),
            poly_price: dec!(0.42).to_string(),
            kalshi_side: "no".into(),
            kalshi_price: dec!(0.53).to_string(),
            spread: dec!(0.05).to_string(),
            spread_pct: dec!(5.0).to_string(),
            max_quantity: 50,
            status: "detected".into(),
            detected_at: Utc::now(),
            executed_at: None,
            resolved_at: None,
        }
    }

    fn make_order(opportunity_id: Uuid) -> OrderRow {
        OrderRow {
            id: Uuid::now_v7(),
            opportunity_id,
            platform: "polymarket".into(),
            platform_order_id: Some("poly-order-123".into()),
            market_id: "tok-yes-123".into(),
            side: "yes".into(),
            price: dec!(0.42).to_string(),
            quantity: 50,
            filled_quantity: 0,
            status: "pending".into(),
            placed_at: Utc::now(),
            filled_at: None,
            cancelled_at: None,
            cancel_reason: None,
        }
    }

    fn make_position(pair_id: Uuid) -> PositionRow {
        PositionRow {
            id: Uuid::now_v7(),
            pair_id,
            poly_side: "yes".into(),
            poly_quantity: 50,
            poly_avg_price: dec!(0.42).to_string(),
            kalshi_side: "no".into(),
            kalshi_quantity: 50,
            kalshi_avg_price: dec!(0.53).to_string(),
            hedged_quantity: 50,
            unhedged_quantity: 0,
            guaranteed_profit: dec!(2.50).to_string(),
            status: "open".into(),
            opened_at: Utc::now(),
            settled_at: None,
        }
    }

    // === Market Pairs ===

    #[tokio::test]
    async fn test_insert_and_get_market_pair() {
        let repo = setup_db().await;
        let pair = make_pair();
        repo.insert_market_pair(&pair).await.unwrap();
        let fetched = repo.get_market_pair(&pair.id).await.unwrap().unwrap();
        assert_eq!(fetched.id, pair.id);
        assert_eq!(fetched.kalshi_ticker, pair.kalshi_ticker);
    }

    #[tokio::test]
    async fn test_list_active_market_pairs() {
        let repo = setup_db().await;
        let pair = make_pair();
        repo.insert_market_pair(&pair).await.unwrap();
        let active = repo.list_active_market_pairs().await.unwrap();
        assert_eq!(active.len(), 1);
    }

    #[tokio::test]
    async fn test_update_market_pair() {
        let repo = setup_db().await;
        let mut pair = make_pair();
        repo.insert_market_pair(&pair).await.unwrap();
        pair.verified = false;
        repo.update_market_pair(&pair).await.unwrap();
        let fetched = repo.get_market_pair(&pair.id).await.unwrap().unwrap();
        assert!(!fetched.verified);
    }

    #[tokio::test]
    async fn test_delete_market_pair() {
        let repo = setup_db().await;
        let pair = make_pair();
        repo.insert_market_pair(&pair).await.unwrap();
        repo.delete_market_pair(&pair.id).await.unwrap();
        let fetched = repo.get_market_pair(&pair.id).await.unwrap();
        assert!(fetched.is_none());
    }

    // === Opportunities ===

    #[tokio::test]
    async fn test_insert_and_get_opportunity() {
        let repo = setup_db().await;
        let pair = make_pair();
        repo.insert_market_pair(&pair).await.unwrap();
        let opp = make_opportunity(pair.id);
        repo.insert_opportunity(&opp).await.unwrap();
        let fetched = repo.get_opportunity(&opp.id).await.unwrap().unwrap();
        assert_eq!(fetched.id, opp.id);
        assert_eq!(fetched.spread, opp.spread);
    }

    #[tokio::test]
    async fn test_list_opportunities_by_status() {
        let repo = setup_db().await;
        let pair = make_pair();
        repo.insert_market_pair(&pair).await.unwrap();
        let opp = make_opportunity(pair.id);
        repo.insert_opportunity(&opp).await.unwrap();
        let detected = repo.list_opportunities_by_status("detected").await.unwrap();
        assert_eq!(detected.len(), 1);
        let executed = repo.list_opportunities_by_status("executed").await.unwrap();
        assert_eq!(executed.len(), 0);
    }

    #[tokio::test]
    async fn test_update_opportunity_status() {
        let repo = setup_db().await;
        let pair = make_pair();
        repo.insert_market_pair(&pair).await.unwrap();
        let opp = make_opportunity(pair.id);
        repo.insert_opportunity(&opp).await.unwrap();
        repo.update_opportunity_status(&opp.id, "executed", Some(Utc::now())).await.unwrap();
        let fetched = repo.get_opportunity(&opp.id).await.unwrap().unwrap();
        assert_eq!(fetched.status, "executed");
    }

    // === Orders ===

    #[tokio::test]
    async fn test_insert_and_get_order() {
        let repo = setup_db().await;
        let pair = make_pair();
        repo.insert_market_pair(&pair).await.unwrap();
        let opp = make_opportunity(pair.id);
        repo.insert_opportunity(&opp).await.unwrap();
        let order = make_order(opp.id);
        repo.insert_order(&order).await.unwrap();
        let fetched = repo.get_order(&order.id).await.unwrap().unwrap();
        assert_eq!(fetched.id, order.id);
    }

    #[tokio::test]
    async fn test_list_orders_by_status() {
        let repo = setup_db().await;
        let pair = make_pair();
        repo.insert_market_pair(&pair).await.unwrap();
        let opp = make_opportunity(pair.id);
        repo.insert_opportunity(&opp).await.unwrap();
        let order = make_order(opp.id);
        repo.insert_order(&order).await.unwrap();
        let pending = repo.list_orders_by_status("pending").await.unwrap();
        assert_eq!(pending.len(), 1);
    }

    // === Positions ===

    #[tokio::test]
    async fn test_insert_and_get_position() {
        let repo = setup_db().await;
        let pair = make_pair();
        repo.insert_market_pair(&pair).await.unwrap();
        let pos = make_position(pair.id);
        repo.insert_position(&pos).await.unwrap();
        let fetched = repo.get_position(&pos.id).await.unwrap().unwrap();
        assert_eq!(fetched.id, pos.id);
        assert_eq!(fetched.hedged_quantity, 50);
    }

    #[tokio::test]
    async fn test_list_open_positions() {
        let repo = setup_db().await;
        let pair = make_pair();
        repo.insert_market_pair(&pair).await.unwrap();
        let pos = make_position(pair.id);
        repo.insert_position(&pos).await.unwrap();
        let open = repo.list_open_positions().await.unwrap();
        assert_eq!(open.len(), 1);
    }

    // === Price Snapshots ===

    #[tokio::test]
    async fn test_insert_and_list_price_snapshots() {
        let repo = setup_db().await;
        let pair = make_pair();
        repo.insert_market_pair(&pair).await.unwrap();
        let snapshot = NewPriceSnapshot {
            pair_id: pair.id,
            poly_yes_price: dec!(0.42).to_string(),
            kalshi_yes_price: dec!(0.47).to_string(),
            spread: dec!(0.05).to_string(),
            captured_at: Utc::now(),
        };
        repo.insert_price_snapshot(&snapshot).await.unwrap();
        let snapshots = repo.list_price_snapshots(&pair.id, 10).await.unwrap();
        assert_eq!(snapshots.len(), 1);
    }

    // === Daily P&L ===

    #[tokio::test]
    async fn test_upsert_and_get_daily_pnl() {
        let repo = setup_db().await;
        let today = Utc::now().date_naive();
        let pnl = DailyPnlRow {
            date: today,
            trades_executed: 5,
            trades_filled: 4,
            gross_profit: dec!(10.50).to_string(),
            fees_paid: dec!(2.00).to_string(),
            net_profit: dec!(8.50).to_string(),
            capital_deployed: dec!(500.00).to_string(),
        };
        repo.upsert_daily_pnl(&pnl).await.unwrap();
        let fetched = repo.get_daily_pnl(today).await.unwrap().unwrap();
        assert_eq!(fetched.trades_executed, 5);
        assert_eq!(fetched.net_profit, "8.50");
    }

    // === Foreign Key Constraint ===

    #[tokio::test]
    async fn test_order_requires_valid_opportunity() {
        let repo = setup_db().await;
        let order = make_order(Uuid::now_v7()); // non-existent opportunity
        let result = repo.insert_order(&order).await;
        assert!(result.is_err(), "should reject order with invalid opportunity_id");
    }
}
```

**Note:** Adapt the field names and constructor signatures to match the actual `MarketPairRow`, `OpportunityRow`, `OrderRow`, `PositionRow`, `NewPriceSnapshot`, `DailyPnlRow` structs in `crates/arb-db/src/models.rs`. Read those structs first and adjust the test helpers accordingly. The test names and logic are correct — only the field names may need tweaking.

---

## Fix 7: arb-risk Test Suite

**File:** `crates/arb-risk/src/manager.rs` — add at the bottom:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};
    use rust_decimal_macros::dec;
    use uuid::Uuid;

    fn default_manager() -> RiskManager {
        RiskManager::new(RiskConfig::default())
    }

    fn running_manager() -> RiskManager {
        let mut rm = default_manager();
        rm.set_engine_running(true);
        rm
    }

    // Valid params that pass all checks
    fn valid_check_params() -> (Uuid, bool, Decimal, Decimal, chrono::DateTime<Utc>, u32, Decimal, Decimal, Decimal, Decimal, u32) {
        (
            Uuid::now_v7(),        // pair_id
            true,                   // pair_verified
            dec!(5.0),             // spread (5% > default 3% min)
            dec!(3.0),             // min_spread
            Utc::now() + Duration::hours(48), // close_time (48h > 24h min)
            50,                     // quantity
            dec!(0.42),            // poly_price
            dec!(0.53),            // kalshi_price
            dec!(5000),            // poly_balance
            dec!(5000),            // kalshi_balance
            100,                    // book_depth (100 > 50 min)
        )
    }

    #[test]
    fn test_passes_with_valid_params() {
        let rm = running_manager();
        let (pair_id, verified, spread, min_spread, close, qty, pp, kp, pb, kb, depth) = valid_check_params();
        assert!(rm.pre_trade_check(pair_id, verified, spread, min_spread, close, qty, pp, kp, pb, kb, depth).is_ok());
    }

    #[test]
    fn test_rejects_engine_not_running() {
        let rm = default_manager(); // engine_running = false
        let (pair_id, verified, spread, min_spread, close, qty, pp, kp, pb, kb, depth) = valid_check_params();
        let err = rm.pre_trade_check(pair_id, verified, spread, min_spread, close, qty, pp, kp, pb, kb, depth).unwrap_err();
        assert!(matches!(err, RiskError::EngineNotRunning));
    }

    #[test]
    fn test_rejects_unverified_pair() {
        let rm = running_manager();
        let (pair_id, _, spread, min_spread, close, qty, pp, kp, pb, kb, depth) = valid_check_params();
        let err = rm.pre_trade_check(pair_id, false, spread, min_spread, close, qty, pp, kp, pb, kb, depth).unwrap_err();
        assert!(matches!(err, RiskError::PairNotVerified(_)));
    }

    #[test]
    fn test_rejects_spread_too_low() {
        let rm = running_manager();
        let (pair_id, verified, _, min_spread, close, qty, pp, kp, pb, kb, depth) = valid_check_params();
        let err = rm.pre_trade_check(pair_id, verified, dec!(1.0), min_spread, close, qty, pp, kp, pb, kb, depth).unwrap_err();
        assert!(matches!(err, RiskError::SpreadTooLow { .. }));
    }

    #[test]
    fn test_rejects_too_close_to_expiry() {
        let rm = running_manager();
        let (pair_id, verified, spread, min_spread, _, qty, pp, kp, pb, kb, depth) = valid_check_params();
        let close = Utc::now() + Duration::hours(2); // 2h < 24h min
        let err = rm.pre_trade_check(pair_id, verified, spread, min_spread, close, qty, pp, kp, pb, kb, depth).unwrap_err();
        assert!(matches!(err, RiskError::TooCloseToExpiry { .. }));
    }

    #[test]
    fn test_rejects_insufficient_poly_balance() {
        let rm = running_manager();
        let (pair_id, verified, spread, min_spread, close, qty, pp, kp, _, kb, depth) = valid_check_params();
        let err = rm.pre_trade_check(pair_id, verified, spread, min_spread, close, qty, pp, kp, dec!(1), kb, depth).unwrap_err();
        assert!(matches!(err, RiskError::InsufficientBalance { .. }));
    }

    #[test]
    fn test_rejects_insufficient_kalshi_balance() {
        let rm = running_manager();
        let (pair_id, verified, spread, min_spread, close, qty, pp, kp, pb, _, depth) = valid_check_params();
        let err = rm.pre_trade_check(pair_id, verified, spread, min_spread, close, qty, pp, kp, pb, dec!(1), depth).unwrap_err();
        assert!(matches!(err, RiskError::InsufficientBalance { .. }));
    }

    #[test]
    fn test_rejects_position_limit_exceeded() {
        let mut rm = running_manager();
        let (pair_id, verified, spread, min_spread, close, _, pp, kp, pb, kb, depth) = valid_check_params();
        // Pre-fill exposure near the limit
        rm.exposure_mut().add_position(pair_id, dec!(990));
        let err = rm.pre_trade_check(pair_id, verified, spread, min_spread, close, 50, pp, kp, pb, kb, depth).unwrap_err();
        assert!(matches!(err, RiskError::PositionLimitExceeded { .. }));
    }

    #[test]
    fn test_rejects_total_exposure_exceeded() {
        let mut rm = running_manager();
        let (pair_id, verified, spread, min_spread, close, qty, pp, kp, pb, kb, depth) = valid_check_params();
        // Fill total exposure near limit
        rm.exposure_mut().add_position(Uuid::now_v7(), dec!(9990));
        let err = rm.pre_trade_check(pair_id, verified, spread, min_spread, close, qty, pp, kp, pb, kb, depth).unwrap_err();
        assert!(matches!(err, RiskError::TotalExposureExceeded { .. }));
    }

    #[test]
    fn test_rejects_daily_loss_exceeded() {
        let mut rm = running_manager();
        let (pair_id, verified, spread, min_spread, close, qty, pp, kp, pb, kb, depth) = valid_check_params();
        rm.exposure_mut().record_unwind_loss(dec!(250)); // > 200 max
        let err = rm.pre_trade_check(pair_id, verified, spread, min_spread, close, qty, pp, kp, pb, kb, depth).unwrap_err();
        assert!(matches!(err, RiskError::DailyLossExceeded { .. }));
    }

    #[test]
    fn test_rejects_insufficient_book_depth() {
        let rm = running_manager();
        let (pair_id, verified, spread, min_spread, close, qty, pp, kp, pb, kb, _) = valid_check_params();
        let err = rm.pre_trade_check(pair_id, verified, spread, min_spread, close, qty, pp, kp, pb, kb, 10).unwrap_err();
        assert!(matches!(err, RiskError::InsufficientLiquidity { .. }));
    }

    #[test]
    fn test_exposure_tracker_add_remove() {
        let mut tracker = ExposureTracker::new();
        let pair = Uuid::now_v7();
        tracker.add_position(pair, dec!(100));
        assert_eq!(tracker.total_exposure(), dec!(100));
        assert_eq!(tracker.market_exposure(&pair), dec!(100));
        tracker.remove_position(&pair, dec!(100));
        assert_eq!(tracker.total_exposure(), dec!(0));
    }

    #[test]
    fn test_exposure_tracker_daily_reset() {
        let mut tracker = ExposureTracker::new();
        tracker.record_unwind_loss(dec!(50));
        assert_eq!(tracker.daily_loss(), dec!(50));
        tracker.reset_daily();
        assert_eq!(tracker.daily_loss(), dec!(0));
        assert_eq!(tracker.unwind_rate_pct(), dec!(0));
    }

    #[test]
    fn test_risk_config_default() {
        let config = RiskConfig::default();
        assert_eq!(config.max_daily_loss, dec!(200));
        assert_eq!(config.min_time_to_close_hours, 24);
        assert_eq!(config.min_book_depth, 50);
    }
}
```

**Note:** Adapt the test helper params to match the exact `pre_trade_check()` signature in `manager.rs`. The function takes 11 parameters — the tests above match the current signature. If any param name or type differs, adjust accordingly.

---

## Verification

After all fixes, run these commands — ALL must pass:

```bash
# 1. Clippy (zero errors, zero warnings)
cargo clippy --workspace -- -D warnings

# 2. Tests (must be >= 90)
cargo test --workspace 2>&1 | grep "test result"
# Expected: 27 (poly) + 38 (kalshi) + 3 (types) + ~15 (db) + ~14 (risk) = 97+

# 3. Quick sanity
cargo build --workspace
cargo run -- --help
```

---

## Summary of Changes

| File | Change | Type |
|------|--------|------|
| `crates/arb-polymarket/src/connector.rs` | Fix loop-that-never-loops, add market_id validation, add price validation | Bug fix |
| `crates/arb-kalshi/src/connector.rs` | Add market_id validation, add price validation | Bug fix |
| `crates/arb-polymarket/src/ws.rs` | Add ping interval + staleness detection | Reliability |
| `crates/arb-kalshi/src/ws.rs` | Add ping interval + staleness detection | Reliability |
| `crates/arb-cli/src/main.rs` | `#[allow(dead_code)]` on unused config structs | Clippy |
| `crates/arb-db/src/repo.rs` | Add 15 CRUD tests | Tests |
| `crates/arb-risk/src/manager.rs` | Add 14 risk check tests | Tests |
