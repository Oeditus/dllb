//! Concurrent access test: multiple readers + one writer.

use std::sync::Arc;

use dllb_core::RecordId;
use dllb_core::Value;
use dllb_document::{Collection, Document};
use dllb_storage::db::DllbStorage;

#[test]
fn concurrent_writer_and_readers() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("concurrent.redb");
    let storage = Arc::new(DllbStorage::open(&path).unwrap());

    let n = 50; // documents to create

    // Writer thread: creates n documents sequentially.
    let writer_storage = Arc::clone(&storage);
    let writer = std::thread::spawn(move || {
        let coll = Collection::new(&writer_storage, "ns", "db", "item");
        for i in 0..n {
            let id = format!("item_{i}");
            let doc = Document::new(RecordId::new("item", &id)).with_field("idx", Value::Int(i));
            coll.create(doc).unwrap();
        }
    });

    // Reader threads: continuously count documents.
    let readers: Vec<_> = (0..4)
        .map(|_| {
            let reader_storage = Arc::clone(&storage);
            std::thread::spawn(move || {
                let coll = Collection::new(&reader_storage, "ns", "db", "item");
                let mut max_seen = 0usize;
                for _ in 0..100 {
                    let count = coll.count().unwrap();
                    if count > max_seen {
                        max_seen = count;
                    }
                    // Small sleep to avoid busy-waiting.
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                max_seen
            })
        })
        .collect();

    writer.join().expect("writer panicked");

    for reader in readers {
        let _max = reader.join().expect("reader panicked");
        // We just verify no panics and no corruption.
    }

    // Final verification: all documents present.
    let coll = Collection::new(&storage, "ns", "db", "item");
    assert_eq!(coll.count().unwrap(), n as usize);
}
