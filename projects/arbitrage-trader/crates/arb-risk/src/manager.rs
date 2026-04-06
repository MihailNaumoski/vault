use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use thiserror::Error;
use uuid::Uuid;

use crate::exposure::ExposureTracker;
use crate::limits::RiskConfig;

/// Errors returned by pre-trade risk checks.
#[derive(Debug, Error)]
pub enum RiskError {
    #[error("engine is not running")]
    EngineNotRunning,

    #[error("market pair {0} is not verified")]
    PairNotVerified(Uuid),

    #[error("spread {spread} is below minimum threshold {min}")]
    SpreadTooLow { spread: Decimal, min: Decimal },

    #[error("market closes in {hours_remaining}h, minimum is {min_hours}h")]
    TooCloseToExpiry { hours_remaining: i64, min_hours: u32 },

    #[error("insufficient balance on {platform}: need {needed}, have {available}")]
    InsufficientBalance {
        platform: String,
        needed: Decimal,
        available: Decimal,
    },

    #[error("position limit exceeded for market {pair_id}: would be {would_be}, max is {max}")]
    PositionLimitExceeded {
        pair_id: Uuid,
        would_be: Decimal,
        max: Decimal,
    },

    #[error("total exposure would be {would_be}, max is {max}")]
    TotalExposureExceeded { would_be: Decimal, max: Decimal },

    #[error("unhedged exposure {current} exceeds max {max}")]
    UnhedgedExposureExceeded { current: Decimal, max: Decimal },

    #[error("daily loss {current} exceeds max {max}")]
    DailyLossExceeded { current: Decimal, max: Decimal },

    #[error("insufficient book depth: {available} contracts, need {min}")]
    InsufficientLiquidity { available: u32, min: u32 },
}

/// Risk manager that performs pre-trade checks based on configured limits.
pub struct RiskManager {
    config: RiskConfig,
    exposure: ExposureTracker,
    engine_running: bool,
}

impl RiskManager {
    pub fn new(config: RiskConfig) -> Self {
        Self {
            config,
            exposure: ExposureTracker::new(),
            engine_running: false,
        }
    }

    pub fn set_engine_running(&mut self, running: bool) {
        self.engine_running = running;
    }

    pub fn exposure(&self) -> &ExposureTracker {
        &self.exposure
    }

    pub fn exposure_mut(&mut self) -> &mut ExposureTracker {
        &mut self.exposure
    }

    pub fn config(&self) -> &RiskConfig {
        &self.config
    }

    /// Run all pre-trade checks for an opportunity.
    /// Matches Section 10.1 of the spec.
    #[allow(clippy::too_many_arguments)]
    pub fn pre_trade_check(
        &self,
        pair_id: Uuid,
        pair_verified: bool,
        spread: Decimal,
        min_spread: Decimal,
        close_time: DateTime<Utc>,
        quantity: u32,
        poly_price: Decimal,
        kalshi_price: Decimal,
        poly_balance: Decimal,
        kalshi_balance: Decimal,
        book_depth: u32,
    ) -> Result<(), RiskError> {
        // 1. System is running
        if !self.engine_running {
            return Err(RiskError::EngineNotRunning);
        }

        // 2. Market pair is verified
        if !pair_verified {
            return Err(RiskError::PairNotVerified(pair_id));
        }

        // 3. Spread exceeds minimum threshold
        if spread < min_spread {
            return Err(RiskError::SpreadTooLow {
                spread,
                min: min_spread,
            });
        }

        // 4. Market closes in > min_time_to_close_hours
        let hours_remaining = (close_time - Utc::now()).num_hours();
        if hours_remaining < self.config.min_time_to_close_hours as i64 {
            return Err(RiskError::TooCloseToExpiry {
                hours_remaining,
                min_hours: self.config.min_time_to_close_hours,
            });
        }

        // 5. Sufficient balance on both platforms
        let poly_cost = poly_price * Decimal::from(quantity);
        let kalshi_cost = kalshi_price * Decimal::from(quantity);
        if poly_balance < poly_cost {
            return Err(RiskError::InsufficientBalance {
                platform: "polymarket".into(),
                needed: poly_cost,
                available: poly_balance,
            });
        }
        if kalshi_balance < kalshi_cost {
            return Err(RiskError::InsufficientBalance {
                platform: "kalshi".into(),
                needed: kalshi_cost,
                available: kalshi_balance,
            });
        }

        // 6. Position size within per-market limit
        let current_market_exposure = self.exposure.market_exposure(&pair_id);
        let additional = poly_cost + kalshi_cost;
        let would_be = current_market_exposure + additional;
        if would_be > self.config.max_position_per_market {
            return Err(RiskError::PositionLimitExceeded {
                pair_id,
                would_be,
                max: self.config.max_position_per_market,
            });
        }

        // 7. Total exposure within global limit
        let total_would_be = self.exposure.total_exposure() + additional;
        if total_would_be > self.config.max_total_exposure {
            return Err(RiskError::TotalExposureExceeded {
                would_be: total_would_be,
                max: self.config.max_total_exposure,
            });
        }

        // 8. Max unhedged exposure not exceeded
        if self.exposure.unhedged_exposure() > self.config.max_unhedged_exposure {
            return Err(RiskError::UnhedgedExposureExceeded {
                current: self.exposure.unhedged_exposure(),
                max: self.config.max_unhedged_exposure,
            });
        }

        // 9. Daily loss limit not exceeded
        if self.exposure.daily_loss() > self.config.max_daily_loss {
            return Err(RiskError::DailyLossExceeded {
                current: self.exposure.daily_loss(),
                max: self.config.max_daily_loss,
            });
        }

        // 10. Order book has sufficient depth
        if book_depth < self.config.min_book_depth {
            return Err(RiskError::InsufficientLiquidity {
                available: book_depth,
                min: self.config.min_book_depth,
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use rust_decimal_macros::dec;

    fn default_config() -> RiskConfig {
        RiskConfig::default()
    }

    fn running_manager() -> RiskManager {
        let mut m = RiskManager::new(default_config());
        m.set_engine_running(true);
        m
    }

    /// Helper that returns parameters where all checks pass.
    fn valid_params() -> (Uuid, bool, Decimal, Decimal, DateTime<Utc>, u32, Decimal, Decimal, Decimal, Decimal, u32) {
        let pair_id = Uuid::now_v7();
        let pair_verified = true;
        let spread = dec!(0.05);
        let min_spread = dec!(0.02);
        let close_time = Utc::now() + Duration::hours(48);
        let quantity = 10;
        let poly_price = dec!(0.45);
        let kalshi_price = dec!(0.50);
        let poly_balance = dec!(5000.00);
        let kalshi_balance = dec!(5000.00);
        let book_depth = 100;
        (pair_id, pair_verified, spread, min_spread, close_time, quantity, poly_price, kalshi_price, poly_balance, kalshi_balance, book_depth)
    }

    #[test]
    fn test_valid_params_pass() {
        let m = running_manager();
        let (pid, ver, spread, min_s, ct, qty, pp, kp, pb, kb, bd) = valid_params();
        assert!(m.pre_trade_check(pid, ver, spread, min_s, ct, qty, pp, kp, pb, kb, bd).is_ok());
    }

    #[test]
    fn test_engine_not_running() {
        let m = RiskManager::new(default_config()); // engine_running = false
        let (pid, ver, spread, min_s, ct, qty, pp, kp, pb, kb, bd) = valid_params();
        let err = m.pre_trade_check(pid, ver, spread, min_s, ct, qty, pp, kp, pb, kb, bd).unwrap_err();
        assert!(matches!(err, RiskError::EngineNotRunning));
    }

    #[test]
    fn test_unverified_pair() {
        let m = running_manager();
        let (pid, _ver, spread, min_s, ct, qty, pp, kp, pb, kb, bd) = valid_params();
        let err = m.pre_trade_check(pid, false, spread, min_s, ct, qty, pp, kp, pb, kb, bd).unwrap_err();
        assert!(matches!(err, RiskError::PairNotVerified(_)));
    }

    #[test]
    fn test_spread_too_low() {
        let m = running_manager();
        let (pid, ver, _spread, _min_s, ct, qty, pp, kp, pb, kb, bd) = valid_params();
        let err = m.pre_trade_check(pid, ver, dec!(0.01), dec!(0.02), ct, qty, pp, kp, pb, kb, bd).unwrap_err();
        assert!(matches!(err, RiskError::SpreadTooLow { .. }));
    }

    #[test]
    fn test_too_close_to_expiry() {
        let m = running_manager();
        let (pid, ver, spread, min_s, _ct, qty, pp, kp, pb, kb, bd) = valid_params();
        let close_time = Utc::now() + Duration::hours(2); // default min is 24h
        let err = m.pre_trade_check(pid, ver, spread, min_s, close_time, qty, pp, kp, pb, kb, bd).unwrap_err();
        assert!(matches!(err, RiskError::TooCloseToExpiry { .. }));
    }

    #[test]
    fn test_insufficient_poly_balance() {
        let m = running_manager();
        let (pid, ver, spread, min_s, ct, qty, pp, kp, _pb, kb, bd) = valid_params();
        let err = m.pre_trade_check(pid, ver, spread, min_s, ct, qty, pp, kp, dec!(0.01), kb, bd).unwrap_err();
        assert!(matches!(err, RiskError::InsufficientBalance { .. }));
    }

    #[test]
    fn test_insufficient_kalshi_balance() {
        let m = running_manager();
        let (pid, ver, spread, min_s, ct, qty, pp, kp, pb, _kb, bd) = valid_params();
        let err = m.pre_trade_check(pid, ver, spread, min_s, ct, qty, pp, kp, pb, dec!(0.01), bd).unwrap_err();
        assert!(matches!(err, RiskError::InsufficientBalance { .. }));
    }

    #[test]
    fn test_position_limit_exceeded() {
        let mut m = running_manager();
        let pair_id = Uuid::now_v7();
        // Fill up the per-market exposure to near the limit (default 1000)
        m.exposure_mut().add_position(pair_id, dec!(995.00));
        let close_time = Utc::now() + Duration::hours(48);
        let err = m.pre_trade_check(
            pair_id, true, dec!(0.05), dec!(0.02), close_time,
            10, dec!(0.45), dec!(0.50), dec!(5000), dec!(5000), 100,
        ).unwrap_err();
        assert!(matches!(err, RiskError::PositionLimitExceeded { .. }));
    }

    #[test]
    fn test_total_exposure_exceeded() {
        let mut m = running_manager();
        // Fill up total exposure to near the global limit (default 10000)
        let big_pair = Uuid::now_v7();
        m.exposure_mut().add_position(big_pair, dec!(9995.00));
        let pair_id = Uuid::now_v7();
        let close_time = Utc::now() + Duration::hours(48);
        let err = m.pre_trade_check(
            pair_id, true, dec!(0.05), dec!(0.02), close_time,
            10, dec!(0.45), dec!(0.50), dec!(5000), dec!(5000), 100,
        ).unwrap_err();
        assert!(matches!(err, RiskError::TotalExposureExceeded { .. }));
    }

    #[test]
    fn test_daily_loss_exceeded() {
        let mut m = running_manager();
        // Record losses exceeding the daily limit (default 200)
        m.exposure_mut().record_unwind_loss(dec!(250.00));
        let (pid, ver, spread, min_s, ct, qty, pp, kp, pb, kb, bd) = valid_params();
        let err = m.pre_trade_check(pid, ver, spread, min_s, ct, qty, pp, kp, pb, kb, bd).unwrap_err();
        assert!(matches!(err, RiskError::DailyLossExceeded { .. }));
    }

    #[test]
    fn test_insufficient_book_depth() {
        let m = running_manager();
        let (pid, ver, spread, min_s, ct, qty, pp, kp, pb, kb, _bd) = valid_params();
        let err = m.pre_trade_check(pid, ver, spread, min_s, ct, qty, pp, kp, pb, kb, 5).unwrap_err();
        assert!(matches!(err, RiskError::InsufficientLiquidity { .. }));
    }

    #[test]
    fn test_exposure_tracker_add_remove() {
        let mut tracker = ExposureTracker::new();
        let pair_id = Uuid::now_v7();
        tracker.add_position(pair_id, dec!(100.00));
        assert_eq!(tracker.market_exposure(&pair_id), dec!(100.00));
        assert_eq!(tracker.total_exposure(), dec!(100.00));
        tracker.remove_position(&pair_id, dec!(60.00));
        assert_eq!(tracker.market_exposure(&pair_id), dec!(40.00));
        assert_eq!(tracker.total_exposure(), dec!(40.00));
        tracker.remove_position(&pair_id, dec!(40.00));
        assert_eq!(tracker.market_exposure(&pair_id), dec!(0));
    }

    #[test]
    fn test_exposure_tracker_daily_reset() {
        let mut tracker = ExposureTracker::new();
        tracker.record_unwind_loss(dec!(50.00));
        tracker.record_unwind_loss(dec!(30.00));
        assert_eq!(tracker.daily_loss(), dec!(80.00));
        assert_eq!(tracker.unwind_rate_pct(), dec!(0)); // no trades yet
        tracker.add_position(Uuid::now_v7(), dec!(100.00));
        tracker.reset_daily();
        assert_eq!(tracker.daily_loss(), dec!(0));
    }

    #[test]
    fn test_risk_config_defaults() {
        let config = RiskConfig::default();
        assert_eq!(config.max_position_per_market, dec!(1000.00));
        assert_eq!(config.max_total_exposure, dec!(10000.00));
        assert_eq!(config.max_unhedged_exposure, dec!(500.00));
        assert_eq!(config.max_daily_loss, dec!(200.00));
        assert_eq!(config.min_time_to_close_hours, 24);
        assert_eq!(config.min_book_depth, 50);
        assert_eq!(config.max_unwind_rate_pct, dec!(20.0));
    }
}
