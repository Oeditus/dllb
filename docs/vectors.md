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
