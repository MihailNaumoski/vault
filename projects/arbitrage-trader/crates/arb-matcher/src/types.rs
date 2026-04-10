use serde::{Deserialize, Serialize};
use arb_types::Market;

/// Token classification for weighted scoring.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenType {
    Entity,
    Number,
    Date,
    Keyword,
}

/// Score breakdown for a market match candidate.
///
/// The expanded struct provides transparency into *why* a match was made or
/// rejected, enabling debugging and threshold calibration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchScore {
    /// Jaccard similarity on meaningful token sets.
    pub jaccard: f64,
    /// Entity/keyword-weighted overlap score.
    pub weighted_overlap: f64,
    /// Combined token-based text score: `0.60 * weighted_overlap + 0.40 * jaccard`.
    pub text_score: f64,
    /// Backward-compat alias — same value as `text_score`.
    pub text_similarity: f64,
    /// Jaro-Winkler character-level tiebreaker (kept for debugging).
    pub jaro_winkler: f64,
    /// Time proximity signal (0.0..1.0).
    pub close_time_score: f64,
    /// Final weighted composite: `0.65 * text + 0.25 * time + 0.10 * jw`.
    pub composite: f64,
    /// Number of shared entities (for debugging/display).
    pub shared_entities: usize,
    /// Number of shared meaningful tokens.
    pub shared_tokens: usize,
    /// Whether categories matched (or at least one was Other).
    pub category_match: bool,
}

/// A candidate match between a Polymarket and Kalshi market.
#[derive(Debug, Clone)]
pub struct MatchCandidate {
    pub poly_market: Market,
    pub kalshi_market: Market,
    pub score: MatchScore,
}

/// Decision based on score thresholds.
#[derive(Debug, Clone, PartialEq)]
pub enum MatchDecision {
    AutoVerified,
    NeedsReview,
    Rejected,
}

impl MatchScore {
    /// Decision using the new default thresholds (0.80 / 0.55).
    pub fn decision(&self) -> MatchDecision {
        self.decision_with_thresholds(0.80, 0.55)
    }

    pub fn decision_with_thresholds(&self, auto_verified: f64, needs_review: f64) -> MatchDecision {
        if self.composite >= auto_verified {
            MatchDecision::AutoVerified
        } else if self.composite >= needs_review {
            MatchDecision::NeedsReview
        } else {
            MatchDecision::Rejected
        }
    }
}
