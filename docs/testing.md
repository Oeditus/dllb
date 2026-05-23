# Testing Strategy

This document describes the test strategy, coverage, and how to run tests.

## Running Tests

```bash
# All workspace tests
cargo test --workspace

# Single crate
cargo test -p dllb-storage
cargo test -p dllb-document
cargo test -p dllb-graph
cargo test -p dllb-search
cargo test -p dllb-vector
cargo test -p dllb-query

# Clippy (all crates)
cargo clippy --workspace -- -D warnings
```

## Test Coverage Summary

| Crate | Unit | Integration | Doc | Total | Notes |
|-------|------|-------------|-----|-------|-------|
| dllb-storage | 7 | 20 | 2 | 29 | Key encoding, backend CRUD, E2E, recovery, concurrent |
| dllb-document | 16 | 14 | 1 | 31 | Builder, serde, validation, index encoding, Collection |
| dllb-graph | 1 | 17 | 0 | 18 | Edge builder, CRUD, traversal, walk, filtered |
| dllb-search | 0 | 9 | 0 | 9 | FtsIndex, FtsManager, BM25, stemming, persistence |
| dllb-vector | 7 | 9 | 0 | 16 | Distance metrics, brute-force, HNSW recall |
| dllb-query | 11 | 16 | 0 | 27 | Tokenizer, parser, executor, edge cases |
| **Total** | **42** | **85** | **3** | **130** | |

## Test Categories

### Unit Tests (in-module)
- Key encoding/decoding roundtrips
- Distance metric correctness (cosine, euclidean, dot product)
- Document builder, serialization (MessagePack + JSON)
- Schema validation (schemafull, schemaless, vector dimensions)
- Index value encoding sort order (i64, f64, string, bool)
- Tokenizer and parser for all statement types

### Integration Tests (separate test files)
- Storage: put/get/delete/scan/batch, prefix scan, concurrent readers
- Document: Collection CRUD, schema enforcement, secondary indexes, unique constraints
- Graph: edge CRUD, bidirectional traversal, multi-hop walk, filtered traversal
- Search: BM25 ranking, delete, update, stemming, multi-index isolation, persistence
- Vector: brute-force exact KNN, HNSW recall (500 and 1000 vectors), multiple metrics
- Query: end-to-end CREATE/SELECT/DELETE/RELATE through the executor

### Hardening Tests
- **Cross-model E2E**: documents + graph edges + traversal in a single test
- **Crash recovery**: data persists across close + reopen (redb durability)
- **Concurrent access**: writer thread + 4 reader threads, no panics or corruption
- **Edge cases**: empty tables, auto-generated IDs, unicode values, 50-field documents, boolean/float literals

## Quality Gates

All of the following must pass before every commit:

1. `cargo test --workspace` -- 0 failures
2. `cargo clippy --workspace -- -D warnings` -- 0 warnings
3. `cargo fmt --all --check` -- 0 formatting issues

## Future: Benchmarks

Performance benchmarks (criterion) are planned but not yet implemented:
- Storage: put/get throughput at 1K-10K operations
- Document: create/scan throughput
- Graph: edge creation + traversal latency
- Search: index 1K documents + search latency
- Vector: HNSW insert 10K vectors + KNN query latency
