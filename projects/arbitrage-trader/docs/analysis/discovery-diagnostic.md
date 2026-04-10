# Discovery Diagnostic: Why Are There Zero Matches?

**Run ID:** `019d7296-408b-7fc2-9bea-9f97af6b05a9`
**Date:** 2026-04-09
**Markets:** 151 Polymarket x 2,998 Kalshi (filtered-in)
**Raw:** 600 Polymarket / 3,000 Kalshi
**Matches Found:** 0

## Executive Summary

There are zero matches because **Polymarket and Kalshi do not currently offer the same markets in the same format**. The two platforms serve fundamentally different market structures:

- **Polymarket**: Binary outcome markets ("Will X happen?", "Team A vs Team B")
- **Kalshi**: Primarily bracket/prop markets ("Over/under 239.5 points", "Shiba Inu price range", "Temperature above 36.99 degrees")

Even when both platforms cover the **same event** (e.g., Lakers vs Warriors NBA game), they offer **different bet types** that cannot be arbitraged against each other.

## Match Funnel Breakdown

| Stage | Count | % of Total |
|-------|------:|----------:|
| Total cross-product (151 x 2998) | 452,698 | 100.0% |
| Blocked by category | 63,964 | 14.1% |
| Blocked by entity (no shared entities) | 38,675 | 8.5% |
| Blocked by token count (< 2 shared tokens) | 347,471 | 76.8% |
| Scored (passed all gates) | 2,588 | 0.6% |
| Above 0.55 threshold | 0 | 0.0% |

**The dominant blocker is token count (76.8%)** -- most pairs share fewer than 2 meaningful tokens because the platforms use completely different vocabulary and question structures.

## Category Distribution

| Category | Polymarket | Kalshi | Overlap? |
|----------|----------:|-------:|----------|
| Sports | 48 | 2,433 | Yes, but different bet types |
| Other | 68 | 224 | Minimal real overlap |
| Politics | 24 | 13 | Only 1 unique Kalshi politics question |
| Crypto | 9 | 58 | Different coins entirely |
| Weather | 0 | 270 | Kalshi only |
| Economics | 1 | 0 | Polymarket only |
| Science | 1 | 0 | Polymarket only |

### Key Category Findings

**Sports (biggest potential overlap, 48 x 2,433):**
- Polymarket has: moneyline/winner markets (e.g., "Lakers vs. Warriors", "Cincinnati Reds vs. Miami Marlins")
- Kalshi has: spreads, props, totals (e.g., "Los Angeles L vs Golden State: First Half Total?", "Cincinnati vs Miami Total Runs?")
- **Critical**: Polymarket uses team nicknames ("Lakers"), Kalshi uses city abbreviations ("Los Angeles L"). The entity dictionary doesn't map these, so they fail the entity/token gates.
- Even if naming matched, these are different bet types and cannot be directly arbitraged.

**Crypto (9 x 58):**
- Polymarket has: Bitcoin and Ethereum price threshold markets ("Will Bitcoin reach $80,000 in April?")
- Kalshi has: ONLY Shiba Inu price range bracket markets (58 variants of "Shiba Inu price range on Apr 10, 2026?")
- Zero entity overlap (Bitcoin/Ethereum vs Shiba Inu) -- ALL 522 Crypto x Crypto pairs are entity-blocked

**Politics (24 x 13):**
- Polymarket has: election outcomes, geopolitical events
- Kalshi has: only 1 unique politics question (a Mark Carney speech prediction with 13 bracket variants)
- No real overlap

## Near-Misses (Top Scored Pairs)

The highest-scoring pair achieved only 0.511 composite (threshold is 0.55):

| Score | Poly Question | Kalshi Question | Issue |
|------:|--------------|----------------|-------|
| 0.511 | Cincinnati Reds vs. Miami Marlins | Cincinnati vs Miami Total Runs? | Different bet type (winner vs total) |
| 0.494 | Detroit Tigers vs. Minnesota Twins | Detroit vs Minnesota Total Runs? | Different bet type |
| 0.448 | Cincinnati Reds vs. Miami Marlins | Cincinnati vs Miami first 5 innings runs? | Different bet type |
| 0.444 | Detroit Tigers vs. Minnesota Twins | Hanshin Tigers vs Chunichi Dragons winner? | Different teams (false positive) |
| 0.438 | Athletics vs. New York Yankees | New York Y vs Tampa Bay Winner? | Different matchup (A's != Tampa Bay) |

None of these are true arbitrage opportunities -- they are either different bet types on the same event, or entirely different events that happen to share some words.

## Root Causes

### 1. No True Market Overlap (Primary Cause)
The platforms simply don't offer equivalent markets right now. Polymarket focuses on longer-term binary outcomes while Kalshi focuses on daily bracket/prop markets.

### 2. Naming Convention Mismatch
Even where events overlap, naming differs:
- "Lakers" vs "Los Angeles L"
- "Athletics" vs "A's"
- "Cincinnati Reds" vs "Cincinnati"
- "New York Yankees" vs "New York Y"

The entity dictionary doesn't contain sports team nickname-to-city mappings.

### 3. Kalshi Market Structure
Kalshi's markets are overwhelmingly bracket-style: a single event generates 10-30 separate markets with different thresholds. This inflates the raw count (2,998 markets) but provides very few unique events (~200 truly distinct topics).

### 4. Polymarket Market Coverage
Polymarket's filtered set (151 markets) is small, skewing toward politics, crypto, and specific sports events (Champions League, FIFA World Cup, Masters golf). These specific events do not appear on Kalshi at all.

## Recommendations

1. **Short-term**: The matcher is working correctly. The absence of matches reflects genuine market structure differences, not a matcher bug.

2. **Market overlap monitoring**: Track category overlap over time. Crypto is the most likely convergence point if Kalshi adds Bitcoin/Ethereum markets or Polymarket adds Shiba Inu.

3. **Sports name normalization**: If sports matching becomes viable, add team nickname-to-city mappings to the entity dictionary (e.g., "Lakers" -> "Los Angeles Lakers", "Los Angeles L" -> "Los Angeles Lakers").

4. **Expand Polymarket fetch**: Currently fetching only 600 markets (3 pages x 200). Increasing pagination might find more niche markets that overlap with Kalshi.

5. **Consider semantic matching**: For sports, a higher-level "same event" detector that groups different bet types under one event could identify partial arbitrage opportunities across bet types (though these require more complex modeling than simple price comparison).
