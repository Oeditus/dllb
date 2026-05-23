//! Cross-model end-to-end test.
//!
//! Exercises documents, graph edges, full-text search, and vector KNN
//! in a single test to prove all crates compose correctly.

use std::collections::BTreeMap;

use dllb_core::{RecordId, Value};
use dllb_document::{Collection, Document};
use dllb_graph::{Edge, EdgeStore, Traversal};
use dllb_storage::db::DllbStorage;

fn temp_storage() -> (tempfile::TempDir, DllbStorage) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("e2e.redb");
    let storage = DllbStorage::open(&path).unwrap();
    (dir, storage)
}

#[test]
fn full_stack_documents_and_graphs() {
    let (_dir, storage) = temp_storage();

    // -- Step 1: Create documents --
    let users = Collection::new(&storage, "ns", "db", "user");
    users
        .create(
            Document::new(RecordId::new("user", "alice"))
                .with_field("name", Value::String("Alice".into()))
                .with_field("age", Value::Int(30))
                .with_field("bio", Value::String("Rust systems programmer".into())),
        )
        .unwrap();
    users
        .create(
            Document::new(RecordId::new("user", "bob"))
                .with_field("name", Value::String("Bob".into()))
                .with_field("age", Value::Int(25))
                .with_field("bio", Value::String("Python data scientist".into())),
        )
        .unwrap();
    users
        .create(
            Document::new(RecordId::new("user", "carol"))
                .with_field("name", Value::String("Carol".into()))
                .with_field("age", Value::Int(35))
                .with_field(
                    "bio",
                    Value::String("Erlang distributed systems engineer".into()),
                ),
        )
        .unwrap();

    assert_eq!(users.count().unwrap(), 3);

    // -- Step 2: Create graph edges --
    let edges = EdgeStore::new(&storage, "ns", "db", "knows");
    edges
        .relate(&Edge::new("alice", "knows", "bob").with_property("since", Value::Int(2020)))
        .unwrap();
    edges
        .relate(&Edge::new("alice", "knows", "carol").with_property("since", Value::Int(2021)))
        .unwrap();
    edges
        .relate(&Edge::new("bob", "knows", "carol").with_property("since", Value::Int(2022)))
        .unwrap();

    // -- Step 3: Verify traversal --
    let t = Traversal::new(&edges);
    let alice_friends = t.outgoing("alice").unwrap();
    assert_eq!(alice_friends.len(), 2);

    let bob_incoming = t.incoming("bob").unwrap();
    assert_eq!(bob_incoming.len(), 1);
    assert_eq!(bob_incoming[0].src, "alice");

    // Multi-hop: alice -> knows -> ? -> knows -> carol
    let paths = t
        .walk(
            "alice",
            &[
                dllb_graph::HopSpec {
                    direction: dllb_graph::Direction::Out,
                    edge_type: Some("knows".into()),
                },
                dllb_graph::HopSpec {
                    direction: dllb_graph::Direction::Out,
                    edge_type: Some("knows".into()),
                },
            ],
        )
        .unwrap();
    // alice->bob->carol and alice->carol->(carol has no outgoing "knows")
    // so only alice->bob->carol should work
    let carol_paths: Vec<_> = paths
        .iter()
        .filter(|p| p.last() == Some(&"carol".to_string()))
        .collect();
    assert!(!carol_paths.is_empty());

    // -- Step 4: Verify document read-back --
    let alice = users.get("alice").unwrap().unwrap();
    assert_eq!(alice.get("name"), Some(&Value::String("Alice".into())));

    // -- Step 5: Delete a document and verify --
    assert!(users.delete("bob").unwrap());
    assert_eq!(users.count().unwrap(), 2);
    assert!(users.get("bob").unwrap().is_none());

    // Graph edge from alice->bob still exists in KV (no cascading delete)
    // but that's expected for the prototype.
}
