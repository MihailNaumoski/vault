use reqwest::Client;
use tracing::debug;

use crate::auth::KalshiAuth;
use crate::error::KalshiError;
use crate::rate_limit::KalshiRateLimiter;
use crate::types::{
    KalshiBalanceResponse, KalshiBookResponse, KalshiConfig, KalshiEventResponse,
    KalshiEventsListResponse, KalshiMarketResponse, KalshiMarketsListResponse,
    KalshiOrderRequest, KalshiOrderResponse, KalshiOrdersListResponse,
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
    ///
    /// Returns the full response including the pagination cursor.
    /// Use [`fetch_all_markets`] for automatic pagination.
    pub async fn fetch_markets(
        &self,
        cursor: Option<&str>,
        status: Option<&str>,
    ) -> Result<KalshiMarketsListResponse, KalshiError> {
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
        req = req.query(&[("limit", "1000")]);

        let headers = self.auth.headers("GET", &format!("/trade-api/v2{}", path))?;
        req = req.headers(headers);

        let resp = self.send_with_retry(req).await?;

        let list: KalshiMarketsListResponse = resp.json().await.map_err(KalshiError::Http)?;
        Ok(list)
    }

    /// Fetch all markets with automatic cursor pagination.
    ///
    /// Pages through up to `max_pages` of results (default 3, i.e. up to 3000
    /// markets at limit=1000 per page). Stops early if no cursor is returned
    /// (meaning we have all results).
    pub async fn fetch_all_markets(
        &self,
        status: Option<&str>,
        max_pages: usize,
    ) -> Result<Vec<KalshiMarketResponse>, KalshiError> {
        let max_pages = if max_pages == 0 { 3 } else { max_pages };
        let mut all_markets = Vec::new();
        let mut cursor: Option<String> = None;

        for page in 0..max_pages {
            let response = self
                .fetch_markets(cursor.as_deref(), status)
                .await?;
            let count = response.markets.len();
            all_markets.extend(response.markets);

            debug!(page = page + 1, fetched = count, total = all_markets.len(), "Kalshi pagination");

            match response.cursor {
                Some(c) if !c.is_empty() => cursor = Some(c),
                _ => break, // No more pages
            }
        }

        Ok(all_markets)
    }

    /// Fetch a single market by ticker.
    pub async fn fetch_market(&self, ticker: &str) -> Result<KalshiMarketResponse, KalshiError> {
        let path = format!("/markets/{}", ticker);
        self.rate_limiter.acquire_read().await;

        let full_url = format!("{}{}", self.base_url, path);
        let api_path = format!("/trade-api/v2{}", path);
        let headers = self.auth.headers("GET", &api_path)?;

        let req = self.http.get(&full_url).headers(headers);
        let resp = self.send_with_retry(req).await?;

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

        let req = self.http.get(&full_url).headers(headers);
        let resp = self.send_with_retry(req).await?;

        #[derive(serde::Deserialize)]
        struct Wrapper {
            orderbook: KalshiBookResponse,
        }
        let wrapper: Wrapper = resp.json().await.map_err(KalshiError::Http)?;
        Ok(wrapper.orderbook)
    }

    // -----------------------------------------------------------------------
    // Events endpoints (100 req/s rate limit — market data)
    // -----------------------------------------------------------------------

    /// List events, optionally filtered by cursor and status.
    ///
    /// When `with_nested_markets` is true, each event includes its sub-markets.
    /// Returns the full response including the pagination cursor.
    pub async fn fetch_events(
        &self,
        cursor: Option<&str>,
        status: Option<&str>,
        with_nested_markets: bool,
    ) -> Result<KalshiEventsListResponse, KalshiError> {
        let path = "/events";
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
        if with_nested_markets {
            req = req.query(&[("with_nested_markets", "true")]);
        }

        let headers = self.auth.headers("GET", &format!("/trade-api/v2{}", path))?;
        req = req.headers(headers);

        let resp = self.send_with_retry(req).await?;

        let list: KalshiEventsListResponse = resp.json().await.map_err(KalshiError::Http)?;
        Ok(list)
    }

    /// Fetch all events with automatic cursor pagination.
    ///
    /// Pages through up to `max_pages` of results (default 5, i.e. up to 1000
    /// events at limit=200 per page). Stops early if no cursor is returned.
    /// When `with_nested_markets` is true, each event includes its sub-markets.
    pub async fn fetch_all_events(
        &self,
        status: Option<&str>,
        max_pages: usize,
        with_nested_markets: bool,
    ) -> Result<Vec<KalshiEventResponse>, KalshiError> {
        let max_pages = if max_pages == 0 { 5 } else { max_pages };
        let mut all_events = Vec::new();
        let mut cursor: Option<String> = None;

        for page in 0..max_pages {
            let response = self
                .fetch_events(cursor.as_deref(), status, with_nested_markets)
                .await?;
            let count = response.events.len();
            all_events.extend(response.events);

            debug!(page = page + 1, fetched = count, total = all_events.len(), "Kalshi events pagination");

            match response.cursor {
                Some(c) if !c.is_empty() => cursor = Some(c),
                _ => break, // No more pages
            }
        }

        Ok(all_events)
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

        let request = self.http.post(&full_url).headers(headers).json(req);
        let resp = self.send_with_retry(request).await?;

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

        let req = self.http.delete(&full_url).headers(headers);
        self.send_with_retry(req).await?;

        Ok(())
    }

    /// List all open orders (single request, addresses batching gap G5).
    pub async fn fetch_open_orders(&self) -> Result<Vec<KalshiOrderResponse>, KalshiError> {
        let path = "/portfolio/orders";
        self.rate_limiter.acquire_read().await;

        let full_url = format!("{}{}", self.base_url, path);
        let api_path = format!("/trade-api/v2{}", path);
        let headers = self.auth.headers("GET", &api_path)?;

        let req = self.http.get(&full_url).headers(headers);
        let resp = self.send_with_retry(req).await?;

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

        let req = self.http.get(&full_url).headers(headers);
        let resp = self.send_with_retry(req).await?;

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

        let req = self.http.get(&full_url).headers(headers);
        let resp = self.send_with_retry(req).await?;

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

        let req = self.http.get(&full_url).headers(headers);
        let resp = self.send_with_retry(req).await?;

        let balance: KalshiBalanceResponse = resp.json().await.map_err(KalshiError::Http)?;
        Ok(balance)
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /// Send a request with automatic 429 retry logic.
    ///
    /// If the server returns HTTP 429, reads the `Retry-After` header
    /// (defaulting to 1 second) and retries up to 3 times.
    async fn send_with_retry(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, KalshiError> {
        // We need to clone the request for retries. RequestBuilder doesn't
        // support Clone, so we build it once. For the retry path we
        // reconstruct from the try_clone on the built Request.
        let built = request.build().map_err(KalshiError::Http)?;

        let mut attempts = 0u32;
        let max_retries = 3u32;
        let mut current_request = built;

        loop {
            // Clone for potential retry (try_clone returns None for streaming bodies)
            let retry_req = current_request.try_clone();

            let resp = self
                .http
                .execute(current_request)
                .await
                .map_err(KalshiError::Http)?;
            self.log_rate_limit_headers(&resp);

            if resp.status() == 429 {
                attempts += 1;
                if attempts > max_retries {
                    return Err(KalshiError::RateLimited);
                }

                // Read Retry-After header (seconds), default to 1s
                let wait_secs: u64 = resp
                    .headers()
                    .get("Retry-After")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(1);

                debug!(
                    attempt = attempts,
                    wait_secs,
                    "Kalshi 429 rate limited, retrying"
                );
                tokio::time::sleep(std::time::Duration::from_secs(wait_secs)).await;

                current_request = retry_req.ok_or(KalshiError::RateLimited)?;
                continue;
            }

            if !resp.status().is_success() {
                let status = resp.status().as_u16();
                let body = resp.text().await.unwrap_or_default();
                return Err(KalshiError::Api {
                    status,
                    message: body,
                });
            }

            return Ok(resp);
        }
    }

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
