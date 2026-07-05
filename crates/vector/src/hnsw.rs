//! HNSW (Hierarchical Navigable Small World) index for approximate
//! nearest neighbor search.
//!
//! This is an in-memory implementation following the original HNSW paper.
//! KV-backed persistence will be added in a later hardening pass.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashSet};
use std::io::Cursor;
use std::io::Read as _;

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

    // ── Persistence ───────────────────────────────────────────────────────

    const MAGIC: &[u8; 4] = b"HNSW";
    const VERSION: u8 = 1;

    /// Serialize the entire HNSW index state to a compact binary format.
    pub fn serialize(&self) -> Vec<u8> {
        let size = self.snapshot_size();
        let mut buf: Vec<u8> = Vec::with_capacity(size);

        // Magic + version
        buf.extend_from_slice(Self::MAGIC);
        buf.push(Self::VERSION);

        // Config
        buf.extend_from_slice(&(self.config.m as u32).to_le_bytes());
        buf.extend_from_slice(&(self.config.ef_construction as u32).to_le_bytes());
        buf.extend_from_slice(&(self.config.max_layers as u32).to_le_bytes());

        // Metric
        let metric_byte: u8 = match self.metric {
            DistanceMetric::Cosine => 0,
            DistanceMetric::Euclidean => 1,
            DistanceMetric::DotProduct => 2,
        };
        buf.push(metric_byte);

        // Dimension
        buf.extend_from_slice(&(self._dim as u32).to_le_bytes());

        // Entry point (-1 for None)
        let ep: i64 = match self.entry_point {
            Some(idx) => idx as i64,
            None => -1,
        };
        buf.extend_from_slice(&ep.to_le_bytes());

        // Max level
        buf.extend_from_slice(&(self.max_level as u32).to_le_bytes());

        // Node count
        buf.extend_from_slice(&(self.nodes.len() as u32).to_le_bytes());

        // Nodes
        for node in &self.nodes {
            // id
            let id_bytes = node.id.as_bytes();
            buf.extend_from_slice(&(id_bytes.len() as u32).to_le_bytes());
            buf.extend_from_slice(id_bytes);

            // deleted
            buf.push(if node.deleted { 1 } else { 0 });

            // level
            buf.extend_from_slice(&(node._level as u32).to_le_bytes());

            // vector (raw f32 LE bytes)
            for &v in &node.vector {
                buf.extend_from_slice(&v.to_le_bytes());
            }

            // neighbors
            let num_layers = node.neighbors.len() as u32;
            buf.extend_from_slice(&num_layers.to_le_bytes());
            for layer_neighbors in &node.neighbors {
                buf.extend_from_slice(&(layer_neighbors.len() as u32).to_le_bytes());
                for &neighbor_idx in layer_neighbors {
                    buf.extend_from_slice(&(neighbor_idx as u32).to_le_bytes());
                }
            }
        }

        buf
    }

    /// Reconstruct an HnswIndex from serialized bytes.
    pub fn deserialize(data: &[u8]) -> Result<Self, String> {
        let mut cursor = Cursor::new(data);

        // Helper closures for reading
        macro_rules! read_bytes {
            ($n:expr) => {{
                let mut buf = [0u8; $n];
                cursor
                    .read_exact(&mut buf)
                    .map_err(|e| format!("unexpected end of data: {e}"))?;
                buf
            }};
        }

        // Magic
        let magic = read_bytes!(4);
        if &magic != Self::MAGIC {
            return Err(format!(
                "invalid magic: expected {:?}, got {:?}",
                Self::MAGIC,
                magic
            ));
        }

        // Version
        let version = read_bytes!(1)[0];
        if version != Self::VERSION {
            return Err(format!(
                "unsupported version: expected {}, got {}",
                Self::VERSION,
                version
            ));
        }

        // Config
        let m = u32::from_le_bytes(read_bytes!(4)) as usize;
        let ef_construction = u32::from_le_bytes(read_bytes!(4)) as usize;
        let max_layers = u32::from_le_bytes(read_bytes!(4)) as usize;
        let config = HnswConfig {
            m,
            ef_construction,
            max_layers,
        };

        // Metric
        let metric_byte = read_bytes!(1)[0];
        let metric = match metric_byte {
            0 => DistanceMetric::Cosine,
            1 => DistanceMetric::Euclidean,
            2 => DistanceMetric::DotProduct,
            other => return Err(format!("unknown metric byte: {other}")),
        };

        // Dimension
        let dim = u32::from_le_bytes(read_bytes!(4)) as usize;

        // Entry point
        let ep_raw = i64::from_le_bytes(read_bytes!(8));
        let entry_point = if ep_raw < 0 {
            None
        } else {
            Some(ep_raw as usize)
        };

        // Max level
        let max_level = u32::from_le_bytes(read_bytes!(4)) as usize;

        // Node count
        let node_count = u32::from_le_bytes(read_bytes!(4)) as usize;

        // Nodes
        let mut nodes = Vec::with_capacity(node_count);
        for _ in 0..node_count {
            // id
            let id_len = u32::from_le_bytes(read_bytes!(4)) as usize;
            let mut id_buf = vec![0u8; id_len];
            cursor
                .read_exact(&mut id_buf)
                .map_err(|e| format!("unexpected end of data reading id: {e}"))?;
            let id =
                String::from_utf8(id_buf).map_err(|e| format!("invalid utf-8 in node id: {e}"))?;

            // deleted
            let deleted = read_bytes!(1)[0] != 0;

            // level
            let level = u32::from_le_bytes(read_bytes!(4)) as usize;

            // vector
            let mut vector = Vec::with_capacity(dim);
            for _ in 0..dim {
                vector.push(f32::from_le_bytes(read_bytes!(4)));
            }

            // neighbors
            let num_layers = u32::from_le_bytes(read_bytes!(4)) as usize;
            let mut neighbors = Vec::with_capacity(num_layers);
            for _ in 0..num_layers {
                let neighbor_count = u32::from_le_bytes(read_bytes!(4)) as usize;
                let mut layer_neighbors = Vec::with_capacity(neighbor_count);
                for _ in 0..neighbor_count {
                    layer_neighbors.push(u32::from_le_bytes(read_bytes!(4)) as usize);
                }
                neighbors.push(layer_neighbors);
            }

            nodes.push(HnswNode {
                id,
                vector,
                _level: level,
                neighbors,
                deleted,
            });
        }

        let ml = 1.0 / (m as f64).ln();

        Ok(Self {
            config,
            metric,
            _dim: dim,
            nodes,
            entry_point,
            max_level,
            ml,
        })
    }

    /// Estimate serialized size in bytes without actually serializing.
    pub fn snapshot_size(&self) -> usize {
        let mut size = 0usize;

        // Magic(4) + version(1)
        size += 5;
        // Config: m(4) + ef_construction(4) + max_layers(4)
        size += 12;
        // Metric(1) + dim(4) + entry_point(8) + max_level(4) + node_count(4)
        size += 21;

        for node in &self.nodes {
            // id_len(4) + id_bytes
            size += 4 + node.id.len();
            // deleted(1) + level(4)
            size += 5;
            // vector: dim * 4 bytes
            size += node.vector.len() * 4;
            // num_layers(4)
            size += 4;
            for layer_neighbors in &node.neighbors {
                // neighbor_count(4) + neighbor_indices(4 each)
                size += 4 + layer_neighbors.len() * 4;
            }
        }

        size
    }

    // ── End Persistence ──────────────────────────────────────────────────

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

#[cfg(test)]
mod persistence_tests {
    use super::*;

    fn make_index_with_vectors() -> HnswIndex {
        let config = HnswConfig {
            m: 8,
            ef_construction: 50,
            max_layers: 4,
        };
        let mut index = HnswIndex::new(3, DistanceMetric::Cosine, config);
        index.insert("a", vec![1.0, 0.0, 0.0]);
        index.insert("b", vec![0.0, 1.0, 0.0]);
        index.insert("c", vec![0.0, 0.0, 1.0]);
        index.insert("d", vec![1.0, 1.0, 0.0]);
        index.insert("e", vec![0.5, 0.5, 0.5]);
        index
    }

    #[test]
    fn round_trip_preserves_search_results() {
        let index = make_index_with_vectors();
        let query = vec![1.0, 0.1, 0.0];
        let results_before = index.search(&query, 3);

        let bytes = index.serialize();
        let restored = HnswIndex::deserialize(&bytes).expect("deserialize should succeed");

        let results_after = restored.search(&query, 3);
        assert_eq!(results_before.len(), results_after.len());
        for (before, after) in results_before.iter().zip(results_after.iter()) {
            assert_eq!(before.id, after.id);
            assert!((before.distance - after.distance).abs() < 1e-6);
        }
        assert_eq!(index.len(), restored.len());
    }

    #[test]
    fn empty_index_round_trip() {
        let config = HnswConfig::default();
        let index = HnswIndex::new(4, DistanceMetric::Euclidean, config);
        assert!(index.is_empty());

        let bytes = index.serialize();
        let restored = HnswIndex::deserialize(&bytes).expect("deserialize should succeed");

        assert!(restored.is_empty());
        assert_eq!(restored.search(&[1.0, 2.0, 3.0, 4.0], 5).len(), 0);
    }

    #[test]
    fn deleted_node_survives_round_trip() {
        let mut index = make_index_with_vectors();
        assert!(index.remove("b"));
        assert_eq!(index.len(), 4);

        let bytes = index.serialize();
        let restored = HnswIndex::deserialize(&bytes).expect("deserialize should succeed");

        assert_eq!(restored.len(), 4);
        // Search should not return "b"
        let results = restored.search(&[0.0, 1.0, 0.0], 5);
        assert!(results.iter().all(|hit| hit.id != "b"));
    }

    #[test]
    fn invalid_magic_returns_error() {
        let mut bytes = vec![0u8; 50];
        bytes[0] = b'X';
        bytes[1] = b'Y';
        bytes[2] = b'Z';
        bytes[3] = b'W';
        let result = HnswIndex::deserialize(&bytes);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("invalid magic"));
    }

    #[test]
    fn snapshot_size_close_to_actual() {
        let index = make_index_with_vectors();
        let estimated = index.snapshot_size();
        let actual = index.serialize().len();
        assert_eq!(
            estimated, actual,
            "snapshot_size should exactly match serialized length"
        );
    }
}
