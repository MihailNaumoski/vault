use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::Platform;

/// A real-time price update from a platform WebSocket feed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceUpdate {
    pub platform: Platform,
    pub market_id: String,
    pub yes_price: Decimal,
    pub no_price: Decimal,
    pub timestamp: DateTime<Utc>,
}

/// Handle for a WebSocket subscription that can be used to unsubscribe.
pub struct SubHandle {
    pub cancel_tx: tokio::sync::oneshot::Sender<()>,
}

impl SubHandle {
    /// Cancel this subscription.
    pub fn cancel(self) {
        let _ = self.cancel_tx.send(());
    }
}
