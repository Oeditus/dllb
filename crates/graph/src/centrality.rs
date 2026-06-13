//! Degree centrality over a directed edge list.
//!
//! Returns the raw degree of each node (the count of incident edges in the
//! chosen direction), which is the directly useful signal for code-intel style
//! questions such as "which function has the most callers" (in-degree). Each
//! edge occurrence is counted, and both endpoints of every edge are registered
//! as nodes (so a pure sink still appears with out-degree 0).

use std::collections::{HashMap, HashSet};

/// Which degree to measure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CentralityKind {
    /// Total incident edges (in + out).
    Degree,
    /// Incoming edges only.
    InDegree,
    /// Outgoing edges only.
    OutDegree,
}

/// Compute degree centrality over a directed `(src, dst)` edge list.
///
/// Returns `(node, score)` sorted by score descending, then node id ascending
/// for determinism. `score` is the raw degree count as an `f64`.
pub fn degree_centrality(edges: &[(String, String)], kind: CentralityKind) -> Vec<(String, f64)> {
    let mut indeg: HashMap<&str, f64> = HashMap::new();
    let mut outdeg: HashMap<&str, f64> = HashMap::new();
    let mut nodes: HashSet<&str> = HashSet::new();

    for (src, dst) in edges {
        nodes.insert(src.as_str());
        nodes.insert(dst.as_str());
        *outdeg.entry(src.as_str()).or_insert(0.0) += 1.0;
        *indeg.entry(dst.as_str()).or_insert(0.0) += 1.0;
    }

    let mut result: Vec<(String, f64)> = nodes
        .into_iter()
        .map(|n| {
            let score = match kind {
                CentralityKind::InDegree => *indeg.get(n).unwrap_or(&0.0),
                CentralityKind::OutDegree => *outdeg.get(n).unwrap_or(&0.0),
                CentralityKind::Degree => {
                    indeg.get(n).unwrap_or(&0.0) + outdeg.get(n).unwrap_or(&0.0)
                }
            };
            (n.to_string(), score)
        })
        .collect();

    result.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(a: &str, b: &str) -> (String, String) {
        (a.to_string(), b.to_string())
    }

    #[test]
    fn empty_graph_has_no_scores() {
        assert!(degree_centrality(&[], CentralityKind::Degree).is_empty());
    }

    #[test]
    fn in_degree_ranks_most_referenced_first() {
        // a and b both call c; c has in-degree 2.
        let edges = vec![e("a", "c"), e("b", "c")];
        let ranked = degree_centrality(&edges, CentralityKind::InDegree);
        assert_eq!(ranked[0].0, "c");
        assert_eq!(ranked[0].1, 2.0);
    }

    #[test]
    fn out_degree_ranks_most_calling_first() {
        let edges = vec![e("a", "b"), e("a", "c")];
        let ranked = degree_centrality(&edges, CentralityKind::OutDegree);
        assert_eq!(ranked[0].0, "a");
        assert_eq!(ranked[0].1, 2.0);
    }

    #[test]
    fn total_degree_sums_both_directions() {
        // hub: in from a, out to c -> total degree 2.
        let edges = vec![e("a", "hub"), e("hub", "c")];
        let ranked = degree_centrality(&edges, CentralityKind::Degree);
        let hub = ranked.iter().find(|(n, _)| n == "hub").unwrap().1;
        assert_eq!(hub, 2.0);
    }
}
