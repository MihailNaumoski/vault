#[cfg(feature = "mock")]
pub use mock_impl::*;

#[cfg(feature = "mock")]
mod mock_impl {
    use arb_types::*;
    use async_trait::async_trait;
    use parking_lot::Mutex;
    use rust_decimal::Decimal;
    use std::collections::HashMap;
    use std::sync::Arc;

    /// Shared mutable state for the mock connector.
    ///
    /// Tests inject data into this struct and verify calls after the fact.
    #[derive(Debug, Default)]
    pub struct MockState {
        /// Markets to return from `list_markets` / `get_market`.
        pub markets: Vec<Market>,
        /// Order books keyed by market_id.
        pub order_books: HashMap<String, OrderBook>,
        /// Orders to return from `list_open_orders` / `get_order`.
        pub orders: Vec<OrderResponse>,
        /// Positions to return from `get_positions`.
        pub positions: Vec<PlatformPosition>,
        /// Balance to return from `get_balance`.
        pub balance: Decimal,
        /// Records all orders placed via `place_limit_order`.
        pub placed_orders: Vec<LimitOrderRequest>,
        /// Records all order IDs passed to `cancel_order`.
        pub cancelled_orders: Vec<String>,
        /// If set, the next call will return this error (then clears it).
        pub should_fail: Option<String>,
        /// Price updates queued for `subscribe_prices`.
        pub price_updates: Vec<PriceUpdate>,
    }

    impl MockState {
        /// Set a failure that will trigger on the next call.
        pub fn inject_failure(&mut self, message: &str) {
            self.should_fail = Some(message.to_string());
        }

        /// Take the pending failure, if any.
        fn take_failure(&mut self) -> Option<ArbError> {
            self.should_fail.take().map(|msg| ArbError::PlatformError {
                platform: Platform::Kalshi,
                message: msg,
            })
        }
    }

    /// Mock implementation of the Kalshi connector for testing.
    ///
    /// All data is read from / written to `MockState` via `Arc<Mutex<>>`.
    pub struct MockKalshiConnector {
        pub state: Arc<Mutex<MockState>>,
    }

    impl MockKalshiConnector {
        /// Create a new mock connector with default (empty) state.
        pub fn new() -> Self {
            Self {
                state: Arc::new(Mutex::new(MockState::default())),
            }
        }

        /// Create a new mock connector with shared state for test assertions.
        pub fn with_state(state: Arc<Mutex<MockState>>) -> Self {
            Self { state }
        }
    }

    impl Default for MockKalshiConnector {
        fn default() -> Self {
            Self::new()
        }
    }

    #[async_trait]
    impl PredictionMarketConnector for MockKalshiConnector {
        fn platform(&self) -> Platform {
            Platform::Kalshi
        }

        async fn list_markets(&self, status: MarketStatus) -> Result<Vec<Market>, ArbError> {
            let mut state = self.state.lock();
            if let Some(err) = state.take_failure() {
                return Err(err);
            }
            Ok(state
                .markets
                .iter()
                .filter(|m| m.status == status)
                .cloned()
                .collect())
        }

        async fn get_market(&self, id: &str) -> Result<Market, ArbError> {
            let mut state = self.state.lock();
            if let Some(err) = state.take_failure() {
                return Err(err);
            }
            state
                .markets
                .iter()
                .find(|m| m.platform_id == id)
                .cloned()
                .ok_or_else(|| ArbError::MarketNotFound(id.to_string()))
        }

        async fn get_order_book(&self, id: &str) -> Result<OrderBook, ArbError> {
            let mut state = self.state.lock();
            if let Some(err) = state.take_failure() {
                return Err(err);
            }
            state
                .order_books
                .get(id)
                .cloned()
                .ok_or_else(|| ArbError::MarketNotFound(id.to_string()))
        }

        async fn subscribe_prices(
            &self,
            _ids: &[String],
            tx: tokio::sync::mpsc::Sender<PriceUpdate>,
        ) -> Result<SubHandle, ArbError> {
            let updates = {
                let mut state = self.state.lock();
                if let Some(err) = state.take_failure() {
                    return Err(err);
                }
                std::mem::take(&mut state.price_updates)
            };

            let (cancel_tx, mut cancel_rx) = tokio::sync::oneshot::channel::<()>();

            tokio::spawn(async move {
                for update in updates {
                    tokio::select! {
                        result = tx.send(update) => {
                            if result.is_err() {
                                return; // receiver dropped
                            }
                        }
                        _ = &mut cancel_rx => {
                            return; // cancelled
                        }
                    }
                }
                // Hold the channel open until cancelled
                let _ = cancel_rx.await;
            });

            Ok(SubHandle { cancel_tx })
        }

        async fn place_limit_order(
            &self,
            req: &LimitOrderRequest,
        ) -> Result<OrderResponse, ArbError> {
            let mut state = self.state.lock();
            if let Some(err) = state.take_failure() {
                return Err(err);
            }
            state.placed_orders.push(req.clone());

            Ok(OrderResponse {
                order_id: uuid::Uuid::now_v7().to_string(),
                status: OrderStatus::Open,
                filled_quantity: 0,
                price: req.price,
                side: req.side,
                market_id: req.market_id.clone(),
            })
        }

        async fn cancel_order(&self, order_id: &str) -> Result<(), ArbError> {
            let mut state = self.state.lock();
            if let Some(err) = state.take_failure() {
                return Err(err);
            }
            state.cancelled_orders.push(order_id.to_string());
            Ok(())
        }

        async fn get_order(&self, order_id: &str) -> Result<OrderResponse, ArbError> {
            let mut state = self.state.lock();
            if let Some(err) = state.take_failure() {
                return Err(err);
            }
            state
                .orders
                .iter()
                .find(|o| o.order_id == order_id)
                .cloned()
                .ok_or_else(|| ArbError::MarketNotFound(format!("order {order_id}")))
        }

        async fn list_open_orders(&self) -> Result<Vec<OrderResponse>, ArbError> {
            let mut state = self.state.lock();
            if let Some(err) = state.take_failure() {
                return Err(err);
            }
            Ok(state
                .orders
                .iter()
                .filter(|o| o.status == OrderStatus::Open)
                .cloned()
                .collect())
        }

        async fn get_balance(&self) -> Result<Decimal, ArbError> {
            let mut state = self.state.lock();
            if let Some(err) = state.take_failure() {
                return Err(err);
            }
            Ok(state.balance)
        }

        async fn get_positions(&self) -> Result<Vec<PlatformPosition>, ArbError> {
            let mut state = self.state.lock();
            if let Some(err) = state.take_failure() {
                return Err(err);
            }
            Ok(state.positions.clone())
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use chrono::Utc;
        use rust_decimal_macros::dec;

        fn make_test_market(platform_id: &str, status: MarketStatus) -> Market {
            Market {
                id: MarketId::new(),
                platform: Platform::Kalshi,
                platform_id: platform_id.to_string(),
                question: format!("Test market {platform_id}"),
                yes_price: dec!(0.55),
                no_price: dec!(0.45),
                volume: dec!(1000),
                liquidity: dec!(500),
                status,
                close_time: Utc::now(),
                updated_at: Utc::now(),
            }
        }

        #[tokio::test]
        async fn test_mock_list_markets() {
            let state = Arc::new(Mutex::new(MockState::default()));
            {
                let mut s = state.lock();
                s.markets.push(make_test_market("PRES-2026-DEM", MarketStatus::Open));
                s.markets.push(make_test_market("PRES-2026-REP", MarketStatus::Open));
                s.markets.push(make_test_market("CLOSED-MKT", MarketStatus::Closed));
            }
            let connector = MockKalshiConnector::with_state(state);

            let open = connector.list_markets(MarketStatus::Open).await.unwrap();
            assert_eq!(open.len(), 2);

            let closed = connector.list_markets(MarketStatus::Closed).await.unwrap();
            assert_eq!(closed.len(), 1);
        }

        #[tokio::test]
        async fn test_mock_get_market() {
            let state = Arc::new(Mutex::new(MockState::default()));
            {
                let mut s = state.lock();
                s.markets.push(make_test_market("PRES-2026-DEM", MarketStatus::Open));
            }
            let connector = MockKalshiConnector::with_state(state);

            let market = connector.get_market("PRES-2026-DEM").await.unwrap();
            assert_eq!(market.platform_id, "PRES-2026-DEM");

            let err = connector.get_market("NONEXISTENT").await;
            assert!(err.is_err());
        }

        #[tokio::test]
        async fn test_mock_place_order() {
            let state = Arc::new(Mutex::new(MockState::default()));
            let connector = MockKalshiConnector::with_state(state.clone());

            let req = LimitOrderRequest {
                market_id: "PRES-2026-DEM".to_string(),
                side: Side::Yes,
                price: dec!(0.42),
                quantity: 10,
            };
            let resp = connector.place_limit_order(&req).await.unwrap();
            assert_eq!(resp.status, OrderStatus::Open);
            assert_eq!(resp.price, dec!(0.42));
            assert_eq!(resp.side, Side::Yes);

            // Verify it was recorded
            let s = state.lock();
            assert_eq!(s.placed_orders.len(), 1);
            assert_eq!(s.placed_orders[0].market_id, "PRES-2026-DEM");
            assert_eq!(s.placed_orders[0].quantity, 10);
        }

        #[tokio::test]
        async fn test_mock_cancel_order() {
            let state = Arc::new(Mutex::new(MockState::default()));
            let connector = MockKalshiConnector::with_state(state.clone());

            connector.cancel_order("order-123").await.unwrap();
            connector.cancel_order("order-456").await.unwrap();

            let s = state.lock();
            assert_eq!(s.cancelled_orders.len(), 2);
            assert_eq!(s.cancelled_orders[0], "order-123");
            assert_eq!(s.cancelled_orders[1], "order-456");
        }

        #[tokio::test]
        async fn test_mock_failure_injection() {
            let state = Arc::new(Mutex::new(MockState::default()));
            {
                let mut s = state.lock();
                s.inject_failure("test error");
            }
            let connector = MockKalshiConnector::with_state(state.clone());

            let result = connector.list_markets(MarketStatus::Open).await;
            assert!(result.is_err());
            let err = result.unwrap_err();
            match err {
                ArbError::PlatformError { platform, message } => {
                    assert_eq!(platform, Platform::Kalshi);
                    assert_eq!(message, "test error");
                }
                _ => panic!("expected PlatformError, got: {err:?}"),
            }

            // After failure is consumed, next call should succeed
            let result = connector.list_markets(MarketStatus::Open).await;
            assert!(result.is_ok());
        }

        #[tokio::test]
        async fn test_mock_subscribe_prices() {
            let state = Arc::new(Mutex::new(MockState::default()));
            {
                let mut s = state.lock();
                s.price_updates.push(PriceUpdate {
                    platform: Platform::Kalshi,
                    market_id: "PRES-2026-DEM".to_string(),
                    yes_price: dec!(0.55),
                    no_price: dec!(0.45),
                    timestamp: Utc::now(),
                });
                s.price_updates.push(PriceUpdate {
                    platform: Platform::Kalshi,
                    market_id: "PRES-2026-DEM".to_string(),
                    yes_price: dec!(0.56),
                    no_price: dec!(0.44),
                    timestamp: Utc::now(),
                });
            }
            let connector = MockKalshiConnector::with_state(state);

            let (tx, mut rx) = tokio::sync::mpsc::channel(10);
            let handle = connector
                .subscribe_prices(&["PRES-2026-DEM".to_string()], tx)
                .await
                .unwrap();

            // Should receive both updates
            let update1 = rx.recv().await.unwrap();
            assert_eq!(update1.yes_price, dec!(0.55));

            let update2 = rx.recv().await.unwrap();
            assert_eq!(update2.yes_price, dec!(0.56));

            // Cancel the subscription
            handle.cancel();
        }

        #[tokio::test]
        async fn test_mock_get_balance() {
            let state = Arc::new(Mutex::new(MockState::default()));
            {
                let mut s = state.lock();
                s.balance = dec!(1500.00);
            }
            let connector = MockKalshiConnector::with_state(state);

            let balance = connector.get_balance().await.unwrap();
            assert_eq!(balance, dec!(1500.00));
        }

        #[tokio::test]
        async fn test_mock_get_positions() {
            let state = Arc::new(Mutex::new(MockState::default()));
            {
                let mut s = state.lock();
                s.positions.push(PlatformPosition {
                    market_id: "PRES-2026-DEM".to_string(),
                    side: Side::Yes,
                    quantity: 10,
                    avg_price: dec!(0.42),
                });
            }
            let connector = MockKalshiConnector::with_state(state);

            let positions = connector.get_positions().await.unwrap();
            assert_eq!(positions.len(), 1);
            assert_eq!(positions[0].market_id, "PRES-2026-DEM");
            assert_eq!(positions[0].quantity, 10);
        }

        #[tokio::test]
        async fn test_mock_list_open_orders() {
            let state = Arc::new(Mutex::new(MockState::default()));
            {
                let mut s = state.lock();
                s.orders.push(OrderResponse {
                    order_id: "order-1".to_string(),
                    status: OrderStatus::Open,
                    filled_quantity: 0,
                    price: dec!(0.42),
                    side: Side::Yes,
                    market_id: "PRES-2026-DEM".to_string(),
                });
                s.orders.push(OrderResponse {
                    order_id: "order-2".to_string(),
                    status: OrderStatus::Filled,
                    filled_quantity: 10,
                    price: dec!(0.55),
                    side: Side::No,
                    market_id: "PRES-2026-DEM".to_string(),
                });
            }
            let connector = MockKalshiConnector::with_state(state);

            // list_open_orders should only return Open orders
            let open = connector.list_open_orders().await.unwrap();
            assert_eq!(open.len(), 1);
            assert_eq!(open[0].order_id, "order-1");
        }

        #[tokio::test]
        async fn test_mock_trait_object() {
            // Verify that MockKalshiConnector can be used as Box<dyn PredictionMarketConnector>
            let connector = MockKalshiConnector::new();
            let boxed: Box<dyn PredictionMarketConnector> = Box::new(connector);
            assert_eq!(boxed.platform(), Platform::Kalshi);

            let markets = boxed.list_markets(MarketStatus::Open).await.unwrap();
            assert!(markets.is_empty());
        }
    }
}
