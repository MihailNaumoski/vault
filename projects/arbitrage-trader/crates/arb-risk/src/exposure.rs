use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::HashMap;
use uuid::Uuid;

/// Tracks current exposure across all positions.
pub struct ExposureTracker {
    /// Per-market-pair exposure (pair_id -> total capital deployed).
    per_market: HashMap<Uuid, Decimal>,
    /// Total exposure across all positions.
    total_exposure: Decimal,
    /// Total unhedged exposure (one-legged positions).
    unhedged_exposure: Decimal,
    /// Daily realized loss.
    daily_loss: Decimal,
    /// Number of trades that required unwinding today.
    unwind_count: u32,
    /// Total trades executed today.
    total_trades_today: u32,
}

impl ExposureTracker {
    pub fn new() -> Self {
        Self {
            per_market: HashMap::new(),
            total_exposure: dec!(0),
            unhedged_exposure: dec!(0),
            daily_loss: dec!(0),
            unwind_count: 0,
            total_trades_today: 0,
        }
    }

    /// Get exposure for a specific market pair.
    pub fn market_exposure(&self, pair_id: &Uuid) -> Decimal {
        self.per_market.get(pair_id).copied().unwrap_or(dec!(0))
    }

    /// Get total exposure across all positions.
    pub fn total_exposure(&self) -> Decimal {
        self.total_exposure
    }

    /// Get total unhedged exposure.
    pub fn unhedged_exposure(&self) -> Decimal {
        self.unhedged_exposure
    }

    /// Get daily loss so far.
    pub fn daily_loss(&self) -> Decimal {
        self.daily_loss
    }

    /// Get the unwind rate as a percentage.
    pub fn unwind_rate_pct(&self) -> Decimal {
        if self.total_trades_today == 0 {
            return dec!(0);
        }
        Decimal::from(self.unwind_count) / Decimal::from(self.total_trades_today) * dec!(100)
    }

    /// Record a new position being opened.
    pub fn add_position(&mut self, pair_id: Uuid, capital: Decimal) {
        *self.per_market.entry(pair_id).or_insert(dec!(0)) += capital;
        self.total_exposure += capital;
        self.total_trades_today += 1;
    }

    /// Record a position being closed/settled.
    pub fn remove_position(&mut self, pair_id: &Uuid, capital: Decimal) {
        if let Some(exposure) = self.per_market.get_mut(pair_id) {
            *exposure -= capital;
            if *exposure <= dec!(0) {
                self.per_market.remove(pair_id);
            }
        }
        self.total_exposure -= capital;
    }

    /// Record unhedged exposure change.
    pub fn set_unhedged_exposure(&mut self, amount: Decimal) {
        self.unhedged_exposure = amount;
    }

    /// Record a loss from an unwind event.
    pub fn record_unwind_loss(&mut self, loss: Decimal) {
        self.daily_loss += loss;
        self.unwind_count += 1;
    }

    /// Reset daily counters (call at start of each trading day).
    pub fn reset_daily(&mut self) {
        self.daily_loss = dec!(0);
        self.unwind_count = 0;
        self.total_trades_today = 0;
    }
}

impl Default for ExposureTracker {
    fn default() -> Self {
        Self::new()
    }
}
