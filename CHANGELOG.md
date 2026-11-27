# Changelog

All notable changes to `dllb` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.2.0] - 2026-11-27

### Changed (breaking)
- `DELETE <table>` without a `WHERE` clause is now a parse error. The only
  filterless form is the explicit `DELETE ALL FROM <table>`. Previously a
  missing `WHERE` silently deleted every row in the table.
- `QueryResult::Compacted { reclaimed }` replaces the bare `Ok` returned by
  `COMPACT`, so callers can tell whether space was actually reclaimed.
- `GRAPH PATH` reports `length` and `path` as `NONE` when no path exists,
  instead of the `-1` / `[]` sentinels.
- `ORDER BY` on a graph-traversal `SELECT` is rejected rather than silently
  ignored.
- `SELECT` column names for string literals now escape embedded quotes
  (`'it''s'`), so distinct literals never collapse onto one column.
- The server acknowledges `BEGIN BATCH` with `{"status":"batch_started"}`,
  reports a connection that closes mid-batch instead of silently discarding
  the buffered statements, and honours the requested `OUTCOME` format on
  execution errors (a CSV client no longer receives a JSON error body).
- `EdgeStore`'s three whole-table scanners are now one parameterised
  implementation (`ScanMode`).
- Removed the empty `dllb-transaction` crate and ~20 unused dependencies
  across the workspace.
- Record ids produced by `dllb-code-intel` ingestion percent-encode the
  `file_path` (`lib/my_app.ex` becomes `lib%2Fmy_app.ex`), so generated
  batches always parse. Previously any path containing a space or other
  non-identifier byte produced an unparseable `CREATE`.

### Fixed
- Analytics caches (`GRAPH COMMUNITIES`, `PAGERANK`, `CENTRALITY`,
  `COMPONENTS`) are now invalidated by *any* edge mutation. `EdgeStore` bumps
  the shared write-version registry itself, so embedders that bypass the query
  executor can no longer leave stale results cached.
- `COMPACT` now actually compacts. The previous implementation called
  `Arc::get_mut` on a fresh clone, which can never succeed, so it always
  reported success while reclaiming nothing.
- `DELETE ... WHERE` applies all deletions in one storage transaction instead
  of one commit per row.
- `UPDATE` reads each document once instead of three times.
- `ComputeCache::get` no longer clones the (potentially large) payload while
  holding the cache mutex.
- `DLLB_CACHE_MAX_BYTES` values that would overflow are rejected instead of
  wrapping to a tiny budget.
- Vector writes with the wrong dimensionality fail with
  `DimensionMismatch` instead of silently dropping the document from the index.
- `dllb-cli` gains `--version`/`-V` and `--help`/`-h`, and rejects unknown
  arguments instead of silently ignoring them.
- `install.sh` uses `https://github.com/...` (was `https://github://...`, which
  could never resolve), fails loudly when no release is found instead of
  falling back to a stale hard-coded tag, and verifies `SHA256SUMS`.
- Release tarballs include `LICENSE`, which previously did not exist in the
  repository despite being declared in every `Cargo.toml`.

### Added
- `LICENSE` (MIT).
- `Collection::merge_returning` and `Collection::delete_many`.
- `dllb_graph::WriteVersions` and `global_write_versions()`, re-exported from
  `dllb_query`.
- `dllb-cli` dot-commands: `.indexes` renders the index catalog as a table,
  `.schema` explains the schemaless/schemafull split, and unknown dot-commands
  are reported instead of being sent to the query engine.
- `dllb-cli` drives the `BEGIN BATCH` / `END BATCH` protocol with a
  `dllb[batch]>` prompt; Ctrl-C discards the batch instead of exiting.
- `QueryExecutor::describe_indexes()` and
  `dllb_document::index::load_all_index_definitions()` for catalog
  introspection.
- `dllb_core::Value` implements `Display`, so CLI output and error messages
  render values readably instead of as serde-tagged enums.
- JSON results are projected into plain JSON: a row is now
  `{"name":"Alice","age":30}` instead of `{"name":{"String":"Alice"}}`.
  `Value`'s serde encoding is unchanged -- it is the MessagePack storage
  format and must keep round-tripping through redb.
- `dllb_query::Maintainer`, which owns full-text/vector index lifecycle.
- Unit tests for the executor's pure helpers (`cmp_values`, `order_cmp`,
  `group_key`, `normalize_scores`, `candidate_pool`, `plan_index_lookup`,
  `sort_documents`) and for all three output formats.

## [1.1.0] - 2026-11-18

### Changed
- Updated all direct Rust dependencies to their latest stable releases and refreshed compatible transitive dependencies.
- Upgraded the actor runtime to `joerl` 0.8.0, full-text search to Tantivy 0.27.0, and vector random-number generation to `rand` 0.10.4.
- Upgraded the CLI to `rustyline` 19.0.0 and `marcli` 0.2.0, the query cache to `lru` 0.19.0, and the allocator to `tikv-jemallocator` 0.7.0.
- Updated Criterion to 0.8.2 and migrated all benchmarks to `std::hint::black_box`.
- Refreshed serialization, storage, async runtime, tracing, UUID, and test dependencies.
- Updated installation and container deployment examples for version 1.1.0.

## [1.0.2] - 2026-10-28

### Fixed
- Repeating an identical B-tree, full-text, or vector index definition no longer backfills the entire collection. Existing full-text indexes reopen from disk, missing full-text files are recovered on definition, and vector indexes restore lazily after restart without replacing already-loaded graphs.
- Conflicting index definitions now require explicit `REMOVE INDEX` followed by redefinition, preventing stale entries when fields, dimensions, uniqueness, or index kinds change.

## [1.0.1] - 2026-08-30

### Fixed
- `dllb-server` now derives the Unix domain socket file name from the bound TCP port (`dllb_<port>.sock` by default, or the `DLLB_SOCK` value suffixed with the port), preventing socket path collisions when multiple server instances are spawned simultaneously on different ports.
- Database-open failures (e.g. `Database already open. Cannot acquire lock.`) now print a detailed diagnostic block before panicking: the resolved lock path, database file existence/size/modification time, the associated search-index directory, and troubleshooting steps (`lsof`/`fuser`, using a unique `DLLB_PATH` per instance).

## [1.0.0] - 2026-08-26

### Added
- **Unified Multi-Model Engine**: General Availability release of `dllb` integrating Documents, Graphs, Full-Text Search (Tantivy), and HNSW Vector Embeddings in a single transactional key-value engine backed by `redb`.
- **Erlang/OTP Supervision Trees**: Stateful database actor supervision built on `joerl` for crash recovery and fault tolerance.
- **Code Intelligence First-Class Support**: AST parsing, structural similarity, AST clone detection SQL builtins (`ast::complexity`, `ast::hash`, `ast::similarity`), and code-aware tokenization.
- **Official Installation & Deployment Assets**:
  - One-line install script (`install.sh`).
  - Production `Dockerfile` with multi-stage builds and unprivileged user sandbox.
  - `docker-compose.yml` for instant local deployment.
  - Systemd service unit (`deploy/systemd/dllb.service`).
  - Homebrew formula specification (`deploy/homebrew/dllb.rb`).
  - GitHub Container Registry multi-arch container image publishing (`ghcr.io/oeditus/dllb`).

### Fixed
- Stabilized MVCC transaction boundary cleanups under high concurrency.
- Optimized HNSW distance calculations with SIMD vectorization.

### Security
- Added non-root container execution environment (`UID 10001`).
- Added systemd security sandboxing parameters.
