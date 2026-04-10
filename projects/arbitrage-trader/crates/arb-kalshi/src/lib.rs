//! Kalshi prediction market connector.
//!
//! Provides REST + WebSocket client, RSA-SHA256 authentication, and
//! dual rate limiting (10 req/s trading, 100 req/s market data).

pub mod auth;
pub mod client;
pub mod connector;
pub mod error;
pub mod rate_limit;
pub mod types;
pub mod ws;

#[cfg(feature = "mock")]
pub mod mock;

pub use connector::KalshiConnector;
pub use error::KalshiError;
pub use types::{KalshiConfig, KalshiEventResponse};

#[cfg(feature = "mock")]
pub use mock::MockKalshiConnector;
