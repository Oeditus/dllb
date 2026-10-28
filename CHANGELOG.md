# Changelog

All notable changes to `dllb` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
