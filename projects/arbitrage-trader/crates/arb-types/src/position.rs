use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::order::Side;

/// Status of a hedged position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PositionStatus {
    Open,
    SettledPoly,
    SettledKalshi,
    FullySettled,
}

/// A hedged position across both platforms for one market pair.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub id: Uuid,
    pub pair_id: Uuid,
    pub poly_side: Side,
    pub poly_quantity: u32,
    pub poly_avg_price: Decimal,
    pub kalshi_side: Side,
    pub kalshi_quantity: u32,
    pub kalshi_avg_price: Decimal,
    pub hedged_quantity: u32,
    pub unhedged_quantity: i32,
    pub guaranteed_profit: Decimal,
    pub status: PositionStatus,
    pub opened_at: DateTime<Utc>,
    pub settled_at: Option<DateTime<Utc>>,
}

/// A position on a single platform, as reported by the platform API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformPosition {
    pub market_id: String,
    pub side: Side,
    pub quantity: u32,
    pub avg_price: Decimal,
}
