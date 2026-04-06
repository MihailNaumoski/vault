use governor::{DefaultDirectRateLimiter, Quota, RateLimiter};
use std::num::NonZeroU32;

/// Token-bucket rate limiter for Polymarket API requests (100 req/s).
pub struct PolyRateLimiter {
    limiter: DefaultDirectRateLimiter,
}

impl PolyRateLimiter {
    /// Create a rate limiter for Polymarket (100 requests per second).
    pub fn new() -> Self {
        let quota = Quota::per_second(NonZeroU32::new(100).unwrap());
        Self {
            limiter: RateLimiter::direct(quota),
        }
    }

    /// Create a rate limiter with a custom requests-per-second value.
    pub fn with_rps(rps: u32) -> Self {
        let quota = Quota::per_second(NonZeroU32::new(rps.max(1)).unwrap());
        Self {
            limiter: RateLimiter::direct(quota),
        }
    }

    /// Wait until a request is permitted by the rate limiter.
    pub async fn acquire(&self) {
        self.limiter.until_ready().await;
    }
}

impl Default for PolyRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_rate_limiter_allows_burst() {
        let rl = PolyRateLimiter::new();
        // Should allow a burst of requests without blocking
        for _ in 0..10 {
            rl.acquire().await;
        }
    }

    #[tokio::test]
    async fn test_rate_limiter_with_custom_rps() {
        let rl = PolyRateLimiter::with_rps(50);
        rl.acquire().await;
    }
}
