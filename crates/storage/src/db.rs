//! High-level storage API over the redb backend.
//!
//! Reads and writes both go directly through the backend. redb itself
//! provides MVCC for concurrent readers and a single serialized writer, so no
//! additional actor layer is needed; callers share one `DllbStorage` by
//! reference (or behind an `Arc<DllbStorage>` at the process level).

use std::path::Path;

use dllb_core::Result;

use crate::backend::RedbBackend;
use crate::kv::KvStore;

/// The main entry point for dllb storage.
pub struct DllbStorage {
    backend: RedbBackend,
}

impl DllbStorage {
    /// Open or create a database at the given path.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let backend = RedbBackend::open(path)?;
        Ok(Self { backend })
    }

    /// Compact the underlying database to reclaim free space.
    ///
    /// Returns `true` when space was reclaimed. Takes `&self` so a shared
    /// `DllbStorage` can compact without an exclusive borrow; the backend
    /// serializes the rewrite internally.
    pub fn compact(&self) -> Result<bool> {
        self.backend.compact()
    }

    // ---------------------------------------------------------------
    // Write operations (will route through actor in a later step)
    // ---------------------------------------------------------------

    /// Insert or overwrite a key-value pair.
    pub fn put(&self, key: &[u8], value: &[u8]) -> Result<()> {
        self.backend.put(key, value)
    }

    /// Delete a key.
    pub fn delete(&self, key: &[u8]) -> Result<()> {
        self.backend.delete(key)
    }

    /// Atomically insert multiple key-value pairs.
    pub fn put_batch(&self, ops: &[(&[u8], &[u8])]) -> Result<()> {
        self.backend.put_batch(ops)
    }

    /// Atomically delete multiple keys.
    pub fn delete_batch(&self, keys: &[&[u8]]) -> Result<()> {
        self.backend.delete_batch(keys)
    }

    /// Atomically insert multiple key-value pairs and delete multiple keys
    /// in a single write transaction.
    pub fn write_batch(&self, puts: &[(&[u8], &[u8])], deletes: &[&[u8]]) -> Result<()> {
        self.backend.write_batch(puts, deletes)
    }

    // ---------------------------------------------------------------
    // Read operations (direct, no actor)
    // ---------------------------------------------------------------

    /// Get the value for a key.
    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        self.backend.get(key)
    }

    /// Scan a half-open range `[start, end)`.
    pub fn scan(&self, start: &[u8], end: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        self.backend.scan(start, end)
    }

    /// Scan all keys sharing the given prefix.
    pub fn prefix_scan(&self, prefix: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        self.backend.prefix_scan(prefix)
    }

    /// Count the entries sharing the given prefix without materializing them.
    ///
    /// Backs cheap `COUNT(*)`-style aggregations: the engine advances its
    /// range cursor and never copies keys or values into user space.
    pub fn count_prefix(&self, prefix: &[u8]) -> Result<usize> {
        self.backend.count_prefix(prefix)
    }

    /// Scan only the keys sharing the given prefix, skipping value reads.
    ///
    /// Backs key-derived lookups (document ids, graph neighbor ids) where the
    /// stored value is irrelevant, avoiding per-entry value copies.
    pub fn scan_prefix_keys(&self, prefix: &[u8]) -> Result<Vec<Vec<u8>>> {
        self.backend.scan_prefix_keys(prefix)
    }

    /// Resolve multiple keys in a single read transaction.
    ///
    /// Returns one entry per input key, in order (`None` for absent keys).
    pub fn multi_get(&self, keys: &[&[u8]]) -> Result<Vec<Option<Vec<u8>>>> {
        self.backend.multi_get(keys)
    }

    /// Check whether a key exists.
    pub fn contains(&self, key: &[u8]) -> Result<bool> {
        self.backend.contains(key)
    }
}
