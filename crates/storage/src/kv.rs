//! The `KvStore` trait: the fundamental storage abstraction.

use dllb_core::Result;

/// A key-value store with transactional semantics.
///
/// All operations are synchronous -- redb operations are blocking and
/// sub-millisecond. The actor layer (StorageWriter) handles async dispatch.
pub trait KvStore: Send + Sync {
    /// Get the value for `key`, or `None` if absent.
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>>;

    /// Insert or overwrite `key` with `value`.
    fn put(&self, key: &[u8], value: &[u8]) -> Result<()>;

    /// Delete `key`. No-op if the key does not exist.
    fn delete(&self, key: &[u8]) -> Result<()>;

    /// Scan all key-value pairs in the half-open range `[start, end)`.
    fn scan(&self, start: &[u8], end: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>>;

    /// Insert multiple key-value pairs in a single atomic transaction.
    fn put_batch(&self, ops: &[(&[u8], &[u8])]) -> Result<()>;

    /// Delete multiple keys in a single atomic transaction.
    fn delete_batch(&self, keys: &[&[u8]]) -> Result<()>;

    /// Insert multiple key-value pairs and delete multiple keys in a single
    /// atomic transaction. Deletes are applied before inserts so that a key
    /// present in both lists ends up with the inserted value.
    fn write_batch(&self, puts: &[(&[u8], &[u8])], deletes: &[&[u8]]) -> Result<()> {
        // Default: two separate transactions (correct but not atomic across
        // both). Backends should override for true atomicity.
        if !deletes.is_empty() {
            self.delete_batch(deletes)?;
        }
        if !puts.is_empty() {
            self.put_batch(puts)?;
        }
        Ok(())
    }

    /// Check whether `key` exists without reading the value.
    fn contains(&self, key: &[u8]) -> Result<bool> {
        Ok(self.get(key)?.is_some())
    }

    /// Scan all keys sharing the given `prefix`.
    fn prefix_scan(&self, prefix: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        let end = crate::key::prefix_end(prefix);
        self.scan(prefix, &end)
    }

    /// Count the entries sharing the given `prefix`.
    ///
    /// The default implementation materializes the range; backends should
    /// override to count without copying keys or values out of the engine.
    fn count_prefix(&self, prefix: &[u8]) -> Result<usize> {
        Ok(self.prefix_scan(prefix)?.len())
    }

    /// Scan only the keys sharing the given `prefix`, skipping values.
    ///
    /// The default implementation drops the values after a full scan;
    /// backends should override to avoid copying values entirely.
    fn scan_prefix_keys(&self, prefix: &[u8]) -> Result<Vec<Vec<u8>>> {
        Ok(self
            .prefix_scan(prefix)?
            .into_iter()
            .map(|(k, _)| k)
            .collect())
    }

    /// Resolve multiple point lookups, returning one entry per input key in
    /// the same order (`None` for absent keys).
    ///
    /// The default implementation issues one `get` per key; backends should
    /// override to share a single read transaction across all lookups.
    fn multi_get(&self, keys: &[&[u8]]) -> Result<Vec<Option<Vec<u8>>>> {
        keys.iter().map(|k| self.get(k)).collect()
    }
}
