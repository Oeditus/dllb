//! HNSW (Hierarchical Navigable Small World) index for approximate
//! nearest neighbor search.
//!
//! This is an in-memory implementation following the original HNSW paper.
//! KV-backed persistence will be added in a later hardening pass.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashSet};

use rand::Rng;

use crate::distance::{DistanceMetric, distance};
use crate::{VectorHit, VectorIndex};

/// HNSW configuration parameters.
#[derive(Debug, Clone)]
pub struct HnswConfig {
    /// Max connections per node per layer.
    pub m: usize,
    /// Beam width during construction.
    pub ef_construction: usize,
    /// Maximum number of layers.
    pub max_layers: usize,
}

impl Default for HnswConfig {
    fn default() -> Self {
        Self {
            m: 16,
            ef_construction: 200,
            max_layers: 16,
        }
    }
}

/// Internal node in the HNSW graph.
struct HnswNode {
    id: String,
    vector: Vec<f32>,
    _level: usize,
    /// neighbors[layer] = list of node indices in that layer.
    neighbors: Vec<Vec<usize>>,
    deleted: bool,
}

/// In-memory HNSW approximate nearest neighbor index.
pub struct HnswIndex {
    config: HnswConfig,
    metric: DistanceMetric,
    _dim: usize,
    nodes: Vec<HnswNode>,
    entry_point: Option<usize>,
    max_level: usize,
    ml: f64, // 1.0 / ln(M) for level generation
}

impl HnswIndex {
    /// Create a new empty HNSW index.
    pub fn new(dim: usize, metric: DistanceMetric, config: HnswConfig) -> Self {
        let ml = 1.0 / (config.m as f64).ln();
        Self {
            config,
            metric,
            _dim: dim,
            nodes: Vec::new(),
            entry_point: None,
            max_level: 0,
            ml,
        }
    }

    /// Search with configurable ef (beam width).
    pub fn search_ef(&self, query: &[f32], k: usize, ef: usize) -> Vec<VectorHit> {
        let ep = match self.entry_point {
            Some(ep) => ep,
            None => return vec![],
        };

        // Greedy descend from top layer to layer 1.
        let mut current = ep;
        for layer in (1..=self.max_level).rev() {
            current = self.greedy_closest(query, current, layer);
        }

        // Search at layer 0 with beam width ef.
        let candidates = self.search_layer(query, current, ef.max(k), 0);

        // Return top-k.
        candidates
            .into_iter()
            .filter(|(_, idx)| !self.nodes[*idx].deleted)
            .take(k)
            .map(|(dist, idx)| VectorHit {
                id: self.nodes[idx].id.clone(),
                distance: dist,
            })
            .collect()
    }

    fn random_level(&self) -> usize {
        let mut rng = rand::rng();
        let r: f64 = rng.random();
        let level = (-r.ln() * self.ml).floor() as usize;
        level.min(self.config.max_layers - 1)
    }

    fn dist(&self, query: &[f32], node_idx: usize) -> f32 {
        distance(query, &self.nodes[node_idx].vector, self.metric)
    }

    /// Greedy search for the single closest node at a given layer.
    fn greedy_closest(&self, query: &[f32], start: usize, layer: usize) -> usize {
        let mut current = start;
        let mut current_dist = self.dist(query, current);

        loop {
            let mut changed = false;
            for &neighbor in &self.nodes[current].neighbors[layer] {
                if self.nodes[neighbor].deleted {
                    continue;
                }
                let d = self.dist(query, neighbor);
                if d < current_dist {
                    current = neighbor;
                    current_dist = d;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        current
    }

    /// Beam search at a given layer, returning (distance, node_index) sorted by distance.
    fn search_layer(
        &self,
        query: &[f32],
        start: usize,
        ef: usize,
        layer: usize,
    ) -> Vec<(f32, usize)> {
        let start_dist = self.dist(query, start);
        // Min-heap of candidates to explore.
        let mut candidates: BinaryHeap<Reverse<OrdF32Idx>> = BinaryHeap::new();
        candidates.push(Reverse(OrdF32Idx(start_dist, start)));
        // Max-heap of results (worst result on top for easy eviction).
        let mut results: BinaryHeap<OrdF32Idx> = BinaryHeap::new();
        results.push(OrdF32Idx(start_dist, start));
        let mut visited: HashSet<usize> = HashSet::new();
        visited.insert(start);

        while let Some(Reverse(OrdF32Idx(c_dist, c_idx))) = candidates.pop() {
            // If the closest candidate is farther than the worst result, stop.
            if let Some(&OrdF32Idx(worst_dist, _)) = results.peek()
                && c_dist > worst_dist
                && results.len() >= ef
            {
                break;
            }

            for &neighbor in &self.nodes[c_idx].neighbors[layer] {
                if !visited.insert(neighbor) {
                    continue;
                }
                let d = self.dist(query, neighbor);
                let should_add = results.len() < ef || d < results.peek().unwrap().0;
                if should_add {
                    candidates.push(Reverse(OrdF32Idx(d, neighbor)));
                    results.push(OrdF32Idx(d, neighbor));
                    if results.len() > ef {
                        results.pop(); // evict worst
                    }
                }
            }
        }

        let mut result_vec: Vec<(f32, usize)> =
            results.into_iter().map(|OrdF32Idx(d, i)| (d, i)).collect();
        result_vec.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        result_vec
    }

    /// Connect a new node to its nearest neighbors at a given layer.
    fn connect(&mut self, new_idx: usize, neighbors: &[(f32, usize)], layer: usize) {
        let m = self.config.m;
        let nearest: Vec<usize> = neighbors.iter().take(m).map(|&(_, idx)| idx).collect();

        self.nodes[new_idx].neighbors[layer] = nearest.clone();

        // Add reverse connections (bidirectional).
        for &neighbor_idx in &nearest {
            self.nodes[neighbor_idx].neighbors[layer].push(new_idx);
            // Prune if over capacity.
            if self.nodes[neighbor_idx].neighbors[layer].len() > m * 2 {
                let node_vec = self.nodes[neighbor_idx].vector.clone();
                let mut scored: Vec<(f32, usize)> = self.nodes[neighbor_idx].neighbors[layer]
                    .iter()
                    .map(|&n| (distance(&node_vec, &self.nodes[n].vector, self.metric), n))
                    .collect();
                scored.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
                scored.truncate(m);
                self.nodes[neighbor_idx].neighbors[layer] =
                    scored.into_iter().map(|(_, idx)| idx).collect();
            }
        }
    }
}

impl VectorIndex for HnswIndex {
    fn insert(&mut self, id: &str, vector: Vec<f32>) {
        let level = self.random_level();
        let new_idx = self.nodes.len();
        self.nodes.push(HnswNode {
            id: id.to_string(),
            vector,
            _level: level,
            neighbors: vec![vec![]; level + 1],
            deleted: false,
        });

        if self.entry_point.is_none() {
            self.entry_point = Some(new_idx);
            self.max_level = level;
            return;
        }

        let ep = self.entry_point.unwrap();
        let query = &self.nodes[new_idx].vector.clone();

        // Greedy descend from top to level+1.
        let mut current = ep;
        for layer in (level + 1..=self.max_level).rev() {
            current = self.greedy_closest(query, current, layer);
        }

        // At each layer from min(level, max_level) down to 0, find neighbors and connect.
        let top = level.min(self.max_level);
        for layer in (0..=top).rev() {
            let neighbors = self.search_layer(query, current, self.config.ef_construction, layer);
            self.connect(new_idx, &neighbors, layer);
            if !neighbors.is_empty() {
                current = neighbors[0].1;
            }
        }

        if level > self.max_level {
            self.max_level = level;
            self.entry_point = Some(new_idx);
        }
    }

    fn remove(&mut self, id: &str) -> bool {
        for node in &mut self.nodes {
            if node.id == id && !node.deleted {
                node.deleted = true;
                return true;
            }
        }
        false
    }

    fn search(&self, query: &[f32], k: usize) -> Vec<VectorHit> {
        self.search_ef(query, k, self.config.ef_construction)
    }

    fn len(&self) -> usize {
        self.nodes.iter().filter(|n| !n.deleted).count()
    }
}

/// Helper for ordered f32 + index in BinaryHeap.
#[derive(Clone, Copy)]
struct OrdF32Idx(f32, usize);

impl PartialEq for OrdF32Idx {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}
impl Eq for OrdF32Idx {}

impl PartialOrd for OrdF32Idx {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for OrdF32Idx {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0
            .partial_cmp(&other.0)
            .unwrap_or(std::cmp::Ordering::Equal)
    }
}
