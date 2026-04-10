//! Scoring engine for market match candidates.
//!
//! Combines token-based text similarity (Jaccard + weighted overlap) with
//! close-time proximity and a Jaro-Winkler tiebreaker into a weighted composite.

use std::collections::HashSet;

use arb_types::Market;
use crate::normalize::{classify_tokens, normalize, ClassifiedTokens};
use crate::types::MatchScore;

/// Composite formula weights.
const TEXT_WEIGHT: f64 = 0.65;
const TIME_WEIGHT: f64 = 0.25;
const JW_WEIGHT: f64 = 0.10;

/// Token class weights for weighted overlap calculation.
const ENTITY_WEIGHT: f64 = 3.0;
const KEYWORD_WEIGHT: f64 = 2.0;
const NUMBER_WEIGHT: f64 = 1.5;
const DATE_WEIGHT: f64 = 0.5;

/// Text score sub-weights.
const WEIGHTED_OVERLAP_SUB: f64 = 0.60;
const JACCARD_SUB: f64 = 0.40;

/// Maximum time difference (in hours) that still earns a non-zero time score.
const MAX_TIME_DIFF_HOURS: f64 = 168.0; // 7 days

/// Minimum shared meaningful tokens for a non-zero text score.
const MIN_SHARED_TOKENS: usize = 2;

/// Minimum composite score to be considered a candidate at all.
const MIN_CANDIDATE_SCORE: f64 = 0.55;

// ---------------------------------------------------------------------------
// Public scoring API
// ---------------------------------------------------------------------------

/// Score a pair of markets for match quality.
///
/// Convenience wrapper that normalizes and classifies both questions.
pub fn score(poly: &Market, kalshi: &Market) -> MatchScore {
    let poly_ct = classify_tokens(&poly.question);
    let kalshi_ct = classify_tokens(&kalshi.question);
    let norm_poly = normalize(&poly.question);
    let norm_kalshi = normalize(&kalshi.question);
    score_classified(&poly_ct, &kalshi_ct, &norm_poly, &norm_kalshi, poly, kalshi)
}

/// Score a pair of markets using pre-classified tokens and normalized strings.
///
/// Use this in the pipeline hot loop where classification is done once per market
/// outside the O(n*m) loop.
pub fn score_classified(
    poly_ct: &ClassifiedTokens,
    kalshi_ct: &ClassifiedTokens,
    norm_poly: &str,
    norm_kalshi: &str,
    poly: &Market,
    kalshi: &Market,
) -> MatchScore {
    // Count shared meaningful tokens
    let poly_set: HashSet<&str> = poly_ct.all_meaningful.iter().map(|s| s.as_str()).collect();
    let kalshi_set: HashSet<&str> = kalshi_ct.all_meaningful.iter().map(|s| s.as_str()).collect();
    let shared_tokens = poly_set.intersection(&kalshi_set).count();

    // Entity overlap
    let shared_entities = poly_ct
        .entities
        .iter()
        .filter(|e| kalshi_ct.entities.contains(e))
        .count();

    // Jaccard similarity
    let jaccard = jaccard_similarity(&poly_ct.all_meaningful, &kalshi_ct.all_meaningful);

    // Weighted overlap
    let weighted_ovlp = weighted_overlap(poly_ct, kalshi_ct);

    // Text score: enforce minimum shared tokens
    let text_score = if shared_tokens < MIN_SHARED_TOKENS {
        0.0
    } else {
        WEIGHTED_OVERLAP_SUB * weighted_ovlp + JACCARD_SUB * jaccard
    };

    // Jaro-Winkler tiebreaker on full normalized strings
    let jaro_winkler = if norm_poly.is_empty() || norm_kalshi.is_empty() {
        0.0
    } else {
        strsim::jaro_winkler(norm_poly, norm_kalshi)
    };

    // Close-time proximity
    let time_diff_hours = (poly.close_time - kalshi.close_time)
        .num_seconds()
        .unsigned_abs() as f64
        / 3600.0;
    let close_time_score = if time_diff_hours >= MAX_TIME_DIFF_HOURS {
        0.0
    } else {
        1.0 - (time_diff_hours / MAX_TIME_DIFF_HOURS)
    };

    // Final composite
    let composite = TEXT_WEIGHT * text_score + TIME_WEIGHT * close_time_score + JW_WEIGHT * jaro_winkler;

    MatchScore {
        jaccard,
        weighted_overlap: weighted_ovlp,
        text_score,
        text_similarity: text_score, // backward-compat alias
        jaro_winkler,
        close_time_score,
        composite,
        shared_entities,
        shared_tokens,
        category_match: true, // set by pipeline; default true for direct scoring
    }
}

/// Whether this score is high enough to be considered a candidate.
pub fn is_candidate(score: &MatchScore) -> bool {
    score.composite >= MIN_CANDIDATE_SCORE
}

// ---------------------------------------------------------------------------
// Jaccard similarity
// ---------------------------------------------------------------------------

/// Jaccard similarity on two token lists (as sets).
/// Returns |intersection| / |union|, or 0.0 if both are empty.
pub fn jaccard_similarity(a: &[String], b: &[String]) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 0.0;
    }
    let set_a: HashSet<&str> = a.iter().map(|s| s.as_str()).collect();
    let set_b: HashSet<&str> = b.iter().map(|s| s.as_str()).collect();
    let intersection = set_a.intersection(&set_b).count();
    let union = set_a.union(&set_b).count();
    if union == 0 {
        return 0.0;
    }
    intersection as f64 / union as f64
}

// ---------------------------------------------------------------------------
// Weighted token overlap
// ---------------------------------------------------------------------------

/// Weighted overlap: entity matches count 3x, keywords 2x, numbers 1.5x, dates 0.5x.
pub fn weighted_overlap(a: &ClassifiedTokens, b: &ClassifiedTokens) -> f64 {
    let mut shared_weight = 0.0;
    let mut total_weight = 0.0;

    accumulate_weight(&a.entities, &b.entities, ENTITY_WEIGHT, &mut shared_weight, &mut total_weight);
    accumulate_weight(&a.keywords, &b.keywords, KEYWORD_WEIGHT, &mut shared_weight, &mut total_weight);
    accumulate_weight(&a.numbers, &b.numbers, NUMBER_WEIGHT, &mut shared_weight, &mut total_weight);
    accumulate_weight(&a.dates, &b.dates, DATE_WEIGHT, &mut shared_weight, &mut total_weight);

    if total_weight == 0.0 {
        0.0
    } else {
        shared_weight / total_weight
    }
}

fn accumulate_weight(
    a_tokens: &[String],
    b_tokens: &[String],
    weight: f64,
    shared: &mut f64,
    total: &mut f64,
) {
    let set_a: HashSet<&str> = a_tokens.iter().map(|s| s.as_str()).collect();
    let set_b: HashSet<&str> = b_tokens.iter().map(|s| s.as_str()).collect();
    let intersection = set_a.intersection(&set_b).count();
    let union = set_a.union(&set_b).count();
    *shared += intersection as f64 * weight;
    *total += union as f64 * weight;
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use arb_types::{Market, MarketId, MarketStatus, Platform};
    use chrono::{Duration, Utc};
    use rust_decimal_macros::dec;
    use crate::types::MatchDecision;

    fn make_market(platform: Platform, question: &str, close_offset_hours: i64) -> Market {
        Market {
            id: MarketId::new(),
            platform,
            platform_id: format!("test-{}", question.len()),
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

    // ── Jaccard ──────────────────────────────────────────────────────────

    #[test]
    fn jaccard_identical_tokens() {
        let a = vec!["bitcoin".into(), "hit".into(), "100000".into()];
        let b = vec!["bitcoin".into(), "hit".into(), "100000".into()];
        let j = jaccard_similarity(&a, &b);
        assert!((j - 1.0).abs() < 0.01, "Identical token sets: jaccard={j}");
    }

    #[test]
    fn jaccard_no_overlap() {
        let a = vec!["bitcoin".into(), "hit".into(), "100000".into()];
        let b = vec!["super".into(), "bowl".into(), "winner".into()];
        let j = jaccard_similarity(&a, &b);
        assert!((j - 0.0).abs() < 0.01, "No overlap: jaccard={j}");
    }

    #[test]
    fn jaccard_partial_overlap() {
        let a: Vec<String> = vec!["bitcoin".into(), "hit".into(), "100000".into(), "december".into()];
        let b: Vec<String> = vec!["bitcoin".into(), "reach".into(), "100000".into(), "year".into()];
        let j = jaccard_similarity(&a, &b);
        // intersection: {"bitcoin", "100000"} = 2
        // union: 6 unique tokens
        assert!((j - 2.0 / 6.0).abs() < 0.05, "Partial overlap: jaccard={j}");
    }

    #[test]
    fn jaccard_both_empty() {
        let a: Vec<String> = vec![];
        let b: Vec<String> = vec![];
        let j = jaccard_similarity(&a, &b);
        assert!((j - 0.0).abs() < 0.01, "Both empty: jaccard={j}");
    }

    // ── Weighted overlap ─────────────────────────────────────────────────

    #[test]
    fn weighted_overlap_entity_match_scores_higher() {
        let with_entity = ClassifiedTokens {
            entities: vec!["bitcoin".into()],
            keywords: vec!["hit".into()],
            numbers: vec!["100000".into()],
            dates: vec![],
            all_meaningful: vec!["bitcoin".into(), "hit".into(), "100000".into()],
        };
        let with_entity_b = ClassifiedTokens {
            entities: vec!["bitcoin".into()],
            keywords: vec!["reach".into()],
            numbers: vec!["100000".into()],
            dates: vec![],
            all_meaningful: vec!["bitcoin".into(), "reach".into(), "100000".into()],
        };
        let without_entity = ClassifiedTokens {
            entities: vec![],
            keywords: vec!["hit".into(), "price".into()],
            numbers: vec!["100000".into()],
            dates: vec!["2026".into()],
            all_meaningful: vec!["hit".into(), "price".into(), "100000".into(), "2026".into()],
        };
        let without_entity_b = ClassifiedTokens {
            entities: vec![],
            keywords: vec!["range".into(), "price".into()],
            numbers: vec!["100000".into()],
            dates: vec!["2026".into()],
            all_meaningful: vec!["range".into(), "price".into(), "100000".into(), "2026".into()],
        };

        let score_with = weighted_overlap(&with_entity, &with_entity_b);
        let score_without = weighted_overlap(&without_entity, &without_entity_b);
        assert!(
            score_with > score_without,
            "Entity matches should produce higher weighted scores: {score_with} vs {score_without}"
        );
    }

    // ── Full scoring ─────────────────────────────────────────────────────

    #[test]
    fn identical_questions_score_high() {
        let poly = make_market(Platform::Polymarket, "Will Bitcoin hit $100k?", 24);
        let kalshi = make_market(Platform::Kalshi, "Will Bitcoin hit $100k?", 24);
        let s = score(&poly, &kalshi);
        assert!(
            s.composite >= 0.80,
            "identical questions should AutoVerify: composite={}",
            s.composite
        );
        assert_eq!(s.decision(), MatchDecision::AutoVerified);
    }

    #[test]
    fn very_different_questions_score_low() {
        let poly = make_market(Platform::Polymarket, "Will Bitcoin hit $100k?", 24);
        let kalshi = make_market(Platform::Kalshi, "Who wins the Super Bowl?", 24);
        let s = score(&poly, &kalshi);
        assert!(
            s.composite < 0.55,
            "unrelated questions should be Rejected: composite={}",
            s.composite
        );
    }

    #[test]
    fn similar_questions_different_phrasing() {
        let poly = make_market(Platform::Polymarket, "Will BTC reach $100k by Dec 2025?", 48);
        let kalshi = make_market(
            Platform::Kalshi,
            "Bitcoin to hit $100k before December 2025?",
            48,
        );
        let s = score(&poly, &kalshi);
        assert!(
            s.composite >= 0.55,
            "similar questions should be NeedsReview+: composite={}",
            s.composite
        );
    }

    #[test]
    fn close_time_far_apart_lowers_score() {
        let poly = make_market(Platform::Polymarket, "Will Bitcoin hit $100k?", 24);
        let kalshi = make_market(Platform::Kalshi, "Will Bitcoin hit $100k?", 24 + 200);
        let s = score(&poly, &kalshi);
        assert!(s.close_time_score == 0.0, "time score should be 0 for >168h diff");
    }

    #[test]
    fn close_time_same_gives_full_time_score() {
        let poly = make_market(Platform::Polymarket, "Will Bitcoin hit $100k?", 24);
        let kalshi = make_market(Platform::Kalshi, "Will Bitcoin hit $100k?", 24);
        let s = score(&poly, &kalshi);
        assert!(
            (s.close_time_score - 1.0).abs() < 0.01,
            "same close_time should give score ~1.0"
        );
    }

    #[test]
    fn is_candidate_threshold() {
        let low = MatchScore {
            jaccard: 0.1,
            weighted_overlap: 0.1,
            text_score: 0.1,
            text_similarity: 0.1,
            jaro_winkler: 0.3,
            close_time_score: 0.3,
            composite: 0.3,
            shared_entities: 0,
            shared_tokens: 0,
            category_match: false,
        };
        assert!(!is_candidate(&low));

        let high = MatchScore {
            jaccard: 0.9,
            weighted_overlap: 0.9,
            text_score: 0.9,
            text_similarity: 0.9,
            jaro_winkler: 0.9,
            close_time_score: 0.9,
            composite: 0.9,
            shared_entities: 2,
            shared_tokens: 5,
            category_match: true,
        };
        assert!(is_candidate(&high));
    }

    #[test]
    fn score_decision_auto_verified() {
        let s = MatchScore {
            jaccard: 1.0,
            weighted_overlap: 1.0,
            text_score: 1.0,
            text_similarity: 1.0,
            jaro_winkler: 1.0,
            close_time_score: 1.0,
            composite: 0.97,
            shared_entities: 3,
            shared_tokens: 5,
            category_match: true,
        };
        assert_eq!(s.decision(), MatchDecision::AutoVerified);
    }

    #[test]
    fn score_decision_needs_review() {
        let s = MatchScore {
            jaccard: 0.6,
            weighted_overlap: 0.6,
            text_score: 0.6,
            text_similarity: 0.6,
            jaro_winkler: 0.8,
            close_time_score: 0.8,
            composite: 0.70,
            shared_entities: 1,
            shared_tokens: 3,
            category_match: true,
        };
        assert_eq!(s.decision(), MatchDecision::NeedsReview);
    }

    #[test]
    fn score_decision_rejected() {
        let s = MatchScore {
            jaccard: 0.1,
            weighted_overlap: 0.1,
            text_score: 0.1,
            text_similarity: 0.1,
            jaro_winkler: 0.3,
            close_time_score: 0.3,
            composite: 0.30,
            shared_entities: 0,
            shared_tokens: 1,
            category_match: false,
        };
        assert_eq!(s.decision(), MatchDecision::Rejected);
    }

    #[test]
    fn min_shared_tokens_enforced() {
        // Markets sharing only date tokens should not produce a high score
        let poly = make_market(Platform::Polymarket, "Will something happen in 2026?", 24);
        let kalshi = make_market(Platform::Kalshi, "Will another thing occur in 2026?", 24);
        let s = score(&poly, &kalshi);
        assert!(
            s.composite < 0.55,
            "Single shared date token should not match: composite={}",
            s.composite
        );
    }

    // ── False positive regression tests ──────────────────────────────────

    #[test]
    fn regression_fp1_iran_meeting_vs_shiba_inu() {
        let poly = make_market(Platform::Polymarket, "US x Iran meeting by April 10, 2026?", 24);
        let kalshi = make_market(Platform::Kalshi, "Shiba Inu price range on Apr 10, 2026?", 24);
        let s = score(&poly, &kalshi);
        assert!(
            s.composite < 0.55,
            "Iran meeting vs Shiba Inu must be Rejected, got {}",
            s.composite
        );
    }

    #[test]
    fn regression_fp2_musk_tweets_vs_nyc_temp() {
        let poly = make_market(
            Platform::Polymarket,
            "Will Elon Musk post 260-279 tweets this week?",
            24,
        );
        let kalshi = make_market(
            Platform::Kalshi,
            "Will the temp in NYC be above 36.99 degrees?",
            24,
        );
        let s = score(&poly, &kalshi);
        assert!(
            s.composite < 0.55,
            "Musk tweets vs NYC temp must be Rejected, got {}",
            s.composite
        );
    }

    #[test]
    fn regression_fp3_alvarez_peru_vs_shiba_inu() {
        let poly = make_market(
            Platform::Polymarket,
            "Will Carlos Alvarez win the 2026 Peruvian presidential election?",
            24,
        );
        let kalshi = make_market(Platform::Kalshi, "Shiba Inu price range on Apr 10, 2026?", 24);
        let s = score(&poly, &kalshi);
        assert!(
            s.composite < 0.55,
            "Alvarez election vs Shiba Inu must be Rejected, got {}",
            s.composite
        );
    }

    #[test]
    fn regression_fp4_belmont_peru_vs_shiba_inu() {
        let poly = make_market(
            Platform::Polymarket,
            "Will Ricardo Belmont win the 2026 Peruvian presidential election?",
            24,
        );
        let kalshi = make_market(Platform::Kalshi, "Shiba Inu price range on Apr 10, 2026?", 24);
        let s = score(&poly, &kalshi);
        assert!(
            s.composite < 0.55,
            "Belmont election vs Shiba Inu must be Rejected, got {}",
            s.composite
        );
    }

    // ── True positive tests ──────────────────────────────────────────────

    #[test]
    fn tp1_identical_bitcoin_questions() {
        let poly = make_market(Platform::Polymarket, "Will Bitcoin hit $100k?", 24);
        let kalshi = make_market(Platform::Kalshi, "Will Bitcoin hit $100k?", 24);
        let s = score(&poly, &kalshi);
        assert!(
            s.composite >= 0.80,
            "Identical Bitcoin questions must be AutoVerified, got {}",
            s.composite
        );
        assert_eq!(s.decision(), MatchDecision::AutoVerified);
    }

    #[test]
    fn tp2_bitcoin_different_phrasing() {
        let poly = make_market(
            Platform::Polymarket,
            "Will Bitcoin hit $100k by December 2025?",
            48,
        );
        let kalshi = make_market(
            Platform::Kalshi,
            "Bitcoin to hit $100k before December 2025?",
            48,
        );
        let s = score(&poly, &kalshi);
        assert!(
            s.composite >= 0.55,
            "Similar Bitcoin questions must be NeedsReview+, got {}",
            s.composite
        );
    }

    #[test]
    fn tp3_trump_election_different_phrasing() {
        let poly = make_market(
            Platform::Polymarket,
            "Will Trump win the 2024 presidential election?",
            48,
        );
        let kalshi = make_market(
            Platform::Kalshi,
            "Will Donald Trump win the 2024 US presidential election?",
            48,
        );
        let s = score(&poly, &kalshi);
        assert!(
            s.composite >= 0.55,
            "Trump election questions must match, got {}",
            s.composite
        );
    }

    #[test]
    fn tp4_ethereum_alias_matching() {
        let poly = make_market(Platform::Polymarket, "Will ETH reach $5000 by end of 2025?", 48);
        let kalshi = make_market(
            Platform::Kalshi,
            "Will Ethereum reach $5000 before 2026?",
            48,
        );
        let s = score(&poly, &kalshi);
        assert!(
            s.composite >= 0.55,
            "ETH/Ethereum alias should match, got {}",
            s.composite
        );
    }

    #[test]
    fn tp5_btc_alias_matching() {
        let poly = make_market(Platform::Polymarket, "Will BTC reach $100k?", 24);
        let kalshi = make_market(Platform::Kalshi, "Will Bitcoin reach $100k?", 24);
        let s = score(&poly, &kalshi);
        assert!(
            s.composite >= 0.55,
            "BTC/Bitcoin alias should match, got {}",
            s.composite
        );
    }

    #[test]
    fn tp6_number_format_normalization() {
        let poly = make_market(Platform::Polymarket, "Will Bitcoin hit $100k?", 24);
        let kalshi = make_market(Platform::Kalshi, "Will Bitcoin hit $100,000?", 24);
        let s = score(&poly, &kalshi);
        assert!(
            s.composite >= 0.80,
            "$100k vs $100,000 should match after number normalization, got {}",
            s.composite
        );
    }

    // ── Edge cases ───────────────────────────────────────────────────────

    #[test]
    fn edge_different_price_targets_not_auto_verified() {
        let poly = make_market(Platform::Polymarket, "Will Bitcoin hit $100k?", 24);
        let kalshi = make_market(Platform::Kalshi, "Will Bitcoin hit $50k?", 24);
        let s = score(&poly, &kalshi);
        assert!(
            s.decision() != MatchDecision::AutoVerified,
            "Different price targets should not AutoVerify, got {}",
            s.composite
        );
    }

    #[test]
    fn edge_negation_different_tokens() {
        // "Will X?" vs "Will X NOT?" must NOT be identical after normalization
        let ct1 = classify_tokens("Will Bitcoin hit $100k?");
        let ct2 = classify_tokens("Will Bitcoin NOT hit $100k?");
        assert!(
            ct2.keywords.contains(&"not".to_string()),
            "\"not\" must be preserved as a keyword"
        );
        // They should have different token sets
        let set1: HashSet<&str> = ct1.all_meaningful.iter().map(|s| s.as_str()).collect();
        let set2: HashSet<&str> = ct2.all_meaningful.iter().map(|s| s.as_str()).collect();
        assert_ne!(set1, set2, "Negated question must have different token set");
    }

    #[test]
    fn edge_empty_questions() {
        let poly = make_market(Platform::Polymarket, "", 24);
        let kalshi = make_market(Platform::Kalshi, "", 24);
        let s = score(&poly, &kalshi);
        assert!(s.composite < 0.55, "Empty questions should not match");
    }

    #[test]
    fn edge_very_short_questions() {
        let poly = make_market(Platform::Polymarket, "Yes?", 24);
        let kalshi = make_market(Platform::Kalshi, "No?", 24);
        let s = score(&poly, &kalshi);
        assert!(s.composite < 0.55, "Trivial questions should not match");
    }

    #[test]
    fn edge_close_time_missing_text_still_matches() {
        let poly = make_market(Platform::Polymarket, "Will Bitcoin hit $100k?", 8760);
        let kalshi = make_market(Platform::Kalshi, "Will Bitcoin hit $100k?", 24);
        let s = score(&poly, &kalshi);
        assert!(
            s.composite >= 0.55,
            "Time difference should not completely kill perfect text match, got {}",
            s.composite
        );
    }
}
