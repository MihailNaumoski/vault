use rust_decimal::Decimal;

use crate::auth::PolyAuth;
use crate::error::PolymarketError;
use crate::rate_limit::PolyRateLimiter;
use crate::signing::OrderSigner;
use crate::types::*;

/// Polymarket REST client for market data and trading.
pub struct PolymarketClient {
    http: reqwest::Client,
    auth: PolyAuth,
    signer: OrderSigner,
    rate_limiter: PolyRateLimiter,
    /// CLOB API base URL (e.g. https://clob.polymarket.com).
    base_url: String,
    /// Gamma API base URL (e.g. https://gamma-api.polymarket.com).
    gamma_url: String,
}

impl PolymarketClient {
    /// Create a new client from configuration.
    pub fn new(config: PolyConfig) -> Result<Self, PolymarketError> {
        let signer = OrderSigner::new(&config.private_key, config.chain_id)?;
        let wallet_address = format!("{:?}", signer.address()); // checksummed hex
        let auth = PolyAuth::new(config.api_key, config.secret, config.passphrase, wallet_address)?;
        let http = reqwest::Client::builder()
            .build()
            .map_err(PolymarketError::Http)?;

        Ok(Self {
            http,
            auth,
            signer,
            rate_limiter: PolyRateLimiter::new(),
            base_url: config.clob_url,
            gamma_url: config.gamma_url,
        })
    }

    // -----------------------------------------------------------------------
    // Market data (Gamma API for metadata, CLOB API for prices/books)
    // -----------------------------------------------------------------------

    /// Fetch a page of markets from the Gamma API.
    pub async fn fetch_markets(
        &self,
        next_cursor: Option<&str>,
    ) -> Result<Vec<PolyMarketResponse>, PolymarketError> {
        self.rate_limiter.acquire().await;

        let mut url = format!("{}/markets?active=true&closed=false&limit=50", self.gamma_url);
        if let Some(cursor) = next_cursor {
            url = format!("{url}&next_cursor={cursor}");
        }

        let req = self.http.get(&url);
        let resp = self.send_with_retry(req).await?;
        let markets: Vec<PolyMarketResponse> = resp.json().await?;
        Ok(markets)
    }

    /// Fetch a single market by condition ID from the Gamma API.
    pub async fn fetch_market(
        &self,
        condition_id: &str,
    ) -> Result<PolyMarketResponse, PolymarketError> {
        self.rate_limiter.acquire().await;

        let url = format!("{}/markets/{}", self.gamma_url, condition_id);
        let req = self.http.get(&url);
        let resp = self.send_with_retry(req).await?;
        let market: PolyMarketResponse = resp.json().await?;
        Ok(market)
    }

    /// Fetch the order book for a token ID from the CLOB API.
    pub async fn fetch_order_book(
        &self,
        token_id: &str,
    ) -> Result<PolyBookResponse, PolymarketError> {
        self.rate_limiter.acquire().await;

        let path = format!("/book?token_id={token_id}");
        let url = format!("{}{}", self.base_url, path);
        let req = self.http.get(&url);
        let resp = self.send_with_retry(req).await?;
        let book: PolyBookResponse = resp.json().await?;
        Ok(book)
    }

    /// Fetch the mid-market price for a token ID.
    pub async fn fetch_price(&self, token_id: &str) -> Result<Decimal, PolymarketError> {
        self.rate_limiter.acquire().await;

        let path = format!("/price?token_id={token_id}");
        let url = format!("{}{}", self.base_url, path);
        let req = self.http.get(&url);
        let resp = self.send_with_retry(req).await?;
        let price_resp: serde_json::Value = resp.json().await?;
        let price_str = price_resp
            .get("price")
            .and_then(|v| v.as_str())
            .unwrap_or("0");
        let price: Decimal = price_str
            .parse()
            .map_err(|e| PolymarketError::Api {
                status: 0,
                message: format!("invalid price format: {e}"),
            })?;
        Ok(price)
    }

    // -----------------------------------------------------------------------
    // Trading (CLOB API, authenticated)
    // -----------------------------------------------------------------------

    /// Place an order via the CLOB API.
    pub async fn post_order(
        &self,
        req: &arb_types::LimitOrderRequest,
        token_id: &str,
        neg_risk: bool,
    ) -> Result<PolyOrderResponse, PolymarketError> {
        self.rate_limiter.acquire().await;

        let path = "/order";
        let signed_body = self.signer.sign_order(req, token_id, neg_risk).await?;
        let body_str = serde_json::to_string(&signed_body)?;
        let auth_headers = self.auth.headers("POST", path, &body_str)?;

        let url = format!("{}{}", self.base_url, path);
        let request = self
            .http
            .post(&url)
            .headers(auth_headers)
            .header("Content-Type", "application/json")
            .body(body_str);
        let resp = self.send_with_retry(request).await?;
        let order_resp: PolyOrderResponse = resp.json().await?;
        Ok(order_resp)
    }

    /// Cancel an order by ID.
    pub async fn cancel_order(&self, order_id: &str) -> Result<(), PolymarketError> {
        self.rate_limiter.acquire().await;

        let path = format!("/order/{order_id}");
        let auth_headers = self.auth.headers("DELETE", &path, "")?;

        let url = format!("{}{}", self.base_url, path);
        let req = self.http.delete(&url).headers(auth_headers);
        self.send_with_retry(req).await?;
        Ok(())
    }

    /// Fetch all open orders.
    pub async fn fetch_open_orders(&self) -> Result<Vec<PolyOrderResponse>, PolymarketError> {
        self.rate_limiter.acquire().await;

        let path = "/orders";
        let auth_headers = self.auth.headers("GET", path, "")?;

        let url = format!("{}{}", self.base_url, path);
        let req = self.http.get(&url).headers(auth_headers);
        let resp = self.send_with_retry(req).await?;
        let orders: Vec<PolyOrderResponse> = resp.json().await?;
        Ok(orders)
    }

    /// Fetch a single order by ID.
    pub async fn fetch_order(&self, order_id: &str) -> Result<PolyOrderResponse, PolymarketError> {
        self.rate_limiter.acquire().await;

        let path = format!("/orders/{order_id}");
        let auth_headers = self.auth.headers("GET", &path, "")?;

        let url = format!("{}{}", self.base_url, path);
        let req = self.http.get(&url).headers(auth_headers);
        let resp = self.send_with_retry(req).await?;
        let order: PolyOrderResponse = resp.json().await?;
        Ok(order)
    }

    // -----------------------------------------------------------------------
    // Account (CLOB API, authenticated)
    // -----------------------------------------------------------------------

    /// Fetch current positions.
    pub async fn fetch_positions(&self) -> Result<Vec<PolyPositionResponse>, PolymarketError> {
        self.rate_limiter.acquire().await;

        let path = "/positions";
        let auth_headers = self.auth.headers("GET", path, "")?;

        let url = format!("{}{}", self.base_url, path);
        let req = self.http.get(&url).headers(auth_headers);
        let resp = self.send_with_retry(req).await?;
        let positions: Vec<PolyPositionResponse> = resp.json().await?;
        Ok(positions)
    }

    /// Fetch account balance.
    pub async fn fetch_balance(&self) -> Result<Decimal, PolymarketError> {
        self.rate_limiter.acquire().await;

        let path = "/balance";
        let auth_headers = self.auth.headers("GET", path, "")?;

        let url = format!("{}{}", self.base_url, path);
        let req = self.http.get(&url).headers(auth_headers);
        let resp = self.send_with_retry(req).await?;
        let balance_resp: PolyBalanceResponse = resp.json().await?;
        Ok(balance_resp.balance)
    }

    /// Resolve the (yes_token_id, no_token_id) pair for a given condition ID.
    pub async fn resolve_token_ids(
        &self,
        condition_id: &str,
    ) -> Result<(String, String), PolymarketError> {
        let market = self.fetch_market(condition_id).await?;
        market.extract_token_ids().ok_or_else(|| PolymarketError::Api {
            status: 0,
            message: format!("no token IDs found for condition {condition_id}"),
        })
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// Send a request with automatic 429 retry logic.
    ///
    /// If the server returns HTTP 429, reads the `Retry-After` header
    /// (defaulting to 1 second) and retries up to 3 times.
    async fn send_with_retry(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, PolymarketError> {
        let built = request.build()?;
        let mut attempts = 0u32;
        let max_retries = 3u32;
        let mut current_request = built;

        loop {
            let retry_req = current_request.try_clone();

            let resp = self.http.execute(current_request).await?;

            if resp.status() == 429 {
                attempts += 1;
                if attempts > max_retries {
                    return Err(PolymarketError::RateLimited);
                }

                let wait_secs: u64 = resp
                    .headers()
                    .get("Retry-After")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(1);

                tokio::time::sleep(std::time::Duration::from_secs(wait_secs)).await;

                current_request = retry_req.ok_or(PolymarketError::RateLimited)?;
                continue;
            }

            if !resp.status().is_success() {
                let status_code = resp.status().as_u16();
                return Err(PolymarketError::Api {
                    status: status_code,
                    message: format!("HTTP {}", resp.status()),
                });
            }

            return Ok(resp);
        }
    }

    /// Check HTTP response status and map errors (kept for future use).
    #[allow(dead_code)]
    fn check_response_status(resp: &reqwest::Response) -> Result<(), PolymarketError> {
        let status = resp.status();
        if status.is_success() {
            return Ok(());
        }
        if status.as_u16() == 429 {
            return Err(PolymarketError::RateLimited);
        }
        // For non-success, we cannot consume the body here (it's borrowed),
        // so we return a generic error with the status code.
        Err(PolymarketError::Api {
            status: status.as_u16(),
            message: format!("HTTP {}", status),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fetch_order_book_deserialize() {
        let json = r#"{
            "market": "0xabc",
            "asset_id": "12345",
            "bids": [
                {"price": "0.45", "size": "100"},
                {"price": "0.44", "size": "200"}
            ],
            "asks": [
                {"price": "0.55", "size": "150"},
                {"price": "0.56", "size": "250"}
            ],
            "timestamp": "1700000000"
        }"#;

        let book: PolyBookResponse = serde_json::from_str(json).unwrap();
        assert_eq!(book.bids.len(), 2);
        assert_eq!(book.asks.len(), 2);
        assert_eq!(book.bids[0].price, "0.45");
        assert_eq!(book.asks[0].size, "150");

        // Convert to arb-types OrderBook
        let ob = book.to_order_book("12345");
        assert_eq!(ob.market_id, "12345");
        assert_eq!(ob.bids.len(), 2);
        assert_eq!(ob.asks.len(), 2);
    }

    #[test]
    fn test_order_response_deserialize() {
        let json = r#"{
            "orderID": "ord-123",
            "status": "live",
            "side": "buy",
            "price": "0.55",
            "size": "100",
            "size_matched": "25",
            "market": "mkt-abc",
            "asset_id": "tok-456"
        }"#;

        let resp: PolyOrderResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.order_id, "ord-123");
        assert_eq!(resp.status, "live");

        let converted = resp.to_order_response();
        assert_eq!(converted.order_id, "ord-123");
        assert_eq!(converted.status, arb_types::OrderStatus::Open);
        assert_eq!(converted.filled_quantity, 25);
        assert_eq!(converted.side, arb_types::Side::Yes);
    }

    #[test]
    fn test_market_response_deserialize() {
        let json = r#"{
            "conditionId": "cond-1",
            "question": "Will it rain?",
            "tokens": [
                {"tokenId": "tok-yes", "outcome": "Yes", "price": 0.6},
                {"tokenId": "tok-no", "outcome": "No", "price": 0.4}
            ],
            "outcomes": ["Yes", "No"],
            "volume": 50000,
            "liquidity": 25000,
            "active": true,
            "closed": false
        }"#;

        let market: PolyMarketResponse = serde_json::from_str(json).unwrap();
        assert_eq!(market.condition_id, "cond-1");
        assert_eq!(market.tokens.len(), 2);

        let converted = market.to_market();
        assert_eq!(converted.platform, arb_types::Platform::Polymarket);
        assert_eq!(converted.question, "Will it rain?");
        assert_eq!(converted.status, arb_types::MarketStatus::Open);
    }

    #[test]
    fn test_position_response_deserialize() {
        let json = r#"{
            "assetId": "tok-123",
            "conditionId": "cond-1",
            "side": "buy",
            "size": 50,
            "avgPrice": "0.55"
        }"#;

        let pos: PolyPositionResponse = serde_json::from_str(json).unwrap();
        let converted = pos.to_platform_position();
        assert_eq!(converted.market_id, "cond-1");
        assert_eq!(converted.side, arb_types::Side::Yes);
        assert_eq!(converted.quantity, 50);
    }

    #[test]
    fn test_balance_response_deserialize() {
        let json = r#"{"balance": 1234.56}"#;
        let resp: PolyBalanceResponse = serde_json::from_str(json).unwrap();
        assert!(resp.balance > Decimal::ZERO);
    }
}
