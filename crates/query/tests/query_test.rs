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
        QueryResult::Communities { ref algorithm, ref groups } => {
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
