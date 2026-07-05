//! Structural similarity comparison for MetaAST trees.
//!
//! Provides functions for comparing AST subtrees structurally (ignoring
//! names and values), computing structural fingerprints for fast
//! pre-filtering, and detecting code clones across a set of subtrees.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use crate::meta_ast::{MetaNode, NodeChildren};

/// A pair of subtrees identified as structural clones.
#[derive(Debug, Clone)]
pub struct ClonePair {
    pub index_a: usize,
    pub index_b: usize,
    pub similarity: f64,
}

/// Count the total number of nodes in a subtree.
fn subtree_size(node: &MetaNode) -> usize {
    let children_size: usize = node.child_nodes().iter().map(subtree_size).sum();
    1 + children_size
}

/// Compute structural similarity between two MetaAST subtrees.
///
/// Returns a score in `0.0..=1.0` comparing tree shape and node types,
/// ignoring names, values, and metadata. Uses a greedy best-match pairing
/// of children weighted by subtree size.
///
/// Scoring rules:
/// - Same `NodeType`, both leaves: 1.0
/// - Same `NodeType`, both composite: recursive greedy child matching
/// - Different `NodeType`: 0.0 base, with partial credit if children overlap
pub fn structural_similarity(a: &MetaNode, b: &MetaNode) -> f64 {
    let same_type = a.node_type == b.node_type;

    match (&a.children, &b.children) {
        // Both leaves
        (NodeChildren::Value(_), NodeChildren::Value(_)) => {
            if same_type {
                1.0
            } else {
                0.0
            }
        }
        // Both composite
        (NodeChildren::Nodes(ac), NodeChildren::Nodes(bc)) => {
            if ac.is_empty() && bc.is_empty() {
                // Both empty composites
                return if same_type { 1.0 } else { 0.0 };
            }

            let children_sim = greedy_children_similarity(ac, bc);

            if same_type {
                // Weight: root match counts for 1 node, children weighted by size
                let total_size = (subtree_size(a) + subtree_size(b)) as f64 / 2.0;
                let root_weight = 1.0 / total_size;
                let children_weight = 1.0 - root_weight;
                root_weight * 1.0 + children_weight * children_sim
            } else {
                // Different type: partial credit from children overlap only
                let total_size = (subtree_size(a) + subtree_size(b)) as f64 / 2.0;
                let children_weight = 1.0 - (1.0 / total_size);
                children_weight * children_sim * 0.5
            }
        }
        // One leaf, one composite — structurally different
        _ => {
            if same_type {
                0.3
            } else {
                0.0
            }
        }
    }
}

/// Greedy best-match pairing of two child lists.
///
/// For each child in the smaller list, find the best match in the larger
/// list (by structural similarity weighted by size), then remove the
/// matched pair. Unmatched nodes contribute 0.
fn greedy_children_similarity(a_children: &[MetaNode], b_children: &[MetaNode]) -> f64 {
    if a_children.is_empty() && b_children.is_empty() {
        return 1.0;
    }
    if a_children.is_empty() || b_children.is_empty() {
        return 0.0;
    }

    // Work with the smaller set as the "source" to match against the larger
    let (smaller, larger) = if a_children.len() <= b_children.len() {
        (a_children, b_children)
    } else {
        (b_children, a_children)
    };

    let mut used = vec![false; larger.len()];
    let mut total_weighted_sim = 0.0;
    let mut total_weight = 0.0;

    // Compute sizes for weighting
    let all_sizes_smaller: Vec<usize> = smaller.iter().map(subtree_size).collect();
    let all_sizes_larger: Vec<usize> = larger.iter().map(subtree_size).collect();

    for (i, s_child) in smaller.iter().enumerate() {
        let s_size = all_sizes_smaller[i];
        let mut best_sim = 0.0f64;
        let mut best_j = None;

        for (j, l_child) in larger.iter().enumerate() {
            if used[j] {
                continue;
            }
            // Quick pre-check: same type gets priority
            let sim = structural_similarity(s_child, l_child);
            if sim > best_sim {
                best_sim = sim;
                best_j = Some(j);
            }
        }

        let weight = (s_size + best_j.map_or(0, |j| all_sizes_larger[j])) as f64 / 2.0;
        total_weighted_sim += best_sim * weight;
        total_weight += weight;

        if let Some(j) = best_j {
            used[j] = true;
        }
    }

    // Account for unmatched nodes in the larger set
    for (j, _) in larger.iter().enumerate() {
        if !used[j] {
            let weight = all_sizes_larger[j] as f64;
            // Unmatched contributes 0 similarity
            total_weight += weight;
        }
    }

    if total_weight == 0.0 {
        0.0
    } else {
        total_weighted_sim / total_weight
    }
}

/// Produce a structural fingerprint of a MetaAST tree.
///
/// Returns a sequence of `u64` hashes representing the structural skeleton
/// (node types + arity at each level). Useful for fast pre-filtering before
/// expensive similarity comparison.
///
/// The fingerprint is computed by recursively hashing `(node_type, children_count,
/// child_fingerprints...)` at each node, collecting all node-level hashes in
/// pre-order traversal.
pub fn tree_fingerprint(node: &MetaNode) -> Vec<u64> {
    let mut result = Vec::new();
    collect_fingerprints(node, &mut result);
    result
}

fn node_structural_hash(node: &MetaNode) -> u64 {
    let mut hasher = DefaultHasher::new();
    // Hash the node type discriminant
    std::mem::discriminant(&node.node_type).hash(&mut hasher);
    let children = node.child_nodes();
    children.len().hash(&mut hasher);
    for child in children {
        node_structural_hash(child).hash(&mut hasher);
    }
    hasher.finish()
}

fn collect_fingerprints(node: &MetaNode, out: &mut Vec<u64>) {
    out.push(node_structural_hash(node));
    for child in node.child_nodes() {
        collect_fingerprints(child, out);
    }
}

/// Compute a single Zobrist-style hash of the entire subtree structure.
///
/// Two structurally identical trees (same node types, same arity at each
/// position) produce the same hash. This is equivalent to the root element
/// of `tree_fingerprint`.
pub fn subtree_hash(node: &MetaNode) -> u64 {
    node_structural_hash(node)
}

/// Find pairs of subtrees with structural similarity above the given threshold.
///
/// Uses fingerprints for fast pre-filtering: only pairs whose fingerprint
/// vectors share the same root hash (and thus the same top-level structure)
/// are subjected to the full `structural_similarity` comparison.
///
/// # Arguments
/// - `nodes`: slice of references to MetaAST subtrees
/// - `threshold`: minimum similarity score (0.0..1.0) for a pair to be reported
///
/// # Returns
/// A vector of `ClonePair` structs for all pairs above the threshold,
/// sorted by descending similarity.
pub fn find_clones(nodes: &[&MetaNode], threshold: f64) -> Vec<ClonePair> {
    let fingerprints: Vec<Vec<u64>> = nodes.iter().map(|n| tree_fingerprint(n)).collect();

    let mut pairs = Vec::new();

    for i in 0..nodes.len() {
        for j in (i + 1)..nodes.len() {
            // Pre-filter: check if fingerprint vectors share enough structure
            // to be worth a full comparison. If root hashes match, they are
            // structurally identical at the top level.
            let quick_match = fingerprints[i].first() == fingerprints[j].first();

            if quick_match {
                // Root hashes match — likely very similar, do full check
                let sim = structural_similarity(nodes[i], nodes[j]);
                if sim >= threshold {
                    pairs.push(ClonePair {
                        index_a: i,
                        index_b: j,
                        similarity: sim,
                    });
                }
            } else {
                // Root hashes differ — still check if fingerprint overlap is
                // significant enough to warrant full comparison.
                // Heuristic: if >50% of the smaller fingerprint set appears
                // in the larger set, do the full comparison.
                let (smaller_fp, larger_fp) = if fingerprints[i].len() <= fingerprints[j].len() {
                    (&fingerprints[i], &fingerprints[j])
                } else {
                    (&fingerprints[j], &fingerprints[i])
                };

                if smaller_fp.is_empty() {
                    continue;
                }

                let overlap = smaller_fp.iter().filter(|h| larger_fp.contains(h)).count();
                let overlap_ratio = overlap as f64 / smaller_fp.len() as f64;

                if overlap_ratio > 0.5 {
                    let sim = structural_similarity(nodes[i], nodes[j]);
                    if sim >= threshold {
                        pairs.push(ClonePair {
                            index_a: i,
                            index_b: j,
                            similarity: sim,
                        });
                    }
                }
            }
        }
    }

    pairs.sort_by(|a, b| {
        b.similarity
            .partial_cmp(&a.similarity)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meta_ast::{MetaValue, NodeType};

    /// Build a simple binary op tree: BinaryOp(Variable, Literal)
    fn make_binop(var_name: &str, lit_val: i64) -> MetaNode {
        MetaNode::composite(
            NodeType::BinaryOp,
            vec![("operator".into(), MetaValue::Atom("+".into()))],
            vec![
                MetaNode::leaf(
                    NodeType::Variable,
                    vec![],
                    MetaValue::String(var_name.into()),
                ),
                MetaNode::leaf(NodeType::Literal, vec![], MetaValue::Int(lit_val)),
            ],
        )
    }

    /// Build a function def wrapping a body node
    fn make_fn(name: &str, body: MetaNode) -> MetaNode {
        MetaNode::composite(
            NodeType::FunctionDef,
            vec![
                ("name".into(), MetaValue::String(name.into())),
                ("arity".into(), MetaValue::Int(1)),
            ],
            vec![
                MetaNode::leaf(NodeType::Param, vec![], MetaValue::String("arg".into())),
                body,
            ],
        )
    }

    #[test]
    fn identical_trees_score_one() {
        let a = make_binop("x", 5);
        let b = make_binop("y", 10); // same structure, different values
        let sim = structural_similarity(&a, &b);
        // Structurally identical (same NodeTypes at each position)
        assert!(
            (sim - 1.0).abs() < 1e-10,
            "Expected 1.0 for identical structure, got {sim}"
        );
    }

    #[test]
    fn completely_different_trees_score_low() {
        // A leaf literal vs a deeply nested composite
        let a = MetaNode::leaf(NodeType::Literal, vec![], MetaValue::Int(42));
        let b = MetaNode::composite(
            NodeType::Container,
            vec![],
            vec![MetaNode::composite(
                NodeType::FunctionDef,
                vec![],
                vec![
                    MetaNode::leaf(NodeType::Param, vec![], MetaValue::String("x".into())),
                    MetaNode::composite(
                        NodeType::Block,
                        vec![],
                        vec![
                            MetaNode::leaf(
                                NodeType::Variable,
                                vec![],
                                MetaValue::String("a".into()),
                            ),
                            MetaNode::leaf(
                                NodeType::Variable,
                                vec![],
                                MetaValue::String("b".into()),
                            ),
                        ],
                    ),
                ],
            )],
        );
        let sim = structural_similarity(&a, &b);
        assert!(
            sim < 0.2,
            "Expected near-0 for completely different trees, got {sim}"
        );
    }

    #[test]
    fn same_structure_different_leaf_values_score_high() {
        // Two function defs with identical structure but different variable names/values
        let a = make_fn("add", make_binop("x", 1));
        let b = make_fn("subtract", make_binop("y", 99));
        let sim = structural_similarity(&a, &b);
        assert!(
            sim > 0.95,
            "Expected high similarity for same structure, got {sim}"
        );
    }

    #[test]
    fn fingerprint_equality_implies_structural_equality() {
        let a = make_fn("foo", make_binop("a", 1));
        let b = make_fn("bar", make_binop("b", 2));

        let fp_a = tree_fingerprint(&a);
        let fp_b = tree_fingerprint(&b);

        // Same structure should yield same fingerprints
        assert_eq!(
            fp_a, fp_b,
            "Identical structures must have equal fingerprints"
        );

        // Different structure should yield different fingerprints
        let c = MetaNode::leaf(NodeType::Literal, vec![], MetaValue::Int(0));
        let fp_c = tree_fingerprint(&c);
        assert_ne!(
            fp_a, fp_c,
            "Different structures must have different fingerprints"
        );
    }

    #[test]
    fn find_clones_returns_pairs_above_threshold() {
        let fn1 = make_fn("alpha", make_binop("x", 1));
        let fn2 = make_fn("beta", make_binop("y", 2));
        let fn3 = MetaNode::composite(
            NodeType::Loop,
            vec![],
            vec![
                MetaNode::leaf(NodeType::Variable, vec![], MetaValue::String("i".into())),
                MetaNode::composite(
                    NodeType::Block,
                    vec![],
                    vec![
                        MetaNode::leaf(NodeType::Literal, vec![], MetaValue::Int(0)),
                        MetaNode::leaf(NodeType::Literal, vec![], MetaValue::Int(1)),
                        MetaNode::leaf(NodeType::Literal, vec![], MetaValue::Int(2)),
                    ],
                ),
            ],
        );

        let nodes: Vec<&MetaNode> = vec![&fn1, &fn2, &fn3];
        let clones = find_clones(&nodes, 0.9);

        // fn1 and fn2 are structural clones; fn3 is different
        assert!(
            !clones.is_empty(),
            "Expected at least one clone pair, got {}",
            clones.len()
        );
        assert_eq!(clones[0].index_a, 0);
        assert_eq!(clones[0].index_b, 1);
        assert!(clones[0].similarity > 0.9);

        // fn3 should not be a clone of fn1 or fn2 at 0.9 threshold
        let high_clones: Vec<_> = clones
            .iter()
            .filter(|c| c.index_a == 2 || c.index_b == 2)
            .collect();
        assert!(
            high_clones.is_empty(),
            "Loop node should not be a clone of function nodes"
        );
    }

    #[test]
    fn subtree_hash_identical_structures() {
        let a = make_binop("x", 1);
        let b = make_binop("y", 2);
        assert_eq!(
            subtree_hash(&a),
            subtree_hash(&b),
            "Structurally identical trees must have the same subtree_hash"
        );
    }

    #[test]
    fn subtree_hash_different_structures() {
        let a = make_binop("x", 1);
        let b = MetaNode::composite(
            NodeType::BinaryOp,
            vec![],
            vec![
                MetaNode::leaf(NodeType::Literal, vec![], MetaValue::Int(1)),
                MetaNode::leaf(NodeType::Literal, vec![], MetaValue::Int(2)),
            ],
        );
        // Same top node type and arity, but different child node types
        assert_ne!(
            subtree_hash(&a),
            subtree_hash(&b),
            "Structurally different trees should have different hashes"
        );
    }
}
