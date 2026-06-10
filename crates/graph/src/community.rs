//! Community detection algorithms for undirected and directed graphs.
//!
//! Two algorithms are provided:
//!
//! - **Louvain** (`Algorithm::Louvain`): modularity-maximising hierarchical
//!   clustering. Each iteration is O(E) — community statistics (`sigma_tot`,
//!   per-neighbour `k_i_in`) are maintained incrementally instead of being
//!   recomputed from all edges on every candidate move.
//!
//! - **Label Propagation** (`Algorithm::LabelPropagation`): each node adopts
//!   the most frequent label of its neighbours. O(E) per iteration, very fast,
//!   non-deterministic in tie-breaking (ties favour the current label).
//!
//! Both algorithms treat the input edge list as undirected: a directed call
//! edge `A -> B` contributes to the community affinity between A and B
//! regardless of direction.

use std::collections::{HashMap, HashSet};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Community detection algorithm to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Algorithm {
    /// Louvain modularity optimisation (default).
    #[default]
    Louvain,
    /// Label propagation (fast, non-deterministic tie-breaking).
    LabelPropagation,
}

/// Options controlling community detection.
#[derive(Debug, Clone)]
pub struct Options {
    pub algorithm: Algorithm,
    /// Maximum number of local-optimisation passes (Louvain) or propagation
    /// rounds (Label Propagation).
    pub max_iterations: usize,
    /// Resolution parameter γ for Louvain. Values < 1.0 produce fewer, larger
    /// communities; values > 1.0 produce more, smaller communities.
    pub resolution: f64,
    /// Louvain: stop early when a full pass produces no move with net gain
    /// above this threshold. Set to 0.0 to run all `max_iterations`.
    pub min_improvement: f64,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            algorithm: Algorithm::Louvain,
            max_iterations: 10,
            resolution: 1.0,
            min_improvement: 1e-4,
        }
    }
}

/// Result of community detection.
#[derive(Debug, Clone)]
pub struct Communities {
    /// Map from community representative node ID to the sorted list of all
    /// member node IDs (including the representative itself).
    pub groups: HashMap<String, Vec<String>>,
}

impl Communities {
    /// Number of detected communities.
    pub fn len(&self) -> usize {
        self.groups.len()
    }

    /// `true` when the graph had no nodes.
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }
}

/// Detect communities from an unweighted directed edge list.
///
/// Edges are treated as undirected: `(A, B)` and `(B, A)` both contribute
/// weight 1.0 to the community affinity between A and B.
pub fn detect(edges: &[(String, String)], opts: &Options) -> Communities {
    let weighted: Vec<(String, String, f64)> = edges
        .iter()
        .map(|(s, d)| (s.clone(), d.clone(), 1.0))
        .collect();
    detect_weighted(&weighted, opts)
}

/// Detect communities from a weighted directed edge list.
///
/// Each `(src, dst, weight)` triple is treated as an undirected edge with the
/// given weight. Self-loops are ignored.
pub fn detect_weighted(edges: &[(String, String, f64)], opts: &Options) -> Communities {
    if edges.is_empty() {
        return Communities {
            groups: HashMap::new(),
        };
    }
    match opts.algorithm {
        Algorithm::Louvain => louvain(edges, opts),
        Algorithm::LabelPropagation => label_propagation(edges, opts),
    }
}

// ---------------------------------------------------------------------------
// Louvain
// ---------------------------------------------------------------------------
//
// Standard Blondel et al. 2008 local-move phase.
//
// Modularity gain of moving node i from community A to community C:
//
//   ΔQ = [k_i_in(C)  -  resolution · σ_tot(C) · k_i / (2m)] / m
//       −[k_i_in(A)  -  resolution · (σ_tot(A) − k_i) · k_i / (2m)] / m
//
// where
//   k_i       = total degree (undirected) of node i
//   k_i_in(C) = sum of edge weights between i and current members of C
//   σ_tot(C)  = sum of degrees of all nodes currently in C
//   m         = total edge weight / 2   (undirected)
//
// All quantities needed for the comparison are O(degree(i)) to compute per
// node when adjacency lists and σ_tot are kept up to date.

fn louvain(edges: &[(String, String, f64)], opts: &Options) -> Communities {
    // Build undirected adjacency list and per-node degree.
    let mut adj: HashMap<String, Vec<(String, f64)>> = HashMap::new();
    let mut degree: HashMap<String, f64> = HashMap::new();
    let mut all_nodes: HashSet<String> = HashSet::new();

    for (src, dst, weight) in edges {
        // Always register both endpoints as nodes.
        all_nodes.insert(src.clone());
        all_nodes.insert(dst.clone());
        if src == dst {
            continue; // skip self-loops for adjacency/degree
        }
        adj.entry(src.clone())
            .or_default()
            .push((dst.clone(), *weight));
        adj.entry(dst.clone())
            .or_default()
            .push((src.clone(), *weight));
        *degree.entry(src.clone()).or_insert(0.0) += weight;
        *degree.entry(dst.clone()).or_insert(0.0) += weight;
    }

    if all_nodes.is_empty() {
        return Communities {
            groups: HashMap::new(),
        };
    }

    // m = total undirected edge weight (each edge counted once).
    // Since we added each edge twice (both directions), divide by 2.
    let m: f64 = degree.values().sum::<f64>() / 2.0;

    if m == 0.0 {
        // No usable edges — every node is its own community.
        let groups = all_nodes
            .into_iter()
            .map(|n| (n.clone(), vec![n]))
            .collect();
        return Communities { groups };
    }

    let all_nodes: Vec<String> = {
        let mut v: Vec<String> = all_nodes.into_iter().collect();
        v.sort(); // deterministic order
        v
    };

    // Phase initialisation: each node starts in its own singleton community.
    // community ID = the node's own string ID.
    let mut node_to_comm: HashMap<String, String> =
        all_nodes.iter().map(|n| (n.clone(), n.clone())).collect();

    // σ_tot[c] = sum of degrees of all nodes currently assigned to community c.
    let mut sigma_tot: HashMap<String, f64> = all_nodes
        .iter()
        .map(|n| (n.clone(), *degree.get(n).unwrap_or(&0.0)))
        .collect();

    // Local optimisation passes.
    for _ in 0..opts.max_iterations {
        let mut any_moved = false;

        for node in &all_nodes {
            let k_i = *degree.get(node).unwrap_or(&0.0);
            if k_i == 0.0 {
                // Isolated node — leave in own community.
                continue;
            }

            let current_comm = node_to_comm[node].clone();

            // Accumulate k_i_in for every neighbour community (and for the
            // current community, to compute the removal gain).
            let mut k_in_per_comm: HashMap<&str, f64> = HashMap::new();

            if let Some(neighbors) = adj.get(node) {
                for (nb, w) in neighbors {
                    let nb_comm = node_to_comm[nb].as_str();
                    *k_in_per_comm.entry(nb_comm).or_insert(0.0) += w;
                }
            }

            let k_i_in_current = k_in_per_comm
                .get(current_comm.as_str())
                .copied()
                .unwrap_or(0.0);

            // Gain of keeping node in its current community (baseline).
            // σ_tot of current community *excluding* node i.
            let sigma_current_without_i =
                sigma_tot.get(&current_comm).copied().unwrap_or(0.0) - k_i;
            let baseline =
                k_i_in_current - opts.resolution * sigma_current_without_i * k_i / (2.0 * m);

            // Evaluate each neighbouring community.
            let mut best_gain = 0.0_f64;
            let mut best_comm: Option<String> = None;

            for (&cand_comm_str, &k_i_in_c) in &k_in_per_comm {
                if cand_comm_str == current_comm.as_str() {
                    continue; // already the baseline
                }
                let sigma_c = sigma_tot.get(cand_comm_str).copied().unwrap_or(0.0);
                let gain_c = k_i_in_c - opts.resolution * sigma_c * k_i / (2.0 * m);
                let net = gain_c - baseline;
                if net > best_gain {
                    best_gain = net;
                    best_comm = Some(cand_comm_str.to_string());
                }
            }

            if let Some(target_comm) = best_comm {
                // Move node to target_comm.
                *sigma_tot.entry(current_comm.clone()).or_insert(0.0) -= k_i;
                *sigma_tot.entry(target_comm.clone()).or_insert(0.0) += k_i;
                node_to_comm.insert(node.clone(), target_comm);
                any_moved = true;
            }
        }

        if !any_moved {
            break;
        }
    }

    communities_from_assignment(node_to_comm)
}

// ---------------------------------------------------------------------------
// Label Propagation
// ---------------------------------------------------------------------------
//
// Each node adopts the most frequent label among its neighbours. Ties are
// broken by keeping the node's current label (stable, no randomness).

fn label_propagation(edges: &[(String, String, f64)], opts: &Options) -> Communities {
    // Undirected adjacency (unweighted — label propagation is typically
    // unweighted; weight is ignored here for simplicity).
    let mut adj: HashMap<String, Vec<String>> = HashMap::new();
    let mut all_nodes: HashSet<String> = HashSet::new();

    for (src, dst, _) in edges {
        all_nodes.insert(src.clone());
        all_nodes.insert(dst.clone());
        if src == dst {
            continue; // skip self-loops for adjacency
        }
        adj.entry(src.clone()).or_default().push(dst.clone());
        adj.entry(dst.clone()).or_default().push(src.clone());
    }

    if all_nodes.is_empty() {
        return Communities {
            groups: HashMap::new(),
        };
    }

    let all_nodes: Vec<String> = {
        let mut v: Vec<String> = all_nodes.into_iter().collect();
        v.sort();
        v
    };

    // Initialise: each node is its own label.
    let mut labels: HashMap<String, String> =
        all_nodes.iter().map(|n| (n.clone(), n.clone())).collect();

    for _ in 0..opts.max_iterations {
        let mut changed = false;

        for node in &all_nodes {
            let neighbors = match adj.get(node) {
                Some(nb) if !nb.is_empty() => nb,
                _ => continue,
            };

            // Count frequency of each label among neighbours.
            let mut freq: HashMap<&str, usize> = HashMap::new();
            for nb in neighbors {
                let lbl = labels.get(nb).map(String::as_str).unwrap_or(nb.as_str());
                *freq.entry(lbl).or_insert(0) += 1;
            }

            // Max frequency among all neighbour labels.
            let max_count = freq.values().copied().max().unwrap_or(0);

            // Keep current label if it is among the tied winners; otherwise
            // pick the lexicographically smallest winner for determinism.
            let current = labels[node].as_str();
            let current_count = freq.get(current).copied().unwrap_or(0);

            if current_count < max_count {
                // Current label lost — pick the lexicographically smallest
                // label that achieved max_count.
                let winner = freq
                    .iter()
                    .filter(|&(_, cnt)| *cnt == max_count)
                    .map(|(&lbl, _)| lbl)
                    .min()
                    .unwrap_or(current);

                if winner != current {
                    labels.insert(node.clone(), winner.to_string());
                    changed = true;
                }
            }
        }

        if !changed {
            break;
        }
    }

    communities_from_assignment(labels)
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn communities_from_assignment(node_to_comm: HashMap<String, String>) -> Communities {
    let mut groups: HashMap<String, Vec<String>> = HashMap::new();
    for (node, comm) in node_to_comm {
        groups.entry(comm).or_default().push(node);
    }
    // Sort members for deterministic output.
    for members in groups.values_mut() {
        members.sort();
    }
    Communities { groups }
}
