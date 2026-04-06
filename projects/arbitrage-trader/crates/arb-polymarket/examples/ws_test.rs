//! Standalone WS test for Polymarket CLOB WebSocket.
//!
//! Fetches top markets from Gamma API, subscribes to one via WS,
//! prints every raw message for 30 seconds, then exits.
//!
//! Run: cargo run --example ws_test -p arb-polymarket

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::time::Instant;
use tokio_tungstenite::tungstenite::Message;

const GAMMA_API: &str =
    "https://gamma-api.polymarket.com/markets?active=true&closed=false&limit=5&order=volumeNum&ascending=false";
const WS_URL: &str = "wss://ws-subscriptions-clob.polymarket.com/ws/market";
const RUN_DURATION: Duration = Duration::from_secs(30);
const PING_INTERVAL: Duration = Duration::from_secs(5);

#[tokio::main]
async fn main() {
    println!("[ws_test] Fetching top 5 markets from Gamma API...");

    let markets: serde_json::Value = reqwest::get(GAMMA_API)
        .await
        .expect("Gamma API request failed")
        .json()
        .await
        .expect("Failed to parse Gamma API JSON");

    let first_market = markets
        .as_array()
        .and_then(|arr| arr.first())
        .expect("No markets returned from Gamma API");

    let question = first_market
        .get("question")
        .and_then(|v| v.as_str())
        .unwrap_or("<unknown>");
    println!("[ws_test] Top market: {question}");

    // clobTokenIds is a JSON string containing an array, e.g. "[\"id1\",\"id2\"]"
    let clob_token_ids_raw = first_market
        .get("clobTokenIds")
        .and_then(|v| v.as_str())
        .expect("No clobTokenIds in first market");

    let clob_token_ids: Vec<String> =
        serde_json::from_str(clob_token_ids_raw).expect("Failed to parse clobTokenIds JSON array");

    if clob_token_ids.is_empty() {
        println!("[ws_test] No token IDs found, exiting.");
        return;
    }

    println!(
        "[ws_test] Token IDs ({}): {:?}",
        clob_token_ids.len(),
        clob_token_ids
    );

    // Use the first token ID for subscription
    let token_id = &clob_token_ids[0];
    println!("[ws_test] Subscribing to token: {token_id}");

    println!("[ws_test] Connecting to {WS_URL}...");
    let (ws_stream, _) = tokio_tungstenite::connect_async(WS_URL)
        .await
        .expect("WebSocket connection failed");
    println!("[ws_test] Connected!");

    let (mut write, mut read) = ws_stream.split();

    // Send subscription
    let sub_msg = serde_json::json!({
        "type": "market",
        "assets_ids": [token_id],
        "custom_feature_enabled": true
    });
    let sub_str = sub_msg.to_string();
    println!("[ws_test] Sending subscription: {sub_str}");
    write
        .send(Message::Text(sub_str.into()))
        .await
        .expect("Failed to send subscription");
    println!("[ws_test] Subscription sent. Listening for {RUN_DURATION:?}...\n");

    let start = Instant::now();
    let mut msg_count: u64 = 0;
    let mut ping_timer = tokio::time::interval(PING_INTERVAL);
    // Skip the first immediate tick
    ping_timer.tick().await;

    loop {
        tokio::select! {
            msg = read.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        msg_count += 1;
                        let elapsed = start.elapsed().as_secs_f32();
                        println!("[{elapsed:6.1}s] #{msg_count} TEXT: {text}");
                    }
                    Some(Ok(Message::Binary(data))) => {
                        msg_count += 1;
                        let elapsed = start.elapsed().as_secs_f32();
                        println!("[{elapsed:6.1}s] #{msg_count} BINARY: {} bytes", data.len());
                    }
                    Some(Ok(Message::Pong(data))) => {
                        msg_count += 1;
                        let elapsed = start.elapsed().as_secs_f32();
                        println!("[{elapsed:6.1}s] #{msg_count} PONG: {:?}", String::from_utf8_lossy(&data));
                    }
                    Some(Ok(Message::Ping(data))) => {
                        msg_count += 1;
                        let elapsed = start.elapsed().as_secs_f32();
                        println!("[{elapsed:6.1}s] #{msg_count} PING from server: {:?}", String::from_utf8_lossy(&data));
                    }
                    Some(Ok(Message::Close(frame))) => {
                        let elapsed = start.elapsed().as_secs_f32();
                        println!("[{elapsed:6.1}s] CLOSE frame: {frame:?}");
                        break;
                    }
                    Some(Err(e)) => {
                        let elapsed = start.elapsed().as_secs_f32();
                        println!("[{elapsed:6.1}s] ERROR: {e}");
                        break;
                    }
                    None => {
                        println!("[ws_test] Stream ended.");
                        break;
                    }
                    _ => {}
                }
            }
            _ = ping_timer.tick() => {
                let elapsed = start.elapsed().as_secs_f32();
                println!("[{elapsed:6.1}s] >> Sending PING");
                if let Err(e) = write.send(Message::Text("PING".into())).await {
                    println!("[{elapsed:6.1}s] Failed to send PING: {e}");
                    break;
                }
            }
        }

        if start.elapsed() >= RUN_DURATION {
            println!("\n[ws_test] {RUN_DURATION:?} elapsed, shutting down.");
            break;
        }
    }

    println!(
        "[ws_test] Done. Received {msg_count} messages in {:.1}s.",
        start.elapsed().as_secs_f32()
    );
}
