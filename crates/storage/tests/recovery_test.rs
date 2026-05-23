//! Crash recovery test: verify committed data survives close and reopen.

use dllb_storage::db::DllbStorage;
use dllb_storage::key;
use dllb_storage::kv::KvStore;

#[test]
fn data_persists_across_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("recovery.redb");

    // First session: create data.
    {
        let storage = DllbStorage::open(&path).unwrap();
        let k1 = key::document_key("ns", "db", "user", "alice");
        let k2 = key::document_key("ns", "db", "user", "bob");
        storage.put(&k1, b"alice-data").unwrap();
        storage.put(&k2, b"bob-data").unwrap();
    }
    // DllbStorage dropped here -- simulates process exit.

    // Second session: verify data is still there.
    {
        let storage = DllbStorage::open(&path).unwrap();
        let k1 = key::document_key("ns", "db", "user", "alice");
        let k2 = key::document_key("ns", "db", "user", "bob");

        assert_eq!(storage.get(&k1).unwrap(), Some(b"alice-data".to_vec()));
        assert_eq!(storage.get(&k2).unwrap(), Some(b"bob-data".to_vec()));

        // Prefix scan should find both.
        let prefix = key::table_prefix("ns", "db", "user", key::tag::DOCUMENT);
        let results = storage.prefix_scan(&prefix).unwrap();
        assert_eq!(results.len(), 2);
    }
}

#[test]
fn delete_persists_across_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("recovery_del.redb");

    // First session: create and delete.
    {
        let storage = DllbStorage::open(&path).unwrap();
        let k = key::document_key("ns", "db", "user", "alice");
        storage.put(&k, b"data").unwrap();
        storage.delete(&k).unwrap();
    }

    // Second session: verify deletion persisted.
    {
        let storage = DllbStorage::open(&path).unwrap();
        let k = key::document_key("ns", "db", "user", "alice");
        assert_eq!(storage.get(&k).unwrap(), None);
    }
}
