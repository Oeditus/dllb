# Full-Text Search

This document covers the `dllb-search` crate: Tantivy-backed full-text
indexing with BM25 scoring, configurable analyzers, and document lifecycle.

## Design

Full-text search is provided by [Tantivy](https://github.com/quickwit-oss/tantivy)
as a library. We do not reimplement inverted indexes. Each table/field
combination gets its own Tantivy `Index` directory on disk.

Each Tantivy index stores exactly 2 fields:
- `_id`: STRING (stored + indexed as single token for exact delete/lookup)
- `_text`: TEXT (stored + indexed with the configured analyzer)

## Analyzer Configuration

```rust
// Default: simple tokenizer + lowercase
let config = AnalyzerConfig::Default;

// Language stemming (English, Spanish, French, German, Italian, Portuguese, Russian)
let config = AnalyzerConfig::Language(Language::English);

// Simple: whitespace only, no lowercase, no stemming
let config = AnalyzerConfig::Simple;
```

Analyzers are registered with Tantivy under the name `dllb_analyzer` and
applied to the `_text` field via `TextFieldIndexing`.

## FtsIndex

Single-index operations:

```rust
let idx = FtsIndex::open_or_create(&path, AnalyzerConfig::Default)?;

// Index
idx.index_document("doc1", "the quick brown fox")?;
idx.commit()?;

// Search (BM25 ranked)
let hits = idx.search("brown fox", 10)?;
// hits[0] = SearchHit { id: "doc1", score: 1.23 }

// Update (delete + re-index)
idx.update_document("doc1", "the slow red fox")?;
idx.commit()?;

// Delete
idx.delete_document("doc1")?;
idx.commit()?;
```

### SearchHit

```rust
pub struct SearchHit {
    pub id: String,   // record ID
    pub score: f32,   // BM25 relevance score
}
```

## FtsManager

Manages multiple indexes keyed by `table.field`:

```rust
let mut mgr = FtsManager::new("/data");

mgr.define_index("article", "title", AnalyzerConfig::Default)?;
mgr.define_index("article", "body", AnalyzerConfig::Language(Language::English))?;

mgr.index_document("article", "title", "doc1", "Graph Databases")?;
mgr.index_document("article", "body", "doc1", "Graphs are everywhere...")?;
mgr.commit_all()?;

let hits = mgr.search("article", "title", "graph", 10)?;
```

Index directories: `base_dir/fts/table.field/`

## Commit Semantics

Changes (index/delete/update) are buffered in Tantivy's writer. Call
`commit()` (or `commit_all()` on the manager) to flush to disk and make
changes visible to searchers.

The reader uses `ReloadPolicy::OnCommitWithDelay` -- after `commit()`,
we explicitly `reload()` to ensure immediate visibility in tests.

## Testing

```bash
cargo test -p dllb-search
```

9 integration tests: index+search, BM25 ranking, delete, update, no results,
English stemming, multi-index isolation, undefined index error, persistence
across reopen.
