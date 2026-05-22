# Document Model

This document covers the `dllb-document` crate: the `Document` type,
serialization, schema validation, secondary indexes, and the `Collection`
CRUD API.

## Document

A `Document` wraps a `BTreeMap<String, Value>` with a `RecordId`:

```rust
let doc = Document::new(RecordId::new("user", "alice"))
    .with_field("name", Value::String("Alice".into()))
    .with_field("age", Value::Int(30));
```

The `RecordId` is **not** stored in the serialized value -- it lives in the
KV key (`ns\0db\0table\0*record_id`). This avoids redundancy and keeps
values compact.

## Serialization

| Format | Purpose | Module |
|--------|---------|--------|
| MessagePack | Internal KV storage (compact binary) | `serde::serialize` / `serde::deserialize` |
| JSON | Client I/O (human-readable) | `serde::to_json` / `serde::from_json` |

## Schema Validation

Collections can operate in two modes:

- **Schemaless** (default): any fields accepted, no validation
- **Schemafull**: fields validated against a `TableDefinition`

Schemafull validation checks:
- All required fields are present
- No unknown fields are included
- Each field's `Value` variant matches its declared `FieldType`
- `FieldType::Vector(dim)` verifies exact dimensionality

## Secondary Indexes

### Definition

```rust
let idx = IndexDefinition {
    name: "idx_age".into(),
    fields: vec!["age".into()],
    unique: false,
};
let collection = Collection::new(&storage, "ns", "db", "user")
    .with_index(idx);
```

### Storage

Index entries are KV pairs with empty values:
- Key: `ns\0db\0table\0+index_name\0<encoded_value><0xFF><record_id>`
- Value: `[]` (empty)

### Sort-Preserving Encoding

Field values are encoded to bytes that preserve sort order:

| Type | Encoding | Size |
|------|----------|------|
| `String` | Raw UTF-8 bytes | Variable |
| `Int(i64)` | Big-endian with sign bit flipped | 8 bytes |
| `Float(f64)` | IEEE 754 with sign/exponent bits flipped | 8 bytes |
| `Bool` | `0x00` (false) / `0x01` (true) | 1 byte |

A `0xFF` marker byte separates the encoded value from the record ID in the
key. This allows the encoded value to contain any byte (including `0x00`
from big-endian integers) without ambiguity.

### Unique Indexes

When `unique: true`, the collection checks for existing entries with the
same indexed value before inserting. If a different record already has that
value, `Error::Schema("unique constraint violated")` is returned.

## Collection API

`Collection` is the primary CRUD interface, scoped to a namespace/database/table:

| Method | Description |
|--------|-------------|
| `create(doc)` | Insert with the document's existing ID |
| `create_with_id(id, doc)` | Insert with an explicit ID |
| `get(id)` | Fetch by record ID |
| `update(id, fields)` | Replace all fields |
| `merge(id, fields)` | Partial update (preserves existing fields) |
| `delete(id)` | Remove document and its index entries |
| `scan_all()` | List all documents in the collection |
| `count()` | Number of documents |
| `find_by_index(name, value)` | Query via secondary index |

### Atomicity

All write operations (create, update, delete) maintain index consistency
via atomic `put_batch` / `delete_batch` calls to the storage layer.
Document writes and their associated index entry updates happen in a
single batch.

## Testing

```bash
cargo test -p dllb-document
```

31 tests: 16 unit (document builder, serialization roundtrips, schema
validation, index encoding sort order) + 14 integration (CRUD, schema
enforcement, index queries, unique constraints, cross-table isolation) +
1 doc-test.
