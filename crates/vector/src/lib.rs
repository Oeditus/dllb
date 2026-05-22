//! # dllb-vector
//!
//! Vector embedding support for dllb.
//!
//! Provides:
//! - Native `VECTOR(dim)` field type stored as contiguous `f32` arrays
//! - HNSW (Hierarchical Navigable Small World) index for approximate
//!   nearest neighbor search
//! - Distance metrics: cosine, Euclidean (L2), dot product
//! - Optional `bf16` storage for 2x memory savings
//! - Optional scalar/product quantization (4-32x compression)
//!
//! The HNSW index is managed by the `HnswActor` GenServer, which
//! persists the graph to the KV store and supports journal-based
//! crash recovery.
