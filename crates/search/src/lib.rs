//! # dllb-search
//!
//! Full-text search for dllb, powered by [Tantivy](https://github.com/quickwit-oss/tantivy).
//!
//! Provides BM25-scored inverted indexes with configurable analyzers
//! (default, language-specific stemming, simple whitespace). Each
//! full-text index is a separate Tantivy `Index` on disk.
//!
//! - [`FtsIndex`] wraps a single Tantivy index for one table/field
//! - [`FtsManager`] manages multiple indexes keyed by `table.field`
//! - [`SearchHit`] carries the record ID and BM25 score

pub mod analyzer;
pub mod fts_index;
pub mod manager;

pub use analyzer::{AnalyzerConfig, Language};
pub use fts_index::{FtsIndex, SearchHit};
pub use manager::FtsManager;
