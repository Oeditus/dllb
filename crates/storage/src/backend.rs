//! redb-backed implementation of the [`KvStore`] trait.

use std::path::Path;

use redb::{Database, ReadableDatabase, TableDefinition};

use dllb_core::{Error, Result};

use crate::kv::KvStore;

/// The single redb table that holds all dllb data.
/// Key encoding (see `key` module) handles model separation.
const DATA: TableDefinition<'_, &[u8], &[u8]> = TableDefinition::new("dllb_data");

/// A KV store backed by redb.
///
/// The `Database` is owned directly rather than behind an `Arc`: redb's
/// `compact` requires `&mut Database`, and wrapping it in an `Arc` would make
/// compaction permanently unreachable (`Arc::get_mut` can never succeed once
/// the handle has been cloned for readers). The `Mutex` provides the `&mut`
/// borrow for the rare compaction call while leaving every read and write path
/// on `&self`.
pub struct RedbBackend {
    db: std::sync::Mutex<Database>,
}

impl RedbBackend {
    /// Open or create a database at the given path.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path_ref = path.as_ref();
        let db = Database::create(path_ref).map_err(|e| {
            let canonical = path_ref.canonicalize().unwrap_or_else(|_| {
                std::env::current_dir()
                    .map(|cwd| cwd.join(path_ref))
                    .unwrap_or_else(|_| path_ref.to_path_buf())
            });
            Error::Storage(format!(
                "Failed to acquire lock or open database file '{}' (canonical path: '{}'): {}",
                path_ref.display(),
                canonical.display(),
                e
            ))
        })?;

        // Ensure the data table exists by opening a write transaction.
        let txn = db.begin_write().map_err(map_err)?;
        let _table = txn.open_table(DATA).map_err(map_err)?;
        // Must drop _table before committing (redb requires all table
        // handles dropped before commit).
        drop(_table);
        txn.commit().map_err(map_err)?;

        Ok(Self {
            db: std::sync::Mutex::new(db),
        })
    }

    /// Borrow the database for a read/write transaction.
    ///
    /// The lock is held only for the duration of the transaction, which is
    /// what redb itself serializes on anyway.
    fn db(&self) -> Result<std::sync::MutexGuard<'_, Database>> {
        self.db
            .lock()
            .map_err(|_| Error::Storage("database lock poisoned".into()))
    }

    /// Compact the underlying redb database to reclaim free space.
    ///
    /// Returns `true` when space was actually reclaimed.
    pub fn compact(&self) -> Result<bool> {
        let mut db = self.db()?;
        db.compact().map_err(map_err)
    }
}

impl KvStore for RedbBackend {
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let db = self.db()?;
        let txn = db.begin_read().map_err(map_err)?;
        let table = txn.open_table(DATA).map_err(map_err)?;
        let result: Option<redb::AccessGuard<'_, &[u8]>> = table.get(key).map_err(map_err)?;
        match result {
            Some(guard) => Ok(Some(guard.value().to_vec())),
            None => Ok(None),
        }
    }

    fn put(&self, key: &[u8], value: &[u8]) -> Result<()> {
        let db = self.db()?;
        let txn = db.begin_write().map_err(map_err)?;
        {
            let mut table = txn.open_table(DATA).map_err(map_err)?;
            table.insert(key, value).map_err(map_err)?;
        }
        txn.commit().map_err(map_err)?;
        Ok(())
    }

    fn delete(&self, key: &[u8]) -> Result<()> {
        let db = self.db()?;
        let txn = db.begin_write().map_err(map_err)?;
        {
            let mut table = txn.open_table(DATA).map_err(map_err)?;
            table.remove(key).map_err(map_err)?;
        }
        txn.commit().map_err(map_err)?;
        Ok(())
    }

    fn scan(&self, start: &[u8], end: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        let db = self.db()?;
        let txn = db.begin_read().map_err(map_err)?;
        let table = txn.open_table(DATA).map_err(map_err)?;
        let range: redb::Range<'_, &[u8], &[u8]> = table.range(start..end).map_err(map_err)?;
        let mut results = Vec::new();
        for entry in range {
            let (k, v) = entry.map_err(map_err)?;
            results.push((k.value().to_vec(), v.value().to_vec()));
        }
        Ok(results)
    }

    fn put_batch(&self, ops: &[(&[u8], &[u8])]) -> Result<()> {
        let db = self.db()?;
        let txn = db.begin_write().map_err(map_err)?;
        {
            let mut table = txn.open_table(DATA).map_err(map_err)?;
            for &(key, value) in ops {
                table.insert(key, value).map_err(map_err)?;
            }
        }
        txn.commit().map_err(map_err)?;
        Ok(())
    }

    fn delete_batch(&self, keys: &[&[u8]]) -> Result<()> {
        let db = self.db()?;
        let txn = db.begin_write().map_err(map_err)?;
        {
            let mut table = txn.open_table(DATA).map_err(map_err)?;
            for &key in keys {
                table.remove(key).map_err(map_err)?;
            }
        }
        txn.commit().map_err(map_err)?;
        Ok(())
    }

    fn write_batch(&self, puts: &[(&[u8], &[u8])], deletes: &[&[u8]]) -> Result<()> {
        let db = self.db()?;
        let txn = db.begin_write().map_err(map_err)?;
        {
            let mut table = txn.open_table(DATA).map_err(map_err)?;
            for &key in deletes {
                table.remove(key).map_err(map_err)?;
            }
            for &(key, value) in puts {
                table.insert(key, value).map_err(map_err)?;
            }
        }
        txn.commit().map_err(map_err)?;
        Ok(())
    }

    fn count_prefix(&self, prefix: &[u8]) -> Result<usize> {
        let end = crate::key::prefix_end(prefix);
        let db = self.db()?;
        let txn = db.begin_read().map_err(map_err)?;
        let table = txn.open_table(DATA).map_err(map_err)?;
        let range: redb::Range<'_, &[u8], &[u8]> =
            table.range(prefix..end.as_slice()).map_err(map_err)?;
        // Count by advancing the iterator; never materializes keys or values.
        let mut n = 0usize;
        for entry in range {
            entry.map_err(map_err)?;
            n += 1;
        }
        Ok(n)
    }

    fn scan_prefix_keys(&self, prefix: &[u8]) -> Result<Vec<Vec<u8>>> {
        let end = crate::key::prefix_end(prefix);
        let db = self.db()?;
        let txn = db.begin_read().map_err(map_err)?;
        let table = txn.open_table(DATA).map_err(map_err)?;
        let range: redb::Range<'_, &[u8], &[u8]> =
            table.range(prefix..end.as_slice()).map_err(map_err)?;
        let mut keys = Vec::new();
        for entry in range {
            let (k, _v) = entry.map_err(map_err)?;
            keys.push(k.value().to_vec());
        }
        Ok(keys)
    }

    fn multi_get(&self, keys: &[&[u8]]) -> Result<Vec<Option<Vec<u8>>>> {
        // Share a single read transaction (one MVCC snapshot) across all
        // lookups instead of opening one per key.
        let db = self.db()?;
        let txn = db.begin_read().map_err(map_err)?;
        let table = txn.open_table(DATA).map_err(map_err)?;
        let mut out = Vec::with_capacity(keys.len());
        for &key in keys {
            let guard: Option<redb::AccessGuard<'_, &[u8]>> = table.get(key).map_err(map_err)?;
            out.push(guard.map(|g| g.value().to_vec()));
        }
        Ok(out)
    }
}

/// Map any redb error into `dllb_core::Error::Storage`.
fn map_err(e: impl std::fmt::Display) -> Error {
    Error::Storage(e.to_string())
}
