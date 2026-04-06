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
    max_staleness: Duration,
}

impl Detector {
    pub fn new(price_cache: Arc<PriceCache>, config: &EngineConfig) -> Self {
        Self {
            price_cache,
            min_spread_pct: config.min_spread_pct,
            min_spread_absolute: config.min_spread_absolute,
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

        if spread < self.min_spread_absolute {
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
        let d = Detector::new(cache, &cfg(dec!(3.0), dec!(0.02)));
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
        let d = Detector::new(cache, &cfg(dec!(3.0), dec!(0.02)));
        assert!(d.scan(&[pair_info(pid)]).is_empty());
    }

    #[test]
    fn test_picks_best_direction() {
        let pid = Uuid::now_v7();
        let cache = Arc::new(PriceCache::new());
        fill_cache(&cache, pid, dec!(0.42), dec!(0.58), dec!(0.47), dec!(0.53));
        let d = Detector::new(cache, &cfg(dec!(3.0), dec!(0.02)));
        let opps = d.scan(&[pair_info(pid)]);
        assert_eq!(opps[0].poly_side, Side::Yes);
    }

    #[test]
    fn test_skips_unverified() {
        let pid = Uuid::now_v7();
        let cache = Arc::new(PriceCache::new());
        fill_cache(&cache, pid, dec!(0.42), dec!(0.58), dec!(0.47), dec!(0.53));
        let d = Detector::new(cache, &cfg(dec!(3.0), dec!(0.02)));
        let mut pi = pair_info(pid);
        pi.verified = false;
        assert!(d.scan(&[pi]).is_empty());
    }

    #[test]
    fn test_empty_pairs() {
        let cache = Arc::new(PriceCache::new());
        let d = Detector::new(cache, &cfg(dec!(3.0), dec!(0.02)));
        assert!(d.scan(&[]).is_empty());
    }
}
