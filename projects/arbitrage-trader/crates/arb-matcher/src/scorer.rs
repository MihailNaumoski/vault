//! Scoring engine for market match candidates.
//!
//! Combines text similarity (Jaro-Winkler on normalized questions) with
//! close-time proximity into a weighted composite score.

use arb_types::Market;
use crate::normalize::normalize;
use crate::types::MatchScore;

/// Weight for text similarity in composite score.
const TEXT_WEIGHT: f64 = 0.70;
/// Weight for close-time proximity in composite score.
const TIME_WEIGHT: f64 = 0.30;
/// Maximum time difference (in hours) that still earns a non-zero time score.
const MAX_TIME_DIFF_HOURS: f64 = 168.0; // 7 days

/// Minimum composite score to be considered a candidate at all.
const MIN_CANDIDATE_SCORE: f64 = 0.50;

/// Score a pair of markets for match quality.
///
/// Convenience wrapper that normalizes both questions before scoring.
/// For hot loops, prefer pre-normalizing and calling [`score_normalized`].
pub fn score(poly: &Market, kalshi: &Market) -> MatchScore {
    let norm_poly = normalize(&poly.question);
    let norm_kalshi = normalize(&kalshi.question);
    score_normalized(&norm_poly, &norm_kalshi, poly, kalshi)
}

/// Score a pair of markets using pre-normalized question strings.
///
/// Use this in tight loops where the same market gets compared against many
/// others — avoids redundant normalization work.
pub fn score_normalized(
    norm_poly: &str,
    norm_kalshi: &str,
    poly: &Market,
    kalshi: &Market,
) -> MatchScore {
    let text_similarity = strsim::jaro_winkler(norm_poly, norm_kalshi);

    let time_diff_hours = (poly.close_time - kalshi.close_time)
        .num_seconds()
        .unsigned_abs() as f64
        / 3600.0;
    let close_time_score = if time_diff_hours >= MAX_TIME_DIFF_HOURS {
        0.0
    } else {
        1.0 - (time_diff_hours / MAX_TIME_DIFF_HOURS)
    };

    let composite = text_similarity * TEXT_WEIGHT + close_time_score * TIME_WEIGHT;

    MatchScore {
        text_similarity,
        close_time_score,
        composite,
    }
}

/// Whether this score is high enough to be considered a candidate.
pub fn is_candidate(score: &MatchScore) -> bool {
    score.composite >= MIN_CANDIDATE_SCORE
}

#[cfg(test)]
mod tests {
    use super::*;
    use arb_types::{Market, MarketId, MarketStatus, Platform};
    use chrono::{Duration, Utc};
    use rust_decimal_macros::dec;

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

    #[test]
    fn identical_questions_score_high() {
        let poly = make_market(Platform::Polymarket, "Will Bitcoin hit $100k?", 24);
        let kalshi = make_market(Platform::Kalshi, "Will Bitcoin hit $100k?", 24);
        let s = score(&poly, &kalshi);
        assert!(s.composite >= 0.95, "identical questions: composite={}", s.composite);
    }

    #[test]
    fn very_different_questions_score_low() {
        let poly = make_market(Platform::Polymarket, "Will Bitcoin hit $100k?", 24);
        let kalshi = make_market(Platform::Kalshi, "Who wins the Super Bowl?", 24);
        let s = score(&poly, &kalshi);
        assert!(s.composite < 0.70, "unrelated questions: composite={}", s.composite);
    }

    #[test]
    fn similar_questions_different_phrasing() {
        let poly = make_market(Platform::Polymarket, "Will BTC reach $100k by Dec 2025?", 48);
        let kalshi = make_market(Platform::Kalshi, "Bitcoin to hit $100k before December 2025?", 48);
        let s = score(&poly, &kalshi);
        assert!(s.composite >= 0.50, "similar questions: composite={}", s.composite);
    }

    #[test]
    fn close_time_far_apart_lowers_score() {
        let poly = make_market(Platform::Polymarket, "Will Bitcoin hit $100k?", 24);
        let kalshi = make_market(Platform::Kalshi, "Will Bitcoin hit $100k?", 24 + 200);
        let s = score(&poly, &kalshi);
        // Text is identical so text_similarity ~1.0, but time_score = 0.0
        assert!(s.close_time_score == 0.0, "time score should be 0 for >168h diff");
        // composite = 1.0 * 0.70 + 0.0 * 0.30 = 0.70
        assert!((s.composite - 0.70).abs() < 0.01);
    }

    #[test]
    fn close_time_same_gives_full_time_score() {
        let poly = make_market(Platform::Polymarket, "Will Bitcoin hit $100k?", 24);
        let kalshi = make_market(Platform::Kalshi, "Will Bitcoin hit $100k?", 24);
        let s = score(&poly, &kalshi);
        assert!((s.close_time_score - 1.0).abs() < 0.01, "same close_time should give score ~1.0");
    }

    #[test]
    fn is_candidate_threshold() {
        let low = MatchScore {
            text_similarity: 0.3,
            close_time_score: 0.3,
            composite: 0.3,
        };
        assert!(!is_candidate(&low));

        let high = MatchScore {
            text_similarity: 0.9,
            close_time_score: 0.9,
            composite: 0.9,
        };
        assert!(is_candidate(&high));
    }

    #[test]
    fn score_decision_auto_verified() {
        let s = MatchScore {
            text_similarity: 1.0,
            close_time_score: 1.0,
            composite: 0.97,
        };
        assert_eq!(s.decision(), crate::types::MatchDecision::AutoVerified);
    }

    #[test]
    fn score_decision_needs_review() {
        let s = MatchScore {
            text_similarity: 0.8,
            close_time_score: 0.8,
            composite: 0.80,
        };
        assert_eq!(s.decision(), crate::types::MatchDecision::NeedsReview);
    }

    #[test]
    fn score_decision_rejected() {
        let s = MatchScore {
            text_similarity: 0.3,
            close_time_score: 0.3,
            composite: 0.30,
        };
        assert_eq!(s.decision(), crate::types::MatchDecision::Rejected);
    }
}
