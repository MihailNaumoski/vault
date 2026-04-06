use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use uuid::Uuid;

/// Database row for a market pair.
#[derive(Debug, Clone)]
pub struct MarketPairRow {
    pub id: String,
    pub poly_condition_id: String,
    pub poly_yes_token_id: String,
    pub poly_no_token_id: String,
    pub poly_question: String,
    pub kalshi_ticker: String,
    pub kalshi_question: String,
    pub match_confidence: f64,
    pub verified: bool,
    pub active: bool,
    pub close_time: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Database row for an opportunity.
#[derive(Debug, Clone)]
pub struct OpportunityRow {
    pub id: String,
    pub pair_id: String,
    pub poly_side: String,
    pub poly_price: Decimal,
    pub kalshi_side: String,
    pub kalshi_price: Decimal,
    pub spread: Decimal,
    pub spread_pct: Decimal,
    pub max_quantity: i64,
    pub status: String,
    pub detected_at: DateTime<Utc>,
    pub executed_at: Option<DateTime<Utc>>,
    pub resolved_at: Option<DateTime<Utc>>,
}

/// Database row for an order.
#[derive(Debug, Clone)]
pub struct OrderRow {
    pub id: String,
    pub opportunity_id: String,
    pub platform: String,
    pub platform_order_id: Option<String>,
    pub market_id: String,
    pub side: String,
    pub price: Decimal,
    pub quantity: i64,
    pub filled_quantity: i64,
    pub status: String,
    pub placed_at: DateTime<Utc>,
    pub filled_at: Option<DateTime<Utc>>,
    pub cancelled_at: Option<DateTime<Utc>>,
    pub cancel_reason: Option<String>,
}

/// Database row for a position.
#[derive(Debug, Clone)]
pub struct PositionRow {
    pub id: String,
    pub pair_id: String,
    pub poly_side: String,
    pub poly_quantity: i64,
    pub poly_avg_price: Decimal,
    pub kalshi_side: String,
    pub kalshi_quantity: i64,
    pub kalshi_avg_price: Decimal,
    pub hedged_quantity: i64,
    pub unhedged_quantity: i64,
    pub guaranteed_profit: Decimal,
    pub status: String,
    pub opened_at: DateTime<Utc>,
    pub settled_at: Option<DateTime<Utc>>,
}

/// Database row for a price snapshot.
#[derive(Debug, Clone)]
pub struct PriceSnapshotRow {
    pub id: i64,
    pub pair_id: String,
    pub poly_yes_price: Decimal,
    pub kalshi_yes_price: Decimal,
    pub spread: Decimal,
    pub captured_at: DateTime<Utc>,
}

/// Insert model for a price snapshot (no auto-increment id).
#[derive(Debug, Clone)]
pub struct NewPriceSnapshot {
    pub pair_id: Uuid,
    pub poly_yes_price: Decimal,
    pub kalshi_yes_price: Decimal,
    pub spread: Decimal,
    pub captured_at: DateTime<Utc>,
}

/// Database row for daily P&L.
#[derive(Debug, Clone)]
pub struct DailyPnlRow {
    pub date: String,
    pub trades_executed: i64,
    pub trades_filled: i64,
    pub gross_profit: Decimal,
    pub fees_paid: Decimal,
    pub net_profit: Decimal,
    pub capital_deployed: Decimal,
}
