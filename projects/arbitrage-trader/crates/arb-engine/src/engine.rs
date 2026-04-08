use crate::detector::Detector;
use crate::executor::Executor;
use crate::fees::FeeConfig;
use crate::monitor::Monitor;
use crate::price_cache::PriceCache;
use crate::tracker::Tracker;
use crate::types::{EngineConfig, MonitorAction, PairInfo};
use crate::unwinder::Unwinder;
use arb_db::models::{DailyPnlRow, NewPriceSnapshot};
use arb_db::{Repository, SqliteRepository};
use arb_types::{ArbError, OrderStatus, PredictionMarketConnector, PriceUpdate};
use chrono::Utc;
use parking_lot::Mutex;
use rust_decimal::Decimal;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, Semaphore};
use tracing::{error, info, warn};
use uuid::Uuid;

pub struct Engine {
    poly: Arc<dyn PredictionMarketConnector>,
    kalshi: Arc<dyn PredictionMarketConnector>,
    pub price_cache: Arc<PriceCache>,
    db: Arc<SqliteRepository>,
    detector: Detector,
    executor: Arc<Executor>,
    monitor: Arc<Monitor>,
    tracker: Arc<Tracker>,
    unwinder: Arc<Unwinder>,
    config: EngineConfig,
    mode: String,
    fee_config: FeeConfig,
    shutdown_tx: broadcast::Sender<()>,
    execution_semaphore: Arc<Semaphore>,
    executing_pairs: Arc<Mutex<HashSet<Uuid>>>,
}

impl Engine {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        poly: Arc<dyn PredictionMarketConnector>,
        kalshi: Arc<dyn PredictionMarketConnector>,
        price_cache: Arc<PriceCache>,
        db: Arc<SqliteRepository>,
        executor: Executor,
        monitor: Monitor,
        tracker: Tracker,
        unwinder: Unwinder,
        config: EngineConfig,
        mode: String,
        fee_config: FeeConfig,
    ) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        let detector = Detector::new(price_cache.clone(), &config, &fee_config);
        Self {
            poly,
            kalshi,
            price_cache,
            db,
            detector,
            executor: Arc::new(executor),
            monitor: Arc::new(monitor),
            tracker: Arc::new(tracker),
            unwinder: Arc::new(unwinder),
            config,
            mode,
            fee_config,
            shutdown_tx,
            execution_semaphore: Arc::new(Semaphore::new(2)),
            executing_pairs: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub async fn run(
        self: Arc<Self>,
        mut price_rx: mpsc::Receiver<PriceUpdate>,
        pairs: Vec<PairInfo>,
    ) -> Result<(), ArbError> {
        let mut shutdown_rx = self.shutdown_tx.subscribe();
        let mut scan_timer = tokio::time::interval(std::time::Duration::from_millis(
            self.config.scan_interval_ms,
        ));
        let mut snapshot_timer = tokio::time::interval(std::time::Duration::from_secs(30));
        let mut pnl_timer = tokio::time::interval(std::time::Duration::from_secs(60));
        info!(pairs = pairs.len(), "engine started");

        loop {
            tokio::select! {
                Some(update) = price_rx.recv() => {
                    self.price_cache.update(&update);
                }
                _ = scan_timer.tick() => {
                    let opps = self.detector.scan(&pairs);
                    for mut opp in opps {
                        {
                            let executing = self.executing_pairs.lock();
                            if executing.contains(&opp.pair_id) {
                                continue;
                            }
                        }
                        self.executing_pairs.lock().insert(opp.pair_id);

                        let engine = self.clone();
                        let sem = self.execution_semaphore.clone();
                        let pairs_lock = self.executing_pairs.clone();
                        let pid = opp.pair_id;

                        tokio::spawn(async move {
                            let _permit = match sem.acquire().await {
                                Ok(p) => p,
                                Err(_) => {
                                    pairs_lock.lock().remove(&pid);
                                    return;
                                }
                            };
                            engine.handle_opportunity(&mut opp).await;
                            pairs_lock.lock().remove(&pid);
                        });
                    }
                }
                _ = snapshot_timer.tick() => {
                    self.capture_price_snapshots(&pairs).await;
                }
                _ = pnl_timer.tick() => {
                    self.aggregate_daily_pnl().await;
                }
                _ = shutdown_rx.recv() => {
                    info!("shutdown signal");
                    break;
                }
            }
        }
        Ok(())
    }

    async fn handle_opportunity(self: &Arc<Self>, opp: &mut arb_types::Opportunity) {
        match self.executor.execute(opp).await {
            Ok(result) => {
                match self
                    .monitor
                    .watch_order_pair(
                        &result.poly_order.order_id,
                        &result.kalshi_order.order_id,
                    )
                    .await
                {
                    Ok(MonitorAction::BothFilled {
                        poly_order,
                        kalshi_order,
                    }) => {
                        // Update order statuses in DB
                        self.update_order_in_db(opp.id, &poly_order).await;
                        self.update_order_in_db(opp.id, &kalshi_order).await;

                        if let Err(e) = self
                            .tracker
                            .create_position(opp, &poly_order, &kalshi_order)
                            .await
                        {
                            error!(opp = %opp.id, err = %e, "position creation failed");
                        }
                    }
                    Ok(MonitorAction::NeedsUnwind {
                        filled_platform,
                        filled_order,
                        unfilled_order_id,
                    }) => {
                        // Update filled order status in DB
                        self.update_order_in_db(opp.id, &filled_order).await;
                        // Mark unfilled order as cancelled in DB
                        self.cancel_order_in_db(opp.id, &unfilled_order_id).await;

                        if let Err(e) = self
                            .unwinder
                            .unwind(filled_platform, &filled_order, &unfilled_order_id, None)
                            .await
                        {
                            error!(opp = %opp.id, err = %e, "unwind failed");
                        }
                    }
                    Ok(MonitorAction::BothCancelled) => {
                        // Mark both orders as cancelled in DB
                        self.cancel_order_in_db(opp.id, &result.poly_order.order_id).await;
                        self.cancel_order_in_db(opp.id, &result.kalshi_order.order_id).await;
                        info!(opp = %opp.id, "both expired");
                    }
                    Ok(_) => {}
                    Err(e) => error!(opp = %opp.id, err = %e, "monitor error"),
                }
            }
            Err(e) => warn!(opp = %opp.id, err = %e, "execution failed"),
        }
    }

    /// Update an order's status in the database after the monitor reports its final state.
    async fn update_order_in_db(&self, opp_id: Uuid, order: &arb_types::OrderResponse) {
        let orders = match self.db.list_orders_by_opportunity(&opp_id).await {
            Ok(orders) => orders,
            Err(e) => {
                error!(opp = %opp_id, err = %e, "failed to list orders for status update");
                return;
            }
        };
        // Match by platform_order_id
        if let Some(db_order) = orders.iter().find(|o| {
            o.platform_order_id.as_deref() == Some(&order.order_id)
        }) {
            let id = match db_order.id.parse::<Uuid>() {
                Ok(id) => id,
                Err(_) => return,
            };
            let status = format!("{:?}", order.status).to_lowercase();
            let filled_at = if order.status == OrderStatus::Filled {
                Some(Utc::now())
            } else {
                None
            };
            if let Err(e) = self.db.update_order_status(
                &id,
                &status,
                order.filled_quantity as i64,
                filled_at,
                None,
                None,
            ).await {
                error!(order_id = %order.order_id, err = %e, "failed to update order status");
            }
        }
    }

    /// Mark an order as cancelled in the database by its platform order ID.
    async fn cancel_order_in_db(&self, opp_id: Uuid, platform_order_id: &str) {
        let orders = match self.db.list_orders_by_opportunity(&opp_id).await {
            Ok(orders) => orders,
            Err(e) => {
                error!(opp = %opp_id, err = %e, "failed to list orders for cancel update");
                return;
            }
        };
        if let Some(db_order) = orders.iter().find(|o| {
            o.platform_order_id.as_deref() == Some(platform_order_id)
        }) {
            let id = match db_order.id.parse::<Uuid>() {
                Ok(id) => id,
                Err(_) => return,
            };
            if let Err(e) = self.db.update_order_status(
                &id,
                "cancelled",
                db_order.filled_quantity,
                None,
                Some(Utc::now()),
                Some("monitor_timeout"),
            ).await {
                error!(order_id = platform_order_id, err = %e, "failed to cancel order in db");
            }
        }
    }

    /// Capture price snapshots for all active pairs.
    async fn capture_price_snapshots(&self, pairs: &[PairInfo]) {
        for pair in pairs {
            if let Some(pp) = self.price_cache.get(&pair.pair_id) {
                // Skip pairs with no price data yet
                if pp.poly_yes == Decimal::ZERO || pp.kalshi_yes == Decimal::ZERO {
                    continue;
                }
                let spread = pp.poly_yes - pp.kalshi_yes;
                let snapshot = NewPriceSnapshot {
                    pair_id: pair.pair_id,
                    poly_yes_price: pp.poly_yes,
                    kalshi_yes_price: pp.kalshi_yes,
                    spread,
                    captured_at: Utc::now(),
                };
                if let Err(e) = self.db.insert_price_snapshot(&snapshot).await {
                    error!(pair = %pair.pair_id, err = %e, "failed to insert price snapshot");
                }
            }
        }
    }

    /// Aggregate daily P&L from filled orders and open positions.
    async fn aggregate_daily_pnl(&self) {
        let today = Utc::now().format("%Y-%m-%d").to_string();

        // Count executed orders (all orders placed today)
        let mut all_orders_count = 0i64;
        for status in &["open", "pending", "partial_fill", "filled", "cancelled", "failed"] {
            if let Ok(orders) = self.db.list_orders_by_status(status).await {
                all_orders_count += orders.len() as i64;
            }
        }
        let filled_orders = self.db.list_orders_by_status("filled").await.unwrap_or_default();
        let trades_executed = all_orders_count;
        let trades_filled = filled_orders.len() as i64;

        // Sum profit from open positions filtered by current mode
        let positions = self.db.list_positions_by_mode(&self.mode).await.unwrap_or_default();

        // guaranteed_profit already has fees deducted (net profit per position)
        let net_profit_from_positions: Decimal = positions.iter()
            .map(|p| p.guaranteed_profit)
            .sum();

        // Compute total fees from positions
        let total_fees: Decimal = positions.iter()
            .map(|p| {
                let (_k, _p, fee) = self.fee_config.compute_fees(
                    p.kalshi_avg_price,
                    p.poly_avg_price,
                    p.hedged_quantity as u32,
                );
                fee
            })
            .sum();

        let gross_profit = net_profit_from_positions + total_fees;

        // Capital deployed = sum of all position costs
        let capital_deployed: Decimal = positions.iter()
            .map(|p| {
                p.poly_avg_price * Decimal::from(p.poly_quantity)
                    + p.kalshi_avg_price * Decimal::from(p.kalshi_quantity)
            })
            .sum();

        let pnl = DailyPnlRow {
            date: today,
            mode: self.mode.clone(),
            trades_executed,
            trades_filled,
            gross_profit,
            fees_paid: total_fees,
            net_profit: net_profit_from_positions,
            capital_deployed,
        };

        if let Err(e) = self.db.upsert_daily_pnl(&pnl).await {
            error!(err = %e, "failed to upsert daily P&L");
        }
    }

    pub async fn shutdown(&self) {
        info!("shutting down - cancelling open orders");
        let _ = self.shutdown_tx.send(());
        for conn in [&self.poly, &self.kalshi] {
            if let Ok(orders) = conn.list_open_orders().await {
                for o in &orders {
                    let _ = conn.cancel_order(&o.order_id).await;
                }
                info!(platform = %conn.platform(), cancelled = orders.len());
            }
        }
        info!("shutdown complete");
    }
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "mock")]
    mod with_mocks {
        use crate::engine::*;
        use crate::executor::Executor;
        use crate::fees::FeeConfig;
        use crate::tracker::Tracker;
        use crate::types::OrderConfig;
        use arb_db::SqliteRepository;
        use arb_kalshi::mock::MockState as KalshiState;
        use arb_kalshi::MockKalshiConnector;
        use arb_polymarket::mock::MockState as PolyState;
        use arb_polymarket::MockPolymarketConnector;
        use arb_risk::{RiskConfig, RiskManager};
        use arb_types::order::{OrderBookLevel, Side};
        use arb_types::*;
        use chrono::{Duration as CDur, Utc};
        use parking_lot::RwLock;
        use rust_decimal_macros::dec;

        fn order_config() -> OrderConfig {
            OrderConfig {
                max_order_age_secs: 2,
                max_hedge_wait_secs: 3,
                order_check_interval_ms: 100,
                min_repost_spread: dec!(0.02),
                price_improve_amount: dec!(0.01),
                default_quantity: 50,
            }
        }

        #[allow(dead_code)]
        fn engine_config() -> EngineConfig {
            EngineConfig {
                scan_interval_ms: 100,
                min_spread_pct: dec!(3.0),
                min_spread_absolute: dec!(0.02),
            }
        }

        #[tokio::test]
        async fn test_executor_both_legs_succeed() {
            let db = Arc::new(SqliteRepository::new("sqlite::memory:").await.unwrap());
            db.run_migrations().await.unwrap();

            let poly_state = Arc::new(parking_lot::Mutex::new(PolyState::default()));
            poly_state.lock().balance = dec!(5000);
            poly_state.lock().order_books.insert(
                "poly-tok".into(),
                OrderBook {
                    market_id: "poly-tok".into(),
                    bids: vec![OrderBookLevel {
                        price: dec!(0.40),
                        quantity: 100,
                    }],
                    asks: vec![OrderBookLevel {
                        price: dec!(0.42),
                        quantity: 100,
                    }],
                    timestamp: Utc::now(),
                },
            );
            let poly: Arc<dyn PredictionMarketConnector> =
                Arc::new(MockPolymarketConnector::with_state(poly_state.clone()));

            let kalshi_state = Arc::new(parking_lot::Mutex::new(KalshiState::default()));
            kalshi_state.lock().balance = dec!(5000);
            let kalshi: Arc<dyn PredictionMarketConnector> =
                Arc::new(MockKalshiConnector::with_state(kalshi_state.clone()));

            let rm = Arc::new(RwLock::new(RiskManager::new(RiskConfig::default())));
            rm.write().set_engine_running(true);

            let executor = Executor::new(poly, kalshi, rm, db, order_config(), "paper".into());

            let mut opp = Opportunity {
                id: Uuid::now_v7(),
                pair_id: Uuid::now_v7(),
                poly_side: Side::Yes,
                poly_price: dec!(0.42),
                poly_market_id: "poly-tok".into(),
                poly_yes_token_id: "poly-tok".into(),
                kalshi_side: Side::No,
                kalshi_price: dec!(0.53),
                kalshi_market_id: "kalshi-t".into(),
                spread: dec!(0.05),
                spread_pct: dec!(5.26),
                max_quantity: 0,
                close_time: Utc::now() + CDur::days(30),
                detected_at: Utc::now(),
                status: OpportunityStatus::Detected,
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
            poly_state.lock().order_books.insert(
                "poly-tok".into(),
                OrderBook {
                    market_id: "poly-tok".into(),
                    bids: vec![OrderBookLevel { price: dec!(0.40), quantity: 100 }],
                    asks: vec![OrderBookLevel { price: dec!(0.42), quantity: 100 }],
                    timestamp: Utc::now(),
                },
            );
            let poly: Arc<dyn PredictionMarketConnector> =
                Arc::new(MockPolymarketConnector::with_state(poly_state.clone()));

            // The Kalshi mock's should_fail is consumed by the first method call.
            // The executor calls kalshi.get_balance() during the risk check BEFORE
            // place_limit_order, which would consume a single failure. To work around
            // this, we manually perform the balance + risk check steps outside the
            // executor and then test the dual-leg placement directly.
            //
            // Instead, we use a custom connector wrapper that only fails on place_limit_order.
            let kalshi_state = Arc::new(parking_lot::Mutex::new(KalshiState::default()));
            kalshi_state.lock().balance = dec!(5000);

            // Create a connector that wraps the mock but overrides place_limit_order to fail
            struct FailOnPlaceKalshi {
                inner: MockKalshiConnector,
            }

            #[async_trait::async_trait]
            impl PredictionMarketConnector for FailOnPlaceKalshi {
                fn platform(&self) -> arb_types::Platform { self.inner.platform() }
                async fn list_markets(&self, status: arb_types::MarketStatus) -> Result<Vec<arb_types::Market>, arb_types::ArbError> { self.inner.list_markets(status).await }
                async fn get_market(&self, id: &str) -> Result<arb_types::Market, arb_types::ArbError> { self.inner.get_market(id).await }
                async fn get_order_book(&self, id: &str) -> Result<OrderBook, arb_types::ArbError> { self.inner.get_order_book(id).await }
                async fn subscribe_prices(&self, ids: &[String], tx: tokio::sync::mpsc::Sender<arb_types::PriceUpdate>) -> Result<arb_types::SubHandle, arb_types::ArbError> { self.inner.subscribe_prices(ids, tx).await }
                async fn place_limit_order(&self, _req: &arb_types::LimitOrderRequest) -> Result<OrderResponse, arb_types::ArbError> {
                    Err(arb_types::ArbError::PlatformError {
                        platform: arb_types::Platform::Kalshi,
                        message: "connection refused".into(),
                    })
                }
                async fn cancel_order(&self, order_id: &str) -> Result<(), arb_types::ArbError> { self.inner.cancel_order(order_id).await }
                async fn get_order(&self, order_id: &str) -> Result<OrderResponse, arb_types::ArbError> { self.inner.get_order(order_id).await }
                async fn list_open_orders(&self) -> Result<Vec<OrderResponse>, arb_types::ArbError> { self.inner.list_open_orders().await }
                async fn get_balance(&self) -> Result<rust_decimal::Decimal, arb_types::ArbError> { self.inner.get_balance().await }
                async fn get_positions(&self) -> Result<Vec<arb_types::PlatformPosition>, arb_types::ArbError> { self.inner.get_positions().await }
            }

            let kalshi: Arc<dyn PredictionMarketConnector> = Arc::new(FailOnPlaceKalshi {
                inner: MockKalshiConnector::with_state(kalshi_state.clone()),
            });

            let rm = Arc::new(RwLock::new(RiskManager::new(RiskConfig::default())));
            rm.write().set_engine_running(true);

            let executor = Executor::new(poly, kalshi, rm, db, order_config(), "paper".into());

            let mut opp = Opportunity {
                id: Uuid::now_v7(),
                pair_id: Uuid::now_v7(),
                poly_side: Side::Yes,
                poly_price: dec!(0.42),
                poly_market_id: "poly-tok".into(),
                poly_yes_token_id: "poly-tok".into(),
                kalshi_side: Side::No,
                kalshi_price: dec!(0.53),
                kalshi_market_id: "kalshi-t".into(),
                spread: dec!(0.05),
                spread_pct: dec!(5.26),
                max_quantity: 0,
                close_time: Utc::now() + CDur::days(30),
                detected_at: Utc::now(),
                status: OpportunityStatus::Detected,
            };

            let result = executor.execute(&mut opp).await;
            assert!(result.is_err());
            assert_eq!(poly_state.lock().placed_orders.len(), 1);
            assert_eq!(poly_state.lock().cancelled_orders.len(), 1);
        }

        #[tokio::test]
        async fn test_tracker_creates_position() {
            let db = Arc::new(SqliteRepository::new("sqlite::memory:").await.unwrap());
            db.run_migrations().await.unwrap();
            let rm = Arc::new(RwLock::new(RiskManager::new(RiskConfig::default())));
            rm.write().set_engine_running(true);

            // Use zero fees so the tracker test's profit assertion stays unchanged
            let fee_config = FeeConfig {
                kalshi_taker_fee_pct: dec!(0),
                poly_taker_fee_pct: dec!(0),
            };
            let tracker = Tracker::new(db, rm.clone(), "paper".into(), fee_config);

            let opp = Opportunity {
                id: Uuid::now_v7(),
                pair_id: Uuid::now_v7(),
                poly_side: Side::Yes,
                poly_price: dec!(0.42),
                poly_market_id: "p".into(),
                poly_yes_token_id: "p".into(),
                kalshi_side: Side::No,
                kalshi_price: dec!(0.53),
                kalshi_market_id: "k".into(),
                spread: dec!(0.05),
                spread_pct: dec!(5.26),
                max_quantity: 50,
                close_time: Utc::now() + CDur::days(30),
                detected_at: Utc::now(),
                status: OpportunityStatus::Executing,
            };

            let poly_order = OrderResponse {
                order_id: "p1".into(),
                status: OrderStatus::Filled,
                filled_quantity: 50,
                price: dec!(0.42),
                side: Side::Yes,
                market_id: "p".into(),
            };
            let kalshi_order = OrderResponse {
                order_id: "k1".into(),
                status: OrderStatus::Filled,
                filled_quantity: 50,
                price: dec!(0.53),
                side: Side::No,
                market_id: "k".into(),
            };

            let pos = tracker
                .create_position(&opp, &poly_order, &kalshi_order)
                .await
                .unwrap();
            assert_eq!(pos.hedged_quantity, 50);
            assert_eq!(pos.guaranteed_profit, dec!(2.50));
            assert_eq!(rm.read().exposure().total_exposure(), dec!(47.50));
        }
    }
}
