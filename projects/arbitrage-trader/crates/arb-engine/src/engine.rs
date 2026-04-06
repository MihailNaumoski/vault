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
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        poly: Arc<dyn PredictionMarketConnector>,
        kalshi: Arc<dyn PredictionMarketConnector>,
        price_cache: Arc<PriceCache>,
        executor: Executor,
        monitor: Monitor,
        tracker: Tracker,
        unwinder: Unwinder,
        config: EngineConfig,
    ) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        let detector = Detector::new(price_cache.clone(), &config);
        Self {
            poly,
            kalshi,
            price_cache,
            detector,
            executor: Arc::new(executor),
            monitor: Arc::new(monitor),
            tracker: Arc::new(tracker),
            unwinder: Arc::new(unwinder),
            config,
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
                        if let Err(e) = self
                            .unwinder
                            .unwind(filled_platform, &filled_order, &unfilled_order_id)
                            .await
                        {
                            error!(opp = %opp.id, err = %e, "unwind failed");
                        }
                    }
                    Ok(MonitorAction::BothCancelled) => {
                        info!(opp = %opp.id, "both expired");
                    }
                    Ok(_) => {}
                    Err(e) => error!(opp = %opp.id, err = %e, "monitor error"),
                }
            }
            Err(e) => warn!(opp = %opp.id, err = %e, "execution failed"),
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

            let executor = Executor::new(poly, kalshi, rm, db, order_config());

            let mut opp = Opportunity {
                id: Uuid::now_v7(),
                pair_id: Uuid::now_v7(),
                poly_side: Side::Yes,
                poly_price: dec!(0.42),
                poly_market_id: "poly-tok".into(),
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

            let executor = Executor::new(poly, kalshi, rm, db, order_config());

            let mut opp = Opportunity {
                id: Uuid::now_v7(),
                pair_id: Uuid::now_v7(),
                poly_side: Side::Yes,
                poly_price: dec!(0.42),
                poly_market_id: "poly-tok".into(),
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

            let tracker = Tracker::new(db, rm.clone());

            let opp = Opportunity {
                id: Uuid::now_v7(),
                pair_id: Uuid::now_v7(),
                poly_side: Side::Yes,
                poly_price: dec!(0.42),
                poly_market_id: "p".into(),
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
