use serde::{Deserialize, Serialize};

/// Configuration for the Kalshi connector.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KalshiConfig {
    /// API key ID from the Kalshi dashboard.
    pub api_key_id: String,
    /// RSA private key in PEM format (PKCS#8).
    pub private_key_pem: String,
    /// REST API base URL. Defaults to production.
    #[serde(default = "default_base_url")]
    pub base_url: String,
    /// WebSocket URL. Defaults to production.
    #[serde(default = "default_ws_url")]
    pub ws_url: String,
}

fn default_base_url() -> String {
    "https://trading-api.kalshi.com/trade-api/v2".to_string()
}

fn default_ws_url() -> String {
    "wss://trading-api.kalshi.com/trade-api/ws/v2".to_string()
}

// ---------------------------------------------------------------------------
// Kalshi REST API response types.
// Prices are in cents (1-99) with dollar string fields during migration.
// Conversion to Decimal happens in the connector.
// ---------------------------------------------------------------------------

/// Response from GET /markets/{ticker} and entries in GET /markets.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KalshiMarketResponse {
    pub ticker: String,
    pub title: String,
    pub status: String,
    #[serde(default)]
    pub yes_ask: u32,
    #[serde(default)]
    pub yes_bid: u32,
    #[serde(default)]
    pub no_ask: u32,
    #[serde(default)]
    pub no_bid: u32,
    #[serde(default)]
    pub volume: u64,
    #[serde(default)]
    pub open_interest: u64,
    pub close_time: Option<String>,
    #[serde(default)]
    pub liquidity: u64,
    // Dollar string fields (price format migration)
    #[serde(default)]
    pub yes_ask_dollars: Option<String>,
    #[serde(default)]
    pub yes_bid_dollars: Option<String>,
    #[serde(default)]
    pub no_ask_dollars: Option<String>,
    #[serde(default)]
    pub no_bid_dollars: Option<String>,
    #[serde(default)]
    pub volume_fp: Option<String>,
    #[serde(default)]
    pub open_interest_fp: Option<String>,
}

/// Wrapper for paginated market list responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KalshiMarketsListResponse {
    pub markets: Vec<KalshiMarketResponse>,
    pub cursor: Option<String>,
}

/// Request body for POST /portfolio/orders.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KalshiOrderRequest {
    pub ticker: String,
    pub action: String,       // "buy" or "sell"
    pub side: String,         // "yes" or "no"
    pub r#type: String,       // "limit"
    pub count: u32,           // number of contracts
    pub yes_price: Option<u32>,  // cents, for yes-side limit
    pub no_price: Option<u32>,   // cents, for no-side limit
    // Dollar string fields (price format migration)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub yes_price_dollars: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_price_dollars: Option<String>,
}

/// Response from order operations (POST/GET /portfolio/orders).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KalshiOrderResponse {
    pub order_id: String,
    pub status: String,
    #[serde(default)]
    pub remaining_count: u32,
    #[serde(default)]
    pub filled_count: u32,
    #[serde(default)]
    pub yes_price: u32,
    #[serde(default)]
    pub no_price: u32,
    pub side: String,
    pub ticker: String,
    pub action: String,
    // Dollar string fields (price format migration)
    #[serde(default)]
    pub remaining_count_fp: Option<String>,
    #[serde(default)]
    pub fill_count_fp: Option<String>,
    #[serde(default)]
    pub initial_count_fp: Option<String>,
    #[serde(default)]
    pub yes_price_dollars: Option<String>,
    #[serde(default)]
    pub no_price_dollars: Option<String>,
    #[serde(default)]
    pub taker_fees_dollars: Option<String>,
    #[serde(default)]
    pub maker_fees_dollars: Option<String>,
    #[serde(default)]
    pub created_time: Option<String>,
    #[serde(default)]
    pub last_update_time: Option<String>,
}

/// Wrapper for paginated orders list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KalshiOrdersListResponse {
    pub orders: Vec<KalshiOrderResponse>,
    pub cursor: Option<String>,
}

/// A single level in the Kalshi order book (prices in cents).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KalshiBookLevel {
    pub price: u32,
    pub quantity: u32,
}

/// Response from GET /markets/{ticker}/orderbook.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KalshiBookResponse {
    pub yes: Vec<Vec<u32>>,
    pub no: Vec<Vec<u32>>,
}

/// Response from GET /portfolio/positions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KalshiPositionResponse {
    pub ticker: String,
    pub market_exposure: Option<i64>,
    #[serde(default)]
    pub position: i32,
    #[serde(default)]
    pub resting_orders_count: u32, // deprecated Nov 2025, kept for backward compat
    #[serde(default)]
    pub total_cost: i64,
    // New fields (price format migration)
    #[serde(default)]
    pub total_traded_dollars: Option<String>,
    #[serde(default)]
    pub position_fp: Option<String>,
    #[serde(default)]
    pub market_exposure_dollars: Option<String>,
    #[serde(default)]
    pub realized_pnl_dollars: Option<String>,
    #[serde(default)]
    pub fees_paid_dollars: Option<String>,
    #[serde(default)]
    pub last_updated_ts: Option<String>,
}

/// Wrapper for positions list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KalshiPositionsListResponse {
    pub market_positions: Vec<KalshiPositionResponse>,
    pub cursor: Option<String>,
}

/// Response from GET /portfolio/balance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KalshiBalanceResponse {
    pub balance: i64, // in cents
}

// ---------------------------------------------------------------------------
// Events API response types (GET /events)
// ---------------------------------------------------------------------------

/// A single event from the Kalshi Events API.
///
/// Events group related sub-markets (e.g. "How high will Bitcoin get in 2026?"
/// contains bracket sub-markets for different price thresholds).
/// When fetched with `with_nested_markets=true`, the `markets` field is populated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KalshiEventResponse {
    pub event_ticker: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub mutually_exclusive: bool,
    /// Sub-markets nested under this event (populated when with_nested_markets=true).
    #[serde(default)]
    pub markets: Vec<KalshiMarketResponse>,
    /// Series ticker this event belongs to.
    #[serde(default)]
    pub series_ticker: Option<String>,
    /// Number of sub-markets under this event.
    #[serde(default)]
    pub sub_title: Option<String>,
}

/// Wrapper for paginated event list responses from GET /events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KalshiEventsListResponse {
    pub events: Vec<KalshiEventResponse>,
    pub cursor: Option<String>,
}

// ---------------------------------------------------------------------------
// WebSocket message types
// ---------------------------------------------------------------------------

/// Envelope wrapper for all incoming Kalshi WebSocket messages.
///
/// Messages arrive wrapped: `{"type": "...", "sid": N, "seq": N, "msg": {...}}`.
/// Parse the envelope first, then extract the inner message from `msg`.
#[derive(Debug, Clone, Deserialize)]
pub struct KalshiWsEnvelope {
    #[serde(rename = "type")]
    pub msg_type: String,
    #[serde(default)]
    pub sid: Option<u64>,
    #[serde(default)]
    pub seq: Option<u64>,
    #[serde(default)]
    pub msg: Option<serde_json::Value>,
    #[serde(default)]
    pub id: Option<u64>,
}

/// Parsed incoming WebSocket message from Kalshi.
///
/// Constructed by two-step parsing: envelope first, then inner msg field.
#[derive(Debug, Clone)]
pub enum KalshiWsMessage {
    OrderbookDelta {
        market_ticker: String,
        price_dollars: Option<String>,
        delta_fp: Option<String>,
        side: Option<String>,
    },
    OrderbookSnapshot {
        market_ticker: String,
        yes: Vec<Vec<serde_json::Value>>,
        no: Vec<Vec<serde_json::Value>>,
    },
    Ticker {
        market_ticker: String,
        // Old cent fields (optional for backward compat)
        yes_price: Option<u32>,
        no_price: Option<u32>,
        volume: Option<u64>,
        // New dollar string fields
        price_dollars: Option<String>,
        yes_bid_dollars: Option<String>,
        yes_ask_dollars: Option<String>,
        volume_fp: Option<String>,
        open_interest_fp: Option<String>,
        yes_bid_size_fp: Option<String>,
        yes_ask_size_fp: Option<String>,
        last_trade_size_fp: Option<String>,
        ts: Option<u64>,
        time: Option<String>,
    },
    Fill {
        order_id: String,
        // Old fields (optional for backward compat)
        count: Option<u32>,
        remaining_count: Option<u32>,
        side: Option<String>,
        yes_price: Option<u32>,
        no_price: Option<u32>,
        // New fields
        trade_id: Option<String>,
        market_ticker: Option<String>,
        is_taker: Option<bool>,
        yes_price_dollars: Option<String>,
        count_fp: Option<String>,
        fee_cost: Option<String>,
        action: Option<String>,
        ts: Option<u64>,
        client_order_id: Option<String>,
        post_position_fp: Option<String>,
        purchased_side: Option<String>,
        subaccount: Option<String>,
    },
    Subscribed {
        channel: Option<String>,
        sid: Option<u64>,
    },
    Error {
        code: Option<u64>,
        msg: Option<String>,
    },
    Other,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_market_response() {
        let json = r#"{
            "ticker": "PRES-2026-DEM",
            "title": "Will a Democrat win 2026?",
            "status": "open",
            "yes_ask": 55,
            "yes_bid": 53,
            "no_ask": 47,
            "no_bid": 45,
            "volume": 10000,
            "open_interest": 5000,
            "close_time": "2026-11-04T00:00:00Z",
            "liquidity": 2000
        }"#;
        let market: KalshiMarketResponse = serde_json::from_str(json).unwrap();
        assert_eq!(market.ticker, "PRES-2026-DEM");
        assert_eq!(market.yes_ask, 55);
        assert_eq!(market.no_bid, 45);
        // New dollar fields default to None when absent
        assert!(market.yes_ask_dollars.is_none());
    }

    #[test]
    fn test_deserialize_market_response_with_dollar_fields() {
        let json = r#"{
            "ticker": "PRES-2026-DEM",
            "title": "Will a Democrat win 2026?",
            "status": "open",
            "yes_ask": 55,
            "yes_bid": 53,
            "no_ask": 47,
            "no_bid": 45,
            "volume": 10000,
            "open_interest": 5000,
            "close_time": "2026-11-04T00:00:00Z",
            "liquidity": 2000,
            "yes_ask_dollars": "0.55",
            "yes_bid_dollars": "0.53",
            "volume_fp": "10000.00"
        }"#;
        let market: KalshiMarketResponse = serde_json::from_str(json).unwrap();
        assert_eq!(market.yes_ask_dollars, Some("0.55".to_string()));
        assert_eq!(market.yes_bid_dollars, Some("0.53".to_string()));
        assert_eq!(market.volume_fp, Some("10000.00".to_string()));
    }

    #[test]
    fn test_deserialize_order_response() {
        let json = r#"{
            "order_id": "abc-123",
            "status": "resting",
            "remaining_count": 8,
            "filled_count": 2,
            "yes_price": 45,
            "no_price": 55,
            "side": "yes",
            "ticker": "PRES-2026-DEM",
            "action": "buy"
        }"#;
        let order: KalshiOrderResponse = serde_json::from_str(json).unwrap();
        assert_eq!(order.order_id, "abc-123");
        assert_eq!(order.filled_count, 2);
        assert_eq!(order.yes_price, 45);
        // New dollar fields default to None when absent
        assert!(order.yes_price_dollars.is_none());
    }

    #[test]
    fn test_deserialize_book_response() {
        let json = r#"{
            "yes": [[45, 100], [44, 200]],
            "no": [[55, 100], [56, 200]]
        }"#;
        let book: KalshiBookResponse = serde_json::from_str(json).unwrap();
        assert_eq!(book.yes.len(), 2);
        assert_eq!(book.yes[0], vec![45, 100]);
        assert_eq!(book.no[0], vec![55, 100]);
    }

    #[test]
    fn test_deserialize_balance_response() {
        let json = r#"{"balance": 150000}"#;
        let bal: KalshiBalanceResponse = serde_json::from_str(json).unwrap();
        assert_eq!(bal.balance, 150000);
    }

    #[test]
    fn test_serialize_order_request() {
        let req = KalshiOrderRequest {
            ticker: "PRES-2026-DEM".to_string(),
            action: "buy".to_string(),
            side: "yes".to_string(),
            r#type: "limit".to_string(),
            count: 10,
            yes_price: Some(45),
            no_price: None,
            yes_price_dollars: None,
            no_price_dollars: None,
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["ticker"], "PRES-2026-DEM");
        assert_eq!(json["count"], 10);
        assert_eq!(json["yes_price"], 45);
        // Dollar fields should be omitted when None (skip_serializing_if)
        assert!(json.get("yes_price_dollars").is_none());
    }

    #[test]
    fn test_config_defaults() {
        let json = r#"{
            "api_key_id": "key123",
            "private_key_pem": "-----BEGIN PRIVATE KEY-----\ntest\n-----END PRIVATE KEY-----"
        }"#;
        let config: KalshiConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.base_url, "https://trading-api.kalshi.com/trade-api/v2");
        assert_eq!(config.ws_url, "wss://trading-api.kalshi.com/trade-api/ws/v2");
    }

    #[test]
    fn test_deserialize_position_response() {
        let json = r#"{
            "ticker": "PRES-2026-DEM",
            "position": 10,
            "total_cost": 4500,
            "resting_orders_count": 2
        }"#;
        let pos: KalshiPositionResponse = serde_json::from_str(json).unwrap();
        assert_eq!(pos.ticker, "PRES-2026-DEM");
        assert_eq!(pos.position, 10);
        assert_eq!(pos.total_cost, 4500);
        // New fields default to None
        assert!(pos.total_traded_dollars.is_none());
    }

    #[test]
    fn test_deserialize_position_response_with_new_fields() {
        let json = r#"{
            "ticker": "PRES-2026-DEM",
            "position": 10,
            "total_cost": 4500,
            "total_traded_dollars": "45.00",
            "position_fp": "10.00",
            "market_exposure_dollars": "4.50",
            "realized_pnl_dollars": "1.20",
            "fees_paid_dollars": "0.30",
            "last_updated_ts": "1700000000000"
        }"#;
        let pos: KalshiPositionResponse = serde_json::from_str(json).unwrap();
        assert_eq!(pos.total_traded_dollars, Some("45.00".to_string()));
        assert_eq!(pos.position_fp, Some("10.00".to_string()));
    }

    #[test]
    fn test_deserialize_ws_envelope() {
        let json = r#"{"type": "ticker", "sid": 2, "seq": 5, "msg": {"market_ticker": "TEST"}}"#;
        let envelope: KalshiWsEnvelope = serde_json::from_str(json).unwrap();
        assert_eq!(envelope.msg_type, "ticker");
        assert_eq!(envelope.sid, Some(2));
        assert_eq!(envelope.seq, Some(5));
        assert!(envelope.msg.is_some());
    }

    #[test]
    fn test_deserialize_event_response() {
        let json = r#"{
            "event_ticker": "KXBTCMAX100-26",
            "title": "When will Bitcoin cross $100k again?",
            "category": "Crypto",
            "mutually_exclusive": true,
            "markets": [
                {
                    "ticker": "KXBTCMAX100-26-T1",
                    "title": "Bitcoin above $100k by April 2026",
                    "status": "open",
                    "yes_ask": 65,
                    "yes_bid": 63,
                    "no_ask": 37,
                    "no_bid": 35,
                    "volume": 5000,
                    "open_interest": 2000,
                    "close_time": "2026-04-30T00:00:00Z",
                    "liquidity": 1000
                }
            ]
        }"#;
        let event: KalshiEventResponse = serde_json::from_str(json).unwrap();
        assert_eq!(event.event_ticker, "KXBTCMAX100-26");
        assert_eq!(event.title, "When will Bitcoin cross $100k again?");
        assert_eq!(event.category, "Crypto");
        assert!(event.mutually_exclusive);
        assert_eq!(event.markets.len(), 1);
        assert_eq!(event.markets[0].ticker, "KXBTCMAX100-26-T1");
        assert_eq!(event.markets[0].yes_bid, 63);
    }

    #[test]
    fn test_deserialize_event_response_no_markets() {
        let json = r#"{
            "event_ticker": "KXETHMAXY",
            "title": "How high will Ethereum get in 2026?",
            "category": "Crypto",
            "mutually_exclusive": false
        }"#;
        let event: KalshiEventResponse = serde_json::from_str(json).unwrap();
        assert_eq!(event.event_ticker, "KXETHMAXY");
        assert!(event.markets.is_empty());
    }

    #[test]
    fn test_deserialize_events_list_response() {
        let json = r#"{
            "events": [
                {
                    "event_ticker": "KXBTCMAX100-26",
                    "title": "When will Bitcoin cross $100k again?",
                    "category": "Crypto",
                    "mutually_exclusive": true,
                    "markets": []
                }
            ],
            "cursor": "abc123"
        }"#;
        let list: KalshiEventsListResponse = serde_json::from_str(json).unwrap();
        assert_eq!(list.events.len(), 1);
        assert_eq!(list.cursor, Some("abc123".to_string()));
    }
}
