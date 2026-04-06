// Polymarket CLOB connector — REST + WebSocket + EIP-712 signing.

pub mod auth;
pub mod client;
pub mod connector;
pub mod error;
pub mod rate_limit;
pub mod signing;
pub mod types;
pub mod ws;

#[cfg(feature = "mock")]
pub mod mock;

pub use connector::PolymarketConnector;
pub use error::PolymarketError;
pub use types::PolyConfig;

#[cfg(feature = "mock")]
pub use mock::MockPolymarketConnector;
