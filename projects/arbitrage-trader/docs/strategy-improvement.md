# Strategy Improvement: Market Selection & Unstarted Match Filtering

**Date**: 2026-04-09
**Status**: Implemented, pending paper test validation

---

## Problem Statement

The arbitrage engine had low profitability due to poor market selection:
1. No volume/liquidity filtering — illiquid markets produce phantom spreads that can't actually be filled
2. No close-time filtering at discovery — near-expiry markets with declining liquidity were being traded
3. Weak price extreme filtering — near-decided markets passed through to the detector
4. Arbitrary 12-pair cap ignored pair quality (a bad 10th pair got in while a good 15th was excluded)
5. No "unstarted match" enforcement — the system would trade events already resolving

---

## Decisions Made

### 1. Minimum Volume Threshold: $10,000 (Polymarket 24h)

**What**: Skip any Polymarket market with < $10,000 in 24h trading volume during discovery.

**Why**: Low-volume markets have wide bid-ask spreads that *look* like arbitrage opportunities but cannot be filled. The apparent spread is phantom — placing an order moves the price, and there's no counterparty at the displayed level. $10,000 is the threshold where order books start having meaningful depth for 50-contract ($25-50) orders.

**Trade-off**: May skip legitimate arbs on newer/niche markets. If >90% of discovered markets get rejected, loosen to $5,000.

**Note**: Kalshi doesn't reliably expose volume via API yet. Once it does, add a Kalshi-side volume filter too.

### 2. Price Range Filters (Two Layers)

**Discovery layer**: Reject markets where `poly_yes < 0.10` or `poly_yes > 0.90`
**Detector layer**: Reject any pair where ANY individual price is `< 0.05` or `> 0.95`

**Why**: Markets with extreme prices are effectively decided — the outcome is near-certain. Any remaining cross-platform spread is phantom because counterparties won't take the losing side at reasonable sizes. Two layers catch this at different granularity:
- Discovery (0.10/0.90) is tighter because we're choosing which markets to *monitor* — be selective
- Detector (0.05/0.95) is looser because prices change in real-time — a market at 0.88 when discovered could drift to 0.93 while being monitored, and we want to stop trading it at that point

### 3. Close-Time Minimum: 6 Hours

**What**: Skip markets closing within 6 hours. Applied at all three layers (discovery, detector, risk manager).

**Why**: This targets the **liquidity cliff** — market maker participation drops sharply in the final 4-6 hours before close as they de-risk their positions. This is specifically about fill reliability, not about whether the event outcome is decided (that's what the price extreme filter handles).

**Why not 72h** (first attempt): 72h was too aggressive. It eliminated ~50% of valid markets. The user's intent ("only unstarted matches") means events whose outcome is undetermined, not events 3+ days from close. A market closing in 20 hours for a tomorrow-morning event is perfectly valid.

**Why not 24h** (original value): 24h is safe but unnecessarily conservative. Markets between 6-24h from close still have good liquidity and fill rates. The 6h threshold keeps ~95% of valid markets eligible.

**Why 6h is sufficient**: The system only needs ~60 seconds to fill both legs (`max_hedge_wait_secs = 60`). Six hours provides ample runway. Other protections (price extremes, book depth, staleness) catch the remaining risks.

| Threshold | Markets available | Risk | Verdict |
|-----------|------------------|------|---------|
| 2h | ~98% | High — inside liquidity cliff | Too risky |
| **6h** | **~95%** | **Low — clear of cliff** | **Selected** |
| 24h | ~80% | Very low | Unnecessarily conservative |
| 72h | ~50% | Minimal | Eliminates too many opportunities |

### 4. Quality-Gated Pair Selection (Score >= 0.82, Hard Cap 20, Text Floor 0.80)

**What**: Replace the arbitrary 12-pair cap with a quality threshold, a hard cap, and a text similarity floor:
- Any matched pair with composite score >= 0.82 is accepted
- Hard cap of 20 pairs maximum (prevents runaway pair counts)
- Minimum text similarity of 0.80 required (prevents time_score from compensating for poor text matches)
- AutoVerified threshold raised from 0.85 to 0.90 (only very high-confidence matches are auto-verified for automated trading)

**Why**: The old system took the top 12 pairs regardless of quality — a mediocre 12th pair got in while a good 15th was excluded. The matcher's composite score (70% text similarity + 30% close-time proximity) already captures match reliability. Letting quality determine the active set means:
- More pairs when many good matches exist
- Fewer pairs when the market is thin (better than padding with bad matches)

**Why 0.82**: Raised from the initial 0.75 after analysis. At 0.75, too many marginal matches slipped through. 0.82 ensures only pairs with strong text and timing signals are accepted. The text similarity floor of 0.80 provides an additional safety net — a pair cannot qualify on time_score alone when text similarity is weak.

**Why cap at 20**: Even with a quality gate, unbounded pair counts create operational risk. 20 is well above the typical 8-15 quality matches found in practice, so it only activates as a safety valve.

**Why AutoVerified at 0.90**: Raising from 0.85 means only near-certain matches bypass manual review. Matches scoring 0.82-0.89 still enter the system as NeedsReview, requiring human confirmation before automated trading begins.

### 5. Increased Fetch Limits: 200 Markets (Both Platforms)

**What**: Fetch 200 markets from both Polymarket Gamma API (was 100) and Kalshi API (was unlimited/default).

**Why**: With the new volume and price filters rejecting more markets, we need a larger initial pool to find enough quality matches. Fetching 200 sorted by `volume24hr` descending means we get the most liquid markets first, then the filters trim from there. The Kalshi side now has an explicit `limit=200` query parameter to bound the API response size, improving latency and preventing unbounded fetches.

---

## Implementation Summary

### Three-Layer Filter Architecture

```
Layer 1: DISCOVERY (main.rs — runs once at startup)
  - Volume >= $10,000
  - Price in [0.10, 0.90]
  - Close-time >= 6h
  - Text similarity >= 0.80
  - Match score >= 0.82
  - Hard cap: 20 pairs max
  - AutoVerified threshold: 0.90
  → Determines which pairs are monitored

Layer 2: DETECTOR (detector.rs — runs every 1s scan)
  - All prices in [0.05, 0.95]
  - Close-time >= 6h
  - Spread > min_spread_absolute + estimated_fees
  - Spread% > min_spread_pct (3%)
  → Determines which opportunities are generated

Layer 3: RISK MANAGER (manager.rs — runs per trade)
  - min_time_to_close_hours = 6
  - Book depth >= 50 contracts
  - Balance sufficient
  - Position/exposure limits
  - Daily loss limit
  → Final gate before order placement
```

### Config Changes

| Parameter | Before | After | Layer |
|-----------|--------|-------|-------|
| `min_time_to_close_hours` | 24 | 6 | Risk manager |
| Gamma API limit | 100 | 200 | Discovery |
| Kalshi API limit | unlimited | 200 | Discovery |
| Price filter (discovery) | [0.05, 0.95] | [0.10, 0.90] | Discovery |
| Price filter (detector) | none | [0.05, 0.95] | Detector |
| Volume filter | none | $10,000 min | Discovery |
| Pair quality gate | 12 (arbitrary cap) | score >= 0.82 | Discovery |
| Pair hard cap | 12 | 20 | Discovery |
| Text similarity floor | none | >= 0.80 | Discovery |
| AutoVerified threshold | 0.85 | 0.90 | Matcher |

### Files Changed

| File | Changes |
|------|---------|
| `crates/arb-engine/src/detector.rs` | Close-time filter (6h), price extreme filter, 7 new tests |
| `crates/arb-cli/src/main.rs` | Volume filter, tighter price range, close-time filter, quality gate 0.82, hard cap 20, text floor 0.80, limit=200 |
| `crates/arb-matcher/src/types.rs` | AutoVerified threshold raised from 0.85 to 0.90 |
| `crates/arb-kalshi/src/client.rs` | Added `limit=200` query parameter to `fetch_markets()` |
| `config/default.toml` | `min_time_to_close_hours = 6` |

---

## Recommended Next Steps

1. **Paper test** with new parameters for 48-72 hours
2. Monitor rejection rates at discovery — if too aggressive, loosen volume to $5,000
3. Track phantom spread rate (detected opportunities that fail to fill) — should drop significantly
4. Track fill rates before/after to quantify improvement
5. Add Kalshi volume filtering when their API supports it
