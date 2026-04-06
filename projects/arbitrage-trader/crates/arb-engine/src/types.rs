use arb_types::{OrderResponse, Platform};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
pub struct EngineConfig {
    pub scan_interval_ms: u64,
    pub min_spread_pct: Decimal,
    pub min_spread_absolute: Decimal,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OrderConfig {
    pub max_order_age_secs: u64,
    pub max_hedge_wait_secs: u64,
    pub order_check_interval_ms: u64,
    pub min_repost_spread: Decimal,
    pub price_improve_amount: Decimal,
    pub default_quantity: u32,
}

#[derive(Debug, Clone)]
pub struct ExecutionResult {
    pub opportunity_id: Uuid,
    pub poly_order: OrderResponse,
    pub kalshi_order: OrderResponse,
    pub executed_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub enum MonitorAction {
    BothFilled {
        poly_order: OrderResponse,
        kalshi_order: OrderResponse,
    },
    Waiting,
    PartialHedge {
        filled_platform: Platform,
        filled_order: OrderResponse,
        open_order_id: String,
    },
    NeedsUnwind {
        filled_platform: Platform,
        filled_order: OrderResponse,
        unfilled_order_id: String,
    },
    BothCancelled,
}

#[derive(Debug, Clone)]
pub struct PairInfo {
    pub pair_id: Uuid,
    pub poly_market_id: String,
    pub kalshi_market_id: String,
    pub close_time: DateTime<Utc>,
    pub verified: bool,
    pub poly_yes_token_id: String,
    pub poly_no_token_id: String,
    pub volume: Decimal,
}
