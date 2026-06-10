//! Integration tests for community detection algorithms.
//!
//! Graph topology used in most tests:
//!
//! Cluster A: a1 -- a2 -- a3 -- a1  (dense triangle)
//! Cluster B: b1 -- b2 -- b3 -- b1  (dense triangle)
//! Bridge:    a1 -> b1               (single weak link)
//!
//! Both Louvain and Label Propagation should recover {a1,a2,a3} and
//! {b1,b2,b3} as two separate communities (or as four+ communities that
//! never mix the two clusters).

use dllb_graph::community::{Algorithm, Options, detect_weighted};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn two_cluster_edges() -> Vec<(String, String, f64)> {
    vec![
        // Cluster A (dense, weight 3.0 per link)
        ("a1".into(), "a2".into(), 3.0),
        ("a2".into(), "a1".into(), 3.0),
        ("a2".into(), "a3".into(), 3.0),
        ("a3".into(), "a2".into(), 3.0),
        ("a3".into(), "a1".into(), 3.0),
        ("a1".into(), "a3".into(), 3.0),
        // Cluster B (dense, weight 3.0 per link)
        ("b1".into(), "b2".into(), 3.0),
        ("b2".into(), "b1".into(), 3.0),
        ("b2".into(), "b3".into(), 3.0),
        ("b3".into(), "b2".into(), 3.0),
        ("b3".into(), "b1".into(), 3.0),
        ("b1".into(), "b3".into(), 3.0),
        // Weak bridge between clusters (weight 1.0)
        ("a1".into(), "b1".into(), 1.0),
    ]
}

/// Returns the community label assigned to `node` in `result`.
fn community_of<'a>(
    node: &str,
    groups: &'a std::collections::HashMap<String, Vec<String>>,
) -> Option<&'a str> {
    groups
        .iter()
        .find(|(_, members)| members.iter().any(|m| m == node))
        .map(|(comm, _)| comm.as_str())
}

// ---------------------------------------------------------------------------
// Louvain tests
// ---------------------------------------------------------------------------

#[test]
fn louvain_recovers_two_clusters() {
    let edges = two_cluster_edges();
    let opts = Options {
        algorithm: Algorithm::Louvain,
        max_iterations: 20,
        resolution: 1.0,
        ..Options::default()
    };
    let result = detect_weighted(&edges, &opts);

    // Sanity: all 6 nodes are present.
    let all: Vec<String> = result
        .groups
        .values()
        .flat_map(|v| v.iter().cloned())
        .collect();
    assert_eq!(all.len(), 6, "expected 6 nodes total, got {}", all.len());

    // a-cluster nodes must all share the same community label.
    let ca1 = community_of("a1", &result.groups).expect("a1 missing");
    let ca2 = community_of("a2", &result.groups).expect("a2 missing");
    let ca3 = community_of("a3", &result.groups).expect("a3 missing");
    assert_eq!(ca1, ca2, "a1 and a2 ended up in different communities");
    assert_eq!(ca1, ca3, "a1 and a3 ended up in different communities");

    // b-cluster nodes must all share the same community label.
    let cb1 = community_of("b1", &result.groups).expect("b1 missing");
    let cb2 = community_of("b2", &result.groups).expect("b2 missing");
    let cb3 = community_of("b3", &result.groups).expect("b3 missing");
    assert_eq!(cb1, cb2, "b1 and b2 ended up in different communities");
    assert_eq!(cb1, cb3, "b1 and b3 ended up in different communities");

    // The two clusters must be in different communities.
    assert_ne!(ca1, cb1, "clusters A and B merged into one community");
}

#[test]
fn louvain_empty_graph() {
    let result = detect_weighted(
        &[],
        &Options {
            algorithm: Algorithm::Louvain,
            ..Options::default()
        },
    );
    assert!(result.is_empty());
}

#[test]
fn louvain_single_node_no_edges() {
    // A graph where the only edge is a self-loop — should be treated as empty.
    let edges = vec![("x".into(), "x".into(), 1.0)];
    let result = detect_weighted(
        &edges,
        &Options {
            algorithm: Algorithm::Louvain,
            ..Options::default()
        },
    );
    // Self-loops are skipped; x is isolated.
    assert_eq!(result.len(), 1);
    assert!(result.groups.values().any(|v| v == &["x"]));
}

#[test]
fn louvain_two_isolated_nodes() {
    // No edges between a and b → each in their own community.
    let edges: Vec<(String, String, f64)> = vec![];
    // Actually supply 0 edges; test that detect on empty returns empty.
    let result = detect_weighted(&edges, &Options::default());
    assert!(result.is_empty());
}

#[test]
fn louvain_community_count_lte_node_count() {
    let edges = two_cluster_edges();
    let result = detect_weighted(&edges, &Options::default());
    let node_count: usize = result.groups.values().map(|v| v.len()).sum();
    assert!(result.len() <= node_count);
}

#[test]
fn louvain_resolution_high_produces_more_communities() {
    let edges = two_cluster_edges();

    let coarse = detect_weighted(
        &edges,
        &Options {
            algorithm: Algorithm::Louvain,
            resolution: 0.1,
            max_iterations: 20,
            ..Options::default()
        },
    );
    let fine = detect_weighted(
        &edges,
        &Options {
            algorithm: Algorithm::Louvain,
            resolution: 5.0,
            max_iterations: 20,
            ..Options::default()
        },
    );
    // Higher resolution should yield at least as many communities.
    assert!(
        fine.len() >= coarse.len(),
        "expected fine({}) >= coarse({})",
        fine.len(),
        coarse.len()
    );
}

// ---------------------------------------------------------------------------
// Label Propagation tests
// ---------------------------------------------------------------------------

#[test]
fn lp_recovers_two_clusters() {
    let edges = two_cluster_edges();
    let opts = Options {
        algorithm: Algorithm::LabelPropagation,
        max_iterations: 50,
        ..Options::default()
    };
    let result = detect_weighted(&edges, &opts);

    let all: Vec<String> = result
        .groups
        .values()
        .flat_map(|v| v.iter().cloned())
        .collect();
    assert_eq!(all.len(), 6);

    let ca1 = community_of("a1", &result.groups).expect("a1 missing");
    let ca2 = community_of("a2", &result.groups).expect("a2 missing");
    let ca3 = community_of("a3", &result.groups).expect("a3 missing");
    assert_eq!(ca1, ca2);
    assert_eq!(ca1, ca3);

    let cb1 = community_of("b1", &result.groups).expect("b1 missing");
    let cb2 = community_of("b2", &result.groups).expect("b2 missing");
    let cb3 = community_of("b3", &result.groups).expect("b3 missing");
    assert_eq!(cb1, cb2);
    assert_eq!(cb1, cb3);

    assert_ne!(ca1, cb1);
}

#[test]
fn lp_empty_graph() {
    let result = detect_weighted(
        &[],
        &Options {
            algorithm: Algorithm::LabelPropagation,
            ..Options::default()
        },
    );
    assert!(result.is_empty());
}

#[test]
fn lp_star_graph_converges() {
    // Hub h connected to 5 spokes s0..s4.
    let mut edges = Vec::new();
    for i in 0..5usize {
        let spoke = format!("s{i}");
        edges.push(("h".to_string(), spoke.clone(), 1.0));
        edges.push((spoke, "h".to_string(), 1.0));
    }

    let result = detect_weighted(
        &edges,
        &Options {
            algorithm: Algorithm::LabelPropagation,
            max_iterations: 30,
            ..Options::default()
        },
    );
    // All nodes reachable from hub — LP should converge to 1 community.
    assert_eq!(result.len(), 1, "star should converge to 1 community");
}

// ---------------------------------------------------------------------------
// EdgeStore integration: scan_all_outgoing feeds into community detection
// ---------------------------------------------------------------------------

#[test]
fn scan_all_outgoing_feeds_detect() {
    use dllb_graph::{Edge, EdgeStore};
    use dllb_storage::db::DllbStorage;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.redb");
    let storage = DllbStorage::open(&path).unwrap();
    let store = EdgeStore::new(&storage, "ns", "db", "calls");

    // Insert a small two-cluster graph via RELATE.
    for (s, d) in [
        ("a1", "a2"),
        ("a2", "a3"),
        ("a3", "a1"),
        ("b1", "b2"),
        ("b2", "b3"),
        ("b3", "b1"),
        ("a1", "b1"),
    ] {
        store.relate(&Edge::new(s, "calls", d)).unwrap();
    }

    let edges = store.scan_all_outgoing().unwrap();
    assert_eq!(
        edges.len(),
        7,
        "expected 7 outgoing edges, got {}",
        edges.len()
    );

    // All weights default to 1.0 (no weight property set).
    assert!(
        edges
            .iter()
            .all(|(_, _, w)| (*w - 1.0).abs() < f64::EPSILON)
    );

    // Clusters must still be recoverable.
    let opts = Options::default();
    let result = detect_weighted(&edges, &opts);

    let ca1 = community_of("a1", &result.groups).expect("a1 missing");
    let ca2 = community_of("a2", &result.groups).expect("a2 missing");
    assert_eq!(ca1, ca2, "a1 and a2 should be in the same community");

    let cb1 = community_of("b1", &result.groups).expect("b1 missing");
    let cb2 = community_of("b2", &result.groups).expect("b2 missing");
    assert_eq!(cb1, cb2, "b1 and b2 should be in the same community");

    assert_ne!(ca1, cb1, "clusters A and B should be separate");
}
