use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::Platform;

/// Side of a prediction market position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Yes,
    No,
}

/// Order type — limit only for MVP.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OrderType {
    Limit,
}

/// Status of an order on a platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OrderStatus {
    Pending,
    Open,
    PartialFill,
    Filled,
    Cancelled,
    Failed,
}

/// An order placed on a platform as part of an arbitrage opportunity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub id: Uuid,
    pub opportunity_id: Uuid,
    pub platform: Platform,
    pub platform_order_id: Option<String>,
    pub market_id: String,
    pub side: Side,
    pub price: Decimal,
    pub quantity: u32,
    pub filled_quantity: u32,
    pub order_type: OrderType,
    pub status: OrderStatus,
    pub placed_at: DateTime<Utc>,
    pub filled_at: Option<DateTime<Utc>>,
    pub cancelled_at: Option<DateTime<Utc>>,
    pub cancel_reason: Option<String>,
}

/// Request to place a limit order on a platform.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimitOrderRequest {
    pub market_id: String,
    pub side: Side,
    pub price: Decimal,
    pub quantity: u32,
}

/// Response from a platform after placing or querying an order.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderResponse {
    pub order_id: String,
    pub status: OrderStatus,
    pub filled_quantity: u32,
    pub price: Decimal,
    pub side: Side,
    pub market_id: String,
}

/// A single level in an order book.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBookLevel {
    pub price: Decimal,
    pub quantity: u32,
}

/// Order book for a market.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBook {
    pub market_id: String,
    pub bids: Vec<OrderBookLevel>,
    pub asks: Vec<OrderBookLevel>,
    pub timestamp: DateTime<Utc>,
}

impl Side {
    pub fn opposite(&self) -> Self {
        match self {
            Side::Yes => Side::No,
            Side::No => Side::Yes,
        }
    }
}

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

impl OrderBook {
    /// Get the best (lowest) ask price for a given side.
    pub fn best_ask(&self, _side: Side) -> Option<Decimal> {
        self.asks.first().map(|l| l.price)
    }

    /// Get the best (highest) bid price for a given side.
    pub fn best_bid(&self, _side: Side) -> Option<Decimal> {
        self.bids.last().map(|l| l.price)
    }
}
