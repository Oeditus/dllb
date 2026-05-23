//! Code-aware tokenizer for source code full-text search.
//!
//! Splits identifiers on camelCase and snake_case boundaries, lowercases
//! all tokens, and strips common noise keywords.
//!
//! Designed to be registered as a Tantivy `TextAnalyzer` for indexing
//! source code and docstrings.

/// Noise keywords stripped during tokenization.
/// These are common across many languages and carry little semantic signal.
const NOISE_KEYWORDS: &[&str] = &[
    "self",
    "this",
    "return",
    "def",
    "fn",
    "func",
    "function",
    "class",
    "let",
    "var",
    "const",
    "mut",
    "pub",
    "private",
    "protected",
    "static",
    "void",
    "new",
    "end",
    "do",
    "begin",
    "if",
    "else",
    "elsif",
    "elif",
    "then",
    "for",
    "while",
    "in",
    "of",
    "is",
    "as",
    "and",
    "or",
    "not",
    "true",
    "false",
    "nil",
    "null",
    "none",
    "import",
    "from",
    "require",
    "use",
    "module",
    "defmodule",
    "struct",
    "enum",
    "type",
    "trait",
    "impl",
    "interface",
    "abstract",
    "override",
    "super",
    "yield",
    "async",
    "await",
    "try",
    "catch",
    "raise",
    "throw",
    "finally",
    "rescue",
    "with",
    "match",
    "case",
    "when",
];

/// Tokenize source code text into a list of meaningful tokens.
///
/// - Splits on whitespace and punctuation
/// - Splits camelCase: `parseJSON` -> `["parse", "json"]`
/// - Splits snake_case: `parse_json` -> `["parse", "json"]`
/// - Lowercases all tokens
/// - Removes noise keywords and single-character tokens
pub fn code_tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();

    // Split on non-alphanumeric/underscore characters.
    for word in text.split(|c: char| !c.is_alphanumeric() && c != '_') {
        if word.is_empty() {
            continue;
        }
        // Split on underscores (snake_case).
        for part in word.split('_') {
            if part.is_empty() {
                continue;
            }
            // Split on camelCase boundaries.
            for sub in split_camel_case(part) {
                let lower = sub.to_lowercase();
                if lower.len() > 1 && !is_noise(&lower) {
                    tokens.push(lower);
                }
            }
        }
    }

    tokens
}

/// Split a word on camelCase boundaries.
///
/// `parseJSON` -> `["parse", "JSON"]`
/// `HTMLParser` -> `["HTML", "Parser"]`
/// `simple` -> `["simple"]`
fn split_camel_case(s: &str) -> Vec<&str> {
    let bytes = s.as_bytes();
    let mut parts = Vec::new();
    let mut start = 0;

    for i in 1..bytes.len() {
        let prev_upper = bytes[i - 1].is_ascii_uppercase();
        let curr_upper = bytes[i].is_ascii_uppercase();
        let curr_lower = bytes[i].is_ascii_lowercase();

        // Split before an uppercase letter following a lowercase letter: parseJSON -> parse|JSON
        if !prev_upper && curr_upper {
            parts.push(&s[start..i]);
            start = i;
        }
        // Split inside a run of uppercase before the last one: HTMLParser -> HTML|Parser
        if prev_upper && curr_lower && i - 1 > start {
            parts.push(&s[start..i - 1]);
            start = i - 1;
        }
    }

    if start < s.len() {
        parts.push(&s[start..]);
    }
    parts
}

/// Check if a token is a noise keyword.
fn is_noise(token: &str) -> bool {
    NOISE_KEYWORDS.contains(&token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camel_case_split() {
        assert_eq!(split_camel_case("parseJSON"), vec!["parse", "JSON"]);
        assert_eq!(split_camel_case("HTMLParser"), vec!["HTML", "Parser"]);
        assert_eq!(split_camel_case("simple"), vec!["simple"]);
        assert_eq!(split_camel_case("camelCase"), vec!["camel", "Case"]);
        assert_eq!(split_camel_case("ABCDef"), vec!["ABC", "Def"]);
    }

    #[test]
    fn snake_case_tokenize() {
        let tokens = code_tokenize("parse_json_data");
        assert_eq!(tokens, vec!["parse", "json", "data"]);
    }

    #[test]
    fn camel_case_tokenize() {
        let tokens = code_tokenize("parseJSONData");
        assert_eq!(tokens, vec!["parse", "json", "data"]);
    }

    #[test]
    fn mixed_tokenize() {
        let tokens = code_tokenize("fn process_user(self, user_id: i64) -> Result");
        // "fn" and "self" are noise; single chars removed
        assert!(tokens.contains(&"process".to_string()));
        assert!(tokens.contains(&"user".to_string()));
        assert!(tokens.contains(&"id".to_string()));
        assert!(tokens.contains(&"result".to_string()));
        assert!(!tokens.contains(&"fn".to_string()));
        assert!(!tokens.contains(&"self".to_string()));
    }

    #[test]
    fn noise_removal() {
        let tokens = code_tokenize("def initialize(self, value)");
        assert!(!tokens.contains(&"def".to_string()));
        assert!(!tokens.contains(&"self".to_string()));
        assert!(tokens.contains(&"initialize".to_string()));
        assert!(tokens.contains(&"value".to_string()));
    }

    #[test]
    fn real_code_snippet() {
        let code = "async fn fetch_user_profile(userId: String) -> UserProfile";
        let tokens = code_tokenize(code);
        assert!(tokens.contains(&"fetch".to_string()));
        assert!(tokens.contains(&"user".to_string()));
        assert!(tokens.contains(&"profile".to_string()));
        assert!(tokens.contains(&"string".to_string()));
        // "async", "fn" are noise
        assert!(!tokens.contains(&"async".to_string()));
        assert!(!tokens.contains(&"fn".to_string()));
    }
}
