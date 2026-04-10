//! Text normalization and token classification for market question matching.
//!
//! This module provides:
//! - Number format normalization ($100k -> 100000, $1,500 -> 1500)
//! - Expanded stop-word removal (~80+ words, WITHOUT "not")
//! - Entity recognition via static dictionary + alias normalization
//! - Token classification into Entity/Number/Date/Keyword

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use regex::Regex;

// ---------------------------------------------------------------------------
// Stop words — function words with no discriminating value.
// IMPORTANT: "not" is intentionally EXCLUDED — it changes contract semantics.
// See Trading Review HIGH priority #1.
// ---------------------------------------------------------------------------

static STOP_WORDS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "a", "an", "the", "will", "be", "is", "are", "was", "were", "do", "does",
        "did", "has", "have", "had", "of", "in", "on", "at", "to", "for", "by",
        "or", "and", "if", "it", "its", "this", "that", "with", "from",
        "as", "but", "than", "before", "after", "yes", "no", "market",
        // Additions from spec:
        // NOTE: "over" and "under" intentionally EXCLUDED — they carry semantic
        // meaning in prediction markets (sports over/under lines, price thresholds).
        "above", "below", "between", "could", "would", "should", "can", "may", "might",
        "been", "being", "get", "got", "goes", "going", "about",
        "more", "less", "how", "many", "much", "what", "which", "when", "where",
        "who", "whom", "whose", "why", "any", "each", "every", "some", "most",
        "other", "also", "just", "only", "very", "so", "up", "down", "out",
        "then", "there", "here", "into", "through", "during", "against",
    ]
    .into_iter()
    .collect()
});

// ---------------------------------------------------------------------------
// Number normalization regexes (compiled once via LazyLock)
// ---------------------------------------------------------------------------

static RE_DOLLAR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\$").unwrap());
static RE_COMMA_IN_NUMBER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\d),(\d)").unwrap());
static RE_K_SUFFIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\d+(?:\.\d+)?)k\b").unwrap());
static RE_M_SUFFIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\d+(?:\.\d+)?)m\b").unwrap());
static RE_B_SUFFIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\d+(?:\.\d+)?)b\b").unwrap());

// ---------------------------------------------------------------------------
// Entity dictionary — maps aliases to canonical forms.
// Used for: (1) entity detection, (2) alias normalization.
// ---------------------------------------------------------------------------

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
    m.insert("binance", "binance");
    m.insert("coinbase", "coinbase");
    m.insert("litecoin", "litecoin");
    m.insert("ltc", "litecoin");
    m.insert("avalanche", "avalanche");
    m.insert("avax", "avalanche");

    // Countries
    m.insert("us", "united states");
    m.insert("usa", "united states");
    m.insert("u s", "united states");
    m.insert("united states", "united states");
    m.insert("uk", "united kingdom");
    m.insert("u k", "united kingdom");
    m.insert("united kingdom", "united kingdom");
    m.insert("iran", "iran");
    m.insert("china", "china");
    m.insert("russia", "russia");
    m.insert("ukraine", "ukraine");
    m.insert("israel", "israel");
    m.insert("peru", "peru");
    m.insert("peruvian", "peru");
    m.insert("brazil", "brazil");
    m.insert("india", "india");
    m.insert("japan", "japan");
    m.insert("germany", "germany");
    m.insert("france", "france");
    m.insert("mexico", "mexico");
    m.insert("canada", "canada");
    m.insert("australia", "australia");
    m.insert("south korea", "south korea");
    m.insert("north korea", "north korea");
    m.insert("taiwan", "taiwan");
    m.insert("saudi arabia", "saudi arabia");

    // People
    m.insert("trump", "trump");
    m.insert("donald trump", "trump");
    m.insert("biden", "biden");
    m.insert("joe biden", "biden");
    m.insert("musk", "elon musk");
    m.insert("elon musk", "elon musk");
    m.insert("elon", "elon musk");
    m.insert("desantis", "desantis");
    m.insert("ron desantis", "desantis");
    m.insert("putin", "putin");
    m.insert("xi jinping", "xi jinping");
    m.insert("zelensky", "zelensky");
    m.insert("harris", "harris");
    m.insert("kamala harris", "harris");
    m.insert("pelosi", "pelosi");
    m.insert("powell", "powell");
    m.insert("jerome powell", "powell");

    // Cities
    m.insert("nyc", "new york city");
    m.insert("new york city", "new york city");
    m.insert("new york", "new york city");
    m.insert("la", "los angeles");
    m.insert("los angeles", "los angeles");
    m.insert("sf", "san francisco");
    m.insert("san francisco", "san francisco");
    m.insert("chicago", "chicago");
    m.insert("miami", "miami");
    m.insert("london", "london");
    m.insert("tokyo", "tokyo");

    // Orgs/Indices
    m.insert("fed", "federal reserve");
    m.insert("federal reserve", "federal reserve");
    m.insert("sec", "securities and exchange commission");
    m.insert("gdp", "gross domestic product");
    m.insert("cpi", "consumer price index");
    m.insert("s&p", "s&p 500");
    m.insert("sp500", "s&p 500");
    m.insert("s p", "s&p 500");
    m.insert("nasdaq", "nasdaq");
    m.insert("dow", "dow jones");
    m.insert("dow jones", "dow jones");

    // Sports leagues
    m.insert("nfl", "national football league");
    m.insert("nba", "national basketball association");
    m.insert("mlb", "major league baseball");
    m.insert("nhl", "national hockey league");
    m.insert("ufc", "ultimate fighting championship");
    m.insert("fifa", "fifa");
    m.insert("super bowl", "super bowl");
    m.insert("world cup", "world cup");
    m.insert("champions league", "champions league");

    // Tech companies
    m.insert("apple", "apple");
    m.insert("aapl", "apple");
    m.insert("tesla", "tesla");
    m.insert("tsla", "tesla");
    m.insert("google", "google");
    m.insert("googl", "google");
    m.insert("amazon", "amazon");
    m.insert("amzn", "amazon");
    m.insert("meta", "meta");
    m.insert("microsoft", "microsoft");
    m.insert("msft", "microsoft");
    m.insert("nvidia", "nvidia");
    m.insert("nvda", "nvidia");
    m.insert("spacex", "spacex");
    m.insert("openai", "openai");

    // Geopolitical
    m.insert("nato", "nato");
    m.insert("eu", "european union");
    m.insert("european union", "european union");
    m.insert("un", "united nations");
    m.insert("united nations", "united nations");

    // Space/Science
    m.insert("nasa", "nasa");

    m
});

// ---------------------------------------------------------------------------
// Month names for date detection
// ---------------------------------------------------------------------------

static MONTHS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "january", "february", "march", "april", "may", "june",
        "july", "august", "september", "october", "november", "december",
        "jan", "feb", "mar", "apr", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ]
    .into_iter()
    .collect()
});

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Classified tokens extracted from a market question.
#[derive(Debug, Clone, Default)]
pub struct ClassifiedTokens {
    pub entities: Vec<String>,
    pub numbers: Vec<String>,
    pub dates: Vec<String>,
    pub keywords: Vec<String>,
    pub all_meaningful: Vec<String>,
}

// ---------------------------------------------------------------------------
// Number normalization
// ---------------------------------------------------------------------------

/// Normalize number formats in raw text before tokenization.
/// "$100k" -> "100000", "$1.5m" -> "1500000", "$100,000" -> "100000"
pub fn normalize_numbers(text: &str) -> String {
    let text = RE_DOLLAR.replace_all(text, "");
    // Repeatedly collapse commas between digits: "100,000,000" -> "100000000"
    let mut text = text.into_owned();
    loop {
        let next = RE_COMMA_IN_NUMBER.replace_all(&text, "$1$2").into_owned();
        if next == text {
            break;
        }
        text = next;
    }

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

// ---------------------------------------------------------------------------
// Basic normalization (lowercasing + punctuation stripping)
// ---------------------------------------------------------------------------

/// Normalize a market question string for comparison.
///
/// Steps:
/// 1. Normalize number formats ($100k -> 100000)
/// 2. Lowercase
/// 3. Strip non-alphanumeric characters (keep spaces)
/// 4. Collapse whitespace
pub fn normalize(question: &str) -> String {
    let with_numbers = normalize_numbers(question);
    let lower = with_numbers.to_lowercase();
    let cleaned: String = lower
        .chars()
        .map(|c| if c.is_alphanumeric() || c == ' ' { c } else { ' ' })
        .collect();
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ---------------------------------------------------------------------------
// Token extraction (simple, for backward compat + Jaro-Winkler input)
// ---------------------------------------------------------------------------

/// Extract meaningful tokens from a question, filtering stop words.
/// Returns flat token list (no classification).
pub fn extract_tokens(question: &str) -> Vec<String> {
    let normalized = normalize(question);
    normalized
        .split_whitespace()
        .filter(|w| !STOP_WORDS.contains(w))
        .map(|w| w.to_string())
        .collect()
}

/// Count how many tokens two questions share (after normalization).
pub fn shared_token_count(q1: &str, q2: &str) -> usize {
    let tokens_a = extract_tokens(q1);
    let tokens_b: Vec<String> = extract_tokens(q2);
    tokens_a.iter().filter(|t| tokens_b.contains(t)).count()
}

// ---------------------------------------------------------------------------
// Token classification
// ---------------------------------------------------------------------------

/// Classify tokens from a market question into entities, numbers, dates,
/// and keywords. Uses entity dictionary for recognition + alias normalization.
pub fn classify_tokens(question: &str) -> ClassifiedTokens {
    let normalized = normalize(question);
    if normalized.is_empty() {
        return ClassifiedTokens::default();
    }

    let words: Vec<&str> = normalized.split_whitespace().collect();

    let mut result = ClassifiedTokens::default();
    let mut consumed = vec![false; words.len()];

    // Pass 1: Multi-word entity detection (trigram, bigram, then unigram)
    let mut i = 0;
    while i < words.len() {
        let found = if i + 2 < words.len() {
            let tri = format!("{} {} {}", words[i], words[i + 1], words[i + 2]);
            ENTITY_DICT.get(tri.as_str()).map(|canon| (*canon, 3))
        } else {
            None
        }
        .or_else(|| {
            if i + 1 < words.len() {
                let bi = format!("{} {}", words[i], words[i + 1]);
                ENTITY_DICT.get(bi.as_str()).map(|canon| (*canon, 2))
            } else {
                None
            }
        })
        .or_else(|| ENTITY_DICT.get(words[i]).map(|canon| (*canon, 1)));

        if let Some((canonical, consumed_count)) = found {
            // Only add if not already present (dedup)
            let canon_str = canonical.to_string();
            if !result.entities.contains(&canon_str) {
                result.entities.push(canon_str);
            }
            for c in consumed.iter_mut().skip(i).take(consumed_count) {
                *c = true;
            }
            i += consumed_count;
        } else {
            i += 1;
        }
    }

    // Pass 2: Classify remaining (non-consumed) tokens
    for (idx, word) in words.iter().enumerate() {
        if consumed[idx] {
            continue;
        }

        // Skip stop words
        if STOP_WORDS.contains(word) {
            continue;
        }

        // Check if date-like BEFORE number — years like "2025" are dates, not numbers
        if is_date_token(word) {
            if !result.dates.contains(&word.to_string()) {
                result.dates.push(word.to_string());
            }
            continue;
        }

        // Check if number (all digits, or digits with decimal point)
        if is_number(word) {
            if !result.numbers.contains(&word.to_string()) {
                result.numbers.push(word.to_string());
            }
            continue;
        }

        // Otherwise it is a keyword
        let w = word.to_string();
        if !result.keywords.contains(&w) {
            result.keywords.push(w);
        }
    }

    // Build all_meaningful as union (preserving order, deduped)
    let mut seen = HashSet::new();
    for tok in result
        .entities
        .iter()
        .chain(result.numbers.iter())
        .chain(result.dates.iter())
        .chain(result.keywords.iter())
    {
        if seen.insert(tok.clone()) {
            result.all_meaningful.push(tok.clone());
        }
    }

    result
}

/// Check if a token is a number (digits only, or digits with one decimal point).
fn is_number(token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    let mut has_dot = false;
    for c in token.chars() {
        if c == '.' {
            if has_dot {
                return false;
            }
            has_dot = true;
        } else if !c.is_ascii_digit() {
            return false;
        }
    }
    // Must have at least one digit
    token.chars().any(|c| c.is_ascii_digit())
}

/// Check if a token is date-like (month name or year in 2020-2035 range).
fn is_date_token(token: &str) -> bool {
    if MONTHS.contains(token) {
        return true;
    }
    // Years 2020-2035
    if let Ok(year) = token.parse::<u32>() {
        return (2020..=2035).contains(&year);
    }
    false
}

/// Count entity overlap between two entity lists.
pub fn entity_overlap(a: &[String], b: &[String]) -> usize {
    a.iter().filter(|e| b.contains(e)).count()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ── Basic normalization ──────────────────────────────────────────────

    #[test]
    fn normalize_strips_punctuation() {
        assert_eq!(normalize("Will Bitcoin hit $100k?"), "will bitcoin hit 100000");
    }

    #[test]
    fn normalize_collapses_whitespace() {
        assert_eq!(normalize("  too   many   spaces  "), "too many spaces");
    }

    #[test]
    fn normalize_lowercases() {
        assert_eq!(normalize("HELLO World"), "hello world");
    }

    // ── Number normalization ─────────────────────────────────────────────

    #[test]
    fn number_norm_100k() {
        assert_eq!(normalize_numbers("$100k"), "100000");
    }

    #[test]
    fn number_norm_1_5m() {
        assert_eq!(normalize_numbers("$1.5m"), "1500000");
    }

    #[test]
    fn number_norm_comma_separated() {
        assert_eq!(normalize_numbers("$100,000"), "100000");
    }

    #[test]
    fn number_norm_large_comma() {
        assert_eq!(normalize_numbers("$1,000,000"), "1000000");
    }

    #[test]
    fn number_norm_no_change() {
        assert_eq!(normalize_numbers("100000"), "100000");
    }

    #[test]
    fn number_norm_decimal_no_suffix() {
        assert_eq!(normalize_numbers("36.99"), "36.99");
    }

    // ── Token extraction ─────────────────────────────────────────────────

    #[test]
    fn extract_tokens_removes_stop_words() {
        let tokens = extract_tokens("Will the price of Bitcoin hit $100k?");
        assert!(!tokens.contains(&"will".to_string()));
        assert!(!tokens.contains(&"the".to_string()));
        assert!(!tokens.contains(&"of".to_string()));
        assert!(tokens.contains(&"price".to_string()));
        assert!(tokens.contains(&"bitcoin".to_string()));
        assert!(tokens.contains(&"hit".to_string()));
        // $100k normalized to 100000
        assert!(tokens.contains(&"100000".to_string()));
    }

    #[test]
    fn extract_tokens_empty_string() {
        assert!(extract_tokens("").is_empty());
    }

    #[test]
    fn not_is_not_a_stop_word() {
        let tokens = extract_tokens("Will Bitcoin NOT hit $100k?");
        assert!(tokens.contains(&"not".to_string()),
            "\"not\" must NOT be a stop word — it changes contract semantics");
    }

    // ── Token classification ─────────────────────────────────────────────

    #[test]
    fn classify_bitcoin_question() {
        let ct = classify_tokens("Will Bitcoin hit $100k?");
        assert!(ct.entities.contains(&"bitcoin".to_string()));
        assert!(ct.numbers.contains(&"100000".to_string()));
        assert!(ct.keywords.contains(&"hit".to_string()));
    }

    #[test]
    fn classify_btc_alias() {
        let ct = classify_tokens("Will BTC reach $100k?");
        assert!(ct.entities.contains(&"bitcoin".to_string()),
            "BTC should be normalized to bitcoin, got {:?}", ct.entities);
    }

    #[test]
    fn classify_eth_alias() {
        let ct = classify_tokens("Will ETH reach $5000?");
        assert!(ct.entities.contains(&"ethereum".to_string()),
            "ETH should be normalized to ethereum, got {:?}", ct.entities);
    }

    #[test]
    fn classify_elon_musk_multi_word() {
        let ct = classify_tokens("Will Elon Musk post 260 tweets?");
        assert!(ct.entities.contains(&"elon musk".to_string()),
            "Elon Musk should be detected as multi-word entity, got {:?}", ct.entities);
    }

    #[test]
    fn classify_shiba_inu_multi_word() {
        let ct = classify_tokens("Shiba Inu price range on Apr 10, 2026?");
        assert!(ct.entities.contains(&"shiba inu".to_string()),
            "Shiba Inu should be detected, got {:?}", ct.entities);
    }

    #[test]
    fn classify_country_entities() {
        let ct = classify_tokens("US x Iran meeting by April 10?");
        assert!(ct.entities.iter().any(|e| e == "united states" || e == "us"),
            "US should be detected, got {:?}", ct.entities);
        assert!(ct.entities.contains(&"iran".to_string()),
            "Iran should be detected, got {:?}", ct.entities);
    }

    #[test]
    fn classify_date_tokens() {
        let ct = classify_tokens("Will something happen by December 2025?");
        assert!(ct.dates.contains(&"december".to_string()),
            "december should be a date token, got {:?}", ct.dates);
        assert!(ct.dates.contains(&"2025".to_string()),
            "2025 should be a date token, got {:?}", ct.dates);
    }

    #[test]
    fn classify_number_formats_match() {
        // $100k and $100,000 should produce the same number token after normalization
        let ct1 = classify_tokens("Will Bitcoin hit $100k?");
        let ct2 = classify_tokens("Will Bitcoin hit $100,000?");
        assert_eq!(ct1.numbers, ct2.numbers,
            "100k and 100,000 should normalize to same number");
    }

    // ── Entity overlap ───────────────────────────────────────────────────

    #[test]
    fn entity_overlap_zero_for_different_subjects() {
        let a = classify_tokens("Will Iran negotiate with the US?");
        let b = classify_tokens("Shiba Inu price on Apr 10?");
        let shared = entity_overlap(&a.entities, &b.entities);
        assert_eq!(shared, 0, "Iran/US vs Shiba Inu should have zero entity overlap");
    }

    #[test]
    fn entity_overlap_positive_for_same_subject() {
        let a = classify_tokens("Will Bitcoin hit $100k by December?");
        let b = classify_tokens("Bitcoin to reach $100k before year end?");
        let shared = entity_overlap(&a.entities, &b.entities);
        assert!(shared >= 1, "Both mention Bitcoin, should share entity");
    }

    // ── Shared token count ───────────────────────────────────────────────

    #[test]
    fn shared_token_count_identical() {
        let count = shared_token_count(
            "Will Bitcoin hit $100k by December?",
            "Will Bitcoin hit $100k by December?",
        );
        assert!(count >= 3, "identical questions should share many tokens, got {count}");
    }

    #[test]
    fn shared_token_count_different_phrasing() {
        let count = shared_token_count(
            "Will Bitcoin reach $100k before 2025?",
            "Bitcoin to hit $100k by end of 2024?",
        );
        assert!(count >= 2, "similar questions should share some tokens, got {count}");
    }
}
