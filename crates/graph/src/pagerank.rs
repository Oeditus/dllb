//! PageRank centrality over a directed, optionally weighted edge list.
//!
//! Standard power iteration with damping. Edge weights bias the rank a node
//! distributes toward its out-neighbours: each out-edge receives a share
//! proportional to its weight divided by the node's total out-weight. Nodes
//! with no positive out-weight are "dangling" and redistribute their rank
//! uniformly across all nodes so the total stays normalised.

use std::collections::HashMap;

/// Parameters controlling PageRank.
#[derive(Debug, Clone)]
pub struct PageRankOptions {
    /// Damping factor (probability of following an edge vs. teleporting).
    pub damping: f64,
    /// Maximum number of power-iteration steps.
    pub max_iterations: usize,
    /// Stop early once the L1 change between iterations drops below this.
    pub tolerance: f64,
}

impl Default for PageRankOptions {
    fn default() -> Self {
        Self {
            damping: 0.85,
            max_iterations: 100,
            tolerance: 1e-6,
        }
    }
}

/// Compute weighted PageRank over a directed edge list.
///
/// Edges are `(src, dst, weight)`; non-positive weights contribute nothing to
/// a node's out-strength. Returns `(node, score)` sorted by score descending,
/// then node id ascending for determinism. Scores sum to approximately 1.0.
pub fn pagerank(edges: &[(String, String, f64)], opts: &PageRankOptions) -> Vec<(String, f64)> {
    // Assign a dense index to each distinct node id.
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut labels: Vec<String> = Vec::new();
    for (src, dst, _) in edges {
        node_index(src, &mut index, &mut labels);
        node_index(dst, &mut index, &mut labels);
    }

    let n = labels.len();
    if n == 0 {
        return Vec::new();
    }

    // Outgoing adjacency with clamped weights and per-node out-strength.
    let mut out_links: Vec<Vec<(usize, f64)>> = vec![Vec::new(); n];
    let mut out_strength: Vec<f64> = vec![0.0; n];
    for (src, dst, weight) in edges {
        let w = if *weight > 0.0 { *weight } else { 0.0 };
        if w == 0.0 {
            continue;
        }
        let si = index[src.as_str()];
        let di = index[dst.as_str()];
        out_links[si].push((di, w));
        out_strength[si] += w;
    }

    let n_f = n as f64;
    let teleport = (1.0 - opts.damping) / n_f;
    let mut rank = vec![1.0 / n_f; n];

    for _ in 0..opts.max_iterations {
        // Rank held by dangling nodes is spread uniformly across all nodes.
        let dangling_sum: f64 = rank
            .iter()
            .zip(&out_strength)
            .filter(|&(_, &s)| s == 0.0)
            .map(|(&r, _)| r)
            .sum();
        let base = teleport + opts.damping * dangling_sum / n_f;

        let mut next = vec![base; n];
        for (i, links) in out_links.iter().enumerate() {
            if out_strength[i] == 0.0 {
                continue;
            }
            let share = opts.damping * rank[i] / out_strength[i];
            for &(di, w) in links {
                next[di] += share * w;
            }
        }

        let diff: f64 = next.iter().zip(&rank).map(|(a, b)| (a - b).abs()).sum();
        rank = next;
        if diff < opts.tolerance {
            break;
        }
    }

    let mut result: Vec<(String, f64)> = labels.into_iter().zip(rank).collect();
    result.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    result
}

/// Intern a node id to a dense index, registering it on first sight.
fn node_index(id: &str, index: &mut HashMap<String, usize>, labels: &mut Vec<String>) -> usize {
    if let Some(&i) = index.get(id) {
        return i;
    }
    let i = labels.len();
    index.insert(id.to_string(), i);
    labels.push(id.to_string());
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(a: &str, b: &str) -> (String, String, f64) {
        (a.to_string(), b.to_string(), 1.0)
    }

    #[test]
    fn empty_graph_yields_no_ranks() {
        assert!(pagerank(&[], &PageRankOptions::default()).is_empty());
    }

    #[test]
    fn ranks_sum_to_one() {
        let edges = vec![w("a", "b"), w("b", "c"), w("c", "a"), w("a", "c")];
        let ranks = pagerank(&edges, &PageRankOptions::default());
        let total: f64 = ranks.iter().map(|(_, s)| s).sum();
        assert!((total - 1.0).abs() < 1e-6, "ranks summed to {total}");
    }

    #[test]
    fn most_referenced_node_ranks_highest() {
        // b, c, d all point to a; a has the most incoming authority.
        let edges = vec![w("b", "a"), w("c", "a"), w("d", "a")];
        let ranks = pagerank(&edges, &PageRankOptions::default());
        assert_eq!(ranks[0].0, "a");
    }

    #[test]
    fn symmetric_two_cycle_is_balanced() {
        let edges = vec![w("a", "b"), w("b", "a")];
        let ranks = pagerank(&edges, &PageRankOptions::default());
        let a = ranks.iter().find(|(n, _)| n == "a").unwrap().1;
        let b = ranks.iter().find(|(n, _)| n == "b").unwrap().1;
        assert!((a - b).abs() < 1e-6);
    }

    #[test]
    fn weight_biases_distribution() {
        // a splits rank between b and c, heavily favouring b.
        let edges = vec![("a".into(), "b".into(), 9.0), ("a".into(), "c".into(), 1.0)];
        let ranks = pagerank(&edges, &PageRankOptions::default());
        let b = ranks.iter().find(|(n, _)| n == "b").unwrap().1;
        let c = ranks.iter().find(|(n, _)| n == "c").unwrap().1;
        assert!(b > c, "b ({b}) should outrank c ({c})");
    }
}
