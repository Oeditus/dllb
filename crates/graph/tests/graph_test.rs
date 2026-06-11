//! Integration tests for the graph model.

use std::collections::BTreeMap;

use dllb_core::Value;
use dllb_graph::{Direction, Edge, EdgeStore, HopSpec, Traversal};
use dllb_storage::db::DllbStorage;

fn temp_storage() -> (tempfile::TempDir, DllbStorage) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.redb");
    let storage = DllbStorage::open(&path).unwrap();
    (dir, storage)
}

fn es(storage: &DllbStorage) -> EdgeStore<'_> {
    EdgeStore::new(storage, "ns", "db", "edges")
}

// -------------------------------------------------------------------
// Edge CRUD
// -------------------------------------------------------------------

#[test]
fn relate_and_get_roundtrip() {
    let (_dir, storage) = temp_storage();
    let store = es(&storage);

    let edge = Edge::new("alice", "knows", "bob");
    store.relate(&edge).unwrap();

    let got = store.get("alice", "knows", "bob").unwrap().unwrap();
    assert_eq!(got.src, "alice");
    assert_eq!(got.edge_type, "knows");
    assert_eq!(got.dst, "bob");
    assert!(got.properties.is_empty());
}

#[test]
fn relate_with_properties() {
    let (_dir, storage) = temp_storage();
    let store = es(&storage);

    let edge = Edge::new("alice", "knows", "bob")
        .with_property("since", Value::Int(2020))
        .with_property("weight", Value::Float(0.9));
    store.relate(&edge).unwrap();

    let got = store.get("alice", "knows", "bob").unwrap().unwrap();
    assert_eq!(got.properties.get("since"), Some(&Value::Int(2020)));
    assert_eq!(got.properties.get("weight"), Some(&Value::Float(0.9)));
}

#[test]
fn get_missing_returns_none() {
    let (_dir, storage) = temp_storage();
    let store = es(&storage);
    assert!(store.get("alice", "knows", "bob").unwrap().is_none());
}

#[test]
fn delete_returns_true_then_false() {
    let (_dir, storage) = temp_storage();
    let store = es(&storage);

    store.relate(&Edge::new("alice", "knows", "bob")).unwrap();
    assert!(store.delete("alice", "knows", "bob").unwrap());
    assert!(!store.delete("alice", "knows", "bob").unwrap());
    assert!(store.get("alice", "knows", "bob").unwrap().is_none());
}

#[test]
fn update_properties() {
    let (_dir, storage) = temp_storage();
    let store = es(&storage);

    store
        .relate(&Edge::new("alice", "knows", "bob").with_property("weight", Value::Float(0.5)))
        .unwrap();

    let mut new_props = BTreeMap::new();
    new_props.insert("weight".into(), Value::Float(0.9));
    store
        .update_properties("alice", "knows", "bob", new_props)
        .unwrap();

    let got = store.get("alice", "knows", "bob").unwrap().unwrap();
    assert_eq!(got.properties.get("weight"), Some(&Value::Float(0.9)));
}

// -------------------------------------------------------------------
// Traversal: single-hop
// -------------------------------------------------------------------

#[test]
fn outgoing_all() {
    let (_dir, storage) = temp_storage();
    let store = es(&storage);

    store.relate(&Edge::new("alice", "knows", "bob")).unwrap();
    store.relate(&Edge::new("alice", "knows", "carol")).unwrap();
    store.relate(&Edge::new("alice", "likes", "dave")).unwrap();

    let t = Traversal::new(&store);
    let edges = t.outgoing("alice").unwrap();
    assert_eq!(edges.len(), 3);
}

#[test]
fn outgoing_typed() {
    let (_dir, storage) = temp_storage();
    let store = es(&storage);

    store.relate(&Edge::new("alice", "knows", "bob")).unwrap();
    store.relate(&Edge::new("alice", "knows", "carol")).unwrap();
    store.relate(&Edge::new("alice", "likes", "dave")).unwrap();

    let t = Traversal::new(&store);
    let knows = t.outgoing_typed("alice", "knows").unwrap();
    assert_eq!(knows.len(), 2);

    let likes = t.outgoing_typed("alice", "likes").unwrap();
    assert_eq!(likes.len(), 1);
    assert_eq!(likes[0].dst, "dave");
}

#[test]
fn incoming_all() {
    let (_dir, storage) = temp_storage();
    let store = es(&storage);

    store.relate(&Edge::new("alice", "knows", "bob")).unwrap();
    store.relate(&Edge::new("carol", "knows", "bob")).unwrap();
    store.relate(&Edge::new("dave", "likes", "bob")).unwrap();

    let t = Traversal::new(&store);
    let edges = t.incoming("bob").unwrap();
    assert_eq!(edges.len(), 3);
}

#[test]
fn incoming_typed() {
    let (_dir, storage) = temp_storage();
    let store = es(&storage);

    store.relate(&Edge::new("alice", "knows", "bob")).unwrap();
    store.relate(&Edge::new("carol", "knows", "bob")).unwrap();
    store.relate(&Edge::new("dave", "likes", "bob")).unwrap();

    let t = Traversal::new(&store);
    let knows = t.incoming_typed("bob", "knows").unwrap();
    assert_eq!(knows.len(), 2);
}

#[test]
fn bidirectional_consistency() {
    let (_dir, storage) = temp_storage();
    let store = es(&storage);

    store.relate(&Edge::new("alice", "knows", "bob")).unwrap();

    let t = Traversal::new(&store);
    // alice's outgoing "knows" edges should include bob.
    let out = t.outgoing_typed("alice", "knows").unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].dst, "bob");

    // bob's incoming "knows" edges should include alice.
    let inc = t.incoming_typed("bob", "knows").unwrap();
    assert_eq!(inc.len(), 1);
    assert_eq!(inc[0].src, "alice");
}

#[test]
fn delete_removes_both_directions() {
    let (_dir, storage) = temp_storage();
    let store = es(&storage);

    store.relate(&Edge::new("alice", "knows", "bob")).unwrap();
    store.delete("alice", "knows", "bob").unwrap();

    let t = Traversal::new(&store);
    assert!(t.outgoing_typed("alice", "knows").unwrap().is_empty());
    assert!(t.incoming_typed("bob", "knows").unwrap().is_empty());
}

// -------------------------------------------------------------------
// Traversal: neighbor-only (IDs without edge properties)
// -------------------------------------------------------------------

#[test]
fn outgoing_neighbors_match_outgoing_dsts() {
    let (_dir, storage) = temp_storage();
    let store = es(&storage);

    store.relate(&Edge::new("alice", "knows", "bob")).unwrap();
    store.relate(&Edge::new("alice", "knows", "carol")).unwrap();
    store.relate(&Edge::new("alice", "likes", "dave")).unwrap();

    let t = Traversal::new(&store);
    // All edge types.
    let mut all = t.outgoing_neighbors("alice").unwrap();
    all.sort();
    assert_eq!(all, vec!["bob", "carol", "dave"]);

    // Typed.
    let mut knows = t.outgoing_neighbors_typed("alice", "knows").unwrap();
    knows.sort();
    assert_eq!(knows, vec!["bob", "carol"]);
}

#[test]
fn incoming_neighbors_match_incoming_srcs() {
    let (_dir, storage) = temp_storage();
    let store = es(&storage);

    store.relate(&Edge::new("alice", "knows", "bob")).unwrap();
    store.relate(&Edge::new("carol", "knows", "bob")).unwrap();
    store.relate(&Edge::new("dave", "likes", "bob")).unwrap();

    let t = Traversal::new(&store);
    let mut all = t.incoming_neighbors("bob").unwrap();
    all.sort();
    assert_eq!(all, vec!["alice", "carol", "dave"]);

    let mut knows = t.incoming_neighbors_typed("bob", "knows").unwrap();
    knows.sort();
    assert_eq!(knows, vec!["alice", "carol"]);
}

#[test]
fn neighbors_empty_for_unknown_vertex() {
    let (_dir, storage) = temp_storage();
    let store = es(&storage);
    let t = Traversal::new(&store);

    assert!(t.outgoing_neighbors("nobody").unwrap().is_empty());
    assert!(t.incoming_neighbors("nobody").unwrap().is_empty());
}

#[test]
fn scan_all_edges_returns_outgoing_pairs_only() {
    let (_dir, storage) = temp_storage();
    let store = es(&storage);

    store.relate(&Edge::new("a", "knows", "b")).unwrap();
    store.relate(&Edge::new("b", "knows", "c")).unwrap();

    // Key-only scan must yield exactly the outgoing (src, dst) pairs, never
    // the incoming reverse pointers.
    let mut edges = store.scan_all_edges().unwrap();
    edges.sort();
    assert_eq!(
        edges,
        vec![
            ("a".to_string(), "b".to_string()),
            ("b".to_string(), "c".to_string())
        ]
    );
}

// -------------------------------------------------------------------
// Traversal: multi-hop walk
// -------------------------------------------------------------------

#[test]
fn walk_two_hops() {
    let (_dir, storage) = temp_storage();
    let store = es(&storage);

    store.relate(&Edge::new("alice", "knows", "bob")).unwrap();
    store.relate(&Edge::new("bob", "likes", "widget")).unwrap();

    let t = Traversal::new(&store);
    let paths = t
        .walk(
            "alice",
            &[
                HopSpec {
                    direction: Direction::Out,
                    edge_type: Some("knows".into()),
                },
                HopSpec {
                    direction: Direction::Out,
                    edge_type: Some("likes".into()),
                },
            ],
        )
        .unwrap();

    assert_eq!(paths, vec![vec!["alice", "bob", "widget"]]);
}

#[test]
fn walk_three_hops() {
    let (_dir, storage) = temp_storage();
    let store = es(&storage);

    store.relate(&Edge::new("a", "e1", "b")).unwrap();
    store.relate(&Edge::new("b", "e2", "c")).unwrap();
    store.relate(&Edge::new("c", "e3", "d")).unwrap();

    let t = Traversal::new(&store);
    let paths = t
        .walk(
            "a",
            &[
                HopSpec {
                    direction: Direction::Out,
                    edge_type: Some("e1".into()),
                },
                HopSpec {
                    direction: Direction::Out,
                    edge_type: Some("e2".into()),
                },
                HopSpec {
                    direction: Direction::Out,
                    edge_type: Some("e3".into()),
                },
            ],
        )
        .unwrap();

    assert_eq!(paths, vec![vec!["a", "b", "c", "d"]]);
}

#[test]
fn walk_fan_out() {
    let (_dir, storage) = temp_storage();
    let store = es(&storage);

    store.relate(&Edge::new("a", "knows", "b")).unwrap();
    store.relate(&Edge::new("a", "knows", "c")).unwrap();
    store.relate(&Edge::new("b", "likes", "x")).unwrap();
    store.relate(&Edge::new("c", "likes", "y")).unwrap();

    let t = Traversal::new(&store);
    let paths = t
        .walk(
            "a",
            &[
                HopSpec {
                    direction: Direction::Out,
                    edge_type: Some("knows".into()),
                },
                HopSpec {
                    direction: Direction::Out,
                    edge_type: Some("likes".into()),
                },
            ],
        )
        .unwrap();

    assert_eq!(paths.len(), 2);
}

// -------------------------------------------------------------------
// Filtered traversal
// -------------------------------------------------------------------

#[test]
fn outgoing_filtered_by_property() {
    let (_dir, storage) = temp_storage();
    let store = es(&storage);

    store
        .relate(&Edge::new("alice", "knows", "bob").with_property("close", Value::Bool(true)))
        .unwrap();
    store
        .relate(&Edge::new("alice", "knows", "carol").with_property("close", Value::Bool(false)))
        .unwrap();

    let t = Traversal::new(&store);
    let close_friends = t
        .outgoing_filtered("alice", |e| {
            e.properties.get("close") == Some(&Value::Bool(true))
        })
        .unwrap();

    assert_eq!(close_friends.len(), 1);
    assert_eq!(close_friends[0].dst, "bob");
}

// -------------------------------------------------------------------
// Isolation
// -------------------------------------------------------------------

#[test]
fn cross_table_isolation() {
    let (_dir, storage) = temp_storage();
    let social = EdgeStore::new(&storage, "ns", "db", "social");
    let work = EdgeStore::new(&storage, "ns", "db", "work");

    social.relate(&Edge::new("alice", "knows", "bob")).unwrap();
    work.relate(&Edge::new("alice", "reports_to", "boss"))
        .unwrap();

    let st = Traversal::new(&social);
    let wt = Traversal::new(&work);

    assert_eq!(st.outgoing("alice").unwrap().len(), 1);
    assert_eq!(wt.outgoing("alice").unwrap().len(), 1);
    assert_eq!(st.outgoing("alice").unwrap()[0].edge_type, "knows");
    assert_eq!(wt.outgoing("alice").unwrap()[0].edge_type, "reports_to");
}

#[test]
fn empty_traversal() {
    let (_dir, storage) = temp_storage();
    let store = es(&storage);
    let t = Traversal::new(&store);

    assert!(t.outgoing("nobody").unwrap().is_empty());
    assert!(t.incoming("nobody").unwrap().is_empty());
}
