use rust_decimal::Decimal;
use rust_decimal_macros::dec;

/// Convert Kalshi price in cents (1-99) to a normalized decimal (0.01-0.99).
pub fn kalshi_cents_to_decimal(cents: u32) -> Decimal {
    Decimal::from(cents) / dec!(100)
}

/// Validate that a price is in the valid range [0, 1].
pub fn validate_price(price: Decimal) -> bool {
    price >= dec!(0) && price <= dec!(1)
}

/// Calculate the arbitrage spread: 1.00 - buy_price_a - buy_price_b.
/// A positive spread means guaranteed profit per share.
pub fn calculate_spread(buy_price_a: Decimal, buy_price_b: Decimal) -> Decimal {
    dec!(1) - buy_price_a - buy_price_b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kalshi_cents_to_decimal() {
        assert_eq!(kalshi_cents_to_decimal(42), dec!(0.42));
        assert_eq!(kalshi_cents_to_decimal(1), dec!(0.01));
        assert_eq!(kalshi_cents_to_decimal(99), dec!(0.99));
    }

    #[test]
    fn test_validate_price() {
        assert!(validate_price(dec!(0)));
        assert!(validate_price(dec!(0.5)));
        assert!(validate_price(dec!(1)));
        assert!(!validate_price(dec!(1.01)));
        assert!(!validate_price(dec!(-0.01)));
    }

    #[test]
    fn test_calculate_spread() {
        // Buy YES at 0.42, buy NO at 0.53 → spread = 0.05
        assert_eq!(calculate_spread(dec!(0.42), dec!(0.53)), dec!(0.05));
        // No arbitrage: 0.50 + 0.50 = 1.00
        assert_eq!(calculate_spread(dec!(0.50), dec!(0.50)), dec!(0));
        // Negative spread (no opportunity)
        assert_eq!(calculate_spread(dec!(0.55), dec!(0.50)), dec!(-0.05));
    }
}
