use arb_types::{Platform, PriceUpdate};
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::time::Duration;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct PricePair {
    pub poly_yes: Decimal,
    pub poly_no: Decimal,
    pub kalshi_yes: Decimal,
    pub kalshi_no: Decimal,
    pub poly_updated: DateTime<Utc>,
    pub kalshi_updated: DateTime<Utc>,
}

pub struct PriceCache {
    prices: RwLock<HashMap<Uuid, PricePair>>,
    market_to_pair: RwLock<HashMap<(Platform, String), Uuid>>,
}

impl PriceCache {
    pub fn new() -> Self {
        Self {
            prices: RwLock::new(HashMap::new()),
            market_to_pair: RwLock::new(HashMap::new()),
        }
    }

    pub fn register_pair(&self, pair_id: Uuid, poly_market_id: &str, kalshi_market_id: &str) {
        let mut mapping = self.market_to_pair.write();
        mapping.insert(
            (Platform::Polymarket, poly_market_id.to_string()),
            pair_id,
        );
        mapping.insert((Platform::Kalshi, kalshi_market_id.to_string()), pair_id);

        // Seed initial entry so TUI shows pairs immediately (with zero prices until first update)
        let mut prices = self.prices.write();
        prices.entry(pair_id).or_insert_with(|| PricePair {
            poly_yes: Decimal::ZERO,
            poly_no: Decimal::ZERO,
            kalshi_yes: Decimal::ZERO,
            kalshi_no: Decimal::ZERO,
            poly_updated: Utc::now(),
            kalshi_updated: Utc::now(),
        });
    }

    pub fn update(&self, update: &PriceUpdate) -> Option<Uuid> {
        let pair_id = {
            let mapping = self.market_to_pair.read();
            mapping
                .get(&(update.platform, update.market_id.clone()))
                .copied()?
        };

        let mut prices = self.prices.write();
        let entry = prices.entry(pair_id).or_insert_with(|| PricePair {
            poly_yes: Decimal::ZERO,
            poly_no: Decimal::ZERO,
            kalshi_yes: Decimal::ZERO,
            kalshi_no: Decimal::ZERO,
            poly_updated: update.timestamp,
            kalshi_updated: update.timestamp,
        });

        match update.platform {
            Platform::Polymarket => {
                entry.poly_yes = update.yes_price;
                entry.poly_no = update.no_price;
                entry.poly_updated = update.timestamp;
            }
            Platform::Kalshi => {
                entry.kalshi_yes = update.yes_price;
                entry.kalshi_no = update.no_price;
                entry.kalshi_updated = update.timestamp;
            }
        }
        Some(pair_id)
    }

    pub fn get(&self, pair_id: &Uuid) -> Option<PricePair> {
        self.prices.read().get(pair_id).cloned()
    }

    pub fn is_fresh(&self, pair_id: &Uuid, max_age: Duration) -> bool {
        let prices = self.prices.read();
        let Some(pp) = prices.get(pair_id) else {
            return false;
        };
        let now = Utc::now();
        let max_chrono =
            chrono::Duration::from_std(max_age).unwrap_or(chrono::Duration::seconds(60));
        (now - pp.poly_updated) < max_chrono && (now - pp.kalshi_updated) < max_chrono
    }
}

impl Default for PriceCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn make_update(
        platform: Platform,
        market_id: &str,
        yes: Decimal,
        no: Decimal,
    ) -> PriceUpdate {
        PriceUpdate {
            platform,
            market_id: market_id.into(),
            yes_price: yes,
            no_price: no,
            timestamp: Utc::now(),
        }
    }

    #[test]
    fn test_register_and_update() {
        let cache = PriceCache::new();
        let pid = Uuid::now_v7();
        cache.register_pair(pid, "poly-1", "kalshi-1");
        let r = cache.update(&make_update(
            Platform::Polymarket,
            "poly-1",
            dec!(0.42),
            dec!(0.58),
        ));
        assert_eq!(r, Some(pid));
        let pp = cache.get(&pid).unwrap();
        assert_eq!(pp.poly_yes, dec!(0.42));
    }

    #[test]
    fn test_unknown_market() {
        let cache = PriceCache::new();
        assert!(cache
            .update(&make_update(
                Platform::Polymarket,
                "unknown",
                dec!(0.5),
                dec!(0.5)
            ))
            .is_none());
    }

    #[test]
    fn test_both_platforms() {
        let cache = PriceCache::new();
        let pid = Uuid::now_v7();
        cache.register_pair(pid, "p1", "k1");
        cache.update(&make_update(
            Platform::Polymarket,
            "p1",
            dec!(0.42),
            dec!(0.58),
        ));
        cache.update(&make_update(
            Platform::Kalshi,
            "k1",
            dec!(0.47),
            dec!(0.53),
        ));
        let pp = cache.get(&pid).unwrap();
        assert_eq!(pp.poly_yes, dec!(0.42));
        assert_eq!(pp.kalshi_yes, dec!(0.47));
    }

    #[test]
    fn test_is_fresh() {
        let cache = PriceCache::new();
        let pid = Uuid::now_v7();
        cache.register_pair(pid, "p", "k");
        cache.update(&make_update(
            Platform::Polymarket,
            "p",
            dec!(0.5),
            dec!(0.5),
        ));
        cache.update(&make_update(Platform::Kalshi, "k", dec!(0.5), dec!(0.5)));
        assert!(cache.is_fresh(&pid, Duration::from_secs(60)));
    }

    #[test]
    fn test_not_fresh_missing() {
        let cache = PriceCache::new();
        assert!(!cache.is_fresh(&Uuid::now_v7(), Duration::from_secs(60)));
    }
}
