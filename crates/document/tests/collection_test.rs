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

#[test]
fn scan_ids_returns_ids_only_in_key_order() {
    let (_dir, storage) = temp_storage();
    let c = coll(&storage);

    c.create(make_doc("carol", "C", 3)).unwrap();
    c.create(make_doc("alice", "A", 1)).unwrap();
    c.create(make_doc("bob", "B", 2)).unwrap();

    // IDs come back in sorted key order, without touching document bodies.
    let ids = c.scan_ids().unwrap();
    assert_eq!(ids, vec!["alice", "bob", "carol"]);
}

#[test]
fn get_many_preserves_order_and_skips_missing() {
    let (_dir, storage) = temp_storage();
    let c = coll(&storage);

    c.create(make_doc("alice", "Alice", 30)).unwrap();
    c.create(make_doc("carol", "Carol", 40)).unwrap();

    // "bob" does not exist and must be skipped while order is preserved.
    let ids = vec!["alice".to_string(), "bob".to_string(), "carol".to_string()];
    let docs = c.get_many(&ids).unwrap();
    let names: Vec<&Value> = docs.iter().filter_map(|d| d.get("name")).collect();
    assert_eq!(
        names,
        vec![
            &Value::String("Alice".into()),
            &Value::String("Carol".into())
        ]
    );
}

#[test]
fn get_many_empty_input_is_empty() {
    let (_dir, storage) = temp_storage();
    let c = coll(&storage);
    assert!(c.get_many(&[]).unwrap().is_empty());
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
    let idx = IndexDefinition::btree("idx_age", vec!["age".into()], false);
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
    let idx = IndexDefinition::btree("idx_email", vec!["email".into()], true);
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
    let idx = IndexDefinition::btree("idx_age", vec!["age".into()], false);
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
    let idx = IndexDefinition::btree("idx_age", vec!["age".into()], false);
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
// Index catalog persistence
// -------------------------------------------------------------------

#[test]
fn catalog_save_load_remove_roundtrip() {
    use dllb_document::index::{
        load_index_definitions, remove_index_definition, save_index_definition,
    };

    let (_dir, storage) = temp_storage();

    let by_age = IndexDefinition::btree("by_age", vec!["age".into()], false);
    let by_email = IndexDefinition::btree("by_email", vec!["email".into()], true);
    save_index_definition(&storage, "ns", "db", "user", &by_age).unwrap();
    save_index_definition(&storage, "ns", "db", "user", &by_email).unwrap();

    let mut defs = load_index_definitions(&storage, "ns", "db", "user").unwrap();
    defs.sort_by(|a, b| a.name.cmp(&b.name));
    assert_eq!(defs, vec![by_age.clone(), by_email.clone()]);

    // Removing one leaves the other; removing a missing one reports false.
    assert!(remove_index_definition(&storage, "ns", "db", "user", "by_email").unwrap());
    assert!(!remove_index_definition(&storage, "ns", "db", "user", "by_email").unwrap());
    let defs = load_index_definitions(&storage, "ns", "db", "user").unwrap();
    assert_eq!(defs, vec![by_age]);
}

#[test]
fn collection_load_attaches_catalog_indexes() {
    use dllb_document::index::save_index_definition;

    let (_dir, storage) = temp_storage();
    save_index_definition(
        &storage,
        "ns",
        "db",
        "user",
        &IndexDefinition::btree("by_age", vec!["age".into()], false),
    )
    .unwrap();

    // A catalog-loaded collection sees the index and maintains it on write.
    let c = Collection::load(&storage, "ns", "db", "user").unwrap();
    assert_eq!(c.indexes().len(), 1);
    c.create(make_doc("alice", "Alice", 30)).unwrap();
    c.create(make_doc("bob", "Bob", 30)).unwrap();

    let ids = c.find_ids_by_index("by_age", &Value::Int(30)).unwrap();
    assert_eq!(ids.len(), 2);
    let docs = c.find_by_index("by_age", &Value::Int(30)).unwrap();
    assert_eq!(docs.len(), 2);
}

#[test]
fn find_ids_by_range_bounds_are_correct() {
    let (_dir, storage) = temp_storage();
    let idx = IndexDefinition::btree("by_age", vec!["age".into()], false);
    let c = coll(&storage).with_index(idx);
    for (id, age) in [("a", 10), ("b", 20), ("c", 30), ("d", 40)] {
        c.create(make_doc(id, id, age)).unwrap();
    }

    // 20 <= age <= 30  -> b, c
    let ids = c
        .find_ids_by_range(
            "by_age",
            Some(&(Value::Int(20), true)),
            Some(&(Value::Int(30), true)),
        )
        .unwrap();
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(sorted, vec!["b", "c"]);

    // age > 20  -> c, d
    let gt = c
        .find_ids_by_range("by_age", Some(&(Value::Int(20), false)), None)
        .unwrap();
    assert_eq!(gt.len(), 2);

    // age < 40  -> a, b, c
    let lt = c
        .find_ids_by_range("by_age", None, Some(&(Value::Int(40), false)))
        .unwrap();
    assert_eq!(lt.len(), 3);

    // 20 < age < 40 (both exclusive) -> c
    let between = c
        .find_ids_by_range(
            "by_age",
            Some(&(Value::Int(20), false)),
            Some(&(Value::Int(40), false)),
        )
        .unwrap();
    assert_eq!(between, vec!["c"]);
}

#[test]
fn composite_index_scan_full_prefix_and_range() {
    let (_dir, storage) = temp_storage();
    let idx = IndexDefinition::btree("by_tenant_age", vec!["tenant".into(), "age".into()], false);
    let c = coll(&storage).with_index(idx);
    let mk = |id: &str, tenant: &str, age: i64| {
        Document::new(RecordId::new("user", id))
            .with_field("tenant", Value::String(tenant.into()))
            .with_field("age", Value::Int(age))
    };
    c.create(mk("a", "acme", 30)).unwrap();
    c.create(mk("b", "acme", 40)).unwrap();
    c.create(mk("c", "acme", 50)).unwrap();
    c.create(mk("d", "globex", 30)).unwrap();

    // Full tuple: tenant=acme AND age=40 -> {b}
    let full = c
        .find_ids_for_scan(
            "by_tenant_age",
            &[Value::String("acme".into()), Value::Int(40)],
            None,
            None,
            2,
        )
        .unwrap();
    assert_eq!(full, vec!["b"]);

    // Leading prefix: tenant=acme -> {a,b,c}
    let mut prefix = c
        .find_ids_for_scan(
            "by_tenant_age",
            &[Value::String("acme".into())],
            None,
            None,
            2,
        )
        .unwrap();
    prefix.sort();
    assert_eq!(prefix, vec!["a", "b", "c"]);

    // Prefix + range: tenant=acme AND age > 30 -> {b,c}
    let mut pr = c
        .find_ids_for_scan(
            "by_tenant_age",
            &[Value::String("acme".into())],
            Some(&(Value::Int(30), false)),
            None,
            2,
        )
        .unwrap();
    pr.sort();
    assert_eq!(pr, vec!["b", "c"]);

    // Prefix + bounded range: tenant=acme AND 30 <= age <= 40 -> {a,b}
    let mut between = c
        .find_ids_for_scan(
            "by_tenant_age",
            &[Value::String("acme".into())],
            Some(&(Value::Int(30), true)),
            Some(&(Value::Int(40), true)),
            2,
        )
        .unwrap();
    between.sort();
    assert_eq!(between, vec!["a", "b"]);
}

#[test]
fn composite_unique_index_enforces_the_tuple() {
    let (_dir, storage) = temp_storage();
    let idx = IndexDefinition::btree(
        "uq_tenant_email",
        vec!["tenant".into(), "email".into()],
        true,
    );
    let c = coll(&storage).with_index(idx);
    let mk = |id: &str, tenant: &str, email: &str| {
        Document::new(RecordId::new("user", id))
            .with_field("tenant", Value::String(tenant.into()))
            .with_field("email", Value::String(email.into()))
    };
    c.create(mk("a", "acme", "x@y.z")).unwrap();
    // Same email but a different tenant -> the tuple differs, so it is allowed.
    c.create(mk("b", "globex", "x@y.z")).unwrap();
    // Identical tuple -> rejected.
    assert!(c.create(mk("c", "acme", "x@y.z")).is_err());
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
