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
**Status:** Implemented (Phase 2 complete)

Document model: CRUD operations, MessagePack/JSON serialization, schema
validation (schemaless + schemafull), secondary B-tree indexes with
sort-preserving encoding and unique constraints.

| Module | Contents |
|--------|----------|
| `document` | `Document` struct with builder pattern and field accessors |
| `serde` | MessagePack (internal) and JSON (client) serialization |
| `validate` | Schema validation: required fields, type checking, vector dimensions |
| `index` | `IndexDefinition`, sort-preserving `encode_index_value`, `find_by_index` |
| `collection` | `Collection` CRUD: create/get/update/merge/delete/scan_all/count/find_by_index |

See [documents.md](documents.md) for detailed documentation.

## dllb-graph

**Path:** `crates/graph/`
**Status:** Implemented (Phase 3 complete)

Native graph model with bidirectional edge storage, direction-aware prefix
scans, and multi-hop traversal.

| Module | Contents |
|--------|----------|
| `edge` | `Edge` struct with builder pattern and arbitrary properties |
| `store` | `EdgeStore` CRUD: relate/get/delete/update_properties (atomic bidirectional writes) |
| `traverse` | `Traversal` engine: outgoing/incoming/typed/walk/filtered; `Direction`, `HopSpec` types |

See [graphs.md](graphs.md) for detailed documentation.

## dllb-search

**Path:** `crates/search/`
**Status:** Implemented (Phase 4 complete)

Tantivy-backed full-text search with BM25 scoring and configurable analyzers.

| Module | Contents |
|--------|----------|
| `analyzer` | `AnalyzerConfig` (Default/Language/Simple), `Language` enum, `build_analyzer()` |
| `fts_index` | `FtsIndex` wrapping a single Tantivy index; `SearchHit` with id + BM25 score |
| `manager` | `FtsManager` managing multiple `table.field` indexes with lifecycle methods |

See [search.md](search.md) for detailed documentation.

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
