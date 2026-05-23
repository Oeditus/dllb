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
**Status:** Implemented (Phase 5 complete)

Vector similarity search with distance metrics, brute-force exact KNN,
and in-memory HNSW approximate nearest neighbor index.

| Module | Contents |
|--------|----------|
| `distance` | `DistanceMetric` (Cosine/Euclidean/DotProduct), distance functions |
| `brute_force` | `BruteForceIndex` -- exact KNN via linear scan |
| `hnsw` | `HnswIndex`, `HnswConfig` -- in-memory HNSW graph with configurable M/ef/layers |
| (lib) | `VectorHit` (id + distance), `VectorIndex` trait |

See [vectors.md](vectors.md) for detailed documentation.

## dllb-code-intel

**Path:** `crates/code-intel/`
**Status:** Implemented (Phase 5b complete)

Rust-native companion to the Elixir metastatic library. MetaAST types
faithful to METAST_SPEC.md, code-aware tokenizer, predefined schemas,
and structural extraction utilities.

| Module | Contents |
|--------|----------|
| `meta_ast` | `NodeType` (38 variants), `MetaNode`, `MetaValue`, `NodeChildren`, `Layer` |
| `tokenizer` | `code_tokenize()` -- camelCase/snake_case splitting, noise keyword stripping |
| `schemas` | `ast_node_schema()` (11-field TableDefinition), 6 edge type constants |
| `extract` | `walk()`, `extract_functions()`, `extract_imports()`, `extract_variables()`, `extract_calls()`, `node_count()`, `depth()` |

See [code-intel.md](code-intel.md) for detailed documentation.

## dllb-query

**Path:** `crates/query/`
**Status:** Implemented (Phase 6 -- minimal viable)

SQL-like query engine with tokenizer, recursive-descent parser, and direct
executor. Supports CREATE, SELECT (with WHERE), DELETE, RELATE.

| Module | Contents |
|--------|----------|
| `ast` | `Statement`, `SelectFields`, `FromTarget`, `WhereClause`, `Literal`, `RecordRef` |
| `tokenizer` | `Token` enum, `tokenize()` -- keywords, idents, literals, symbols |
| `parser` | `parse()` -- hand-written recursive descent |
| `executor` | `QueryExecutor`, `QueryResult` -- dispatches to Collection/EdgeStore APIs |

See [query.md](query.md) for detailed documentation.

## dllb-server

**Path:** `crates/server/`
**Type:** Binary
**Status:** Implemented (Phase 7 complete)

Tokio-based TCP server with line-based text protocol. Each connection is
a tokio task. Queries are parsed and executed via `QueryExecutor::run()`,
responses are JSON lines. Configurable via environment variables.

See [server.md](server.md) for detailed documentation.

## dllb-cli

**Path:** `crates/cli/`
**Type:** Binary
**Status:** Implemented (Phase 7 complete)

Interactive REPL with rustyline line editing and command history.
Opens an embedded database directly (no network). Supports `.help`,
`.quit`/`.exit` commands and `--path`/`--ns`/`--db` arguments.

See [server.md](server.md) for detailed documentation.
