# Phase 3 — Market Matching Engine — Build Prompt

**Goal:** Build `arb-matcher` so `arb --match` fetches markets from both platforms, finds equivalent pairs via fuzzy matching, presents candidates for human verification, and persists verified pairs to DB + TOML.

**Project root:** `/Users/mihail/projects/vault/projects/arbitrage-trader`
**Depends on:** Phase 2.5 complete (97 tests, clippy clean)

---

## Context: What Already Exists

**Types (arb-types/src/market.rs):**
```rust
pub struct Market {
    pub id: MarketId,
    pub platform: Platform,
    pub platform_id: String,    // condition_id for Poly, ticker for Kalshi
    pub question: String,
    pub yes_price: Decimal,
    pub no_price: Decimal,
    pub volume: Decimal,
    pub liquidity: Decimal,
    pub status: MarketStatus,
    pub close_time: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub struct MarketRef {
    pub platform_id: String,
    pub question: String,
    pub close_time: DateTime<Utc>,
}

pub struct MarketPair {
    pub id: Uuid,
    pub polymarket: MarketRef,
    pub kalshi: MarketRef,
    pub match_confidence: f64,
    pub verified: bool,
    pub active: bool,
    pub created_at: DateTime<Utc>,
}
```

**DB row (arb-db/src/models.rs) — MarketPairRow has MORE fields than MarketPair:**
```rust
pub struct MarketPairRow {
    pub id: String,
    pub poly_condition_id: String,
    pub poly_yes_token_id: String,   // needed for order placement
    pub poly_no_token_id: String,    // needed for order placement
    pub poly_question: String,
    pub kalshi_ticker: String,
    pub kalshi_question: String,
    pub match_confidence: f64,
    pub verified: bool,
    pub active: bool,
    pub close_time: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

**DB methods available:**
- `insert_market_pair(&MarketPairRow)`
- `get_market_pair(&Uuid) -> Option<MarketPairRow>`
- `list_active_market_pairs() -> Vec<MarketPairRow>`
- `update_market_pair(&MarketPairRow)`
- `delete_market_pair(&Uuid)`

**Polymarket Market response has token IDs:**
```rust
pub struct PolyMarketResponse {
    pub condition_id: String,
    pub question: String,
    pub tokens: Vec<PolyToken>,  // [{token_id, outcome, price}]
    // ...
}
```

**Workspace already has:** `strsim = "0.11"`, `clap` with derive, `serde`, `tokio`, `chrono`, `uuid`

**CLI (arb-cli) already has:** `--match` flag parsed via clap, currently prints "not yet implemented".

---

## Prompt 3-A: Core Matching Engine

### Files to Create

```
crates/arb-matcher/src/
    lib.rs          — module declarations + re-exports
    normalize.rs    — text normalization
    scorer.rs       — multi-signal similarity scoring
    types.rs        — MatchCandidate, MatchScore, MatchDecision
```

### Update Cargo.toml

**File:** `crates/arb-matcher/Cargo.toml`

```toml
[package]
name = "arb-matcher"
version = "0.1.0"
edition = "2021"

[dependencies]
arb-types = { workspace = true }
strsim = { workspace = true }
chrono = { workspace = true }
uuid = { workspace = true }
serde = { workspace = true }
tracing = { workspace = true }
rust_decimal = { workspace = true }

[dev-dependencies]
rust_decimal_macros = { workspace = true }
```

### src/types.rs

```rust
use arb_types::Market;
use serde::{Deserialize, Serialize};

/// Score breakdown for a market match candidate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchScore {
    /// Jaro-Winkler similarity on normalized question text (0.0 - 1.0)
    pub text_similarity: f64,
    /// Close-time proximity score (1.0 if within 24h, decays to 0.0 at 7 days)
    pub close_time_score: f64,
    /// Weighted composite score
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
    /// Score >= 0.95 — auto-verify (still logged for audit)
    AutoVerified,
    /// Score >= 0.70 — show to human for review
    NeedsReview,
    /// Score < 0.70 — rejected, don't show
    Rejected,
}

impl MatchScore {
    pub fn decision(&self) -> MatchDecision {
        if self.composite >= 0.95 {
            MatchDecision::AutoVerified
        } else if self.composite >= 0.70 {
            MatchDecision::NeedsReview
        } else {
            MatchDecision::Rejected
        }
    }
}
```

### src/normalize.rs

```rust
/// Stop words to remove from market questions before comparison.
const STOP_WORDS: &[&str] = &[
    "will", "the", "a", "an", "of", "in", "on", "by", "to", "for",
    "be", "is", "are", "was", "were", "at", "or", "and", "before",
    "after", "this", "that", "it", "its", "with", "from", "as",
    "but", "not", "no", "yes", "do", "does", "did", "has", "have",
    "had", "been", "being", "if", "than", "then", "so", "what",
    "which", "who", "whom", "how", "when", "where", "why",
];

/// Normalize a market question for fuzzy comparison.
///
/// Steps:
/// 1. Lowercase
/// 2. Remove all non-alphanumeric characters (keep spaces)
/// 3. Remove stop words
/// 4. Collapse whitespace
///
/// Example: "Will the U.S. GDP grow by 3%?" → "us gdp grow 3"
pub fn normalize(text: &str) -> String {
    let lowered = text.to_lowercase();
    let cleaned: String = lowered
        .chars()
        .map(|c| if c.is_alphanumeric() || c.is_whitespace() { c } else { ' ' })
        .collect();
    cleaned
        .split_whitespace()
        .filter(|word| !STOP_WORDS.contains(word))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Extract significant tokens (length >= 3) for candidate pre-filtering.
pub fn extract_tokens(text: &str) -> Vec<String> {
    normalize(text)
        .split_whitespace()
        .filter(|w| w.len() >= 3)
        .map(|w| w.to_string())
        .collect()
}

/// Count shared tokens between two token sets.
pub fn shared_token_count(a: &[String], b: &[String]) -> usize {
    a.iter().filter(|t| b.contains(t)).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_basic() {
        assert_eq!(normalize("Will the U.S. GDP grow by 3%?"), "us gdp grow 3");
    }

    #[test]
    fn test_normalize_removes_punctuation() {
        assert_eq!(normalize("Biden's victory (2028)"), "bidens victory 2028");
    }

    #[test]
    fn test_normalize_removes_stop_words() {
        assert_eq!(normalize("Will it be a yes or no"), "");
    }

    #[test]
    fn test_extract_tokens() {
        let tokens = extract_tokens("Will Bitcoin reach $100,000 by December 2026?");
        assert!(tokens.contains(&"bitcoin".to_string()));
        assert!(tokens.contains(&"reach".to_string()));
        assert!(tokens.contains(&"100000".to_string()));
        assert!(tokens.contains(&"december".to_string()));
        assert!(tokens.contains(&"2026".to_string()));
    }

    #[test]
    fn test_shared_tokens() {
        let a = extract_tokens("Will Bitcoin reach $100K in 2026?");
        let b = extract_tokens("Bitcoin to hit $100K by end of 2026");
        assert!(shared_token_count(&a, &b) >= 2); // "bitcoin", "100k", "2026"
    }

    #[test]
    fn test_normalize_empty() {
        assert_eq!(normalize(""), "");
    }

    #[test]
    fn test_normalize_only_stop_words() {
        assert_eq!(normalize("will the a an"), "");
    }
}
```

### src/scorer.rs

```rust
use crate::normalize::{extract_tokens, normalize, shared_token_count};
use crate::types::MatchScore;
use arb_types::Market;
use chrono::Utc;

/// Compute a match score between a Polymarket market and a Kalshi market.
pub fn score(poly: &Market, kalshi: &Market) -> MatchScore {
    // 1. Text similarity (Jaro-Winkler on normalized questions)
    let poly_normalized = normalize(&poly.question);
    let kalshi_normalized = normalize(&kalshi.question);

    let text_similarity = if poly_normalized.is_empty() || kalshi_normalized.is_empty() {
        0.0
    } else {
        strsim::jaro_winkler(&poly_normalized, &kalshi_normalized)
    };

    // 2. Close-time proximity
    //    1.0 if within 24 hours, linear decay to 0.0 at 7 days (168 hours)
    let time_delta_hours = (poly.close_time - kalshi.close_time)
        .num_hours()
        .unsigned_abs() as f64;
    let close_time_score = (1.0 - time_delta_hours / 168.0).max(0.0);

    // 3. Composite: text 70%, time 30%
    //    Text similarity is the primary signal. Close-time is a sanity check.
    let composite = text_similarity * 0.70 + close_time_score * 0.30;

    MatchScore {
        text_similarity,
        close_time_score,
        composite,
    }
}

/// Pre-filter: returns true if markets share enough tokens to be worth scoring.
/// This avoids O(N*M) full Jaro-Winkler comparisons.
pub fn is_candidate(poly: &Market, kalshi: &Market, min_shared_tokens: usize) -> bool {
    let poly_tokens = extract_tokens(&poly.question);
    let kalshi_tokens = extract_tokens(&kalshi.question);
    shared_token_count(&poly_tokens, &kalshi_tokens) >= min_shared_tokens
}

#[cfg(test)]
mod tests {
    use super::*;
    use arb_types::{MarketId, MarketStatus, Platform};
    use chrono::{Duration, Utc};
    use rust_decimal_macros::dec;

    fn make_market(platform: Platform, question: &str, close_offset_days: i64) -> Market {
        Market {
            id: MarketId::new(),
            platform,
            platform_id: "test-id".into(),
            question: question.into(),
            yes_price: dec!(0.50),
            no_price: dec!(0.50),
            volume: dec!(10000),
            liquidity: dec!(5000),
            status: MarketStatus::Open,
            close_time: Utc::now() + Duration::days(close_offset_days),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn test_identical_questions_high_score() {
        let poly = make_market(Platform::Polymarket, "Will Bitcoin reach $100K in 2026?", 30);
        let kalshi = make_market(Platform::Kalshi, "Will Bitcoin reach $100K in 2026?", 30);
        let s = score(&poly, &kalshi);
        assert!(s.text_similarity > 0.99, "identical text should be ~1.0, got {}", s.text_similarity);
        assert!(s.composite > 0.95);
    }

    #[test]
    fn test_similar_questions_good_score() {
        let poly = make_market(Platform::Polymarket, "Will Bitcoin reach $100,000 by December 2026?", 30);
        let kalshi = make_market(Platform::Kalshi, "Bitcoin to hit $100K by end of 2026", 30);
        let s = score(&poly, &kalshi);
        assert!(s.text_similarity > 0.5, "similar markets should score > 0.5, got {}", s.text_similarity);
    }

    #[test]
    fn test_different_questions_low_score() {
        let poly = make_market(Platform::Polymarket, "Will Bitcoin reach $100K?", 30);
        let kalshi = make_market(Platform::Kalshi, "Will the Fed cut rates in July?", 30);
        let s = score(&poly, &kalshi);
        assert!(s.composite < 0.70, "different markets should score < 0.70, got {}", s.composite);
    }

    #[test]
    fn test_close_time_same_day() {
        let poly = make_market(Platform::Polymarket, "Test question A", 30);
        let kalshi = make_market(Platform::Kalshi, "Test question A", 30);
        let s = score(&poly, &kalshi);
        assert!(s.close_time_score > 0.9);
    }

    #[test]
    fn test_close_time_far_apart() {
        let poly = make_market(Platform::Polymarket, "Test question A", 30);
        let kalshi = make_market(Platform::Kalshi, "Test question A", 60);
        let s = score(&poly, &kalshi);
        assert!(s.close_time_score < 0.8, "30 day gap should reduce time score, got {}", s.close_time_score);
    }

    #[test]
    fn test_candidate_prefilter_passes() {
        let poly = make_market(Platform::Polymarket, "Will Bitcoin reach $100K in 2026?", 30);
        let kalshi = make_market(Platform::Kalshi, "Bitcoin to hit $100K by 2026", 30);
        assert!(is_candidate(&poly, &kalshi, 2));
    }

    #[test]
    fn test_candidate_prefilter_rejects() {
        let poly = make_market(Platform::Polymarket, "Will Bitcoin reach $100K?", 30);
        let kalshi = make_market(Platform::Kalshi, "Will the Fed cut rates?", 30);
        assert!(!is_candidate(&poly, &kalshi, 2));
    }

    #[test]
    fn test_empty_question_scores_zero() {
        let poly = make_market(Platform::Polymarket, "", 30);
        let kalshi = make_market(Platform::Kalshi, "Something", 30);
        let s = score(&poly, &kalshi);
        assert_eq!(s.text_similarity, 0.0);
    }
}
```

### src/lib.rs

```rust
pub mod normalize;
pub mod scorer;
pub mod types;
pub mod pipeline;
pub mod store;

pub use pipeline::MatchPipeline;
pub use store::PairStore;
pub use types::{MatchCandidate, MatchDecision, MatchScore};
```

### Verification

```bash
cargo test -p arb-matcher
# Expected: 15+ tests from normalize + scorer
cargo clippy -p arb-matcher -- -D warnings
```

---

## Prompt 3-B: Pipeline + Pair Store

### Files to Create

```
crates/arb-matcher/src/
    pipeline.rs     — end-to-end matching (fetch → pre-filter → score → sort)
    store.rs        — pairs persistence (TOML loading + DB bridge)

config/
    pairs.toml      — initial empty config with example comment
```

### src/pipeline.rs

```rust
use crate::scorer::{is_candidate, score};
use crate::types::{MatchCandidate, MatchDecision};
use arb_types::Market;
use tracing::info;

/// Minimum shared tokens to be considered a candidate (pre-filter).
const MIN_SHARED_TOKENS: usize = 2;

pub struct MatchPipeline {
    /// Minimum composite score to include in results.
    pub min_score: f64,
}

impl Default for MatchPipeline {
    fn default() -> Self {
        Self { min_score: 0.70 }
    }
}

impl MatchPipeline {
    pub fn new(min_score: f64) -> Self {
        Self { min_score }
    }

    /// Find matching markets between Polymarket and Kalshi market lists.
    ///
    /// Returns candidates sorted by composite score (highest first).
    /// Pre-filters using shared tokens to avoid O(N*M) full scoring.
    pub fn find_matches(
        &self,
        poly_markets: &[Market],
        kalshi_markets: &[Market],
    ) -> Vec<MatchCandidate> {
        let mut candidates = Vec::new();

        info!(
            poly_count = poly_markets.len(),
            kalshi_count = kalshi_markets.len(),
            "starting match pipeline"
        );

        for poly in poly_markets {
            // Track best match for this poly market
            let mut best: Option<MatchCandidate> = None;

            for kalshi in kalshi_markets {
                // Pre-filter: skip pairs that don't share enough tokens
                if !is_candidate(poly, kalshi, MIN_SHARED_TOKENS) {
                    continue;
                }

                let match_score = score(poly, kalshi);

                if match_score.composite < self.min_score {
                    continue;
                }

                // Keep only the best Kalshi match per Polymarket market
                let is_better = best
                    .as_ref()
                    .map_or(true, |b| match_score.composite > b.score.composite);

                if is_better {
                    best = Some(MatchCandidate {
                        poly_market: poly.clone(),
                        kalshi_market: kalshi.clone(),
                        score: match_score,
                    });
                }
            }

            if let Some(candidate) = best {
                candidates.push(candidate);
            }
        }

        // Sort by composite score descending
        candidates.sort_by(|a, b| {
            b.score
                .composite
                .partial_cmp(&a.score.composite)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        info!(
            candidates = candidates.len(),
            "match pipeline complete"
        );

        candidates
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arb_types::{MarketId, MarketStatus, Platform};
    use chrono::{Duration, Utc};
    use rust_decimal_macros::dec;

    fn make_market(platform: Platform, question: &str, close_offset_days: i64) -> Market {
        Market {
            id: MarketId::new(),
            platform,
            platform_id: format!("{}-{}", if platform == Platform::Polymarket { "poly" } else { "kalshi" }, question.len()),
            question: question.into(),
            yes_price: dec!(0.50),
            no_price: dec!(0.50),
            volume: dec!(10000),
            liquidity: dec!(5000),
            status: MarketStatus::Open,
            close_time: Utc::now() + Duration::days(close_offset_days),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn test_pipeline_finds_known_pair() {
        let poly = vec![
            make_market(Platform::Polymarket, "Will Bitcoin reach $100K in 2026?", 30),
            make_market(Platform::Polymarket, "Will Ethereum hit $10K?", 60),
        ];
        let kalshi = vec![
            make_market(Platform::Kalshi, "Bitcoin to reach $100K by 2026", 30),
            make_market(Platform::Kalshi, "Will the Fed raise rates?", 30),
        ];

        let pipeline = MatchPipeline::default();
        let matches = pipeline.find_matches(&poly, &kalshi);

        assert!(!matches.is_empty(), "should find at least 1 match");
        assert!(
            matches[0].poly_market.question.contains("Bitcoin"),
            "best match should be the Bitcoin pair"
        );
    }

    #[test]
    fn test_pipeline_rejects_unrelated() {
        let poly = vec![make_market(Platform::Polymarket, "Will Bitcoin reach $100K?", 30)];
        let kalshi = vec![make_market(Platform::Kalshi, "Will the Fed cut rates in July?", 30)];

        let pipeline = MatchPipeline::default();
        let matches = pipeline.find_matches(&poly, &kalshi);

        assert!(matches.is_empty(), "unrelated markets should not match");
    }

    #[test]
    fn test_pipeline_sorted_by_score() {
        let poly = vec![
            make_market(Platform::Polymarket, "Will Bitcoin reach $100K in 2026?", 30),
            make_market(Platform::Polymarket, "Will Ethereum reach $10K in 2026?", 30),
        ];
        let kalshi = vec![
            make_market(Platform::Kalshi, "Will Bitcoin reach $100K in 2026?", 30), // exact match
            make_market(Platform::Kalshi, "Ethereum to hit $10K by end 2026", 30),  // similar
        ];

        let pipeline = MatchPipeline::default();
        let matches = pipeline.find_matches(&poly, &kalshi);

        if matches.len() >= 2 {
            assert!(
                matches[0].score.composite >= matches[1].score.composite,
                "results should be sorted by score descending"
            );
        }
    }

    #[test]
    fn test_pipeline_empty_inputs() {
        let pipeline = MatchPipeline::default();
        assert!(pipeline.find_matches(&[], &[]).is_empty());
        let m = make_market(Platform::Polymarket, "test", 30);
        assert!(pipeline.find_matches(&[m], &[]).is_empty());
    }

    #[test]
    fn test_pipeline_performance_100x100() {
        let poly: Vec<_> = (0..100)
            .map(|i| make_market(Platform::Polymarket, &format!("Market question number {}", i), 30))
            .collect();
        let kalshi: Vec<_> = (0..100)
            .map(|i| make_market(Platform::Kalshi, &format!("Market question number {}", i), 30))
            .collect();

        let pipeline = MatchPipeline::default();
        let start = std::time::Instant::now();
        let _matches = pipeline.find_matches(&poly, &kalshi);
        let elapsed = start.elapsed();

        assert!(
            elapsed.as_secs() < 5,
            "100x100 matching should complete in < 5s, took {:?}",
            elapsed
        );
    }
}
```

### src/store.rs

```rust
use arb_db::{SqliteRepository, Repository};
use arb_db::models::MarketPairRow;
use arb_types::Market;
use crate::types::MatchCandidate;
use chrono::Utc;
use serde::Deserialize;
use std::path::Path;
use tracing::info;
use uuid::Uuid;

/// A pair definition as stored in config/pairs.toml.
#[derive(Debug, Clone, Deserialize)]
pub struct TomlPair {
    pub poly_condition_id: String,
    pub poly_yes_token_id: String,
    pub poly_no_token_id: String,
    pub kalshi_ticker: String,
    pub label: String,
    #[serde(default = "default_true")]
    pub verified: bool,
    #[serde(default = "default_true")]
    pub active: bool,
}

fn default_true() -> bool { true }

#[derive(Debug, Deserialize)]
struct PairsFile {
    #[serde(default)]
    pair: Vec<TomlPair>,
}

pub struct PairStore {
    db: std::sync::Arc<SqliteRepository>,
}

impl PairStore {
    pub fn new(db: std::sync::Arc<SqliteRepository>) -> Self {
        Self { db }
    }

    /// Load pairs from a TOML file and upsert into the database.
    /// Returns the number of pairs loaded.
    pub async fn load_from_toml(&self, path: &Path) -> anyhow::Result<usize> {
        let content = tokio::fs::read_to_string(path).await?;
        let pairs_file: PairsFile = toml::from_str(&content)?;

        let mut count = 0;
        for tp in &pairs_file.pair {
            let row = MarketPairRow {
                id: Uuid::now_v7().to_string(),
                poly_condition_id: tp.poly_condition_id.clone(),
                poly_yes_token_id: tp.poly_yes_token_id.clone(),
                poly_no_token_id: tp.poly_no_token_id.clone(),
                poly_question: tp.label.clone(),
                kalshi_ticker: tp.kalshi_ticker.clone(),
                kalshi_question: tp.label.clone(),
                match_confidence: 1.0, // manually configured = full confidence
                verified: tp.verified,
                active: tp.active,
                close_time: Utc::now() + chrono::Duration::days(365), // placeholder
                created_at: Utc::now(),
                updated_at: Utc::now(),
            };
            self.db.insert_market_pair(&row).await?;
            count += 1;
        }

        info!(count, path = %path.display(), "loaded pairs from TOML");
        Ok(count)
    }

    /// Save a match candidate to the database as an unverified pair.
    /// The poly_yes_token_id and poly_no_token_id are extracted from the
    /// Polymarket market's platform data if available, otherwise left empty
    /// (must be filled in during verification).
    pub async fn save_candidate(
        &self,
        candidate: &MatchCandidate,
        poly_yes_token_id: &str,
        poly_no_token_id: &str,
    ) -> anyhow::Result<Uuid> {
        let id = Uuid::now_v7();
        let row = MarketPairRow {
            id: id.to_string(),
            poly_condition_id: candidate.poly_market.platform_id.clone(),
            poly_yes_token_id: poly_yes_token_id.to_string(),
            poly_no_token_id: poly_no_token_id.to_string(),
            poly_question: candidate.poly_market.question.clone(),
            kalshi_ticker: candidate.kalshi_market.platform_id.clone(),
            kalshi_question: candidate.kalshi_market.question.clone(),
            match_confidence: candidate.score.composite,
            verified: false,  // always unverified until human approves
            active: true,
            close_time: candidate.poly_market.close_time.min(candidate.kalshi_market.close_time),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        self.db.insert_market_pair(&row).await?;
        Ok(id)
    }

    /// Mark a pair as verified (human approved).
    pub async fn verify_pair(&self, pair_id: &str) -> anyhow::Result<()> {
        let uuid = Uuid::parse_str(pair_id)?;
        if let Some(mut row) = self.db.get_market_pair(&uuid).await? {
            row.verified = true;
            row.updated_at = Utc::now();
            self.db.update_market_pair(&row).await?;
            info!(pair_id, "pair verified");
        }
        Ok(())
    }

    /// Get all active, verified pairs.
    pub async fn active_verified_pairs(&self) -> anyhow::Result<Vec<MarketPairRow>> {
        let all = self.db.list_active_market_pairs().await?;
        Ok(all.into_iter().filter(|p| p.verified).collect())
    }

    /// Deactivate pairs whose close_time has passed.
    pub async fn deactivate_expired(&self) -> anyhow::Result<u32> {
        let all = self.db.list_active_market_pairs().await?;
        let now = Utc::now();
        let mut count = 0u32;
        for mut pair in all {
            if pair.close_time < now {
                pair.active = false;
                pair.updated_at = now;
                self.db.update_market_pair(&pair).await?;
                count += 1;
            }
        }
        if count > 0 {
            info!(count, "deactivated expired pairs");
        }
        Ok(count)
    }
}
```

### config/pairs.toml

```toml
# Verified market pairs — manually configured.
# Each [[pair]] maps a Polymarket market to a Kalshi market.
#
# To add a pair:
# 1. Run `arb --match` to discover candidates
# 2. Verify the match manually (check resolution criteria!)
# 3. Add the pair here with the correct token IDs
#
# Example:
# [[pair]]
# poly_condition_id = "0xabc123..."
# poly_yes_token_id = "12345..."
# poly_no_token_id = "67890..."
# kalshi_ticker = "PRES-2026-DEM"
# label = "Democrat wins 2026 presidential election"
# verified = true
# active = true
```

### Update arb-matcher/Cargo.toml — add missing deps

Add these to `[dependencies]`:
```toml
arb-db = { workspace = true }
toml = { workspace = true }
anyhow = { workspace = true }
tokio = { workspace = true }
```

Check if `toml` is in workspace deps. If not, add `toml = "0.8"` to workspace `[workspace.dependencies]` in root Cargo.toml.

### Verification

```bash
cargo test -p arb-matcher
# Expected: 15+ tests from normalize + scorer + pipeline
cargo clippy -p arb-matcher -- -D warnings
```

---

## Prompt 3-C: CLI --match Command

### File to Modify

`crates/arb-cli/src/main.rs`

### Add Dependency

In `crates/arb-cli/Cargo.toml`, add:
```toml
arb-matcher = { workspace = true }
arb-polymarket = { workspace = true }
arb-kalshi = { workspace = true }
```

### Implementation

Replace the `if args.r#match` block in main() with:

```rust
if args.r#match {
    info!("Starting market match scan...");

    // 1. Init connectors (read-only — only fetching markets, not trading)
    // For --match mode, we need API credentials to list markets
    // If credentials aren't set, show a helpful error
    let poly_connector = match init_polymarket_connector(&app_config) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Cannot init Polymarket connector: {}", e);
            eprintln!("Set POLY_API_KEY, POLY_API_SECRET, POLY_PRIVATE_KEY in .env");
            return Ok(());
        }
    };
    let kalshi_connector = match init_kalshi_connector(&app_config) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Cannot init Kalshi connector: {}", e);
            eprintln!("Set KALSHI_API_KEY_ID, KALSHI_PRIVATE_KEY_PEM in .env");
            return Ok(());
        }
    };

    // 2. Fetch active markets from both platforms
    use arb_types::MarketStatus;
    println!("Fetching Polymarket markets...");
    let poly_markets = poly_connector.list_markets(MarketStatus::Open).await?;
    println!("  Found {} active Polymarket markets", poly_markets.len());

    println!("Fetching Kalshi markets...");
    let kalshi_markets = kalshi_connector.list_markets(MarketStatus::Open).await?;
    println!("  Found {} active Kalshi markets", kalshi_markets.len());

    // 3. Run matching pipeline
    let pipeline = arb_matcher::MatchPipeline::default();
    let candidates = pipeline.find_matches(&poly_markets, &kalshi_markets);
    println!("\nFound {} match candidates:\n", candidates.len());

    // 4. Display results
    println!("{:<6} {:<50} {:<50} {}", "Score", "Polymarket", "Kalshi", "Decision");
    println!("{}", "-".repeat(120));

    for candidate in &candidates {
        let decision = candidate.score.decision();
        let decision_str = match decision {
            arb_matcher::MatchDecision::AutoVerified => "✅ AUTO",
            arb_matcher::MatchDecision::NeedsReview => "🔍 REVIEW",
            arb_matcher::MatchDecision::Rejected => "❌ SKIP",
        };

        let poly_q: String = candidate.poly_market.question.chars().take(48).collect();
        let kalshi_q: String = candidate.kalshi_market.question.chars().take(48).collect();

        println!(
            "{:.3}  {:<50} {:<50} {}",
            candidate.score.composite,
            poly_q,
            kalshi_q,
            decision_str,
        );
    }

    // 5. Prompt for verification (basic stdin)
    let reviewable: Vec<_> = candidates
        .iter()
        .filter(|c| matches!(c.score.decision(), arb_matcher::MatchDecision::NeedsReview | arb_matcher::MatchDecision::AutoVerified))
        .collect();

    if !reviewable.is_empty() {
        println!("\n{} candidates ready for review.", reviewable.len());
        println!("Run with live API credentials to verify pairs interactively.");
        println!("Verified pairs will be saved to the database.");
    }

    return Ok(());
}
```

**Note:** The `init_polymarket_connector` and `init_kalshi_connector` functions need to be created. They should read credentials from environment variables and construct the connector configs. If connector initialization is complex, you can create a `connectors.rs` module in arb-cli, or inline them. The important thing is that `--match` mode works for fetching and displaying — actual trading is not needed here.

If initializing real connectors is too complex for this phase (requires all auth setup), create a **simpler version** that works without credentials by using the mock connectors with sample data:

```rust
// Fallback if no credentials: use mock data for demonstration
if args.r#match {
    info!("Match mode — scanning for market pairs...");

    // For now, demonstrate the pipeline with mock data
    // TODO: Replace with real connector initialization when credentials are available
    println!("Note: Using mock market data. Set API credentials in .env for live scanning.\n");

    // Create sample markets for testing the pipeline
    use arb_types::{Market, MarketId, MarketStatus, Platform};
    use chrono::{Duration, Utc};
    use rust_decimal_macros::dec;

    let poly_markets = vec![
        // ... sample Polymarket markets
    ];
    let kalshi_markets = vec![
        // ... sample Kalshi markets
    ];

    let pipeline = arb_matcher::MatchPipeline::default();
    let candidates = pipeline.find_matches(&poly_markets, &kalshi_markets);

    // ... display logic same as above
    return Ok(());
}
```

**Choose the approach based on whether the connector initialization code is straightforward to wire up.** Either way, the matcher pipeline and display must work.

### Verification

```bash
# Full workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo build --workspace

# Specific check
cargo run -- --match
# Should either show "no credentials" message or display matched candidates
```

---

## Phase 3 Acceptance Criteria

- [ ] `cargo test -p arb-matcher` passes with >= 20 tests
- [ ] `cargo test --workspace` passes (all previous + matcher tests)
- [ ] `cargo clippy --workspace -- -D warnings` clean
- [ ] `normalize()` correctly strips punctuation, stop words, lowercases
- [ ] `score()` gives > 0.85 for known equivalent market texts
- [ ] `score()` gives < 0.70 for unrelated market texts
- [ ] `MatchPipeline::find_matches()` returns sorted candidates
- [ ] Pre-filter (shared tokens) reduces unnecessary comparisons
- [ ] `PairStore::load_from_toml()` loads pairs into DB
- [ ] `PairStore::active_verified_pairs()` only returns verified + active
- [ ] `PairStore::deactivate_expired()` deactivates past-close-time pairs
- [ ] `arb --match` runs without panic and displays output
- [ ] `config/pairs.toml` exists with example comments

## Execution Order

```
Prompt 3-A first  → normalize.rs, scorer.rs, types.rs (standalone, no DB needed)
Prompt 3-B second → pipeline.rs, store.rs (needs types from 3-A)
Prompt 3-C last   → CLI wiring (needs pipeline + store from 3-B)
```
