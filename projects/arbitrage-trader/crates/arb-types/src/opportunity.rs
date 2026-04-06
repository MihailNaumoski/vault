use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::order::Side;

/// Status of an arbitrage opportunity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OpportunityStatus {
    Detected,
    Executing,
    Filled,
    Expired,
    Failed,
}

/// A detected arbitrage opportunity across a market pair.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Opportunity {
    pub id: Uuid,
    pub pair_id: Uuid,
    pub poly_side: Side,
    pub poly_price: Decimal,
    pub poly_market_id: String,
    pub poly_yes_token_id: String,
    pub kalshi_side: Side,
    pub kalshi_price: Decimal,
    pub kalshi_market_id: String,
    pub spread: Decimal,
    pub spread_pct: Decimal,
    pub max_quantity: u32,
    pub close_time: DateTime<Utc>,
    pub detected_at: DateTime<Utc>,
    pub status: OpportunityStatus,
}
