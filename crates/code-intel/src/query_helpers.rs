//! High-level AST query utilities for common code intelligence operations.
//!
//! These functions provide structural queries over MetaAST trees:
//! parent/sibling/ancestor lookups, type and name searches, scope resolution,
//! call target extraction, and complexity estimation.

use crate::meta_ast::{MetaNode, MetaValue, NodeType};

/// Find the immediate parent of a target node in the tree.
/// Uses pointer equality to identify the target.
pub fn find_parent<'a>(root: &'a MetaNode, target: &MetaNode) -> Option<&'a MetaNode> {
    if std::ptr::eq(root as *const MetaNode, target as *const MetaNode) {
        return None;
    }
    find_parent_inner(root, target)
}

fn find_parent_inner<'a>(node: &'a MetaNode, target: &MetaNode) -> Option<&'a MetaNode> {
    for child in node.child_nodes() {
        if std::ptr::eq(child as *const MetaNode, target as *const MetaNode) {
            return Some(node);
        }
        if let Some(parent) = find_parent_inner(child, target) {
            return Some(parent);
        }
    }
    None
}

/// Find all sibling nodes (same parent, excluding the target itself).
/// Uses pointer equality to identify the target.
pub fn find_siblings<'a>(root: &'a MetaNode, target: &MetaNode) -> Vec<&'a MetaNode> {
    match find_parent(root, target) {
        Some(parent) => parent
            .child_nodes()
            .iter()
            .filter(|child| !std::ptr::eq(*child as *const MetaNode, target as *const MetaNode))
            .collect(),
        None => vec![],
    }
}

/// Return the path from root to the target's parent (empty if target is root or not found).
pub fn ancestors<'a>(root: &'a MetaNode, target: &MetaNode) -> Vec<&'a MetaNode> {
    if std::ptr::eq(root as *const MetaNode, target as *const MetaNode) {
        return vec![];
    }
    let mut path = Vec::new();
    if ancestors_inner(root, target, &mut path) {
        // path contains root..parent (the last element is the immediate parent)
        path
    } else {
        vec![]
    }
}

fn ancestors_inner<'a>(
    node: &'a MetaNode,
    target: &MetaNode,
    path: &mut Vec<&'a MetaNode>,
) -> bool {
    for child in node.child_nodes() {
        if std::ptr::eq(child as *const MetaNode, target as *const MetaNode) {
            path.push(node);
            return true;
        }
    }
    for child in node.child_nodes() {
        path.push(node);
        if ancestors_inner(child, target, path) {
            return true;
        }
        path.pop();
    }
    false
}

/// Collect all nodes of a given type in the tree (depth-first).
pub fn find_by_type(node: &MetaNode, node_type: NodeType) -> Vec<&MetaNode> {
    let mut results = Vec::new();
    find_by_type_inner(node, node_type, &mut results);
    results
}

fn find_by_type_inner<'a>(
    node: &'a MetaNode,
    node_type: NodeType,
    results: &mut Vec<&'a MetaNode>,
) {
    if node.node_type == node_type {
        results.push(node);
    }
    for child in node.child_nodes() {
        find_by_type_inner(child, node_type, results);
    }
}

/// Find all nodes whose "name" metadata field matches the given string.
pub fn find_by_name<'a>(node: &'a MetaNode, name: &str) -> Vec<&'a MetaNode> {
    let mut results = Vec::new();
    find_by_name_inner(node, name, &mut results);
    results
}

fn find_by_name_inner<'a>(node: &'a MetaNode, name: &str, results: &mut Vec<&'a MetaNode>) {
    if node.get_meta_str("name") == Some(name) {
        results.push(node);
    }
    for child in node.child_nodes() {
        find_by_name_inner(child, name, results);
    }
}

/// Find the nearest FunctionDef ancestor of the target node.
pub fn containing_function<'a>(root: &'a MetaNode, target: &MetaNode) -> Option<&'a MetaNode> {
    let path = ancestors(root, target);
    path.into_iter()
        .rev()
        .find(|n| n.node_type == NodeType::FunctionDef)
}

/// Find the nearest Container ancestor (module/class) of the target node.
pub fn containing_container<'a>(root: &'a MetaNode, target: &MetaNode) -> Option<&'a MetaNode> {
    let path = ancestors(root, target);
    path.into_iter()
        .rev()
        .find(|n| n.node_type == NodeType::Container)
}

/// Given a line number, find all nodes whose line range contains it.
/// Looks for "line" (single line), or "line_start"/"line_end" in metadata.
/// Returns nodes from outermost to innermost.
pub fn scope_at(root: &MetaNode, line: i64) -> Vec<&MetaNode> {
    let mut results = Vec::new();
    scope_at_inner(root, line, &mut results);
    results
}

fn scope_at_inner<'a>(node: &'a MetaNode, line: i64, results: &mut Vec<&'a MetaNode>) {
    let contains = if let (Some(&MetaValue::Int(start)), Some(&MetaValue::Int(end))) =
        (node.get_meta("line_start"), node.get_meta("line_end"))
    {
        line >= start && line <= end
    } else if let Some(&MetaValue::Int(l)) = node.get_meta("line") {
        line == l
    } else {
        false
    };

    if contains {
        results.push(node);
    }

    // Always recurse into children to find inner scopes
    for child in node.child_nodes() {
        scope_at_inner(child, line, results);
    }
}

/// Extract all function call target names within a subtree (flattens nested calls).
pub fn call_targets(node: &MetaNode) -> Vec<String> {
    let mut targets = Vec::new();
    call_targets_inner(node, &mut targets);
    targets
}

fn call_targets_inner(node: &MetaNode, targets: &mut Vec<String>) {
    if node.node_type == NodeType::FunctionCall
        && let Some(name) = node.get_meta_str("name")
    {
        targets.push(name.to_string());
    }
    for child in node.child_nodes() {
        call_targets_inner(child, targets);
    }
}

/// Estimate cyclomatic complexity of a function body by counting branch points.
/// Counts: Conditional, Loop, PatternMatch, MatchArm, ExceptionHandling nodes.
/// Base complexity is 1 (a straight-line function).
pub fn complexity_estimate(node: &MetaNode) -> usize {
    let mut count = 1; // base complexity
    count_branches(node, &mut count);
    count
}

fn count_branches(node: &MetaNode, count: &mut usize) {
    match node.node_type {
        NodeType::Conditional
        | NodeType::Loop
        | NodeType::PatternMatch
        | NodeType::MatchArm
        | NodeType::ExceptionHandling => {
            *count += 1;
        }
        _ => {}
    }
    for child in node.child_nodes() {
        count_branches(child, count);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meta_ast::*;

    /// Build a test tree:
    /// Container "MyModule" (lines 1-20)
    ///   FunctionDef "process" (lines 2-15)
    ///     Conditional (line 3)
    ///       FunctionCall "validate"
    ///       FunctionCall "transform"
    ///     Loop (line 7)
    ///       FunctionCall "step"
    ///   FunctionDef "helper" (lines 16-19)
    ///     Variable "x"
    fn test_tree() -> MetaNode {
        MetaNode::composite(
            NodeType::Container,
            vec![
                ("name".into(), MetaValue::String("MyModule".into())),
                ("line_start".into(), MetaValue::Int(1)),
                ("line_end".into(), MetaValue::Int(20)),
            ],
            vec![
                MetaNode::composite(
                    NodeType::FunctionDef,
                    vec![
                        ("name".into(), MetaValue::String("process".into())),
                        ("line_start".into(), MetaValue::Int(2)),
                        ("line_end".into(), MetaValue::Int(15)),
                    ],
                    vec![
                        MetaNode::composite(
                            NodeType::Conditional,
                            vec![("line".into(), MetaValue::Int(3))],
                            vec![
                                MetaNode::composite(
                                    NodeType::FunctionCall,
                                    vec![("name".into(), MetaValue::String("validate".into()))],
                                    vec![],
                                ),
                                MetaNode::composite(
                                    NodeType::FunctionCall,
                                    vec![("name".into(), MetaValue::String("transform".into()))],
                                    vec![],
                                ),
                            ],
                        ),
                        MetaNode::composite(
                            NodeType::Loop,
                            vec![
                                ("line_start".into(), MetaValue::Int(7)),
                                ("line_end".into(), MetaValue::Int(10)),
                            ],
                            vec![MetaNode::composite(
                                NodeType::FunctionCall,
                                vec![("name".into(), MetaValue::String("step".into()))],
                                vec![],
                            )],
                        ),
                    ],
                ),
                MetaNode::composite(
                    NodeType::FunctionDef,
                    vec![
                        ("name".into(), MetaValue::String("helper".into())),
                        ("line_start".into(), MetaValue::Int(16)),
                        ("line_end".into(), MetaValue::Int(19)),
                    ],
                    vec![MetaNode::leaf(
                        NodeType::Variable,
                        vec![("name".into(), MetaValue::String("x".into()))],
                        MetaValue::String("x".into()),
                    )],
                ),
            ],
        )
    }

    #[test]
    fn test_find_parent() {
        let tree = test_tree();
        // The first child of root is the "process" FunctionDef
        let process_fn = &tree.child_nodes()[0];
        let parent = find_parent(&tree, process_fn);
        assert!(parent.is_some());
        assert!(std::ptr::eq(
            parent.unwrap() as *const MetaNode,
            &tree as *const MetaNode
        ));

        // Root has no parent
        assert!(find_parent(&tree, &tree).is_none());
    }

    #[test]
    fn test_find_siblings() {
        let tree = test_tree();
        let process_fn = &tree.child_nodes()[0];
        let siblings = find_siblings(&tree, process_fn);
        assert_eq!(siblings.len(), 1);
        assert_eq!(siblings[0].get_meta_str("name"), Some("helper"));
    }

    #[test]
    fn test_ancestors() {
        let tree = test_tree();
        // Navigate to the Loop node: root -> process_fn -> loop
        let process_fn = &tree.child_nodes()[0];
        let loop_node = &process_fn.child_nodes()[1];
        let path = ancestors(&tree, loop_node);
        assert_eq!(path.len(), 2); // [Container, FunctionDef]
        assert_eq!(path[0].node_type, NodeType::Container);
        assert_eq!(path[1].node_type, NodeType::FunctionDef);
    }

    #[test]
    fn test_find_by_type_and_name() {
        let tree = test_tree();
        let calls = find_by_type(&tree, NodeType::FunctionCall);
        assert_eq!(calls.len(), 3);

        let named = find_by_name(&tree, "validate");
        assert_eq!(named.len(), 1);
        assert_eq!(named[0].node_type, NodeType::FunctionCall);
    }

    #[test]
    fn test_containing_function_and_container() {
        let tree = test_tree();
        // The "step" call is inside process_fn -> Loop -> FunctionCall
        let process_fn = &tree.child_nodes()[0];
        let loop_node = &process_fn.child_nodes()[1];
        let step_call = &loop_node.child_nodes()[0];

        let func = containing_function(&tree, step_call);
        assert!(func.is_some());
        assert_eq!(func.unwrap().get_meta_str("name"), Some("process"));

        let container = containing_container(&tree, step_call);
        assert!(container.is_some());
        assert_eq!(container.unwrap().get_meta_str("name"), Some("MyModule"));
    }

    #[test]
    fn test_scope_at() {
        let tree = test_tree();
        // Line 8 is inside Container(1-20), FunctionDef "process"(2-15), Loop(7-10)
        let scopes = scope_at(&tree, 8);
        assert_eq!(scopes.len(), 3);
        assert_eq!(scopes[0].node_type, NodeType::Container);
        assert_eq!(scopes[1].node_type, NodeType::FunctionDef);
        assert_eq!(scopes[2].node_type, NodeType::Loop);

        // Line 17 is inside Container(1-20), FunctionDef "helper"(16-19)
        let scopes = scope_at(&tree, 17);
        assert_eq!(scopes.len(), 2);
        assert_eq!(scopes[0].node_type, NodeType::Container);
        assert_eq!(scopes[1].node_type, NodeType::FunctionDef);
    }

    #[test]
    fn test_call_targets() {
        let tree = test_tree();
        let targets = call_targets(&tree);
        assert_eq!(targets, vec!["validate", "transform", "step"]);
    }

    #[test]
    fn test_complexity_estimate() {
        let tree = test_tree();
        let process_fn = &tree.child_nodes()[0];
        // process has: Conditional(+1) + Loop(+1) + base(1) = 3
        assert_eq!(complexity_estimate(process_fn), 3);

        // helper has no branches: base(1)
        let helper_fn = &tree.child_nodes()[1];
        assert_eq!(complexity_estimate(helper_fn), 1);
    }
}
