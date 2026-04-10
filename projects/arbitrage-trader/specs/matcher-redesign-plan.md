# Matcher Redesign: Architecture Plan

## Problem Statement

The current market matcher uses Jaro-Winkler character-level similarity as its primary scoring signal (70% weight), which produces catastrophic false positives when matching cross-platform prediction markets. Character-level edit distance has no concept of entities, topics, or semantic meaning. Real production data shows a 100% false positive rate -- zero correct matches from 36 x 200 = 7,200 comparisons.

### Root Cause Analysis

Jaro-Winkler scores high on pairs that share:
- Common structural patterns ("Will X by Y?", "Will X on Z?")
- Date fragments ("April 10, 2026" / "Apr 10, 2026")
- Short common character sequences across unrelated topics

It cannot distinguish:
- **Entities**: "Iran" vs "Shiba Inu" (both have 'i', 'n', partial overlaps)
- **Topic categories**: political events vs crypto prices vs weather
- **Semantic meaning**: "meeting" vs "price range"

The token pre-filter (`MIN_SHARED_TOKENS = 1`) is too permissive -- a single shared token like "price" or "2026" lets unrelated pairs through.

## Proposed Architecture: Multi-Stage Token-Semantic Pipeline

### Design Principles

1. **Cheap filters first, expensive scorers last** (funnel pattern)
2. **Entity-aware**: proper nouns and key subjects must overlap for a match
3. **Token-level, not character-level**: similarity operates on meaningful words
4. **No ML models, no external APIs**: pure Rust, deterministic, fast
5. **Backward compatible**: same `MatchPipeline`, `MatchCandidate`, `MatchScore` public API

### Pipeline Stages

```
Stage 1: Enhanced Normalization + Token Extraction
   |
Stage 2: Category Pre-Filter (hard gate)
   |
Stage 3: Entity Overlap Gate (hard gate)
   |
Stage 4: Token Similarity Scoring (Jaccard + weighted overlap)
   |
Stage 5: Close-Time Proximity (soft signal)
   |
Stage 6: Composite Score + Greedy Dedup
```

---

### Stage 1: Enhanced Normalization + Token Extraction

**File: `normalize.rs` (rewrite)**

#### 1a. Expanded Stop Word List

Current list has 37 words. Expand to ~80+ covering prediction market boilerplate:

```
Additions: "will", "be", "is", "are", "was", "were", "do", "does", "did",
"above", "below", "between", "reach", "hit", "exceed", "drop", "fall",
"rise", "range", "price", "win", "lose", "happen", "occur",
"before", "after", "during", "through", "about", "over", "under",
"more", "less", "than", "how", "many", "much", "what", "which",
"when", "where", "who", "whom", "whose", "why",
"any", "each", "every", "some", "most", "other",
"could", "would", "should", "can", "may", "might",
"been", "being", "get", "got", "goes", "going"
```

Keep domain-significant words even if they seem generic: "price", "win", "election", "temperature".

**Decision**: Actually, "price" and "win" are borderline. "price" appears in crypto AND weather ("price range"). Keep "price" as meaningful because it distinguishes pricing markets from event-outcome markets. Keep "win" because it distinguishes competition markets. Remove only true function words.

#### 1b. Token Classification

Classify each token into one of:
- **Entity**: Proper nouns, recognized names (detected by capitalization in original text before lowercasing, or membership in known entity lists)
- **Number**: Numeric values including dollar amounts, percentages, temperatures
- **Date**: Date-like tokens (month names, year numbers 2024-2030, day numbers when adjacent to months)
- **Keyword**: Domain-meaningful common nouns and verbs (e.g., "bitcoin", "election", "temperature", "tweet")
- **Stop**: Function words to discard

Implementation approach:
```rust
pub struct ClassifiedTokens {
    pub entities: Vec<String>,    // "bitcoin", "iran", "elon musk", "nyc"
    pub numbers: Vec<String>,     // "100k", "36.99", "260", "279"
    pub dates: Vec<String>,       // "april", "2026", "apr 10"
    pub keywords: Vec<String>,    // "election", "meeting", "tweets", "temperature"
    pub all_meaningful: Vec<String>, // union of above (for Jaccard)
}
```

#### 1c. Entity Recognition (Lightweight, No ML)

Since we cannot use ML, use a **rule-based approach**:

1. **Capitalization heuristic**: Before lowercasing, scan for words that start with uppercase letters (excluding sentence-start). Consecutive capitalized words form multi-word entities (e.g., "Elon Musk", "Carlos Alvarez", "Shiba Inu").

2. **Known entity dictionary**: Maintain a static list of ~200 commonly traded entities:
   - **Crypto**: bitcoin, btc, ethereum, eth, solana, sol, dogecoin, doge, shiba inu, xrp, cardano, ada...
   - **Countries**: usa, us, iran, china, russia, ukraine, israel, peru, brazil...
   - **People**: trump, biden, musk, elon...
   - **Sports**: super bowl, nba, nfl, mlb, champions league...
   - **Indices/Assets**: s&p, sp500, nasdaq, dow, gold, oil, wti...

3. **Alias normalization**: Map known aliases to canonical forms:
   - "btc" -> "bitcoin", "eth" -> "ethereum"
   - "nyc" -> "new york city"
   - "us" -> "united states"

This dictionary does NOT need to be exhaustive. Its purpose is to boost matching precision for the most common market topics. Unknown entities still get caught by the capitalization heuristic.

---

### Stage 2: Category Pre-Filter (Hard Gate)

**New file: `category.rs`**

Assign each market a coarse-grained category based on keyword presence:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MarketCategory {
    Crypto,
    Politics,
    Sports,
    Weather,
    Economics,
    Entertainment,
    Science,
    Other,
}
```

**Category detection rules** (keyword-based, checked in order, first match wins):

| Category | Trigger keywords |
|----------|-----------------|
| Crypto | bitcoin, btc, ethereum, eth, solana, dogecoin, shiba inu, crypto, token, nft, defi, altcoin, xrp, cardano, polkadot, chainlink |
| Politics | election, president, congress, senate, vote, poll, government, democrat, republican, minister, prime minister, parliament, legislation, bill, law |
| Sports | super bowl, nba, nfl, mlb, world cup, champions league, playoff, championship, tournament, game, match (in sports context), season, finals |
| Weather | temperature, temp, weather, fahrenheit, celsius, hurricane, tornado, rainfall, snowfall, heatwave, climate |
| Economics | gdp, inflation, interest rate, fed, federal reserve, unemployment, jobs report, cpi, ppi, treasury, yield, s&p, nasdaq, dow |
| Entertainment | oscar, emmy, grammy, box office, movie, album, song, billboard, streaming, netflix, disney |
| Science | nasa, spacex, launch, asteroid, earthquake, volcano |
| Other | (default) |

**Matching rule**: Two markets can only match if:
- Their categories are the **same**, OR
- At least one market is categorized as `Other`

This is a **hard gate** -- if a Crypto market gets compared against a Politics market, skip immediately. This alone would have prevented ALL four false positives in the real data (Crypto vs Politics, Crypto vs Weather, Tweet-counting vs Weather).

**Cost**: O(1) per market (computed once, cached). Negligible.

---

### Stage 3: Entity Overlap Gate (Hard Gate)

**New logic in `pipeline.rs`**

After category pre-filter passes, check entity overlap:

```rust
let poly_entities = &poly_classified[i].entities;
let kalshi_entities = &kalshi_classified[j].entities;

let shared_entities = poly_entities.iter()
    .filter(|e| kalshi_entities.contains(e))
    .count();

// HARD GATE: Must share at least 1 entity, OR both have no entities
if !poly_entities.is_empty() && !kalshi_entities.is_empty() && shared_entities == 0 {
    continue; // skip — different subjects
}
```

**Why this works for the false positives**:
- "US x Iran meeting" has entities ["us", "iran"]. "Shiba Inu price range" has entities ["shiba inu"]. Zero overlap -> SKIP.
- "Elon Musk post 260-279 tweets" has entities ["elon musk"]. "temperature in NYC" has entities ["nyc"]. Zero overlap -> SKIP.
- "Carlos Alvarez win 2026 Peruvian election" has entities ["carlos alvarez", "peruvian"]. "Shiba Inu price range" has entities ["shiba inu"]. Zero overlap -> SKIP.

**Edge case**: If BOTH markets have empty entity lists (e.g., very generic questions), allow them through to the scoring stage -- the token similarity will handle it.

---

### Stage 4: Token Similarity Scoring (Replaces Jaro-Winkler)

**File: `scorer.rs` (rewrite)**

Replace `strsim::jaro_winkler` with a multi-signal token-based scorer:

#### 4a. Jaccard Similarity on Meaningful Tokens

```
jaccard = |intersection(A, B)| / |union(A, B)|
```

Where A and B are sets of meaningful tokens (entities + numbers + dates + keywords). This operates on **words**, not characters, so it cannot be fooled by shared character substrings.

Example:
- A = {"us", "iran", "meeting", "april", "10", "2026"}
- B = {"shiba", "inu", "price", "range", "apr", "10", "2026"}
- intersection = {"10", "2026"} (only 2 tokens)
- union = {"us", "iran", "meeting", "april", "shiba", "inu", "price", "range", "apr", "10", "2026"} (11 tokens)
- Jaccard = 2/11 = 0.18 -> VERY LOW, correctly rejects

Compare with a true match:
- A = {"bitcoin", "hit", "100k"} (from Polymarket "Will Bitcoin hit $100k?")
- B = {"bitcoin", "hit", "100k"} (from Kalshi "Will Bitcoin hit $100k?")
- Jaccard = 3/3 = 1.0 -> PERFECT

#### 4b. Weighted Token Overlap

Not all tokens are equally important. An entity match ("bitcoin" = "bitcoin") is far more significant than a date match ("2026" = "2026").

**Token weights**:
| Token class | Weight |
|-------------|--------|
| Entity | 3.0 |
| Keyword | 2.0 |
| Number | 1.5 |
| Date | 0.5 |

```
weighted_overlap = sum(weight[t] for t in intersection) / sum(weight[t] for t in union)
```

This down-weights date coincidences (many markets share "2026") and up-weights entity matches.

#### 4c. Minimum Token Overlap Threshold

Require `|intersection| >= 2` meaningful tokens to produce a non-zero score. A single shared token like "2026" is insufficient.

#### 4d. Composite Text Score

```
text_score = 0.60 * weighted_overlap + 0.40 * jaccard
```

Both signals reinforce each other. Jaccard catches broadly similar questions; weighted overlap rewards entity and keyword matches specifically.

#### 4e. Optional: Jaro-Winkler as Tiebreaker

Keep `strsim::jaro_winkler` as a minor tiebreaker signal (5-10% weight) ONLY for candidates that already pass the token-based scoring. This helps distinguish "Will Bitcoin hit $100k by December?" vs "Will Bitcoin hit $100k by March?" where token overlap is identical but character-level similarity differs slightly.

---

### Stage 5: Close-Time Proximity (Unchanged)

Keep the existing close-time scoring logic. Markets about the same event should close at similar times.

```
time_score = 1.0 - (time_diff_hours / 168.0)  // 0 if >7 days apart
```

This remains a soft signal, not a hard gate -- different platforms might set slightly different close times.

---

### Stage 6: Final Composite Score

```
composite = 0.65 * text_score + 0.25 * time_score + 0.10 * jaro_winkler_tiebreaker
```

**Decision thresholds** (updated):
| Composite | Decision |
|-----------|----------|
| >= 0.80 | AutoVerified |
| >= 0.55 | NeedsReview |
| < 0.55 | Rejected |

The AutoVerified threshold is lowered from 0.85 to 0.80 because the new scoring system is more precise -- a 0.80 under token-based scoring represents much higher actual match quality than a 0.80 under Jaro-Winkler.

The NeedsReview threshold is raised from 0.50 to 0.55 to reduce the review burden.

---

### Updated MatchScore Struct

```rust
pub struct MatchScore {
    pub jaccard: f64,              // Jaccard similarity on meaningful tokens
    pub weighted_overlap: f64,     // Entity/keyword-weighted overlap
    pub text_score: f64,           // Combined token-based text score
    pub jaro_winkler: f64,         // Character-level tiebreaker (kept for debugging)
    pub close_time_score: f64,     // Time proximity signal
    pub composite: f64,            // Final weighted score
    pub shared_entities: usize,    // Number of shared entities (for debugging/display)
    pub shared_tokens: usize,      // Number of shared meaningful tokens
    pub category_match: bool,      // Whether categories matched
}
```

The expanded struct aids debugging and allows the TUI to show WHY a match was made or rejected.

---

## File Change Summary

| File | Action | Description |
|------|--------|-------------|
| `normalize.rs` | **Major rewrite** | Add token classification, entity recognition, alias normalization, expanded stop words |
| `category.rs` | **New file** | Market category inference from keywords |
| `scorer.rs` | **Major rewrite** | Replace Jaro-Winkler primary scoring with Jaccard + weighted token overlap |
| `pipeline.rs` | **Moderate changes** | Add category pre-filter and entity overlap gate stages |
| `types.rs` | **Moderate changes** | Expand MatchScore struct with new fields |
| `lib.rs` | **Minor change** | Add `pub mod category;` |
| `store.rs` | **No changes** | Persistence layer is scoring-agnostic |

---

## Performance Analysis

Current: O(n * m) with Jaro-Winkler per pair. For 200 x 200 = 40,000 pairs:
- Jaro-Winkler: ~1 microsecond per pair = ~40ms total

New pipeline with pre-filters:
- Category pre-filter: O(1) lookup, eliminates ~80% of pairs (most markets are in different categories)
- Entity overlap gate: O(e1 * e2) where e is entity count per market (~3-5), eliminates ~90% of remaining pairs
- Token scoring on survivors: ~2-5 microseconds per pair (set operations on ~5-10 tokens)
- Estimated survivors reaching scoring: ~400-800 pairs (from 40,000)
- **Total estimated time: <10ms for 200x200**, faster than current due to aggressive pre-filtering

**No new external crate dependencies required.** The `strsim` crate is kept for the tiebreaker signal. All new logic is implemented with `std::collections::HashMap/HashSet`.

---

## Trade-Offs and Risks

### Accepted Trade-Offs

1. **Entity dictionary maintenance**: The static entity list needs occasional updates as new topics emerge (new crypto coins, new political figures). Mitigation: the capitalization heuristic catches unknown entities; the dictionary is a precision booster, not a requirement.

2. **Category rigidity**: Keyword-based categories may miscategorize edge cases (e.g., "Will the SEC approve a Bitcoin ETF?" is both Crypto and Politics/Economics). Mitigation: `Other` category acts as wildcard; markets can match across categories if one is `Other`.

3. **No fuzzy entity matching**: "BTC" and "Bitcoin" need explicit alias mapping. Mitigation: maintain alias table for top ~50 entities; unknown aliases fall through to the capitalization heuristic.

### Rejected Alternatives

1. **TF-IDF cosine similarity**: Would require building a corpus-wide term frequency index. Adds complexity for marginal benefit over Jaccard when entity pre-filtering is already in place. Could be added later as a refinement.

2. **Embedding-based matching (sentence-transformers)**: Violates the "no ML models" constraint. Would require ONNX runtime or similar in Rust. Deferred to a future phase if token-based matching proves insufficient.

3. **LLM-based matching (send pairs to Claude API)**: Too slow and expensive for 40,000 comparisons. Could be used as a verification step for the ~10-20 NeedsReview candidates per run.

### Risks

1. **False negatives**: The hard gates (category, entity overlap) could reject true matches if categorization or entity extraction fails. Mitigation: log all gate rejections at DEBUG level; `Other` category is permissive; entity gate allows through when both sides have empty entity lists.

2. **Alias coverage**: An unmapped alias (e.g., a new crypto ticker) could cause a true match to fail entity overlap. Mitigation: the system degrades gracefully -- the match may still pass via keyword overlap if entities are not recognized, since unrecognized words become keywords rather than entities.

---

## Migration Strategy

1. **Implement behind a feature flag** or config toggle so the old Jaro-Winkler pipeline can be restored instantly if the new pipeline has issues.
2. **Dual-run mode**: For the first few sessions, run both old and new pipelines and log the differences. This catches false negatives the new pipeline might introduce.
3. **Existing tests**: Update all existing unit tests. The false-positive regression tests from real data should all pass (i.e., those pairs must score below threshold).
4. **New tests**: Add tests for each pipeline stage independently (category classification, entity extraction, token scoring).
