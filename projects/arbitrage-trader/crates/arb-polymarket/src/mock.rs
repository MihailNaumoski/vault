#[cfg(feature = "mock")]
pub use inner::*;

#[cfg(feature = "mock")]
mod inner {
    use std::collections::HashMap;
    use std::sync::Arc;

    use async_trait::async_trait;
    use parking_lot::Mutex;
    use rust_decimal::Decimal;
    use tokio::sync::mpsc;

    use arb_types::*;

    /// Mutable state backing the mock connector for test assertions.
    #[derive(Debug)]
    pub struct MockState {
        pub markets: Vec<Market>,
        pub order_books: HashMap<String, OrderBook>,
        pub orders: Vec<OrderResponse>,
        pub positions: Vec<PlatformPosition>,
        pub balance: Decimal,
        /// Records all order placement requests.
        pub placed_orders: Vec<LimitOrderRequest>,
        /// Records all cancelled order IDs.
        pub cancelled_orders: Vec<String>,
        /// When set, the next call returns this error (then resets).
        pub should_fail: Option<ArbError>,
        /// Price updates to send when `subscribe_prices` is called.
        pub price_updates: Vec<PriceUpdate>,
        /// Auto-incrementing order ID counter.
        next_order_id: u64,
    }

    impl Default for MockState {
        fn default() -> Self {
            Self {
                markets: Vec::new(),
                order_books: HashMap::new(),
                orders: Vec::new(),
                positions: Vec::new(),
                balance: Decimal::ZERO,
                placed_orders: Vec::new(),
                cancelled_orders: Vec::new(),
                should_fail: None,
                price_updates: Vec::new(),
                next_order_id: 1,
            }
        }
    }

    impl MockState {
        fn maybe_fail(&mut self) -> Result<(), ArbError> {
            if let Some(err) = self.should_fail.take() {
                Err(err)
            } else {
                Ok(())
            }
        }
    }

    /// A mock Polymarket connector for testing.
    pub struct MockPolymarketConnector {
        pub state: Arc<Mutex<MockState>>,
    }

    impl MockPolymarketConnector {
        /// Create a new mock connector with default (empty) state.
        pub fn new() -> Self {
            Self {
                state: Arc::new(Mutex::new(MockState::default())),
            }
        }

        /// Create a mock connector sharing the given state.
        pub fn with_state(state: Arc<Mutex<MockState>>) -> Self {
            Self { state }
        }
    }

    impl Default for MockPolymarketConnector {
        fn default() -> Self {
            Self::new()
        }
    }

    #[async_trait]
    impl PredictionMarketConnector for MockPolymarketConnector {
        fn platform(&self) -> Platform {
            Platform::Polymarket
        }

        async fn list_markets(&self, status: MarketStatus) -> Result<Vec<Market>, ArbError> {
            let mut state = self.state.lock();
            state.maybe_fail()?;
            Ok(state
                .markets
                .iter()
                .filter(|m| m.status == status)
                .cloned()
                .collect())
        }

        async fn get_market(&self, id: &str) -> Result<Market, ArbError> {
            let mut state = self.state.lock();
            state.maybe_fail()?;
            state
                .markets
                .iter()
                .find(|m| m.platform_id == id)
                .cloned()
                .ok_or_else(|| ArbError::MarketNotFound(id.to_string()))
        }

        async fn get_order_book(&self, id: &str) -> Result<OrderBook, ArbError> {
            let mut state = self.state.lock();
            state.maybe_fail()?;
            state
                .order_books
                .get(id)
                .cloned()
                .ok_or_else(|| ArbError::MarketNotFound(id.to_string()))
        }

        async fn subscribe_prices(
            &self,
            _ids: &[String],
            tx: mpsc::Sender<PriceUpdate>,
        ) -> Result<SubHandle, ArbError> {
            let updates = {
                let mut state = self.state.lock();
                state.maybe_fail()?;
                std::mem::take(&mut state.price_updates)
            };

            let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel::<()>();

            tokio::spawn(async move {
                for update in updates {
                    if tx.send(update).await.is_err() {
                        return;
                    }
                }
                // Hold open until cancelled
                let _ = cancel_rx.await;
            });

            Ok(SubHandle { cancel_tx })
        }

        async fn place_limit_order(
            &self,
            req: &LimitOrderRequest,
        ) -> Result<OrderResponse, ArbError> {
            let mut state = self.state.lock();
            state.maybe_fail()?;
            state.placed_orders.push(req.clone());

            let order_id = format!("mock-ord-{}", state.next_order_id);
            state.next_order_id += 1;

            let response = OrderResponse {
                order_id,
                status: OrderStatus::Open,
                filled_quantity: 0,
                price: req.price,
                side: req.side,
                market_id: req.market_id.clone(),
            };
            state.orders.push(response.clone());
            Ok(response)
        }

        async fn cancel_order(&self, order_id: &str) -> Result<(), ArbError> {
            let mut state = self.state.lock();
            state.maybe_fail()?;
            state.cancelled_orders.push(order_id.to_string());

            // Update order status if it exists
            for order in &mut state.orders {
                if order.order_id == order_id {
                    order.status = OrderStatus::Cancelled;
                }
            }
            Ok(())
        }

        async fn get_order(&self, order_id: &str) -> Result<OrderResponse, ArbError> {
            let mut state = self.state.lock();
            state.maybe_fail()?;
            state
                .orders
                .iter()
                .find(|o| o.order_id == order_id)
                .cloned()
                .ok_or_else(|| ArbError::Other(format!("order not found: {order_id}")))
        }

        async fn list_open_orders(&self) -> Result<Vec<OrderResponse>, ArbError> {
            let mut state = self.state.lock();
            state.maybe_fail()?;
            Ok(state
                .orders
                .iter()
                .filter(|o| o.status == OrderStatus::Open)
                .cloned()
                .collect())
        }

        async fn get_balance(&self) -> Result<Decimal, ArbError> {
            let mut state = self.state.lock();
            state.maybe_fail()?;
            Ok(state.balance)
        }

        async fn get_positions(&self) -> Result<Vec<PlatformPosition>, ArbError> {
            let mut state = self.state.lock();
            state.maybe_fail()?;
            Ok(state.positions.clone())
        }
    }
}

#[cfg(all(test, feature = "mock"))]
mod tests {
    use super::inner::*;
    use arb_types::*;
    use chrono::Utc;
    use rust_decimal_macros::dec;
    use std::sync::Arc;

    fn make_market(platform_id: &str, status: MarketStatus) -> Market {
        Market {
            id: MarketId::new(),
            platform: Platform::Polymarket,
            platform_id: platform_id.to_string(),
            question: format!("Market {platform_id}"),
            yes_price: dec!(0.60),
            no_price: dec!(0.40),
            volume: dec!(10000),
            liquidity: dec!(5000),
            status,
            close_time: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn test_mock_list_markets() {
        let state = Arc::new(parking_lot::Mutex::new(MockState::default()));
        {
            let mut s = state.lock();
            s.markets.push(make_market("m1", MarketStatus::Open));
            s.markets.push(make_market("m2", MarketStatus::Closed));
            s.markets.push(make_market("m3", MarketStatus::Open));
        }

        let conn = MockPolymarketConnector::with_state(state);
        let open = conn.list_markets(MarketStatus::Open).await.unwrap();
        assert_eq!(open.len(), 2);

        let closed = conn.list_markets(MarketStatus::Closed).await.unwrap();
        assert_eq!(closed.len(), 1);
    }

    #[tokio::test]
    async fn test_mock_place_order() {
        let state = Arc::new(parking_lot::Mutex::new(MockState::default()));
        let conn = MockPolymarketConnector::with_state(state.clone());

        let req = LimitOrderRequest {
            market_id: "tok-1".to_string(),
            side: Side::Yes,
            price: dec!(0.55),
            quantity: 100,
        };

        let resp = conn.place_limit_order(&req).await.unwrap();
        assert_eq!(resp.status, OrderStatus::Open);
        assert_eq!(resp.price, dec!(0.55));

        let s = state.lock();
        assert_eq!(s.placed_orders.len(), 1);
        assert_eq!(s.placed_orders[0].market_id, "tok-1");
    }

    #[tokio::test]
    async fn test_mock_cancel_order() {
        let state = Arc::new(parking_lot::Mutex::new(MockState::default()));
        let conn = MockPolymarketConnector::with_state(state.clone());

        // Place then cancel
        let req = LimitOrderRequest {
            market_id: "tok-1".to_string(),
            side: Side::Yes,
            price: dec!(0.55),
            quantity: 100,
        };
        let resp = conn.place_limit_order(&req).await.unwrap();
        conn.cancel_order(&resp.order_id).await.unwrap();

        let s = state.lock();
        assert_eq!(s.cancelled_orders.len(), 1);
        assert_eq!(s.cancelled_orders[0], resp.order_id);
    }

    #[tokio::test]
    async fn test_mock_failure_injection() {
        let state = Arc::new(parking_lot::Mutex::new(MockState::default()));
        {
            let mut s = state.lock();
            s.should_fail = Some(ArbError::PlatformError {
                platform: Platform::Polymarket,
                message: "injected failure".to_string(),
            });
        }

        let conn = MockPolymarketConnector::with_state(state.clone());
        let result = conn.list_markets(MarketStatus::Open).await;
        assert!(result.is_err());

        // After failure, should_fail is consumed
        let result2 = conn.list_markets(MarketStatus::Open).await;
        assert!(result2.is_ok());
    }

    #[tokio::test]
    async fn test_mock_subscribe_prices() {
        let state = Arc::new(parking_lot::Mutex::new(MockState::default()));
        {
            let mut s = state.lock();
            s.price_updates.push(PriceUpdate {
                platform: Platform::Polymarket,
                market_id: "tok-1".to_string(),
                yes_price: dec!(0.65),
                no_price: dec!(0.35),
                timestamp: Utc::now(),
            });
            s.price_updates.push(PriceUpdate {
                platform: Platform::Polymarket,
                market_id: "tok-2".to_string(),
                yes_price: dec!(0.70),
                no_price: dec!(0.30),
                timestamp: Utc::now(),
            });
        }

        let conn = MockPolymarketConnector::with_state(state);
        let (tx, mut rx) = tokio::sync::mpsc::channel(10);
        let handle = conn
            .subscribe_prices(&["tok-1".to_string(), "tok-2".to_string()], tx)
            .await
            .unwrap();

        // Should receive both updates
        let u1 = rx.recv().await.unwrap();
        assert_eq!(u1.market_id, "tok-1");

        let u2 = rx.recv().await.unwrap();
        assert_eq!(u2.market_id, "tok-2");

        // Cancel the subscription
        handle.cancel();
    }

    #[tokio::test]
    async fn test_mock_get_balance() {
        let state = Arc::new(parking_lot::Mutex::new(MockState::default()));
        {
            let mut s = state.lock();
            s.balance = dec!(5000.50);
        }

        let conn = MockPolymarketConnector::with_state(state);
        let balance = conn.get_balance().await.unwrap();
        assert_eq!(balance, dec!(5000.50));
    }

    #[tokio::test]
    async fn test_mock_trait_object() {
        let conn = MockPolymarketConnector::new();
        let boxed: Box<dyn PredictionMarketConnector> = Box::new(conn);
        assert_eq!(boxed.platform(), Platform::Polymarket);

        // Can call methods through the trait object
        let markets = boxed.list_markets(MarketStatus::Open).await.unwrap();
        assert!(markets.is_empty());
    }
}
