use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::Deserialize;

/// Fee configuration loaded from config/default.toml `[fees]` section.
#[derive(Debug, Clone, Deserialize)]
pub struct FeeConfig {
    /// Kalshi taker fee as a percentage of profit (e.g. 7.0 = 7%).
    pub kalshi_taker_fee_pct: Decimal,
    /// Polymarket CLOB taker fee as a percentage (currently 0%).
    pub poly_taker_fee_pct: Decimal,
}

impl Default for FeeConfig {
    fn default() -> Self {
        Self {
            kalshi_taker_fee_pct: dec!(7.0),
            poly_taker_fee_pct: dec!(0.0),
        }
    }
}

impl FeeConfig {
    /// Kalshi fee rate as a fraction (e.g. 0.07 for 7%).
    pub fn kalshi_rate(&self) -> Decimal {
        self.kalshi_taker_fee_pct / dec!(100)
    }

    /// Polymarket fee rate as a fraction.
    pub fn poly_rate(&self) -> Decimal {
        self.poly_taker_fee_pct / dec!(100)
    }

    /// Compute fees for a single position given prices and hedged quantity.
    ///
    /// Returns `(kalshi_fee, poly_fee, total_fee)`.
    pub fn compute_fees(
        &self,
        kalshi_price: Decimal,
        poly_price: Decimal,
        hedged_quantity: u32,
    ) -> (Decimal, Decimal, Decimal) {
        let qty = Decimal::from(hedged_quantity);
        let kalshi_fee = self.kalshi_rate() * kalshi_price * qty;
        let poly_fee = self.poly_rate() * poly_price * qty;
        let total = kalshi_fee + poly_fee;
        (kalshi_fee, poly_fee, total)
    }

    /// Estimate the round-trip fee cost per contract for a given set of prices.
    /// Used by the detector to set a fee-aware minimum spread.
    pub fn estimated_round_trip_fee(&self, kalshi_price: Decimal, poly_price: Decimal) -> Decimal {
        self.kalshi_rate() * kalshi_price + self.poly_rate() * poly_price
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_default_fees() {
        let cfg = FeeConfig::default();
        assert_eq!(cfg.kalshi_rate(), dec!(0.07));
        assert_eq!(cfg.poly_rate(), dec!(0.00));
    }

    #[test]
    fn test_compute_fees_kalshi_only() {
        let cfg = FeeConfig::default();
        let (k, p, total) = cfg.compute_fees(dec!(0.53), dec!(0.42), 50);
        // kalshi_fee = 0.07 * 0.53 * 50 = 1.855
        assert_eq!(k, dec!(1.855));
        assert_eq!(p, dec!(0.00));
        assert_eq!(total, dec!(1.855));
    }

    #[test]
    fn test_compute_fees_both_platforms() {
        let cfg = FeeConfig {
            kalshi_taker_fee_pct: dec!(7.0),
            poly_taker_fee_pct: dec!(2.0),
        };
        let (k, p, total) = cfg.compute_fees(dec!(0.53), dec!(0.42), 50);
        // kalshi_fee = 0.07 * 0.53 * 50 = 1.855
        // poly_fee = 0.02 * 0.42 * 50 = 0.42
        assert_eq!(k, dec!(1.855));
        assert_eq!(p, dec!(0.42));
        assert_eq!(total, dec!(2.275));
    }

    #[test]
    fn test_estimated_round_trip_fee() {
        let cfg = FeeConfig::default();
        let fee = cfg.estimated_round_trip_fee(dec!(0.53), dec!(0.42));
        // 0.07 * 0.53 + 0.00 * 0.42 = 0.0371
        assert_eq!(fee, dec!(0.0371));
    }
}
