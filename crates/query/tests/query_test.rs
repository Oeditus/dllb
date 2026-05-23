//! Integration tests for the query engine.

use dllb_core::Value;
use dllb_graph::{EdgeStore, Traversal};
use dllb_query::{QueryExecutor, QueryResult};
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

    let result = e.run("SELECT * FROM user;").unwrap();
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

    let result = e.run("CREATE user:alice SET name = 'Alice';").unwrap();
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

    let result = e.run("SELECT name FROM user;").unwrap();
    match result {
        QueryResult::Rows(rows) => {
            assert_eq!(rows.len(), 1);
            assert!(rows[0].contains_key("name"));
            assert!(!rows[0].contains_key("age"));
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

    let result = e.run("SELECT * FROM user:alice;").unwrap();
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

    let result = e.run("SELECT * FROM user:ghost;").unwrap();
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

    let result = e.run("DELETE user:alice;").unwrap();
    match result {
        QueryResult::Deleted { existed } => assert!(existed),
        _ => panic!("expected Deleted"),
    }

    // Verify gone.
    let result = e.run("SELECT * FROM user:alice;").unwrap();
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

    let result = e.run("SELECT * FROM user WHERE age = 30;").unwrap();
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

    let result = e
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

    let result = e.run("SELECT * FROM empty_table;").unwrap();
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
    let result = e.run("CREATE user SET name = 'Anonymous';").unwrap();
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

    let result = e.run("SELECT * FROM user:uni;").unwrap();
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

    let result = e.run("SELECT * FROM big:doc;").unwrap();
    match result {
        QueryResult::Rows(rows) => {
            assert_eq!(rows.len(), 1);
            // Should have 50 fields + id.
            assert!(rows[0].len() >= 50);
        }
        _ => panic!("expected Rows"),
    }
}

#[test]
fn boolean_and_float_values() {
    let (_dir, storage) = temp_storage();
    let e = exec(&storage);

    e.run("CREATE item:x SET active = true, score = 3.14;")
        .unwrap();

    let result = e.run("SELECT * FROM item:x;").unwrap();
    match result {
        QueryResult::Rows(rows) => {
            assert_eq!(rows[0].get("active"), Some(&Value::Bool(true)));
            assert_eq!(rows[0].get("score"), Some(&Value::Float(3.14)));
        }
        _ => panic!("expected Rows"),
    }
}
