use crate::fees::FeeConfig;
use crate::price_cache::PriceCache;
use crate::types::{EngineConfig, PairInfo};
use arb_types::order::Side;
use arb_types::{Opportunity, OpportunityStatus};
use chrono::Utc;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

pub struct Detector {
    price_cache: Arc<PriceCache>,
    min_spread_pct: Decimal,
    min_spread_absolute: Decimal,
    fee_config: FeeConfig,
    max_staleness: Duration,
}

impl Detector {
    pub fn new(price_cache: Arc<PriceCache>, config: &EngineConfig, fee_config: &FeeConfig) -> Self {
        Self {
            price_cache,
            min_spread_pct: config.min_spread_pct,
            min_spread_absolute: config.min_spread_absolute,
            fee_config: fee_config.clone(),
            max_staleness: Duration::from_secs(30),
        }
    }

    pub fn scan(&self, pairs: &[PairInfo]) -> Vec<Opportunity> {
        pairs
            .iter()
            .filter(|p| p.verified)
            .filter_map(|p| self.check_pair(p))
            .collect()
    }

    fn check_pair(&self, pair: &PairInfo) -> Option<Opportunity> {
        let prices = self.price_cache.get(&pair.pair_id)?;

        if !self.price_cache.is_fresh(&pair.pair_id, self.max_staleness) {
            return None;
        }

        // Guard: reject any price that is zero — zero prices are always data errors
        // (e.g. missing WS data defaulting to 0) and would produce phantom spreads
        // like spread = 1 - 0.52 - 0 = 0.48 which are entirely fictitious.
        if prices.poly_yes.is_zero()
            || prices.poly_no.is_zero()
            || prices.kalshi_yes.is_zero()
            || prices.kalshi_no.is_zero()
        {
            return None;
        }

        // Close-time filter: skip pairs closing within 6 hours.
        // The final ~4-6 hours before market close see a liquidity cliff as market
        // makers pull orders. 6h gives enough runway for both arb legs to fill
        // while keeping the vast majority of valid markets eligible.
        // Note: "decided" markets are already caught by the price extreme filter
        // below — this gate targets liquidity decline, not outcome certainty.
        let hours_to_close = (pair.close_time - Utc::now()).num_hours();
        if hours_to_close < 6 {
            return None;
        }

        // Price extreme filter: reject near-decided markets.
        // Any individual price below 0.05 or above 0.95 indicates the market is
        // effectively decided — remaining spread is phantom and counterparties
        // won't fill at reasonable sizes.
        if prices.poly_yes < dec!(0.05)
            || prices.poly_yes > dec!(0.95)
            || prices.poly_no < dec!(0.05)
            || prices.poly_no > dec!(0.95)
            || prices.kalshi_yes < dec!(0.05)
            || prices.kalshi_yes > dec!(0.95)
            || prices.kalshi_no < dec!(0.05)
            || prices.kalshi_no > dec!(0.95)
        {
            return None;
        }

        let spread_a = dec!(1) - prices.poly_yes - prices.kalshi_no;
        let spread_b = dec!(1) - prices.poly_no - prices.kalshi_yes;

        let (spread, poly_side, kalshi_side, poly_price, kalshi_price) = if spread_a >= spread_b {
            (
                spread_a,
                Side::Yes,
                Side::No,
                prices.poly_yes,
                prices.kalshi_no,
            )
        } else {
            (
                spread_b,
                Side::No,
                Side::Yes,
                prices.poly_no,
                prices.kalshi_yes,
            )
        };

        // Fee-adjusted minimum: a spread must exceed base threshold + estimated fees
        let estimated_fees = self.fee_config.estimated_round_trip_fee(kalshi_price, poly_price);
        let effective_min_spread = self.min_spread_absolute + estimated_fees;
        if spread < effective_min_spread {
            return None;
        }

        let combined = poly_price + kalshi_price;
        if combined <= Decimal::ZERO {
            return None;
        }
        let spread_pct = (spread / combined) * dec!(100);
        if spread_pct < self.min_spread_pct {
            return None;
        }

        Some(Opportunity {
            id: Uuid::now_v7(),
            pair_id: pair.pair_id,
            poly_side,
            poly_price,
            poly_market_id: pair.poly_market_id.clone(),
            poly_yes_token_id: pair.poly_yes_token_id.clone(),
            kalshi_side,
            kalshi_price,
            kalshi_market_id: pair.kalshi_market_id.clone(),
            spread,
            spread_pct,
            max_quantity: 0,
            close_time: pair.close_time,
            detected_at: Utc::now(),
            status: OpportunityStatus::Detected,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fees::FeeConfig;
    use arb_types::{Platform, PriceUpdate};
    use chrono::Duration as CDur;
    use rust_decimal_macros::dec;

    fn cfg(pct: Decimal, abs: Decimal) -> EngineConfig {
        EngineConfig {
            scan_interval_ms: 1000,
            min_spread_pct: pct,
            min_spread_absolute: abs,
        }
    }

    /// Zero-fee config for tests that want to keep legacy behavior
    fn zero_fees() -> FeeConfig {
        FeeConfig {
            kalshi_taker_fee_pct: dec!(0),
            poly_taker_fee_pct: dec!(0),
        }
    }

    fn pair_info(pair_id: Uuid) -> PairInfo {
        PairInfo {
            pair_id,
            poly_market_id: "poly-tok".into(),
            kalshi_market_id: "KALSHI-T".into(),
            close_time: Utc::now() + CDur::days(30),
            verified: true,
            poly_yes_token_id: "poly-tok".into(),
            poly_no_token_id: "poly-tok-no".into(),
            volume: Decimal::ZERO,
        }
    }

    fn fill_cache(
        cache: &PriceCache,
        pair_id: Uuid,
        py: Decimal,
        pn: Decimal,
        ky: Decimal,
        kn: Decimal,
    ) {
        cache.register_pair(pair_id, "poly-tok", "KALSHI-T");
        cache.update(&PriceUpdate {
            platform: Platform::Polymarket,
            market_id: "poly-tok".into(),
            yes_price: py,
            no_price: pn,
            timestamp: Utc::now(),
        });
        cache.update(&PriceUpdate {
            platform: Platform::Kalshi,
            market_id: "KALSHI-T".into(),
            yes_price: ky,
            no_price: kn,
            timestamp: Utc::now(),
        });
    }

    #[test]
    fn test_detects_above_threshold() {
        let pid = Uuid::now_v7();
        let cache = Arc::new(PriceCache::new());
        fill_cache(&cache, pid, dec!(0.42), dec!(0.58), dec!(0.47), dec!(0.53));
        let d = Detector::new(cache, &cfg(dec!(3.0), dec!(0.02)), &zero_fees());
        let opps = d.scan(&[pair_info(pid)]);
        assert_eq!(opps.len(), 1);
        assert_eq!(opps[0].spread, dec!(0.05));
        assert_eq!(opps[0].poly_side, Side::Yes);
        assert_eq!(opps[0].poly_market_id, "poly-tok");
    }

    #[test]
    fn test_rejects_below_threshold() {
        let pid = Uuid::now_v7();
        let cache = Arc::new(PriceCache::new());
        fill_cache(&cache, pid, dec!(0.50), dec!(0.50), dec!(0.51), dec!(0.49));
        let d = Detector::new(cache, &cfg(dec!(3.0), dec!(0.02)), &zero_fees());
        assert!(d.scan(&[pair_info(pid)]).is_empty());
    }

    #[test]
    fn test_picks_best_direction() {
        let pid = Uuid::now_v7();
        let cache = Arc::new(PriceCache::new());
        fill_cache(&cache, pid, dec!(0.42), dec!(0.58), dec!(0.47), dec!(0.53));
        let d = Detector::new(cache, &cfg(dec!(3.0), dec!(0.02)), &zero_fees());
        let opps = d.scan(&[pair_info(pid)]);
        assert_eq!(opps[0].poly_side, Side::Yes);
    }

    #[test]
    fn test_skips_unverified() {
        let pid = Uuid::now_v7();
        let cache = Arc::new(PriceCache::new());
        fill_cache(&cache, pid, dec!(0.42), dec!(0.58), dec!(0.47), dec!(0.53));
        let d = Detector::new(cache, &cfg(dec!(3.0), dec!(0.02)), &zero_fees());
        let mut pi = pair_info(pid);
        pi.verified = false;
        assert!(d.scan(&[pi]).is_empty());
    }

    #[test]
    fn test_empty_pairs() {
        let cache = Arc::new(PriceCache::new());
        let d = Detector::new(cache, &cfg(dec!(3.0), dec!(0.02)), &zero_fees());
        assert!(d.scan(&[]).is_empty());
    }

    #[test]
    fn test_rejects_zero_kalshi_no_price() {
        // Regression: zero kalshi_no creates phantom spread = 1 - poly_yes - 0
        let pid = Uuid::now_v7();
        let cache = Arc::new(PriceCache::new());
        fill_cache(&cache, pid, dec!(0.50), dec!(0.50), dec!(0.48), dec!(0));
        let d = Detector::new(cache, &cfg(dec!(1.0), dec!(0.01)), &zero_fees());
        assert!(d.scan(&[pair_info(pid)]).is_empty());
    }

    #[test]
    fn test_rejects_zero_kalshi_yes_price() {
        let pid = Uuid::now_v7();
        let cache = Arc::new(PriceCache::new());
        fill_cache(&cache, pid, dec!(0.50), dec!(0.50), dec!(0), dec!(0.52));
        let d = Detector::new(cache, &cfg(dec!(1.0), dec!(0.01)), &zero_fees());
        assert!(d.scan(&[pair_info(pid)]).is_empty());
    }

    #[test]
    fn test_rejects_zero_poly_price() {
        let pid = Uuid::now_v7();
        let cache = Arc::new(PriceCache::new());
        fill_cache(&cache, pid, dec!(0), dec!(0.50), dec!(0.48), dec!(0.52));
        let d = Detector::new(cache, &cfg(dec!(1.0), dec!(0.01)), &zero_fees());
        assert!(d.scan(&[pair_info(pid)]).is_empty());
    }

    #[test]
    fn test_fee_adjusted_threshold_rejects_unprofitable() {
        // Spread of 5 cents looks good with 2 cent threshold,
        // but with 7% Kalshi fees on 0.53 price = 0.0371 per contract,
        // effective min = 0.02 + 0.0371 = 0.0571 > 0.05 spread → rejected
        let pid = Uuid::now_v7();
        let cache = Arc::new(PriceCache::new());
        fill_cache(&cache, pid, dec!(0.42), dec!(0.58), dec!(0.47), dec!(0.53));
        let fees = FeeConfig {
            kalshi_taker_fee_pct: dec!(7.0),
            poly_taker_fee_pct: dec!(0.0),
        };
        let d = Detector::new(cache, &cfg(dec!(3.0), dec!(0.02)), &fees);
        assert!(d.scan(&[pair_info(pid)]).is_empty());
    }

    #[test]
    fn test_rejects_near_expiry_pair() {
        // Pair closing in 3 hours (< 6h threshold) should be rejected
        let pid = Uuid::now_v7();
        let cache = Arc::new(PriceCache::new());
        fill_cache(&cache, pid, dec!(0.42), dec!(0.58), dec!(0.47), dec!(0.53));
        let d = Detector::new(cache, &cfg(dec!(3.0), dec!(0.02)), &zero_fees());
        let mut pi = pair_info(pid);
        pi.close_time = Utc::now() + CDur::hours(3); // Only 3h out — below 6h minimum
        assert!(d.scan(&[pi]).is_empty());
    }

    #[test]
    fn test_accepts_far_expiry_pair() {
        // Pair closing in 7 days (> 6h threshold) should be accepted if spread is good
        let pid = Uuid::now_v7();
        let cache = Arc::new(PriceCache::new());
        fill_cache(&cache, pid, dec!(0.42), dec!(0.58), dec!(0.47), dec!(0.53));
        let d = Detector::new(cache, &cfg(dec!(3.0), dec!(0.02)), &zero_fees());
        let mut pi = pair_info(pid);
        pi.close_time = Utc::now() + CDur::days(7);
        let opps = d.scan(&[pi]);
        assert_eq!(opps.len(), 1);
    }

    #[test]
    fn test_rejects_extreme_poly_yes_high() {
        // poly_yes = 0.96 (> 0.95) — market is essentially decided YES
        let pid = Uuid::now_v7();
        let cache = Arc::new(PriceCache::new());
        fill_cache(&cache, pid, dec!(0.96), dec!(0.04), dec!(0.90), dec!(0.10));
        let d = Detector::new(cache, &cfg(dec!(1.0), dec!(0.01)), &zero_fees());
        assert!(d.scan(&[pair_info(pid)]).is_empty());
    }

    #[test]
    fn test_rejects_extreme_kalshi_no_high() {
        // kalshi_no = 0.96 (> 0.95) — near-decided on Kalshi side
        let pid = Uuid::now_v7();
        let cache = Arc::new(PriceCache::new());
        fill_cache(&cache, pid, dec!(0.50), dec!(0.50), dec!(0.04), dec!(0.96));
        let d = Detector::new(cache, &cfg(dec!(1.0), dec!(0.01)), &zero_fees());
        assert!(d.scan(&[pair_info(pid)]).is_empty());
    }

    #[test]
    fn test_rejects_extreme_poly_yes_low() {
        // poly_yes = 0.04 (< 0.05) — market is essentially decided NO
        let pid = Uuid::now_v7();
        let cache = Arc::new(PriceCache::new());
        fill_cache(&cache, pid, dec!(0.04), dec!(0.96), dec!(0.10), dec!(0.90));
        let d = Detector::new(cache, &cfg(dec!(1.0), dec!(0.01)), &zero_fees());
        assert!(d.scan(&[pair_info(pid)]).is_empty());
    }

    #[test]
    fn test_accepts_moderate_prices() {
        // All prices in the safe range [0.10, 0.90] — should pass price filter
        let pid = Uuid::now_v7();
        let cache = Arc::new(PriceCache::new());
        fill_cache(&cache, pid, dec!(0.42), dec!(0.58), dec!(0.47), dec!(0.53));
        let d = Detector::new(cache, &cfg(dec!(3.0), dec!(0.02)), &zero_fees());
        let opps = d.scan(&[pair_info(pid)]);
        assert_eq!(opps.len(), 1);
    }
}
