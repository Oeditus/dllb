//! Analyzer configuration for full-text indexes.
//!
//! Maps high-level analyzer choices to Tantivy's `TextAnalyzer` with
//! appropriate tokenizers and filters.

use tantivy::tokenizer::{LowerCaser, SimpleTokenizer, Stemmer, TextAnalyzer, WhitespaceTokenizer};

/// High-level analyzer configuration for a full-text index.
#[derive(Debug, Clone)]
pub enum AnalyzerConfig {
    /// Tantivy's default: simple tokenizer + lowercase.
    Default,
    /// Language-specific stemming + lowercase.
    Language(Language),
    /// Whitespace tokenizer only (no stemming, no lowercase).
    Simple,
}

/// Supported stemming languages.
#[derive(Debug, Clone, Copy)]
pub enum Language {
    English,
    Spanish,
    French,
    German,
    Italian,
    Portuguese,
    Russian,
}

/// The analyzer name registered with Tantivy.
pub const ANALYZER_NAME: &str = "dllb_analyzer";

/// Build a Tantivy `TextAnalyzer` from the config.
pub fn build_analyzer(config: &AnalyzerConfig) -> TextAnalyzer {
    match config {
        AnalyzerConfig::Default => TextAnalyzer::builder(SimpleTokenizer::default())
            .filter(LowerCaser)
            .build(),
        AnalyzerConfig::Language(lang) => {
            let stemmer = match lang {
                Language::English => Stemmer::new(tantivy::tokenizer::Language::English),
                Language::Spanish => Stemmer::new(tantivy::tokenizer::Language::Spanish),
                Language::French => Stemmer::new(tantivy::tokenizer::Language::French),
                Language::German => Stemmer::new(tantivy::tokenizer::Language::German),
                Language::Italian => Stemmer::new(tantivy::tokenizer::Language::Italian),
                Language::Portuguese => Stemmer::new(tantivy::tokenizer::Language::Portuguese),
                Language::Russian => Stemmer::new(tantivy::tokenizer::Language::Russian),
            };
            TextAnalyzer::builder(SimpleTokenizer::default())
                .filter(LowerCaser)
                .filter(stemmer)
                .build()
        }
        AnalyzerConfig::Simple => TextAnalyzer::builder(WhitespaceTokenizer::default()).build(),
    }
}
