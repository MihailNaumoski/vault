use serde::{Deserialize, Serialize};
use arb_types::Market;

/// Score breakdown for a market match candidate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchScore {
    pub text_similarity: f64,
    pub close_time_score: f64,
    pub composite: f64,
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
    pub fn decision(&self) -> MatchDecision {
        if self.composite >= 0.90 {
            MatchDecision::AutoVerified
        } else if self.composite >= 0.50 {
            MatchDecision::NeedsReview
        } else {
            MatchDecision::Rejected
        }
    }
}
