# Matcher Redesign: Research Handoff

**Research Team**: Doc Researcher + SDK Analyst
**Date**: 2026-04-09
**Purpose**: Implementation research for the 6-stage matcher pipeline

---

## 1. Current Codebase Analysis

### 1a. Existing File Structure (`crates/arb-matcher/src/`)

| File | Lines | Purpose | Change Needed |
|------|-------|---------|---------------|
| `normalize.rs` | 101 | Stop words (37), `normalize()`, `extract_tokens()`, `shared_token_count()` | **Major rewrite** — add token classification, entity recognition, alias normalization, number normalization |
| `scorer.rs` | 178 | `score()` / `score_normalized()` using `strsim::jaro_winkler`, weights: 70% text / 30% time | **Major rewrite** — replace Jaro-Winkler primary with Jaccard + weighted overlap |
| `pipeline.rs` | 369 | `MatchPipeline::find_matches()`, token pre-filter (`MIN_SHARED_TOKENS=1`), greedy dedup | **Moderate changes** — insert category pre-filter, entity overlap gate |
| `types.rs` | 43 | `MatchScore` (3 fields), `MatchCandidate`, `MatchDecision` enum, thresholds (0.85/0.50) | **Moderate changes** — expand `MatchScore` to ~9 fields, update thresholds (0.80/0.55) |
| `lib.rs` | 10 | Module declarations + re-exports | **Minor** — add `pub mod category;` |
| `store.rs` | 174 | `PairStore` for SQLite persistence via `arb-db` | **No changes** — scoring-agnostic |

### 1b. Current `normalize.rs` — What Exists

```rust
// 37 stop words — covers articles, prepositions, conjunctions, basic verbs
const STOP_WORDS: &[&str] = &[
    "a", "an", "the", "will", "be", "is", "are", "was", "were", "do", "does",
    "did", "has", "have", "had", "of", "in", "on", "at", "to", "for", "by",
    "or", "and", "not", "if", "it", "its", "this", "that", "with", "from",
    "as", "but", "than", "before", "after", "yes", "no", "market",
];
```

**Key observation**: `"not"` IS currently a stop word. The Trading review (HIGH priority #1) requires removing it — "not" changes contract semantics entirely ("Will X" vs "Will NOT X").

The current `normalize()` function:
1. Lowercases
2. Replaces non-alphanumeric chars with spaces (strips `$`, `,`, `.`, `?`, etc.)
3. Collapses whitespace

**Problem for number normalization**: `$100,000` becomes `100 000` (two tokens: "100" and "000"). And `$100k` becomes `100k` (one token). These won't match. The Trading review (HIGH priority #2) requires fixing this.

### 1c. Current `scorer.rs` — How Jaro-Winkler Is Used

```rust
const TEXT_WEIGHT: f64 = 0.70;
const TIME_WEIGHT: f64 = 0.30;
const MAX_TIME_DIFF_HOURS: f64 = 168.0; // 7 days

let text_similarity = strsim::jaro_winkler(norm_poly, norm_kalshi);
let composite = text_similarity * TEXT_WEIGHT + close_time_score * TIME_WEIGHT;
```

Single call to `strsim::jaro_winkler` on the full normalized question string. No token-level analysis. This is why it produces false positives — character-level similarity on two long strings with shared structure ("Will X by Y?") scores high regardless of different subjects.

### 1d. Current `types.rs` — MatchScore Structure

```rust
pub struct MatchScore {
    pub text_similarity: f64,    // Single Jaro-Winkler value
    pub close_time_score: f64,
    pub composite: f64,
}
```

Thresholds: AutoVerified >= 0.85, NeedsReview >= 0.50. Plan changes to 0.80 / 0.55.

### 1e. Current `pipeline.rs` — Flow

1. Pre-normalize all questions (outside O(n*m) loop)
2. Pre-extract tokens (outside O(n*m) loop)
3. For each (poly, kalshi) pair:
   - Token pre-filter: skip if `shared_tokens < 1`
   - Score with Jaro-Winkler
   - Keep if composite >= min_score
4. Sort descending by composite
5. Greedy dedup: each platform_id used at most once
6. Truncate to max_results

The dedup logic and overall flow are solid. The changes are additive: insert category pre-filter and entity gate between steps 2 and 3, replace the scorer in step 3.

---

## 2. Dependency Analysis

### 2a. Existing Dependencies (from `Cargo.toml`)

| Crate | Version | Used For | Keep? |
|-------|---------|----------|-------|
| `strsim` | 0.11 | `jaro_winkler()` | YES — demoted to 10% tiebreaker weight |
| `chrono` | 0.4 | Close-time math | YES — unchanged |
| `uuid` | 1.11 | Market/pair IDs | YES — unchanged |
| `serde` | 1.0 | Serialization | YES — unchanged |
| `tracing` | 0.1 | Logging | YES — unchanged |
| `arb-types` | workspace | `Market`, `Platform` types | YES — unchanged |
| `arb-db` | workspace | Repository trait | YES — unchanged |
| `toml` | 0.8 | Config parsing | YES — unchanged |
| `anyhow` | 1.0 | Error handling | YES — unchanged |
| `tokio` | 1.44 | Async runtime | YES — unchanged |
| `rust_decimal` | 1.36 | Decimal math | YES — unchanged |

### 2b. New Dependencies Needed

| Crate | Version | Purpose | Justification |
|-------|---------|---------|---------------|
| `regex` | 1.x | Number normalization (`$100k` -> `100000`, comma stripping) | Trading review HIGH #2 requires this. No regex in current deps. Simple patterns only. |

**That's it.** Only ONE new dependency needed. Everything else can be done with `std` library types.

### 2c. What `strsim` 0.11.1 Offers

The `strsim` crate provides these functions:

| Function | Returns | Operates On | Useful? |
|----------|---------|-------------|---------|
| `jaro_winkler(a, b)` | 0.0..1.0 | Characters | YES — keep as 10% tiebreaker |
| `jaro(a, b)` | 0.0..1.0 | Characters | No — Jaro-Winkler supersedes |
| `sorensen_dice(a, b)` | 0.0..1.0 | Character bigrams | **Possibly** — could use on entity names for fuzzy matching, but deferred |
| `normalized_levenshtein(a, b)` | 0.0..1.0 | Characters | No — edit distance, not similarity |
| `normalized_damerau_levenshtein(a, b)` | 0.0..1.0 | Characters | No — same issue |
| `hamming(a, b)` | Result<usize> | Characters (equal length) | No |

**Critical finding**: `strsim` does NOT have Jaccard similarity. Jaccard must be implemented manually. This is trivial — it's a set intersection / set union ratio on token sets. No crate needed.

**Critical finding**: `strsim` does NOT have token-level metrics. All its functions operate on character sequences. The weighted overlap scoring must be implemented from scratch. Again, this is straightforward with `HashSet` operations.

---

## 3. Implementation Patterns — Component by Component

### 3a. Stop Word List — Using `const` Slice

**Recommendation**: Keep as `const &[&str]` (current pattern). No crate needed.

```rust
/// Stop words — function words with no discriminating value.
/// IMPORTANT: "not" is intentionally EXCLUDED — it changes contract semantics.
/// See Trading Review item #1.
const STOP_WORDS: &[&str] = &[
    "a", "an", "the", "will", "be", "is", "are", "was", "were", "do", "does",
    "did", "has", "have", "had", "of", "in", "on", "at", "to", "for", "by",
    "or", "and", "if", "it", "its", "this", "that", "with", "from",
    "as", "but", "than", "before", "after", "yes", "no", "market",
    // Additions from spec:
    "above", "below", "between", "could", "would", "should", "can", "may", "might",
    "been", "being", "get", "got", "goes", "going", "about", "over", "under",
    "more", "less", "how", "many", "much", "what", "which", "when", "where",
    "who", "whom", "whose", "why", "any", "each", "every", "some", "most",
    "other", "also", "just", "only", "very", "so", "up", "down", "out",
    "then", "there", "here", "into", "through", "during", "against",
];
```

**Performance**: For ~80 words, a linear scan of a `&[&str]` slice is faster than a HashSet lookup due to cache locality and no hashing overhead. The breakeven point for HashSet vs. linear scan is typically around 20-30 items for short strings, but since we call this per-token (5-10 tokens per question), a `HashSet` pre-built once would also be fine. Either approach works.

**Recommendation**: Convert to a `HashSet<&str>` built once via `std::sync::LazyLock` (stable since Rust 1.80, we're on 1.94). This gives O(1) lookup per token.

```rust
use std::collections::HashSet;
use std::sync::LazyLock;

static STOP_WORDS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "a", "an", "the", "will", "be", /* ... full list ... */
    ].into_iter().collect()
});
```

### 3b. Entity Dictionary — `std::sync::LazyLock` + `HashMap`

**Options evaluated**:

| Approach | Pros | Cons | Verdict |
|----------|------|------|---------|
| `phf` crate (compile-time perfect hash) | Zero runtime init cost | Adds dependency, verbose macro syntax, harder to maintain | REJECT |
| `lazy_static!` macro | Well-known | Deprecated in favor of `std::sync::LazyLock` | REJECT |
| `once_cell::Lazy` | Widely used | External crate, `LazyLock` is the std equivalent now | REJECT |
| **`std::sync::LazyLock`** | No external deps, idiomatic, stable since 1.80 | First-access init (negligible for static data) | **RECOMMENDED** |

**Implementation pattern for ~200-entry entity dictionary**:

```rust
use std::collections::HashMap;
use std::sync::LazyLock;

/// Known entities mapped to their canonical form.
/// Used for: (1) entity detection, (2) alias normalization.
static ENTITY_DICT: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    let mut m = HashMap::with_capacity(200);
    // Crypto
    m.insert("bitcoin", "bitcoin");
    m.insert("btc", "bitcoin");
    m.insert("ethereum", "ethereum");
    m.insert("eth", "ethereum");
    m.insert("solana", "solana");
    m.insert("sol", "solana");
    m.insert("dogecoin", "dogecoin");
    m.insert("doge", "dogecoin");
    m.insert("shiba inu", "shiba inu");
    m.insert("shiba", "shiba inu");
    m.insert("xrp", "ripple");
    m.insert("ripple", "ripple");
    m.insert("cardano", "cardano");
    m.insert("ada", "cardano");
    m.insert("polkadot", "polkadot");
    m.insert("dot", "polkadot");
    m.insert("chainlink", "chainlink");
    // Countries
    m.insert("us", "united states");
    m.insert("usa", "united states");
    m.insert("united states", "united states");
    m.insert("uk", "united kingdom");
    m.insert("united kingdom", "united kingdom");
    m.insert("iran", "iran");
    m.insert("china", "china");
    m.insert("russia", "russia");
    m.insert("ukraine", "ukraine");
    m.insert("israel", "israel");
    m.insert("peru", "peru");
    m.insert("peruvian", "peru");
    m.insert("brazil", "brazil");
    // People
    m.insert("trump", "trump");
    m.insert("donald trump", "trump");
    m.insert("biden", "biden");
    m.insert("joe biden", "biden");
    m.insert("musk", "elon musk");
    m.insert("elon musk", "elon musk");
    m.insert("elon", "elon musk");
    // Cities
    m.insert("nyc", "new york city");
    m.insert("new york city", "new york city");
    m.insert("new york", "new york city");
    m.insert("la", "los angeles");
    m.insert("los angeles", "los angeles");
    m.insert("sf", "san francisco");
    m.insert("san francisco", "san francisco");
    // Orgs/Indices
    m.insert("fed", "federal reserve");
    m.insert("federal reserve", "federal reserve");
    m.insert("sec", "securities and exchange commission");
    m.insert("gdp", "gross domestic product");
    m.insert("cpi", "consumer price index");
    // Sports
    m.insert("nfl", "national football league");
    m.insert("nba", "national basketball association");
    m.insert("mlb", "major league baseball");
    m.insert("ufc", "ultimate fighting championship");
    m.insert("nhl", "national hockey league");
    // ... extend to ~200 entries
    m
});
```

**Multi-word entity detection**: The dictionary contains multi-word entries like "elon musk", "shiba inu", "new york city". The token classifier needs to check bigrams and trigrams against the dictionary, not just unigrams.

**Recommended approach**: After splitting into tokens, scan for multi-word matches using a sliding window:

```rust
fn detect_entities(tokens: &[&str]) -> Vec<String> {
    let mut entities = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        // Try trigram first, then bigram, then unigram
        let found = if i + 2 < tokens.len() {
            let tri = format!("{} {} {}", tokens[i], tokens[i+1], tokens[i+2]);
            ENTITY_DICT.get(tri.as_str()).map(|canon| (canon, 3))
        } else {
            None
        }.or_else(|| {
            if i + 1 < tokens.len() {
                let bi = format!("{} {}", tokens[i], tokens[i+1]);
                ENTITY_DICT.get(bi.as_str()).map(|canon| (canon, 2))
            } else {
                None
            }
        }).or_else(|| {
            ENTITY_DICT.get(tokens[i]).map(|canon| (canon, 1))
        });

        if let Some((canonical, consumed)) = found {
            entities.push(canonical.to_string());
            i += consumed;
        } else {
            i += 1;
        }
    }
    entities
}
```

**Performance**: HashMap lookup is O(1) amortized. For a 10-token question checking trigrams + bigrams + unigrams: ~30 lookups max. Negligible.

### 3c. Category Classification — Keyword Scan with `&[&str]` Arrays

**Pattern**: Static arrays of keywords per category, scan tokens for membership.

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

struct CategoryRule {
    category: MarketCategory,
    keywords: &'static [&'static str],
}

static CATEGORY_RULES: &[CategoryRule] = &[
    CategoryRule {
        category: MarketCategory::Crypto,
        keywords: &["bitcoin", "btc", "ethereum", "eth", "solana", "sol",
                    "dogecoin", "doge", "shiba", "crypto", "token", "nft",
                    "defi", "altcoin", "xrp", "cardano", "polkadot",
                    "chainlink", "blockchain", "binance", "coinbase"],
    },
    CategoryRule {
        category: MarketCategory::Politics,
        keywords: &["election", "president", "congress", "senate", "vote",
                    "poll", "government", "democrat", "republican", "minister",
                    "parliament", "legislation", "governor", "mayor",
                    "political", "diplomat", "treaty", "sanction"],
    },
    // ... remaining categories
];

pub fn classify(tokens: &[String]) -> MarketCategory {
    for rule in CATEGORY_RULES {
        if tokens.iter().any(|t| rule.keywords.contains(&t.as_str())) {
            return rule.category;
        }
    }
    MarketCategory::Other
}
```

**Trading review note (MEDIUM #4)**: Consider supporting a secondary category for cross-domain markets like "SEC approves Bitcoin ETF". Implementation: return `(MarketCategory, Option<MarketCategory>)`. Match if either primary or secondary overlaps.

```rust
pub fn classify_with_secondary(tokens: &[String]) -> (MarketCategory, Option<MarketCategory>) {
    let mut primary = None;
    let mut secondary = None;
    for rule in CATEGORY_RULES {
        if tokens.iter().any(|t| rule.keywords.contains(&t.as_str())) {
            if primary.is_none() {
                primary = Some(rule.category);
            } else if secondary.is_none() && Some(rule.category) != primary {
                secondary = Some(rule.category);
                break; // Only need two
            }
        }
    }
    (primary.unwrap_or(MarketCategory::Other), secondary)
}
```

### 3d. Jaccard Similarity — Manual Implementation

**No crate needed.** `strsim` does not provide token-level Jaccard. Implement with `HashSet`:

```rust
use std::collections::HashSet;

pub fn jaccard_similarity(a: &[String], b: &[String]) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 0.0; // Both empty = no signal, not perfect match
    }
    let set_a: HashSet<&str> = a.iter().map(|s| s.as_str()).collect();
    let set_b: HashSet<&str> = b.iter().map(|s| s.as_str()).collect();
    let intersection = set_a.intersection(&set_b).count();
    let union = set_a.union(&set_b).count();
    if union == 0 {
        return 0.0;
    }
    intersection as f64 / union as f64
}
```

**Performance**: HashSet construction is O(n) for n tokens. Intersection/union are O(min(n,m)). For 5-10 tokens per question, this is sub-microsecond.

### 3e. Weighted Token Overlap — Manual Implementation

```rust
/// Token class weights for weighted overlap calculation.
const ENTITY_WEIGHT: f64 = 3.0;
const KEYWORD_WEIGHT: f64 = 2.0;
const NUMBER_WEIGHT: f64 = 1.5;
const DATE_WEIGHT: f64 = 0.5;

pub fn weighted_overlap(a: &ClassifiedTokens, b: &ClassifiedTokens) -> f64 {
    let mut shared_weight = 0.0;
    let mut total_weight = 0.0;

    // Helper: count shared and total for a token class
    fn accumulate(
        a_tokens: &[String], b_tokens: &[String], weight: f64,
        shared: &mut f64, total: &mut f64,
    ) {
        let set_a: HashSet<&str> = a_tokens.iter().map(|s| s.as_str()).collect();
        let set_b: HashSet<&str> = b_tokens.iter().map(|s| s.as_str()).collect();
        let intersection = set_a.intersection(&set_b).count();
        let union = set_a.union(&set_b).count();
        *shared += intersection as f64 * weight;
        *total += union as f64 * weight;
    }

    accumulate(&a.entities, &b.entities, ENTITY_WEIGHT, &mut shared_weight, &mut total_weight);
    accumulate(&a.keywords, &b.keywords, KEYWORD_WEIGHT, &mut shared_weight, &mut total_weight);
    accumulate(&a.numbers, &b.numbers, NUMBER_WEIGHT, &mut shared_weight, &mut total_weight);
    accumulate(&a.dates, &b.dates, DATE_WEIGHT, &mut shared_weight, &mut total_weight);

    if total_weight == 0.0 { 0.0 } else { shared_weight / total_weight }
}
```

### 3f. Number Normalization — `regex` Crate

**This is the one new dependency.** The Trading review marked this HIGH priority.

```rust
use std::sync::LazyLock;
use regex::Regex;

/// Normalize number formats: strip $ and commas, expand k/m/b suffixes.
/// "$100k" -> "100000", "$1.5m" -> "1500000", "$100,000" -> "100000"
static RE_DOLLAR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\$").unwrap());
static RE_COMMA_IN_NUMBER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\d),(\d)").unwrap());
static RE_K_SUFFIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\d+(?:\.\d+)?)k\b").unwrap());
static RE_M_SUFFIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\d+(?:\.\d+)?)m\b").unwrap());
static RE_B_SUFFIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\d+(?:\.\d+)?)b\b").unwrap());

pub fn normalize_numbers(text: &str) -> String {
    let text = RE_DOLLAR.replace_all(text, "");
    let text = RE_COMMA_IN_NUMBER.replace_all(&text, "$1$2"); // "100,000" -> "100000"

    // Expand suffixes: "100k" -> "100000"
    let text = RE_K_SUFFIX.replace_all(&text, |caps: &regex::Captures| {
        expand_suffix(&caps[1], 1_000.0)
    });
    let text = RE_M_SUFFIX.replace_all(&text, |caps: &regex::Captures| {
        expand_suffix(&caps[1], 1_000_000.0)
    });
    let text = RE_B_SUFFIX.replace_all(&text, |caps: &regex::Captures| {
        expand_suffix(&caps[1], 1_000_000_000.0)
    });

    text.into_owned()
}

fn expand_suffix(num_str: &str, multiplier: f64) -> String {
    if let Ok(n) = num_str.parse::<f64>() {
        format!("{}", (n * multiplier) as u64)
    } else {
        num_str.to_string()
    }
}
```

**Test cases**:
- `"$100k"` -> `"100000"`
- `"$1.5m"` -> `"1500000"`
- `"$100,000"` -> `"100000"`
- `"100000"` -> `"100000"` (no change)
- `"36.99"` -> `"36.99"` (no suffix, no change)

**Alternative without regex**: Could use manual string scanning with `str::find()` and `str::replace()`. This avoids the `regex` dependency but is harder to maintain and more error-prone. Given that `regex` is a well-maintained, widely-used crate with excellent performance (compiles patterns to DFAs), the dependency is justified for the reliability and readability it provides.

**Performance**: Compiled `Regex` objects are cached in `LazyLock` statics. Each `replace_all` call is O(n) where n is string length. For ~50-char questions, this is negligible.

### 3g. Token Classification — The `ClassifiedTokens` Struct

```rust
#[derive(Debug, Clone, Default)]
pub struct ClassifiedTokens {
    pub entities: Vec<String>,     // "bitcoin", "iran", "elon musk"
    pub numbers: Vec<String>,      // "100000", "36.99", "5000"
    pub dates: Vec<String>,        // "april", "2026", "december"
    pub keywords: Vec<String>,     // "election", "hit", "price"
    pub all_meaningful: Vec<String>, // Union of above for Jaccard
}
```

**Classification logic order**:
1. Run number normalization on the raw question BEFORE lowercasing/tokenizing
2. Lowercase and tokenize
3. Multi-word entity detection (trigram/bigram/unigram sliding window against ENTITY_DICT)
4. For remaining tokens not consumed by entity detection:
   - Check if stop word -> discard
   - Check if number (all digits, or digits with `.`) -> classify as Number
   - Check if date-like (month names, years 2020-2035) -> classify as Date
   - Otherwise -> classify as Keyword
5. Build `all_meaningful` as the union of entities + numbers + dates + keywords

**Date detection without NLP**: Use a static set of month names and abbreviations:

```rust
static MONTHS: &[&str] = &[
    "january", "february", "march", "april", "may", "june",
    "july", "august", "september", "october", "november", "december",
    "jan", "feb", "mar", "apr", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];

fn is_date_token(token: &str) -> bool {
    MONTHS.contains(&token) || is_year(token)
}

fn is_year(token: &str) -> bool {
    token.len() == 4
        && token.chars().all(|c| c.is_ascii_digit())
        && matches!(token.parse::<u32>(), Ok(y) if (2020..=2035).contains(&y))
}
```

### 3h. Capitalization Heuristic for Unknown Entities

The spec calls for detecting proper nouns via capitalization in the original text (before lowercasing). Implementation:

```rust
/// Extract capitalized word sequences from the original (pre-lowercase) text.
/// Sentence-start words are excluded via a simple heuristic.
pub fn extract_capitalized_entities(original: &str) -> Vec<String> {
    let words: Vec<&str> = original.split_whitespace().collect();
    let mut entities = Vec::new();
    let mut current_entity = Vec::new();

    for (idx, word) in words.iter().enumerate() {
        // Strip punctuation for capitalization check
        let clean: String = word.chars().filter(|c| c.is_alphabetic()).collect();
        if clean.is_empty() {
            flush_entity(&mut current_entity, &mut entities);
            continue;
        }

        let first_char = clean.chars().next().unwrap();
        let is_capitalized = first_char.is_uppercase();
        let is_sentence_start = idx == 0
            || words.get(idx - 1).map_or(false, |prev| prev.ends_with('.') || prev.ends_with('?') || prev.ends_with('!'));

        if is_capitalized && !is_sentence_start {
            current_entity.push(clean.to_lowercase());
        } else {
            flush_entity(&mut current_entity, &mut entities);
        }
    }
    flush_entity(&mut current_entity, &mut entities);
    entities
}

fn flush_entity(current: &mut Vec<String>, entities: &mut Vec<String>) {
    if !current.is_empty() {
        entities.push(current.join(" "));
        current.clear();
    }
}
```

**Trading review note (6b)**: Dictionary lookup should be PRIMARY, capitalization as FALLBACK. The implementation should:
1. First pass: scan all tokens against ENTITY_DICT (catches "bitcoin", "btc", "iran", etc. even when lowercased)
2. Second pass: for tokens NOT matched by dictionary, use capitalization heuristic on original text
3. Merge results, dedup

### 3i. Composite Score Formula

Per the plan, with Trading review adjustments:

```rust
const TEXT_WEIGHT: f64 = 0.65;
const TIME_WEIGHT: f64 = 0.25;
const JW_TIEBREAKER_WEIGHT: f64 = 0.10;

// text_score = 0.60 * weighted_overlap + 0.40 * jaccard
let text_score = 0.60 * weighted_overlap + 0.40 * jaccard;

// Apply minimum token threshold (AC-6)
let text_score = if shared_meaningful_tokens < 2 { 0.0 } else { text_score };

let jw = strsim::jaro_winkler(&norm_poly, &norm_kalshi);

let composite = TEXT_WEIGHT * text_score + TIME_WEIGHT * time_score + JW_TIEBREAKER_WEIGHT * jw;
```

---

## 4. Crates NOT Needed (Evaluated and Rejected)

| Crate | Why Considered | Why Rejected |
|-------|---------------|-------------|
| `nlprule` | Tokenization, POS tagging, lemmatization | Way too heavy (~100MB model files). We need simple tokenization only. |
| `rust-stemmers` | Word stemming (running -> run) | Adds complexity, not needed — prediction market questions use simple forms. If "hit" vs "reach" matters, stemming won't help (they stem to different roots). |
| `unicode-segmentation` | Unicode-aware word segmentation | Overkill — `split_whitespace()` handles our ASCII-dominated market questions. |
| `rust-bert` / `rust-tokenizers` | ML-based NLP | Violates "no ML" constraint. |
| `phf` | Compile-time perfect hash maps | `std::sync::LazyLock` + `HashMap` is simpler, no extra dependency, same performance for our scale (~200 entries). |
| `once_cell` / `lazy_static` | Lazy initialization | Superseded by `std::sync::LazyLock` (stable since Rust 1.80, we're on 1.94). |
| `rsnltk` | Rust NLP toolkit | Python-dependent, heavy, unsuitable for our pure-Rust constraint. |
| `intent-classifier` | Rule-based intent classification | Too generic for our narrow keyword-based category classification. Our approach is simpler and more transparent. |

---

## 5. Gotchas and Performance Concerns

### 5a. Multi-Word Entity Matching Order

When scanning for entities like "elon musk" vs. just "elon", the sliding window MUST try longer n-grams first (trigram -> bigram -> unigram). Otherwise "elon" matches alone and "musk" becomes an orphaned keyword.

### 5b. Entity Dictionary Key Collisions

Some short tokens are ambiguous:
- "sol" = Solana (crypto) or sol (sun/currency)
- "ada" = Cardano (crypto) or a person's name
- "la" = Los Angeles or Spanish article
- "dot" = Polkadot (crypto) or punctuation/generic word
- "may" = month or modal verb

**Mitigation**: The category classification runs independently and provides context. If a market is categorized as Crypto, "sol" is likely Solana. For MVP, accept the ambiguity — the entity dictionary is a precision booster, not a sole decision-maker. The combination of category gate + entity gate + token scoring handles edge cases.

### 5c. Regex Compilation Cost

`Regex::new()` is expensive (microseconds to milliseconds). MUST compile once and cache in `LazyLock` statics. NEVER compile inside the hot loop. The patterns in section 3f above already use this pattern.

### 5d. String Allocation in the Hot Path

The current `normalize()` allocates a new String per question. With the new pipeline, each question gets:
- 1 String from `normalize()`
- 1 String from `normalize_numbers()`
- 1 Vec<String> for tokens
- 1 `ClassifiedTokens` struct with 5 Vecs

All of these are computed ONCE per market (outside the O(n*m) loop), so the allocations are O(n+m), not O(n*m). This is fine.

Inside the O(n*m) loop, the category check is O(1) (pre-computed), the entity overlap check uses references (no allocation), and the Jaccard/weighted overlap build temporary `HashSet`s. These are small (5-10 elements) and stack-friendly.

### 5e. `HashSet` vs. Sorted Vec for Small Collections

For entity overlap checking (typically 1-5 entities per market), a sorted `Vec` with binary search might be faster than `HashSet` due to lower overhead. However, the difference is negligible at this scale. Use `HashSet` for clarity.

### 5f. Threshold Sensitivity

The plan's thresholds (AutoVerified >= 0.80, NeedsReview >= 0.55) are tuned for the new scoring formula. They MUST NOT be used with the old Jaro-Winkler scorer. The Trading review recommends `auto_verify_enabled: bool` config flag (default: false) to prevent automated trading on uncalibrated scores.

---

## 6. Concrete Implementation Checklist

### Phase 1: New Files and Data Structures

- [ ] **Add `regex` to workspace `Cargo.toml`**: `regex = "1.11"` (latest stable)
- [ ] **Add `regex` to `arb-matcher/Cargo.toml`**: `regex = { workspace = true }`
- [ ] **Create `category.rs`**: `MarketCategory` enum, `CATEGORY_RULES` static, `classify()` function, `classify_with_secondary()` for cross-domain support
- [ ] **Add `pub mod category;` to `lib.rs`**

### Phase 2: Rewrite `normalize.rs`

- [ ] **Remove "not" from stop words** (Trading HIGH #1)
- [ ] **Expand stop words** to ~80 words per spec (section 4)
- [ ] **Add number normalization** with `regex` (Trading HIGH #2): `normalize_numbers()` function
- [ ] **Add `ClassifiedTokens` struct** with entities, numbers, dates, keywords, all_meaningful fields
- [ ] **Add `ENTITY_DICT`** as `LazyLock<HashMap<&str, &str>>` with ~200 entries
- [ ] **Add `classify_tokens()` function** that returns `ClassifiedTokens`:
  - Number normalization first
  - Lowercase + strip punctuation
  - Multi-word entity detection (sliding window: trigram -> bigram -> unigram)
  - Capitalization heuristic for unknown entities (fallback)
  - Stop word filtering
  - Number/date/keyword classification for remaining tokens
- [ ] **Keep `normalize()` function** for backward compatibility (scorer still uses it for Jaro-Winkler tiebreaker)
- [ ] **Update `extract_tokens()`** to use new stop word list
- [ ] **Add tests**: entity extraction, alias normalization, number normalization, stop word changes

### Phase 3: Rewrite `scorer.rs`

- [ ] **Add `jaccard_similarity()` function** operating on token slices
- [ ] **Add `weighted_overlap()` function** using token class weights (entity=3.0, keyword=2.0, number=1.5, date=0.5)
- [ ] **Update `score()` / `score_normalized()`** to:
  - Accept `ClassifiedTokens` for both markets
  - Compute Jaccard on `all_meaningful` tokens
  - Compute weighted overlap on classified tokens
  - Compute text_score = 0.60 * weighted_overlap + 0.40 * jaccard
  - Apply minimum shared token threshold (>= 2 for non-zero text_score)
  - Keep Jaro-Winkler as 10% tiebreaker
  - Compute composite = 0.65 * text_score + 0.25 * time_score + 0.10 * jw
- [ ] **Update `is_candidate()` threshold** from 0.50 to 0.55
- [ ] **Add tests**: Jaccard on identical/disjoint/partial sets, weighted overlap ordering, composite math

### Phase 4: Update `types.rs`

- [ ] **Expand `MatchScore` struct** to include: jaccard, weighted_overlap, text_score, jaro_winkler, close_time_score, composite, shared_entities, shared_tokens, category_match
- [ ] **Update `decision()` thresholds**: AutoVerified >= 0.80, NeedsReview >= 0.55
- [ ] **Add `auto_verify_enabled` field** to `MatchPipeline` config (Trading HIGH #3), default false
- [ ] **Update all existing tests** that construct `MatchScore` manually

### Phase 5: Update `pipeline.rs`

- [ ] **Pre-compute `ClassifiedTokens`** for all markets (outside O(n*m) loop)
- [ ] **Pre-compute `MarketCategory`** for all markets (outside O(n*m) loop)
- [ ] **Add Stage 2: Category pre-filter** — skip if categories differ (unless one is Other). With secondary category support: skip only if no primary/secondary overlap.
- [ ] **Add Stage 3: Entity overlap gate** — skip if both have entities but share zero
- [ ] **Update Stage 4**: Pass `ClassifiedTokens` to new scorer
- [ ] **Keep Stage 5** (close-time proximity) unchanged
- [ ] **Keep Stage 6** (greedy dedup) unchanged
- [ ] **Update `MIN_SHARED_TOKENS` from 1 to 2** (now enforced in scorer, can be removed from pipeline pre-filter or kept as a fast-path optimization)
- [ ] **Add tests**: category blocking, entity gate, full pipeline integration, performance 200x200 < 100ms

### Phase 6: Tests and Validation

- [ ] **Port all regression test cases** from the spec (section 3.1-3.7)
- [ ] **Add false positive regression tests** for all 4 known bad pairs
- [ ] **Add true positive tests** for known good pairs (Bitcoin, Trump, ETH/Ethereum alias)
- [ ] **Add negation test** ("Will X" vs "Will NOT X" must not AutoVerify)
- [ ] **Add number format test** ("$100k" vs "$100,000" should match)
- [ ] **Performance benchmark**: 200x200 in < 100ms (release build)
- [ ] **Dual-run validation**: If feasible, run old + new pipeline in parallel for comparison logging

---

## 7. Summary of Recommendations

### Dependencies

| Action | Detail |
|--------|--------|
| **ADD** | `regex = "1.11"` to workspace and arb-matcher |
| **KEEP** | `strsim = "0.11"` (demoted to tiebreaker) |
| **USE** | `std::sync::LazyLock` for all static data (no external crate) |
| **USE** | `std::collections::{HashMap, HashSet}` for entity dict, token sets, Jaccard |

### Key Patterns

| Component | Approach |
|-----------|----------|
| Stop words | `LazyLock<HashSet<&str>>` with ~80 entries, "not" excluded |
| Entity dictionary | `LazyLock<HashMap<&str, &str>>` with ~200 entries, sliding window matching |
| Category classification | Static `&[CategoryRule]` array, first-keyword-match, secondary category support |
| Jaccard similarity | Manual `HashSet` intersection/union on token slices |
| Weighted overlap | Manual per-class (entity/keyword/number/date) weighted intersection/union |
| Number normalization | `regex` crate with `LazyLock<Regex>` for k/m/b expansion + comma stripping |
| Capitalization detection | Original-text scan before lowercasing, dictionary-primary / caps-fallback |

### Critical Trading Review Items (Must Implement)

1. **Remove "not" from stop words** — prevents matching opposite contracts
2. **Add number normalization** — prevents false negatives on "$100k" vs "$100,000"
3. **Add `auto_verify_enabled` config flag** — prevents automated trading on uncalibrated scorer
