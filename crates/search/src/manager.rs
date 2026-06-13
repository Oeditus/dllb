//! [`FtsManager`] -- manages multiple full-text indexes.
//!
//! Each table/field combination gets its own Tantivy index directory.
//! The manager provides a single entry point for all FTS operations.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use dllb_core::{Error, Result};

use crate::analyzer::AnalyzerConfig;
use crate::fts_index::{FtsIndex, SearchHit};

/// Manages full-text indexes for multiple table/field combinations.
pub struct FtsManager {
    base_dir: PathBuf,
    indexes: HashMap<String, FtsIndex>,
}

impl FtsManager {
    /// Create a new manager with the given base directory for index storage.
    pub fn new(base_dir: impl AsRef<Path>) -> Self {
        Self {
            base_dir: base_dir.as_ref().to_path_buf(),
            indexes: HashMap::new(),
        }
    }

    /// Define and open a full-text index for a table/field combination.
    pub fn define_index(
        &mut self,
        table: &str,
        field: &str,
        analyzer: AnalyzerConfig,
    ) -> Result<()> {
        let key = index_key(table, field);
        let path = self.base_dir.join("fts").join(&key);
        let index = FtsIndex::open_or_create(&path, analyzer)?;
        self.indexes.insert(key, index);
        Ok(())
    }

    /// Index a document's text content.
    pub fn index_document(&self, table: &str, field: &str, id: &str, text: &str) -> Result<()> {
        let idx = self.get_index(table, field)?;
        idx.index_document(id, text)
    }

    /// Delete a document from the full-text index.
    pub fn delete_document(&self, table: &str, field: &str, id: &str) -> Result<()> {
        let idx = self.get_index(table, field)?;
        idx.delete_document(id)
    }

    /// Update a document's indexed text.
    pub fn update_document(&self, table: &str, field: &str, id: &str, text: &str) -> Result<()> {
        let idx = self.get_index(table, field)?;
        idx.update_document(id, text)
    }

    /// Search a full-text index.
    pub fn search(
        &self,
        table: &str,
        field: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SearchHit>> {
        let idx = self.get_index(table, field)?;
        idx.search(query, limit)
    }

    /// Commit all pending writes across all indexes.
    pub fn commit_all(&self) -> Result<()> {
        for idx in self.indexes.values() {
            idx.commit()?;
        }
        Ok(())
    }

    /// Drop a full-text index: close its in-memory handle and remove the
    /// on-disk Tantivy directory. Idempotent (a missing index is a no-op).
    pub fn drop_index(&mut self, table: &str, field: &str) -> Result<()> {
        let key = index_key(table, field);
        self.indexes.remove(&key);
        let path = self.base_dir.join("fts").join(&key);
        if path.exists() {
            std::fs::remove_dir_all(&path).map_err(|e| Error::Storage(e.to_string()))?;
        }
        Ok(())
    }

    fn get_index(&self, table: &str, field: &str) -> Result<&FtsIndex> {
        let key = index_key(table, field);
        self.indexes
            .get(&key)
            .ok_or_else(|| Error::Index(format!("no FTS index defined for {table}.{field}")))
    }
}

fn index_key(table: &str, field: &str) -> String {
    format!("{table}.{field}")
}
