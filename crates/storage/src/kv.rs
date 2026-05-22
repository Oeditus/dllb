//! The `KvStore` trait: the fundamental storage abstraction.

use dllb_core::Result;

/// A key-value store with transactional semantics.
pub trait KvStore: Send + Sync {
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>>;
    fn put(&self, key: &[u8], value: &[u8]) -> Result<()>;
    fn delete(&self, key: &[u8]) -> Result<()>;

    /// Scan all key-value pairs in the range `[start, end)`.
    fn scan(&self, start: &[u8], end: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>>;
}
