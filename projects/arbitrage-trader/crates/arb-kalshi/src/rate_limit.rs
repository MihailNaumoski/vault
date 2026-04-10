use governor::{Quota, RateLimiter};
use std::num::NonZeroU32;

/// Type alias for the governor direct rate limiter.
type DirectLimiter = RateLimiter<
    governor::state::direct::NotKeyed,
    governor::state::InMemoryState,
    governor::clock::DefaultClock,
>;

/// Dual rate limiter for Kalshi API endpoints.
///
/// Kalshi enforces two separate rate limits (Basic tier defaults):
/// - **Write** endpoints (POST/DELETE to order endpoints): 10 requests/second
/// - **Read** endpoints (everything else): 20 requests/second
///
/// This struct holds one governor rate limiter for each tier.
pub struct KalshiRateLimiter {
    write: DirectLimiter,
    read: DirectLimiter,
}

impl KalshiRateLimiter {
    /// Create a new dual rate limiter with Kalshi's Basic tier limits.
    pub fn new() -> Self {
        Self::with_limits(20, 10)
    }

    /// Create a dual rate limiter with custom per-second limits.
    ///
    /// Useful for higher-tier Kalshi API plans:
    /// - Basic: 20 read/s, 10 write/s (default)
    /// - Pro:   100 read/s, 50 write/s
    pub fn with_limits(read_per_sec: u32, write_per_sec: u32) -> Self {
        let write_quota = Quota::per_second(NonZeroU32::new(write_per_sec).unwrap());
        let read_quota = Quota::per_second(NonZeroU32::new(read_per_sec).unwrap());
        Self {
            write: RateLimiter::direct(write_quota),
            read: RateLimiter::direct(read_quota),
        }
    }

    /// Wait for the write rate limiter (10 req/s).
    ///
    /// Used for: POST /portfolio/orders, DELETE /portfolio/orders/{id}.
    pub async fn acquire_write(&self) {
        self.write.until_ready().await;
    }

    /// Wait for the read rate limiter (20 req/s).
    ///
    /// Used for: GET /markets/*, GET /portfolio/orders, GET /portfolio/positions,
    /// GET /portfolio/balance, and all other read endpoints.
    pub async fn acquire_read(&self) {
        self.read.until_ready().await;
    }

    /// Classify a (method, path) pair and acquire the correct rate limiter.
    ///
    /// Returns `true` if it used the write limiter, `false` for read.
    pub async fn acquire_for_endpoint(&self, method: &str, path: &str) -> bool {
        if Self::is_write_endpoint(method, path) {
            self.acquire_write().await;
            true
        } else {
            self.acquire_read().await;
            false
        }
    }

    /// Determine whether a (method, path) pair is a write endpoint (10 req/s)
    /// or a read endpoint (20 req/s).
    ///
    /// Write endpoints: POST or DELETE to paths containing /portfolio/orders.
    /// Everything else (including GET /portfolio/*) is read-limited.
    pub fn is_write_endpoint(method: &str, path: &str) -> bool {
        let m = method.to_uppercase();
        (m == "POST" || m == "DELETE") && path.contains("/portfolio/orders")
    }
}

impl Default for KalshiRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_endpoint_classification_write() {
        // POST/DELETE to order endpoints are write-limited
        assert!(KalshiRateLimiter::is_write_endpoint(
            "POST",
            "/trade-api/v2/portfolio/orders"
        ));
        assert!(KalshiRateLimiter::is_write_endpoint(
            "DELETE",
            "/trade-api/v2/portfolio/orders/abc-123"
        ));
    }

    #[test]
    fn test_endpoint_classification_read_portfolio() {
        // GET to portfolio endpoints are read-limited (not write)
        assert!(!KalshiRateLimiter::is_write_endpoint(
            "GET",
            "/trade-api/v2/portfolio/orders"
        ));
        assert!(!KalshiRateLimiter::is_write_endpoint(
            "GET",
            "/trade-api/v2/portfolio/orders/abc-123"
        ));
        assert!(!KalshiRateLimiter::is_write_endpoint(
            "GET",
            "/trade-api/v2/portfolio/positions"
        ));
        assert!(!KalshiRateLimiter::is_write_endpoint(
            "GET",
            "/trade-api/v2/portfolio/balance"
        ));
    }

    #[test]
    fn test_endpoint_classification_read_market_data() {
        // Market data endpoints are read-limited
        assert!(!KalshiRateLimiter::is_write_endpoint(
            "GET",
            "/trade-api/v2/markets"
        ));
        assert!(!KalshiRateLimiter::is_write_endpoint(
            "GET",
            "/trade-api/v2/markets/PRES-2026-DEM"
        ));
        assert!(!KalshiRateLimiter::is_write_endpoint(
            "GET",
            "/trade-api/v2/markets/PRES-2026-DEM/orderbook"
        ));
    }

    #[tokio::test]
    async fn test_acquire_for_endpoint_routes_correctly() {
        let limiter = KalshiRateLimiter::new();

        // POST to orders should use write limiter
        let used_write = limiter
            .acquire_for_endpoint("POST", "/trade-api/v2/portfolio/orders")
            .await;
        assert!(used_write);

        // GET to orders should use read limiter
        let used_write = limiter
            .acquire_for_endpoint("GET", "/trade-api/v2/portfolio/orders")
            .await;
        assert!(!used_write);

        // GET to markets should use read limiter
        let used_write = limiter
            .acquire_for_endpoint("GET", "/trade-api/v2/markets/PRES-2026-DEM")
            .await;
        assert!(!used_write);
    }

    #[tokio::test]
    async fn test_dual_limiter_independent_budgets() {
        let limiter = KalshiRateLimiter::new();

        // We should be able to acquire from both limiters without blocking
        // (they have independent budgets)
        limiter.acquire_write().await;
        limiter.acquire_read().await;

        // Quick burst of write calls (within the 10/s budget)
        for _ in 0..5 {
            limiter.acquire_write().await;
        }

        // Read calls should not be affected by write usage
        for _ in 0..5 {
            limiter.acquire_read().await;
        }
    }
}
