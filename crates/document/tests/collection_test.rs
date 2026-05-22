//! Integration tests for the document Collection.

use std::collections::BTreeMap;

use dllb_core::schema::{FieldDefinition, FieldType, SchemaMode, TableDefinition};
use dllb_core::{RecordId, Value};
use dllb_document::{Collection, Document, IndexDefinition};
use dllb_storage::db::DllbStorage;

fn temp_storage() -> (tempfile::TempDir, DllbStorage) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.redb");
    let storage = DllbStorage::open(&path).unwrap();
    (dir, storage)
}

fn coll<'s>(storage: &'s DllbStorage) -> Collection<'s> {
    Collection::new(storage, "ns", "db", "user")
}

fn make_doc(id: &str, name: &str, age: i64) -> Document {
    Document::new(RecordId::new("user", id))
        .with_field("name", Value::String(name.into()))
        .with_field("age", Value::Int(age))
}

// -------------------------------------------------------------------
// Basic CRUD
// -------------------------------------------------------------------

#[test]
fn create_and_get_roundtrip() {
    let (_dir, storage) = temp_storage();
    let c = coll(&storage);

    let doc = make_doc("alice", "Alice", 30);
    c.create(doc.clone()).unwrap();

    let got = c.get("alice").unwrap().unwrap();
    assert_eq!(got.get("name"), Some(&Value::String("Alice".into())));
    assert_eq!(got.get("age"), Some(&Value::Int(30)));
}

#[test]
fn create_with_explicit_id() {
    let (_dir, storage) = temp_storage();
    let c = coll(&storage);

    let doc = Document::new(RecordId::new("user", "placeholder"))
        .with_field("name", Value::String("Bob".into()));
    let id = c.create_with_id("bob", doc).unwrap();
    assert_eq!(id.id, "bob");

    let got = c.get("bob").unwrap().unwrap();
    assert_eq!(got.get("name"), Some(&Value::String("Bob".into())));
}

#[test]
fn create_duplicate_returns_error() {
    let (_dir, storage) = temp_storage();
    let c = coll(&storage);

    c.create(make_doc("alice", "Alice", 30)).unwrap();
    let err = c.create(make_doc("alice", "Alice 2", 31)).unwrap_err();
    assert!(err.to_string().contains("already exists"));
}

#[test]
fn get_missing_returns_none() {
    let (_dir, storage) = temp_storage();
    let c = coll(&storage);
    assert!(c.get("ghost").unwrap().is_none());
}

#[test]
fn update_replaces_all_fields() {
    let (_dir, storage) = temp_storage();
    let c = coll(&storage);

    c.create(make_doc("alice", "Alice", 30)).unwrap();

    let mut new_fields = BTreeMap::new();
    new_fields.insert("name".into(), Value::String("Alicia".into()));
    new_fields.insert("email".into(), Value::String("a@b.c".into()));
    c.update("alice", new_fields).unwrap();

    let got = c.get("alice").unwrap().unwrap();
    assert_eq!(got.get("name"), Some(&Value::String("Alicia".into())));
    assert_eq!(got.get("email"), Some(&Value::String("a@b.c".into())));
    // "age" was not in the new fields, so it should be gone.
    assert_eq!(got.get("age"), None);
}

#[test]
fn merge_preserves_existing_fields() {
    let (_dir, storage) = temp_storage();
    let c = coll(&storage);

    c.create(make_doc("alice", "Alice", 30)).unwrap();

    let mut patch = BTreeMap::new();
    patch.insert("age".into(), Value::Int(31));
    patch.insert("email".into(), Value::String("a@b.c".into()));
    c.merge("alice", patch).unwrap();

    let got = c.get("alice").unwrap().unwrap();
    assert_eq!(got.get("name"), Some(&Value::String("Alice".into()))); // preserved
    assert_eq!(got.get("age"), Some(&Value::Int(31))); // updated
    assert_eq!(got.get("email"), Some(&Value::String("a@b.c".into()))); // added
}

#[test]
fn delete_returns_true_then_false() {
    let (_dir, storage) = temp_storage();
    let c = coll(&storage);

    c.create(make_doc("alice", "Alice", 30)).unwrap();
    assert!(c.delete("alice").unwrap());
    assert!(!c.delete("alice").unwrap());
    assert!(c.get("alice").unwrap().is_none());
}

#[test]
fn scan_all_and_count() {
    let (_dir, storage) = temp_storage();
    let c = coll(&storage);

    c.create(make_doc("a", "A", 1)).unwrap();
    c.create(make_doc("b", "B", 2)).unwrap();
    c.create(make_doc("c", "C", 3)).unwrap();

    let all = c.scan_all().unwrap();
    assert_eq!(all.len(), 3);
    assert_eq!(c.count().unwrap(), 3);
}

// -------------------------------------------------------------------
// Schema enforcement
// -------------------------------------------------------------------

#[test]
fn schemafull_rejects_invalid_doc() {
    let (_dir, storage) = temp_storage();
    let schema = TableDefinition {
        name: "user".into(),
        schema_mode: SchemaMode::Schemafull,
        fields: vec![FieldDefinition {
            name: "name".into(),
            field_type: FieldType::String,
            required: true,
        }],
    };
    let c = Collection::new(&storage, "ns", "db", "user").with_schema(schema);

    // Missing required field "name".
    let doc = Document::new(RecordId::new("user", "bad")).with_field("age", Value::Int(30));
    let err = c.create(doc).unwrap_err();
    assert!(err.to_string().contains("missing required field"));
}

// -------------------------------------------------------------------
// Secondary indexes
// -------------------------------------------------------------------

#[test]
fn find_by_index_returns_matching_docs() {
    let (_dir, storage) = temp_storage();
    let idx = IndexDefinition {
        name: "idx_age".into(),
        fields: vec!["age".into()],
        unique: false,
    };
    let c = coll(&storage).with_index(idx);

    c.create(make_doc("alice", "Alice", 30)).unwrap();
    c.create(make_doc("bob", "Bob", 25)).unwrap();
    c.create(make_doc("carol", "Carol", 30)).unwrap();

    let found = c.find_by_index("idx_age", &Value::Int(30)).unwrap();
    assert_eq!(found.len(), 2);
    let names: Vec<&Value> = found.iter().filter_map(|d| d.get("name")).collect();
    assert!(names.contains(&&Value::String("Alice".into())));
    assert!(names.contains(&&Value::String("Carol".into())));
}

#[test]
fn unique_index_rejects_duplicate_value() {
    let (_dir, storage) = temp_storage();
    let idx = IndexDefinition {
        name: "idx_email".into(),
        fields: vec!["email".into()],
        unique: true,
    };
    let c = coll(&storage).with_index(idx);

    let doc1 = make_doc("alice", "Alice", 30).with_field("email", Value::String("a@b.c".into()));
    c.create(doc1).unwrap();

    let doc2 = make_doc("bob", "Bob", 25).with_field("email", Value::String("a@b.c".into())); // same email
    let err = c.create(doc2).unwrap_err();
    assert!(err.to_string().contains("unique constraint"));
}

#[test]
fn delete_removes_index_entries() {
    let (_dir, storage) = temp_storage();
    let idx = IndexDefinition {
        name: "idx_age".into(),
        fields: vec!["age".into()],
        unique: false,
    };
    let c = coll(&storage).with_index(idx);

    c.create(make_doc("alice", "Alice", 30)).unwrap();
    assert_eq!(
        c.find_by_index("idx_age", &Value::Int(30)).unwrap().len(),
        1
    );

    c.delete("alice").unwrap();
    assert!(
        c.find_by_index("idx_age", &Value::Int(30))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn update_maintains_index_entries() {
    let (_dir, storage) = temp_storage();
    let idx = IndexDefinition {
        name: "idx_age".into(),
        fields: vec!["age".into()],
        unique: false,
    };
    let c = coll(&storage).with_index(idx);

    c.create(make_doc("alice", "Alice", 30)).unwrap();
    assert_eq!(
        c.find_by_index("idx_age", &Value::Int(30)).unwrap().len(),
        1
    );

    // Update age from 30 to 31.
    let mut new_fields = BTreeMap::new();
    new_fields.insert("name".into(), Value::String("Alice".into()));
    new_fields.insert("age".into(), Value::Int(31));
    c.update("alice", new_fields).unwrap();

    // Old index entry should be gone.
    assert!(
        c.find_by_index("idx_age", &Value::Int(30))
            .unwrap()
            .is_empty()
    );
    // New index entry should exist.
    assert_eq!(
        c.find_by_index("idx_age", &Value::Int(31)).unwrap().len(),
        1
    );
}

// -------------------------------------------------------------------
// Cross-table isolation
// -------------------------------------------------------------------

#[test]
fn different_tables_are_isolated() {
    let (_dir, storage) = temp_storage();
    let users = Collection::new(&storage, "ns", "db", "user");
    let products = Collection::new(&storage, "ns", "db", "product");

    users.create(make_doc("alice", "Alice", 30)).unwrap();

    let product = Document::new(RecordId::new("product", "widget"))
        .with_field("name", Value::String("Widget".into()));
    products.create(product).unwrap();

    // Each table only sees its own documents.
    assert_eq!(users.count().unwrap(), 1);
    assert_eq!(products.count().unwrap(), 1);
    assert!(users.get("widget").unwrap().is_none());
    assert!(products.get("alice").unwrap().is_none());
}
