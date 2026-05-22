# Crate Reference

Quick reference for each crate in the dllb workspace.

## dllb-core

**Path:** `crates/core/`

Foundation types shared by all other crates.

| Module | Contents |
|--------|----------|
| `error` | `Error` enum (Storage, Serialization, NotFound, Schema, Conflict, Query, Index, DimensionMismatch, Other) and `Result<T>` alias |
| `record_id` | `RecordId` -- composite `table:id` identifier with UUID generation and `FromStr` parsing |
| `value` | `Value` enum -- dynamically-typed values: None, Bool, Int, Float, String, Bytes, Array, Object, RecordId, Vector |
| `schema` | `FieldType`, `SchemaMode` (Schemaless/Schemafull), `FieldDefinition`, `TableDefinition` |

## dllb-storage

**Path:** `crates/storage/`
**Status:** Implemented (Phase 1 complete)

The storage engine: KV abstraction, redb backend, key encoding, and actor.

| Module | Contents |
|--------|----------|
| `kv` | `KvStore` trait -- 8 methods (get, put, delete, scan, put_batch, delete_batch, contains, prefix_scan) |
| `key` | `KeyBuilder` (fluent), `KeyParts`/`parse_key` (decoder), convenience constructors, `prefix_end`, `validate_segment`, tag constants |
| `backend` | `RedbBackend` -- `KvStore` impl over redb with `Arc<Database>` for clone-friendly reads |
| `actor` | `StorageWriter` GenServer with `StorageCall`/`StorageCast`/`StorageReply` message types |
| `db` | `DllbStorage` -- high-level API combining backend + actor |

See [storage.md](storage.md) for detailed documentation.

## dllb-transaction

**Path:** `crates/transaction/`
**Status:** Stub (Phase 1 deferred -- redb provides built-in MVCC)

Will hold MVCC transaction manager, timestamp allocation, conflict detection,
and garbage collection of old versions.

## dllb-document

**Path:** `crates/document/`
**Status:** Stub (Phase 2)

Document model: CRUD operations over the KV store, MessagePack serialization,
secondary B-tree indexes, schema validation.

## dllb-graph

**Path:** `crates/graph/`
**Status:** Stub (Phase 3)

Native graph model: edge storage as bidirectional KV pairs, BFS/DFS traversal,
multi-hop path queries, pattern matching.

## dllb-search

**Path:** `crates/search/`
**Status:** Stub (Phase 4)

Tantivy integration: full-text index management, BM25 scoring, configurable
analyzers, transactional sync with the KV store via `FtsActor`.

## dllb-vector

**Path:** `crates/vector/`
**Status:** Stub (Phase 5)

HNSW approximate nearest neighbor index, VECTOR data type, distance metrics
(cosine, L2, dot product), bf16 storage, optional quantization.

## dllb-code-intel

**Path:** `crates/code-intel/`
**Status:** Stub (Phase 5b)

AST/MetaAST code intelligence layer: predefined schemas for source code nodes,
code-aware tokenizer (camelCase/snake_case splitting), cross-repository
structural pattern recognition.

## dllb-query

**Path:** `crates/query/`
**Status:** Stub (Phase 6)

Query engine: SQL-like declarative language parser, logical/physical planner,
optimizer (index selection, predicate pushdown), streaming batched executor
with cross-model support (document + graph + full-text + vector).

## dllb-server

**Path:** `crates/server/`
**Type:** Binary

TCP/WebSocket server. Each client connection is an actor supervised by
`client_sup`. Protocol: text-based (Redis RESP-like) for the prototype.

## dllb-cli

**Path:** `crates/cli/`
**Type:** Binary

Interactive REPL for issuing queries. Will use `rustyline` for history and
tab completion.
