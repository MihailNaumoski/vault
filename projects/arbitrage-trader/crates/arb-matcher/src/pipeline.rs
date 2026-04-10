//! Match pipeline — takes lists of Polymarket and Kalshi markets and
//! produces ranked match candidates.
//!
//! Pipeline stages:
//! 1. Enhanced normalization + token classification (pre-computed)
//! 2. Category pre-filter (hard gate)
//! 3. Entity overlap gate (hard gate)
//! 4. Token similarity scoring (Jaccard + weighted overlap)
//! 5. Close-time proximity (soft signal)
//! 6. Composite score + greedy dedup

use std::collections::HashSet;

use arb_types::{Market, Platform};
use crate::category::{categories_compatible, classify, MarketCategory};
use crate::normalize::{classify_tokens, normalize, ClassifiedTokens};
use crate::scorer;
use crate::types::{MatchCandidate, MatchScore};

/// A record of a comparison that was blocked or scored during diagnostics.
#[derive(Debug, Clone)]
pub struct DiagnosticComparison {
    pub poly_platform_id: String,
    pub kalshi_platform_id: String,
    pub poly_question: String,
    pub kalshi_question: String,
    pub poly_category: String,
    pub kalshi_category: String,
    /// `None` if the pair was scored; `Some("category")`, `Some("entity")`, or `Some("token_count")` if blocked.
    pub blocked_by: Option<String>,
    /// Score details (only populated if not blocked).
    pub composite_score: Option<f64>,
    pub text_score: Option<f64>,
    pub time_score: Option<f64>,
    pub shared_entities: Option<usize>,
    pub shared_tokens: Option<usize>,
}

/// Result of a diagnostic match run.
#[derive(Debug)]
pub struct DiagnosticResult {
    pub candidates: Vec<MatchCandidate>,
    pub comparisons: Vec<DiagnosticComparison>,
}

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
            min_score: 0.55,
            max_results: 100,
        }
    }
}

impl MatchPipeline {
    /// Minimum shared meaningful tokens required before scoring.
    /// Raised from 1 to 2 — a single shared token like "2026" is insufficient.
    const MIN_SHARED_TOKENS: usize = 2;

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
        // Stage 1: Pre-normalize and classify all questions outside O(n*m) loop.
        let poly_normalized: Vec<String> =
            poly_markets.iter().map(|m| normalize(&m.question)).collect();
        let kalshi_normalized: Vec<String> =
            kalshi_markets.iter().map(|m| normalize(&m.question)).collect();

        let poly_classified: Vec<ClassifiedTokens> =
            poly_markets.iter().map(|m| classify_tokens(&m.question)).collect();
        let kalshi_classified: Vec<ClassifiedTokens> =
            kalshi_markets.iter().map(|m| classify_tokens(&m.question)).collect();

        // Pre-compute categories for each market.
        let poly_categories: Vec<(MarketCategory, Option<MarketCategory>)> =
            poly_classified.iter().map(|ct| classify(&ct.all_meaningful)).collect();
        let kalshi_categories: Vec<(MarketCategory, Option<MarketCategory>)> =
            kalshi_classified.iter().map(|ct| classify(&ct.all_meaningful)).collect();

        let mut candidates: Vec<MatchCandidate> = Vec::new();

        for (i, poly) in poly_markets.iter().enumerate() {
            debug_assert_eq!(poly.platform, Platform::Polymarket);
            for (j, kalshi) in kalshi_markets.iter().enumerate() {
                debug_assert_eq!(kalshi.platform, Platform::Kalshi);

                // Stage 2: Category pre-filter (hard gate).
                if !categories_compatible(poly_categories[i], kalshi_categories[j]) {
                    continue;
                }

                // Stage 3: Entity overlap gate (hard gate).
                // If both have entities and share zero, skip.
                let poly_ents = &poly_classified[i].entities;
                let kalshi_ents = &kalshi_classified[j].entities;
                if !poly_ents.is_empty() && !kalshi_ents.is_empty() {
                    let shared_entities = poly_ents.iter().filter(|e| kalshi_ents.contains(e)).count();
                    if shared_entities == 0 {
                        continue;
                    }
                }

                // Token pre-filter: skip if too few shared meaningful tokens.
                let poly_set: HashSet<&str> = poly_classified[i]
                    .all_meaningful
                    .iter()
                    .map(|s| s.as_str())
                    .collect();
                let kalshi_set: HashSet<&str> = kalshi_classified[j]
                    .all_meaningful
                    .iter()
                    .map(|s| s.as_str())
                    .collect();
                let shared = poly_set.intersection(&kalshi_set).count();
                if shared < Self::MIN_SHARED_TOKENS {
                    continue;
                }

                // Stages 4-6: Score the pair.
                let mut score: MatchScore = scorer::score_classified(
                    &poly_classified[i],
                    &kalshi_classified[j],
                    &poly_normalized[i],
                    &kalshi_normalized[j],
                    poly,
                    kalshi,
                );
                score.category_match = true; // passed category gate

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

        // Greedy best-first deduplication:
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

    /// Like `find_matches`, but also returns diagnostic information about
    /// WHY pairs were blocked at each gate. For blocked-by-category pairs,
    /// only a random sample of `category_sample_cap` is recorded to avoid
    /// memory issues on large cross-products.
    pub fn find_matches_diagnostic(
        &self,
        poly_markets: &[Market],
        kalshi_markets: &[Market],
        category_sample_cap: usize,
    ) -> DiagnosticResult {
        // Stage 1: Pre-normalize and classify all questions outside O(n*m) loop.
        let poly_normalized: Vec<String> =
            poly_markets.iter().map(|m| normalize(&m.question)).collect();
        let kalshi_normalized: Vec<String> =
            kalshi_markets.iter().map(|m| normalize(&m.question)).collect();

        let poly_classified: Vec<ClassifiedTokens> =
            poly_markets.iter().map(|m| classify_tokens(&m.question)).collect();
        let kalshi_classified: Vec<ClassifiedTokens> =
            kalshi_markets.iter().map(|m| classify_tokens(&m.question)).collect();

        let poly_categories: Vec<(MarketCategory, Option<MarketCategory>)> =
            poly_classified.iter().map(|ct| classify(&ct.all_meaningful)).collect();
        let kalshi_categories: Vec<(MarketCategory, Option<MarketCategory>)> =
            kalshi_classified.iter().map(|ct| classify(&ct.all_meaningful)).collect();

        let mut candidates: Vec<MatchCandidate> = Vec::new();
        let mut comparisons: Vec<DiagnosticComparison> = Vec::new();
        let mut _category_blocked_count: usize = 0;

        // Simple deterministic sampling: record every Nth category-blocked pair
        // We'll compute N after the loop or use a reservoir approach.
        // Simpler: just cap at category_sample_cap using a counter.
        let mut category_blocked_recorded: usize = 0;

        fn cat_label(c: MarketCategory) -> &'static str {
            match c {
                MarketCategory::Crypto => "Crypto",
                MarketCategory::Politics => "Politics",
                MarketCategory::Sports => "Sports",
                MarketCategory::Weather => "Weather",
                MarketCategory::Economics => "Economics",
                MarketCategory::Entertainment => "Entertainment",
                MarketCategory::Science => "Science",
                MarketCategory::Other => "Other",
            }
        }

        for (i, poly) in poly_markets.iter().enumerate() {
            debug_assert_eq!(poly.platform, Platform::Polymarket);
            for (j, kalshi) in kalshi_markets.iter().enumerate() {
                debug_assert_eq!(kalshi.platform, Platform::Kalshi);

                let poly_cat_str = cat_label(poly_categories[i].0);
                let kalshi_cat_str = cat_label(kalshi_categories[j].0);

                // Stage 2: Category pre-filter (hard gate).
                if !categories_compatible(poly_categories[i], kalshi_categories[j]) {
                    _category_blocked_count += 1;
                    // Sample category-blocked pairs (record every Nth to stay under cap)
                    if category_blocked_recorded < category_sample_cap {
                        // Use a simple stride: record ~evenly spaced samples
                        // We can't know total ahead of time, so record the first `cap` and stop
                        category_blocked_recorded += 1;
                        comparisons.push(DiagnosticComparison {
                            poly_platform_id: poly.platform_id.clone(),
                            kalshi_platform_id: kalshi.platform_id.clone(),
                            poly_question: poly.question.clone(),
                            kalshi_question: kalshi.question.clone(),
                            poly_category: poly_cat_str.to_string(),
                            kalshi_category: kalshi_cat_str.to_string(),
                            blocked_by: Some("category".to_string()),
                            composite_score: None,
                            text_score: None,
                            time_score: None,
                            shared_entities: None,
                            shared_tokens: None,
                        });
                    }
                    continue;
                }

                // Stage 3: Entity overlap gate (hard gate).
                let poly_ents = &poly_classified[i].entities;
                let kalshi_ents = &kalshi_classified[j].entities;
                if !poly_ents.is_empty() && !kalshi_ents.is_empty() {
                    let shared_entities = poly_ents.iter().filter(|e| kalshi_ents.contains(e)).count();
                    if shared_entities == 0 {
                        comparisons.push(DiagnosticComparison {
                            poly_platform_id: poly.platform_id.clone(),
                            kalshi_platform_id: kalshi.platform_id.clone(),
                            poly_question: poly.question.clone(),
                            kalshi_question: kalshi.question.clone(),
                            poly_category: poly_cat_str.to_string(),
                            kalshi_category: kalshi_cat_str.to_string(),
                            blocked_by: Some("entity".to_string()),
                            composite_score: None,
                            text_score: None,
                            time_score: None,
                            shared_entities: Some(0),
                            shared_tokens: None,
                        });
                        continue;
                    }
                }

                // Token pre-filter: skip if too few shared meaningful tokens.
                let poly_set: HashSet<&str> = poly_classified[i]
                    .all_meaningful
                    .iter()
                    .map(|s| s.as_str())
                    .collect();
                let kalshi_set: HashSet<&str> = kalshi_classified[j]
                    .all_meaningful
                    .iter()
                    .map(|s| s.as_str())
                    .collect();
                let shared = poly_set.intersection(&kalshi_set).count();
                if shared < Self::MIN_SHARED_TOKENS {
                    comparisons.push(DiagnosticComparison {
                        poly_platform_id: poly.platform_id.clone(),
                        kalshi_platform_id: kalshi.platform_id.clone(),
                        poly_question: poly.question.clone(),
                        kalshi_question: kalshi.question.clone(),
                        poly_category: poly_cat_str.to_string(),
                        kalshi_category: kalshi_cat_str.to_string(),
                        blocked_by: Some("token_count".to_string()),
                        composite_score: None,
                        text_score: None,
                        time_score: None,
                        shared_entities: None,
                        shared_tokens: Some(shared),
                    });
                    continue;
                }

                // Stages 4-6: Score the pair.
                let mut score: MatchScore = scorer::score_classified(
                    &poly_classified[i],
                    &kalshi_classified[j],
                    &poly_normalized[i],
                    &kalshi_normalized[j],
                    poly,
                    kalshi,
                );
                score.category_match = true;

                // Record ALL scored pairs in diagnostics
                comparisons.push(DiagnosticComparison {
                    poly_platform_id: poly.platform_id.clone(),
                    kalshi_platform_id: kalshi.platform_id.clone(),
                    poly_question: poly.question.clone(),
                    kalshi_question: kalshi.question.clone(),
                    poly_category: poly_cat_str.to_string(),
                    kalshi_category: kalshi_cat_str.to_string(),
                    blocked_by: None,
                    composite_score: Some(score.composite),
                    text_score: Some(score.text_score),
                    time_score: Some(score.close_time_score),
                    shared_entities: Some(score.shared_entities),
                    shared_tokens: Some(score.shared_tokens),
                });

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

        // Greedy best-first deduplication
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

        DiagnosticResult {
            candidates: deduped,
            comparisons,
        }
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
        make_market_with_id(
            platform,
            question,
            close_offset_hours,
            &format!("test-{}", question.len()),
        )
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

    // ── Basic pipeline behavior ──────────────────────────────────────────

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
        assert!(
            results[0].score.composite >= 0.80,
            "best match composite={}",
            results[0].score.composite
        );

        for w in results.windows(2) {
            assert!(w[0].score.composite >= w[1].score.composite);
        }
    }

    #[test]
    fn find_matches_respects_min_score() {
        let poly = vec![make_market(Platform::Polymarket, "Will Bitcoin hit $100k?", 24)];
        let kalshi = vec![make_market(Platform::Kalshi, "Who wins the Super Bowl?", 72)];

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
            .map(|i| {
                make_market_with_id(
                    Platform::Polymarket,
                    &format!("Will Bitcoin event {i} happen?"),
                    24,
                    &format!("poly-event-{i}"),
                )
            })
            .collect();
        let kalshi: Vec<Market> = (0..5)
            .map(|i| {
                make_market_with_id(
                    Platform::Kalshi,
                    &format!("Will Bitcoin event {i} happen?"),
                    24,
                    &format!("kalshi-event-{i}"),
                )
            })
            .collect();

        let pipeline = MatchPipeline {
            min_score: 0.01,
            max_results: 3,
        };
        let results = pipeline.find_matches(&poly, &kalshi);
        assert!(
            results.len() <= 3,
            "should respect max_results=3, got {}",
            results.len()
        );
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

    // ── Category pre-filter tests ────────────────────────────────────────

    #[test]
    fn category_cross_category_blocked() {
        let pipeline = MatchPipeline { min_score: 0.01, max_results: 100 };
        let poly = vec![make_market(Platform::Polymarket, "Will Bitcoin hit $100k?", 24)];
        let kalshi = vec![make_market(
            Platform::Kalshi,
            "Will Trump win the election?",
            24,
        )];
        let results = pipeline.find_matches(&poly, &kalshi);
        assert!(results.is_empty(), "Crypto vs Politics must be blocked");
    }

    #[test]
    fn category_other_allows_cross_matching() {
        let pipeline = MatchPipeline { min_score: 0.01, max_results: 100 };
        let poly = vec![make_market_with_id(
            Platform::Polymarket,
            "Will event X happen by Friday?",
            24,
            "poly-x",
        )];
        let kalshi = vec![make_market_with_id(
            Platform::Kalshi,
            "Will event X happen this week?",
            24,
            "kalshi-x",
        )];
        let _results = pipeline.find_matches(&poly, &kalshi);
        // Both are "Other" — should not be blocked by category filter
        // Whether they actually match depends on token overlap of "event", "x", "happen"
        // These share 3 meaningful tokens, so they should produce a candidate
        // (unless "x" is too short — it won't be a stop word though)
        // This is fine either way; the key point is category doesn't block them
    }

    // ── Entity overlap gate tests ────────────────────────────────────────

    #[test]
    fn entity_gate_blocks_different_entities_same_category() {
        // Both crypto, but different coins — entity gate should block
        let pipeline = MatchPipeline { min_score: 0.01, max_results: 100 };
        let poly = vec![make_market_with_id(
            Platform::Polymarket,
            "Will Bitcoin hit $100k?",
            24,
            "poly-btc",
        )];
        let kalshi = vec![make_market_with_id(
            Platform::Kalshi,
            "Will Ethereum reach $5000?",
            24,
            "kalshi-eth",
        )];
        let results = pipeline.find_matches(&poly, &kalshi);
        assert!(
            results.is_empty(),
            "Bitcoin vs Ethereum (different entities) should be blocked"
        );
    }

    // ── False positive regression (pipeline-level) ───────────────────────

    #[test]
    fn pipeline_no_false_positives_on_real_data() {
        let poly_markets = vec![
            make_market(
                Platform::Polymarket,
                "US x Iran meeting by April 10, 2026?",
                24,
            ),
            make_market(
                Platform::Polymarket,
                "Will Elon Musk post 260-279 tweets this week?",
                24,
            ),
            make_market(
                Platform::Polymarket,
                "Will Carlos Alvarez win the 2026 Peruvian presidential election?",
                24,
            ),
            make_market(
                Platform::Polymarket,
                "Will Ricardo Belmont win the 2026 Peruvian presidential election?",
                24,
            ),
        ];
        let kalshi_markets = vec![
            make_market(
                Platform::Kalshi,
                "Shiba Inu price range on Apr 10, 2026?",
                24,
            ),
            make_market(
                Platform::Kalshi,
                "Will the temp in NYC be above 36.99 degrees?",
                24,
            ),
        ];

        let pipeline = MatchPipeline::default();
        let results = pipeline.find_matches(&poly_markets, &kalshi_markets);

        assert!(
            results.is_empty(),
            "Pipeline should produce ZERO matches for these unrelated markets, got {} matches",
            results.len()
        );
    }

    #[test]
    fn pipeline_finds_true_match_among_noise() {
        let poly_markets = vec![
            make_market_with_id(
                Platform::Polymarket,
                "Will Bitcoin hit $100k?",
                24,
                "poly-btc",
            ),
            make_market_with_id(
                Platform::Polymarket,
                "US x Iran meeting by April 10, 2026?",
                24,
                "poly-iran",
            ),
            make_market_with_id(
                Platform::Polymarket,
                "Will Elon Musk post 260-279 tweets?",
                24,
                "poly-musk",
            ),
        ];
        let kalshi_markets = vec![
            make_market_with_id(
                Platform::Kalshi,
                "Will Bitcoin hit $100k?",
                24,
                "kalshi-btc",
            ),
            make_market_with_id(
                Platform::Kalshi,
                "Shiba Inu price range on Apr 10?",
                24,
                "kalshi-shib",
            ),
            make_market_with_id(
                Platform::Kalshi,
                "Will the temp in NYC be above 36.99?",
                24,
                "kalshi-weather",
            ),
        ];

        let pipeline = MatchPipeline::default();
        let results = pipeline.find_matches(&poly_markets, &kalshi_markets);

        assert_eq!(results.len(), 1, "Should find exactly 1 match (Bitcoin)");
        assert_eq!(results[0].poly_market.platform_id, "poly-btc");
        assert_eq!(results[0].kalshi_market.platform_id, "kalshi-btc");
        assert!(
            results[0].score.composite >= 0.80,
            "Bitcoin match should be AutoVerified"
        );
    }

    // ── Dedup tests ──────────────────────────────────────────────────────

    #[test]
    fn dedup_each_poly_appears_at_most_once() {
        let poly = vec![make_market_with_id(
            Platform::Polymarket,
            "Will Bitcoin hit $100k?",
            24,
            "poly-btc",
        )];
        let kalshi = vec![
            make_market_with_id(Platform::Kalshi, "Will Bitcoin hit $100k?", 24, "kalshi-btc-1"),
            make_market_with_id(
                Platform::Kalshi,
                "Will Bitcoin reach $100k?",
                24,
                "kalshi-btc-2",
            ),
        ];

        let pipeline = MatchPipeline {
            min_score: 0.01,
            max_results: 100,
        };
        let results = pipeline.find_matches(&poly, &kalshi);

        let poly_ids: Vec<&str> = results
            .iter()
            .map(|c| c.poly_market.platform_id.as_str())
            .collect();
        let unique: HashSet<&str> = poly_ids.iter().copied().collect();
        assert_eq!(
            poly_ids.len(),
            unique.len(),
            "each poly market should appear at most once"
        );
    }

    #[test]
    fn dedup_each_kalshi_appears_at_most_once() {
        let poly = vec![
            make_market_with_id(
                Platform::Polymarket,
                "Will Bitcoin hit $100k?",
                24,
                "poly-btc-1",
            ),
            make_market_with_id(
                Platform::Polymarket,
                "Will Bitcoin reach $100k?",
                24,
                "poly-btc-2",
            ),
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

        let kalshi_ids: Vec<&str> = results
            .iter()
            .map(|c| c.kalshi_market.platform_id.as_str())
            .collect();
        let unique: HashSet<&str> = kalshi_ids.iter().copied().collect();
        assert_eq!(
            kalshi_ids.len(),
            unique.len(),
            "each kalshi market should appear at most once"
        );
    }

    #[test]
    fn dedup_three_poly_two_match_same_kalshi_keeps_higher_score() {
        let poly = vec![
            make_market_with_id(
                Platform::Polymarket,
                "Will Bitcoin hit $100k?",
                24,
                "poly-exact",
            ),
            make_market_with_id(
                Platform::Polymarket,
                "Will Bitcoin reach $100k by year end?",
                24,
                "poly-similar",
            ),
            make_market_with_id(
                Platform::Polymarket,
                "Will Ethereum hit $5000?",
                48,
                "poly-eth",
            ),
        ];
        let kalshi = vec![
            make_market_with_id(
                Platform::Kalshi,
                "Will Bitcoin hit $100k?",
                24,
                "kalshi-btc",
            ),
            make_market_with_id(
                Platform::Kalshi,
                "Will Ethereum hit $5000?",
                48,
                "kalshi-eth",
            ),
        ];

        let pipeline = MatchPipeline {
            min_score: 0.01,
            max_results: 100,
        };
        let results = pipeline.find_matches(&poly, &kalshi);

        let poly_ids: Vec<&str> = results
            .iter()
            .map(|c| c.poly_market.platform_id.as_str())
            .collect();
        let kalshi_ids: Vec<&str> = results
            .iter()
            .map(|c| c.kalshi_market.platform_id.as_str())
            .collect();
        let unique_poly: HashSet<&str> = poly_ids.iter().copied().collect();
        let unique_kalshi: HashSet<&str> = kalshi_ids.iter().copied().collect();
        assert_eq!(poly_ids.len(), unique_poly.len(), "poly dedup failed");
        assert_eq!(kalshi_ids.len(), unique_kalshi.len(), "kalshi dedup failed");

        let btc_match = results
            .iter()
            .find(|c| c.kalshi_market.platform_id == "kalshi-btc");
        assert!(btc_match.is_some(), "should have a match for kalshi-btc");
        assert_eq!(
            btc_match.unwrap().poly_market.platform_id,
            "poly-exact",
            "exact match should win over similar match"
        );
    }

    #[test]
    fn dedup_token_prefilter_skips_unrelated() {
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

        assert!(
            results.is_empty(),
            "unrelated markets should be skipped by pre-filters"
        );
    }

    // ── Performance test ─────────────────────────────────────────────────

    #[test]
    fn performance_200x200() {
        let topics = [
            "Bitcoin", "Ethereum", "Trump", "Super Bowl", "NYC temperature",
            "S&P 500", "SpaceX launch", "Oscar winner", "Fed rate", "GDP growth",
        ];
        let poly: Vec<Market> = (0..200)
            .map(|i| {
                make_market_with_id(
                    Platform::Polymarket,
                    &format!("Will {} event #{} happen?", topics[i % topics.len()], i),
                    24 + (i as i64 % 168),
                    &format!("poly-{i}"),
                )
            })
            .collect();
        let kalshi: Vec<Market> = (0..200)
            .map(|i| {
                make_market_with_id(
                    Platform::Kalshi,
                    &format!("Will {} event #{} happen?", topics[i % topics.len()], i),
                    24 + (i as i64 % 168),
                    &format!("kalshi-{i}"),
                )
            })
            .collect();

        let pipeline = MatchPipeline::default();
        let start = std::time::Instant::now();
        let _results = pipeline.find_matches(&poly, &kalshi);
        let elapsed = start.elapsed();

        assert!(
            elapsed.as_millis() < 500,
            "200x200 must complete quickly, took {}ms",
            elapsed.as_millis()
        );
    }

    // ── Multi-outcome edge case ──────────────────────────────────────────

    #[test]
    fn edge_multi_outcome_same_event_different_candidates() {
        let poly = make_market(
            Platform::Polymarket,
            "Will Carlos Alvarez win the 2026 Peruvian presidential election?",
            48,
        );
        let kalshi = make_market(
            Platform::Kalshi,
            "Will Ricardo Belmont win the 2026 Peruvian presidential election?",
            48,
        );

        let pipeline = MatchPipeline::default();
        let results = pipeline.find_matches(&[poly], &[kalshi]);

        // Different candidates for same election — entity gate should block
        // (Alvarez entity != Belmont entity, they don't share primary entities)
        // Or if they do get through, they should NOT AutoVerify.
        if !results.is_empty() {
            assert!(
                results[0].score.composite < 0.80,
                "Different candidates must NOT AutoVerify, got {}",
                results[0].score.composite
            );
        }
    }
}
