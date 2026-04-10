# Matcher Redesign: Detailed Specification

**Depends on**: [matcher-redesign-plan.md](./matcher-redesign-plan.md)

---

## 1. Definitions

- **True Positive (TP)**: Two markets on different platforms that refer to the same real-world event AND the matcher scores them above the acceptance threshold.
- **False Positive (FP)**: Two markets on different platforms that refer to different real-world events BUT the matcher scores them above the acceptance threshold.
- **True Negative (TN)**: Two unrelated markets correctly scored below threshold.
- **False Negative (FN)**: Two markets that DO refer to the same event but the matcher scores them below threshold.
- **Meaningful token**: A token remaining after stop-word removal, classified as entity, keyword, number, or date.
- **Entity**: A proper noun, recognized asset name, person name, country, or organization identified by capitalization heuristic or dictionary lookup.
- **Category**: A coarse-grained topic label (Crypto, Politics, Sports, Weather, Economics, Entertainment, Science, Other) assigned to a market based on keyword presence.

---

## 2. Acceptance Criteria

### AC-1: Zero false positives on known bad pairs

The following pairs, drawn from real production data, MUST score below the Rejected threshold (composite < 0.55):

| # | Polymarket Question | Kalshi Question | Required: Rejected |
|---|--------------------|-----------------|--------------------|
| 1 | "US x Iran meeting by April 10, 2026?" | "Shiba Inu price range on Apr 10, 2026?" | YES |
| 2 | "Will Elon Musk post 260-279 tweets this week?" | "Will the temp in NYC be above 36.99 degrees?" | YES |
| 3 | "Will Carlos Alvarez win the 2026 Peruvian presidential election?" | "Shiba Inu price range on Apr 10, 2026?" | YES |
| 4 | "Will Ricardo Belmont win the 2026 Peruvian presidential election?" | "Shiba Inu price range on Apr 10, 2026?" | YES |

### AC-2: True matches score above NeedsReview threshold

The following pairs MUST score at or above the NeedsReview threshold (composite >= 0.55):

| # | Polymarket Question | Kalshi Question | Required: NeedsReview or AutoVerified |
|---|--------------------|-----------------|--------------------|
| 1 | "Will Bitcoin hit $100k?" | "Will Bitcoin hit $100k?" | YES (AutoVerified) |
| 2 | "Will Bitcoin hit $100k by December 2025?" | "Bitcoin to hit $100k before December 2025?" | YES |
| 3 | "Will Trump win the 2024 presidential election?" | "Will Donald Trump win the 2024 US presidential election?" | YES |
| 4 | "Will ETH reach $5000 by end of 2025?" | "Will Ethereum reach $5000 before 2026?" | YES |

### AC-3: Identical questions score AutoVerified

When two markets have identical questions (case-insensitive, ignoring punctuation) and close times within 24 hours, the composite score MUST be >= 0.80 (AutoVerified threshold).

### AC-4: Category pre-filter blocks cross-category pairs

The pipeline MUST NOT produce match candidates where:
- One market is categorized as Crypto and the other as Politics
- One market is categorized as Crypto and the other as Weather
- One market is categorized as Sports and the other as Economics
- (Any two different non-Other categories)

Exception: If either market is categorized as Other, cross-category matching is allowed.

### AC-5: Entity overlap gate blocks different-subject pairs

When both markets have at least one recognized entity each, and they share zero entities (after alias normalization), the pipeline MUST skip scoring the pair entirely.

### AC-6: Minimum shared token count

Pairs with fewer than 2 shared meaningful tokens MUST score 0.0 for the text component, regardless of other signals.

### AC-7: Performance requirement

Matching 200 Polymarket markets against 200 Kalshi markets (40,000 candidate pairs) MUST complete in under 100 milliseconds on a single thread (release build). The pre-filters should eliminate >80% of pairs before scoring.

### AC-8: Expanded MatchScore struct

The `MatchScore` struct MUST include at minimum:
- `jaccard: f64` -- Jaccard similarity on meaningful token sets
- `weighted_overlap: f64` -- Entity/keyword-weighted overlap score
- `text_score: f64` -- Combined token-based text score
- `close_time_score: f64` -- Time proximity signal
- `composite: f64` -- Final weighted composite
- `shared_entities: usize` -- Count of shared entities
- `shared_tokens: usize` -- Count of shared meaningful tokens

### AC-9: Backward-compatible public API

The following public API items MUST continue to exist and function:
- `MatchPipeline::find_matches(&self, poly: &[Market], kalshi: &[Market]) -> Vec<MatchCandidate>`
- `MatchCandidate` with fields `poly_market`, `kalshi_market`, `score`
- `MatchScore` with field `composite` and method `decision() -> MatchDecision`
- `MatchDecision` enum with variants `AutoVerified`, `NeedsReview`, `Rejected`

### AC-10: Greedy dedup preserved

Each Polymarket market MUST appear in at most one MatchCandidate in the output. Each Kalshi market MUST appear in at most one MatchCandidate. The highest composite scorer wins.

### AC-11: Decision thresholds updated

- AutoVerified: composite >= 0.80
- NeedsReview: composite >= 0.55
- Rejected: composite < 0.55

---

## 3. Test Cases

### 3.1 False Positive Regression Tests (from real data)

These are the highest priority tests. They encode the actual production failures that motivated this redesign.

```rust
#[test]
fn regression_fp1_iran_meeting_vs_shiba_inu() {
    // Real FP: old scorer gave 0.810
    let poly = make_market(Poly, "US x Iran meeting by April 10, 2026?", 24);
    let kalshi = make_market(Kalshi, "Shiba Inu price range on Apr 10, 2026?", 24);
    let score = score(&poly, &kalshi);
    assert!(score.composite < 0.55,
        "Iran meeting vs Shiba Inu must be Rejected, got {}", score.composite);
    // Category gate: Politics vs Crypto -> blocked
    // Entity gate: {"us", "iran"} vs {"shiba inu"} -> zero overlap -> blocked
}

#[test]
fn regression_fp2_musk_tweets_vs_nyc_temp() {
    // Real FP: old scorer gave 0.741
    let poly = make_market(Poly, "Will Elon Musk post 260-279 tweets this week?", 24);
    let kalshi = make_market(Kalshi, "Will the temp in NYC be above 36.99 degrees?", 24);
    let score = score(&poly, &kalshi);
    assert!(score.composite < 0.55,
        "Musk tweets vs NYC temp must be Rejected, got {}", score.composite);
    // Category gate: Entertainment/Other vs Weather -> likely blocked
    // Entity gate: {"elon musk"} vs {"nyc"} -> zero overlap -> blocked
}

#[test]
fn regression_fp3_alvarez_peru_vs_shiba_inu() {
    // Real FP: old scorer gave 0.709
    let poly = make_market(Poly, "Will Carlos Alvarez win the 2026 Peruvian presidential election?", 24);
    let kalshi = make_market(Kalshi, "Shiba Inu price range on Apr 10, 2026?", 24);
    let score = score(&poly, &kalshi);
    assert!(score.composite < 0.55,
        "Alvarez election vs Shiba Inu must be Rejected, got {}", score.composite);
    // Category gate: Politics vs Crypto -> blocked
}

#[test]
fn regression_fp4_belmont_peru_vs_shiba_inu() {
    // Real FP: old scorer gave 0.707
    let poly = make_market(Poly, "Will Ricardo Belmont win the 2026 Peruvian presidential election?", 24);
    let kalshi = make_market(Kalshi, "Shiba Inu price range on Apr 10, 2026?", 24);
    let score = score(&poly, &kalshi);
    assert!(score.composite < 0.55,
        "Belmont election vs Shiba Inu must be Rejected, got {}", score.composite);
    // Category gate: Politics vs Crypto -> blocked
}
```

### 3.2 True Positive Tests (known good matches)

```rust
#[test]
fn tp1_identical_bitcoin_questions() {
    let poly = make_market(Poly, "Will Bitcoin hit $100k?", 24);
    let kalshi = make_market(Kalshi, "Will Bitcoin hit $100k?", 24);
    let score = score(&poly, &kalshi);
    assert!(score.composite >= 0.80,
        "Identical Bitcoin questions must be AutoVerified, got {}", score.composite);
    assert_eq!(score.decision(), MatchDecision::AutoVerified);
}

#[test]
fn tp2_bitcoin_different_phrasing() {
    let poly = make_market(Poly, "Will Bitcoin hit $100k by December 2025?", 48);
    let kalshi = make_market(Kalshi, "Bitcoin to hit $100k before December 2025?", 48);
    let score = score(&poly, &kalshi);
    assert!(score.composite >= 0.55,
        "Similar Bitcoin questions must be NeedsReview+, got {}", score.composite);
    // Shared entities: {"bitcoin"}, shared tokens: {"bitcoin", "hit", "100k", "december", "2025"}
}

#[test]
fn tp3_trump_election_different_phrasing() {
    let poly = make_market(Poly, "Will Trump win the 2024 presidential election?", 48);
    let kalshi = make_market(Kalshi, "Will Donald Trump win the 2024 US presidential election?", 48);
    let score = score(&poly, &kalshi);
    assert!(score.composite >= 0.55,
        "Trump election questions must match, got {}", score.composite);
    // Shared entities: {"trump"} or {"donald trump"}, shared keywords: {"presidential", "election", "2024"}
}

#[test]
fn tp4_ethereum_alias_matching() {
    let poly = make_market(Poly, "Will ETH reach $5000 by end of 2025?", 48);
    let kalshi = make_market(Kalshi, "Will Ethereum reach $5000 before 2026?", 48);
    let score = score(&poly, &kalshi);
    assert!(score.composite >= 0.55,
        "ETH/Ethereum alias should match, got {}", score.composite);
    // Alias normalization: "eth" -> "ethereum"
    // Shared entities: {"ethereum"}, shared tokens: {"ethereum", "reach", "5000"}
}

#[test]
fn tp5_btc_alias_matching() {
    let poly = make_market(Poly, "Will BTC reach $100k?", 24);
    let kalshi = make_market(Kalshi, "Will Bitcoin reach $100k?", 24);
    let score = score(&poly, &kalshi);
    assert!(score.composite >= 0.55,
        "BTC/Bitcoin alias should match, got {}", score.composite);
}
```

### 3.3 Category Pre-Filter Tests

```rust
#[test]
fn category_crypto_detected() {
    assert_eq!(classify("Will Bitcoin hit $100k?"), MarketCategory::Crypto);
    assert_eq!(classify("Shiba Inu price range on Apr 10?"), MarketCategory::Crypto);
    assert_eq!(classify("Will ETH reach $5000?"), MarketCategory::Crypto);
}

#[test]
fn category_politics_detected() {
    assert_eq!(classify("Will Trump win the 2024 election?"), MarketCategory::Politics);
    assert_eq!(classify("Will Carlos Alvarez win the 2026 Peruvian presidential election?"), MarketCategory::Politics);
    assert_eq!(classify("US x Iran meeting by April 10?"), MarketCategory::Politics);
}

#[test]
fn category_weather_detected() {
    assert_eq!(classify("Will the temp in NYC be above 36.99 degrees?"), MarketCategory::Weather);
}

#[test]
fn category_sports_detected() {
    assert_eq!(classify("Who will win the Super Bowl 2025?"), MarketCategory::Sports);
    assert_eq!(classify("Will the Lakers win the NBA championship?"), MarketCategory::Sports);
}

#[test]
fn category_cross_category_blocked() {
    let pipeline = MatchPipeline::default();
    let poly = vec![make_market(Poly, "Will Bitcoin hit $100k?", 24)]; // Crypto
    let kalshi = vec![make_market(Kalshi, "Will Trump win the election?", 24)]; // Politics
    let results = pipeline.find_matches(&poly, &kalshi);
    assert!(results.is_empty(), "Crypto vs Politics must be blocked");
}

#[test]
fn category_other_allows_cross_matching() {
    // A market with no clear category should still be matchable
    let poly = vec![make_market(Poly, "Will event X happen by Friday?", 24)]; // Other
    let kalshi = vec![make_market(Kalshi, "Will event X happen this week?", 24)]; // Other
    let results = MatchPipeline::default().find_matches(&poly, &kalshi);
    // Should not be blocked by category filter (both Other)
    // Whether it actually matches depends on token overlap
}
```

### 3.4 Entity Extraction Tests

```rust
#[test]
fn entity_extraction_proper_nouns() {
    let tokens = classify_tokens("Will Elon Musk post 260 tweets?");
    assert!(tokens.entities.contains(&"elon musk".to_string()));
}

#[test]
fn entity_extraction_crypto_names() {
    let tokens = classify_tokens("Will Bitcoin hit $100k?");
    assert!(tokens.entities.contains(&"bitcoin".to_string()));
}

#[test]
fn entity_extraction_country_names() {
    let tokens = classify_tokens("US x Iran meeting by April 10?");
    assert!(tokens.entities.iter().any(|e| e == "us" || e == "united states"));
    assert!(tokens.entities.contains(&"iran".to_string()));
}

#[test]
fn entity_alias_normalization() {
    let tokens = classify_tokens("Will BTC reach $100k?");
    assert!(tokens.entities.contains(&"bitcoin".to_string()),
        "BTC should be normalized to bitcoin");
}

#[test]
fn entity_alias_eth_to_ethereum() {
    let tokens = classify_tokens("Will ETH reach $5000?");
    assert!(tokens.entities.contains(&"ethereum".to_string()),
        "ETH should be normalized to ethereum");
}

#[test]
fn entity_overlap_gate_blocks_different_entities() {
    let a = classify_tokens("Will Iran negotiate with the US?");
    let b = classify_tokens("Shiba Inu price on Apr 10?");
    let shared = entity_overlap(&a.entities, &b.entities);
    assert_eq!(shared, 0, "Iran/US vs Shiba Inu should have zero entity overlap");
}

#[test]
fn entity_overlap_gate_passes_same_entities() {
    let a = classify_tokens("Will Bitcoin hit $100k by December?");
    let b = classify_tokens("Bitcoin to reach $100k before year end?");
    let shared = entity_overlap(&a.entities, &b.entities);
    assert!(shared >= 1, "Both mention Bitcoin, should share entity");
}
```

### 3.5 Token Scoring Tests

```rust
#[test]
fn jaccard_identical_tokens() {
    let a = vec!["bitcoin", "hit", "100k"];
    let b = vec!["bitcoin", "hit", "100k"];
    let j = jaccard_similarity(&a, &b);
    assert!((j - 1.0).abs() < 0.01, "Identical token sets: jaccard={j}");
}

#[test]
fn jaccard_no_overlap() {
    let a = vec!["bitcoin", "hit", "100k"];
    let b = vec!["super", "bowl", "winner"];
    let j = jaccard_similarity(&a, &b);
    assert!((j - 0.0).abs() < 0.01, "No overlap: jaccard={j}");
}

#[test]
fn jaccard_partial_overlap() {
    let a = vec!["bitcoin", "hit", "100k", "december"];
    let b = vec!["bitcoin", "reach", "100k", "year"];
    // intersection: {"bitcoin", "100k"} = 2
    // union: {"bitcoin", "hit", "100k", "december", "reach", "year"} = 6
    let j = jaccard_similarity(&a, &b);
    assert!((j - 2.0/6.0).abs() < 0.05, "Partial overlap: jaccard={j}");
}

#[test]
fn weighted_overlap_entities_count_more() {
    // Two pairs with same number of shared tokens, but one shares entities
    let score_with_entity = weighted_score(
        &ClassifiedTokens { entities: vec!["bitcoin"], keywords: vec!["hit"], numbers: vec!["100k"], dates: vec![] },
        &ClassifiedTokens { entities: vec!["bitcoin"], keywords: vec!["reach"], numbers: vec!["100k"], dates: vec![] },
    );
    let score_without_entity = weighted_score(
        &ClassifiedTokens { entities: vec![], keywords: vec!["hit", "price"], numbers: vec!["100k"], dates: vec!["2026"] },
        &ClassifiedTokens { entities: vec![], keywords: vec!["range", "price"], numbers: vec!["100k"], dates: vec!["2026"] },
    );
    assert!(score_with_entity > score_without_entity,
        "Entity matches should produce higher weighted scores");
}

#[test]
fn min_shared_tokens_enforced() {
    // Only 1 shared token should produce text_score = 0.0
    let poly = make_market(Poly, "Will something happen in 2026?", 24);
    let kalshi = make_market(Kalshi, "Will another thing happen in 2026?", 24);
    let score = score(&poly, &kalshi);
    // Only "2026" (maybe "happen") shared -- if only 1, text_score = 0
    // This depends on stop word list; "happen" might be kept
    // The key assertion: sharing only date tokens should not produce a high score
    assert!(score.composite < 0.55, "Single shared date token should not match");
}
```

### 3.6 Edge Cases

```rust
#[test]
fn edge_same_topic_different_timeframe() {
    // "Will Bitcoin hit $100k by December 2025?" vs "Will Bitcoin hit $100k by March 2025?"
    // Same subject, different deadline -- these are DIFFERENT markets
    let poly = make_market(Poly, "Will Bitcoin hit $100k by December 2025?", 720); // ~30 days
    let kalshi = make_market(Kalshi, "Will Bitcoin hit $100k by March 2025?", 24); // ~1 day
    let score = score(&poly, &kalshi);
    // Tokens are very similar, but close_time differs significantly
    // This SHOULD score NeedsReview (human must verify the timeframe)
    // The time signal should pull the composite down
    // We do NOT require it to be Rejected -- it's genuinely ambiguous
}

#[test]
fn edge_same_topic_different_threshold() {
    // "Will Bitcoin hit $100k?" vs "Will Bitcoin hit $50k?"
    // Same asset, different price target -- these are DIFFERENT markets
    let poly = make_market(Poly, "Will Bitcoin hit $100k?", 24);
    let kalshi = make_market(Kalshi, "Will Bitcoin hit $50k?", 24);
    let score = score(&poly, &kalshi);
    // Shared: {"bitcoin", "hit"}, Different: {"100k"} vs {"50k"}
    // Should score moderate (NeedsReview) not AutoVerified
    assert!(score.decision() != MatchDecision::AutoVerified,
        "Different price targets should not AutoVerify");
}

#[test]
fn edge_multi_outcome_same_event() {
    // Same election, different candidates
    // "Will Carlos Alvarez win the 2026 Peruvian election?" vs
    // "Will Ricardo Belmont win the 2026 Peruvian election?"
    // These are DIFFERENT markets (different outcomes of same event)
    let poly = make_market(Poly, "Will Carlos Alvarez win the 2026 Peruvian presidential election?", 48);
    let kalshi = make_market(Kalshi, "Will Ricardo Belmont win the 2026 Peruvian presidential election?", 48);
    let score = score(&poly, &kalshi);
    // Shared entities: possibly "peruvian" but NOT the candidate names
    // Shared keywords: "presidential", "election", "2026"
    // Entity gate: Alvarez != Belmont -> different primary entities
    // Should be Rejected or low NeedsReview at most
    assert!(score.composite < 0.80,
        "Different candidates for same election must NOT AutoVerify, got {}", score.composite);
}

#[test]
fn edge_empty_question() {
    let poly = make_market(Poly, "", 24);
    let kalshi = make_market(Kalshi, "", 24);
    let score = score(&poly, &kalshi);
    // Empty questions should not crash and should not match
    assert!(score.composite < 0.55, "Empty questions should not match");
}

#[test]
fn edge_very_short_question() {
    let poly = make_market(Poly, "Yes?", 24);
    let kalshi = make_market(Kalshi, "No?", 24);
    let score = score(&poly, &kalshi);
    assert!(score.composite < 0.55, "Trivial questions should not match");
}

#[test]
fn edge_unicode_and_special_chars() {
    let poly = make_market(Poly, "Will Carlos Alvarez win the 2026 election?", 48);
    let kalshi = make_market(Kalshi, "Will Carlos Alvarez win the 2026 election?", 48);
    let score = score(&poly, &kalshi);
    assert!(score.composite >= 0.55,
        "Unicode characters (accented) should not break matching");
}

#[test]
fn edge_number_in_different_formats() {
    // "$100k" vs "$100,000" vs "100000"
    let poly = make_market(Poly, "Will Bitcoin hit $100k?", 24);
    let kalshi = make_market(Kalshi, "Will Bitcoin hit $100,000?", 24);
    let score = score(&poly, &kalshi);
    // Ideally these would match via number normalization
    // For MVP, this is an acceptable false negative if number normalization is not implemented
    // AC: At minimum, this should not crash
}

#[test]
fn edge_close_time_missing_or_far_future() {
    // Market with close time 1 year away vs 1 day away
    let poly = make_market(Poly, "Will Bitcoin hit $100k?", 8760); // 1 year
    let kalshi = make_market(Kalshi, "Will Bitcoin hit $100k?", 24); // 1 day
    let score = score(&poly, &kalshi);
    // Text should match perfectly, but time_score = 0 (>168h diff)
    // composite = 0.65 * text_score + 0.25 * 0.0 + 0.10 * jw
    // Should still be NeedsReview at minimum from text alone
    assert!(score.composite >= 0.55,
        "Time difference should not completely kill an otherwise perfect text match");
}
```

### 3.7 Pipeline Integration Tests

```rust
#[test]
fn pipeline_no_false_positives_on_real_data() {
    // The full pipeline should produce zero matches for these known-bad pairs
    let poly_markets = vec![
        make_market(Poly, "US x Iran meeting by April 10, 2026?", 24),
        make_market(Poly, "Will Elon Musk post 260-279 tweets this week?", 24),
        make_market(Poly, "Will Carlos Alvarez win the 2026 Peruvian presidential election?", 24),
        make_market(Poly, "Will Ricardo Belmont win the 2026 Peruvian presidential election?", 24),
    ];
    let kalshi_markets = vec![
        make_market(Kalshi, "Shiba Inu price range on Apr 10, 2026?", 24),
        make_market(Kalshi, "Will the temp in NYC be above 36.99 degrees?", 24),
    ];

    let pipeline = MatchPipeline::default();
    let results = pipeline.find_matches(&poly_markets, &kalshi_markets);

    assert!(results.is_empty(),
        "Pipeline should produce ZERO matches for these unrelated markets, got {} matches", results.len());
}

#[test]
fn pipeline_finds_true_match_among_noise() {
    // Mix of unrelated markets with one true match hidden in there
    let poly_markets = vec![
        make_market_with_id(Poly, "Will Bitcoin hit $100k?", 24, "poly-btc"),
        make_market_with_id(Poly, "US x Iran meeting by April 10, 2026?", 24, "poly-iran"),
        make_market_with_id(Poly, "Will Elon Musk post 260-279 tweets?", 24, "poly-musk"),
    ];
    let kalshi_markets = vec![
        make_market_with_id(Kalshi, "Will Bitcoin hit $100k?", 24, "kalshi-btc"),
        make_market_with_id(Kalshi, "Shiba Inu price range on Apr 10?", 24, "kalshi-shib"),
        make_market_with_id(Kalshi, "Will the temp in NYC be above 36.99?", 24, "kalshi-weather"),
    ];

    let pipeline = MatchPipeline::default();
    let results = pipeline.find_matches(&poly_markets, &kalshi_markets);

    assert_eq!(results.len(), 1, "Should find exactly 1 match (Bitcoin)");
    assert_eq!(results[0].poly_market.platform_id, "poly-btc");
    assert_eq!(results[0].kalshi_market.platform_id, "kalshi-btc");
    assert!(results[0].score.composite >= 0.80, "Bitcoin match should be AutoVerified");
}

#[test]
fn pipeline_performance_200x200() {
    // Generate 200 poly + 200 kalshi markets with diverse topics
    let topics = ["Bitcoin", "Ethereum", "Trump", "Super Bowl", "NYC temperature",
                  "S&P 500", "SpaceX launch", "Oscar winner", "Fed rate", "GDP growth"];
    let poly: Vec<Market> = (0..200)
        .map(|i| make_market_with_id(
            Poly,
            &format!("Will {} event #{} happen?", topics[i % topics.len()], i),
            24 + (i as i64 % 168),
            &format!("poly-{i}"),
        ))
        .collect();
    let kalshi: Vec<Market> = (0..200)
        .map(|i| make_market_with_id(
            Kalshi,
            &format!("Will {} event #{} happen?", topics[i % topics.len()], i),
            24 + (i as i64 % 168),
            &format!("kalshi-{i}"),
        ))
        .collect();

    let pipeline = MatchPipeline::default();
    let start = std::time::Instant::now();
    let _results = pipeline.find_matches(&poly, &kalshi);
    let elapsed = start.elapsed();

    assert!(elapsed.as_millis() < 100,
        "200x200 must complete in <100ms, took {}ms", elapsed.as_millis());
}
```

---

## 4. Stop Word List (Complete)

The following words MUST be treated as stop words and removed during token extraction:

```
a, an, the, will, be, is, are, was, were, do, does, did, has, have, had,
of, in, on, at, to, for, by, or, and, not, if, it, its, this, that,
with, from, as, but, than, before, after, yes, no, market,
above, below, between, could, would, should, can, may, might,
been, being, get, got, goes, going, about, over, under, more, less,
how, many, much, what, which, when, where, who, whom, whose, why,
any, each, every, some, most, other, also, just, only, very, so,
up, down, out, then, there, here, into, through, during, against
```

The following words are **explicitly NOT stop words** (they carry domain meaning):
- "price", "win", "lose", "hit", "reach", "election", "temperature", "rate"
- All entity names (handled separately)
- All numbers (handled separately)

---

## 5. Entity Alias Table (Minimum Required)

The following alias mappings MUST be implemented in the first version:

| Alias(es) | Canonical form |
|-----------|---------------|
| btc | bitcoin |
| eth | ethereum |
| sol | solana |
| doge | dogecoin |
| ada | cardano |
| dot | polkadot |
| xrp | ripple |
| us, usa, u.s., u.s.a. | united states |
| uk, u.k. | united kingdom |
| nyc | new york city |
| la | los angeles |
| sf | san francisco |
| gdp | gross domestic product |
| cpi | consumer price index |
| fed | federal reserve |
| sec | securities and exchange commission |
| nfl | national football league |
| nba | national basketball association |
| mlb | major league baseball |
| ufc | ultimate fighting championship |

---

## 6. Category Keyword Table (Minimum Required)

Each category MUST be detected by the presence of at least one keyword from its list:

| Category | Keywords |
|----------|----------|
| Crypto | bitcoin, btc, ethereum, eth, solana, sol, dogecoin, doge, shiba, crypto, token, nft, defi, altcoin, xrp, cardano, polkadot, chainlink, blockchain, binance, coinbase |
| Politics | election, president, congress, senate, vote, poll, government, democrat, republican, minister, parliament, legislation, governor, mayor, political, diplomat, treaty, sanction |
| Sports | super bowl, nba, nfl, mlb, nhl, ufc, championship, tournament, playoff, finals, match, game, league, team, coach, player, season, cup, olympic |
| Weather | temperature, temp, weather, fahrenheit, celsius, hurricane, tornado, rainfall, snowfall, heatwave, climate, degrees, forecast, storm |
| Economics | gdp, inflation, interest rate, fed, federal reserve, unemployment, jobs, cpi, ppi, treasury, yield, s&p, sp500, nasdaq, dow, recession, growth, deficit, debt |
| Entertainment | oscar, emmy, grammy, box office, movie, film, album, song, billboard, streaming, netflix, disney, broadway, concert, award |
| Science | nasa, spacex, launch, asteroid, earthquake, volcano, research, discovery, vaccine, pandemic, virus |
| Other | (default when no keywords match) |

---

## 7. File Deliverables

| File | Status | Contents |
|------|--------|----------|
| `crates/arb-matcher/src/normalize.rs` | Rewrite | Expanded stop words, `ClassifiedTokens` struct, `classify_tokens()`, alias normalization |
| `crates/arb-matcher/src/category.rs` | New | `MarketCategory` enum, `classify()` function, keyword tables |
| `crates/arb-matcher/src/scorer.rs` | Rewrite | `jaccard_similarity()`, `weighted_overlap()`, composite scoring with new formula |
| `crates/arb-matcher/src/pipeline.rs` | Modify | Category pre-filter stage, entity overlap gate stage, updated scoring call |
| `crates/arb-matcher/src/types.rs` | Modify | Expanded `MatchScore` struct, updated `decision()` thresholds |
| `crates/arb-matcher/src/lib.rs` | Modify | Add `pub mod category;` |

---

## 8. Out of Scope (Deferred)

- TF-IDF corpus-wide term weighting (can be added later as a refinement)
- ML embedding-based matching (requires ONNX runtime)
- LLM verification of NeedsReview candidates (can be added as post-pipeline step)
- Number format normalization ($100k = $100,000 = 100000) -- nice to have but not required for MVP
- Multi-language support
- Fuzzy entity matching (Levenshtein on entity names) -- only exact + alias matching required for MVP
