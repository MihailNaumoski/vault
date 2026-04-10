# First Match Analysis: Discovery Run 2026-04-09

Run ID: `019d737f-04ae-7cd0-90a3-90aff82b16b6`  
Timestamp: 2026-04-09T18:26:37 UTC  
Markets: 132 Polymarket (filtered in) x 3,995 Kalshi (filtered in)

---

## 1. The Match: Greenland Acquisition

### Match Details

| Field | Polymarket | Kalshi |
|-------|-----------|--------|
| **Question** | Will the US acquire part of Greenland in 2026? | Will the US acquire any part of Greenland? |
| **Yes Price** | $0.195 | $0.26 |
| **No Price** | $0.805 | $0.66 |
| **Volume** | $62,800 | $957,952 |
| **Close Time** | 2026-12-31T00:00:00Z | 2027-01-01T15:00:00Z |
| **Platform ID** | 0x890fc3...b4ce | KXGREENTERRITORY-29-27 |

### Score Breakdown

| Component | Value | Weight |
|-----------|-------|--------|
| Text Score | 0.888 | 65% |
| Time Score | 0.768 | 25% |
| Jaro-Winkler | (tiebreaker) | 10% |
| **Composite** | **0.862** | |
| Shared Entities | 1 ("united states") | |
| Shared Tokens | 4 | |

### Is There a Spread?

**Yes, but inverted.** Polymarket YES = $0.195, Kalshi YES = $0.26.

However, the questions are slightly different:
- Polymarket: "acquire part of Greenland **in 2026**" (time-bounded)
- Kalshi: "acquire **any part** of Greenland" (close time Jan 1, 2027 -- effectively the same)

The 6.5-cent spread on YES prices ($0.195 vs $0.26) reflects Kalshi pricing higher probability. But the close times differ by only ~39 hours (within the 168-hour MAX_TIME_DIFF_HOURS window), so these are essentially the same market.

**Tradeability Assessment:** The spread is only 6.5 cents on YES. After fees (~2% per side), the net spread would be ~2.5 cents. With Kalshi volume at $958K, there's real liquidity. Polymarket volume at $63K is thinner but tradeable. **Marginal but tradeable on the YES side.** Would need to check live order books for actual available size.

### Related Greenland Markets NOT Matched

| Polymarket | Kalshi | Why Not Matched |
|-----------|--------|----------------|
| "Will Trump acquire Greenland before 2027?" ($140K vol) | "Will the US acquire any part of Greenland?" ($958K vol) | **Entity gate blocked** -- Poly has entity "trump", Kalshi has entity "united states", zero overlap |
| "Will Trump acquire Greenland before 2027?" ($140K vol) | "Will Trump buy Greenland?" ($38 vol) | Scored 0.396 -- time_score=0 because close times differ (Dec 31 2026 vs Jul 1 2026 = ~6 months apart) |

**Critical Finding:** "Will Trump acquire Greenland before 2027?" vs "Will the US acquire any part of Greenland?" is a legitimate match that humans would pair, but the entity gate kills it because "trump" != "united states". This is a **false negative caused by the entity gate being too strict**.

---

## 2. Near-Misses: Top Scored Pairs Below Threshold

The match threshold is `min_score = 0.55`. Only 1 pair crossed it. Here are the top near-misses:

| # | Poly Question | Kalshi Question | Composite | Text | Time | Issue |
|---|--------------|-----------------|-----------|------|------|-------|
| 1 | Detroit Tigers vs. Minnesota Twins | Detroit vs Minnesota Total Runs? | 0.494 | 0.429 | 0.512 | Sports: same game, different bet type (winner vs totals) |
| 2 | Chicago White Sox vs. Kansas City Royals | Chicago WS vs Kansas City Total Runs? | 0.488 | 0.417 | 0.512 | Same issue -- winner vs totals |
| 3 | Trump out as President before 2027? | Who will leave their role in the Trump Admin before 2027? | 0.484 | 0.270 | 0.970 | **Near-miss legitimate match** -- high time score, low text score due to different phrasing |
| 4 | Arizona Diamondbacks vs. New York Mets | Arizona vs New York M Total Runs? | 0.472 | 0.397 | 0.512 | Sports: same game, different bet type |
| 5 | Will Marco Rubio win the 2028 Republican presidential nomination? | Will a Trump family member be the 2028 Republican presidential nominee? | 0.465 | 0.235 | 0.911 | Different candidates, correctly not matched |
| 6 | Will Naftali Bennett be the next Prime Minister of Israel? | Who will be the next new Prime Minister of Israel? | 0.460 | 0.589 | **0.0** | **FALSE NEGATIVE** -- close times differ catastrophically (2026-12-31 vs 2045-01-01) |
| 7 | Keiko Fujimori first round of 2026 Peruvian election | Peru presidential election: first round second place? | 0.441 | 0.581 | 0.0 | Different bet types (winner vs second place) |
| 8 | Hungary PM Fidesz-KDNP seats | Hungary parliamentary election: Fidesz-KDNP number of seats? | 0.391 | 0.391 | 0.0 | **Related but different** -- Poly asks about PM, Kalshi about seat count |

### Key False Negatives Identified

**1. Israel PM (Bennett):** Polymarket close_time = 2026-12-31, Kalshi close_time = **2045-01-01**. The Kalshi market has an absurdly far-out close date (19 years!), giving time_score = 0.0. Without this penalty, text_score alone (0.589) would push composite to ~0.44. Still below threshold, but the time score is unfairly killing what should be a related pair.

**2. Hungary PM (Orban):** Polymarket close_time = 2026-04-12, Kalshi close_time = 2027-05-01. The 12.5-month gap exceeds MAX_TIME_DIFF_HOURS (168h = 7 days), giving time_score = 0.0. These ARE the same event (2026 Hungarian election), but Kalshi set a far-out expiry. Text_score = 0.398 is also too low because the phrasing differs significantly.

**3. Trump/Greenland entity mismatch** (described above): A clear human match blocked at the entity gate.

---

## 3. Match Funnel

### Gate Breakdown

| Stage | Count | Percentage |
|-------|-------|-----------|
| **Total comparisons** | 437,534 | 100% |
| Blocked by `token_count` (< 2 shared tokens) | 392,110 | 89.6% |
| Blocked by `entity` (both have entities, zero overlap) | 43,131 | 9.9% |
| Blocked by `category` (incompatible categories) | 100 | 0.02% |
| **Made it to scoring** | **2,193** | **0.5%** |
| Scored >= 0.55 (match) | 1 | 0.0002% |

### Score Distribution of 2,193 Scored Pairs

| Score Bucket | Count | Avg Score |
|-------------|-------|-----------|
| 0.80-1.00 (match) | 1 | 0.862 |
| 0.60-0.79 | 0 | -- |
| 0.40-0.59 | 110 | 0.446 |
| 0.20-0.39 | 736 | 0.261 |
| 0.00-0.19 | 1,346 | 0.163 |

**Key insight:** There is a **massive gap** between the one match (0.862) and the next best score (0.494). The 0.60-0.79 bucket is completely empty. This means the current algorithm is very binary -- pairs either match very well or not at all. There are no "close calls."

### Time Score Distribution (of 2,193 scored pairs)

| Time Score | Count | Percentage |
|-----------|-------|-----------|
| **= 0** (close times > 7 days apart) | 1,930 | 88% |
| **> 0** (close times within 7 days) | 263 | 12% |

**88% of all scored pairs have time_score = 0.** This means the 7-day MAX_TIME_DIFF_HOURS is aggressively filtering out most of the score. Since time_score has 25% weight, any pair with time_score = 0 can only reach a maximum composite of 0.75 (if text_score = 1.0 and jaro_winkler = 1.0). This makes it **mathematically impossible** for pairs with close times >7 days apart to reach the 0.80 AutoVerify threshold.

---

## 4. Platform Overlap Analysis

### Category Distribution

| Category | Poly (in) | Poly (out) | Kalshi (in) | Kalshi (out) | Both In? |
|----------|----------|-----------|------------|-------------|---------|
| **Sports** | 38 | 247 | 2,836 | 0 | Yes |
| **Other** | 58 | 64 | 491 | 3 | Yes |
| **Politics** | 24 | 89 | 198 | 0 | Yes |
| **Crypto** | 11 | 50 | 97 | 0 | Yes |
| **Economics** | 0 | 11 | 27 | 0 | No -- Poly all filtered out |
| **Entertainment** | 0 | 0 | 66 | 0 | No -- not on Poly |
| **Weather** | 0 | 7 | 271 | 0 | No -- Poly all filtered out |
| **Science** | 1 | 0 | 9 | 0 | Yes (minimal) |

### Topic Overlap Within Categories

**Crypto (11 Poly vs 97 Kalshi):**
- **Polymarket:** Bitcoin price targets (above $72K, $75K, $80K, $85K), Ethereum dip targets, MegaETH token, Bitcoin weekly ranges
- **Kalshi:** Bitcoin hourly price brackets (Jan 2028, Dec 2028, Apr 2029), Shiba Inu daily price, Shiba Inu price ranges
- **Problem:** Polymarket has short-term (April 2026) crypto bets. Kalshi has long-dated hourly resolution markets (2028-2029). The close times are years apart, so even if text matches, time_score = 0. These are fundamentally different market structures -- Polymarket does "will X reach $Y this month" while Kalshi does "what will be the exact hourly price in 2028."

**Politics (24 Poly vs 198 Kalshi):**
- **Polymarket:** Hungary PM (Orban, Magyar), 2028 US presidential nominations (Newsom, Rubio, AOC, Vance, Ossoff), Peru elections, Trump impeachment, Israel PM (Bennett), Brazil elections
- **Kalshi:** 2028 presidential race (nominee, VP, party), state senate races (50+ states), governor races, EU referenda, international elections (Ghana, Philippines, Poland, Taiwan)
- **Overlap:** 2028 US presidential nomination is on both but structured differently. Polymarket asks "Will [specific person] win?" while Kalshi asks "Who will be the nominee?" (multi-outcome). Hungary and Israel PM markets exist on both but have massive close-time mismatches (discussed above).

**Other (58 Poly vs 491 Kalshi):**
- **Polymarket:** Iran conflict (7+ markets), oil prices (WTI), China/Taiwan invasion, Greenland acquisition, Musk tweet counts, UFO/alien disclosure, UAE/Iran strikes, Hezbollah ceasefire
- **Kalshi:** Citrini scenario, tech layoffs, Greenland, NFL, Elon trillionaire, nuclear fusion, Supreme Court changes, Trump administration actions, GTA VI pricing
- **Overlap:** Greenland (matched!). Iran is the biggest missed overlap -- Polymarket has massive Iran conflict markets ($500K-$1.4M volume each) but Kalshi only has "Will Iran become a democracy?" ($0 volume) and "US imports from Iran" ($0 volume). The Iran markets on the two platforms are asking about completely different things.

**Sports (38 Poly vs 2,836 Kalshi):**
- Polymarket has game winners. Kalshi has total runs, spreads, individual player stats (rebounds, assists, 3-pointers). These are different bet types for the same games. The near-misses at 0.49 composite (Detroit Tigers, White Sox) are the algorithm correctly identifying same-game markets but scoring them below threshold because they're different bet types.

---

## 5. Events vs Markets Impact

### Kalshi Market Growth
| Run | Timestamp | Kalshi Raw | Kalshi Filtered In | Matches |
|-----|-----------|-----------|-------------------|---------|
| Pre-events (14:31) | 2026-04-09T14:31:08Z | 3,000 | 2,998 | 0 |
| Post-events (18:26) | 2026-04-09T18:26:37Z | 3,998 | 3,995 | 1 |

The Events API added **~998 new Kalshi markets**. This 33% increase led directly to the first match.

### Event-Sourced Market Categories (by prefix analysis)
The Kalshi market ID prefixes reveal the event structure:
- **KXMLB*** (MLB baseball): 816 markets (hits, runs, HR, K's, totals, spreads)
- **KXNBA*** (NBA basketball): 873 markets (points, rebounds, assists, 3PT, spreads, totals)
- **KXBTCD*** (Bitcoin daily): 39 markets (hourly price resolution)
- **KXHOUSERACE**: 359 markets (US House races)
- **KXTEMPNYCH**: 30 markets (NYC temperature)
- **KXSHIBAD/KXSHIBA**: 58 markets (Shiba Inu price)
- **KXWTI**: 25 markets (crude oil)
- **KXCS2MAP**: 28 markets (Counter-Strike esports)
- **KXATPCHALLENGERMATCH**: 30 markets (tennis)
- **KXALBUMSTREAMSU**: 26 markets (Spotify streams)

### Did Events Bring Expected Crypto/Politics?
- **Crypto:** Events brought in hourly Bitcoin price resolution and Shiba Inu markets. However, these are price-at-time-X markets (2028-2029), not "will BTC reach $Y this month" style that Polymarket uses. **Structural mismatch -- same asset, incompatible bet types.**
- **Politics:** Events brought in US House races and some multi-outcome politics markets. However, Polymarket's politics markets are mostly about individuals ("Will Rubio win?") while Kalshi's are about outcomes ("Who will be the nominee?"). Multi-outcome Kalshi markets with multi-word questions tend to score poorly on text similarity.

---

## 6. Close-Time Filter Impact

### The 56 Filtered Polymarket Markets

Filter: markets with close_time before the current time (expired or imminent expiry).

**Close-time filtered range:** 2026-03-31 to 2026-04-10  
**Filtered-in range:** 2026-04-11 to 2028-11-07

| Category | Count | Notable Markets |
|----------|-------|----------------|
| Sports | 47 | NBA/NHL/MLB game winners, spreads, O/U for today's games |
| Other | 6 | Iran conflict deadlines (Apr 7, Apr 10), Elon tweet counts, Trump visit China |
| Crypto | 2 | Bitcoin above $74K on Apr 10, Bitcoin above $72K on Apr 10 |
| Economics | 1 | S&P 500 Up/Down on Apr 9 |

### Would Any Have Matched Kalshi?

**Iran markets (2 filtered):**
- "Iran x Israel/US conflict ends by April 7?" ($1.37M volume) -- Kalshi has no equivalent Iran conflict market
- "US x Iran meeting by April 10, 2026?" ($320K volume) -- Kalshi has no equivalent

**Crypto (2 filtered):**
- "Will the price of Bitcoin be above $74,000 on April 10?" -- Kalshi's Bitcoin markets are for 2028-2029 hourly prices, not April 2026. No match possible.

**Verdict: None of the 56 would have produced matches.** The close-time filter isn't hiding opportunities.

---

## 7. The Price Filter: A Bigger Hidden Issue

412 Polymarket markets were filtered out by the `price` filter. The price filter removes markets with YES price outside the `0.05 - 0.95` range (roughly). These include:

- **$45.9M volume** "US x Iran ceasefire by April 7?" (yes = $0.999)
- **$930K volume** Bitcoin price markets at extreme probabilities ($0.999 or $0.0005)
- **$1.9M volume** Denmark PM market (yes = $0.0015)

These are all at extreme yes/no prices (near 0 or 1), which correctly indicates very low or very high probability events. These are untradeable for arbitrage because the spread would need to overcome the near-certainty pricing. **The price filter is working correctly.**

---

## 8. Root Cause Analysis: Why Only 1 Match?

Three structural problems prevent more matches:

### Problem 1: Time Score Kills 88% of Scored Pairs

MAX_TIME_DIFF_HOURS = 168 hours (7 days). If close times differ by more than a week, time_score = 0. Since time_score has 25% weight, this caps maximum composite at 0.75.

**Impact:** Markets about the same event with different close times (e.g., Hungary PM: Apr 12 vs May 1 2027) get time_score = 0 and can never match.

**Fix option:** Increase MAX_TIME_DIFF_HOURS to 30 days (720 hours) for non-sports markets. Many prediction markets about the same event set different "resolution" dates (e.g., Polymarket resolves Dec 31 2026, Kalshi resolves Jan 1 2027). The Greenland match only worked because close times differed by just 39 hours.

### Problem 2: Entity Gate Is Too Strict for Synonym Entities

The entity gate requires shared entities when both sides have entities. "Trump" != "united states" kills the Trump/Greenland pair despite being about the same topic.

**Impact:** Any pair where one platform uses a person's name and the other uses a country/organization gets blocked. This is common in geopolitics ("Trump does X" vs "US does X").

**Fix option:** Add entity relationships/synonyms for geopolitical contexts. Or soften the entity gate to allow pairs through when they share keywords even without entity overlap.

### Problem 3: Structural Market Mismatch Between Platforms

- **Polymarket:** Binary outcome ("Will X happen?"), short-to-medium term, focused on crypto/geopolitics/elections
- **Kalshi:** Heavy on sports stats (individual player props, game totals), hourly crypto price resolution, multi-outcome markets ("Who will win?")
- Most sports pairs are "same game, different bet type" (winner vs total runs)
- Most crypto pairs are "same asset, different time horizon" (April 2026 vs January 2028)
- Most politics pairs are "same race, different structure" (specific candidate vs multi-outcome)

This is the fundamental challenge: even when both platforms cover the same topic, the bet structure differs enough that text similarity scores are low.

---

## 9. Recommendations

### High Confidence (likely to find 2-5 more matches)

1. **Increase MAX_TIME_DIFF_HOURS to 720h (30 days) for non-sports categories.** This would allow:
   - Hungary PM markets (Apr 12 vs May 1 -- 19 days apart)
   - Pairs where close times differ by weeks but markets are equivalent
   - Estimated impact: +1-3 matches from currently zero-time-score pairs

2. **Add "greenland" to entity dictionary and softened entity gate.** Specifically:
   - Add entity relationships: trump <-> united states (in geopolitical context)
   - Allow entity gate pass-through when keyword overlap is high (>= 3 shared tokens) even without shared entities
   - Estimated impact: +1 immediate match (Trump/Greenland pair), +unknown from other blocked pairs

3. **Lower the match threshold from 0.55 to 0.50 for NeedsReview tier.** Currently there's a huge gap (0.494 to 0.862). The sports pairs at 0.49 are correctly identified as related but different bet types. A NeedsReview tier at 0.50 would surface these for human validation.

### Medium Confidence (requires more investigation)

4. **Handle multi-outcome Kalshi markets.** Kalshi's "Who will be the nominee?" is equivalent to Polymarket's individual "Will [person] win?" markets. This requires understanding that one multi-outcome market maps to multiple binary markets. The text similarity between "Will Marco Rubio win the 2028 Republican presidential nomination?" and "Who will be the Republican nominee in 2028?" is inherently low because the phrasing is so different.

5. **Add topic-level matching for sports.** Detect when two markets are about the same game but different bet types. Currently "Detroit Tigers vs. Minnesota Twins" (winner) matches with "Detroit vs Minnesota Total Runs?" at 0.49 -- these ARE related markets but not directly arbitrageable (different bet types).

### Lower Priority

6. **Monitor Iran market equivalence.** Polymarket has $500K-$1.4M Iran conflict markets. Kalshi only has "Will Iran become a democracy?" (0 volume) and "US imports from Iran" (0 volume). When/if Kalshi adds real Iran conflict markets, this becomes the single highest-value matching opportunity.

7. **Watch for more Kalshi events.** The jump from 3,000 to 3,998 Kalshi markets directly enabled the first match. As Kalshi adds more events (especially in politics and crypto with matching bet structures), more matches become possible.

---

## 10. Summary

| Metric | Value |
|--------|-------|
| Match found | 1 (Greenland acquisition) |
| Spread on match | ~6.5 cents YES (marginal after fees) |
| False negatives identified | 2-3 (Trump/Greenland entity block, Israel PM time mismatch, Hungary PM time mismatch) |
| Root cause of low match count | 88% of scored pairs killed by 7-day time window; entity gate too strict; structural market type mismatches between platforms |
| Most promising quick wins | Relax time window to 30 days + soften entity gate = likely 2-5 more matches |
| Biggest untapped opportunity | Iran conflict markets ($2M+ Poly volume with zero Kalshi equivalent) |
