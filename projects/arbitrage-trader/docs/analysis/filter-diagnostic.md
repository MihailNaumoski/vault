# Filter Diagnostic: Zero Match Root Cause Analysis

**Date**: 2026-04-09
**Context**: `cargo run -- --match` returns 0 matches. 200 raw Polymarket markets -> 37 after filtering -> 0 categorized as Crypto. Kalshi has 37 Crypto markets. No category overlap between platforms.

---

## Problem 1: All Polymarket Crypto Markets Filtered Out

### Filter pipeline (main.rs lines 221-265)

Applied sequentially to each raw Gamma API market:

1. **Outcome prices parse gate**: `prices.len() < 2` -> skip
2. **Price filter**: `poly_yes < 0.10 || poly_yes > 0.90` -> skip
3. **Empty condition_id gate**: skip if blank
4. **Volume filter**: `market_volume < 10,000` -> skip (uses `volume24hr` with `volume` fallback)
5. **Close-time filter**: `hours_to_close < 6` -> skip

### Root cause: Price filter eliminates crypto markets

Crypto prediction markets on Polymarket are typically structured as **price-range brackets**:
- "Will BTC be above $90,000 on April 15?" -> yes_price ~0.95 (very likely) -> **filtered at >0.90**
- "Will BTC be below $50,000 on April 15?" -> yes_price ~0.03 (very unlikely) -> **filtered at <0.10**
- "Will BTC be between $85,000-$90,000 on April 15?" -> yes_price ~0.04 -> **filtered at <0.10**

The [0.10, 0.90] price filter is designed to exclude "already-decided" markets to avoid illiquid/uninteresting markets. But crypto price brackets are **inherently skewed** -- only the 1-2 brackets closest to the current price have "interesting" probabilities, and those often sit right at the 0.08-0.12 or 0.88-0.93 boundary.

### Secondary factor: Volume field interpretation

The Gamma API `volume24hr` field may return 0 for markets where trading has recently moved to a new bracket. The code falls back to `volume` (total lifetime volume), which should mitigate this. However, newly created bracket markets may genuinely have low 24h volume even if the parent event is heavily traded.

### Recommendation

1. **Widen price filter from [0.10, 0.90] to [0.05, 0.95]** -- This captures crypto brackets that are plausible but skewed. Markets at 0.05-0.10 and 0.90-0.95 still have meaningful probability and active trading.
2. **Lower volume threshold from $10,000 to $1,000** -- Crypto brackets fragment volume across many sub-markets. The parent event may have millions in volume, but each bracket gets a fraction.
3. **Consider category-specific thresholds** (future enhancement) -- Crypto markets inherently have more extreme probabilities than politics/sports. A per-category price filter would be ideal but adds complexity.

---

## Problem 2: Kalshi Sports Miscategorized as "Other"

### Current Sports keywords in category.rs

```
"super bowl", "nba", "nfl", "mlb", "nhl", "ufc",
"championship", "tournament", "playoff", "finals",
"league", "team", "coach", "player", "season", "cup",
"olympic", "lakers", "yankees", "warriors"
```

### Kalshi sports market format

Kalshi sports titles use betting-line format:
- `"yes Miami,yes Over 239.5 points scored"`
- `"yes Lakers,yes Under 215.5 points scored"`

These contain:
- **City/team names** (Miami, Lakers) -- only 3 teams in our keywords (lakers, yankees, warriors)
- **Betting terms** (over, under, points, scored, spread, moneyline, total) -- NONE in Sports keywords
- **No league acronyms** (no "nba", "nfl" etc.) in the market title itself

### Critical issue: "over" and "under" are STOP WORDS

In `normalize.rs` line 28-29, both `"over"` and `"under"` are listed as stop words. They get stripped before category classification even sees them. So even if we add them as Sports keywords, they'd be removed first.

### Recommendation

1. **Remove "over" and "under" from the stop words list** -- In prediction markets, these carry strong semantic meaning (over/under lines in sports, price thresholds in crypto). They are not function words in this domain.
2. **Add sports betting keywords to category.rs**:
   - Betting terms: `"points", "scored", "spread", "moneyline", "total", "goals", "runs", "touchdown", "assists", "rebounds"`
   - Game terms: `"game", "match", "win", "loss", "score"`
   - Additional team names and cities commonly seen on Kalshi:
     - NBA: `"heat", "celtics", "knicks", "nuggets", "cavaliers", "thunder", "pacers", "rockets", "nets", "bucks", "suns", "76ers", "sixers", "grizzlies", "mavericks", "timberwolves", "pelicans", "hawks", "pistons", "wizards", "hornets", "magic", "raptors", "spurs", "kings", "blazers", "clippers", "bulls", "jazz", "pacers"`
     - NFL: `"chiefs", "eagles", "cowboys", "dolphins", "bills", "ravens", "lions", "bengals", "browns", "steelers", "titans", "jaguars", "texans", "colts", "chargers", "raiders", "broncos", "jets", "patriots", "saints", "falcons", "panthers", "buccaneers", "rams", "49ers", "seahawks", "cardinals", "commanders", "giants", "bears", "packers", "vikings"`
     - MLB: `"dodgers", "astros", "braves", "padres", "mets", "phillies", "cubs", "cardinals", "reds", "brewers", "pirates", "marlins", "nationals", "rockies", "diamondbacks", "guardians", "twins", "royals", "white sox", "tigers", "orioles", "rays", "blue jays", "red sox", "angels", "athletics", "mariners", "rangers"`
   - Note: Adding ALL teams would be large. A pragmatic approach is to add the top 20 most-traded teams plus the betting terms. The betting terms alone (points, scored, spread, etc.) would catch most Kalshi sports markets.

---

## Problem 3: Malformed Kalshi Markets

### Examples
- `"1+1 = 3"` -- mathematical tautology/novelty
- `"1+1 = 2"` -- mathematical tautology/novelty
- `"yes Miami,yes Over 239.5 points scored"` -- multi-leg structured format (not malformed, just different)

### Analysis

The "1+1 = X" markets are Kalshi novelty/test markets. They will never match anything on Polymarket and waste matcher comparisons (each adds O(n) comparisons in the pipeline).

The "yes City,yes Over/Under X.X points scored" format is Kalshi's standard sports betting title format. These ARE real markets but need proper category classification (solved by Problem 2 fix).

### Recommendation

1. **Add a minimum title length filter for Kalshi markets**: Skip markets with `title.len() < 15` characters. Real prediction market questions are almost always longer than 15 characters.
2. **Add a "gibberish detector"**: Skip Kalshi markets whose title starts with `"yes "` (comma-separated multi-outcome format) -- OR better, parse them to extract the meaningful question. For now, the simpler approach is fine.
3. **Do NOT skip "yes X,yes Y" format**: These are real sports markets. The category fix from Problem 2 will handle them correctly. The matcher should see them.

---

## Summary of Recommended Changes

| File | Change | Priority |
|------|--------|----------|
| `main.rs` (both filter paths) | Widen price filter: `[0.05, 0.95]` | HIGH |
| `main.rs` (both filter paths) | Lower volume threshold: `$1,000` | HIGH |
| `normalize.rs` | Remove "over" and "under" from STOP_WORDS | HIGH |
| `category.rs` | Add sports betting keywords: "points", "scored", "spread", "moneyline", "total", "goals", "game", "match", "win" | HIGH |
| `category.rs` | Add common team names: heat, celtics, knicks, nuggets, cavaliers, thunder, dolphins, chiefs, eagles, cowboys, etc. | MEDIUM |
| `main.rs` | Add Kalshi title length filter (skip < 15 chars) | LOW |

### Expected impact

- Widening price filter: ~20-40 more Polymarket markets should survive, including crypto brackets
- Lowering volume threshold: ~10-20 more markets, especially newly created brackets
- Sports keyword fix: Kalshi sports markets will correctly categorize, enabling Sports-Sports matching
- Combined: We should see non-zero matches, particularly in Crypto and Sports categories

---

## Post-Implementation Results (2026-04-09)

All changes above were implemented. Results of `cargo run -- --match`:

### Filter improvements
- Price filter [0.05, 0.95]: 134 markets filtered (from 200 raw) -- crypto markets still too extreme (0.0005-0.0025 or 0.995-0.9995)
- Volume filter $1,000: 0 filtered (all surviving markets have sufficient volume)
- Close-time filter: 13 filtered
- Net: 53 Poly markets survive (up from 37 with old filters)

### Category classification improvements
- Sports: 18 Poly x 129 Kalshi (was 16 x 63) -- huge improvement from player prop keywords
- Other: 23 Poly x 2 Kalshi (was 25 x 68) -- almost all Kalshi player props correctly classified
- Kalshi title filter (< 15 chars): removed 2 novelty markets ("1+1 = 3", "1+1 = 2")

### Why still 0 matches
The platforms trade **fundamentally different event types** right now:
- **Polymarket Sports**: Championship outcomes (Masters golf, Champions League, NBA Finals, FIFA World Cup)
- **Kalshi Sports**: Player props (Nick Kurtz home runs, Max Meyer strikeouts), game totals, esports
- **Polymarket Other**: Geopolitics (Iran/US, crude oil, Elon tweets)
- **Kalshi Crypto**: Individual coin price brackets (same structure as Poly's, but Poly's are all too extreme to survive price filter)

The matcher infrastructure is now **correct** -- it classifies properly and would find matches if they existed. The zero-match result reflects genuine lack of overlapping markets between the two platforms' top-200 lists, not a bug in the system.

### Path forward
1. **Increase Gamma API limit** from 200 to 500+ to capture more diverse Poly markets
2. **Fetch Kalshi markets by category** (e.g., event_group filter) to target categories where Poly has markets
3. **Add Polymarket event-level fetching** -- fetch all sub-markets under heavily-traded events to find crypto brackets with viable prices
4. **Run the matcher during active sports seasons** when both platforms are more likely to have championship/outcome markets
