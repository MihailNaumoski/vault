# API Pagination & Rate Limit Improvements

**Date**: 2026-04-09
**Status**: Implemented

---

## Problem Statement

The market discovery pipeline was only fetching a small fraction of available markets from both platforms:
- Polymarket: 200 markets (1 page) out of 1000+ available
- Kalshi: 200 markets (1 request, arbitrary limit) out of 3000+ available

With only ~7,400 cross-platform comparisons (37 filtered Poly × 200 Kalshi), the system had almost no chance of finding overlapping events. Additionally, there was no retry logic for rate limit errors — a single 429 response would cause a hard failure.

---

## Research Findings

### Polymarket Gamma API
| Item | Value |
|------|-------|
| Rate limit | 30 req/s (300 req/10s) |
| Pagination | Offset-based (`limit` + `offset` params) |
| Max per request | No hard max documented |
| WS limits | PING every 10s, no subscription cap |

### Kalshi REST API
| Item | Value |
|------|-------|
| Rate limit (Basic tier) | 20 read/s, 10 write/s |
| Rate limit (Advanced) | 30/30 |
| Rate limit (Premier) | 100/100 |
| Rate limit (Prime) | 400/400 |
| Pagination | Cursor-based (`cursor` field in response) |
| Max per request | 1000 (`limit` param, range 0-1000) |
| WS limits | Server pings every 10s, no subscription cap |

---

## Decisions Made

### 1. Kalshi Fetch Limit: 200 → 1000 per page

**What**: Increased the `limit` query parameter from 200 to 1000 in `fetch_markets()`.

**Why**: The Kalshi API supports up to 1000 results per request. We were leaving 80% of available markets on the table with `limit=200`. A single request with `limit=1000` uses the same rate limit budget (1 request) but returns 5x more data.

### 2. Kalshi Cursor Pagination: 3 pages max (up to 3000 markets)

**What**: Added `fetch_all_markets()` method that loops through pages using the cursor returned by the API. Safety cap of 3 pages (3000 markets max).

**Why**: Kalshi has 3000+ open markets. A single page of 1000 misses two-thirds. Cursor pagination is the API's intended mechanism for large result sets. The 3-page cap prevents infinite loops while capturing virtually all open markets.

**Rate impact**: 3 GET requests at 20 req/s budget = negligible (0.15 seconds of budget).

### 3. Polymarket Gamma Pagination: 3 pages × 200 (up to 600 markets)

**What**: Added offset-based pagination to the Gamma API fetch in both `--match` mode and the engine startup discovery path. Fetches pages at offset 0, 200, 400 with 100ms delay between pages.

**Why**: The Gamma API returns markets sorted by 24h volume descending. The first 200 are the most liquid, but markets 200-600 may include legitimate arbitrage candidates in different categories (crypto, politics) that were being missed entirely.

**Rate impact**: 3 GET requests at 30 req/s budget with 100ms delays = well within limits.

### 4. HTTP 429 Retry Logic (Both Platforms)

**What**: Added `send_with_retry()` helper to both Kalshi and Polymarket clients. On 429 response:
1. Read `Retry-After` header (default 1 second if absent)
2. Wait the specified duration
3. Retry up to 3 times
4. Error out after 3 failures

**Why**: Previously, a single rate limit response caused a hard failure — the entire discovery pipeline would abort. With pagination making multiple requests per platform, retry logic is essential for resilience.

### 5. Configurable Kalshi Rate Limiter

**What**: Added `KalshiRateLimiter::with_limits(read_per_sec, write_per_sec)` constructor. The default `new()` still uses Basic tier (20/10) for backward compatibility.

**Why**: Kalshi has 4 rate limit tiers (Basic through Prime). Users with higher-tier API keys were throttled to Basic limits unnecessarily. The configurable constructor allows matching the limiter to the actual tier.

---

## Impact

### Market coverage: before vs after

| | Before | After | Improvement |
|---|--------|-------|-------------|
| Polymarket raw | 200 (1 page) | 600 (3 pages) | **3x** |
| Polymarket filtered | ~37 | ~155 | **4.2x** |
| Kalshi raw | 200 (1 page) | 3000 (3 pages) | **15x** |
| Kalshi filtered | ~200 | ~2998 | **15x** |
| Cross-platform comparisons | ~7,400 | ~464,690 | **63x** |

### Rate limit budget usage

| Platform | Requests | Budget | Usage |
|----------|----------|--------|-------|
| Polymarket Gamma | 3 | 30/s | 10% of 1-second budget |
| Kalshi | 3 | 20/s | 15% of 1-second budget |

Safe margin on both platforms. Even at Basic tier.

---

## Files Changed

| File | Changes |
|------|---------|
| `crates/arb-kalshi/src/client.rs` | `limit=1000`, cursor pagination (`fetch_all_markets`), 429 retry logic |
| `crates/arb-kalshi/src/connector.rs` | Uses `fetch_all_markets()` with 3-page cap |
| `crates/arb-kalshi/src/rate_limit.rs` | Added `with_limits()` configurable constructor |
| `crates/arb-polymarket/src/client.rs` | 429 retry logic |
| `crates/arb-cli/src/main.rs` | Gamma offset pagination (3 pages), both `--match` and engine startup paths |

---

## Recommended Next Steps

1. Add Kalshi rate limit tier to `config/default.toml` and wire it through to the connector
2. Monitor actual 429 rates in production — if they occur, the retry logic will log warnings
3. If Kalshi market count exceeds 3000, increase the page cap
4. Consider caching market lists with a TTL to avoid re-fetching on every startup
