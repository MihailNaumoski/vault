use thiserror::Error;

/// Errors specific to the Polymarket connector.
#[derive(Debug, Error)]
pub enum PolymarketError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("auth error: {0}")]
    Auth(String),

    #[error("signing error: {0}")]
    Signing(String),

    #[error("websocket error: {0}")]
    WebSocket(String),

    #[error("rate limited")]
    RateLimited,

    #[error("api error {status}: {message}")]
    Api { status: u16, message: String },

    #[error("deserialization error: {0}")]
    Deserialize(#[from] serde_json::Error),
}

impl From<PolymarketError> for arb_types::ArbError {
    fn from(e: PolymarketError) -> Self {
        match e {
            PolymarketError::Http(inner) => arb_types::ArbError::PlatformError {
                platform: arb_types::Platform::Polymarket,
                message: inner.to_string(),
            },
            PolymarketError::Auth(msg) => arb_types::ArbError::AuthError {
                platform: arb_types::Platform::Polymarket,
                message: msg,
            },
            PolymarketError::Signing(msg) => arb_types::ArbError::AuthError {
                platform: arb_types::Platform::Polymarket,
                message: format!("signing: {msg}"),
            },
            PolymarketError::WebSocket(msg) => arb_types::ArbError::WebSocket(msg),
            PolymarketError::RateLimited => arb_types::ArbError::RateLimited {
                platform: arb_types::Platform::Polymarket,
                retry_after_ms: 1000,
            },
            PolymarketError::Api { status, message } => arb_types::ArbError::PlatformError {
                platform: arb_types::Platform::Polymarket,
                message: format!("HTTP {status}: {message}"),
            },
            PolymarketError::Deserialize(inner) => arb_types::ArbError::Serialization(inner),
        }
    }
}
