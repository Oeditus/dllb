//! Integration tests for the redb-backed storage engine.

use dllb_storage::backend::RedbBackend;
use dllb_storage::db::DllbStorage;
use dllb_storage::key;
use dllb_storage::kv::KvStore;

fn temp_db() -> (tempfile::TempDir, RedbBackend) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.redb");
    let backend = RedbBackend::open(&path).unwrap();
    (dir, backend)
}

fn temp_storage() -> (tempfile::TempDir, DllbStorage) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.redb");
    let storage = DllbStorage::open(&path).unwrap();
    (dir, storage)
}

// -----------------------------------------------------------------------
// RedbBackend (KvStore trait) tests
// -----------------------------------------------------------------------

#[test]
fn put_get_roundtrip() {
    let (_dir, db) = temp_db();
    db.put(b"hello", b"world").unwrap();
    assert_eq!(db.get(b"hello").unwrap(), Some(b"world".to_vec()));
}

#[test]
fn get_missing_key_returns_none() {
    let (_dir, db) = temp_db();
    assert_eq!(db.get(b"nonexistent").unwrap(), None);
}

#[test]
fn put_delete_get_returns_none() {
    let (_dir, db) = temp_db();
    db.put(b"key", b"val").unwrap();
    db.delete(b"key").unwrap();
    assert_eq!(db.get(b"key").unwrap(), None);
}

#[test]
fn delete_nonexistent_is_noop() {
    let (_dir, db) = temp_db();
    db.delete(b"ghost").unwrap(); // should not error
}

#[test]
fn scan_range() {
    let (_dir, db) = temp_db();
    db.put(b"a", b"1").unwrap();
    db.put(b"b", b"2").unwrap();
    db.put(b"c", b"3").unwrap();
    db.put(b"d", b"4").unwrap();

    let results = db.scan(b"b", b"d").unwrap();
    let keys: Vec<&[u8]> = results.iter().map(|(k, _)| k.as_slice()).collect();
    assert_eq!(keys, vec![b"b".as_slice(), b"c"]);
}

#[test]
fn prefix_scan_filters_correctly() {
    let (_dir, db) = temp_db();

    let k1 = key::document_key("ns", "db", "user", "alice");
    let k2 = key::document_key("ns", "db", "user", "bob");
    let k3 = key::document_key("ns", "db", "product", "widget");

    db.put(&k1, b"alice-data").unwrap();
    db.put(&k2, b"bob-data").unwrap();
    db.put(&k3, b"widget-data").unwrap();

    let prefix = key::table_prefix("ns", "db", "user", key::tag::DOCUMENT);
    let results = db.prefix_scan(&prefix).unwrap();

    // Should find alice and bob, but not widget (different table).
    assert_eq!(results.len(), 2);
}

#[test]
fn put_batch_atomic() {
    let (_dir, db) = temp_db();
    let ops: Vec<(&[u8], &[u8])> = vec![(b"x", b"1"), (b"y", b"2"), (b"z", b"3")];
    db.put_batch(&ops).unwrap();

    assert_eq!(db.get(b"x").unwrap(), Some(b"1".to_vec()));
    assert_eq!(db.get(b"y").unwrap(), Some(b"2".to_vec()));
    assert_eq!(db.get(b"z").unwrap(), Some(b"3".to_vec()));
}

#[test]
fn delete_batch_atomic() {
    let (_dir, db) = temp_db();
    db.put(b"a", b"1").unwrap();
    db.put(b"b", b"2").unwrap();
    db.put(b"c", b"3").unwrap();

    db.delete_batch(&[b"a", b"c"]).unwrap();

    assert_eq!(db.get(b"a").unwrap(), None);
    assert_eq!(db.get(b"b").unwrap(), Some(b"2".to_vec()));
    assert_eq!(db.get(b"c").unwrap(), None);
}

#[test]
fn contains_key() {
    let (_dir, db) = temp_db();
    db.put(b"exists", b"yes").unwrap();
    assert!(db.contains(b"exists").unwrap());
    assert!(!db.contains(b"nope").unwrap());
}

#[test]
fn overwrite_existing_key() {
    let (_dir, db) = temp_db();
    db.put(b"key", b"v1").unwrap();
    db.put(b"key", b"v2").unwrap();
    assert_eq!(db.get(b"key").unwrap(), Some(b"v2".to_vec()));
}

#[test]
fn concurrent_readers() {
    let (_dir, db) = temp_db();
    db.put(b"shared", b"value").unwrap();

    let handles: Vec<_> = (0..4)
        .map(|_| {
            let db = db.clone();
            std::thread::spawn(move || {
                for _ in 0..100 {
                    assert_eq!(db.get(b"shared").unwrap(), Some(b"value".to_vec()));
                }
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

// -----------------------------------------------------------------------
// DllbStorage (high-level API) tests
// -----------------------------------------------------------------------

#[test]
fn dllb_storage_roundtrip() {
    let (_dir, storage) = temp_storage();

    let key = key::document_key("default", "mydb", "user", "tobie");
    storage.put(&key, b"{\"name\":\"Tobie\"}").unwrap();

    let val = storage.get(&key).unwrap();
    assert_eq!(val, Some(b"{\"name\":\"Tobie\"}".to_vec()));
}

#[test]
fn dllb_storage_prefix_scan() {
    let (_dir, storage) = temp_storage();

    for id in ["a", "b", "c"] {
        let k = key::document_key("ns", "db", "tbl", id);
        storage.put(&k, id.as_bytes()).unwrap();
    }
    // Different table -- should not appear.
    let other = key::document_key("ns", "db", "other", "x");
    storage.put(&other, b"x").unwrap();

    let prefix = key::table_prefix("ns", "db", "tbl", key::tag::DOCUMENT);
    let results = storage.prefix_scan(&prefix).unwrap();
    assert_eq!(results.len(), 3);
}

#[test]
fn dllb_storage_graph_edges_sorted() {
    let (_dir, storage) = temp_storage();

    let e1 = key::graph_edge_key("ns", "db", "edges", "alice", "knows", "bob");
    let e2 = key::graph_edge_key("ns", "db", "edges", "alice", "knows", "carol");
    let e3 = key::graph_edge_key("ns", "db", "edges", "alice", "likes", "dave");

    storage.put(&e1, b"").unwrap();
    storage.put(&e2, b"").unwrap();
    storage.put(&e3, b"").unwrap();

    // Scan all edges from alice.
    let prefix = key::KeyBuilder::new()
        .namespace("ns")
        .database("db")
        .table("edges")
        .tag(key::tag::GRAPH_EDGE)
        .segment(b"alice")
        .build();

    let results = storage.prefix_scan(&prefix).unwrap();
    // All 3 edges from alice (knows->bob, knows->carol, likes->dave).
    assert_eq!(results.len(), 3);

    // Scan only "knows" edges from alice.
    let knows_prefix = key::KeyBuilder::new()
        .namespace("ns")
        .database("db")
        .table("edges")
        .tag(key::tag::GRAPH_EDGE)
        .segment(b"alice")
        .segment(b"knows")
        .build();

    let knows_results = storage.prefix_scan(&knows_prefix).unwrap();
    assert_eq!(knows_results.len(), 2);
}
