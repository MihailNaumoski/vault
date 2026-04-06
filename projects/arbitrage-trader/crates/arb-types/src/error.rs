use thiserror::Error;

use crate::Platform;

/// Errors that can occur across the arbitrage system.
#[derive(Debug, Error)]
pub enum ArbError {
    #[error("platform error on {platform}: {message}")]
    PlatformError {
        platform: Platform,
        message: String,
    },

    #[error("authentication failed for {platform}: {message}")]
    AuthError {
        platform: Platform,
        message: String,
    },

    #[error("rate limited on {platform}, retry after {retry_after_ms}ms")]
    RateLimited {
        platform: Platform,
        retry_after_ms: u64,
    },

    #[error("order rejected on {platform}: {reason}")]
    OrderRejected {
        platform: Platform,
        reason: String,
    },

    #[error("invalid price: {0}")]
    InvalidPrice(String),

    #[error("market not found: {0}")]
    MarketNotFound(String),

    #[error("pair not verified: {0}")]
    PairNotVerified(String),

    #[error("database error: {0}")]
    Database(String),

    #[error("configuration error: {0}")]
    Config(String),

    #[error("websocket error: {0}")]
    WebSocket(String),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("{0}")]
    Other(String),
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Platform::Polymarket => write!(f, "polymarket"),
            Platform::Kalshi => write!(f, "kalshi"),
        }
    }
}
