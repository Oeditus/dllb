//! Connected components for undirected graphs.
//!
//! Groups vertices into connected components using a union-find (disjoint-set)
//! structure with path-halving and union-by-rank. The input edge list is
//! treated as **undirected** and edge weights are ignored. Both endpoints of
//! every edge are registered as vertices, so a self-loop `A -> A` yields a
//! singleton component for `A`.
//!
//! Like [`crate::community`], components are computed over the edge list only:
//! vertices that never appear as an edge endpoint are not represented.

use std::collections::HashMap;

/// Result of connected-components detection.
#[derive(Debug, Clone)]
pub struct Components {
    /// Map from component representative node ID to the sorted list of all
    /// member node IDs (including the representative itself).
    pub groups: HashMap<String, Vec<String>>,
}

impl Components {
    /// Number of connected components.
    pub fn len(&self) -> usize {
        self.groups.len()
    }

    /// `true` when the graph had no nodes.
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }
}

/// Compute connected components from a weighted directed edge list.
///
/// Edges are treated as undirected and weights are ignored. Both endpoints of
/// every edge are registered, so isolated endpoints and self-loops each form
/// their own singleton component.
pub fn connected_components(edges: &[(String, String, f64)]) -> Components {
    // Assign a dense integer index to each distinct node ID.
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut labels: Vec<String> = Vec::new();

    for (src, dst, _weight) in edges {
        intern(src, &mut index, &mut labels);
        intern(dst, &mut index, &mut labels);
    }

    let n = labels.len();
    if n == 0 {
        return Components {
            groups: HashMap::new(),
        };
    }

    // Union-find over the dense indices.
    let mut parent: Vec<usize> = (0..n).collect();
    let mut rank: Vec<u8> = vec![0; n];

    for (src, dst, _weight) in edges {
        let a = index[src.as_str()];
        let b = index[dst.as_str()];
        union(&mut parent, &mut rank, a, b);
    }

    // Group node labels by their representative root, naming each component
    // after the label of its root index.
    let mut groups: HashMap<String, Vec<String>> = HashMap::new();
    for i in 0..n {
        let root = find(&mut parent, i);
        groups
            .entry(labels[root].clone())
            .or_default()
            .push(labels[i].clone());
    }

    // Deterministic member ordering.
    for members in groups.values_mut() {
        members.sort();
    }

    Components { groups }
}

// ---------------------------------------------------------------------------
// Union-find helpers
// ---------------------------------------------------------------------------

fn intern(id: &str, index: &mut HashMap<String, usize>, labels: &mut Vec<String>) {
    if !index.contains_key(id) {
        let i = labels.len();
        index.insert(id.to_string(), i);
        labels.push(id.to_string());
    }
}

/// Find the representative root of `x`, applying path-halving.
fn find(parent: &mut [usize], mut x: usize) -> usize {
    while parent[x] != x {
        parent[x] = parent[parent[x]];
        x = parent[x];
    }
    x
}

/// Union the sets containing `a` and `b` using union-by-rank.
fn union(parent: &mut [usize], rank: &mut [u8], a: usize, b: usize) {
    let ra = find(parent, a);
    let rb = find(parent, b);
    if ra == rb {
        return;
    }
    match rank[ra].cmp(&rank[rb]) {
        std::cmp::Ordering::Less => parent[ra] = rb,
        std::cmp::Ordering::Greater => parent[rb] = ra,
        std::cmp::Ordering::Equal => {
            parent[rb] = ra;
            rank[ra] += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edge(a: &str, b: &str) -> (String, String, f64) {
        (a.to_string(), b.to_string(), 1.0)
    }

    #[test]
    fn empty_graph_has_no_components() {
        let comps = connected_components(&[]);
        assert!(comps.is_empty());
        assert_eq!(comps.len(), 0);
    }

    #[test]
    fn single_edge_is_one_component() {
        let comps = connected_components(&[edge("a", "b")]);
        assert_eq!(comps.len(), 1);
        let members = comps.groups.values().next().unwrap();
        assert_eq!(members, &vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn two_disjoint_edges_are_two_components() {
        let comps = connected_components(&[edge("a", "b"), edge("c", "d")]);
        assert_eq!(comps.len(), 2);
    }

    #[test]
    fn chain_collapses_to_one_component() {
        // a-b, b-c, c-d -> single component of 4 nodes.
        let comps = connected_components(&[edge("a", "b"), edge("b", "c"), edge("c", "d")]);
        assert_eq!(comps.len(), 1);
        let members = comps.groups.values().next().unwrap();
        assert_eq!(members.len(), 4);
    }

    #[test]
    fn direction_is_ignored() {
        // a->b and b->a are the same undirected edge.
        let comps = connected_components(&[edge("a", "b"), edge("b", "a")]);
        assert_eq!(comps.len(), 1);
    }

    #[test]
    fn self_loop_is_a_singleton() {
        let comps = connected_components(&[edge("a", "a")]);
        assert_eq!(comps.len(), 1);
        let members = comps.groups.values().next().unwrap();
        assert_eq!(members, &vec!["a".to_string()]);
    }

    #[test]
    fn two_triangles_are_two_components() {
        let edges = vec![
            edge("a", "b"),
            edge("b", "c"),
            edge("c", "a"),
            edge("x", "y"),
            edge("y", "z"),
            edge("z", "x"),
        ];
        let comps = connected_components(&edges);
        assert_eq!(comps.len(), 2);
        let mut sizes: Vec<usize> = comps.groups.values().map(|m| m.len()).collect();
        sizes.sort();
        assert_eq!(sizes, vec![3, 3]);
    }
}
