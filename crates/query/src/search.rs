//! Process-wide stateful search-index services (full-text + vector).
//!
//! B-tree secondary indexes live in the redb keyspace and need no shared
//! state. Full-text (Tantivy) and vector (HNSW) indexes are backed by stateful
//! structures *outside* redb -- an on-disk Tantivy store and an in-memory HNSW
//! graph respectively -- so they are held here behind a process-wide `Arc`,
//! mirroring how the compute cache and write-version map are shared across
//! connection handlers.
//!
//! Persistence note: Tantivy indexes persist on disk and are reopened on
//! demand; HNSW is in-memory only, so a vector index is rebuilt from stored
//! embeddings on first use after process start (see the executor's lazy
//! rebuild). Both are handled idempotently here.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::RwLock;

use dllb_core::{Error, Result, Value};
use dllb_search::{AnalyzerConfig, FtsManager, Language};
use dllb_vector::{DistanceMetric, HnswConfig, HnswIndex, VectorIndex};

/// Map any lock-poison error to a storage-style error.
fn lock_err<E>(_: E) -> Error {
    Error::Index("search-service lock poisoned".into())
}

/// A registered vector index: its dimension plus the in-memory HNSW graph.
/// The distance metric is baked into the `HnswIndex` itself at construction.
struct VectorEntry {
    dim: usize,
    index: HnswIndex,
}

/// Process-wide handle to the stateful full-text and vector index structures.
pub struct SearchServices {
    /// Tantivy full-text indexes, keyed internally by `table.field`.
    fts: RwLock<FtsManager>,
    /// `table.field` keys already opened in this process (so reopen is idempotent).
    fts_open: RwLock<HashSet<String>>,
    /// In-memory HNSW indexes keyed by catalog index name.
    vectors: RwLock<HashMap<String, VectorEntry>>,
}

impl SearchServices {
    /// Create services rooted at `base_dir` (full-text indexes live under
    /// `<base_dir>/fts/`).
    pub fn new(base_dir: impl AsRef<Path>) -> Self {
        Self {
            fts: RwLock::new(FtsManager::new(base_dir)),
            fts_open: RwLock::new(HashSet::new()),
            vectors: RwLock::new(HashMap::new()),
        }
    }

    // -- Full-text ----------------------------------------------------------

    /// Ensure a full-text index for `table.field` is open (creating or
    /// reopening its on-disk Tantivy directory). Idempotent within a process.
    pub fn ensure_fulltext(&self, table: &str, field: &str, analyzer: &str) -> Result<()> {
        let key = fts_key(table, field);
        if self.fts_open.read().map_err(lock_err)?.contains(&key) {
            return Ok(());
        }
        let cfg = parse_analyzer(analyzer)?;
        self.fts
            .write()
            .map_err(lock_err)?
            .define_index(table, field, cfg)?;
        self.fts_open.write().map_err(lock_err)?.insert(key);
        Ok(())
    }

    /// Index (or re-index) a document's text in the full-text index.
    pub fn fts_index(&self, table: &str, field: &str, id: &str, text: &str) -> Result<()> {
        self.fts
            .read()
            .map_err(lock_err)?
            .update_document(table, field, id, text)
    }

    /// Remove a document from the full-text index.
    pub fn fts_delete(&self, table: &str, field: &str, id: &str) -> Result<()> {
        self.fts
            .read()
            .map_err(lock_err)?
            .delete_document(table, field, id)
    }

    /// Commit pending full-text writes so they become visible to searches.
    pub fn fts_commit(&self) -> Result<()> {
        self.fts.read().map_err(lock_err)?.commit_all()
    }

    /// Drop a full-text index, closing it and removing its on-disk directory.
    pub fn drop_fulltext(&self, table: &str, field: &str) -> Result<()> {
        let key = fts_key(table, field);
        self.fts
            .write()
            .map_err(lock_err)?
            .drop_index(table, field)?;
        self.fts_open.write().map_err(lock_err)?.remove(&key);
        Ok(())
    }

    /// Search a full-text index, returning `(id, score)` ranked by BM25.
    pub fn fts_search(
        &self,
        table: &str,
        field: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<(String, f32)>> {
        let hits = self
            .fts
            .read()
            .map_err(lock_err)?
            .search(table, field, query, limit)?;
        Ok(hits.into_iter().map(|h| (h.id, h.score)).collect())
    }

    // -- Vector -------------------------------------------------------------

    /// Whether an in-memory vector index is currently registered.
    pub fn vector_exists(&self, name: &str) -> Result<bool> {
        Ok(self.vectors.read().map_err(lock_err)?.contains_key(name))
    }

    /// Register a fresh, empty vector index (replacing any existing one).
    pub fn define_vector(&self, name: &str, dim: usize, metric: DistanceMetric) -> Result<()> {
        let entry = VectorEntry {
            dim,
            index: HnswIndex::new(dim, metric, HnswConfig::default()),
        };
        self.vectors
            .write()
            .map_err(lock_err)?
            .insert(name.to_string(), entry);
        Ok(())
    }

    /// Insert/replace a vector. Dimension mismatches are skipped silently
    /// (the document simply is not represented in the index).
    pub fn vector_insert(&self, name: &str, id: &str, vector: Vec<f32>) -> Result<()> {
        let mut map = self.vectors.write().map_err(lock_err)?;
        if let Some(entry) = map.get_mut(name)
            && vector.len() == entry.dim
        {
            entry.index.remove(id);
            entry.index.insert(id, vector);
        }
        Ok(())
    }

    /// Remove a vector by id.
    pub fn vector_remove(&self, name: &str, id: &str) -> Result<()> {
        let mut map = self.vectors.write().map_err(lock_err)?;
        if let Some(entry) = map.get_mut(name) {
            entry.index.remove(id);
        }
        Ok(())
    }

    /// Search a vector index for the `k` nearest neighbors, returning
    /// `(id, distance)` ordered nearest-first.
    pub fn vector_search(&self, name: &str, query: &[f32], k: usize) -> Result<Vec<(String, f32)>> {
        let map = self.vectors.read().map_err(lock_err)?;
        let entry = map
            .get(name)
            .ok_or_else(|| Error::Index(format!("vector index '{name}' is not loaded")))?;
        if query.len() != entry.dim {
            return Err(Error::Query(format!(
                "query vector has dimension {}, index '{name}' expects {}",
                query.len(),
                entry.dim
            )));
        }
        Ok(entry
            .index
            .search(query, k)
            .into_iter()
            .map(|h| (h.id, h.distance))
            .collect())
    }

    /// Drop a registered index (both kinds), best-effort.
    pub fn drop_vector(&self, name: &str) -> Result<()> {
        self.vectors.write().map_err(lock_err)?.remove(name);
        Ok(())
    }
}

fn fts_key(table: &str, field: &str) -> String {
    format!("{table}.{field}")
}

/// Parse an analyzer name into an [`AnalyzerConfig`]. `default` (or empty)
/// selects the default analyzer; language names enable stemming.
pub fn parse_analyzer(name: &str) -> Result<AnalyzerConfig> {
    let cfg = match name.to_lowercase().as_str() {
        "" | "default" => AnalyzerConfig::Default,
        "simple" => AnalyzerConfig::Simple,
        "english" => AnalyzerConfig::Language(Language::English),
        "spanish" => AnalyzerConfig::Language(Language::Spanish),
        "french" => AnalyzerConfig::Language(Language::French),
        "german" => AnalyzerConfig::Language(Language::German),
        "italian" => AnalyzerConfig::Language(Language::Italian),
        "portuguese" => AnalyzerConfig::Language(Language::Portuguese),
        "russian" => AnalyzerConfig::Language(Language::Russian),
        other => {
            return Err(Error::Query(format!(
                "unknown analyzer '{other}'; expected default, simple, or a language"
            )));
        }
    };
    Ok(cfg)
}

/// Parse a distance-metric name. `cosine` is the default.
pub fn parse_metric(name: &str) -> Result<DistanceMetric> {
    let metric = match name.to_lowercase().as_str() {
        "" | "cosine" => DistanceMetric::Cosine,
        "euclidean" | "l2" => DistanceMetric::Euclidean,
        "dot" | "dotproduct" | "dot_product" => DistanceMetric::DotProduct,
        other => {
            return Err(Error::Query(format!(
                "unknown metric '{other}'; expected cosine, euclidean, or dot"
            )));
        }
    };
    Ok(metric)
}

/// Stringify a [`DistanceMetric`] for catalog storage.
pub fn metric_str(metric: DistanceMetric) -> &'static str {
    match metric {
        DistanceMetric::Cosine => "cosine",
        DistanceMetric::Euclidean => "euclidean",
        DistanceMetric::DotProduct => "dot",
    }
}

/// Extract a dense `f32` vector from a document field value.
///
/// Accepts a native `Value::Vector` or an `Array` of numeric values; returns
/// `None` for any other shape (so the document is simply not indexed).
pub fn value_to_vector(value: &Value) -> Option<Vec<f32>> {
    match value {
        Value::Vector(v) => Some(v.clone()),
        Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for it in items {
                match it {
                    Value::Float(f) => out.push(*f as f32),
                    Value::Int(n) => out.push(*n as f32),
                    _ => return None,
                }
            }
            Some(out)
        }
        _ => None,
    }
}

/// Extract indexable text from a document field value (strings only).
pub fn value_to_text(value: &Value) -> Option<&str> {
    match value {
        Value::String(s) => Some(s.as_str()),
        _ => None,
    }
}
