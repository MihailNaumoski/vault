//! Pair store — persistence layer for market match candidates.
//!
//! Manages the lifecycle of matched market pairs: saving candidates from the
//! pipeline, manual verification, querying active pairs, and deactivating
//! expired ones.

use std::sync::Arc;

use anyhow::Result;
use chrono::Utc;
use serde::Deserialize;
use uuid::Uuid;

use arb_db::{models::MarketPairRow, Repository};
use crate::types::MatchCandidate;

/// Persistent store for market pairs backed by SQLite via `arb_db`.
pub struct PairStore {
    db: Arc<dyn Repository>,
}

/// A manually-configured pair from `config/pairs.toml`.
#[derive(Debug, Clone, Deserialize)]
pub struct TomlPair {
    pub poly_condition_id: String,
    pub poly_yes_token_id: String,
    pub poly_no_token_id: String,
    pub poly_question: String,
    pub kalshi_ticker: String,
    pub kalshi_question: String,
}

/// Root structure of `config/pairs.toml`.
#[derive(Debug, Clone, Deserialize)]
pub struct PairsConfig {
    #[serde(default)]
    pub pairs: Vec<TomlPair>,
}

impl PairStore {
    /// Create a new PairStore wrapping a shared database connection.
    pub fn new(db: Arc<dyn Repository>) -> Self {
        Self { db }
    }

    /// Load manually-configured pairs from a TOML file and insert any that
    /// are not already in the database.
    ///
    /// Skips pairs that already exist (matched by `poly_condition_id` +
    /// `kalshi_ticker`) to avoid duplicates on repeated calls.
    pub async fn load_from_toml(&self, path: &str) -> Result<usize> {
        let content = tokio::fs::read_to_string(path).await?;
        let config: PairsConfig = toml::from_str(&content)?;

        // Build a set of existing (poly_condition_id, kalshi_ticker) keys
        // so we can skip duplicates (BUG 2 fix).
        let existing = self.db.list_active_market_pairs().await?;
        let existing_keys: std::collections::HashSet<(String, String)> = existing
            .iter()
            .map(|p| (p.poly_condition_id.clone(), p.kalshi_ticker.clone()))
            .collect();

        let mut inserted = 0;
        for pair in &config.pairs {
            if existing_keys.contains(&(pair.poly_condition_id.clone(), pair.kalshi_ticker.clone()))
            {
                tracing::debug!(
                    poly = %pair.poly_condition_id,
                    kalshi = %pair.kalshi_ticker,
                    "pair already exists, skipping"
                );
                continue;
            }

            let now = Utc::now();
            let row = MarketPairRow {
                id: Uuid::now_v7().to_string(),
                poly_condition_id: pair.poly_condition_id.clone(),
                poly_yes_token_id: pair.poly_yes_token_id.clone(),
                poly_no_token_id: pair.poly_no_token_id.clone(),
                poly_question: pair.poly_question.clone(),
                kalshi_ticker: pair.kalshi_ticker.clone(),
                kalshi_question: pair.kalshi_question.clone(),
                match_confidence: 1.0, // manually configured = full confidence
                verified: true,
                active: true,
                close_time: now + chrono::Duration::days(365), // default 1 year; updated from API later (BUG 3 fix)
                created_at: now,
                updated_at: now,
            };
            self.db.insert_market_pair(&row).await?;
            inserted += 1;
        }

        tracing::info!(inserted, "loaded pairs from TOML config");
        Ok(inserted)
    }

    /// Save a match candidate to the database as an unverified pair.
    pub async fn save_candidate(&self, candidate: &MatchCandidate) -> Result<()> {
        let now = Utc::now();
        let row = MarketPairRow {
            id: Uuid::now_v7().to_string(),
            poly_condition_id: candidate.poly_market.platform_id.clone(),
            poly_yes_token_id: String::new(), // populated later from API
            poly_no_token_id: String::new(),  // populated later from API
            poly_question: candidate.poly_market.question.clone(),
            kalshi_ticker: candidate.kalshi_market.platform_id.clone(),
            kalshi_question: candidate.kalshi_market.question.clone(),
            match_confidence: candidate.score.composite,
            verified: false,
            active: true,
            close_time: candidate.poly_market.close_time.min(candidate.kalshi_market.close_time),
            created_at: now,
            updated_at: now,
        };
        self.db.insert_market_pair(&row).await?;
        tracing::info!(
            poly = %candidate.poly_market.question,
            kalshi = %candidate.kalshi_market.question,
            score = candidate.score.composite,
            "saved match candidate"
        );
        Ok(())
    }

    /// Mark a pair as verified by a human operator.
    pub async fn verify_pair(&self, pair_id: &Uuid) -> Result<()> {
        let row = self
            .db
            .get_market_pair(pair_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("pair not found: {pair_id}"))?;

        let updated = MarketPairRow {
            verified: true,
            updated_at: Utc::now(),
            ..row
        };
        self.db.update_market_pair(&updated).await?;
        tracing::info!(%pair_id, "pair verified");
        Ok(())
    }

    /// Return all pairs that are both verified and active.
    pub async fn active_verified_pairs(&self) -> Result<Vec<MarketPairRow>> {
        let all_active = self.db.list_active_market_pairs().await?;
        Ok(all_active.into_iter().filter(|p| p.verified).collect())
    }

    /// Deactivate pairs whose close time has passed.
    pub async fn deactivate_expired(&self) -> Result<usize> {
        let now = Utc::now();
        let active = self.db.list_active_market_pairs().await?;
        let mut deactivated = 0;

        for pair in active {
            if pair.close_time < now {
                let updated = MarketPairRow {
                    active: false,
                    updated_at: now,
                    ..pair
                };
                self.db.update_market_pair(&updated).await?;
                deactivated += 1;
            }
        }

        if deactivated > 0 {
            tracing::info!(deactivated, "deactivated expired pairs");
        }
        Ok(deactivated)
    }
}
