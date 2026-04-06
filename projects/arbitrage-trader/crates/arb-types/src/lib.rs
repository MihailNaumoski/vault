pub mod market;
pub mod order;
pub mod position;
pub mod opportunity;
pub mod price;
pub mod error;
pub mod event;

use serde::{Deserialize, Serialize};

// Re-export key types at the crate root for convenience.
pub use market::{Market, MarketId, MarketPair, MarketRef, MarketStatus};
pub use order::{
    LimitOrderRequest, Order, OrderBook, OrderBookLevel, OrderResponse, OrderStatus, OrderType,
    Side,
};
pub use position::{PlatformPosition, Position, PositionStatus};
pub use opportunity::{Opportunity, OpportunityStatus};
pub use price::{calculate_spread, kalshi_cents_to_decimal, validate_price};
pub use error::ArbError;
pub use event::{PriceUpdate, SubHandle};

/// Supported prediction market platforms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Polymarket,
    Kalshi,
}

/// The common async trait that all platform connectors must implement.
#[async_trait::async_trait]
pub trait PredictionMarketConnector: Send + Sync + 'static {
    fn platform(&self) -> Platform;

    // Market data
    async fn list_markets(&self, status: MarketStatus) -> Result<Vec<Market>, ArbError>;
    async fn get_market(&self, id: &str) -> Result<Market, ArbError>;
    async fn get_order_book(&self, id: &str) -> Result<OrderBook, ArbError>;
    async fn subscribe_prices(
        &self,
        ids: &[String],
        tx: tokio::sync::mpsc::Sender<PriceUpdate>,
    ) -> Result<SubHandle, ArbError>;

    // Trading
    async fn place_limit_order(&self, req: &LimitOrderRequest) -> Result<OrderResponse, ArbError>;
    async fn cancel_order(&self, order_id: &str) -> Result<(), ArbError>;
    async fn get_order(&self, order_id: &str) -> Result<OrderResponse, ArbError>;
    async fn list_open_orders(&self) -> Result<Vec<OrderResponse>, ArbError>;

    // Account
    async fn get_balance(&self) -> Result<rust_decimal::Decimal, ArbError>;
    async fn get_positions(&self) -> Result<Vec<PlatformPosition>, ArbError>;
}
