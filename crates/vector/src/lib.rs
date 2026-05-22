//! # dllb-vector
//!
//! Vector embedding support for dllb.
//!
//! Provides:
//! - Distance metrics: cosine, Euclidean (L2), dot product
//! - [`BruteForceIndex`]: exact KNN via linear scan (correctness baseline)
//! - [`HnswIndex`]: approximate KNN via HNSW graph (high recall, sub-linear)
//! - [`VectorIndex`] trait: unified interface for both implementations
//! - [`VectorHit`]: search result with record ID + distance

pub mod brute_force;
pub mod distance;
pub mod hnsw;

pub use brute_force::BruteForceIndex;
pub use distance::DistanceMetric;
pub use hnsw::{HnswConfig, HnswIndex};

/// A search result from a vector similarity query.
#[derive(Debug, Clone)]
pub struct VectorHit {
    /// The record ID of the matching vector.
    pub id: String,
    /// Distance to the query vector (lower = more similar).
    pub distance: f32,
}

/// Unified interface for vector indexes.
pub trait VectorIndex {
    /// Insert a vector with the given ID.
    fn insert(&mut self, id: &str, vector: Vec<f32>);
    /// Remove a vector by ID. Returns true if it existed.
    fn remove(&mut self, id: &str) -> bool;
    /// Find the k nearest vectors to the query.
    fn search(&self, query: &[f32], k: usize) -> Vec<VectorHit>;
    /// Number of vectors in the index.
    fn len(&self) -> usize;
    /// Whether the index is empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
