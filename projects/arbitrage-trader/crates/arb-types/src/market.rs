use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::Platform;

/// Unique identifier for a market, wrapping a UUID v7.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MarketId(pub Uuid);

impl MarketId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }
}

impl Default for MarketId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for MarketId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Market status on a platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MarketStatus {
    Open,
    Closed,
    Settled,
}

/// A single binary-outcome market on one platform.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Market {
    pub id: MarketId,
    pub platform: Platform,
    pub platform_id: String,
    pub question: String,
    pub yes_price: Decimal,
    pub no_price: Decimal,
    pub volume: Decimal,
    pub liquidity: Decimal,
    pub status: MarketStatus,
    pub close_time: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Reference to a market on a specific platform, used within a MarketPair.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketRef {
    pub platform_id: String,
    pub question: String,
    pub close_time: DateTime<Utc>,
}

/// Two markets on different platforms representing the same real-world event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketPair {
    pub id: Uuid,
    pub polymarket: MarketRef,
    pub kalshi: MarketRef,
    pub match_confidence: f64,
    pub verified: bool,
    pub active: bool,
    pub created_at: DateTime<Utc>,
}
