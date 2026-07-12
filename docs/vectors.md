# Vector Embeddings

This document covers the `dllb-vector` crate: distance metrics,
brute-force exact KNN, and the HNSW approximate nearest neighbor index.

## Distance Metrics

All metrics return a non-negative distance where **lower = more similar**.

| Metric | Formula | Range | Best for |
|--------|---------|-------|----------|
| Cosine | 1 - cos(a, b) | [0, 2] | Text/code embeddings |
| Euclidean | L2(a - b) | [0, inf) | Geometric similarity |
| DotProduct | -dot(a, b) | (-inf, inf) | Pre-normalized vectors |

```rust
use dllb_vector::distance::{cosine_distance, euclidean_distance, dot_product};

let d = cosine_distance(&[1.0, 0.0], &[0.0, 1.0]); // ~1.0 (orthogonal)
```

## BruteForceIndex

Exact KNN via linear scan -- O(n) per query. Used as a correctness
baseline and for small datasets (<10K vectors).

```rust
let mut idx = BruteForceIndex::new(DistanceMetric::Cosine);
idx.insert("doc1", vec![0.1, 0.2, 0.3]);
idx.insert("doc2", vec![0.4, 0.5, 0.6]);

let hits = idx.search(&[0.1, 0.2, 0.3], 5);
// hits[0] = VectorHit { id: "doc1", distance: ~0.0 }
```

## HnswIndex

In-memory HNSW graph for approximate KNN -- sub-linear query time.

### Configuration

```rust
let config = HnswConfig {
    m: 16,               // max connections per node per layer
    ef_construction: 200, // beam width during build
    max_layers: 16,       // max layers in the hierarchy
};
let mut idx = HnswIndex::new(768, DistanceMetric::Cosine, config);
```

### Insert / Search / Remove

```rust
idx.insert("doc1", embedding.clone());

// search with default ef (= ef_construction)
let hits = idx.search(&query, 10);

// search with custom ef for speed/recall tradeoff
let hits = idx.search_ef(&query, 10, 50);

idx.remove("doc1");
```

### How it works

**Insert:**
1. Assign a random level via exponential distribution
2. Greedy descend from entry point through layers above the node's level
3. At each layer from the node's level to 0, beam-search for ef_construction
   nearest neighbors and connect (bidirectional edges, pruned to M per layer)

**Search:**
1. Greedy descend from entry point to layer 1
2. Beam search at layer 0 with width ef
3. Return top-k from the candidate set

**Remove:**
Soft delete (mark as deleted). Deleted nodes are skipped during search
but remain in the graph. Full compaction is a post-prototype feature.

### Recall

Measured against brute-force baseline on random vectors:
- 500 vectors, 32-dim, cosine, top-10, ef=50: recall >= 0.6
- 1000 vectors, 32-dim, euclidean, top-10, ef=100: recall >= 0.5

Higher ef values improve recall at the cost of query latency.

## VectorIndex Trait

Unified interface for both implementations:

```rust
pub trait VectorIndex {
    fn insert(&mut self, id: &str, vector: Vec<f32>);
    fn remove(&mut self, id: &str) -> bool;
    fn search(&self, query: &[f32], k: usize) -> Vec<VectorHit>;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool;
}
```

## Testing

```bash
cargo test -p dllb-vector
```

16 tests: 7 unit (distance metrics) + 9 integration (brute-force exact KNN,
remove, empty search, HNSW single vector, remove, recall at 500 and 1000
vectors, different metrics produce different rankings).

## Concurrency & Thread Safety

The query engine manages HNSW vector indexes via a process-wide `SearchServices` instance using a multi-layered, fine-grained locking strategy:

### Fine-Grained Locking
To prevent updates on one vector index from blocking searches on another, index locks are localized:
1. **Map Retrieval**: Read-locking `SearchServices.vectors` is only held momentarily to retrieve and clone an `Arc<RwLock<HnswIndex>>`.
2. **Search Operations**: Multiple readers can concurrently traverse the graph by acquiring a shared read lock (`index.read()`) on the index.
3. **Write Operations**: Write-locking (`index.write()`) is localized to a single HNSW index during vector insertions or removals, keeping other indexes fully available.

### Thread-Safe Lazy Rebuilding
Because HNSW graphs are held in-memory and rebuilt from the KV catalog on first use (e.g., after a process restart), they utilize a **synchronized double-checked locking** pattern to avoid race conditions:
1. `get_or_create_vector` atomically checks the map or creates a new entry with `loaded = false`.
2. The querying thread attempts to acquire the entry's private `rebuild_lock` (a standard mutex).
3. Once the lock is acquired, the thread double-checks the atomic `loaded` flag. If still `false`, it proceeds to scan the KV collection and populate the HNSW graph.
4. When finished, it sets `loaded = true`. Subsequent threads bypass the lock entirely via the atomic fast-path check.

### O(1) Index Lookup
To avoid a slow linear scan during deletion, the `HnswIndex` maintains an internal `node_lookup: HashMap<String, usize>` mapping vector IDs to their indices in the `nodes` vector. This lookup table is dynamically rebuilt from the active (non-deleted) nodes when deserializing the graph.

