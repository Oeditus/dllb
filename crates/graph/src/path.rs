//! Directed shortest-path finding via breadth-first search.
//!
//! Edges are treated as **directed**: a path may only traverse `src -> dst`
//! edges in their stored direction. BFS yields a path with the fewest hops;
//! an optional `max_depth` bounds the number of edges explored.

use std::collections::{HashMap, HashSet, VecDeque};

/// Find a shortest directed path from `src` to `dst`.
///
/// Returns the node sequence `[src, ..., dst]` (inclusive of both ends), or
/// `None` if `dst` is unreachable from `src` within `max_depth` edges. When
/// `max_depth` is `None` the search is unbounded. A zero-length path is
/// returned for `src == dst`.
pub fn shortest_path(
    edges: &[(String, String)],
    src: &str,
    dst: &str,
    max_depth: Option<usize>,
) -> Option<Vec<String>> {
    if src == dst {
        return Some(vec![src.to_string()]);
    }

    // Directed adjacency list over borrowed ids.
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    for (s, d) in edges {
        adj.entry(s.as_str()).or_default().push(d.as_str());
    }

    let mut visited: HashSet<&str> = HashSet::new();
    let mut parent: HashMap<&str, &str> = HashMap::new();
    let mut queue: VecDeque<(&str, usize)> = VecDeque::new();
    visited.insert(src);
    queue.push_back((src, 0));

    while let Some((node, depth)) = queue.pop_front() {
        // Do not expand beyond the depth budget (depth counts edges so far).
        if max_depth.is_some_and(|m| depth >= m) {
            continue;
        }
        let Some(neighbors) = adj.get(node) else {
            continue;
        };
        for &nb in neighbors {
            if !visited.insert(nb) {
                continue;
            }
            parent.insert(nb, node);
            if nb == dst {
                return Some(reconstruct(&parent, dst));
            }
            queue.push_back((nb, depth + 1));
        }
    }

    None
}

/// Walk parent pointers from `dst` back to the (parentless) source.
fn reconstruct(parent: &HashMap<&str, &str>, dst: &str) -> Vec<String> {
    let mut path = vec![dst];
    let mut cur = dst;
    while let Some(&p) = parent.get(cur) {
        path.push(p);
        cur = p;
    }
    path.reverse();
    path.into_iter().map(String::from).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(a: &str, b: &str) -> (String, String) {
        (a.to_string(), b.to_string())
    }

    #[test]
    fn direct_edge_is_a_two_node_path() {
        let edges = vec![e("a", "b")];
        assert_eq!(
            shortest_path(&edges, "a", "b", None),
            Some(vec!["a".into(), "b".into()])
        );
    }

    #[test]
    fn chain_returns_full_path() {
        let edges = vec![e("a", "b"), e("b", "c"), e("c", "d")];
        assert_eq!(
            shortest_path(&edges, "a", "d", None),
            Some(vec!["a".into(), "b".into(), "c".into(), "d".into()])
        );
    }

    #[test]
    fn picks_shorter_of_two_routes() {
        // a->d direct, and a->b->c->d. BFS must prefer the single hop.
        let edges = vec![e("a", "b"), e("b", "c"), e("c", "d"), e("a", "d")];
        assert_eq!(
            shortest_path(&edges, "a", "d", None),
            Some(vec!["a".into(), "d".into()])
        );
    }

    #[test]
    fn direction_is_respected() {
        // Only b->a exists, so a cannot reach b.
        let edges = vec![e("b", "a")];
        assert_eq!(shortest_path(&edges, "a", "b", None), None);
    }

    #[test]
    fn unreachable_returns_none() {
        let edges = vec![e("a", "b"), e("c", "d")];
        assert_eq!(shortest_path(&edges, "a", "d", None), None);
    }

    #[test]
    fn max_depth_bounds_the_search() {
        let edges = vec![e("a", "b"), e("b", "c")];
        // c is 2 hops away; a depth budget of 1 cannot reach it.
        assert_eq!(shortest_path(&edges, "a", "c", Some(1)), None);
        assert_eq!(
            shortest_path(&edges, "a", "c", Some(2)),
            Some(vec!["a".into(), "b".into(), "c".into()])
        );
    }

    #[test]
    fn same_node_is_zero_length() {
        let edges = vec![e("a", "b")];
        assert_eq!(
            shortest_path(&edges, "a", "a", None),
            Some(vec!["a".into()])
        );
    }
}
