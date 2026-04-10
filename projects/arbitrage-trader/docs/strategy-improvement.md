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

### 4. Quality-Gated Pair Selection (Score >= 0.65, Hard Cap 20, Text Floor 0.55)

**What**: Replace the arbitrary 12-pair cap with a quality threshold, a hard cap, and a text similarity floor:
- Any matched pair with composite score >= 0.65 is accepted
- Hard cap of 20 pairs maximum (prevents runaway pair counts)
- Minimum text similarity of 0.55 required (prevents time_score from compensating for poor text matches)
- AutoVerified threshold at 0.85 (only high-confidence matches are auto-verified for automated trading)

**Why**: The old system took the top 12 pairs regardless of quality -- a mediocre 12th pair got in while a good 15th was excluded. The matcher's composite score (70% text similarity + 30% close-time proximity) already captures match reliability. Letting quality determine the active set means:
- More pairs when many good matches exist
- Fewer pairs when the market is thin (better than padding with bad matches)

**Why 0.65**: Initially set to 0.82, then revised after real-world testing showed legitimate cross-platform matches scoring 0.694-0.737 due to different question phrasing. See "Threshold Revision" section below for full analysis.

**Why cap at 20**: Even with a quality gate, unbounded pair counts create operational risk. 20 is well above the typical 8-15 quality matches found in practice, so it only activates as a safety valve.

**Why AutoVerified at 0.85**: Matches scoring 0.65-0.84 enter the system as NeedsReview, requiring human confirmation before automated trading begins. Only near-certain matches (>=0.85) bypass manual review.

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
  - Text similarity >= 0.55
  - Match score >= 0.65
  - Hard cap: 20 pairs max
  - AutoVerified threshold: 0.85
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
| Pair quality gate | 12 (arbitrary cap) | score >= 0.65 (revised from 0.82) | Discovery |
| Pair hard cap | 12 | 20 | Discovery |
| Text similarity floor | none | >= 0.55 (revised from 0.80) | Discovery |
| AutoVerified threshold | 0.85 | 0.85 (revised from 0.90) | Matcher |

### Files Changed

| File | Changes |
|------|---------|
| `crates/arb-engine/src/detector.rs` | Close-time filter (6h), price extreme filter, 7 new tests |
| `crates/arb-cli/src/main.rs` | Volume filter, tighter price range, close-time filter, quality gate 0.65, hard cap 20, text floor 0.55, limit=200, real API --match mode |
| `crates/arb-matcher/src/types.rs` | AutoVerified threshold at 0.85 |
| `crates/arb-kalshi/src/client.rs` | Added `limit=200` query parameter to `fetch_markets()` |
| `config/default.toml` | `min_time_to_close_hours = 6` |

---

## Threshold Revision: Quality Gate Lowered (2026-04-09)

### Problem

Real-world testing revealed that the 0.82 quality gate and 0.80 text similarity floor rejected ALL legitimate cross-platform matches. Polymarket and Kalshi phrase questions very differently, and Jaro-Winkler scores on normalized text are much lower than expected:

| Match | Poly Question | Kalshi Question | Composite | Verdict |
|-------|--------------|-----------------|-----------|---------|
| Bitcoin | "Will Bitcoin hit $100k by December 2025?" | "Bitcoin above $100,000 on December 31, 2025?" | 0.737 | Good match, was REJECTED |
| Ethereum | "Will Ethereum reach $5,000 before 2026?" | "Ethereum price above $5,000 by end of 2025?" | 0.729 | Good match, was REJECTED |
| Fed rate | "Will the Fed cut rates in June 2025?" | "Federal Reserve to cut interest rates at June meeting?" | 0.694 | Good match, was REJECTED |

**Root cause**: The normalizer strips punctuation and lowercases, but cannot unify semantic equivalents like "$100k" vs "$100,000" or "hit" vs "above". Jaro-Winkler is a character-level metric and penalizes these differences heavily. Improving the normalizer (e.g., expanding "$100k" to "100000", synonym mapping) is the long-term fix but requires more development.

### Revised Thresholds

| Parameter | Old | New | Rationale |
|-----------|-----|-----|-----------|
| Quality gate (composite) | 0.82 | **0.65** | All 3 real matches (0.694-0.737) pass. Provides 6.7% margin below worst known good match (Fed rate at 0.694). |
| Text similarity floor | 0.80 | **0.55** | The Fed rate match has text_sim ~0.56 after normalization. Floor must be below worst real case. |
| AutoVerified threshold | 0.90 | **0.85** | With wider NeedsReview band, 0.85 still ensures only high-confidence matches bypass review. |
| NeedsReview floor | 0.50 | **0.50** | Unchanged. Pipeline already filters at 0.50 minimum. |
| Max discovered pairs | 20 | **20** | Unchanged. Safety cap remains. |

**Why 0.65 not 0.70**: The Fed rate match at 0.694 is a clear legitimate match. A 0.70 gate would reject it. Cross-platform phrasing differences are inherently unpredictable -- future legitimate matches may score even lower depending on question wording. 0.65 provides necessary margin.

**Why not lower than 0.65**: Below 0.65, false positive risk increases. The composite already blends text (70%) and time proximity (30%), so 0.65 with a 0.55 text floor still provides meaningful false-positive protection. Matches below 0.55 text similarity are likely genuinely different events.

**False positive mitigation**: NeedsReview (0.50-0.85) pairs still require human confirmation before automated trading. Only AutoVerified (>=0.85) pairs trade automatically. This two-tier system means lowering the gate introduces more candidates for review without increasing automated trading risk.

### Config Location

Thresholds are now configurable via `config/default.toml` under the `[matcher]` section:

```toml
[matcher]
quality_gate = 0.65
text_similarity_floor = 0.55
auto_verified_threshold = 0.85
needs_review_floor = 0.50
max_discovered_pairs = 20
```

### Future Improvements

1. **Normalizer enhancement**: Expand abbreviations ($100k -> 100000), map synonyms (hit/reach/above), strip ordinals (31st -> 31)
2. **Semantic similarity**: Consider embedding-based similarity (e.g., sentence transformers) as an alternative to Jaro-Winkler for cross-platform matching
3. **Adaptive thresholds**: Track confirmed match rates and adjust thresholds based on false positive/negative feedback

---

## Recommended Next Steps

1. **Paper test** with new parameters for 48-72 hours
2. Monitor rejection rates at discovery -- if too aggressive, loosen volume to $5,000
3. Track phantom spread rate (detected opportunities that fail to fill) -- should drop significantly
4. Track fill rates before/after to quantify improvement
5. Add Kalshi volume filtering when their API supports it
6. Validate revised thresholds against a larger sample of real cross-platform matches
