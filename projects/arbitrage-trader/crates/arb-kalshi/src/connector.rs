use arb_types::*;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::client::KalshiClient;
use crate::error::KalshiError;
use crate::types::{KalshiBookResponse, KalshiConfig, KalshiMarketResponse, KalshiOrderRequest};
use crate::ws::KalshiWebSocket;

/// Full `PredictionMarketConnector` implementation for Kalshi.
///
/// This is the trait boundary where all price conversions happen:
/// - **Inbound** (Kalshi cents -> arb-types Decimal): `kalshi_cents_to_decimal()`
/// - **Outbound** (arb-types Decimal -> Kalshi cents): `(price * 100).to_u32()`
pub struct KalshiConnector {
    client: KalshiClient,
    ws: KalshiWebSocket,
}

impl KalshiConnector {
    /// Create a new Kalshi connector from configuration.
    pub fn new(config: KalshiConfig) -> Result<Self, KalshiError> {
        let ws_url = config.ws_url.clone();
        let client = KalshiClient::new(config)?;
        let ws = KalshiWebSocket::new(ws_url, client.auth().clone());
        Ok(Self { client, ws })
    }
}

#[async_trait]
impl PredictionMarketConnector for KalshiConnector {
    fn platform(&self) -> Platform {
        Platform::Kalshi
    }

    async fn list_markets(&self, status: MarketStatus) -> Result<Vec<Market>, ArbError> {
        let status_str = match status {
            MarketStatus::Open => "open",
            MarketStatus::Closed => "closed",
            MarketStatus::Settled => "settled",
        };
        let markets = self
            .client
            .fetch_markets(None, Some(status_str))
            .await
            .map_err(ArbError::from)?;
        Ok(markets
            .into_iter()
            .map(convert_market_response)
            .filter(|m| {
                arb_types::validate_price(m.yes_price)
                    && arb_types::validate_price(m.no_price)
            })
            .collect())
    }

    async fn get_market(&self, id: &str) -> Result<Market, ArbError> {
        let market = self.client.fetch_market(id).await.map_err(ArbError::from)?;
        let market = convert_market_response(market);
        if !arb_types::validate_price(market.yes_price) {
            return Err(ArbError::InvalidPrice(format!(
                "yes_price {} out of [0,1] for market {}",
                market.yes_price, id
            )));
        }
        if !arb_types::validate_price(market.no_price) {
            return Err(ArbError::InvalidPrice(format!(
                "no_price {} out of [0,1] for market {}",
                market.no_price, id
            )));
        }
        Ok(market)
    }

    async fn get_order_book(&self, id: &str) -> Result<OrderBook, ArbError> {
        let book = self
            .client
            .fetch_order_book(id)
            .await
            .map_err(ArbError::from)?;
        Ok(convert_book_response(id, book))
    }

    async fn subscribe_prices(
        &self,
        ids: &[String],
        tx: tokio::sync::mpsc::Sender<PriceUpdate>,
    ) -> Result<SubHandle, ArbError> {
        self.ws.subscribe(ids, tx).await.map_err(ArbError::from)
    }

    async fn place_limit_order(&self, req: &LimitOrderRequest) -> Result<OrderResponse, ArbError> {
        if req.market_id.is_empty() {
            return Err(ArbError::Other(
                "market_id must not be empty".to_string(),
            ));
        }
        let kalshi_req = convert_limit_order_to_kalshi(req);
        let resp = self
            .client
            .post_order(&kalshi_req)
            .await
            .map_err(ArbError::from)?;
        Ok(convert_order_response(resp))
    }

    async fn cancel_order(&self, order_id: &str) -> Result<(), ArbError> {
        self.client
            .cancel_order(order_id)
            .await
            .map_err(ArbError::from)
    }

    async fn get_order(&self, order_id: &str) -> Result<OrderResponse, ArbError> {
        let resp = self
            .client
            .fetch_order(order_id)
            .await
            .map_err(ArbError::from)?;
        Ok(convert_order_response(resp))
    }

    async fn list_open_orders(&self) -> Result<Vec<OrderResponse>, ArbError> {
        // Single request for all open orders (addresses batching gap G5)
        let orders = self
            .client
            .fetch_open_orders()
            .await
            .map_err(ArbError::from)?;
        Ok(orders.into_iter().map(convert_order_response).collect())
    }

    async fn get_balance(&self) -> Result<Decimal, ArbError> {
        let balance = self
            .client
            .fetch_balance()
            .await
            .map_err(ArbError::from)?;
        // Kalshi balance is in cents; convert to dollars
        Ok(Decimal::new(balance.balance, 2))
    }

    async fn get_positions(&self) -> Result<Vec<PlatformPosition>, ArbError> {
        let positions = self
            .client
            .fetch_positions()
            .await
            .map_err(ArbError::from)?;
        Ok(positions
            .into_iter()
            .map(|p| {
                let quantity = p.position.unsigned_abs();
                let side = if p.position >= 0 {
                    Side::Yes
                } else {
                    Side::No
                };
                let avg_price = if quantity > 0 {
                    price::kalshi_cents_to_decimal(
                        (p.total_cost.unsigned_abs() as u32) / quantity,
                    )
                } else {
                    dec!(0)
                };
                PlatformPosition {
                    market_id: p.ticker,
                    side,
                    quantity,
                    avg_price,
                }
            })
            .collect())
    }
}

// ---------------------------------------------------------------------------
// Conversion helpers (Kalshi types <-> arb-types)
// ---------------------------------------------------------------------------

/// Convert a Kalshi market response to the shared `Market` type.
/// Prices are converted from cents to Decimal.
fn convert_market_response(m: KalshiMarketResponse) -> Market {
    let status = match m.status.as_str() {
        "open" | "active" => MarketStatus::Open,
        "closed" => MarketStatus::Closed,
        "settled" | "finalized" => MarketStatus::Settled,
        _ => MarketStatus::Closed,
    };

    let close_time = m
        .close_time
        .as_deref()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(Utc::now);

    // Prefer dollar string fields when available, fall back to cents
    let yes_price = m
        .yes_bid_dollars
        .as_ref()
        .and_then(|s| s.parse::<Decimal>().ok())
        .unwrap_or_else(|| price::kalshi_cents_to_decimal(m.yes_bid));
    let no_price = m
        .no_bid_dollars
        .as_ref()
        .and_then(|s| s.parse::<Decimal>().ok())
        .unwrap_or_else(|| price::kalshi_cents_to_decimal(m.no_bid));
    let volume = m
        .volume_fp
        .as_ref()
        .and_then(|s| s.parse::<Decimal>().ok())
        .unwrap_or_else(|| Decimal::from(m.volume));

    Market {
        id: MarketId::new(),
        platform: Platform::Kalshi,
        platform_id: m.ticker,
        question: m.title,
        yes_price,
        no_price,
        volume,
        liquidity: Decimal::from(m.liquidity),
        status,
        close_time,
        updated_at: Utc::now(),
    }
}

/// Convert a Kalshi order book response to the shared `OrderBook` type.
/// Each level's price is converted from cents to Decimal.
fn convert_book_response(market_id: &str, book: KalshiBookResponse) -> OrderBook {
    let bids = book
        .yes
        .iter()
        .filter_map(|level| {
            if level.len() >= 2 {
                Some(OrderBookLevel {
                    price: price::kalshi_cents_to_decimal(level[0]),
                    quantity: level[1],
                })
            } else {
                None
            }
        })
        .collect();

    let asks = book
        .no
        .iter()
        .filter_map(|level| {
            if level.len() >= 2 {
                Some(OrderBookLevel {
                    price: price::kalshi_cents_to_decimal(level[0]),
                    quantity: level[1],
                })
            } else {
                None
            }
        })
        .collect();

    OrderBook {
        market_id: market_id.to_string(),
        bids,
        asks,
        timestamp: Utc::now(),
    }
}

/// Convert an arb-types `LimitOrderRequest` to a Kalshi order request.
/// Price is converted from Decimal (0.xx) to cents (xx).
pub(crate) fn convert_limit_order_to_kalshi(req: &LimitOrderRequest) -> KalshiOrderRequest {
    let price_cents = decimal_to_cents(req.price);
    let price_str = format!("{}", req.price);
    let (side_str, yes_price, no_price, yes_price_dollars, no_price_dollars) = match req.side {
        Side::Yes => (
            "yes".to_string(),
            Some(price_cents),
            None,
            Some(price_str),
            None,
        ),
        Side::No => (
            "no".to_string(),
            None,
            Some(price_cents),
            None,
            Some(price_str),
        ),
    };

    KalshiOrderRequest {
        ticker: req.market_id.clone(),
        action: "buy".to_string(),
        side: side_str,
        r#type: "limit".to_string(),
        count: req.quantity,
        yes_price,
        no_price,
        yes_price_dollars,
        no_price_dollars,
    }
}

/// Convert a Kalshi order response to the shared `OrderResponse` type.
fn convert_order_response(resp: crate::types::KalshiOrderResponse) -> OrderResponse {
    let status = match resp.status.as_str() {
        "resting" | "pending" => OrderStatus::Open,
        "canceled" | "cancelled" => OrderStatus::Cancelled,
        "executed" => OrderStatus::Filled,
        "partial" => OrderStatus::PartialFill,
        _ => OrderStatus::Pending,
    };

    let side = match resp.side.as_str() {
        "yes" => Side::Yes,
        _ => Side::No,
    };

    // Prefer dollar string fields, fall back to cents
    let price_decimal = if resp.yes_price > 0 || resp.yes_price_dollars.is_some() {
        resp.yes_price_dollars
            .as_ref()
            .and_then(|s| s.parse::<Decimal>().ok())
            .unwrap_or_else(|| price::kalshi_cents_to_decimal(resp.yes_price))
    } else {
        resp.no_price_dollars
            .as_ref()
            .and_then(|s| s.parse::<Decimal>().ok())
            .unwrap_or_else(|| price::kalshi_cents_to_decimal(resp.no_price))
    };

    OrderResponse {
        order_id: resp.order_id,
        status,
        filled_quantity: resp.filled_count,
        price: price_decimal,
        side,
        market_id: resp.ticker,
    }
}

/// Convert a Decimal price (0.xx) to cents (xx) for the Kalshi API.
pub(crate) fn decimal_to_cents(price: Decimal) -> u32 {
    use rust_decimal::prelude::ToPrimitive;
    let cents = price * dec!(100);
    cents.to_u32().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn test_market_conversion_price_normalization() {
        let kalshi_market = KalshiMarketResponse {
            ticker: "PRES-2026-DEM".to_string(),
            title: "Will a Democrat win 2026?".to_string(),
            status: "open".to_string(),
            yes_ask: 55,
            yes_bid: 53,
            no_ask: 47,
            no_bid: 45,
            volume: 10000,
            open_interest: 5000,
            close_time: Some("2026-11-04T00:00:00Z".to_string()),
            liquidity: 2000,
            yes_ask_dollars: None,
            yes_bid_dollars: None,
            no_ask_dollars: None,
            no_bid_dollars: None,
            volume_fp: None,
            open_interest_fp: None,
        };
        let market = convert_market_response(kalshi_market);
        assert_eq!(market.platform, Platform::Kalshi);
        assert_eq!(market.platform_id, "PRES-2026-DEM");
        // yes_price should be 0.53 (from yes_bid cents), not 53
        assert_eq!(market.yes_price, dec!(0.53));
        // no_price should be 0.45 (from no_bid cents), not 45
        assert_eq!(market.no_price, dec!(0.45));
        assert_eq!(market.status, MarketStatus::Open);
    }

    #[test]
    fn test_order_book_cents_to_decimal() {
        let book = KalshiBookResponse {
            yes: vec![vec![42, 100], vec![41, 200]],
            no: vec![vec![58, 100], vec![59, 200]],
        };
        let ob = convert_book_response("PRES-2026-DEM", book);
        assert_eq!(ob.market_id, "PRES-2026-DEM");
        // Bids (yes side) should be in decimal
        assert_eq!(ob.bids[0].price, dec!(0.42));
        assert_eq!(ob.bids[0].quantity, 100);
        assert_eq!(ob.bids[1].price, dec!(0.41));
        // Asks (no side) should be in decimal
        assert_eq!(ob.asks[0].price, dec!(0.58));
        assert_eq!(ob.asks[1].price, dec!(0.59));
    }

    #[test]
    fn test_order_request_decimal_to_cents() {
        let req = LimitOrderRequest {
            market_id: "PRES-2026-DEM".to_string(),
            side: Side::Yes,
            price: dec!(0.42),
            quantity: 10,
        };
        let kalshi_req = convert_limit_order_to_kalshi(&req);
        assert_eq!(kalshi_req.ticker, "PRES-2026-DEM");
        assert_eq!(kalshi_req.action, "buy");
        assert_eq!(kalshi_req.side, "yes");
        assert_eq!(kalshi_req.r#type, "limit");
        assert_eq!(kalshi_req.count, 10);
        assert_eq!(kalshi_req.yes_price, Some(42)); // 0.42 -> 42 cents
        assert!(kalshi_req.no_price.is_none());
    }

    #[test]
    fn test_order_request_no_side() {
        let req = LimitOrderRequest {
            market_id: "PRES-2026-DEM".to_string(),
            side: Side::No,
            price: dec!(0.55),
            quantity: 5,
        };
        let kalshi_req = convert_limit_order_to_kalshi(&req);
        assert_eq!(kalshi_req.side, "no");
        assert!(kalshi_req.yes_price.is_none());
        assert_eq!(kalshi_req.no_price, Some(55)); // 0.55 -> 55 cents
    }

    #[test]
    fn test_order_response_cents_to_decimal() {
        let resp = crate::types::KalshiOrderResponse {
            order_id: "order-abc".to_string(),
            status: "resting".to_string(),
            remaining_count: 8,
            filled_count: 2,
            yes_price: 42,
            no_price: 0,
            side: "yes".to_string(),
            ticker: "PRES-2026-DEM".to_string(),
            action: "buy".to_string(),
            remaining_count_fp: None,
            fill_count_fp: None,
            initial_count_fp: None,
            yes_price_dollars: None,
            no_price_dollars: None,
            taker_fees_dollars: None,
            maker_fees_dollars: None,
            created_time: None,
            last_update_time: None,
        };
        let order = convert_order_response(resp);
        assert_eq!(order.order_id, "order-abc");
        assert_eq!(order.status, OrderStatus::Open);
        assert_eq!(order.filled_quantity, 2);
        assert_eq!(order.price, dec!(0.42)); // 42 cents -> 0.42
        assert_eq!(order.side, Side::Yes);
        assert_eq!(order.market_id, "PRES-2026-DEM");
    }

    #[test]
    fn test_decimal_to_cents() {
        assert_eq!(decimal_to_cents(dec!(0.42)), 42);
        assert_eq!(decimal_to_cents(dec!(0.01)), 1);
        assert_eq!(decimal_to_cents(dec!(0.99)), 99);
        assert_eq!(decimal_to_cents(dec!(0.50)), 50);
    }

    #[test]
    fn test_status_mapping() {
        // Test various Kalshi status strings
        let cases = vec![
            ("open", MarketStatus::Open),
            ("active", MarketStatus::Open),
            ("closed", MarketStatus::Closed),
            ("settled", MarketStatus::Settled),
            ("finalized", MarketStatus::Settled),
            ("unknown", MarketStatus::Closed), // default
        ];
        for (status_str, expected) in cases {
            let m = KalshiMarketResponse {
                ticker: "TEST".to_string(),
                title: "Test".to_string(),
                status: status_str.to_string(),
                yes_ask: 0,
                yes_bid: 0,
                no_ask: 0,
                no_bid: 0,
                volume: 0,
                open_interest: 0,
                close_time: None,
                liquidity: 0,
                yes_ask_dollars: None,
                yes_bid_dollars: None,
                no_ask_dollars: None,
                no_bid_dollars: None,
                volume_fp: None,
                open_interest_fp: None,
            };
            let market = convert_market_response(m);
            assert_eq!(market.status, expected, "status '{}' mismatch", status_str);
        }
    }
}
