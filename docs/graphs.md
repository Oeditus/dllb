# Graph Model

This document covers the `dllb-graph` crate: edge storage, bidirectional
traversal, multi-hop walks, and filtered queries.

## Design

Graph edges are stored as KV pairs in the same keyspace as documents,
using the `~` tag byte. Each edge produces **two KV entries**:

| Direction | Key format | Value |
|-----------|-----------|-------|
| Outgoing | `ns\0db\0table\0~src\0>edge_type\0dst` | MessagePack properties |
| Incoming | `ns\0db\0table\0~dst\0<edge_type\0src` | empty (pointer only) |

The `>` (0x3E) and `<` (0x3C) direction bytes separate outgoing from
incoming edges within a vertex's key region. This means:

- All edges from/to a vertex are colocated under `~vertex_id\0`
- Outgoing and incoming sort into distinct regions (`<` < `>`)
- Direction-aware prefix scans are a single contiguous range

Vertex data (node properties) lives in documents. The graph crate
references vertices by their string IDs.

## Edge

```rust
let edge = Edge::new("alice", "knows", "bob")
    .with_property("since", Value::Int(2020))
    .with_property("weight", Value::Float(0.9));
```

Edges are full documents -- they can carry arbitrary key-value properties.

## EdgeStore (CRUD)

```rust
let store = EdgeStore::new(&storage, "ns", "db", "social");

// Create (RELATE)
store.relate(&edge)?;

// Read
let edge = store.get("alice", "knows", "bob")?;

// Update properties
store.update_properties("alice", "knows", "bob", new_props)?;

// Delete
store.delete("alice", "knows", "bob")?;
```

All writes are atomic: `relate()` writes both outgoing and incoming keys
in a single `put_batch`; `delete()` removes both in a single `delete_batch`.

## Traversal

```rust
let t = Traversal::new(&store);

// Single-hop
let friends = t.outgoing("alice")?;
let knows = t.outgoing_typed("alice", "knows")?;
let followers = t.incoming("bob")?;

// Filtered
let close = t.outgoing_filtered("alice", |e| {
    e.properties.get("close") == Some(&Value::Bool(true))
})?;

// Multi-hop walk: alice->knows->?->likes->?
let paths = t.walk("alice", &[
    HopSpec { direction: Direction::Out, edge_type: Some("knows".into()) },
    HopSpec { direction: Direction::Out, edge_type: Some("likes".into()) },
])?;
// paths = [["alice", "bob", "widget"], ["alice", "carol", "gadget"]]
```

### How traversal works

1. Build a prefix for the scan direction (`>` or `<`)
2. `prefix_scan` over the KV store -- single contiguous range
3. Parse each key to extract edge_type and target vertex
4. Deserialize properties from the value (outgoing) or fetch from the
   outgoing entry (incoming)

Multi-hop `walk()` chains sequential prefix scans, expanding the vertex
frontier at each hop.

## Key Helpers

The storage crate provides direction-aware key builders:

| Function | Prefix |
|----------|--------|
| `vertex_outgoing_prefix(ns, db, table, id)` | `~id\0>` |
| `vertex_incoming_prefix(ns, db, table, id)` | `~id\0<` |
| `vertex_outgoing_typed_prefix(ns, db, table, id, type)` | `~id\0>type\0` |
| `vertex_incoming_typed_prefix(ns, db, table, id, type)` | `~id\0<type\0` |

## Testing

```bash
cargo test -p dllb-graph
```

18 tests: 1 unit (edge builder) + 17 integration (CRUD roundtrip,
properties, delete, bidirectional consistency, outgoing/incoming/typed
traversal, 2-hop walk, 3-hop walk, fan-out walk, filtered traversal,
cross-table isolation, empty traversal).
