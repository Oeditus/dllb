//! High-level storage API that ties the redb backend and the
//! `StorageWriter` actor together.
//!
//! - Writes go through the actor (serialized, fault-tolerant).
//! - Reads go directly through redb read transactions (no mailbox overhead).

use std::path::Path;
use std::sync::Arc;

use redb::Database;

use dllb_core::Result;

use crate::backend::RedbBackend;
use crate::kv::KvStore;

/// The main entry point for dllb storage.
///
/// Holds a shared redb backend for direct reads and a reference to the
/// `StorageWriter` actor for serialized writes.
pub struct DllbStorage {
    backend: Arc<RedbBackend>,
}

impl DllbStorage {
    /// Open or create a database at the given path.
    ///
    /// In the future this will also spawn the `StorageWriter` actor
    /// into an `ActorSystem`. For now it provides a synchronous API
    /// suitable for Phase 1 testing.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let backend = RedbBackend::open(path)?;
        Ok(Self {
            backend: Arc::new(backend),
        })
    }

    /// Get a shared handle to the underlying redb `Database` for
    /// advanced use cases (e.g., direct read transactions).
    pub fn db_handle(&self) -> Arc<Database> {
        self.backend.db_handle()
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
