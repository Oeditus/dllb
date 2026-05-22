//! Brute-force exact K-nearest-neighbor index.
//!
//! Computes distance to every stored vector -- O(n) per query.
//! Used as a correctness baseline and for small datasets.

use crate::distance::{DistanceMetric, distance};
use crate::{VectorHit, VectorIndex};

/// Exact KNN index using linear scan.
pub struct BruteForceIndex {
    vectors: Vec<(String, Vec<f32>)>,
    metric: DistanceMetric,
}

impl BruteForceIndex {
    pub fn new(metric: DistanceMetric) -> Self {
        Self {
            vectors: Vec::new(),
            metric,
        }
    }
}

impl VectorIndex for BruteForceIndex {
    fn insert(&mut self, id: &str, vector: Vec<f32>) {
        // Remove existing entry with same id (upsert semantics).
        self.vectors.retain(|(existing_id, _)| existing_id != id);
        self.vectors.push((id.to_string(), vector));
    }

    fn remove(&mut self, id: &str) -> bool {
        let before = self.vectors.len();
        self.vectors.retain(|(existing_id, _)| existing_id != id);
        self.vectors.len() < before
    }

    fn search(&self, query: &[f32], k: usize) -> Vec<VectorHit> {
        let mut scored: Vec<VectorHit> = self
            .vectors
            .iter()
            .map(|(id, vec)| VectorHit {
                id: id.clone(),
                distance: distance(query, vec, self.metric),
            })
            .collect();

        scored.sort_by(|a, b| a.distance.partial_cmp(&b.distance).unwrap());
        scored.truncate(k);
        scored
    }

    fn len(&self) -> usize {
        self.vectors.len()
    }
}
