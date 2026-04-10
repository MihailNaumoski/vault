# Matcher Redesign: Trading Review

**Reviewer**: Quant Strategist (Trading Team)
**Date**: 2026-04-09
**Documents Reviewed**:
- `specs/matcher-redesign-plan.md` (Architecture Plan)
- `specs/matcher-redesign-spec.md` (Detailed Spec + Acceptance Criteria)
- `crates/arb-matcher/src/` (Current implementation: normalize.rs, scorer.rs, pipeline.rs, types.rs)

**Verdict**: **APPROVE WITH CHANGES**

---

## 1. Would This Actually Find Correct Matches?

### Test Scenario: Bitcoin $100k across platforms

- Polymarket: "Will Bitcoin hit $100k by December 2025?"
- Kalshi: "Bitcoin above $100,000 on December 31, 2025?"

**Category filter**: Both contain "bitcoin" -> both classified as Crypto. PASS.

**Entity extraction**: "Bitcoin" detected by dictionary in both. PASS -- shared entity = 1.

**Token scoring walkthrough**:
- Poly tokens (after stop removal): {"bitcoin", "hit", "100k", "december", "2025"}
- Kalshi tokens (after stop removal): {"bitcoin", "100", "000", "december", "31", "2025"}
- **CONCERN**: "$100k" normalizes to "100k", but "$100,000" normalizes to "100 000" (two tokens after punctuation stripping). These will NOT match as identical tokens. The spec explicitly defers number normalization ($100k = $100,000) to "Out of Scope" (Section 8). This is a **real false negative risk** for one of the most common market patterns.

**Recommendation**: At minimum, add a normalization rule that strips trailing "k" and appends "000" (so "100k" -> "100000") and collapses comma-separated digit groups ("100,000" -> "100000"). This is not ML -- it is a simple regex. Without this, many legitimate crypto/economics matches will fail because platforms format dollar amounts differently.

### Verdict on core matching: The architecture is sound. The multi-stage funnel (category -> entity -> token scoring) is the right approach. It will find correct matches when questions are phrased similarly. The main gap is number format divergence.

---

## 2. False Positive Protection Assessment

### Known bad pair analysis:

| Pair | Category Gate | Entity Gate | Would Block? |
|------|--------------|-------------|-------------|
| "US x Iran meeting" vs "Shiba Inu price range" | Politics vs Crypto | {"us","iran"} vs {"shiba inu"} | YES (double blocked) |
| "Elon Musk tweets" vs "NYC temperature" | Other/Entertainment vs Weather | {"elon musk"} vs {"nyc"} | YES (double blocked) |
| "Peruvian election" vs "Shiba Inu" | Politics vs Crypto | {"carlos alvarez","peruvian"} vs {"shiba inu"} | YES (double blocked) |
| "Belmont Peru election" vs "Shiba Inu" | Politics vs Crypto | {"ricardo belmont","peruvian"} vs {"shiba inu"} | YES (double blocked) |

**Assessment**: All four known false positives are killed by BOTH gates independently. This is excellent defense-in-depth. Even if one gate has a bug, the other catches it.

**Additional false positive scenarios to consider**:
- "Will Bitcoin hit $100k?" vs "Will Bitcoin hit $50k?" -- Same category, same entity. This passes both gates and goes to scoring. Tokens: {"bitcoin","hit","100k"} vs {"bitcoin","hit","50k"}. Jaccard = 2/4 = 0.50. Weighted overlap would be moderate (entity "bitcoin" matches, keyword "hit" matches, but numbers differ). The spec has a test case for this (edge_same_topic_different_threshold) requiring it NOT AutoVerify. **This is borderline** -- need to verify the composite math lands below 0.80.
- "Will the Super Bowl happen in 2026?" vs "Will the NBA finals happen in 2026?" -- Same category (Sports), entities might both be empty if "super bowl" and "nba" are treated as keywords not entities. Keywords: {"super","bowl","happen","2026"} vs {"nba","finals","happen","2026"}. Jaccard = 2/6 = 0.33. Should be rejected. PASS.

**Verdict on FP protection**: Strong. The dual-gate architecture is robust.

---

## 3. Edge Cases from a Trading Perspective

### 3a. Same topic, different resolution date

- "Bitcoin above $100k by Dec 2025" vs "Bitcoin above $100k by March 2026"
- These are DIFFERENT contracts with DIFFERENT payoffs. A trader who assumes they are equivalent will lose money.
- The plan relies on close_time_score (Stage 5) to differentiate. Time weight is only 25% of composite. If text_score is very high (say 0.90), composite = 0.65 * 0.90 + 0.25 * 0.0 + 0.10 * 0.85 = 0.585 + 0.0 + 0.085 = 0.67 (NeedsReview).
- **This is acceptable** -- it lands in NeedsReview, not AutoVerified. Human review catches the date difference.
- **CONCERN**: If close times happen to be within 7 days of each other (e.g., "by end of March" vs "on March 31"), the time score won't help. The text tokens would be nearly identical. This could AutoVerify incorrectly.
- **Recommendation**: Consider extracting date tokens specifically and comparing them. Two markets with identical non-date tokens but different date tokens should get a penalty. The current design just ignores this -- dates are low-weight (0.5) in the weighted overlap, but they still contribute positively even when different months appear.

### 3b. Same topic, different threshold

- "Bitcoin above $100k" vs "Bitcoin above $90k"
- Numbers "100k" vs "90k" are different tokens. Jaccard on {"bitcoin","hit","100k"} vs {"bitcoin","hit","90k"} = 2/4 = 0.50.
- Weighted overlap: entity "bitcoin" (3.0), keyword "hit" (2.0), numbers differ. Shared weight = 5.0 out of union weight ~8.0 = 0.625.
- text_score = 0.60 * 0.625 + 0.40 * 0.50 = 0.375 + 0.20 = 0.575.
- composite = 0.65 * 0.575 + 0.25 * 1.0 + 0.10 * ~0.90 = 0.374 + 0.25 + 0.09 = 0.714.
- **This is NeedsReview (0.714 >= 0.55, < 0.80)**. The human reviewer would catch the threshold difference. Acceptable.
- **However**, if we add number normalization (recommended above), "100k" -> "100000" and "90k" -> "90000" would still differ. Good.

### 3c. Multi-outcome events

- Polymarket: "Who will win the 2026 Peruvian election?" (multi-outcome, one market with shares for each candidate)
- Kalshi: "Will Carlos Alvarez win the 2026 Peruvian election?" (binary yes/no)
- These are RELATED but NOT equivalent contracts. The Polymarket "Alvarez" share is equivalent to the Kalshi binary, but the Polymarket market as a whole is not.
- **CONCERN**: The spec does not address how Polymarket multi-outcome markets are decomposed. If Polymarket sends the umbrella question "Who will win?" it will share entities ("peruvian") and keywords ("election","2026") with the Kalshi binary. Jaccard would be moderate. This could produce a NeedsReview match that is misleading.
- **Recommendation**: The ingestion layer (not the matcher) should decompose Polymarket multi-outcome markets into individual binary positions before matching. Each Polymarket outcome share ("Alvarez: Yes", "Belmont: Yes") should be matched independently against Kalshi binaries. If this decomposition already happens upstream, document it in the spec. If not, add it as a known limitation.

### 3d. Negation blindness

- "Will Bitcoin hit $100k?" vs "Will Bitcoin NOT hit $100k?"
- After stop word removal, "not" is removed (it is in the stop list). Both become {"bitcoin","hit","100k"}.
- Jaccard = 1.0. This would AutoVerify.
- **CRITICAL CONCERN**: These are OPPOSITE contracts. Matching them as equivalent and trading on the spread would guarantee a loss (you'd be long YES on both sides of the same question).
- **Recommendation**: Remove "not" from the stop word list. It is a semantically critical word in prediction markets. Alternatively, add a negation detection pass that flags questions with negation words and penalizes matches where one side has negation and the other does not.

### 3e. "By" vs "On" vs "Before" temporal semantics

- "Bitcoin above $100k BY December 31" (cumulative -- anytime before that date)
- "Bitcoin above $100k ON December 31" (point-in-time -- specifically that day)
- These are DIFFERENT contracts. "by" and "on" are currently stop words.
- **CONCERN**: Removing both from stop words would help but is insufficient -- they'd just become low-weight keywords. The deeper issue is that temporal prepositions change the contract meaning.
- **Recommendation**: Flag this as a known limitation. For MVP, the NeedsReview threshold should catch most of these (human verifies). Long-term, temporal semantics parsing would be valuable.

---

## 4. Missing Considerations

### 4a. Number format normalization (HIGH PRIORITY)

As noted in section 1. "$100k" vs "$100,000" vs "$100000" are the same number. Platforms use different formats. Without normalization, many legitimate matches in crypto and economics categories will fail. This is listed as "Out of Scope" but should be promoted to MVP scope. The implementation is trivial:
- Strip `$` and `,` characters
- Convert `(\d+)k` to `\1000`
- Convert `(\d+)m` to `\1000000`

### 4b. Negation handling (HIGH PRIORITY)

See section 3d. "not" must not be a stop word. At minimum, keep it as a keyword. Ideally, add negation detection.

### 4c. Entity dictionary gaps

The dictionary covers major crypto, countries, people, and sports. Missing categories:
- **Companies**: Apple, Tesla, Google, Amazon, Meta -- increasingly traded on prediction markets (earnings, stock price targets)
- **Central banks**: ECB, BOJ, BOE, PBOC -- rate decision markets
- **Geopolitical regions**: Middle East, EU, NATO, BRICS
- **Specific elections**: midterms, primaries, runoff -- these are keywords, not entities, but should be in the keyword list

The plan acknowledges this is non-exhaustive and the capitalization heuristic catches unknowns. Acceptable for MVP.

### 4d. Manual override / allowlist mechanism

The plan has no provision for manually forcing or blocking matches. In production:
- A trader might spot a match the algorithm missed and want to force it
- A false positive might slip through and need to be permanently blocked
- **Recommendation**: Add a `match_overrides` config table with `(poly_id, kalshi_id, action: Force|Block)` entries. The pipeline checks this before scoring. Low implementation cost, high operational value.

### 4e. Confidence calibration and monitoring

The plan lacks a feedback loop. After deployment:
- How do we know the thresholds (0.80, 0.55) are correct?
- What if market language shifts over time?
- **Recommendation**: Log all match decisions (including Rejected pairs above 0.40) with full score breakdowns. Periodically review NeedsReview outcomes after human verification to calibrate thresholds. This is operational, not code, but should be mentioned in the plan.

---

## 5. Threshold Assessment

### Current proposal:
- AutoVerified >= 0.80
- NeedsReview >= 0.55
- Rejected < 0.55

### Analysis:

The thresholds are applied to a composite score where text_score has 65% weight. A composite of 0.80 means:
- If time_score = 1.0 and jw_tiebreaker = 1.0: text_score needs to be >= (0.80 - 0.25 - 0.10) / 0.65 = 0.692
- If time_score = 0.5 and jw_tiebreaker = 0.8: text_score needs to be >= (0.80 - 0.125 - 0.08) / 0.65 = 0.915

So AutoVerified requires very high text similarity when time scores are moderate. This seems appropriately conservative.

### Should ANY matches be auto-verified for automated trading?

**Strong recommendation: NO, not initially.**

For the first phase of deployment, ALL matches should require human review (effectively: treat everything as NeedsReview or Rejected). Reasons:
1. The scoring system is untested on real production data at scale
2. A single false positive in automated trading means real money lost
3. The matcher's job is to surface candidates efficiently, not to make trading decisions
4. After 2-4 weeks of human-verified data, calibrate the AutoVerified threshold based on observed precision

**Recommendation**: Add a config flag `auto_verify_enabled: bool` (default: false). When false, treat AutoVerified same as NeedsReview (require human confirmation). Flip to true only after calibration proves the threshold has >99% precision.

### NeedsReview at 0.55:

This seems reasonable. Anything below 0.55 with the new scoring system would have very low token overlap. The question is whether this creates too large a review queue. With aggressive pre-filtering (category + entity gates), the number of NeedsReview candidates per run should be manageable (estimated 5-15 for 200x200 markets).

---

## 6. Specific Technical Concerns

### 6a. Category classification: "first match wins" order dependency

The spec says categories are checked in order and first match wins. This means:
- "Will the SEC approve a Bitcoin ETF?" contains both "bitcoin" (Crypto) and "sec" (if added to Economics/Politics).
- Depending on check order, it could be Crypto or Economics.
- If one platform phrases it as "Bitcoin ETF approval" (-> Crypto) and the other as "SEC ruling on BTC ETF" (-> could be Economics if SEC is checked first), they'd be in different categories and get blocked.

**Recommendation**: When a market matches multiple categories, assign the FIRST keyword-hit category but also store a secondary category. Allow matching if either the primary OR secondary categories match. This is a small extension to the `MarketCategory` model (add `Option<MarketCategory>` for secondary) but prevents false negatives on cross-domain topics.

### 6b. Entity extraction: capitalization heuristic fragility

The plan uses capitalization to detect entities. But:
- All-caps text ("WILL BITCOIN HIT $100K?") -- every word looks capitalized
- Sentence-start words ("Bitcoin hits...") -- "Bitcoin" is capitalized because it starts the sentence, which is correct here but would be wrong for "The price of Bitcoin..."
- Platform-specific formatting: some platforms lowercase everything or uppercase everything

**Recommendation**: The dictionary lookup should be the PRIMARY entity detection method, with capitalization as FALLBACK only. The spec seems to intend this but doesn't make the priority explicit.

### 6c. Jaccard on small token sets

With aggressive stop word removal, many questions reduce to 3-4 meaningful tokens. Jaccard on small sets is noisy:
- 3 tokens shared out of 4 total = 0.75
- 2 tokens shared out of 4 total = 0.50
- A single token difference swings Jaccard by 0.25

**Recommendation**: The weighted overlap (60% of text_score) mitigates this somewhat, but consider adding a length normalization factor. Alternatively, keep the Jaro-Winkler tiebreaker weight at 10-15% rather than 10% to smooth out Jaccard noise on short questions.

---

## 7. Summary of Recommendations

### Must-fix before implementation (HIGH):

| # | Issue | Impact | Effort |
|---|-------|--------|--------|
| 1 | Remove "not" from stop words | Prevents matching opposite contracts (guaranteed loss) | Trivial |
| 2 | Add basic number normalization ($100k = $100,000) | Prevents false negatives on most common market pattern | Small (regex) |
| 3 | Add `auto_verify_enabled` config flag, default false | Prevents automated trading on uncalibrated matcher | Small |

### Should-fix (MEDIUM):

| # | Issue | Impact | Effort |
|---|-------|--------|--------|
| 4 | Secondary category support for cross-domain markets | Prevents FN on SEC/Bitcoin-type markets | Medium |
| 5 | Match override table (force/block) | Operational safety valve | Small |
| 6 | Date token comparison penalty | Prevents same-subject-different-date AutoVerify | Medium |

### Nice-to-have (LOW):

| # | Issue | Impact | Effort |
|---|-------|--------|--------|
| 7 | Company entity dictionary (TSLA, AAPL, etc.) | Better coverage for stock/earnings markets | Small |
| 8 | Decision logging for threshold calibration | Long-term precision improvement | Small |
| 9 | Multi-outcome market decomposition documentation | Clarity on upstream requirements | Documentation only |

---

## 8. Final Verdict

**APPROVE WITH CHANGES**

The architecture is fundamentally sound. The multi-stage funnel (category gate -> entity gate -> token scoring) is a massive improvement over the current Jaro-Winkler approach. It correctly addresses all four known false positives and would dramatically improve match quality.

The three HIGH-priority items must be addressed before implementation:
1. **Negation blindness** is a trading-safety issue -- matching "will X" with "will NOT X" could cause direct financial loss.
2. **Number normalization** is needed to avoid false negatives on the most common class of prediction markets (price thresholds).
3. **AutoVerify should be disabled by default** until the system proves itself on real data.

With these three changes, the plan is ready for implementation.
