use rust_decimal::Decimal;
use serde::Deserialize;

/// Risk configuration matching the [risk] section in config/default.toml.
#[derive(Debug, Clone, Deserialize)]
pub struct RiskConfig {
    pub max_position_per_market: Decimal,
    pub max_total_exposure: Decimal,
    pub max_unhedged_exposure: Decimal,
    pub max_daily_loss: Decimal,
    pub min_time_to_close_hours: u32,
    pub min_book_depth: u32,
    pub max_unwind_rate_pct: Decimal,
}

impl Default for RiskConfig {
    fn default() -> Self {
        use rust_decimal_macros::dec;
        Self {
            max_position_per_market: dec!(1000.00),
            max_total_exposure: dec!(10000.00),
            max_unhedged_exposure: dec!(500.00),
            max_daily_loss: dec!(200.00),
            min_time_to_close_hours: 24,
            min_book_depth: 50,
            max_unwind_rate_pct: dec!(20.0),
        }
    }
}
