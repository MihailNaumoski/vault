use futures_util::{SinkExt, StreamExt};
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet};
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
            // Prefer dollar fields, fall back to cents.
            // CRITICAL: Never default to zero — a zero price creates phantom spreads
            // in the detector (spread = 1 - poly - 0 ≈ 0.98). If we can't determine
            // a price, skip the update entirely so stale-but-real data is preserved.
            let yes_p = yes_bid_dollars
                .as_ref()
                .and_then(|s| s.parse::<rust_decimal::Decimal>().ok())
                .or_else(|| yes_price.map(kalshi_cents_to_decimal));

            let no_p = yes_ask_dollars
                .as_ref()
                .and_then(|s| s.parse::<rust_decimal::Decimal>().ok())
                .map(|ask| rust_decimal_macros::dec!(1) - ask)
                .or_else(|| no_price.map(kalshi_cents_to_decimal));

            // Both prices must be present; partial updates would corrupt spread math
            let (yes_p, no_p) = match (yes_p, no_p) {
                (Some(y), Some(n)) => (y, n),
                _ => return None,
            };

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
            // Delegate to LocalOrderbook for consistent price derivation.
            // This ensures the snapshot path uses the same logic as the delta
            // path (best_bid for YES, best_no_bid for NO), avoiding the
            // inconsistency where this path previously used the first NO level
            // while the delta path used a different derivation.
            let book = LocalOrderbook::from_snapshot(yes, no);
            book.to_price_update(market_ticker)
        }
        // Deltas need local orderbook state to compute full prices;
        // handled in connect_and_run() with stateful processing.
        KalshiWsMessage::OrderbookDelta { .. } => None,
        _ => None,
    }
}

/// Local orderbook state for computing best bid/ask from deltas.
#[derive(Debug, Default)]
struct LocalOrderbook {
    /// YES side: price_str -> quantity (as f64 for delta arithmetic)
    yes_levels: HashMap<String, f64>,
    /// NO side: price_str -> quantity
    no_levels: HashMap<String, f64>,
}

impl LocalOrderbook {
    /// Initialize from a snapshot message's yes/no arrays.
    fn from_snapshot(yes: &[Vec<serde_json::Value>], no: &[Vec<serde_json::Value>]) -> Self {
        let mut book = Self::default();
        for level in yes {
            if level.len() >= 2 {
                if let (Some(price), Some(qty)) = (
                    level[0].as_str().map(|s| s.to_string()).or_else(|| level[0].as_f64().map(|f| format!("{:.2}", f / 100.0))),
                    level[1].as_str().and_then(|s| s.parse::<f64>().ok()).or_else(|| level[1].as_f64()),
                ) {
                    book.yes_levels.insert(price, qty);
                }
            }
        }
        for level in no {
            if level.len() >= 2 {
                if let (Some(price), Some(qty)) = (
                    level[0].as_str().map(|s| s.to_string()).or_else(|| level[0].as_f64().map(|f| format!("{:.2}", f / 100.0))),
                    level[1].as_str().and_then(|s| s.parse::<f64>().ok()).or_else(|| level[1].as_f64()),
                ) {
                    book.no_levels.insert(price, qty);
                }
            }
        }
        book
    }

    /// Apply a delta: add/remove quantity at a price level.
    fn apply_delta(&mut self, price_dollars: &str, delta_fp: &str, side: &str) {
        let delta: f64 = delta_fp.parse().unwrap_or(0.0);
        let levels = match side {
            "yes" => &mut self.yes_levels,
            "no" => &mut self.no_levels,
            _ => return,
        };
        let entry = levels.entry(price_dollars.to_string()).or_insert(0.0);
        *entry += delta;
        if *entry <= 0.0 {
            levels.remove(price_dollars);
        }
    }

    /// Best bid = highest yes price with quantity > 0.
    /// Returns `None` when the yes side is empty (no levels).
    fn best_bid(&self) -> Option<rust_decimal::Decimal> {
        self.yes_levels
            .keys()
            .filter_map(|p| p.parse::<rust_decimal::Decimal>().ok())
            .max()
    }

    /// Best NO bid = highest NO bid price (max of no_levels).
    /// In Kalshi's binary market, the best NO bid at price P means there's a
    /// YES ask at (1-P). So: yes_best_ask = 1 - best_no_bid.
    /// Returns `None` when the NO side is empty (no levels).
    fn best_no_bid(&self) -> Option<rust_decimal::Decimal> {
        self.no_levels
            .keys()
            .filter_map(|p| p.parse::<rust_decimal::Decimal>().ok())
            .max()
    }

    /// Compute a PriceUpdate from the current state.
    ///
    /// Derives prices consistently with the ticker handler:
    /// - `yes_price` = best YES bid (highest bid on the YES side)
    /// - `no_price`  = 1 - yes_best_ask = 1 - (1 - best_no_bid) = best_no_bid
    ///
    /// This ensures the orderbook snapshot/delta path produces the same no_price
    /// semantics as the ticker path (`no_price = 1 - yes_ask_dollars`).
    ///
    /// Returns `None` when either side is empty — emitting a zero price would
    /// corrupt the price cache and leak phantom spreads into the detector/TUI.
    fn to_price_update(&self, market_ticker: &str) -> Option<PriceUpdate> {
        let yes_price = self.best_bid()?;
        // Derive no_price from the NO bids, consistent with ticker path:
        // ticker: no_price = 1 - yes_ask_dollars
        // orderbook: yes_ask = 1 - best_no_bid, so no_price = best_no_bid
        let no_price = self.best_no_bid()?;
        Some(PriceUpdate {
            platform: Platform::Kalshi,
            market_id: market_ticker.to_string(),
            yes_price,
            no_price,
            timestamp: chrono::Utc::now(),
        })
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
    let key_val = auth_headers
        .get("KALSHI-ACCESS-KEY")
        .ok_or_else(|| KalshiError::Auth("auth headers missing key".to_string()))?
        .to_str()
        .map_err(|e| KalshiError::Auth(format!("invalid key header encoding: {e}")))?;
    let sig_val = auth_headers
        .get("KALSHI-ACCESS-SIGNATURE")
        .ok_or_else(|| KalshiError::Auth("auth headers missing signature".to_string()))?
        .to_str()
        .map_err(|e| KalshiError::Auth(format!("invalid signature header encoding: {e}")))?;
    let ts_val = auth_headers
        .get("KALSHI-ACCESS-TIMESTAMP")
        .ok_or_else(|| KalshiError::Auth("auth headers missing timestamp".to_string()))?
        .to_str()
        .map_err(|e| KalshiError::Auth(format!("invalid timestamp header encoding: {e}")))?;

    let host = url.replace("wss://", "").replace("ws://", "");
    let host = host.split('/').next().unwrap_or("");

    let request = tokio_tungstenite::tungstenite::http::Request::builder()
        .uri(url)
        .header("KALSHI-ACCESS-KEY", key_val)
        .header("KALSHI-ACCESS-SIGNATURE", sig_val)
        .header("KALSHI-ACCESS-TIMESTAMP", ts_val)
        .header("Host", host)
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

    info!(
        tickers = ?tickers,
        count = tickers.len(),
        "Kalshi WS subscribed to tickers"
    );

    // Maintain local orderbook state for delta processing
    let mut orderbooks: HashMap<String, LocalOrderbook> = HashMap::new();

    // Track which tickers have received at least one price update.
    // After `silent_ticker_timeout`, warn about any ticker that hasn't received data.
    let mut tickers_with_data: HashSet<String> = HashSet::new();
    let silent_ticker_timeout = tokio::time::Instant::now() + Duration::from_secs(30);
    let mut silent_check_done = false;

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
                            let price_update = match &ws_msg {
                                KalshiWsMessage::OrderbookSnapshot { market_ticker, yes, no } => {
                                    let book = LocalOrderbook::from_snapshot(yes, no);
                                    let update = book.to_price_update(market_ticker);
                                    orderbooks.insert(market_ticker.clone(), book);
                                    update
                                }
                                KalshiWsMessage::OrderbookDelta { market_ticker, price_dollars, delta_fp, side } => {
                                    if let (Some(price), Some(delta), Some(s)) = (price_dollars.as_ref(), delta_fp.as_ref(), side.as_ref()) {
                                        let book = orderbooks.entry(market_ticker.clone()).or_default();
                                        book.apply_delta(price, delta, s);
                                        book.to_price_update(market_ticker)
                                    } else {
                                        None
                                    }
                                }
                                other => ws_message_to_price_update(other),
                            };
                            if let Some(update) = price_update {
                                tickers_with_data.insert(update.market_id.clone());
                                if tx.send(update).await.is_err() {
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
                // Check for silent tickers once after the timeout period
                if !silent_check_done && tokio::time::Instant::now() >= silent_ticker_timeout {
                    silent_check_done = true;
                    let silent: Vec<&String> = tickers
                        .iter()
                        .filter(|t| !tickers_with_data.contains(*t))
                        .collect();
                    if !silent.is_empty() {
                        warn!(
                            silent_tickers = ?silent,
                            active_tickers = ?tickers_with_data,
                            "Kalshi WS: {} of {} tickers received NO data after 30s — \
                             these may be dead/delisted tickers",
                            silent.len(),
                            tickers.len()
                        );
                    } else {
                        info!(
                            "Kalshi WS: all {} tickers confirmed active (received data within 30s)",
                            tickers.len()
                        );
                    }
                }

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
        // no_price = best_no_bid = max(0.58, 0.59) = 0.59
        // This is consistent with ticker path: no_price = 1 - yes_ask
        // where yes_ask = 1 - best_no_bid = 1 - 0.59 = 0.41
        // so no_price = 1 - 0.41 = 0.59
        assert_eq!(update.no_price, dec!(0.59));
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

    #[test]
    fn test_local_orderbook_from_snapshot() {
        let yes = vec![
            vec![serde_json::Value::String("0.42".into()), serde_json::Value::String("100.00".into())],
            vec![serde_json::Value::String("0.41".into()), serde_json::Value::String("200.00".into())],
        ];
        let no = vec![
            vec![serde_json::Value::String("0.58".into()), serde_json::Value::String("100.00".into())],
            vec![serde_json::Value::String("0.59".into()), serde_json::Value::String("200.00".into())],
        ];
        let book = LocalOrderbook::from_snapshot(&yes, &no);
        assert_eq!(book.best_bid(), Some(dec!(0.42)));
        // best_no_bid = max(0.58, 0.59) = 0.59
        assert_eq!(book.best_no_bid(), Some(dec!(0.59)));
    }

    #[test]
    fn test_local_orderbook_apply_delta_add() {
        let mut book = LocalOrderbook::default();
        book.apply_delta("0.42", "100.00", "yes");
        book.apply_delta("0.58", "100.00", "no");
        assert_eq!(book.best_bid(), Some(dec!(0.42)));
        assert_eq!(book.best_no_bid(), Some(dec!(0.58)));
    }

    #[test]
    fn test_local_orderbook_apply_delta_remove() {
        let mut book = LocalOrderbook::default();
        book.apply_delta("0.42", "100.00", "yes");
        book.apply_delta("0.43", "50.00", "yes");
        assert_eq!(book.best_bid(), Some(dec!(0.43)));
        // Remove the 0.43 level entirely
        book.apply_delta("0.43", "-50.00", "yes");
        assert_eq!(book.best_bid(), Some(dec!(0.42)));
    }

    #[test]
    fn test_local_orderbook_empty_returns_none() {
        let book = LocalOrderbook::default();
        assert_eq!(book.best_bid(), None);
        assert_eq!(book.best_no_bid(), None);
        // to_price_update should return None when either side is empty
        assert!(book.to_price_update("EMPTY-MKT").is_none());
    }

    #[test]
    fn test_local_orderbook_price_update_from_delta() {
        let mut book = LocalOrderbook::default();
        book.apply_delta("0.45", "100.00", "yes");
        book.apply_delta("0.55", "100.00", "no");
        let update = book.to_price_update("TEST-MKT").expect("both sides present");
        assert_eq!(update.platform, Platform::Kalshi);
        assert_eq!(update.market_id, "TEST-MKT");
        assert_eq!(update.yes_price, dec!(0.45));
        assert_eq!(update.no_price, dec!(0.55));
    }

    #[test]
    fn test_local_orderbook_one_side_empty_returns_none() {
        let mut book = LocalOrderbook::default();
        // Only yes side has levels
        book.apply_delta("0.45", "100.00", "yes");
        assert!(book.to_price_update("ONE-SIDE").is_none());

        // Now add no side
        book.apply_delta("0.55", "100.00", "no");
        assert!(book.to_price_update("ONE-SIDE").is_some());

        // Remove all no levels
        book.apply_delta("0.55", "-100.00", "no");
        assert!(book.to_price_update("ONE-SIDE").is_none());
    }
}
