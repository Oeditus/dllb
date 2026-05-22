//! # dllb-search
//!
//! Full-text search for dllb, powered by [Tantivy](https://github.com/quickwit-oss/tantivy).
//!
//! Provides BM25-scored inverted indexes with configurable analyzers
//! (whitespace, stemming for 17+ languages). Each full-text index is a
//! separate Tantivy `Index` on disk, managed by the `FtsActor` GenServer.
//!
//! Index updates are synchronized with KV store commits to ensure
//! consistency. On crash recovery, the supervisor rebuilds the Tantivy
//! index from the KV store.
