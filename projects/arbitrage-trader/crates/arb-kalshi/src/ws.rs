use futures_util::{SinkExt, StreamExt};
use parking_lot::RwLock;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, error, info, warn};

use arb_types::{price::kalshi_cents_to_decimal, Platform, PriceUpdate, SubHandle};

use crate::auth::KalshiAuth;
use crate::error::KalshiError;
use crate::types::{KalshiWsEnvelope, KalshiWsMessage};

/// Reconnection policy with exponential backoff and jitter.
#[derive(Debug, Clone)]
struct ReconnectPolicy {
    initial_delay: Duration,
    max_delay: Duration,
    jitter_factor: f64,
    max_consecutive_failures: u32,
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
    /// Compute the delay for a given failure count.
    fn delay_for_attempt(&self, attempt: u32) -> Duration {
        let base = self
            .initial_delay
            .as_secs_f64()
            * 2.0_f64.powi(attempt as i32);
        let capped = base.min(self.max_delay.as_secs_f64());
        let jitter_range = capped * self.jitter_factor;
        // Simple deterministic jitter: alternate adding/subtracting
        let jittered = if attempt.is_multiple_of(2) {
            capped + jitter_range * 0.5
        } else {
            capped - jitter_range * 0.5
        };
        Duration::from_secs_f64(jittered.max(0.1))
    }
}

/// WebSocket client for the Kalshi real-time data feed.
///
/// Handles:
/// - Authenticated connection via HTTP upgrade headers (RSA-PSS signed)
/// - Channel subscriptions: `orderbook_delta`, `ticker`, `fill`
/// - Automatic reconnection with exponential backoff
/// - Price conversion from cents/dollars to Decimal on ingestion
pub struct KalshiWebSocket {
    url: String,
    auth: KalshiAuth,
    subscribed_ids: Arc<RwLock<Vec<String>>>,
}

impl KalshiWebSocket {
    /// Create a new WebSocket client.
    pub fn new(url: String, auth: KalshiAuth) -> Self {
        Self {
            url,
            auth,
            subscribed_ids: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Subscribe to price updates for the given market tickers.
    ///
    /// Spawns a background task that:
    /// 1. Connects to the Kalshi WebSocket endpoint with auth headers
    /// 2. Subscribes to `orderbook_delta` and `ticker` channels
    /// 3. Parses incoming messages (envelope -> inner) into `PriceUpdate`
    /// 4. Forwards updates to the provided sender
    /// 5. Reconnects with exponential backoff on disconnect
    ///
    /// Returns a `SubHandle` whose `cancel_tx` stops the background task.
    pub async fn subscribe(
        &self,
        tickers: &[String],
        tx: mpsc::Sender<PriceUpdate>,
    ) -> Result<SubHandle, KalshiError> {
        // Store subscription IDs for reconnection
        {
            let mut ids = self.subscribed_ids.write();
            ids.clear();
            ids.extend(tickers.iter().cloned());
        }

        let (cancel_tx, cancel_rx) = oneshot::channel::<()>();
        let url = self.url.clone();
        let auth = self.auth.clone();
        let subscribed_ids = self.subscribed_ids.clone();
        let tickers_owned: Vec<String> = tickers.to_vec();

        tokio::spawn(async move {
            ws_task(url, auth, subscribed_ids, tickers_owned, tx, cancel_rx).await;
        });

        Ok(SubHandle { cancel_tx })
    }
}

/// Build a channel subscription message.
pub(crate) fn build_subscribe_message(
    id: u64,
    channels: &[&str],
    tickers: &[String],
) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "cmd": "subscribe",
        "params": {
            "channels": channels,
            "market_tickers": tickers
        }
    })
}

// ---------------------------------------------------------------------------
// Inner deserialization types for WebSocket message payloads
// ---------------------------------------------------------------------------

mod ws_inner {
    use serde::Deserialize;

    #[derive(Deserialize)]
    pub struct OrderbookDeltaMsg {
        pub market_ticker: String,
        #[serde(default)]
        pub price_dollars: Option<String>,
        #[serde(default)]
        pub delta_fp: Option<String>,
        #[serde(default)]
        pub side: Option<String>,
    }

    #[derive(Deserialize)]
    pub struct OrderbookSnapshotMsg {
        pub market_ticker: String,
        #[serde(default)]
        pub yes: Vec<Vec<serde_json::Value>>,
        #[serde(default)]
        pub no: Vec<Vec<serde_json::Value>>,
    }

    #[derive(Deserialize)]
    pub struct TickerMsg {
        pub market_ticker: String,
        #[serde(default)]
        pub yes_price: Option<u32>,
        #[serde(default)]
        pub no_price: Option<u32>,
        #[serde(default)]
        pub volume: Option<u64>,
        #[serde(default)]
        pub price_dollars: Option<String>,
        #[serde(default)]
        pub yes_bid_dollars: Option<String>,
        #[serde(default)]
        pub yes_ask_dollars: Option<String>,
        #[serde(default)]
        pub volume_fp: Option<String>,
        #[serde(default)]
        pub open_interest_fp: Option<String>,
        #[serde(default)]
        pub yes_bid_size_fp: Option<String>,
        #[serde(default)]
        pub yes_ask_size_fp: Option<String>,
        #[serde(default)]
        pub last_trade_size_fp: Option<String>,
        #[serde(default)]
        pub ts: Option<u64>,
        #[serde(default)]
        pub time: Option<String>,
    }

    #[derive(Deserialize)]
    pub struct FillMsg {
        pub order_id: String,
        #[serde(default)]
        pub count: Option<u32>,
        #[serde(default)]
        pub remaining_count: Option<u32>,
        #[serde(default)]
        pub side: Option<String>,
        #[serde(default)]
        pub yes_price: Option<u32>,
        #[serde(default)]
        pub no_price: Option<u32>,
        #[serde(default)]
        pub trade_id: Option<String>,
        #[serde(default)]
        pub market_ticker: Option<String>,
        #[serde(default)]
        pub is_taker: Option<bool>,
        #[serde(default)]
        pub yes_price_dollars: Option<String>,
        #[serde(default)]
        pub count_fp: Option<String>,
        #[serde(default)]
        pub fee_cost: Option<String>,
        #[serde(default)]
        pub action: Option<String>,
        #[serde(default)]
        pub ts: Option<u64>,
        #[serde(default)]
        pub client_order_id: Option<String>,
        #[serde(default)]
        pub post_position_fp: Option<String>,
        #[serde(default)]
        pub purchased_side: Option<String>,
        #[serde(default)]
        pub subaccount: Option<String>,
    }

    #[derive(Deserialize)]
    pub struct SubscribedMsg {
        #[serde(default)]
        pub channel: Option<String>,
        #[serde(default)]
        pub sid: Option<u64>,
    }

    #[derive(Deserialize)]
    pub struct ErrorMsg {
        #[serde(default)]
        pub code: Option<u64>,
        #[serde(default)]
        pub msg: Option<String>,
    }
}

/// Parse a raw WebSocket text message into a `KalshiWsMessage`.
///
/// Uses two-step parsing: envelope first, then inner msg based on type.
pub(crate) fn parse_ws_message(text: &str) -> Option<KalshiWsMessage> {
    let envelope: KalshiWsEnvelope = serde_json::from_str(text).ok()?;

    match envelope.msg_type.as_str() {
        "orderbook_delta" => {
            let msg = envelope.msg?;
            let inner: ws_inner::OrderbookDeltaMsg = serde_json::from_value(msg).ok()?;
            Some(KalshiWsMessage::OrderbookDelta {
                market_ticker: inner.market_ticker,
                price_dollars: inner.price_dollars,
                delta_fp: inner.delta_fp,
                side: inner.side,
            })
        }
        "orderbook_snapshot" => {
            let msg = envelope.msg?;
            let inner: ws_inner::OrderbookSnapshotMsg = serde_json::from_value(msg).ok()?;
            Some(KalshiWsMessage::OrderbookSnapshot {
                market_ticker: inner.market_ticker,
                yes: inner.yes,
                no: inner.no,
            })
        }
        "ticker" => {
            let msg = envelope.msg?;
            let inner: ws_inner::TickerMsg = serde_json::from_value(msg).ok()?;
            Some(KalshiWsMessage::Ticker {
                market_ticker: inner.market_ticker,
                yes_price: inner.yes_price,
                no_price: inner.no_price,
                volume: inner.volume,
                price_dollars: inner.price_dollars,
                yes_bid_dollars: inner.yes_bid_dollars,
                yes_ask_dollars: inner.yes_ask_dollars,
                volume_fp: inner.volume_fp,
                open_interest_fp: inner.open_interest_fp,
                yes_bid_size_fp: inner.yes_bid_size_fp,
                yes_ask_size_fp: inner.yes_ask_size_fp,
                last_trade_size_fp: inner.last_trade_size_fp,
                ts: inner.ts,
                time: inner.time,
            })
        }
        "fill" => {
            let msg = envelope.msg?;
            let inner: ws_inner::FillMsg = serde_json::from_value(msg).ok()?;
            Some(KalshiWsMessage::Fill {
                order_id: inner.order_id,
                count: inner.count,
                remaining_count: inner.remaining_count,
                side: inner.side,
                yes_price: inner.yes_price,
                no_price: inner.no_price,
                trade_id: inner.trade_id,
                market_ticker: inner.market_ticker,
                is_taker: inner.is_taker,
                yes_price_dollars: inner.yes_price_dollars,
                count_fp: inner.count_fp,
                fee_cost: inner.fee_cost,
                action: inner.action,
                ts: inner.ts,
                client_order_id: inner.client_order_id,
                post_position_fp: inner.post_position_fp,
                purchased_side: inner.purchased_side,
                subaccount: inner.subaccount,
            })
        }
        "subscribed" => {
            let msg = envelope.msg.unwrap_or(serde_json::Value::Object(Default::default()));
            let inner: ws_inner::SubscribedMsg = serde_json::from_value(msg).ok()?;
            Some(KalshiWsMessage::Subscribed {
                channel: inner.channel,
                sid: inner.sid,
            })
        }
        "error" => {
            let msg = envelope.msg.unwrap_or(serde_json::Value::Object(Default::default()));
            let inner: ws_inner::ErrorMsg = serde_json::from_value(msg).ok()?;
            Some(KalshiWsMessage::Error {
                code: inner.code,
                msg: inner.msg,
            })
        }
        _ => Some(KalshiWsMessage::Other),
    }
}

/// Convert a `KalshiWsMessage` into a `PriceUpdate`, if applicable.
pub(crate) fn ws_message_to_price_update(msg: &KalshiWsMessage) -> Option<PriceUpdate> {
    match msg {
        KalshiWsMessage::Ticker {
            market_ticker,
            yes_price,
            no_price,
            yes_bid_dollars,
            yes_ask_dollars,
            ..
        } => {
            // Prefer dollar fields, fall back to cents
            let yes_p = yes_bid_dollars
                .as_ref()
                .and_then(|s| s.parse::<rust_decimal::Decimal>().ok())
                .or_else(|| yes_price.map(kalshi_cents_to_decimal))
                .unwrap_or_default();

            let no_p = yes_ask_dollars
                .as_ref()
                .and_then(|s| s.parse::<rust_decimal::Decimal>().ok())
                .map(|ask| rust_decimal_macros::dec!(1) - ask)
                .or_else(|| no_price.map(kalshi_cents_to_decimal))
                .unwrap_or_default();

            Some(PriceUpdate {
                platform: Platform::Kalshi,
                market_id: market_ticker.clone(),
                yes_price: yes_p,
                no_price: no_p,
                timestamp: chrono::Utc::now(),
            })
        }
        KalshiWsMessage::OrderbookSnapshot {
            market_ticker,
            yes,
            no,
        } => {
            // Extract best prices from snapshot
            let yes_price = yes
                .first()
                .and_then(|level| level.first())
                .and_then(|v| match v {
                    serde_json::Value::String(s) => s.parse::<rust_decimal::Decimal>().ok(),
                    serde_json::Value::Number(n) => {
                        n.as_u64().map(|c| kalshi_cents_to_decimal(c as u32))
                    }
                    _ => None,
                })
                .unwrap_or_default();
            let no_price = no
                .first()
                .and_then(|level| level.first())
                .and_then(|v| match v {
                    serde_json::Value::String(s) => s.parse::<rust_decimal::Decimal>().ok(),
                    serde_json::Value::Number(n) => {
                        n.as_u64().map(|c| kalshi_cents_to_decimal(c as u32))
                    }
                    _ => None,
                })
                .unwrap_or_default();
            Some(PriceUpdate {
                platform: Platform::Kalshi,
                market_id: market_ticker.clone(),
                yes_price,
                no_price,
                timestamp: chrono::Utc::now(),
            })
        }
        // Deltas need local orderbook state to compute full prices
        KalshiWsMessage::OrderbookDelta { .. } => None,
        _ => None,
    }
}

/// Background task managing the WebSocket connection lifecycle.
async fn ws_task(
    url: String,
    auth: KalshiAuth,
    _subscribed_ids: Arc<RwLock<Vec<String>>>,
    tickers: Vec<String>,
    tx: mpsc::Sender<PriceUpdate>,
    mut cancel_rx: oneshot::Receiver<()>,
) {
    let policy = ReconnectPolicy::default();
    let mut consecutive_failures: u32 = 0;

    loop {
        // Check cancellation before attempting connection
        if cancel_rx.try_recv().is_ok() {
            debug!("Kalshi WS task cancelled");
            return;
        }

        match connect_and_run(&url, &auth, &tickers, &tx, &mut cancel_rx).await {
            Ok(()) => {
                // Clean exit (cancelled)
                debug!("Kalshi WS task exiting cleanly");
                return;
            }
            Err(e) => {
                consecutive_failures += 1;
                warn!(
                    error = %e,
                    failures = consecutive_failures,
                    "Kalshi WS disconnected"
                );

                if consecutive_failures >= policy.max_consecutive_failures {
                    error!(
                        "Kalshi WS exceeded max consecutive failures ({}), giving up",
                        policy.max_consecutive_failures
                    );
                    return;
                }

                let delay = policy.delay_for_attempt(consecutive_failures - 1);
                info!("Kalshi WS reconnecting in {:?}", delay);

                tokio::select! {
                    _ = tokio::time::sleep(delay) => {},
                    _ = &mut cancel_rx => {
                        debug!("Kalshi WS cancelled during backoff");
                        return;
                    }
                }
            }
        }
    }
}

/// Connect to WebSocket with auth headers, subscribe, and process messages.
///
/// Auth is done via HTTP headers during the WebSocket upgrade handshake.
/// Returns `Ok(())` on clean cancellation, `Err` on connection failure.
async fn connect_and_run(
    url: &str,
    auth: &KalshiAuth,
    tickers: &[String],
    tx: &mpsc::Sender<PriceUpdate>,
    cancel_rx: &mut oneshot::Receiver<()>,
) -> Result<(), KalshiError> {
    use tokio_tungstenite::tungstenite::Message;

    // Build authenticated WebSocket request with auth headers in the upgrade
    let auth_headers = auth.headers("GET", "/trade-api/ws/v2")?;
    let request = tokio_tungstenite::tungstenite::http::Request::builder()
        .uri(url)
        .header(
            "KALSHI-ACCESS-KEY",
            auth_headers
                .get("KALSHI-ACCESS-KEY")
                .expect("auth headers missing key")
                .to_str()
                .unwrap(),
        )
        .header(
            "KALSHI-ACCESS-SIGNATURE",
            auth_headers
                .get("KALSHI-ACCESS-SIGNATURE")
                .expect("auth headers missing signature")
                .to_str()
                .unwrap(),
        )
        .header(
            "KALSHI-ACCESS-TIMESTAMP",
            auth_headers
                .get("KALSHI-ACCESS-TIMESTAMP")
                .expect("auth headers missing timestamp")
                .to_str()
                .unwrap(),
        )
        .header("Host", url.replace("wss://", "").replace("ws://", "").split('/').next().unwrap_or(""))
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header("Sec-WebSocket-Key", tokio_tungstenite::tungstenite::handshake::client::generate_key())
        .body(())
        .map_err(|e| KalshiError::WebSocket(format!("failed to build ws request: {e}")))?;

    let (ws_stream, _) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|e| KalshiError::WebSocket(format!("connect failed: {e}")))?;

    let (mut write, mut read) = ws_stream.split();

    // Subscribe to channels (auth is done via HTTP headers, no separate auth step)
    let orderbook_sub = build_subscribe_message(1, &["orderbook_delta"], tickers);
    write
        .send(Message::Text(orderbook_sub.to_string().into()))
        .await
        .map_err(|e| KalshiError::WebSocket(format!("subscribe send failed: {e}")))?;

    let ticker_sub = build_subscribe_message(2, &["ticker"], tickers);
    write
        .send(Message::Text(ticker_sub.to_string().into()))
        .await
        .map_err(|e| KalshiError::WebSocket(format!("subscribe send failed: {e}")))?;

    debug!("Kalshi WS subscribed to {} tickers", tickers.len());

    // Process messages with staleness detection
    let mut last_message_time = tokio::time::Instant::now();
    let ping_interval = std::time::Duration::from_secs(30);
    let stale_timeout = std::time::Duration::from_secs(90);

    loop {
        tokio::select! {
            msg = read.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        last_message_time = tokio::time::Instant::now();
                        if let Some(ws_msg) = parse_ws_message(text.as_ref()) {
                            if let Some(price_update) = ws_message_to_price_update(&ws_msg) {
                                if tx.send(price_update).await.is_err() {
                                    debug!("Kalshi WS receiver dropped, exiting");
                                    return Ok(());
                                }
                            }
                        }
                    }
                    Some(Ok(Message::Ping(data))) => {
                        last_message_time = tokio::time::Instant::now();
                        let _ = write.send(Message::Pong(data)).await;
                    }
                    Some(Ok(Message::Pong(_))) => {
                        last_message_time = tokio::time::Instant::now();
                    }
                    Some(Ok(Message::Close(_))) => {
                        return Err(KalshiError::WebSocket("server closed connection".to_string()));
                    }
                    Some(Err(e)) => {
                        return Err(KalshiError::WebSocket(format!("read error: {e}")));
                    }
                    None => {
                        return Err(KalshiError::WebSocket("stream ended".to_string()));
                    }
                    _ => {}
                }
            }
            _ = tokio::time::sleep(ping_interval) => {
                if last_message_time.elapsed() > stale_timeout {
                    warn!("Kalshi WS stale (no messages for {:?}), reconnecting", stale_timeout);
                    return Err(KalshiError::WebSocket("stale connection".to_string()));
                }
                // Send active ping
                if let Err(e) = write.send(Message::Ping(vec![].into())).await {
                    warn!("Kalshi WS ping failed: {e}");
                    return Err(KalshiError::WebSocket(format!("ping failed: {e}")));
                }
            }
            _ = &mut *cancel_rx => {
                debug!("Kalshi WS task cancelled");
                let _ = write.send(Message::Close(None)).await;
                return Ok(());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_parse_orderbook_snapshot() {
        let json = r#"{
            "type": "orderbook_snapshot",
            "sid": 1,
            "seq": 1,
            "msg": {
                "market_ticker": "PRES-2026-DEM",
                "yes": [["0.42", "100.00"], ["0.41", "200.00"]],
                "no": [["0.58", "100.00"], ["0.59", "200.00"]]
            }
        }"#;
        let msg = parse_ws_message(json).unwrap();
        let update = ws_message_to_price_update(&msg).unwrap();
        assert_eq!(update.platform, Platform::Kalshi);
        assert_eq!(update.market_id, "PRES-2026-DEM");
        assert_eq!(update.yes_price, dec!(0.42));
        assert_eq!(update.no_price, dec!(0.58));
    }

    #[test]
    fn test_parse_orderbook_delta() {
        let json = r#"{
            "type": "orderbook_delta",
            "sid": 1,
            "seq": 2,
            "msg": {
                "market_ticker": "PRES-2026-DEM",
                "price_dollars": "0.42",
                "delta_fp": "-10.00",
                "side": "yes"
            }
        }"#;
        let msg = parse_ws_message(json).unwrap();
        // Deltas need local state, so they return None for PriceUpdate
        assert!(ws_message_to_price_update(&msg).is_none());
        match msg {
            KalshiWsMessage::OrderbookDelta {
                market_ticker,
                price_dollars,
                ..
            } => {
                assert_eq!(market_ticker, "PRES-2026-DEM");
                assert_eq!(price_dollars, Some("0.42".to_string()));
            }
            _ => panic!("expected OrderbookDelta"),
        }
    }

    #[test]
    fn test_parse_ticker_message() {
        let json = r#"{
            "type": "ticker",
            "sid": 2,
            "seq": 1,
            "msg": {
                "market_ticker": "PRES-2026-DEM",
                "yes_price": 55,
                "no_price": 45,
                "volume": 5000,
                "yes_bid_dollars": "0.55",
                "yes_ask_dollars": "0.56"
            }
        }"#;
        let msg = parse_ws_message(json).unwrap();
        let update = ws_message_to_price_update(&msg).unwrap();
        assert_eq!(update.platform, Platform::Kalshi);
        assert_eq!(update.market_id, "PRES-2026-DEM");
        // Should prefer dollar fields: yes_bid_dollars = 0.55
        assert_eq!(update.yes_price, dec!(0.55));
        // no_price from 1 - yes_ask_dollars: 1 - 0.56 = 0.44
        assert_eq!(update.no_price, dec!(0.44));
    }

    #[test]
    fn test_parse_ticker_message_cents_fallback() {
        let json = r#"{
            "type": "ticker",
            "sid": 2,
            "seq": 1,
            "msg": {
                "market_ticker": "PRES-2026-DEM",
                "yes_price": 55,
                "no_price": 45,
                "volume": 5000
            }
        }"#;
        let msg = parse_ws_message(json).unwrap();
        let update = ws_message_to_price_update(&msg).unwrap();
        assert_eq!(update.yes_price, dec!(0.55));
        assert_eq!(update.no_price, dec!(0.45));
    }

    #[test]
    fn test_parse_fill_message() {
        let json = r#"{
            "type": "fill",
            "sid": 3,
            "seq": 1,
            "msg": {
                "order_id": "order-abc",
                "count": 5,
                "remaining_count": 5,
                "side": "yes",
                "yes_price": 42,
                "no_price": 58,
                "trade_id": "trade-123",
                "market_ticker": "PRES-2026-DEM"
            }
        }"#;
        let msg = parse_ws_message(json).unwrap();
        // Fill messages don't produce PriceUpdate
        assert!(ws_message_to_price_update(&msg).is_none());
        match msg {
            KalshiWsMessage::Fill {
                order_id,
                count,
                trade_id,
                market_ticker,
                ..
            } => {
                assert_eq!(order_id, "order-abc");
                assert_eq!(count, Some(5));
                assert_eq!(trade_id, Some("trade-123".to_string()));
                assert_eq!(market_ticker, Some("PRES-2026-DEM".to_string()));
            }
            _ => panic!("expected Fill"),
        }
    }

    #[test]
    fn test_subscribe_message_format() {
        let tickers = vec![
            "PRES-2026-DEM".to_string(),
            "PRES-2026-REP".to_string(),
        ];
        let msg = build_subscribe_message(2, &["orderbook_delta"], &tickers);
        assert_eq!(msg["id"], 2);
        assert_eq!(msg["cmd"], "subscribe");
        assert_eq!(msg["params"]["channels"][0], "orderbook_delta");
        assert_eq!(msg["params"]["market_tickers"][0], "PRES-2026-DEM");
        assert_eq!(msg["params"]["market_tickers"][1], "PRES-2026-REP");
    }

    #[test]
    fn test_reconnect_policy_backoff() {
        let policy = ReconnectPolicy::default();

        // First attempt: ~1s
        let d0 = policy.delay_for_attempt(0);
        assert!(d0.as_secs_f64() >= 0.5 && d0.as_secs_f64() <= 2.0);

        // Second attempt: ~2s
        let d1 = policy.delay_for_attempt(1);
        assert!(d1.as_secs_f64() >= 1.0 && d1.as_secs_f64() <= 4.0);

        // Third attempt: ~4s
        let d2 = policy.delay_for_attempt(2);
        assert!(d2.as_secs_f64() >= 2.0 && d2.as_secs_f64() <= 8.0);

        // Should cap at max_delay (30s)
        let d10 = policy.delay_for_attempt(10);
        assert!(d10.as_secs_f64() <= 40.0); // 30s + jitter
    }

    #[test]
    fn test_parse_unknown_message_type() {
        // Unknown message types should be parsed as Other
        let json = r#"{"type": "something_new", "sid": 1, "seq": 1, "msg": {"data": 123}}"#;
        let msg = parse_ws_message(json).unwrap();
        assert!(matches!(msg, KalshiWsMessage::Other));
    }

    #[test]
    fn test_parse_subscribed_message() {
        let json = r#"{
            "type": "subscribed",
            "id": 1,
            "msg": {"channel": "orderbook_delta", "sid": 1}
        }"#;
        let msg = parse_ws_message(json).unwrap();
        match msg {
            KalshiWsMessage::Subscribed { channel, sid } => {
                assert_eq!(channel, Some("orderbook_delta".to_string()));
                assert_eq!(sid, Some(1));
            }
            _ => panic!("expected Subscribed"),
        }
    }

    #[test]
    fn test_parse_error_message() {
        let json = r#"{
            "type": "error",
            "id": 123,
            "msg": {"code": 6, "msg": "Already subscribed"}
        }"#;
        let msg = parse_ws_message(json).unwrap();
        match msg {
            KalshiWsMessage::Error { code, msg } => {
                assert_eq!(code, Some(6));
                assert_eq!(msg, Some("Already subscribed".to_string()));
            }
            _ => panic!("expected Error"),
        }
    }

    #[test]
    fn test_cents_conversion_in_updates() {
        // Verify that all price conversions from cents to decimal are correct
        assert_eq!(kalshi_cents_to_decimal(1), dec!(0.01));
        assert_eq!(kalshi_cents_to_decimal(50), dec!(0.50));
        assert_eq!(kalshi_cents_to_decimal(99), dec!(0.99));
    }
}
