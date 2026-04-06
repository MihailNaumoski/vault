use arb_types::*;
use arb_types::order::OrderStatus;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::info;

/// Paper trading state — tracks simulated orders, positions, balance.
#[derive(Debug)]
pub struct PaperState {
    orders: HashMap<String, PaperOrder>,
    balance: Decimal,
    initial_balance: Decimal,
    next_order_id: u64,
    fill_probability: f64,
    fill_delay_ms: u64,
}

#[derive(Debug, Clone)]
struct PaperOrder {
    response: OrderResponse,
    request: LimitOrderRequest,
    #[allow(dead_code)]
    placed_at: DateTime<Utc>,
    will_fill: bool,
    fill_after: DateTime<Utc>,
}

impl PaperState {
    pub fn new(initial_balance: Decimal, fill_probability: f64, fill_delay_ms: u64) -> Self {
        Self {
            orders: HashMap::new(),
            balance: initial_balance,
            initial_balance,
            next_order_id: 1,
            fill_probability,
            fill_delay_ms,
        }
    }

    pub fn total_pnl(&self) -> Decimal {
        self.balance - self.initial_balance
    }
}

/// Wraps a real connector for market data, simulates trading locally.
///
/// SAFETY: place_limit_order and cancel_order NEVER touch the network.
/// Only market data methods (list_markets, get_order_book, subscribe_prices)
/// delegate to the inner real connector.
pub struct PaperConnector {
    inner: Arc<dyn PredictionMarketConnector>,
    state: Arc<Mutex<PaperState>>,
    platform: Platform,
}

impl PaperConnector {
    pub fn new(
        inner: Arc<dyn PredictionMarketConnector>,
        initial_balance: Decimal,
        fill_probability: f64,
        fill_delay_ms: u64,
    ) -> Self {
        let platform = inner.platform();
        Self {
            inner,
            state: Arc::new(Mutex::new(PaperState::new(initial_balance, fill_probability, fill_delay_ms))),
            platform,
        }
    }

    pub fn state(&self) -> &Arc<Mutex<PaperState>> {
        &self.state
    }
}

#[async_trait]
impl PredictionMarketConnector for PaperConnector {
    fn platform(&self) -> Platform {
        self.platform
    }

    // === MARKET DATA: delegate to real connector ===

    async fn list_markets(&self, status: MarketStatus) -> Result<Vec<Market>, ArbError> {
        self.inner.list_markets(status).await
    }

    async fn get_market(&self, id: &str) -> Result<Market, ArbError> {
        self.inner.get_market(id).await
    }

    async fn get_order_book(&self, id: &str) -> Result<OrderBook, ArbError> {
        self.inner.get_order_book(id).await
    }

    async fn subscribe_prices(
        &self,
        ids: &[String],
        tx: mpsc::Sender<PriceUpdate>,
    ) -> Result<SubHandle, ArbError> {
        self.inner.subscribe_prices(ids, tx).await
    }

    // === TRADING: simulated locally, ZERO network calls ===

    async fn place_limit_order(&self, req: &LimitOrderRequest) -> Result<OrderResponse, ArbError> {
        let mut state = self.state.lock();

        // Check simulated balance
        let cost = req.price * Decimal::from(req.quantity);
        if state.balance < cost {
            return Err(ArbError::OrderRejected {
                platform: self.platform,
                reason: format!("paper: insufficient balance ({} < {})", state.balance, cost),
            });
        }

        let order_id = format!("paper-{}-{}", self.platform, state.next_order_id);
        state.next_order_id += 1;

        // Determine if this order will fill (probability-based)
        let will_fill = rand::random::<f64>() < state.fill_probability;
        let fill_after = Utc::now() + chrono::Duration::milliseconds(state.fill_delay_ms as i64);

        let response = OrderResponse {
            order_id: order_id.clone(),
            status: OrderStatus::Open,
            filled_quantity: 0,
            price: req.price,
            side: req.side,
            market_id: req.market_id.clone(),
        };

        state.orders.insert(order_id.clone(), PaperOrder {
            response: response.clone(),
            request: req.clone(),
            placed_at: Utc::now(),
            will_fill,
            fill_after,
        });

        // Deduct balance immediately (reserved for this order)
        state.balance -= cost;

        info!(
            platform = %self.platform,
            order_id,
            will_fill,
            price = %req.price,
            qty = req.quantity,
            "paper order placed"
        );

        Ok(response)
    }

    async fn cancel_order(&self, order_id: &str) -> Result<(), ArbError> {
        let mut state = self.state.lock();
        if let Some(order) = state.orders.get_mut(order_id) {
            if order.response.status == OrderStatus::Open {
                order.response.status = OrderStatus::Cancelled;
                // Refund reserved balance
                let cost = order.request.price * Decimal::from(order.request.quantity);
                state.balance += cost;
                info!(order_id, "paper order cancelled, balance refunded");
            }
        }
        Ok(())
    }

    async fn get_order(&self, order_id: &str) -> Result<OrderResponse, ArbError> {
        let mut state = self.state.lock();
        let order = state.orders.get_mut(order_id)
            .ok_or_else(|| ArbError::Other(format!("paper order not found: {order_id}")))?;

        // Check if fill time has passed
        if order.will_fill && order.response.status == OrderStatus::Open && Utc::now() >= order.fill_after {
            order.response.status = OrderStatus::Filled;
            order.response.filled_quantity = order.request.quantity;
            info!(order_id, "paper order filled");
        }

        Ok(order.response.clone())
    }

    async fn list_open_orders(&self) -> Result<Vec<OrderResponse>, ArbError> {
        let state = self.state.lock();
        Ok(state.orders.values()
            .filter(|o| o.response.status == OrderStatus::Open)
            .map(|o| o.response.clone())
            .collect())
    }

    async fn get_balance(&self) -> Result<Decimal, ArbError> {
        Ok(self.state.lock().balance)
    }

    async fn get_positions(&self) -> Result<Vec<PlatformPosition>, ArbError> {
        // Paper positions tracked by the engine's Tracker, not here
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use arb_types::order::Side;

    // Use a simple mock for the inner connector in paper tests
    struct DummyConnector;

    #[async_trait]
    impl PredictionMarketConnector for DummyConnector {
        fn platform(&self) -> Platform { Platform::Polymarket }
        async fn list_markets(&self, _: MarketStatus) -> Result<Vec<Market>, ArbError> { Ok(vec![]) }
        async fn get_market(&self, _: &str) -> Result<Market, ArbError> { Err(ArbError::Other("dummy".into())) }
        async fn get_order_book(&self, _: &str) -> Result<OrderBook, ArbError> { Ok(OrderBook::default()) }
        async fn subscribe_prices(&self, _: &[String], _: mpsc::Sender<PriceUpdate>) -> Result<SubHandle, ArbError> {
            let (tx, _) = tokio::sync::oneshot::channel();
            Ok(SubHandle { cancel_tx: tx })
        }
        async fn place_limit_order(&self, _: &LimitOrderRequest) -> Result<OrderResponse, ArbError> {
            panic!("REAL place_limit_order called in paper mode — THIS MUST NEVER HAPPEN");
        }
        async fn cancel_order(&self, _: &str) -> Result<(), ArbError> {
            panic!("REAL cancel_order called in paper mode — THIS MUST NEVER HAPPEN");
        }
        async fn get_order(&self, _: &str) -> Result<OrderResponse, ArbError> {
            panic!("REAL get_order called in paper mode");
        }
        async fn list_open_orders(&self) -> Result<Vec<OrderResponse>, ArbError> { Ok(vec![]) }
        async fn get_balance(&self) -> Result<Decimal, ArbError> { Ok(dec!(10000)) }
        async fn get_positions(&self) -> Result<Vec<PlatformPosition>, ArbError> { Ok(vec![]) }
    }

    #[tokio::test]
    async fn test_paper_place_and_get_order() {
        let inner: Arc<dyn PredictionMarketConnector> = Arc::new(DummyConnector);
        let paper = PaperConnector::new(inner, dec!(10000), 1.0, 0); // 100% fill, instant

        let req = LimitOrderRequest {
            market_id: "tok-123".into(),
            side: Side::Yes,
            price: dec!(0.42),
            quantity: 50,
        };
        let resp = paper.place_limit_order(&req).await.unwrap();
        assert_eq!(resp.status, OrderStatus::Open);
        assert!(resp.order_id.starts_with("paper-"));

        // Balance should be deducted
        let bal = paper.get_balance().await.unwrap();
        assert_eq!(bal, dec!(10000) - dec!(21.00)); // 0.42 * 50

        // After fill delay (0ms), should be filled
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let filled = paper.get_order(&resp.order_id).await.unwrap();
        assert_eq!(filled.status, OrderStatus::Filled);
        assert_eq!(filled.filled_quantity, 50);
    }

    #[tokio::test]
    async fn test_paper_cancel_refunds_balance() {
        let inner: Arc<dyn PredictionMarketConnector> = Arc::new(DummyConnector);
        let paper = PaperConnector::new(inner, dec!(10000), 0.0, 5000); // 0% fill

        let req = LimitOrderRequest {
            market_id: "tok-123".into(),
            side: Side::Yes,
            price: dec!(0.50),
            quantity: 100,
        };
        let resp = paper.place_limit_order(&req).await.unwrap();
        assert_eq!(paper.get_balance().await.unwrap(), dec!(9950)); // 10000 - 50

        paper.cancel_order(&resp.order_id).await.unwrap();
        assert_eq!(paper.get_balance().await.unwrap(), dec!(10000)); // refunded
    }

    #[tokio::test]
    async fn test_paper_rejects_insufficient_balance() {
        let inner: Arc<dyn PredictionMarketConnector> = Arc::new(DummyConnector);
        let paper = PaperConnector::new(inner, dec!(10), 1.0, 0); // only $10

        let req = LimitOrderRequest {
            market_id: "tok-123".into(),
            side: Side::Yes,
            price: dec!(0.50),
            quantity: 100, // costs $50
        };
        let result = paper.place_limit_order(&req).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_paper_never_calls_real_trading() {
        // DummyConnector panics if real trading methods are called
        // PaperConnector should never trigger those panics
        let inner: Arc<dyn PredictionMarketConnector> = Arc::new(DummyConnector);
        let paper = PaperConnector::new(inner, dec!(10000), 1.0, 0);

        // These should all work without panic
        let req = LimitOrderRequest { market_id: "t".into(), side: Side::Yes, price: dec!(0.5), quantity: 10 };
        let resp = paper.place_limit_order(&req).await.unwrap();
        paper.cancel_order(&resp.order_id).await.unwrap();
        let _ = paper.get_order(&resp.order_id).await;
        let _ = paper.list_open_orders().await;
        // If we get here without panic, the safety boundary held
    }

    #[tokio::test]
    async fn test_paper_list_open_orders() {
        let inner: Arc<dyn PredictionMarketConnector> = Arc::new(DummyConnector);
        let paper = PaperConnector::new(inner, dec!(10000), 0.0, 99999); // never fills

        let req = LimitOrderRequest { market_id: "t".into(), side: Side::Yes, price: dec!(0.1), quantity: 10 };
        paper.place_limit_order(&req).await.unwrap();
        paper.place_limit_order(&req).await.unwrap();

        let open = paper.list_open_orders().await.unwrap();
        assert_eq!(open.len(), 2);
    }
}
