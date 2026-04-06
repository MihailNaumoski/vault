//! Match pipeline — takes lists of Polymarket and Kalshi markets and
//! produces ranked match candidates.

use std::collections::HashSet;

use arb_types::{Market, Platform};
use crate::normalize::{extract_tokens, normalize};
use crate::scorer;
use crate::types::{MatchCandidate, MatchScore};

/// Configuration for the match pipeline.
#[derive(Debug, Clone)]
pub struct MatchPipeline {
    /// Minimum composite score to include a candidate.
    pub min_score: f64,
    /// Maximum number of candidates to return.
    pub max_results: usize,
}

impl Default for MatchPipeline {
    fn default() -> Self {
        Self {
            min_score: 0.50,
            max_results: 100,
        }
    }
}

impl MatchPipeline {
    /// Minimum shared meaningful tokens required before running the expensive
    /// Jaro-Winkler scorer. Pairs with zero shared tokens are almost certainly
    /// unrelated and can be skipped.
    const MIN_SHARED_TOKENS: usize = 1;

    /// Find the best matches between two sets of markets.
    ///
    /// Returns candidates sorted by composite score (highest first), filtered
    /// by `min_score` and capped at `max_results`.
    ///
    /// Each Polymarket market maps to **at most one** Kalshi market (the
    /// highest-scoring one) and each Kalshi market is used at most once
    /// (greedy best-first assignment).
    pub fn find_matches(
        &self,
        poly_markets: &[Market],
        kalshi_markets: &[Market],
    ) -> Vec<MatchCandidate> {
        // Pre-normalize questions outside the O(n×m) loop (IMPROVEMENT 1).
        let poly_normalized: Vec<String> =
            poly_markets.iter().map(|m| normalize(&m.question)).collect();
        let kalshi_normalized: Vec<String> =
            kalshi_markets.iter().map(|m| normalize(&m.question)).collect();

        // Pre-extract tokens for the shared-token pre-filter (IMPROVEMENT 2).
        let poly_tokens: Vec<Vec<String>> =
            poly_markets.iter().map(|m| extract_tokens(&m.question)).collect();
        let kalshi_tokens: Vec<Vec<String>> =
            kalshi_markets.iter().map(|m| extract_tokens(&m.question)).collect();

        let mut candidates: Vec<MatchCandidate> = Vec::new();

        for (i, poly) in poly_markets.iter().enumerate() {
            debug_assert_eq!(poly.platform, Platform::Polymarket);
            for (j, kalshi) in kalshi_markets.iter().enumerate() {
                debug_assert_eq!(kalshi.platform, Platform::Kalshi);

                // Token pre-filter: skip obviously unrelated pairs.
                let shared = poly_tokens[i]
                    .iter()
                    .filter(|t| kalshi_tokens[j].contains(t))
                    .count();
                if shared < Self::MIN_SHARED_TOKENS {
                    continue;
                }

                let score: MatchScore = scorer::score_normalized(
                    &poly_normalized[i],
                    &kalshi_normalized[j],
                    poly,
                    kalshi,
                );
                if score.composite >= self.min_score {
                    candidates.push(MatchCandidate {
                        poly_market: poly.clone(),
                        kalshi_market: kalshi.clone(),
                        score,
                    });
                }
            }
        }

        // Sort descending by composite score.
        candidates.sort_by(|a, b| {
            b.score
                .composite
                .partial_cmp(&a.score.composite)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Greedy best-first deduplication (BUG 1 fix):
        // Each poly market maps to at most one kalshi market, and vice versa.
        let mut used_poly: HashSet<String> = HashSet::new();
        let mut used_kalshi: HashSet<String> = HashSet::new();
        let mut deduped: Vec<MatchCandidate> = Vec::new();

        for candidate in candidates {
            if used_poly.contains(&candidate.poly_market.platform_id)
                || used_kalshi.contains(&candidate.kalshi_market.platform_id)
            {
                continue;
            }
            used_poly.insert(candidate.poly_market.platform_id.clone());
            used_kalshi.insert(candidate.kalshi_market.platform_id.clone());
            deduped.push(candidate);
        }

        deduped.truncate(self.max_results);
        deduped
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use arb_types::{MarketId, MarketStatus};
    use chrono::{Duration, Utc};
    use rust_decimal_macros::dec;

    fn make_market(platform: Platform, question: &str, close_offset_hours: i64) -> Market {
        make_market_with_id(platform, question, close_offset_hours, &format!("test-{}", question.len()))
    }

    fn make_market_with_id(
        platform: Platform,
        question: &str,
        close_offset_hours: i64,
        platform_id: &str,
    ) -> Market {
        Market {
            id: MarketId::new(),
            platform,
            platform_id: platform_id.to_string(),
            question: question.to_string(),
            yes_price: dec!(0.50),
            no_price: dec!(0.50),
            volume: dec!(10000),
            liquidity: dec!(5000),
            status: MarketStatus::Open,
            close_time: Utc::now() + Duration::hours(close_offset_hours),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn find_matches_returns_sorted_candidates() {
        let poly = vec![
            make_market(Platform::Polymarket, "Will Bitcoin hit $100k?", 24),
            make_market(Platform::Polymarket, "Will ETH reach $5000?", 48),
        ];
        let kalshi = vec![
            make_market(Platform::Kalshi, "Will Bitcoin hit $100k?", 24),
            make_market(Platform::Kalshi, "Who wins the Super Bowl?", 72),
        ];

        let pipeline = MatchPipeline::default();
        let results = pipeline.find_matches(&poly, &kalshi);

        assert!(!results.is_empty(), "should find at least one match");
        // First result should be the identical Bitcoin question
        assert!(
            results[0].score.composite >= 0.90,
            "best match composite={}",
            results[0].score.composite
        );

        // Results should be sorted descending
        for w in results.windows(2) {
            assert!(w[0].score.composite >= w[1].score.composite);
        }
    }

    #[test]
    fn find_matches_respects_min_score() {
        let poly = vec![make_market(
            Platform::Polymarket,
            "Will Bitcoin hit $100k?",
            24,
        )];
        let kalshi = vec![make_market(
            Platform::Kalshi,
            "Who wins the Super Bowl?",
            72,
        )];

        let pipeline = MatchPipeline {
            min_score: 0.90,
            max_results: 100,
        };
        let results = pipeline.find_matches(&poly, &kalshi);
        assert!(results.is_empty(), "no match should pass 0.90 threshold");
    }

    #[test]
    fn find_matches_respects_max_results() {
        let poly: Vec<Market> = (0..5)
            .map(|i| make_market_with_id(Platform::Polymarket, &format!("Will event {i} happen?"), 24, &format!("poly-event-{i}")))
            .collect();
        let kalshi: Vec<Market> = (0..5)
            .map(|i| make_market_with_id(Platform::Kalshi, &format!("Will event {i} happen?"), 24, &format!("kalshi-event-{i}")))
            .collect();

        let pipeline = MatchPipeline {
            min_score: 0.01,
            max_results: 3,
        };
        let results = pipeline.find_matches(&poly, &kalshi);
        assert!(results.len() <= 3, "should respect max_results=3, got {}", results.len());
    }

    #[test]
    fn find_matches_empty_inputs() {
        let pipeline = MatchPipeline::default();
        assert!(pipeline.find_matches(&[], &[]).is_empty());
        let poly = vec![make_market(Platform::Polymarket, "test?", 24)];
        assert!(pipeline.find_matches(&poly, &[]).is_empty());
        let kalshi = vec![make_market(Platform::Kalshi, "test?", 24)];
        assert!(pipeline.find_matches(&[], &kalshi).is_empty());
    }

    #[test]
    fn performance_many_markets() {
        let poly: Vec<Market> = (0..50)
            .map(|i| make_market_with_id(Platform::Polymarket, &format!("Poly question number {i}"), 24, &format!("poly-perf-{i}")))
            .collect();
        let kalshi: Vec<Market> = (0..50)
            .map(|i| make_market_with_id(Platform::Kalshi, &format!("Kalshi question number {i}"), 24, &format!("kalshi-perf-{i}")))
            .collect();

        let pipeline = MatchPipeline::default();
        let start = std::time::Instant::now();
        let results = pipeline.find_matches(&poly, &kalshi);
        let elapsed = start.elapsed();

        assert!(
            elapsed.as_secs() < 5,
            "50x50 matching took too long: {:?}",
            elapsed
        );
        // All 2500 comparisons should produce at least some candidates
        assert!(!results.is_empty());
    }

    #[test]
    fn dedup_each_poly_appears_at_most_once() {
        // One poly market that matches two different kalshi markets.
        let poly = vec![make_market_with_id(
            Platform::Polymarket,
            "Will Bitcoin hit $100k?",
            24,
            "poly-btc",
        )];
        let kalshi = vec![
            make_market_with_id(Platform::Kalshi, "Will Bitcoin hit $100k?", 24, "kalshi-btc-1"),
            make_market_with_id(Platform::Kalshi, "Will Bitcoin reach $100k?", 24, "kalshi-btc-2"),
        ];

        let pipeline = MatchPipeline {
            min_score: 0.01,
            max_results: 100,
        };
        let results = pipeline.find_matches(&poly, &kalshi);

        let poly_ids: Vec<&str> = results.iter().map(|c| c.poly_market.platform_id.as_str()).collect();
        let unique: HashSet<&str> = poly_ids.iter().copied().collect();
        assert_eq!(poly_ids.len(), unique.len(), "each poly market should appear at most once");
    }

    #[test]
    fn dedup_each_kalshi_appears_at_most_once() {
        // Two poly markets that both match the same kalshi market.
        let poly = vec![
            make_market_with_id(Platform::Polymarket, "Will Bitcoin hit $100k?", 24, "poly-btc-1"),
            make_market_with_id(Platform::Polymarket, "Will Bitcoin reach $100k?", 24, "poly-btc-2"),
        ];
        let kalshi = vec![make_market_with_id(
            Platform::Kalshi,
            "Will Bitcoin hit $100k?",
            24,
            "kalshi-btc",
        )];

        let pipeline = MatchPipeline {
            min_score: 0.01,
            max_results: 100,
        };
        let results = pipeline.find_matches(&poly, &kalshi);

        let kalshi_ids: Vec<&str> = results.iter().map(|c| c.kalshi_market.platform_id.as_str()).collect();
        let unique: HashSet<&str> = kalshi_ids.iter().copied().collect();
        assert_eq!(kalshi_ids.len(), unique.len(), "each kalshi market should appear at most once");
    }

    #[test]
    fn dedup_three_poly_two_match_same_kalshi_keeps_higher_score() {
        // 3 poly markets, 2 of which match the same kalshi market.
        // The one with the higher score (exact match) should win.
        let poly = vec![
            make_market_with_id(Platform::Polymarket, "Will Bitcoin hit $100k?", 24, "poly-exact"),
            make_market_with_id(Platform::Polymarket, "Will Bitcoin reach $100k by year end?", 24, "poly-similar"),
            make_market_with_id(Platform::Polymarket, "Will Ethereum hit $5000?", 48, "poly-eth"),
        ];
        let kalshi = vec![
            make_market_with_id(Platform::Kalshi, "Will Bitcoin hit $100k?", 24, "kalshi-btc"),
            make_market_with_id(Platform::Kalshi, "Will Ethereum hit $5000?", 48, "kalshi-eth"),
        ];

        let pipeline = MatchPipeline {
            min_score: 0.01,
            max_results: 100,
        };
        let results = pipeline.find_matches(&poly, &kalshi);

        // Each platform_id should appear at most once.
        let poly_ids: Vec<&str> = results.iter().map(|c| c.poly_market.platform_id.as_str()).collect();
        let kalshi_ids: Vec<&str> = results.iter().map(|c| c.kalshi_market.platform_id.as_str()).collect();
        let unique_poly: HashSet<&str> = poly_ids.iter().copied().collect();
        let unique_kalshi: HashSet<&str> = kalshi_ids.iter().copied().collect();
        assert_eq!(poly_ids.len(), unique_poly.len(), "poly dedup failed");
        assert_eq!(kalshi_ids.len(), unique_kalshi.len(), "kalshi dedup failed");

        // The exact match ("Will Bitcoin hit $100k?" on both sides) should
        // beat the "similar" poly for kalshi-btc.
        let btc_match = results.iter().find(|c| c.kalshi_market.platform_id == "kalshi-btc");
        assert!(btc_match.is_some(), "should have a match for kalshi-btc");
        assert_eq!(
            btc_match.unwrap().poly_market.platform_id,
            "poly-exact",
            "exact match should win over similar match"
        );
    }

    #[test]
    fn dedup_token_prefilter_skips_unrelated() {
        // Two completely unrelated markets should not match even at low threshold,
        // because the token pre-filter should skip them.
        let poly = vec![make_market_with_id(
            Platform::Polymarket,
            "Will Bitcoin hit $100k?",
            24,
            "poly-btc",
        )];
        let kalshi = vec![make_market_with_id(
            Platform::Kalshi,
            "Who wins the Super Bowl?",
            72,
            "kalshi-nfl",
        )];

        let pipeline = MatchPipeline {
            min_score: 0.01,
            max_results: 100,
        };
        let results = pipeline.find_matches(&poly, &kalshi);

        // These share zero meaningful tokens, so the pre-filter should skip them.
        assert!(results.is_empty(), "unrelated markets should be skipped by token pre-filter");
    }
}
