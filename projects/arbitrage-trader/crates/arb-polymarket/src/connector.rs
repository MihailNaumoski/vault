use async_trait::async_trait;
use rust_decimal::Decimal;
use tokio::sync::mpsc;

use arb_types::*;

use crate::client::PolymarketClient;
use crate::error::PolymarketError;
use crate::types::PolyConfig;
use crate::ws::PolyWebSocket;

/// Full Polymarket connector implementing `PredictionMarketConnector`.
pub struct PolymarketConnector {
    client: PolymarketClient,
    ws: PolyWebSocket,
}

impl PolymarketConnector {
    /// Create a new connector from configuration.
    pub fn new(config: PolyConfig) -> Result<Self, PolymarketError> {
        let ws_url = config.ws_url.clone();
        let client = PolymarketClient::new(config)?;
        let ws = PolyWebSocket::new(ws_url);
        Ok(Self { client, ws })
    }

    /// Resolve the (yes_token_id, no_token_id) pair for a given condition ID.
    pub async fn resolve_token_ids(
        &self,
        condition_id: &str,
    ) -> Result<(String, String), crate::error::PolymarketError> {
        self.client.resolve_token_ids(condition_id).await
    }
}

#[async_trait]
impl PredictionMarketConnector for PolymarketConnector {
    fn platform(&self) -> Platform {
        Platform::Polymarket
    }

    async fn list_markets(&self, status: MarketStatus) -> Result<Vec<Market>, ArbError> {
        // MVP: fetch the first page only
        let page = self
            .client
            .fetch_markets(None)
            .await
            .map_err(ArbError::from)?;

        tracing::debug!(raw_count = page.len(), "Gamma API returned markets");
        if let Some(first) = page.first() {
            tracing::debug!(
                q = %first.question,
                tokens = first.tokens.len(),
                outcome_prices = ?first.outcome_prices,
                active = first.active,
                closed = first.closed,
                "First market sample"
            );
            let m = first.to_market();
            tracing::debug!(yes = %m.yes_price, no = %m.no_price, status = ?m.status, "Converted market");
        }

        let markets = page
            .iter()
            .map(|m| m.to_market())
            .filter(|m| m.status == status)
            .filter(|m| {
                arb_types::validate_price(m.yes_price)
                    && arb_types::validate_price(m.no_price)
            })
            .collect();

        Ok(markets)
    }

    async fn get_market(&self, id: &str) -> Result<Market, ArbError> {
        let resp = self
            .client
            .fetch_market(id)
            .await
            .map_err(ArbError::from)?;
        let market = resp.to_market();
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
        let resp = self
            .client
            .fetch_order_book(id)
            .await
            .map_err(ArbError::from)?;
        Ok(resp.to_order_book(id))
    }

    async fn subscribe_prices(
        &self,
        ids: &[String],
        tx: mpsc::Sender<PriceUpdate>,
    ) -> Result<SubHandle, ArbError> {
        self.ws
            .subscribe(ids, tx)
            .await
            .map_err(ArbError::from)
    }

    async fn place_limit_order(&self, req: &LimitOrderRequest) -> Result<OrderResponse, ArbError> {
        if req.market_id.is_empty() {
            return Err(ArbError::Other(
                "market_id must not be empty".to_string(),
            ));
        }
        let token_id = &req.market_id;
        let resp = self
            .client
            .post_order(req, token_id)
            .await
            .map_err(ArbError::from)?;
        Ok(resp.to_order_response())
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
        Ok(resp.to_order_response())
    }

    async fn list_open_orders(&self) -> Result<Vec<OrderResponse>, ArbError> {
        let orders = self
            .client
            .fetch_open_orders()
            .await
            .map_err(ArbError::from)?;
        Ok(orders.iter().map(|o| o.to_order_response()).collect())
    }

    async fn get_balance(&self) -> Result<Decimal, ArbError> {
        self.client
            .fetch_balance()
            .await
            .map_err(ArbError::from)
    }

    async fn get_positions(&self) -> Result<Vec<PlatformPosition>, ArbError> {
        let positions = self
            .client
            .fetch_positions()
            .await
            .map_err(ArbError::from)?;
        Ok(positions
            .iter()
            .map(|p| p.to_platform_position())
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;

    #[test]
    fn test_market_conversion() {
        let poly_market = PolyMarketResponse {
            condition_id: "cond-123".to_string(),
            question: "Will BTC hit 100k?".to_string(),
            tokens: vec![
                PolyToken {
                    token_id: "tok-yes".to_string(),
                    outcome: "Yes".to_string(),
                    price: rust_decimal_macros::dec!(0.65),
                },
                PolyToken {
                    token_id: "tok-no".to_string(),
                    outcome: "No".to_string(),
                    price: rust_decimal_macros::dec!(0.35),
                },
            ],
            outcomes: vec!["Yes".to_string(), "No".to_string()],
            outcome_prices: None,
            volume: rust_decimal_macros::dec!(100000),
            liquidity: rust_decimal_macros::dec!(50000),
            active: true,
            closed: false,
            end_date_iso: None,
            market_slug: None,
            clob_token_ids: None,
        };

        let market = poly_market.to_market();
        assert_eq!(market.platform, Platform::Polymarket);
        assert_eq!(market.platform_id, "cond-123");
        assert_eq!(market.question, "Will BTC hit 100k?");
        assert_eq!(market.yes_price, rust_decimal_macros::dec!(0.65));
        assert_eq!(market.no_price, rust_decimal_macros::dec!(0.35));
        assert_eq!(market.status, MarketStatus::Open);
    }

    #[test]
    fn test_order_book_conversion() {
        let poly_book = PolyBookResponse {
            market: Some("mkt-1".to_string()),
            asset_id: Some("tok-1".to_string()),
            bids: vec![
                PolyBookLevel {
                    price: "0.45".to_string(),
                    size: "100".to_string(),
                },
                PolyBookLevel {
                    price: "0.44".to_string(),
                    size: "200".to_string(),
                },
            ],
            asks: vec![
                PolyBookLevel {
                    price: "0.55".to_string(),
                    size: "150".to_string(),
                },
            ],
            timestamp: None,
            min_order_size: None,
            tick_size: None,
            neg_risk: None,
            last_trade_price: None,
            hash: None,
        };

        let ob = poly_book.to_order_book("tok-1");
        assert_eq!(ob.market_id, "tok-1");
        assert_eq!(ob.bids.len(), 2);
        assert_eq!(ob.asks.len(), 1);
        assert_eq!(ob.bids[0].price, rust_decimal_macros::dec!(0.45));
        assert_eq!(ob.bids[0].quantity, 100);
    }

    #[test]
    fn test_order_response_conversion() {
        let poly_order = PolyOrderResponse {
            order_id: "ord-1".to_string(),
            status: "live".to_string(),
            side: "buy".to_string(),
            price: rust_decimal_macros::dec!(0.50),
            size: "100".to_string(),
            filled_size: "30".to_string(),
            market: "mkt-1".to_string(),
            asset_id: "tok-1".to_string(),
        };

        let resp = poly_order.to_order_response();
        assert_eq!(resp.order_id, "ord-1");
        assert_eq!(resp.status, OrderStatus::Open);
        assert_eq!(resp.filled_quantity, 30);
        assert_eq!(resp.side, Side::Yes);
        assert_eq!(resp.market_id, "tok-1");
    }
}
