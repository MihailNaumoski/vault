use thiserror::Error;

/// Errors specific to the Kalshi connector.
#[derive(Debug, Error)]
pub enum KalshiError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("auth error: {0}")]
    Auth(String),

    #[error("websocket error: {0}")]
    WebSocket(String),

    #[error("rate limited")]
    RateLimited,

    #[error("api error {status}: {message}")]
    Api { status: u16, message: String },

    #[error("deserialization error: {0}")]
    Deserialize(#[from] serde_json::Error),

    #[error("rsa error: {0}")]
    Rsa(String),
}

impl From<KalshiError> for arb_types::ArbError {
    fn from(e: KalshiError) -> Self {
        match e {
            KalshiError::Http(inner) => arb_types::ArbError::PlatformError {
                platform: arb_types::Platform::Kalshi,
                message: inner.to_string(),
            },
            KalshiError::Auth(msg) => arb_types::ArbError::AuthError {
                platform: arb_types::Platform::Kalshi,
                message: msg,
            },
            KalshiError::WebSocket(msg) => arb_types::ArbError::WebSocket(msg),
            KalshiError::RateLimited => arb_types::ArbError::RateLimited {
                platform: arb_types::Platform::Kalshi,
                retry_after_ms: 1000,
            },
            KalshiError::Api { status, message } => arb_types::ArbError::PlatformError {
                platform: arb_types::Platform::Kalshi,
                message: format!("API error {status}: {message}"),
            },
            KalshiError::Deserialize(inner) => arb_types::ArbError::Serialization(inner),
            KalshiError::Rsa(msg) => arb_types::ArbError::AuthError {
                platform: arb_types::Platform::Kalshi,
                message: msg,
            },
        }
    }
}
