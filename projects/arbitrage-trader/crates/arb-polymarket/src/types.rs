use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

/// Configuration for the Polymarket connector.
#[derive(Clone, Serialize, Deserialize)]
pub struct PolyConfig {
    /// CLOB API key.
    pub api_key: String,
    /// Base64-encoded HMAC secret.
    pub secret: String,
    /// API passphrase.
    pub passphrase: String,
    /// Hex-encoded Polygon wallet private key (for EIP-712 signing).
    pub private_key: String,
    /// CLOB API base URL (default: https://clob.polymarket.com).
    #[serde(default = "default_clob_url")]
    pub clob_url: String,
    /// Gamma API base URL (default: https://gamma-api.polymarket.com).
    #[serde(default = "default_gamma_url")]
    pub gamma_url: String,
    /// WebSocket URL (default: wss://ws-subscriptions-clob.polymarket.com/ws/market).
    #[serde(default = "default_ws_url")]
    pub ws_url: String,
    /// Chain ID for EIP-712 signing (default: 137 for Polygon mainnet).
    #[serde(default = "default_chain_id")]
    pub chain_id: u64,
}

impl std::fmt::Debug for PolyConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PolyConfig")
            .field("api_key", &"<redacted>")
            .field("secret", &"<redacted>")
            .field("passphrase", &"<redacted>")
            .field("private_key", &"<redacted>")
            .field("clob_url", &self.clob_url)
            .field("gamma_url", &self.gamma_url)
            .field("ws_url", &self.ws_url)
            .field("chain_id", &self.chain_id)
            .finish()
    }
}

fn default_clob_url() -> String {
    "https://clob.polymarket.com".to_string()
}

fn default_gamma_url() -> String {
    "https://gamma-api.polymarket.com".to_string()
}

fn default_ws_url() -> String {
    "wss://ws-subscriptions-clob.polymarket.com/ws/market".to_string()
}

fn default_chain_id() -> u64 {
    137
}

// ---------------------------------------------------------------------------
// Polymarket API response types (camelCase JSON)
// ---------------------------------------------------------------------------

/// Market response from the Gamma API (GET /markets).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolyMarketResponse {
    pub condition_id: String,
    pub question: String,
    #[serde(default)]
    pub tokens: Vec<PolyToken>,
    #[serde(default)]
    pub outcomes: Vec<String>,
    /// Outcome prices as JSON string array, e.g. "[\"0.535\", \"0.465\"]"
    #[serde(default)]
    pub outcome_prices: Option<String>,
    #[serde(default, deserialize_with = "deserialize_decimal_or_default")]
    pub volume: Decimal,
    #[serde(default, deserialize_with = "deserialize_decimal_or_default")]
    pub liquidity: Decimal,
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub closed: bool,
    #[serde(default)]
    pub end_date_iso: Option<String>,
    #[serde(default)]
    pub market_slug: Option<String>,
    #[serde(default)]
    pub clob_token_ids: Option<String>,
    #[serde(default)]
    pub neg_risk: Option<bool>,
}

/// Token within a PolyMarketResponse.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolyToken {
    pub token_id: String,
    pub outcome: String,
    #[serde(default, deserialize_with = "deserialize_decimal_or_default")]
    pub price: Decimal,
}

/// Order response from the CLOB API.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolyOrderResponse {
    #[serde(alias = "id", alias = "orderID")]
    pub order_id: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub side: String,
    #[serde(default, deserialize_with = "deserialize_decimal_or_default")]
    pub price: Decimal,
    #[serde(default, alias = "original_size")]
    pub size: String,
    #[serde(default, alias = "size_matched")]
    pub filled_size: String,
    #[serde(default)]
    pub market: String,
    #[serde(default)]
    pub asset_id: String,
}

/// Order book response from the CLOB API (GET /book).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolyBookResponse {
    pub market: Option<String>,
    #[serde(default)]
    pub asset_id: Option<String>,
    #[serde(default)]
    pub bids: Vec<PolyBookLevel>,
    #[serde(default)]
    pub asks: Vec<PolyBookLevel>,
    #[serde(default)]
    pub timestamp: Option<String>,
    #[serde(default)]
    pub min_order_size: Option<String>,
    #[serde(default)]
    pub tick_size: Option<String>,
    #[serde(default)]
    pub neg_risk: Option<bool>,
    #[serde(default)]
    pub last_trade_price: Option<String>,
    #[serde(default)]
    pub hash: Option<String>,
}

/// A single level in a Polymarket order book.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolyBookLevel {
    pub price: String,
    pub size: String,
}

/// Position response from the CLOB API (GET /positions).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolyPositionResponse {
    pub asset_id: String,
    #[serde(alias = "market")]
    pub condition_id: String,
    pub side: String,
    #[serde(default, deserialize_with = "deserialize_decimal_or_default")]
    pub size: Decimal,
    #[serde(default, deserialize_with = "deserialize_decimal_or_default")]
    pub avg_price: Decimal,
}

/// Balance response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolyBalanceResponse {
    #[serde(default, deserialize_with = "deserialize_decimal_or_default")]
    pub balance: Decimal,
}

// ---------------------------------------------------------------------------
// WebSocket message types
// ---------------------------------------------------------------------------

/// A single entry in a WebSocket `price_change` event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceChangeEntry {
    pub asset_id: String,
    pub price: String,
    #[serde(default)]
    pub side: Option<String>,
    #[serde(default)]
    pub best_bid: Option<String>,
    #[serde(default)]
    pub best_ask: Option<String>,
}

/// Incoming WebSocket message from Polymarket.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event_type")]
pub enum PolyWsMessage {
    #[serde(rename = "book")]
    Book {
        asset_id: String,
        market: Option<String>,
        #[serde(default)]
        bids: Vec<PolyBookLevel>,
        #[serde(default)]
        asks: Vec<PolyBookLevel>,
        timestamp: Option<String>,
    },
    #[serde(rename = "price_change")]
    PriceChange {
        market: Option<String>,
        #[serde(default)]
        price_changes: Vec<PriceChangeEntry>,
        timestamp: Option<String>,
    },
    #[serde(rename = "last_trade_price")]
    LastTradePrice {
        asset_id: String,
        market: Option<String>,
        #[serde(default)]
        price: String,
    },
    #[serde(rename = "best_bid_ask")]
    BestBidAsk {
        asset_id: String,
        #[serde(default)]
        best_bid: String,
        #[serde(default)]
        best_ask: String,
    },
    #[serde(other)]
    Unknown,
}

// ---------------------------------------------------------------------------
// Conversion helpers: Poly API types -> arb-types
// ---------------------------------------------------------------------------

impl PolyMarketResponse {
    /// Convert to an arb-types Market.
    pub fn to_market(&self) -> arb_types::Market {
        let (yes_price, no_price) = self.outcome_prices();
        let status = if self.closed {
            arb_types::MarketStatus::Closed
        } else if self.active {
            arb_types::MarketStatus::Open
        } else {
            arb_types::MarketStatus::Settled
        };
        let close_time = self
            .end_date_iso
            .as_deref()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(Utc::now);

        arb_types::Market {
            id: arb_types::MarketId::new(),
            platform: arb_types::Platform::Polymarket,
            platform_id: self.condition_id.clone(),
            question: self.question.clone(),
            yes_price,
            no_price,
            volume: self.volume,
            liquidity: self.liquidity,
            status,
            close_time,
            updated_at: Utc::now(),
        }
    }

    /// Extract (yes_token_id, no_token_id) from tokens[] or clob_token_ids fallback.
    pub fn extract_token_ids(&self) -> Option<(String, String)> {
        // Try tokens[] first
        if !self.tokens.is_empty() {
            let yes = self
                .tokens
                .iter()
                .find(|t| t.outcome.eq_ignore_ascii_case("yes"))
                .map(|t| t.token_id.clone());
            let no = self
                .tokens
                .iter()
                .find(|t| t.outcome.eq_ignore_ascii_case("no"))
                .map(|t| t.token_id.clone());
            if let (Some(y), Some(n)) = (yes, no) {
                return Some((y, n));
            }
        }

        // Fallback: parse clob_token_ids JSON string
        if let Some(ref ids_str) = self.clob_token_ids {
            if let Ok(ids) = serde_json::from_str::<Vec<String>>(ids_str) {
                if ids.len() >= 2 {
                    return Some((ids[0].clone(), ids[1].clone()));
                }
            }
        }

        None
    }

    fn outcome_prices(&self) -> (Decimal, Decimal) {
        // Try tokens first (populated by some endpoints)
        let mut yes = Decimal::ZERO;
        let mut no = Decimal::ZERO;
        for t in &self.tokens {
            match t.outcome.to_lowercase().as_str() {
                "yes" => yes = t.price,
                "no" => no = t.price,
                _ => {}
            }
        }
        if yes > Decimal::ZERO || no > Decimal::ZERO {
            return (yes, no);
        }

        // Fallback: parse outcomePrices JSON string, e.g. "[\"0.535\", \"0.465\"]"
        if let Some(ref prices_str) = self.outcome_prices {
            if let Ok(prices) = serde_json::from_str::<Vec<String>>(prices_str) {
                if prices.len() >= 2 {
                    yes = prices[0].parse().unwrap_or_default();
                    no = prices[1].parse().unwrap_or_default();
                }
            }
        }
        (yes, no)
    }
}

impl PolyOrderResponse {
    /// Convert to an arb-types OrderResponse.
    pub fn to_order_response(&self) -> arb_types::OrderResponse {
        let status = match self.status.to_lowercase().as_str() {
            "live" | "open" => arb_types::OrderStatus::Open,
            "matched" | "filled" => arb_types::OrderStatus::Filled,
            "cancelled" | "canceled" => arb_types::OrderStatus::Cancelled,
            "partial" | "partial_fill" => arb_types::OrderStatus::PartialFill,
            _ => arb_types::OrderStatus::Pending,
        };
        let side = if self.side.to_lowercase() == "sell" {
            arb_types::Side::No
        } else {
            arb_types::Side::Yes
        };
        let filled_quantity: u32 = self
            .filled_size
            .parse::<f64>()
            .unwrap_or(0.0) as u32;

        arb_types::OrderResponse {
            order_id: self.order_id.clone(),
            status,
            filled_quantity,
            price: self.price,
            side,
            market_id: self.asset_id.clone(),
        }
    }
}

impl PolyBookResponse {
    /// Convert to an arb-types OrderBook.
    pub fn to_order_book(&self, market_id: &str) -> arb_types::OrderBook {
        let bids = self
            .bids
            .iter()
            .filter_map(|l| {
                let price = l.price.parse::<Decimal>().ok()?;
                let quantity = l.size.parse::<f64>().ok()? as u32;
                Some(arb_types::OrderBookLevel { price, quantity })
            })
            .collect();
        let asks = self
            .asks
            .iter()
            .filter_map(|l| {
                let price = l.price.parse::<Decimal>().ok()?;
                let quantity = l.size.parse::<f64>().ok()? as u32;
                Some(arb_types::OrderBookLevel { price, quantity })
            })
            .collect();

        arb_types::OrderBook {
            market_id: market_id.to_string(),
            bids,
            asks,
            timestamp: Utc::now(),
        }
    }
}

impl PolyPositionResponse {
    /// Convert to an arb-types PlatformPosition.
    pub fn to_platform_position(&self) -> arb_types::PlatformPosition {
        let side = if self.side.to_lowercase() == "sell" || self.side.to_lowercase() == "no" {
            arb_types::Side::No
        } else {
            arb_types::Side::Yes
        };
        arb_types::PlatformPosition {
            market_id: self.condition_id.clone(),
            side,
            quantity: self.size.to_string().parse::<f64>().unwrap_or(0.0) as u32,
            avg_price: self.avg_price,
        }
    }
}

// ---------------------------------------------------------------------------
// Custom deserializer for Decimal fields that may be missing or null
// ---------------------------------------------------------------------------

fn deserialize_decimal_or_default<'de, D>(deserializer: D) -> Result<Decimal, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de;

    struct DecimalVisitor;

    impl<'de> de::Visitor<'de> for DecimalVisitor {
        type Value = Decimal;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a decimal number or string")
        }

        fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
            Decimal::try_from(v).map_err(de::Error::custom)
        }

        fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
            Ok(Decimal::from(v))
        }

        fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
            Ok(Decimal::from(v))
        }

        fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
            if v.is_empty() {
                return Ok(Decimal::ZERO);
            }
            v.parse::<Decimal>().map_err(de::Error::custom)
        }

        fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(Decimal::ZERO)
        }

        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(Decimal::ZERO)
        }
    }

    deserializer.deserialize_any(DecimalVisitor)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_market() -> PolyMarketResponse {
        PolyMarketResponse {
            condition_id: "cond-1".into(),
            question: "Test?".into(),
            tokens: vec![],
            outcomes: vec![],
            outcome_prices: None,
            volume: Decimal::ZERO,
            liquidity: Decimal::ZERO,
            active: true,
            closed: false,
            end_date_iso: None,
            market_slug: None,
            clob_token_ids: None,
            neg_risk: None,
        }
    }

    #[test]
    fn extract_token_ids_from_tokens() {
        let mut m = base_market();
        m.tokens = vec![
            PolyToken { token_id: "tok-yes".into(), outcome: "Yes".into(), price: Decimal::ZERO },
            PolyToken { token_id: "tok-no".into(), outcome: "No".into(), price: Decimal::ZERO },
        ];
        let (yes, no) = m.extract_token_ids().unwrap();
        assert_eq!(yes, "tok-yes");
        assert_eq!(no, "tok-no");
    }

    #[test]
    fn extract_token_ids_from_clob_token_ids() {
        let mut m = base_market();
        m.clob_token_ids = Some(r#"["clob-yes","clob-no"]"#.into());
        let (yes, no) = m.extract_token_ids().unwrap();
        assert_eq!(yes, "clob-yes");
        assert_eq!(no, "clob-no");
    }

    #[test]
    fn extract_token_ids_both_empty() {
        let m = base_market();
        assert!(m.extract_token_ids().is_none());
    }

    #[test]
    fn extract_token_ids_malformed_clob() {
        let mut m = base_market();
        m.clob_token_ids = Some("not-json".into());
        assert!(m.extract_token_ids().is_none());
    }

    #[test]
    fn extract_token_ids_single_entry() {
        let mut m = base_market();
        m.clob_token_ids = Some(r#"["only-one"]"#.into());
        assert!(m.extract_token_ids().is_none());
    }

    #[test]
    fn extract_token_ids_case_insensitive() {
        let mut m = base_market();
        m.tokens = vec![
            PolyToken { token_id: "tok-YES".into(), outcome: "YES".into(), price: Decimal::ZERO },
            PolyToken { token_id: "tok-NO".into(), outcome: "no".into(), price: Decimal::ZERO },
        ];
        let (yes, no) = m.extract_token_ids().unwrap();
        assert_eq!(yes, "tok-YES");
        assert_eq!(no, "tok-NO");
    }
}
