use reqwest::Client;
use tracing::debug;

use crate::auth::KalshiAuth;
use crate::error::KalshiError;
use crate::rate_limit::KalshiRateLimiter;
use crate::types::{
    KalshiBalanceResponse, KalshiBookResponse, KalshiConfig, KalshiMarketResponse,
    KalshiMarketsListResponse, KalshiOrderRequest, KalshiOrderResponse, KalshiOrdersListResponse,
    KalshiPositionResponse, KalshiPositionsListResponse,
};

/// REST client for the Kalshi trading API.
///
/// Handles authentication, rate limiting, and HTTP transport for all
/// Kalshi REST endpoints. Prices in responses remain in cents; conversion
/// to `Decimal` happens in the connector layer.
pub struct KalshiClient {
    http: Client,
    auth: KalshiAuth,
    rate_limiter: KalshiRateLimiter,
    base_url: String,
}

impl KalshiClient {
    /// Create a new Kalshi REST client.
    pub fn new(config: KalshiConfig) -> Result<Self, KalshiError> {
        let auth = KalshiAuth::new(config.api_key_id, &config.private_key_pem)?;
        let http = Client::builder()
            .use_rustls_tls()
            .build()
            .map_err(KalshiError::Http)?;
        Ok(Self {
            http,
            auth,
            rate_limiter: KalshiRateLimiter::new(),
            base_url: config.base_url,
        })
    }

    /// Return a reference to the auth handler (needed by WebSocket).
    pub fn auth(&self) -> &KalshiAuth {
        &self.auth
    }

    /// The base URL for constructing full paths.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    // -----------------------------------------------------------------------
    // Market data endpoints (100 req/s rate limit)
    // -----------------------------------------------------------------------

    /// List markets, optionally filtered by cursor and status.
    pub async fn fetch_markets(
        &self,
        cursor: Option<&str>,
        status: Option<&str>,
    ) -> Result<Vec<KalshiMarketResponse>, KalshiError> {
        let path = "/markets";
        self.rate_limiter.acquire_read().await;

        let full_path = format!("{}{}", self.base_url, path);
        let mut req = self.http.get(&full_path);

        if let Some(c) = cursor {
            req = req.query(&[("cursor", c)]);
        }
        if let Some(s) = status {
            req = req.query(&[("status", s)]);
        }
        req = req.query(&[("limit", "200")]);

        let headers = self.auth.headers("GET", &format!("/trade-api/v2{}", path))?;
        req = req.headers(headers);

        let resp = req.send().await.map_err(KalshiError::Http)?;
        self.log_rate_limit_headers(&resp);

        if resp.status() == 429 {
            return Err(KalshiError::RateLimited);
        }
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(KalshiError::Api {
                status,
                message: body,
            });
        }

        let list: KalshiMarketsListResponse = resp.json().await.map_err(KalshiError::Http)?;
        Ok(list.markets)
    }

    /// Fetch a single market by ticker.
    pub async fn fetch_market(&self, ticker: &str) -> Result<KalshiMarketResponse, KalshiError> {
        let path = format!("/markets/{}", ticker);
        self.rate_limiter.acquire_read().await;

        let full_url = format!("{}{}", self.base_url, path);
        let api_path = format!("/trade-api/v2{}", path);
        let headers = self.auth.headers("GET", &api_path)?;

        let resp = self
            .http
            .get(&full_url)
            .headers(headers)
            .send()
            .await
            .map_err(KalshiError::Http)?;
        self.log_rate_limit_headers(&resp);

        if resp.status() == 429 {
            return Err(KalshiError::RateLimited);
        }
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(KalshiError::Api {
                status,
                message: body,
            });
        }

        #[derive(serde::Deserialize)]
        struct Wrapper {
            market: KalshiMarketResponse,
        }
        let wrapper: Wrapper = resp.json().await.map_err(KalshiError::Http)?;
        Ok(wrapper.market)
    }

    /// Fetch the order book for a market ticker.
    pub async fn fetch_order_book(&self, ticker: &str) -> Result<KalshiBookResponse, KalshiError> {
        let path = format!("/markets/{}/orderbook", ticker);
        self.rate_limiter.acquire_read().await;

        let full_url = format!("{}{}", self.base_url, path);
        let api_path = format!("/trade-api/v2{}", path);
        let headers = self.auth.headers("GET", &api_path)?;

        let resp = self
            .http
            .get(&full_url)
            .headers(headers)
            .send()
            .await
            .map_err(KalshiError::Http)?;
        self.log_rate_limit_headers(&resp);

        if resp.status() == 429 {
            return Err(KalshiError::RateLimited);
        }
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(KalshiError::Api {
                status,
                message: body,
            });
        }

        #[derive(serde::Deserialize)]
        struct Wrapper {
            orderbook: KalshiBookResponse,
        }
        let wrapper: Wrapper = resp.json().await.map_err(KalshiError::Http)?;
        Ok(wrapper.orderbook)
    }

    // -----------------------------------------------------------------------
    // Trading endpoints (10 req/s rate limit)
    // -----------------------------------------------------------------------

    /// Place an order.
    pub async fn post_order(
        &self,
        req: &KalshiOrderRequest,
    ) -> Result<KalshiOrderResponse, KalshiError> {
        let path = "/portfolio/orders";
        self.rate_limiter.acquire_write().await;

        let full_url = format!("{}{}", self.base_url, path);
        let api_path = format!("/trade-api/v2{}", path);
        let headers = self.auth.headers("POST", &api_path)?;

        let resp = self
            .http
            .post(&full_url)
            .headers(headers)
            .json(req)
            .send()
            .await
            .map_err(KalshiError::Http)?;
        self.log_rate_limit_headers(&resp);

        if resp.status() == 429 {
            return Err(KalshiError::RateLimited);
        }
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(KalshiError::Api {
                status,
                message: body,
            });
        }

        #[derive(serde::Deserialize)]
        struct Wrapper {
            order: KalshiOrderResponse,
        }
        let wrapper: Wrapper = resp.json().await.map_err(KalshiError::Http)?;
        Ok(wrapper.order)
    }

    /// Cancel an order by ID.
    pub async fn cancel_order(&self, order_id: &str) -> Result<(), KalshiError> {
        let path = format!("/portfolio/orders/{}", order_id);
        self.rate_limiter.acquire_write().await;

        let full_url = format!("{}{}", self.base_url, path);
        let api_path = format!("/trade-api/v2{}", path);
        let headers = self.auth.headers("DELETE", &api_path)?;

        let resp = self
            .http
            .delete(&full_url)
            .headers(headers)
            .send()
            .await
            .map_err(KalshiError::Http)?;
        self.log_rate_limit_headers(&resp);

        if resp.status() == 429 {
            return Err(KalshiError::RateLimited);
        }
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(KalshiError::Api {
                status,
                message: body,
            });
        }

        Ok(())
    }

    /// List all open orders (single request, addresses batching gap G5).
    pub async fn fetch_open_orders(&self) -> Result<Vec<KalshiOrderResponse>, KalshiError> {
        let path = "/portfolio/orders";
        self.rate_limiter.acquire_read().await;

        let full_url = format!("{}{}", self.base_url, path);
        let api_path = format!("/trade-api/v2{}", path);
        let headers = self.auth.headers("GET", &api_path)?;

        let resp = self
            .http
            .get(&full_url)
            .headers(headers)
            .send()
            .await
            .map_err(KalshiError::Http)?;
        self.log_rate_limit_headers(&resp);

        if resp.status() == 429 {
            return Err(KalshiError::RateLimited);
        }
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(KalshiError::Api {
                status,
                message: body,
            });
        }

        let list: KalshiOrdersListResponse = resp.json().await.map_err(KalshiError::Http)?;
        Ok(list.orders)
    }

    /// Fetch a single order by ID.
    pub async fn fetch_order(&self, order_id: &str) -> Result<KalshiOrderResponse, KalshiError> {
        let path = format!("/portfolio/orders/{}", order_id);
        self.rate_limiter.acquire_read().await;

        let full_url = format!("{}{}", self.base_url, path);
        let api_path = format!("/trade-api/v2{}", path);
        let headers = self.auth.headers("GET", &api_path)?;

        let resp = self
            .http
            .get(&full_url)
            .headers(headers)
            .send()
            .await
            .map_err(KalshiError::Http)?;
        self.log_rate_limit_headers(&resp);

        if resp.status() == 429 {
            return Err(KalshiError::RateLimited);
        }
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(KalshiError::Api {
                status,
                message: body,
            });
        }

        #[derive(serde::Deserialize)]
        struct Wrapper {
            order: KalshiOrderResponse,
        }
        let wrapper: Wrapper = resp.json().await.map_err(KalshiError::Http)?;
        Ok(wrapper.order)
    }

    // -----------------------------------------------------------------------
    // Account endpoints (read-limited — GET requests)
    // -----------------------------------------------------------------------

    /// Fetch current positions.
    pub async fn fetch_positions(&self) -> Result<Vec<KalshiPositionResponse>, KalshiError> {
        let path = "/portfolio/positions";
        self.rate_limiter.acquire_read().await;

        let full_url = format!("{}{}", self.base_url, path);
        let api_path = format!("/trade-api/v2{}", path);
        let headers = self.auth.headers("GET", &api_path)?;

        let resp = self
            .http
            .get(&full_url)
            .headers(headers)
            .send()
            .await
            .map_err(KalshiError::Http)?;
        self.log_rate_limit_headers(&resp);

        if resp.status() == 429 {
            return Err(KalshiError::RateLimited);
        }
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(KalshiError::Api {
                status,
                message: body,
            });
        }

        let list: KalshiPositionsListResponse = resp.json().await.map_err(KalshiError::Http)?;
        Ok(list.market_positions)
    }

    /// Fetch account balance.
    pub async fn fetch_balance(&self) -> Result<KalshiBalanceResponse, KalshiError> {
        let path = "/portfolio/balance";
        self.rate_limiter.acquire_read().await;

        let full_url = format!("{}{}", self.base_url, path);
        let api_path = format!("/trade-api/v2{}", path);
        let headers = self.auth.headers("GET", &api_path)?;

        let resp = self
            .http
            .get(&full_url)
            .headers(headers)
            .send()
            .await
            .map_err(KalshiError::Http)?;
        self.log_rate_limit_headers(&resp);

        if resp.status() == 429 {
            return Err(KalshiError::RateLimited);
        }
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(KalshiError::Api {
                status,
                message: body,
            });
        }

        let balance: KalshiBalanceResponse = resp.json().await.map_err(KalshiError::Http)?;
        Ok(balance)
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /// Log rate limit headers from a response for monitoring.
    fn log_rate_limit_headers(&self, resp: &reqwest::Response) {
        if let Some(remaining) = resp.headers().get("Ratelimit-Remaining") {
            debug!(
                remaining = ?remaining,
                "Kalshi rate limit remaining"
            );
        }
        if let Some(reset) = resp.headers().get("Ratelimit-Reset") {
            debug!(reset = ?reset, "Kalshi rate limit reset");
        }
    }
}

#[cfg(test)]
/// Build a query string fragment for the signing path.
fn build_query_string(cursor: Option<&str>, status: Option<&str>) -> String {
    let mut parts = Vec::new();
    if let Some(c) = cursor {
        parts.push(format!("cursor={c}"));
    }
    if let Some(s) = status {
        parts.push(format!("status={s}"));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!("?{}", parts.join("&"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;
    use rust_decimal::Decimal;

    #[test]
    fn test_fetch_market_deserialize() {
        let json = r#"{
            "market": {
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
            }
        }"#;

        #[derive(serde::Deserialize)]
        struct Wrapper {
            market: KalshiMarketResponse,
        }
        let wrapper: Wrapper = serde_json::from_str(json).unwrap();
        assert_eq!(wrapper.market.ticker, "PRES-2026-DEM");
        assert_eq!(wrapper.market.yes_ask, 55);
        assert_eq!(wrapper.market.no_bid, 45);
    }

    #[test]
    fn test_fetch_order_book_deserialize() {
        let json = r#"{
            "orderbook": {
                "yes": [[42, 100], [41, 200]],
                "no": [[58, 100], [59, 200]]
            }
        }"#;

        #[derive(serde::Deserialize)]
        struct Wrapper {
            orderbook: KalshiBookResponse,
        }
        let wrapper: Wrapper = serde_json::from_str(json).unwrap();
        assert_eq!(wrapper.orderbook.yes.len(), 2);
        assert_eq!(wrapper.orderbook.yes[0], vec![42, 100]); // price=42 cents, qty=100
        assert_eq!(wrapper.orderbook.no[0], vec![58, 100]);
    }

    #[test]
    fn test_price_conversion_cents_to_decimal() {
        // Prices from Kalshi are in cents. The connector converts.
        let cents = 42u32;
        let decimal = arb_types::price::kalshi_cents_to_decimal(cents);
        assert_eq!(decimal, Decimal::new(42, 2)); // 0.42
    }

    #[test]
    fn test_post_order_request_format() {
        let req = KalshiOrderRequest {
            ticker: "PRES-2026-DEM".to_string(),
            action: "buy".to_string(),
            side: "yes".to_string(),
            r#type: "limit".to_string(),
            count: 10,
            yes_price: Some(42),
            no_price: None,
            yes_price_dollars: None,
            no_price_dollars: None,
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["ticker"], "PRES-2026-DEM");
        assert_eq!(json["action"], "buy");
        assert_eq!(json["side"], "yes");
        assert_eq!(json["type"], "limit");
        assert_eq!(json["count"], 10);
        assert_eq!(json["yes_price"], 42);
        assert!(json["no_price"].is_null());
    }

    #[test]
    fn test_build_query_string() {
        assert_eq!(build_query_string(None, None), "");
        assert_eq!(
            build_query_string(Some("abc"), None),
            "?cursor=abc"
        );
        assert_eq!(
            build_query_string(None, Some("open")),
            "?status=open"
        );
        assert_eq!(
            build_query_string(Some("abc"), Some("open")),
            "?cursor=abc&status=open"
        );
    }
}
