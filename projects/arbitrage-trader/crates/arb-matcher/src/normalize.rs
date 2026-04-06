//! Text normalization utilities for market question matching.
//!
//! Prediction market questions from different platforms often phrase the same
//! event differently. This module provides normalization and token extraction
//! so that fuzzy comparison can focus on the semantically meaningful parts.

/// Stop words that carry no discriminating value when comparing questions.
const STOP_WORDS: &[&str] = &[
    "a", "an", "the", "will", "be", "is", "are", "was", "were", "do", "does",
    "did", "has", "have", "had", "of", "in", "on", "at", "to", "for", "by",
    "or", "and", "not", "if", "it", "its", "this", "that", "with", "from",
    "as", "but", "than", "before", "after", "yes", "no", "market",
];

/// Normalize a market question string for comparison.
///
/// Steps:
/// 1. Lowercase
/// 2. Strip non-alphanumeric characters (keep spaces)
/// 3. Collapse whitespace
/// 4. Trim
pub fn normalize(question: &str) -> String {
    let lower = question.to_lowercase();
    let cleaned: String = lower
        .chars()
        .map(|c| if c.is_alphanumeric() || c == ' ' { c } else { ' ' })
        .collect();
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Extract meaningful tokens from a normalized question, filtering stop words.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_punctuation() {
        assert_eq!(normalize("Will Bitcoin hit $100k?"), "will bitcoin hit 100k");
    }

    #[test]
    fn normalize_collapses_whitespace() {
        assert_eq!(normalize("  too   many   spaces  "), "too many spaces");
    }

    #[test]
    fn normalize_lowercases() {
        assert_eq!(normalize("HELLO World"), "hello world");
    }

    #[test]
    fn extract_tokens_removes_stop_words() {
        let tokens = extract_tokens("Will the price of Bitcoin hit $100k?");
        assert!(!tokens.contains(&"will".to_string()));
        assert!(!tokens.contains(&"the".to_string()));
        assert!(!tokens.contains(&"of".to_string()));
        assert!(tokens.contains(&"price".to_string()));
        assert!(tokens.contains(&"bitcoin".to_string()));
        assert!(tokens.contains(&"hit".to_string()));
        assert!(tokens.contains(&"100k".to_string()));
    }

    #[test]
    fn extract_tokens_empty_string() {
        assert!(extract_tokens("").is_empty());
    }

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
