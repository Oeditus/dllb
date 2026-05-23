//! MetaAST tree walking and information extraction utilities.
//!
//! These functions traverse a MetaAST tree and extract structured
//! information suitable for populating dllb documents and graph edges.

use std::collections::HashSet;

use crate::meta_ast::{MetaNode, MetaValue, NodeType};

/// Information about a function definition extracted from a MetaAST tree.
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionInfo {
    pub name: String,
    pub arity: usize,
    pub params: Vec<String>,
    pub visibility: String,
    pub line: Option<i64>,
}

/// Information about an import directive extracted from a MetaAST tree.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportInfo {
    pub source: String,
    pub import_type: String,
    pub language: Option<String>,
}

/// Walk a MetaAST tree depth-first (pre-order), calling `f` on each node.
pub fn walk(node: &MetaNode, f: &mut impl FnMut(&MetaNode)) {
    f(node);
    for child in node.child_nodes() {
        walk(child, f);
    }
}

/// Count the total number of nodes in a MetaAST tree.
pub fn node_count(node: &MetaNode) -> usize {
    let mut count = 0;
    walk(node, &mut |_| count += 1);
    count
}

/// Compute the maximum depth of a MetaAST tree.
pub fn depth(node: &MetaNode) -> usize {
    fn depth_inner(node: &MetaNode) -> usize {
        let child_depth = node
            .child_nodes()
            .iter()
            .map(depth_inner)
            .max()
            .unwrap_or(0);
        1 + child_depth
    }
    depth_inner(node)
}

/// Extract all function definitions from a MetaAST tree.
pub fn extract_functions(node: &MetaNode) -> Vec<FunctionInfo> {
    let mut functions = Vec::new();
    walk(node, &mut |n| {
        if n.node_type == NodeType::FunctionDef {
            let name = n.get_meta_str("name").unwrap_or("").to_string();
            let visibility = n.get_meta_str("visibility").unwrap_or("public").to_string();
            let line = match n.get_meta("line") {
                Some(MetaValue::Int(l)) => Some(*l),
                _ => None,
            };
            let arity = match n.get_meta("arity") {
                Some(MetaValue::Int(a)) => *a as usize,
                _ => 0,
            };

            // Extract param names from the :params metadata.
            let params = match n.get_meta("params") {
                Some(MetaValue::List(param_list)) => param_list
                    .iter()
                    .filter_map(|p| match p {
                        MetaValue::String(s) => Some(s.clone()),
                        _ => None,
                    })
                    .collect(),
                _ => vec![],
            };

            functions.push(FunctionInfo {
                name,
                arity,
                params,
                visibility,
                line,
            });
        }
    });
    functions
}

/// Extract all import directives from a MetaAST tree.
pub fn extract_imports(node: &MetaNode) -> Vec<ImportInfo> {
    let mut imports = Vec::new();
    walk(node, &mut |n| {
        if n.node_type == NodeType::Import {
            let source = n.get_meta_str("source").unwrap_or("").to_string();
            let import_type = n
                .get_meta_str("import_type")
                .unwrap_or("import")
                .to_string();
            let language = n.get_meta_str("language").map(String::from);
            imports.push(ImportInfo {
                source,
                import_type,
                language,
            });
        }
    });
    imports
}

/// Extract all variable names from a MetaAST tree.
pub fn extract_variables(node: &MetaNode) -> HashSet<String> {
    let mut vars = HashSet::new();
    walk(node, &mut |n| {
        if n.node_type == NodeType::Variable
            && let Some(MetaValue::String(name)) = n.leaf_value()
        {
            vars.insert(name.clone());
        }
    });
    vars
}

/// Extract all function call names from a MetaAST tree.
pub fn extract_calls(node: &MetaNode) -> Vec<String> {
    let mut calls = Vec::new();
    walk(node, &mut |n| {
        if n.node_type == NodeType::FunctionCall
            && let Some(name) = n.get_meta_str("name")
        {
            calls.push(name.to_string());
        }
    });
    calls
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meta_ast::*;

    /// Build a sample MetaAST representing:
    /// ```
    /// module MyApp do
    ///   import GenServer
    ///   def add(x, y), do: x + y
    ///   def greet(name), do: IO.puts(name)
    /// end
    /// ```
    fn sample_tree() -> MetaNode {
        let import_node = MetaNode::composite(
            NodeType::Import,
            vec![
                ("source".into(), MetaValue::Atom("GenServer".into())),
                ("import_type".into(), MetaValue::Atom("use".into())),
                ("language".into(), MetaValue::Atom("elixir".into())),
            ],
            vec![],
        );

        let add_fn = MetaNode::composite(
            NodeType::FunctionDef,
            vec![
                ("name".into(), MetaValue::String("add".into())),
                ("arity".into(), MetaValue::Int(2)),
                ("visibility".into(), MetaValue::Atom("public".into())),
                (
                    "params".into(),
                    MetaValue::List(vec![
                        MetaValue::String("x".into()),
                        MetaValue::String("y".into()),
                    ]),
                ),
                ("line".into(), MetaValue::Int(3)),
            ],
            vec![MetaNode::composite(
                NodeType::BinaryOp,
                vec![
                    ("category".into(), MetaValue::Atom("arithmetic".into())),
                    ("operator".into(), MetaValue::Atom("+".into())),
                ],
                vec![
                    MetaNode::leaf(NodeType::Variable, vec![], MetaValue::String("x".into())),
                    MetaNode::leaf(NodeType::Variable, vec![], MetaValue::String("y".into())),
                ],
            )],
        );

        let greet_fn = MetaNode::composite(
            NodeType::FunctionDef,
            vec![
                ("name".into(), MetaValue::String("greet".into())),
                ("arity".into(), MetaValue::Int(1)),
                ("visibility".into(), MetaValue::Atom("public".into())),
                (
                    "params".into(),
                    MetaValue::List(vec![MetaValue::String("name".into())]),
                ),
                ("line".into(), MetaValue::Int(5)),
            ],
            vec![MetaNode::composite(
                NodeType::FunctionCall,
                vec![("name".into(), MetaValue::String("IO.puts".into()))],
                vec![MetaNode::leaf(
                    NodeType::Variable,
                    vec![],
                    MetaValue::String("name".into()),
                )],
            )],
        );

        MetaNode::composite(
            NodeType::Container,
            vec![
                ("container_type".into(), MetaValue::Atom("module".into())),
                ("name".into(), MetaValue::String("MyApp".into())),
                ("language".into(), MetaValue::Atom("elixir".into())),
            ],
            vec![import_node, add_fn, greet_fn],
        )
    }

    #[test]
    fn count_and_depth() {
        let tree = sample_tree();
        // container(1) + import(1) + fn_def(1) + binop(1) + var(1) + var(1)
        //   + fn_def(1) + call(1) + var(1) = 9
        assert_eq!(node_count(&tree), 9);
        assert!(depth(&tree) >= 3);
    }

    #[test]
    fn extract_fns() {
        let tree = sample_tree();
        let fns = extract_functions(&tree);
        assert_eq!(fns.len(), 2);
        assert_eq!(fns[0].name, "add");
        assert_eq!(fns[0].arity, 2);
        assert_eq!(fns[0].params, vec!["x", "y"]);
        assert_eq!(fns[1].name, "greet");
        assert_eq!(fns[1].arity, 1);
    }

    #[test]
    fn extract_imps() {
        let tree = sample_tree();
        let imps = extract_imports(&tree);
        assert_eq!(imps.len(), 1);
        assert_eq!(imps[0].source, "GenServer");
        assert_eq!(imps[0].import_type, "use");
    }

    #[test]
    fn extract_vars() {
        let tree = sample_tree();
        let vars = extract_variables(&tree);
        assert!(vars.contains("x"));
        assert!(vars.contains("y"));
        assert!(vars.contains("name"));
    }

    #[test]
    fn extract_call_names() {
        let tree = sample_tree();
        let calls = extract_calls(&tree);
        assert_eq!(calls, vec!["IO.puts"]);
    }
}
