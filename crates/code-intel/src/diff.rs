//! AST-level diff: detect structural changes between two versions of a MetaAST.
//!
//! Unlike text diffs (which report line changes), this module understands the
//! tree structure and reports semantic changes: added functions, removed imports,
//! modified bodies, renamed containers, etc.
//!
//! Primary use case: incremental re-indexing in ragex. When a file changes, only
//! the functions/modules that actually changed need re-embedding and re-insertion
//! into dllb.

use crate::meta_ast::{MetaNode, MetaValue, NodeChildren, NodeType};

/// A single change detected between two AST versions.
#[derive(Debug, Clone, PartialEq)]
pub struct AstChange {
    /// What kind of change occurred.
    pub kind: ChangeKind,
    /// The node type affected.
    pub node_type: NodeType,
    /// Name of the affected entity (function name, module name, etc.).
    pub name: String,
    /// Optional line number in the new version (None for deletions).
    pub line: Option<i64>,
}

/// Classification of an AST change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    /// A new entity was added.
    Added,
    /// An existing entity was removed.
    Removed,
    /// An entity's body/children changed but its signature is the same.
    Modified,
    /// An entity was renamed (detected via structural similarity of body).
    Renamed,
}

/// Summary of all changes between two AST versions.
#[derive(Debug, Clone, Default)]
pub struct DiffSummary {
    pub changes: Vec<AstChange>,
    pub added: usize,
    pub removed: usize,
    pub modified: usize,
    pub renamed: usize,
}

impl DiffSummary {
    /// True if no changes were detected.
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// Names of entities that need re-indexing (added + modified + renamed).
    pub fn stale_entities(&self) -> Vec<&str> {
        self.changes
            .iter()
            .filter(|c| {
                matches!(
                    c.kind,
                    ChangeKind::Added | ChangeKind::Modified | ChangeKind::Renamed
                )
            })
            .map(|c| c.name.as_str())
            .collect()
    }

    /// Names of entities that should be removed from the index.
    pub fn removed_entities(&self) -> Vec<&str> {
        self.changes
            .iter()
            .filter(|c| c.kind == ChangeKind::Removed)
            .map(|c| c.name.as_str())
            .collect()
    }
}

/// Compare two MetaAST trees (old vs new version of the same file) and produce
/// a summary of structural changes.
///
/// Comparison is done at the "named entity" level — functions, containers,
/// imports — rather than at individual expression nodes. This matches the
/// granularity at which dllb stores AST documents.
pub fn diff_trees(old: &MetaNode, new: &MetaNode) -> DiffSummary {
    let old_entities = collect_named_entities(old);
    let new_entities = collect_named_entities(new);

    let mut changes = Vec::new();

    // Find removed and modified entities
    for old_ent in &old_entities {
        match new_entities.iter().find(|n| n.key == old_ent.key) {
            Some(new_ent) => {
                if !children_equal(old_ent.node, new_ent.node) {
                    changes.push(AstChange {
                        kind: ChangeKind::Modified,
                        node_type: new_ent.node.node_type,
                        name: new_ent.key.clone(),
                        line: get_line(new_ent.node),
                    });
                }
            }
            None => {
                // Check if it was renamed (same structure, different name)
                let renamed_to = find_rename(old_ent, &new_entities, &old_entities);
                if let Some(new_name) = renamed_to {
                    changes.push(AstChange {
                        kind: ChangeKind::Renamed,
                        node_type: old_ent.node.node_type,
                        name: format!("{} -> {}", old_ent.key, new_name),
                        line: None,
                    });
                } else {
                    changes.push(AstChange {
                        kind: ChangeKind::Removed,
                        node_type: old_ent.node.node_type,
                        name: old_ent.key.clone(),
                        line: None,
                    });
                }
            }
        }
    }

    // Find added entities (present in new but not in old, and not a rename target)
    let rename_targets: Vec<String> = changes
        .iter()
        .filter(|c| c.kind == ChangeKind::Renamed)
        .filter_map(|c| c.name.split(" -> ").nth(1).map(String::from))
        .collect();

    for new_ent in &new_entities {
        let in_old = old_entities.iter().any(|o| o.key == new_ent.key);
        let is_rename_target = rename_targets.iter().any(|t| t == &new_ent.key);
        if !in_old && !is_rename_target {
            changes.push(AstChange {
                kind: ChangeKind::Added,
                node_type: new_ent.node.node_type,
                name: new_ent.key.clone(),
                line: get_line(new_ent.node),
            });
        }
    }

    let added = changes
        .iter()
        .filter(|c| c.kind == ChangeKind::Added)
        .count();
    let removed = changes
        .iter()
        .filter(|c| c.kind == ChangeKind::Removed)
        .count();
    let modified = changes
        .iter()
        .filter(|c| c.kind == ChangeKind::Modified)
        .count();
    let renamed = changes
        .iter()
        .filter(|c| c.kind == ChangeKind::Renamed)
        .count();

    DiffSummary {
        changes,
        added,
        removed,
        modified,
        renamed,
    }
}

// -- Internal helpers --------------------------------------------------------

struct NamedEntity<'a> {
    key: String,
    node: &'a MetaNode,
}

fn collect_named_entities(root: &MetaNode) -> Vec<NamedEntity<'_>> {
    let mut entities = Vec::new();
    collect_recursive(root, &mut entities);
    entities
}

fn collect_recursive<'a>(node: &'a MetaNode, out: &mut Vec<NamedEntity<'a>>) {
    match node.node_type {
        NodeType::FunctionDef => {
            if let Some(name) = node.get_meta_str("name") {
                let arity = match node.get_meta("arity") {
                    Some(MetaValue::Int(a)) => *a,
                    _ => 0,
                };
                out.push(NamedEntity {
                    key: format!("fn::{}/{}", name, arity),
                    node,
                });
            }
        }
        NodeType::Container => {
            if let Some(name) = node.get_meta_str("name") {
                out.push(NamedEntity {
                    key: format!("container::{}", name),
                    node,
                });
            }
        }
        NodeType::Import => {
            if let Some(source) = node.get_meta_str("source") {
                out.push(NamedEntity {
                    key: format!("import::{}", source),
                    node,
                });
            }
        }
        _ => {}
    }

    for child in node.child_nodes() {
        collect_recursive(child, out);
    }
}

fn children_equal(a: &MetaNode, b: &MetaNode) -> bool {
    if a.node_type != b.node_type {
        return false;
    }
    match (&a.children, &b.children) {
        (NodeChildren::Value(va), NodeChildren::Value(vb)) => va == vb,
        (NodeChildren::Nodes(ca), NodeChildren::Nodes(cb)) => {
            if ca.len() != cb.len() {
                return false;
            }
            ca.iter().zip(cb.iter()).all(|(a, b)| children_equal(a, b))
        }
        _ => false,
    }
}

fn find_rename(
    old_ent: &NamedEntity<'_>,
    new_entities: &[NamedEntity<'_>],
    old_entities: &[NamedEntity<'_>],
) -> Option<String> {
    // Only attempt rename detection for functions and containers
    if !matches!(
        old_ent.node.node_type,
        NodeType::FunctionDef | NodeType::Container
    ) {
        return None;
    }

    for new_ent in new_entities {
        // Skip if this new entity is already matched to an old one
        if old_entities.iter().any(|o| o.key == new_ent.key) {
            continue;
        }
        // Must be same node type
        if new_ent.node.node_type != old_ent.node.node_type {
            continue;
        }
        // Children must be structurally equal (body didn't change, only name)
        if children_equal(old_ent.node, new_ent.node) {
            return Some(new_ent.key.clone());
        }
    }
    None
}

fn get_line(node: &MetaNode) -> Option<i64> {
    match node.get_meta("line") {
        Some(MetaValue::Int(l)) => Some(*l),
        _ => match node.get_meta("line_start") {
            Some(MetaValue::Int(l)) => Some(*l),
            _ => None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_fn(name: &str, arity: i64, body: Vec<MetaNode>) -> MetaNode {
        MetaNode::composite(
            NodeType::FunctionDef,
            vec![
                ("name".into(), MetaValue::String(name.into())),
                ("arity".into(), MetaValue::Int(arity)),
                ("line".into(), MetaValue::Int(1)),
            ],
            body,
        )
    }

    fn make_module(name: &str, children: Vec<MetaNode>) -> MetaNode {
        MetaNode::composite(
            NodeType::Container,
            vec![
                ("name".into(), MetaValue::String(name.into())),
                ("container_type".into(), MetaValue::Atom("module".into())),
            ],
            children,
        )
    }

    fn make_import(source: &str) -> MetaNode {
        MetaNode::composite(
            NodeType::Import,
            vec![("source".into(), MetaValue::Atom(source.into()))],
            vec![],
        )
    }

    fn make_body_node(op: &str) -> MetaNode {
        MetaNode::composite(
            NodeType::BinaryOp,
            vec![("operator".into(), MetaValue::Atom(op.into()))],
            vec![
                MetaNode::leaf(NodeType::Variable, vec![], MetaValue::String("x".into())),
                MetaNode::leaf(NodeType::Variable, vec![], MetaValue::String("y".into())),
            ],
        )
    }

    #[test]
    fn identical_trees_no_changes() {
        let tree = make_module(
            "App",
            vec![
                make_fn("add", 2, vec![make_body_node("+")]),
                make_fn("sub", 2, vec![make_body_node("-")]),
            ],
        );
        let diff = diff_trees(&tree, &tree);
        assert!(diff.is_empty());
        assert_eq!(diff.added, 0);
        assert_eq!(diff.removed, 0);
        assert_eq!(diff.modified, 0);
    }

    #[test]
    fn added_function() {
        let old = make_module("App", vec![make_fn("add", 2, vec![make_body_node("+")])]);
        let new = make_module(
            "App",
            vec![
                make_fn("add", 2, vec![make_body_node("+")]),
                make_fn("mul", 2, vec![make_body_node("*")]),
            ],
        );
        let diff = diff_trees(&old, &new);
        assert_eq!(diff.added, 1);
        assert_eq!(diff.removed, 0);
        assert!(diff.stale_entities().contains(&"fn::mul/2"));
    }

    #[test]
    fn removed_function() {
        let old = make_module(
            "App",
            vec![
                make_fn("add", 2, vec![make_body_node("+")]),
                make_fn("sub", 2, vec![make_body_node("-")]),
            ],
        );
        let new = make_module("App", vec![make_fn("add", 2, vec![make_body_node("+")])]);
        let diff = diff_trees(&old, &new);
        assert_eq!(diff.removed, 1);
        assert!(diff.removed_entities().contains(&"fn::sub/2"));
    }

    #[test]
    fn modified_function_body() {
        let old = make_module("App", vec![make_fn("calc", 2, vec![make_body_node("+")])]);
        let new = make_module("App", vec![make_fn("calc", 2, vec![make_body_node("*")])]);
        let diff = diff_trees(&old, &new);
        assert_eq!(diff.modified, 1);
        assert!(diff.stale_entities().contains(&"fn::calc/2"));
    }

    #[test]
    fn renamed_function() {
        let body = vec![make_body_node("+")];
        let old = make_module("App", vec![make_fn("add", 2, body.clone())]);
        let new = make_module("App", vec![make_fn("sum", 2, body)]);
        let diff = diff_trees(&old, &new);
        assert_eq!(diff.renamed, 1);
        assert_eq!(diff.added, 0);
        assert_eq!(diff.removed, 0);
    }

    #[test]
    fn import_added_and_removed() {
        let old = make_module(
            "App",
            vec![make_import("GenServer"), make_fn("start", 0, vec![])],
        );
        let new = make_module(
            "App",
            vec![make_import("Supervisor"), make_fn("start", 0, vec![])],
        );
        let diff = diff_trees(&old, &new);
        assert_eq!(diff.added, 1); // Supervisor
        assert_eq!(diff.removed, 1); // GenServer
    }

    #[test]
    fn stale_entities_excludes_removed() {
        let old = make_module(
            "App",
            vec![make_fn("old_fn", 0, vec![]), make_fn("keep", 0, vec![])],
        );
        let new = make_module(
            "App",
            vec![make_fn("keep", 0, vec![]), make_fn("new_fn", 0, vec![])],
        );
        let diff = diff_trees(&old, &new);
        let stale = diff.stale_entities();
        assert!(stale.contains(&"fn::new_fn/0"));
        assert!(!stale.contains(&"fn::old_fn/0"));
        assert!(!stale.contains(&"fn::keep/0"));
    }
}
