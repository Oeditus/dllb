//! redb-backed implementation of the [`KvStore`] trait.

use std::path::Path;
use std::sync::Arc;

use redb::{Database, ReadableDatabase, TableDefinition};

use dllb_core::{Error, Result};

use crate::kv::KvStore;

/// The single redb table that holds all dllb data.
/// Key encoding (see `key` module) handles model separation.
const DATA: TableDefinition<'_, &[u8], &[u8]> = TableDefinition::new("dllb_data");

/// A KV store backed by redb.
///
/// Holds an `Arc<Database>` so it can be cheaply cloned for read access
/// while the `StorageWriter` actor holds the canonical write path.
#[derive(Clone)]
pub struct RedbBackend {
    db: Arc<Database>,
}

impl RedbBackend {
    /// Open or create a database at the given path.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let db = Database::create(path.as_ref()).map_err(map_err)?;

        // Ensure the data table exists by opening a write transaction.
        let txn = db.begin_write().map_err(map_err)?;
        let _table = txn.open_table(DATA).map_err(map_err)?;
        // Must drop _table before committing (redb requires all table
        // handles dropped before commit).
        drop(_table);
        txn.commit().map_err(map_err)?;

        Ok(Self { db: Arc::new(db) })
    }

    /// Get a shared handle to the underlying redb `Database`.
    ///
    /// Callers can use this for direct read transactions, bypassing
    /// the actor mailbox for zero-overhead reads.
    pub fn db_handle(&self) -> Arc<Database> {
        Arc::clone(&self.db)
    }
}

impl KvStore for RedbBackend {
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>> {
        let txn = self.db.begin_read().map_err(map_err)?;
        let table = txn.open_table(DATA).map_err(map_err)?;
        let result: Option<redb::AccessGuard<'_, &[u8]>> = table.get(key).map_err(map_err)?;
        match result {
            Some(guard) => Ok(Some(guard.value().to_vec())),
            None => Ok(None),
        }
    }

    fn put(&self, key: &[u8], value: &[u8]) -> Result<()> {
        let txn = self.db.begin_write().map_err(map_err)?;
        {
            let mut table = txn.open_table(DATA).map_err(map_err)?;
            table.insert(key, value).map_err(map_err)?;
        }
        txn.commit().map_err(map_err)?;
        Ok(())
    }

    fn delete(&self, key: &[u8]) -> Result<()> {
        let txn = self.db.begin_write().map_err(map_err)?;
        {
            let mut table = txn.open_table(DATA).map_err(map_err)?;
            table.remove(key).map_err(map_err)?;
        }
        txn.commit().map_err(map_err)?;
        Ok(())
    }

    fn scan(&self, start: &[u8], end: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        let txn = self.db.begin_read().map_err(map_err)?;
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
        let txn = self.db.begin_write().map_err(map_err)?;
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
        let txn = self.db.begin_write().map_err(map_err)?;
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
        let txn = self.db.begin_write().map_err(map_err)?;
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
}

/// Map any redb error into `dllb_core::Error::Storage`.
fn map_err(e: impl std::fmt::Display) -> Error {
    Error::Storage(e.to_string())
}
