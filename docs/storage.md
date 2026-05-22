# Storage Engine

This document covers the `dllb-storage` crate in detail: the KV abstraction,
redb backend, key encoding scheme, and the StorageWriter actor.

## KvStore Trait

The `KvStore` trait (`crates/storage/src/kv.rs`) defines the storage interface:

```rust
pub trait KvStore: Send + Sync {
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>>;
    fn put(&self, key: &[u8], value: &[u8]) -> Result<()>;
    fn delete(&self, key: &[u8]) -> Result<()>;
    fn scan(&self, start: &[u8], end: &[u8]) -> Result<Vec<(Vec<u8>, Vec<u8>)>>;
    fn put_batch(&self, ops: &[(&[u8], &[u8])]) -> Result<()>;
    fn delete_batch(&self, keys: &[&[u8]]) -> Result<()>;
    fn contains(&self, key: &[u8]) -> Result<bool>;       // default: get + is_some
    fn prefix_scan(&self, prefix: &[u8]) -> Result<...>;  // default: scan(prefix, prefix_end)
}
```

All operations are synchronous. redb operations are blocking but sub-millisecond,
so there is no need for async at this layer. The actor system handles async dispatch.

## redb Backend

`RedbBackend` (`crates/storage/src/backend.rs`) implements `KvStore` over redb.

### Single Table Design

All data lives in one redb table:

```rust
const DATA: TableDefinition<&[u8], &[u8]> = TableDefinition::new("dllb_data");
```

Model separation is handled entirely by the key encoding scheme. This avoids
the overhead of managing many redb tables and keeps range scans efficient --
a prefix scan across all documents in a table is a single contiguous range
in the B-tree.

### Transaction Mapping

| KvStore method | redb transaction |
|----------------|------------------|
| `get`, `scan`, `prefix_scan`, `contains` | `begin_read()` (non-blocking) |
| `put`, `delete` | `begin_write()` + `commit()` |
| `put_batch`, `delete_batch` | Single `begin_write()` for all ops + `commit()` |

### Cloneability

`RedbBackend` is `Clone` (holds `Arc<Database>`). Multiple readers can hold
clones simultaneously while a single writer operates through the actor.

## Key Encoding

See `crates/storage/src/key.rs`.

### Format

```
[namespace][0x00][database][0x00][table][0x00][tag][remainder...]
```

The `0x00` byte separates segments. String segments **must not** contain `0x00`.

### KeyBuilder

Fluent builder API:

```rust
let key = KeyBuilder::new()
    .namespace("default")
    .database("mydb")
    .table("user")
    .tag(tag::DOCUMENT)
    .raw(b"alice")
    .build();
```

### Convenience Constructors

| Function | Key format |
|----------|-----------|
| `document_key(ns, db, table, id)` | `ns\0db\0table\0*id` |
| `graph_edge_key(ns, db, table, src, edge, dst)` | `ns\0db\0table\0~src\0edge\0dst` |
| `index_key(ns, db, table, idx, val, id)` | `ns\0db\0table\0+idx\0val\0id` |
| `metadata_key(ns, db, table)` | `ns\0db\0table\0!` |

### Prefix Helpers

- `table_prefix(ns, db, table, tag)` -- prefix for all entries of a tag type in a table
- `prefix_end(prefix)` -- increment last byte for exclusive range end (with 0xFF carry)

### KeyParts (Parser)

```rust
let parts = parse_key(&key)?;
// parts.namespace, parts.database, parts.table, parts.tag, parts.remainder
```

## StorageWriter Actor

`StorageWriter` (`crates/storage/src/actor.rs`) is a joerl GenServer that
serializes write access to redb.

### Message Types

**Call (synchronous, returns reply):**

| Variant | Reply |
|---------|-------|
| `Get { key }` | `Value(Option<Vec<u8>>)` |
| `Scan { start, end }` | `Entries(Vec<(Vec<u8>, Vec<u8>)>)` |
| `PrefixScan { prefix }` | `Entries(Vec<(Vec<u8>, Vec<u8>)>)` |
| `Contains { key }` | `Bool(bool)` |

**Cast (fire-and-forget):**

| Variant | Effect |
|---------|--------|
| `Put { key, value }` | Insert/overwrite |
| `Delete { key }` | Remove key |
| `PutBatch { ops }` | Atomic multi-insert |
| `DeleteBatch { keys }` | Atomic multi-delete |

### Read Bypass

Reads do NOT need to go through the actor. The `DllbStorage` high-level API
reads directly from redb via `begin_read()`, which is non-blocking and supports
concurrent readers. Only writes are serialized through the actor.

## DllbStorage

`DllbStorage` (`crates/storage/src/db.rs`) is the main entry point:

```rust
let storage = DllbStorage::open("data.redb")?;

// Write (currently synchronous; will route through actor later)
storage.put(&key, &value)?;

// Read (always direct, no actor overhead)
let val = storage.get(&key)?;
let entries = storage.prefix_scan(&prefix)?;
```

## Error Handling

All redb errors are mapped to `dllb_core::Error::Storage(message)` via a
generic `map_err` helper. The error enum also covers serialization, schema
violations, transaction conflicts, and index errors.

## Testing

Tests live in `crates/storage/tests/backend_test.rs`:

- Put/get roundtrip, missing key, delete, overwrite
- Scan range, prefix scan with table isolation
- Batch put/delete atomicity
- Concurrent readers (4 threads x 100 reads)
- Graph edge prefix scan with edge-type filtering
- DllbStorage high-level API roundtrip

Run with:

```bash
cargo test -p dllb-storage
```
