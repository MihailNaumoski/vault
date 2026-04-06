use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use parking_lot::RwLock;
use tokio::sync::mpsc;
use tracing::{debug, error, warn};

use arb_types::{Platform, PriceUpdate, SubHandle};

use crate::error::PolymarketError;
use crate::types::PolyWsMessage;

/// Reconnection policy with exponential backoff and jitter.
#[derive(Debug, Clone)]
pub struct ReconnectPolicy {
    pub initial_delay: Duration,
    pub max_delay: Duration,
    pub jitter_factor: f64,
    pub max_consecutive_failures: u32,
}

impl Default for ReconnectPolicy {
    fn default() -> Self {
        Self {
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(30),
            jitter_factor: 0.25,
            max_consecutive_failures: 10,
        }
    }
}

impl ReconnectPolicy {
    /// Calculate the delay for a given failure count.
    pub fn delay_for(&self, failures: u32) -> Duration {
        let base_ms = self.initial_delay.as_millis() as f64 * 2.0f64.powi(failures as i32);
        let capped_ms = base_ms.min(self.max_delay.as_millis() as f64);

        // Apply jitter: +/- jitter_factor
        let jitter_range = capped_ms * self.jitter_factor;
        let jitter = (rand::random::<f64>() * 2.0 - 1.0) * jitter_range;
        let final_ms = (capped_ms + jitter).max(0.0);

        Duration::from_millis(final_ms as u64)
    }
}

/// Polymarket WebSocket client for real-time price feeds.
pub struct PolyWebSocket {
    url: String,
    subscribed_ids: RwLock<Vec<String>>,
    reconnect_policy: ReconnectPolicy,
}

impl PolyWebSocket {
    /// Create a new WebSocket client pointing at the given URL.
    pub fn new(url: String) -> Self {
        Self {
            url,
            subscribed_ids: RwLock::new(Vec::new()),
            reconnect_policy: ReconnectPolicy::default(),
        }
    }

    /// Subscribe to price updates for the given token IDs.
    ///
    /// Spawns a background task that connects, subscribes, parses messages,
    /// and forwards `PriceUpdate` values through the provided sender.
    /// Returns a `SubHandle` whose cancel signal stops the background task.
    pub async fn subscribe(
        &self,
        token_ids: &[String],
        tx: mpsc::Sender<PriceUpdate>,
    ) -> Result<SubHandle, PolymarketError> {
        // Store subscribed IDs for reconnection
        {
            let mut ids = self.subscribed_ids.write();
            for id in token_ids {
                if !ids.contains(id) {
                    ids.push(id.clone());
                }
            }
        }

        let url = self.url.clone();
        let ids = token_ids.to_vec();
        let policy = self.reconnect_policy.clone();

        let (cancel_tx, mut cancel_rx) = tokio::sync::oneshot::channel::<()>();

        tokio::spawn(async move {
            let mut failures: u32 = 0;

            loop {
                match tokio_tungstenite::connect_async(&url).await {
                    Ok((ws_stream, _)) => {
                        debug!("WebSocket connected to {}", url);
                        failures = 0;

                        let (mut write, mut read) = ws_stream.split();

                        // Subscribe to all token IDs — format matches official Polymarket Rust SDK
                        let sub_msg = serde_json::json!({
                            "type": "market",
                            "assets_ids": ids,
                            "custom_feature_enabled": true
                        });
                        debug!("Sending WS subscription: {}", sub_msg);
                        if let Err(e) = write
                            .send(tokio_tungstenite::tungstenite::Message::Text(
                                sub_msg.to_string().into(),
                            ))
                            .await
                        {
                            warn!("Failed to send subscribe message: {e}");
                            break; // trigger reconnect
                        }

                        // Read messages with staleness detection
                        let mut last_message_time = tokio::time::Instant::now();
                        let ping_interval = Duration::from_secs(5);
                        let stale_timeout = Duration::from_secs(30);

                        loop {
                            tokio::select! {
                                msg = read.next() => {
                                    match msg {
                                        Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => {
                                            last_message_time = tokio::time::Instant::now();
                                            if &*text == "PONG" {
                                                continue;
                                            }
                                            for update in parse_ws_messages(text.as_ref()) {
                                                if tx.send(update).await.is_err() {
                                                    debug!("Price update receiver dropped, stopping WS");
                                                    return;
                                                }
                                            }
                                        }
                                        Some(Ok(tokio_tungstenite::tungstenite::Message::Pong(_))) => {
                                            last_message_time = tokio::time::Instant::now();
                                        }
                                        Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) => {
                                            debug!("WebSocket closed by server");
                                            break;
                                        }
                                        Some(Err(e)) => {
                                            warn!("WebSocket read error: {e}");
                                            break;
                                        }
                                        None => {
                                            debug!("WebSocket stream ended");
                                            break;
                                        }
                                        _ => {} // binary
                                    }
                                }
                                _ = tokio::time::sleep(ping_interval) => {
                                    // Check staleness
                                    if last_message_time.elapsed() > stale_timeout {
                                        warn!("WebSocket stale (no messages for {:?}), reconnecting", stale_timeout);
                                        break;
                                    }
                                    // Send active ping (text-based per Polymarket docs)
                                    if let Err(e) = write
                                        .send(tokio_tungstenite::tungstenite::Message::Text("PING".into()))
                                        .await
                                    {
                                        warn!("Failed to send ping: {e}");
                                        break;
                                    }
                                }
                                _ = &mut cancel_rx => {
                                    debug!("WebSocket subscription cancelled");
                                    let _ = write.close().await;
                                    return;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        warn!("WebSocket connection failed: {e}");
                    }
                }

                // Reconnection logic
                failures += 1;
                if failures >= policy.max_consecutive_failures {
                    error!(
                        "WebSocket max consecutive failures ({}) reached, stopping",
                        policy.max_consecutive_failures
                    );
                    return;
                }

                let delay = policy.delay_for(failures);
                debug!("Reconnecting in {:?} (attempt {failures})", delay);

                tokio::select! {
                    _ = tokio::time::sleep(delay) => {}
                    _ = &mut cancel_rx => {
                        debug!("WebSocket subscription cancelled during backoff");
                        return;
                    }
                }
            }
        });

        Ok(SubHandle { cancel_tx })
    }
}

/// Parse a raw WebSocket text message into price updates.
/// Returns all parseable updates — the price cache filters by recognized token ID.
pub fn parse_ws_messages(text: &str) -> Vec<PriceUpdate> {
    let msg: PolyWsMessage = match serde_json::from_str(text) {
        Ok(m) => m,
        Err(_) => return vec![],
    };
    match msg {
        PolyWsMessage::Book { .. } => {
            // Book is a full order book dump — individual levels are not
            // reliable as "the price". Use best_bid_ask or price_change instead.
            vec![]
        }
        PolyWsMessage::BestBidAsk {
            asset_id,
            best_bid,
            best_ask,
        } => {
            let bid: rust_decimal::Decimal = match best_bid.parse() { Ok(v) => v, Err(_) => return vec![] };
            let ask: rust_decimal::Decimal = match best_ask.parse() { Ok(v) => v, Err(_) => return vec![] };
            let yes_price = (bid + ask) / rust_decimal::Decimal::from(2);
            let no_price = rust_decimal::Decimal::ONE - yes_price;
            vec![PriceUpdate {
                platform: Platform::Polymarket,
                market_id: asset_id,
                yes_price,
                no_price,
                timestamp: chrono::Utc::now(),
            }]
        }
        PolyWsMessage::PriceChange {
            price_changes, ..
        } => {
            // Use best_bid/best_ask midpoint as the market price (not the
            // order-book level in `price`). The price cache filters by token ID.
            price_changes
                .iter()
                .filter_map(|entry| {
                    let bid: rust_decimal::Decimal = entry.best_bid.as_deref()?.parse().ok()?;
                    let ask: rust_decimal::Decimal = entry.best_ask.as_deref()?.parse().ok()?;
                    let yes_price = (bid + ask) / rust_decimal::Decimal::from(2);
                    let no_price = rust_decimal::Decimal::ONE - yes_price;
                    Some(PriceUpdate {
                        platform: Platform::Polymarket,
                        market_id: entry.asset_id.clone(),
                        yes_price,
                        no_price,
                        timestamp: chrono::Utc::now(),
                    })
                })
                .collect()
        }
        PolyWsMessage::LastTradePrice {
            asset_id, price, ..
        } => {
            if let Ok(yes_price) = price.parse::<rust_decimal::Decimal>() {
                let no_price = rust_decimal::Decimal::ONE - yes_price;
                vec![PriceUpdate {
                    platform: Platform::Polymarket,
                    market_id: asset_id,
                    yes_price,
                    no_price,
                    timestamp: chrono::Utc::now(),
                }]
            } else {
                vec![]
            }
        }
        PolyWsMessage::Unknown => vec![],
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_book_message_ignored_for_pricing() {
        let msg = r#"{
            "event_type": "book",
            "asset_id": "tok-123",
            "market": "mkt-1",
            "bids": [{"price": "0.45", "size": "100"}],
            "asks": [{"price": "0.55", "size": "200"}],
            "timestamp": "1700000000"
        }"#;
        // Book events are not used for pricing — only best_bid_ask and price_change are
        assert!(parse_ws_messages(msg).is_empty());
    }

    #[test]
    fn test_parse_best_bid_ask() {
        let msg = r#"{
            "event_type": "best_bid_ask",
            "asset_id": "tok-123",
            "market": "mkt-1",
            "best_bid": "0.54",
            "best_ask": "0.56",
            "spread": "0.02",
            "timestamp": "1700000000"
        }"#;
        let update = parse_ws_messages(msg).into_iter().next().unwrap();
        assert_eq!(update.platform, Platform::Polymarket);
        assert_eq!(update.market_id, "tok-123");
        assert_eq!(update.yes_price, rust_decimal_macros::dec!(0.55));
        assert_eq!(update.no_price, rust_decimal_macros::dec!(0.45));
    }

    #[test]
    fn test_parse_last_trade_price() {
        let msg = r#"{
            "event_type": "last_trade_price",
            "asset_id": "tok-456",
            "market": "mkt-2",
            "price": "0.70"
        }"#;

        let update = parse_ws_messages(msg).into_iter().next().unwrap();
        assert_eq!(update.market_id, "tok-456");
        assert_eq!(update.yes_price, rust_decimal_macros::dec!(0.70));
        assert_eq!(update.no_price, rust_decimal_macros::dec!(0.30));
    }

    #[test]
    fn test_parse_price_change() {
        let msg = r#"{
            "event_type": "price_change",
            "market": "mkt-3",
            "price_changes": [{"asset_id": "tok-789", "price": "0.75", "side": "buy", "best_bid": "0.79", "best_ask": "0.81"}],
            "timestamp": "123"
        }"#;

        let update = parse_ws_messages(msg).into_iter().next().unwrap();
        assert_eq!(update.market_id, "tok-789");
        // Midpoint of best_bid (0.79) and best_ask (0.81) = 0.80
        assert_eq!(update.yes_price, rust_decimal_macros::dec!(0.80));
    }

    #[test]
    fn test_parse_unknown_message() {
        let msg = r#"{"event_type": "tick_size_change", "data": {}}"#;
        assert!(parse_ws_messages(msg).is_empty());
    }

    #[test]
    fn test_reconnect_policy_backoff() {
        let policy = ReconnectPolicy {
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(30),
            jitter_factor: 0.0, // no jitter for deterministic test
            max_consecutive_failures: 10,
        };

        // failure 0 -> 1s, 1 -> 2s, 2 -> 4s, 3 -> 8s, 4 -> 16s, 5 -> 30s (capped)
        assert_eq!(policy.delay_for(0), Duration::from_secs(1));
        assert_eq!(policy.delay_for(1), Duration::from_secs(2));
        assert_eq!(policy.delay_for(2), Duration::from_secs(4));
        assert_eq!(policy.delay_for(3), Duration::from_secs(8));
        assert_eq!(policy.delay_for(4), Duration::from_secs(16));
        assert_eq!(policy.delay_for(5), Duration::from_secs(30)); // capped
        assert_eq!(policy.delay_for(10), Duration::from_secs(30)); // still capped
    }

    #[test]
    fn test_reconnect_policy_jitter() {
        let policy = ReconnectPolicy {
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(30),
            jitter_factor: 0.25,
            max_consecutive_failures: 10,
        };

        // With 25% jitter, delay for failure 0 should be between 750ms and 1250ms
        for _ in 0..50 {
            let delay = policy.delay_for(0);
            let ms = delay.as_millis();
            assert!(ms >= 750, "delay too low: {ms}ms");
            assert!(ms <= 1250, "delay too high: {ms}ms");
        }
    }
}
