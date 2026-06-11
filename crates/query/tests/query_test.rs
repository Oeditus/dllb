//! Integration tests for the query engine.

use std::sync::Arc;

use dllb_core::Value;
use dllb_graph::{EdgeStore, Traversal};
use dllb_query::{ComputeCache, QueryExecutor, QueryResult, WriteVersions};
use dllb_storage::db::DllbStorage;

fn temp_storage() -> (tempfile::TempDir, DllbStorage) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.redb");
    let storage = DllbStorage::open(&path).unwrap();
    (dir, storage)
}

fn exec(storage: &DllbStorage) -> QueryExecutor<'_> {
    QueryExecutor::new(storage, "ns", "db")
}

/// Build an executor that shares real caches — for testing cache behaviour.
fn exec_with_shared_cache<'s>(
    storage: &'s DllbStorage,
    cache: Arc<ComputeCache>,
    versions: Arc<WriteVersions>,
) -> QueryExecutor<'s> {
    QueryExecutor::new_with_cache(storage, "ns", "db", cache, versions)
}

// -------------------------------------------------------------------
// CREATE + SELECT roundtrip
// -------------------------------------------------------------------

#[test]
fn create_and_select_star() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    e.run("CREATE user:alice SET name = 'Alice', age = 30;")
        .unwrap();
    e.run("CREATE user:bob SET name = 'Bob', age = 25;")
        .unwrap();

    let (result, _outcome) = e.run("SELECT * FROM user;").unwrap();
    match result {
        QueryResult::Rows(rows) => {
            assert_eq!(rows.len(), 2);
            // Both rows should have name, age, and id fields.
            assert!(
                rows.iter()
                    .all(|r| r.contains_key("name") && r.contains_key("age"))
            );
        }
        _ => panic!("expected Rows"),
    }
}

// -------------------------------------------------------------------
// UPDATE
// -------------------------------------------------------------------

#[test]
fn update_record_merges_fields() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    e.run("CREATE user:alice SET name = 'Alice', age = 30;")
        .unwrap();

    let (result, _) = e.run("UPDATE user:alice SET age = 31;").unwrap();
    assert!(matches!(result, QueryResult::Update { matched: 1 }));

    // age updated, name preserved (partial SET semantics).
    let (result, _) = e.run("SELECT * FROM user:alice;").unwrap();
    match result {
        QueryResult::Rows(rows) => {
            assert_eq!(rows[0].get("name"), Some(&Value::String("Alice".into())));
            assert_eq!(rows[0].get("age"), Some(&Value::Int(31)));
        }
        _ => panic!("expected Rows"),
    }
}

#[test]
fn update_missing_record_matches_zero() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    let (result, _) = e.run("UPDATE user:ghost SET age = 1;").unwrap();
    assert!(matches!(result, QueryResult::Update { matched: 0 }));
}

#[test]
fn update_table_with_where_matches_subset() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    e.run("CREATE user:a SET age = 30;").unwrap();
    e.run("CREATE user:b SET age = 30;").unwrap();
    e.run("CREATE user:c SET age = 20;").unwrap();

    let (result, _) = e
        .run("UPDATE user SET tier = 'gold' WHERE age = 30;")
        .unwrap();
    assert!(matches!(result, QueryResult::Update { matched: 2 }));

    let (result, _) = e.run("SELECT * FROM user WHERE tier = 'gold';").unwrap();
    match result {
        QueryResult::Rows(rows) => assert_eq!(rows.len(), 2),
        _ => panic!("expected Rows"),
    }
}

#[test]
fn update_table_without_where_updates_all() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    e.run("CREATE user:a SET age = 30;").unwrap();
    e.run("CREATE user:b SET age = 40;").unwrap();

    let (result, _) = e.run("UPDATE user SET active = true;").unwrap();
    assert!(matches!(result, QueryResult::Update { matched: 2 }));
}

// -------------------------------------------------------------------
// COUNT + IS [NOT] NONE
// -------------------------------------------------------------------

#[test]
fn count_all_rows() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    e.run("CREATE user:a SET name = 'A';").unwrap();
    e.run("CREATE user:b SET name = 'B';").unwrap();
    e.run("CREATE user:c SET name = 'C';").unwrap();

    let (result, _) = e.run("COUNT user;").unwrap();
    assert!(matches!(result, QueryResult::Count { count: 3 }));
}

#[test]
fn count_with_where() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    e.run("CREATE user:a SET age = 30;").unwrap();
    e.run("CREATE user:b SET age = 30;").unwrap();
    e.run("CREATE user:c SET age = 20;").unwrap();

    let (result, _) = e.run("COUNT user WHERE age = 30;").unwrap();
    assert!(matches!(result, QueryResult::Count { count: 2 }));
}

#[test]
fn count_is_not_none_and_is_none() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    // Two rows carry `tag`, one does not.
    e.run("CREATE node:a SET name = 'a', tag = 'x';").unwrap();
    e.run("CREATE node:b SET name = 'b', tag = 'y';").unwrap();
    e.run("CREATE node:c SET name = 'c';").unwrap();

    let (set_count, _) = e.run("COUNT node WHERE tag IS NOT NONE;").unwrap();
    assert!(matches!(set_count, QueryResult::Count { count: 2 }));

    let (unset_count, _) = e.run("COUNT node WHERE tag IS NONE;").unwrap();
    assert!(matches!(unset_count, QueryResult::Count { count: 1 }));
}

#[test]
fn count_is_not_none_counts_embeddings_set_via_update() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    // Simulate the embedding-write path: rows exist, then a value is attached
    // by a filtered UPDATE. Only updated rows count as "vectors".
    e.run("CREATE ast_node:f1 SET kind = 'function_def';")
        .unwrap();
    e.run("CREATE ast_node:f2 SET kind = 'function_def';")
        .unwrap();

    let (before, _) = e.run("COUNT ast_node WHERE emb IS NOT NONE;").unwrap();
    assert!(matches!(before, QueryResult::Count { count: 0 }));

    e.run("UPDATE ast_node:f1 SET emb = 'vec';").unwrap();

    let (after, _) = e.run("COUNT ast_node WHERE emb IS NOT NONE;").unwrap();
    assert!(matches!(after, QueryResult::Count { count: 1 }));
}

#[test]
fn update_stores_array_embedding_counted_by_is_not_none() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    e.run("CREATE ast_node:f1 SET kind = 'function_def';")
        .unwrap();

    // Array literal exercising decimal, negative, and exponent components --
    // the shape an embedding write produces.
    e.run("UPDATE ast_node:f1 SET source_embedding = [0.1, -0.2, 1.0e-5];")
        .unwrap();

    let (count, _) = e
        .run("COUNT ast_node WHERE source_embedding IS NOT NONE;")
        .unwrap();
    assert!(matches!(count, QueryResult::Count { count: 1 }));

    // The value round-trips as a Value::Array of floats.
    let (rows, _) = e.run("SELECT * FROM ast_node:f1;").unwrap();
    match rows {
        QueryResult::Rows(rows) => match rows[0].get("source_embedding") {
            Some(Value::Array(items)) => {
                assert_eq!(items.len(), 3);
                assert_eq!(items[0], Value::Float(0.1));
                assert_eq!(items[1], Value::Float(-0.2));
            }
            other => panic!("expected array embedding, got {other:?}"),
        },
        _ => panic!("expected Rows"),
    }
}

// -------------------------------------------------------------------
// GRAPH COMPONENTS
// -------------------------------------------------------------------

/// Two disjoint triangles on the `calls` edge table (no bridge).
fn build_two_disjoint_triangles(storage: &DllbStorage) {
    let e = exec(storage);
    for (a, b) in [("a1", "a2"), ("a2", "a3"), ("a3", "a1")] {
        e.run(&format!("RELATE fn:{a}->calls->fn:{b}")).unwrap();
    }
    for (a, b) in [("b1", "b2"), ("b2", "b3"), ("b3", "b1")] {
        e.run(&format!("RELATE fn:{a}->calls->fn:{b}")).unwrap();
    }
}

#[test]
fn components_counts_disjoint_clusters() {
    let (_dir, storage) = temp_storage();
    build_two_disjoint_triangles(&storage);

    let e = exec(&storage);
    let (result, _) = e.run("GRAPH COMPONENTS calls").unwrap();
    match result {
        QueryResult::Components {
            count,
            largest,
            nodes,
        } => {
            assert_eq!(count, 2);
            assert_eq!(largest, 3);
            assert_eq!(nodes, 6);
        }
        _ => panic!("expected Components"),
    }
}

#[test]
fn components_bridge_merges_clusters() {
    let (_dir, storage) = temp_storage();
    // Two clusters joined by a weak bridge -> a single component.
    build_two_cluster_graph(&storage);

    let e = exec(&storage);
    let (result, _) = e.run("GRAPH COMPONENTS calls").unwrap();
    match result {
        QueryResult::Components { count, nodes, .. } => {
            assert_eq!(count, 1);
            assert_eq!(nodes, 6);
        }
        _ => panic!("expected Components"),
    }
}

#[test]
fn components_empty_graph() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    let (result, _) = e.run("GRAPH COMPONENTS calls").unwrap();
    match result {
        QueryResult::Components {
            count,
            largest,
            nodes,
        } => {
            assert_eq!(count, 0);
            assert_eq!(largest, 0);
            assert_eq!(nodes, 0);
        }
        _ => panic!("expected Components"),
    }
}

#[test]
fn components_second_call_is_cache_hit_and_relate_invalidates() {
    let (_dir, storage) = temp_storage();
    build_two_disjoint_triangles(&storage);

    let cache = Arc::new(ComputeCache::default());
    let versions = Arc::new(WriteVersions::default());

    let e1 = exec_with_shared_cache(&storage, Arc::clone(&cache), Arc::clone(&versions));
    let (first, _) = e1.run("GRAPH COMPONENTS calls").unwrap();
    assert!(matches!(first, QueryResult::Components { .. }));

    let e2 = exec_with_shared_cache(&storage, Arc::clone(&cache), Arc::clone(&versions));
    let (second, _) = e2.run("GRAPH COMPONENTS calls").unwrap();
    assert!(
        matches!(second, QueryResult::CachedResponse(_)),
        "second call should hit the cache"
    );

    // A new edge bumps the version and invalidates the cache.
    let e3 = exec_with_shared_cache(&storage, Arc::clone(&cache), Arc::clone(&versions));
    e3.run("RELATE fn:a1->calls->fn:b1").unwrap();

    let e4 = exec_with_shared_cache(&storage, Arc::clone(&cache), Arc::clone(&versions));
    let (after, _) = e4.run("GRAPH COMPONENTS calls").unwrap();
    match after {
        QueryResult::Components { count, .. } => assert_eq!(count, 1),
        _ => panic!("expected recomputed Components after RELATE"),
    }
}

#[test]
fn create_returns_id() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    let (result, _outcome) = e.run("CREATE user:alice SET name = 'Alice';").unwrap();
    match result {
        QueryResult::Created { id } => {
            assert_eq!(id.table, "user");
            assert_eq!(id.id, "alice");
        }
        _ => panic!("expected Created"),
    }
}

// -------------------------------------------------------------------
// SELECT named fields
// -------------------------------------------------------------------

#[test]
fn select_named_fields() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    e.run("CREATE user:alice SET name = 'Alice', age = 30;")
        .unwrap();

    let (result, _outcome) = e.run("SELECT name FROM user;").unwrap();
    match result {
        QueryResult::Rows(rows) => {
            assert_eq!(rows.len(), 1);
            assert!(rows[0].contains_key("name"));
            assert!(!rows[0].contains_key("age"));
        }
        _ => panic!("expected Rows"),
    }
}

#[test]
fn select_limit_caps_rows() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    e.run("CREATE user:a SET name = 'A';").unwrap();
    e.run("CREATE user:b SET name = 'B';").unwrap();
    e.run("CREATE user:c SET name = 'C';").unwrap();

    let (result, _outcome) = e.run("SELECT * FROM user LIMIT 2;").unwrap();
    match result {
        QueryResult::Rows(rows) => {
            assert_eq!(rows.len(), 2);
        }
        _ => panic!("expected Rows"),
    }
}

#[test]
fn select_where_limit_caps_filtered_rows() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    e.run("CREATE user:a SET age = 30;").unwrap();
    e.run("CREATE user:b SET age = 30;").unwrap();
    e.run("CREATE user:c SET age = 30;").unwrap();
    e.run("CREATE user:d SET age = 20;").unwrap();

    let (result, _outcome) = e.run("SELECT * FROM user WHERE age = 30 LIMIT 2;").unwrap();
    match result {
        QueryResult::Rows(rows) => {
            assert_eq!(rows.len(), 2);
            assert!(
                rows.iter()
                    .all(|row| row.get("age") == Some(&Value::Int(30)))
            );
        }
        _ => panic!("expected Rows"),
    }
}

// -------------------------------------------------------------------
// SELECT point lookup (table:id)
// -------------------------------------------------------------------

#[test]
fn select_point_lookup() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    e.run("CREATE user:alice SET name = 'Alice';").unwrap();
    e.run("CREATE user:bob SET name = 'Bob';").unwrap();

    let (result, _outcome) = e.run("SELECT * FROM user:alice;").unwrap();
    match result {
        QueryResult::Rows(rows) => {
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].get("name"), Some(&Value::String("Alice".into())));
        }
        _ => panic!("expected Rows"),
    }
}

#[test]
fn select_missing_record_returns_empty() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    let (result, _outcome) = e.run("SELECT * FROM user:ghost;").unwrap();
    match result {
        QueryResult::Rows(rows) => assert!(rows.is_empty()),
        _ => panic!("expected Rows"),
    }
}

// -------------------------------------------------------------------
// DELETE
// -------------------------------------------------------------------

#[test]
fn delete_removes_record() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    e.run("CREATE user:alice SET name = 'Alice';").unwrap();

    let (result, _outcome) = e.run("DELETE user:alice;").unwrap();
    match result {
        QueryResult::Deleted { existed } => assert!(existed),
        _ => panic!("expected Deleted"),
    }

    // Verify gone.
    let (result, _outcome) = e.run("SELECT * FROM user:alice;").unwrap();
    match result {
        QueryResult::Rows(rows) => assert!(rows.is_empty()),
        _ => panic!("expected Rows"),
    }
}

// -------------------------------------------------------------------
// WHERE clause
// -------------------------------------------------------------------

#[test]
fn select_where_filters() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    e.run("CREATE user:alice SET name = 'Alice', age = 30;")
        .unwrap();
    e.run("CREATE user:bob SET name = 'Bob', age = 25;")
        .unwrap();
    e.run("CREATE user:carol SET name = 'Carol', age = 30;")
        .unwrap();

    let (result, _outcome) = e.run("SELECT * FROM user WHERE age = 30;").unwrap();
    match result {
        QueryResult::Rows(rows) => {
            assert_eq!(rows.len(), 2);
            let names: Vec<&Value> = rows.iter().filter_map(|r| r.get("name")).collect();
            assert!(names.contains(&&Value::String("Alice".into())));
            assert!(names.contains(&&Value::String("Carol".into())));
        }
        _ => panic!("expected Rows"),
    }
}

// -------------------------------------------------------------------
// RELATE
// -------------------------------------------------------------------

#[test]
fn relate_creates_edge() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    let (result, _outcome) = e
        .run("RELATE user:alice->knows->user:bob SET since = 2020;")
        .unwrap();
    assert!(matches!(result, QueryResult::Ok));

    // Verify the edge exists via EdgeStore directly.
    let store = EdgeStore::new(&storage, "ns", "db", "knows");
    let edge = store.get("alice", "knows", "bob").unwrap().unwrap();
    assert_eq!(edge.properties.get("since"), Some(&Value::Int(2020)));
}

#[test]
fn relate_traversable() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    e.run("RELATE user:alice->knows->user:bob;").unwrap();
    e.run("RELATE user:alice->knows->user:carol;").unwrap();

    let store = EdgeStore::new(&storage, "ns", "db", "knows");
    let t = Traversal::new(&store);
    let out = t.outgoing("alice").unwrap();
    assert_eq!(out.len(), 2);
}

// -------------------------------------------------------------------
// WHERE range comparisons
// -------------------------------------------------------------------

#[test]
fn select_where_gt() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    e.run("CREATE user:alice SET name = 'Alice', age = 30;")
        .unwrap();
    e.run("CREATE user:bob SET name = 'Bob', age = 25;")
        .unwrap();
    e.run("CREATE user:carol SET name = 'Carol', age = 40;")
        .unwrap();

    let (result, _outcome) = e.run("SELECT * FROM user WHERE age > 28;").unwrap();
    match result {
        QueryResult::Rows(rows) => {
            assert_eq!(rows.len(), 2);
            let names: Vec<&Value> = rows.iter().filter_map(|r| r.get("name")).collect();
            assert!(names.contains(&&Value::String("Alice".into())));
            assert!(names.contains(&&Value::String("Carol".into())));
        }
        _ => panic!("expected Rows"),
    }
}

#[test]
fn select_where_lt() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    e.run("CREATE user:alice SET age = 30;").unwrap();
    e.run("CREATE user:bob SET age = 25;").unwrap();

    let (result, _outcome) = e.run("SELECT * FROM user WHERE age < 28;").unwrap();
    match result {
        QueryResult::Rows(rows) => {
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].get("age"), Some(&Value::Int(25)));
        }
        _ => panic!("expected Rows"),
    }
}

#[test]
fn select_where_ne() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    e.run("CREATE user:alice SET name = 'Alice';").unwrap();
    e.run("CREATE user:bob SET name = 'Bob';").unwrap();

    let (result, _outcome) = e.run("SELECT * FROM user WHERE name != 'Bob';").unwrap();
    match result {
        QueryResult::Rows(rows) => {
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].get("name"), Some(&Value::String("Alice".into())));
        }
        _ => panic!("expected Rows"),
    }
}

#[test]
fn select_where_and_range() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    e.run("CREATE user:a SET age = 18;").unwrap();
    e.run("CREATE user:b SET age = 25;").unwrap();
    e.run("CREATE user:c SET age = 35;").unwrap();
    e.run("CREATE user:d SET age = 45;").unwrap();

    // 20 <= age <= 30
    let (result, _outcome) = e
        .run("SELECT * FROM user WHERE age >= 20 AND age <= 30;")
        .unwrap();
    match result {
        QueryResult::Rows(rows) => {
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].get("age"), Some(&Value::Int(25)));
        }
        _ => panic!("expected Rows"),
    }
}

// -------------------------------------------------------------------
// Graph traversal SELECT
// -------------------------------------------------------------------

#[test]
fn select_traversal_outgoing() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    // Create records (destination records must exist for lookup).
    e.run("CREATE user:alice SET name = 'Alice';").unwrap();
    e.run("CREATE user:bob SET name = 'Bob';").unwrap();
    e.run("CREATE user:carol SET name = 'Carol';").unwrap();

    // Alice knows Bob and Carol.
    e.run("RELATE user:alice->knows->user:bob;").unwrap();
    e.run("RELATE user:alice->knows->user:carol;").unwrap();

    let (result, _outcome) = e.run("SELECT ->knows->user FROM user:alice;").unwrap();
    match result {
        QueryResult::Rows(rows) => {
            assert_eq!(rows.len(), 2);
            let names: Vec<&Value> = rows.iter().filter_map(|r| r.get("name")).collect();
            assert!(names.contains(&&Value::String("Bob".into())));
            assert!(names.contains(&&Value::String("Carol".into())));
        }
        _ => panic!("expected Rows"),
    }
}

#[test]
fn select_traversal_with_projection() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    e.run("CREATE user:alice SET name = 'Alice', age = 30;")
        .unwrap();
    e.run("CREATE user:bob SET name = 'Bob', age = 25;")
        .unwrap();
    e.run("RELATE user:alice->knows->user:bob;").unwrap();

    let (result, _outcome) = e.run("SELECT ->knows->user.name FROM user:alice;").unwrap();
    match result {
        QueryResult::Rows(rows) => {
            assert_eq!(rows.len(), 1);
            // Projection: only "name" and "id" should be present.
            assert!(rows[0].contains_key("name"));
            assert!(!rows[0].contains_key("age"));
            assert_eq!(rows[0].get("name"), Some(&Value::String("Bob".into())));
        }
        _ => panic!("expected Rows"),
    }
}

#[test]
fn select_traversal_incoming() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    e.run("CREATE user:alice SET name = 'Alice';").unwrap();
    e.run("CREATE user:bob SET name = 'Bob';").unwrap();
    e.run("RELATE user:alice->likes->user:bob;").unwrap();

    // Who likes bob? (incoming edges)
    let (result, _outcome) = e.run("SELECT <-likes<-user FROM user:bob;").unwrap();
    match result {
        QueryResult::Rows(rows) => {
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].get("name"), Some(&Value::String("Alice".into())));
        }
        _ => panic!("expected Rows"),
    }
}

#[test]
fn select_traversal_with_where_on_dest() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    e.run("CREATE user:alice SET name = 'Alice', age = 30;")
        .unwrap();
    e.run("CREATE user:bob SET name = 'Bob', age = 25;")
        .unwrap();
    e.run("CREATE user:carol SET name = 'Carol', age = 40;")
        .unwrap();
    e.run("RELATE user:alice->knows->user:bob;").unwrap();
    e.run("RELATE user:alice->knows->user:carol;").unwrap();

    // Among alice's connections, only those with age > 35.
    let (result, _outcome) = e
        .run("SELECT ->knows->user FROM user:alice WHERE age > 35;")
        .unwrap();
    match result {
        QueryResult::Rows(rows) => {
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].get("name"), Some(&Value::String("Carol".into())));
        }
        _ => panic!("expected Rows"),
    }
}

#[test]
fn select_traversal_limit_caps_rows() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    e.run("CREATE user:alice SET name = 'Alice';").unwrap();
    e.run("CREATE user:bob SET name = 'Bob';").unwrap();
    e.run("CREATE user:carol SET name = 'Carol';").unwrap();
    e.run("CREATE user:dave SET name = 'Dave';").unwrap();

    e.run("RELATE user:alice->knows->user:bob;").unwrap();
    e.run("RELATE user:alice->knows->user:carol;").unwrap();
    e.run("RELATE user:alice->knows->user:dave;").unwrap();

    let (result, _outcome) = e
        .run("SELECT ->knows->user FROM user:alice LIMIT 2;")
        .unwrap();
    match result {
        QueryResult::Rows(rows) => {
            assert_eq!(rows.len(), 2);
        }
        _ => panic!("expected Rows"),
    }
}

#[test]
fn select_traversal_empty_result() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    e.run("CREATE user:alice SET name = 'Alice';").unwrap();

    // No edges exist yet.
    let (result, _outcome) = e.run("SELECT ->knows->user FROM user:alice;").unwrap();
    match result {
        QueryResult::Rows(rows) => assert!(rows.is_empty()),
        _ => panic!("expected Rows"),
    }
}

// -------------------------------------------------------------------
// ON CONFLICT UPDATE
// -------------------------------------------------------------------

#[test]
fn on_conflict_update_creates_when_no_conflict() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    let (result, _) = e
        .run("CREATE user:alice SET name = 'Alice', age = 30 ON CONFLICT UPDATE;")
        .unwrap();
    match result {
        QueryResult::Created { id } => {
            assert_eq!(id.table, "user");
            assert_eq!(id.id, "alice");
        }
        _ => panic!("expected Created"),
    }

    // Verify the document was stored.
    let (result, _) = e.run("SELECT * FROM user:alice;").unwrap();
    match result {
        QueryResult::Rows(rows) => {
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].get("name"), Some(&Value::String("Alice".into())));
            assert_eq!(rows[0].get("age"), Some(&Value::Int(30)));
        }
        _ => panic!("expected Rows"),
    }
}

#[test]
fn on_conflict_update_merges_on_conflict() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    // First create.
    e.run("CREATE user:alice SET name = 'Alice', age = 30;")
        .unwrap();

    // Second create with ON CONFLICT UPDATE -- should merge.
    let (result, _) = e
        .run("CREATE user:alice SET name = 'Alice Updated', age = 31 ON CONFLICT UPDATE;")
        .unwrap();
    match result {
        QueryResult::Updated { id } => {
            assert_eq!(id.table, "user");
            assert_eq!(id.id, "alice");
        }
        _ => panic!("expected Updated, got {result:?}"),
    }

    // Verify the fields were merged.
    let (result, _) = e.run("SELECT * FROM user:alice;").unwrap();
    match result {
        QueryResult::Rows(rows) => {
            assert_eq!(rows.len(), 1);
            assert_eq!(
                rows[0].get("name"),
                Some(&Value::String("Alice Updated".into()))
            );
            assert_eq!(rows[0].get("age"), Some(&Value::Int(31)));
        }
        _ => panic!("expected Rows"),
    }
}

#[test]
fn on_conflict_update_set_applies_explicit_fields() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    // First create with name and age.
    e.run("CREATE user:alice SET name = 'Alice', age = 30;")
        .unwrap();

    // ON CONFLICT UPDATE SET -- only update age, ignore the name in the CREATE SET.
    let (result, _) = e
        .run("CREATE user:alice SET name = 'Ignored', age = 99 ON CONFLICT UPDATE SET age = 31;")
        .unwrap();
    assert!(matches!(result, QueryResult::Updated { .. }));

    let (result, _) = e.run("SELECT * FROM user:alice;").unwrap();
    match result {
        QueryResult::Rows(rows) => {
            assert_eq!(rows.len(), 1);
            // name should remain 'Alice' (from the original), not 'Ignored'.
            assert_eq!(rows[0].get("name"), Some(&Value::String("Alice".into())));
            // age should be 31 (from ON CONFLICT UPDATE SET).
            assert_eq!(rows[0].get("age"), Some(&Value::Int(31)));
        }
        _ => panic!("expected Rows"),
    }
}

#[test]
fn on_conflict_update_preserves_existing_fields() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    // Create with three fields.
    e.run("CREATE user:alice SET name = 'Alice', age = 30, active = true;")
        .unwrap();

    // ON CONFLICT UPDATE with only age -- name and active should be preserved.
    let (result, _) = e
        .run("CREATE user:alice SET age = 31 ON CONFLICT UPDATE;")
        .unwrap();
    assert!(matches!(result, QueryResult::Updated { .. }));

    let (result, _) = e.run("SELECT * FROM user:alice;").unwrap();
    match result {
        QueryResult::Rows(rows) => {
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].get("name"), Some(&Value::String("Alice".into())));
            assert_eq!(rows[0].get("age"), Some(&Value::Int(31)));
            assert_eq!(rows[0].get("active"), Some(&Value::Bool(true)));
        }
        _ => panic!("expected Rows"),
    }
}

#[test]
fn on_conflict_update_without_id_errors() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    // ON CONFLICT UPDATE without explicit ID should error.
    let result = e.run("CREATE user SET name = 'Alice' ON CONFLICT UPDATE;");
    assert!(result.is_err());
}

// -------------------------------------------------------------------
// Parse errors
// -------------------------------------------------------------------

#[test]
fn parse_error_returns_err() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);
    assert!(e.run("INVALID SYNTAX HERE").is_err());
}

#[test]
fn create_without_set_returns_err() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);
    assert!(e.run("CREATE user:alice;").is_err());
}

// -------------------------------------------------------------------
// Edge cases
// -------------------------------------------------------------------

#[test]
fn select_empty_table() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    let (result, _outcome) = e.run("SELECT * FROM empty_table;").unwrap();
    match result {
        QueryResult::Rows(rows) => assert!(rows.is_empty()),
        _ => panic!("expected Rows"),
    }
}

#[test]
fn create_auto_generates_id() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    // CREATE without :id -- parser currently requires :id or plain table.
    // Using CREATE table SET ... (no colon) should auto-generate.
    let (result, _outcome) = e.run("CREATE user SET name = 'Anonymous';").unwrap();
    match result {
        QueryResult::Created { id } => {
            assert_eq!(id.table, "user");
            assert!(!id.id.is_empty());
        }
        _ => panic!("expected Created"),
    }
}

#[test]
fn unicode_field_values() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    e.run("CREATE user:uni SET name = 'Aleksei';").unwrap();

    let (result, _outcome) = e.run("SELECT * FROM user:uni;").unwrap();
    match result {
        QueryResult::Rows(rows) => {
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].get("name"), Some(&Value::String("Aleksei".into())));
        }
        _ => panic!("expected Rows"),
    }
}

#[test]
fn large_document_many_fields() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    // Build a CREATE with 50 fields.
    let fields: Vec<String> = (0..50).map(|i| format!("f{i} = {i}")).collect();
    let query = format!("CREATE big:doc SET {};", fields.join(", "));
    e.run(&query).unwrap();

    let (result, _outcome) = e.run("SELECT * FROM big:doc;").unwrap();
    match result {
        QueryResult::Rows(rows) => {
            assert_eq!(rows.len(), 1);
            // Should have 50 fields + id.
            assert!(rows[0].len() >= 50);
        }
        _ => panic!("expected Rows"),
    }
}

// -------------------------------------------------------------------
// Compute cache
// -------------------------------------------------------------------

/// Two-cluster directed graph suitable for community detection.
fn build_two_cluster_graph(storage: &DllbStorage) {
    let e = exec(storage);
    // Dense cluster A
    for (a, b) in [("a1", "a2"), ("a2", "a3"), ("a3", "a1")] {
        e.run(&format!("RELATE fn:{a}->calls->fn:{b}")).unwrap();
    }
    // Dense cluster B
    for (a, b) in [("b1", "b2"), ("b2", "b3"), ("b3", "b1")] {
        e.run(&format!("RELATE fn:{a}->calls->fn:{b}")).unwrap();
    }
    // Weak bridge
    e.run("RELATE fn:a1->calls->fn:b1").unwrap();
}

#[test]
fn communities_first_call_returns_communities_result() {
    let (_dir, storage) = temp_storage();
    build_two_cluster_graph(&storage);

    let e = exec(&storage);
    let (result, _) = e.run("GRAPH COMMUNITIES calls").unwrap();
    match result {
        QueryResult::Communities { algorithm, groups } => {
            assert_eq!(algorithm, "louvain");
            // 6 nodes → at most 6 communities, at least 2.
            assert!(groups.len() >= 2 && groups.len() <= 6);
        }
        _ => panic!("expected Communities on first call"),
    }
}

#[test]
fn communities_second_call_is_cache_hit() {
    let (_dir, storage) = temp_storage();
    build_two_cluster_graph(&storage);

    let cache = Arc::new(ComputeCache::default());
    let versions = Arc::new(WriteVersions::default());

    // Seed the write version so version == 0 after building edges above
    // (edges were written without the shared version map, so we manually bump).
    // Actually: the RELATE calls in build_two_cluster_graph used a plain
    // exec() with its own private versions, so the shared `versions` still
    // sits at 0.  The shared executor will compute and cache at version=0.
    // A subsequent call with the same shared executor must return CachedResponse.

    let e1 = exec_with_shared_cache(&storage, Arc::clone(&cache), Arc::clone(&versions));
    let (first, _) = e1.run("GRAPH COMMUNITIES calls").unwrap();
    assert!(
        matches!(first, QueryResult::Communities { .. }),
        "first call should compute and return Communities"
    );

    // Second call with same shared cache must be a hit.
    let e2 = exec_with_shared_cache(&storage, Arc::clone(&cache), Arc::clone(&versions));
    let (second, _) = e2.run("GRAPH COMMUNITIES calls").unwrap();
    assert!(
        matches!(second, QueryResult::CachedResponse(_)),
        "second call should be served from cache"
    );
}

#[test]
fn relate_invalidates_communities_cache() {
    let (_dir, storage) = temp_storage();
    build_two_cluster_graph(&storage);

    let cache = Arc::new(ComputeCache::default());
    let versions = Arc::new(WriteVersions::default());

    // First GRAPH COMMUNITIES — populates cache at version=0.
    let e = exec_with_shared_cache(&storage, Arc::clone(&cache), Arc::clone(&versions));
    let (first, _) = e.run("GRAPH COMMUNITIES calls").unwrap();
    assert!(matches!(first, QueryResult::Communities { .. }));

    // Second GRAPH COMMUNITIES — should be a cache hit (version still 0).
    let e = exec_with_shared_cache(&storage, Arc::clone(&cache), Arc::clone(&versions));
    let (hit, _) = e.run("GRAPH COMMUNITIES calls").unwrap();
    assert!(
        matches!(hit, QueryResult::CachedResponse(_)),
        "should be cached before any new RELATE"
    );

    // Add a new edge — this bumps the version via the shared WriteVersions.
    let e = exec_with_shared_cache(&storage, Arc::clone(&cache), Arc::clone(&versions));
    e.run("RELATE fn:a2->calls->fn:b2").unwrap();

    // Third GRAPH COMMUNITIES — cache is stale (version=1), must recompute.
    let e = exec_with_shared_cache(&storage, Arc::clone(&cache), Arc::clone(&versions));
    let (after_write, _) = e.run("GRAPH COMMUNITIES calls").unwrap();
    assert!(
        matches!(after_write, QueryResult::Communities { .. }),
        "cache should be invalidated after RELATE"
    );
}

#[test]
fn communities_cache_result_is_consistent_with_fresh_compute() {
    let (_dir, storage) = temp_storage();
    build_two_cluster_graph(&storage);

    let cache = Arc::new(ComputeCache::default());
    let versions = Arc::new(WriteVersions::default());

    // First call — compute.
    let e = exec_with_shared_cache(&storage, Arc::clone(&cache), Arc::clone(&versions));
    let (first, _) = e.run("GRAPH COMMUNITIES calls").unwrap();
    let first_json = match first {
        QueryResult::Communities {
            ref algorithm,
            ref groups,
        } => {
            format!("{}:{}", algorithm, groups.len())
        }
        _ => panic!("expected Communities"),
    };

    // Second call — cache hit, returns pre-serialised JSON string.
    let e = exec_with_shared_cache(&storage, Arc::clone(&cache), Arc::clone(&versions));
    let (second, _) = e.run("GRAPH COMMUNITIES calls").unwrap();
    let cached_payload = match second {
        QueryResult::CachedResponse(s) => s,
        _ => panic!("expected CachedResponse"),
    };

    // The cached JSON must contain the same community count as the first result.
    let community_count_str = first_json.split(':').nth(1).unwrap();
    assert!(
        cached_payload.contains(&format!("\"community_count\":{community_count_str}")),
        "cached JSON should embed the same community count as the computed result"
    );
}

// -------------------------------------------------------------------
// BEGIN BATCH / END BATCH (execute_batch)
// -------------------------------------------------------------------

#[test]
fn batch_creates_single_transaction() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    let stmts: Vec<dllb_query::ast::Statement> = [
        "CREATE user:alice SET name = 'Alice', age = 30",
        "CREATE user:bob SET name = 'Bob', age = 25",
        "CREATE user:carol SET name = 'Carol', age = 35",
    ]
    .into_iter()
    .map(|q| dllb_query::parse(q).unwrap().statement)
    .collect();

    let result = e.execute_batch(&stmts).unwrap();
    match result {
        QueryResult::Batch {
            count: 3,
            created: 3,
            updated: 0,
        } => {}
        other => panic!("expected Batch{{count:3,created:3}}, got {other:?}"),
    }

    // Verify all three records exist.
    let (rows_result, _) = e.run("SELECT * FROM user").unwrap();
    match rows_result {
        QueryResult::Rows(rows) => assert_eq!(rows.len(), 3),
        _ => panic!("expected Rows"),
    }
}

#[test]
fn batch_upsert_updates_existing() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    // Pre-create a record.
    e.run("CREATE user:alice SET name = 'Alice', age = 30")
        .unwrap();

    // Batch with upsert: alice already exists, bob is new.
    let stmts: Vec<dllb_query::ast::Statement> = [
        "CREATE user:alice SET name = 'Alice Updated', age = 31 ON CONFLICT UPDATE",
        "CREATE user:bob SET name = 'Bob', age = 25 ON CONFLICT UPDATE",
    ]
    .into_iter()
    .map(|q| dllb_query::parse(q).unwrap().statement)
    .collect();

    let result = e.execute_batch(&stmts).unwrap();
    match result {
        QueryResult::Batch {
            count: 2,
            created: 1,
            updated: 1,
        } => {}
        other => panic!("expected Batch{{count:2,created:1,updated:1}}, got {other:?}"),
    }

    // Verify alice was updated.
    let (result, _) = e.run("SELECT * FROM user:alice").unwrap();
    match result {
        QueryResult::Rows(rows) => {
            assert_eq!(rows[0].get("age"), Some(&Value::Int(31)));
        }
        _ => panic!("expected Rows"),
    }
}

#[test]
fn batch_creates_and_relates() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    let stmts: Vec<dllb_query::ast::Statement> = [
        "CREATE fn:parse SET name = 'parse'",
        "CREATE fn:run SET name = 'run'",
        "RELATE fn:parse->calls->fn:run SET callee = 'run'",
    ]
    .into_iter()
    .map(|q| dllb_query::parse(q).unwrap().statement)
    .collect();

    let result = e.execute_batch(&stmts).unwrap();
    match result {
        QueryResult::Batch {
            count: 3,
            created: 2,
            ..
        } => {}
        other => panic!("expected Batch with 2 created, got {other:?}"),
    }

    // Verify the edge exists via traversal.
    let es = EdgeStore::new(&storage, "ns", "db", "calls");
    let tv = Traversal::new(&es);
    let outgoing = tv.outgoing_typed("parse", "calls").unwrap();
    assert_eq!(outgoing.len(), 1);
    assert_eq!(outgoing[0].dst, "run");
}

#[test]
fn batch_rejects_select() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    let stmts: Vec<dllb_query::ast::Statement> =
        ["CREATE user:alice SET name = 'Alice'", "SELECT * FROM user"]
            .into_iter()
            .map(|q| dllb_query::parse(q).unwrap().statement)
            .collect();

    let err = e.execute_batch(&stmts);
    assert!(err.is_err(), "batch should reject SELECT");
}

#[test]
fn boolean_and_float_values() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    e.run("CREATE item:x SET active = true, score = 2.72;")
        .unwrap();

    let (result, _outcome) = e.run("SELECT * FROM item:x;").unwrap();
    match result {
        QueryResult::Rows(rows) => {
            assert_eq!(rows[0].get("active"), Some(&Value::Bool(true)));
            assert_eq!(rows[0].get("score"), Some(&Value::Float(2.72)));
        }
        _ => panic!("expected Rows"),
    }
}

// -------------------------------------------------------------------
// DEFINE / REMOVE INDEX and index-accelerated filtering
// -------------------------------------------------------------------

/// Count the rows in a `QueryResult::Rows`.
fn row_count(result: QueryResult) -> usize {
    match result {
        QueryResult::Rows(rows) => rows.len(),
        other => panic!("expected Rows, got {other:?}"),
    }
}

fn count_value(result: QueryResult) -> usize {
    match result {
        QueryResult::Count { count } => count,
        other => panic!("expected Count, got {other:?}"),
    }
}

#[test]
fn define_index_backfills_then_select_eq_is_correct() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    // Rows exist *before* the index is defined, so DEFINE must backfill.
    e.run("CREATE user:a SET name = 'A', age = 30;").unwrap();
    e.run("CREATE user:b SET name = 'B', age = 30;").unwrap();
    e.run("CREATE user:c SET name = 'C', age = 40;").unwrap();

    let (def, _) = e.run("DEFINE INDEX by_age ON user FIELDS age;").unwrap();
    assert!(matches!(def, QueryResult::Ok));

    // Indexed equality returns exactly the matching rows.
    let (rows, _) = e.run("SELECT * FROM user WHERE age = 30;").unwrap();
    assert_eq!(row_count(rows), 2);

    // And COUNT over the indexed equality matches.
    let (cnt, _) = e.run("COUNT user WHERE age = 30;").unwrap();
    assert_eq!(count_value(cnt), 2);
}

#[test]
fn index_is_maintained_on_create_update_delete() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    e.run("DEFINE INDEX by_age ON user FIELDS age;").unwrap();

    // CREATE after the index exists must populate the index.
    e.run("CREATE user:a SET name = 'A', age = 30;").unwrap();
    e.run("CREATE user:b SET name = 'B', age = 30;").unwrap();
    assert_eq!(
        count_value(e.run("COUNT user WHERE age = 30;").unwrap().0),
        2
    );

    // UPDATE moves a row from age 30 to 40: both buckets must reflect it.
    e.run("UPDATE user:a SET age = 40;").unwrap();
    assert_eq!(
        count_value(e.run("COUNT user WHERE age = 30;").unwrap().0),
        1
    );
    assert_eq!(
        count_value(e.run("COUNT user WHERE age = 40;").unwrap().0),
        1
    );
    assert_eq!(
        row_count(e.run("SELECT * FROM user WHERE age = 40;").unwrap().0),
        1
    );

    // DELETE removes the row from the index.
    e.run("DELETE user:b;").unwrap();
    assert_eq!(
        count_value(e.run("COUNT user WHERE age = 30;").unwrap().0),
        0
    );
}

#[test]
fn indexed_equality_with_residual_and_filter() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    e.run("CREATE user:a SET age = 30, tier = 'gold';").unwrap();
    e.run("CREATE user:b SET age = 30, tier = 'silver';")
        .unwrap();
    e.run("CREATE user:c SET age = 40, tier = 'gold';").unwrap();
    e.run("DEFINE INDEX by_age ON user FIELDS age;").unwrap();

    // age = 30 narrows via the index; tier = 'gold' is the residual filter.
    let (rows, _) = e
        .run("SELECT * FROM user WHERE age = 30 AND tier = 'gold';")
        .unwrap();
    assert_eq!(row_count(rows), 1);
    let (cnt, _) = e
        .run("COUNT user WHERE age = 30 AND tier = 'gold';")
        .unwrap();
    assert_eq!(count_value(cnt), 1);
}

#[test]
fn unique_index_rejects_duplicate_on_create() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    e.run("DEFINE INDEX uniq_email ON user FIELDS email UNIQUE;")
        .unwrap();
    e.run("CREATE user:a SET email = 'x@y.z';").unwrap();
    // Second insert with the same email violates the unique index.
    assert!(e.run("CREATE user:b SET email = 'x@y.z';").is_err());
}

#[test]
fn define_unique_index_rejects_existing_duplicates() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    e.run("CREATE user:a SET email = 'x@y.z';").unwrap();
    e.run("CREATE user:b SET email = 'x@y.z';").unwrap();

    // Defining a unique index over already-duplicate data must fail, and must
    // not persist a partial catalog entry (a later SELECT still works).
    assert!(
        e.run("DEFINE INDEX uniq_email ON user FIELDS email UNIQUE;")
            .is_err()
    );
    let (rows, _) = e.run("SELECT * FROM user WHERE email = 'x@y.z';").unwrap();
    assert_eq!(row_count(rows), 2);
}

#[test]
fn remove_index_falls_back_to_scan() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    e.run("CREATE user:a SET age = 30;").unwrap();
    e.run("CREATE user:b SET age = 30;").unwrap();
    e.run("DEFINE INDEX by_age ON user FIELDS age;").unwrap();
    assert_eq!(
        count_value(e.run("COUNT user WHERE age = 30;").unwrap().0),
        2
    );

    let (removed, _) = e.run("REMOVE INDEX by_age ON user;").unwrap();
    assert!(matches!(removed, QueryResult::Ok));

    // After removal the same query still returns correct results via scan.
    assert_eq!(
        count_value(e.run("COUNT user WHERE age = 30;").unwrap().0),
        2
    );
    assert_eq!(
        row_count(e.run("SELECT * FROM user WHERE age = 30;").unwrap().0),
        2
    );
}
