//! Batch ingestion pipeline for loading MetaAST data into dllb.
//!
//! Bridges ragex's analysis output (MetaAST trees) to dllb's storage layer
//! by producing batches of document and edge operations ready for bulk insertion.

use crate::extract::{extract_calls, extract_functions, extract_imports, walk};
use crate::meta_ast::{MetaNode, MetaValue, NodeType};
use crate::schemas::{EDGE_CALLS, EDGE_CONTAINS, EDGE_IMPORTS};

/// A document to be inserted into the ast_node table.
#[derive(Debug, Clone)]
pub struct AstDocument {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub language: String,
    pub file_path: String,
    pub line_start: Option<i64>,
    pub line_end: Option<i64>,
    pub source_text: Option<String>,
    pub signature: Option<String>,
    pub docstring: Option<String>,
}

/// A graph edge to be created between AST nodes.
#[derive(Debug, Clone)]
pub struct AstEdge {
    pub from_id: String,
    pub edge_type: String,
    pub to_id: String,
}

/// Complete batch of operations to insert a file's AST into dllb.
#[derive(Debug, Clone)]
pub struct IngestBatch {
    pub documents: Vec<AstDocument>,
    pub edges: Vec<AstEdge>,
    pub file_path: String,
    pub language: String,
    pub stats: IngestStats,
}

/// Statistics about the ingestion batch.
#[derive(Debug, Clone, Default)]
pub struct IngestStats {
    pub containers: usize,
    pub functions: usize,
    pub imports: usize,
    pub calls: usize,
    pub edges: usize,
}

/// Generate a deterministic ID for a container node.
fn container_id(file_path: &str, kind: &str, name: &str) -> String {
    format!("{}::{}::{}", file_path, kind, name)
}

/// Generate a deterministic ID for a function node.
fn function_id(file_path: &str, name: &str, arity: usize) -> String {
    format!("{}::{}/{}", file_path, name, arity)
}

/// Generate a deterministic ID for an import target.
fn import_id(source: &str) -> String {
    format!("import::{}", source)
}

/// Generate a deterministic ID for a call target.
fn call_target_id(target_name: &str) -> String {
    format!("call_target::{}", target_name)
}

/// Escape single quotes in a string value for dllb query generation.
fn escape_str(s: &str) -> String {
    s.replace('\'', "''")
}

/// Walk the MetaAST tree and produce a batch of dllb operations.
///
/// Produces:
/// - A document for each Container node (module/class) and FunctionDef node
/// - Containment edges: container -> contains -> function
/// - Import edges: container -> imports -> (stable ID from import source)
/// - Call edges: function -> calls -> (stable ID from call target name)
pub fn prepare_batch(root: &MetaNode, file_path: &str, language: &str) -> IngestBatch {
    let mut documents = Vec::new();
    let mut edges = Vec::new();
    let mut stats = IngestStats::default();

    // Process container nodes at the top level and nested
    walk(root, &mut |node| {
        if node.node_type == NodeType::Container {
            let name = node.get_meta_str("name").unwrap_or("anonymous").to_string();
            let kind = node
                .get_meta_str("container_type")
                .unwrap_or("container")
                .to_string();
            let line_start = match node.get_meta("line") {
                Some(MetaValue::Int(l)) => Some(*l),
                _ => None,
            };
            let source_text = node.get_meta_str("source").map(String::from);
            let docstring = node.get_meta_str("doc").map(String::from);

            let cid = container_id(file_path, &kind, &name);

            documents.push(AstDocument {
                id: cid.clone(),
                name: name.clone(),
                kind: kind.clone(),
                language: language.to_string(),
                file_path: file_path.to_string(),
                line_start,
                line_end: None,
                source_text,
                signature: None,
                docstring,
            });
            stats.containers += 1;

            // Extract functions within this container
            let functions = extract_functions(node);
            for func in &functions {
                let fid = function_id(file_path, &func.name, func.arity);

                documents.push(AstDocument {
                    id: fid.clone(),
                    name: func.name.clone(),
                    kind: "function_def".to_string(),
                    language: language.to_string(),
                    file_path: file_path.to_string(),
                    line_start: func.line,
                    line_end: None,
                    source_text: None,
                    signature: Some(format!(
                        "{}({}) [{}]",
                        func.name,
                        func.params.join(", "),
                        func.visibility
                    )),
                    docstring: None,
                });
                stats.functions += 1;

                // Containment edge
                edges.push(AstEdge {
                    from_id: cid.clone(),
                    edge_type: EDGE_CONTAINS.to_string(),
                    to_id: fid.clone(),
                });
                stats.edges += 1;
            }

            // Extract imports within this container
            let imports = extract_imports(node);
            for imp in &imports {
                let iid = import_id(&imp.source);
                edges.push(AstEdge {
                    from_id: cid.clone(),
                    edge_type: EDGE_IMPORTS.to_string(),
                    to_id: iid,
                });
                stats.imports += 1;
                stats.edges += 1;
            }

            // Extract calls from each function within this container
            for func in &functions {
                let fid = function_id(file_path, &func.name, func.arity);
                // Walk the function's subtree to find calls
                // We need to find the FunctionDef node for this function
                for child in node.child_nodes() {
                    if child.node_type == NodeType::FunctionDef
                        && child.get_meta_str("name") == Some(&func.name)
                    {
                        let calls = extract_calls(child);
                        for call in &calls {
                            let tid = call_target_id(call);
                            edges.push(AstEdge {
                                from_id: fid.clone(),
                                edge_type: EDGE_CALLS.to_string(),
                                to_id: tid,
                            });
                            stats.calls += 1;
                            stats.edges += 1;
                        }
                    }
                }
            }
        }
    });

    // Handle top-level FunctionDef nodes not inside a Container
    if root.node_type == NodeType::FunctionDef {
        let functions = extract_functions(root);
        for func in &functions {
            let fid = function_id(file_path, &func.name, func.arity);

            documents.push(AstDocument {
                id: fid.clone(),
                name: func.name.clone(),
                kind: "function_def".to_string(),
                language: language.to_string(),
                file_path: file_path.to_string(),
                line_start: func.line,
                line_end: None,
                source_text: None,
                signature: Some(format!(
                    "{}({}) [{}]",
                    func.name,
                    func.params.join(", "),
                    func.visibility
                )),
                docstring: None,
            });
            stats.functions += 1;

            let calls = extract_calls(root);
            for call in &calls {
                let tid = call_target_id(call);
                edges.push(AstEdge {
                    from_id: fid.clone(),
                    edge_type: EDGE_CALLS.to_string(),
                    to_id: tid,
                });
                stats.calls += 1;
                stats.edges += 1;
            }
        }
    }

    IngestBatch {
        documents,
        edges,
        file_path: file_path.to_string(),
        language: language.to_string(),
        stats,
    }
}

/// Convert an ingestion batch into dllb query language strings.
///
/// Produces:
/// - `BEGIN BATCH` header
/// - `CREATE ast_node:{id} SET ...` for each document
/// - `RELATE ast_node:{from} -> {edge_type} -> ast_node:{to}` for each edge
/// - `END BATCH` trailer
pub fn generate_dllb_queries(batch: &IngestBatch) -> Vec<String> {
    let mut queries = Vec::new();

    queries.push("BEGIN BATCH".to_string());

    for doc in &batch.documents {
        let mut sets = Vec::new();
        sets.push(format!("name = '{}'", escape_str(&doc.name)));
        sets.push(format!("kind = '{}'", escape_str(&doc.kind)));
        sets.push(format!("language = '{}'", escape_str(&doc.language)));
        sets.push(format!("file_path = '{}'", escape_str(&doc.file_path)));

        if let Some(line) = doc.line_start {
            sets.push(format!("line_start = {}", line));
        }
        if let Some(line) = doc.line_end {
            sets.push(format!("line_end = {}", line));
        }
        if let Some(ref text) = doc.source_text {
            sets.push(format!("source_text = '{}'", escape_str(text)));
        }
        if let Some(ref sig) = doc.signature {
            sets.push(format!("signature = '{}'", escape_str(sig)));
        }
        if let Some(ref ds) = doc.docstring {
            sets.push(format!("docstring = '{}'", escape_str(ds)));
        }

        queries.push(format!(
            "CREATE ast_node:{} SET {}",
            escape_str(&doc.id),
            sets.join(", ")
        ));
    }

    for edge in &batch.edges {
        queries.push(format!(
            "RELATE ast_node:{} -> {} -> ast_node:{}",
            escape_str(&edge.from_id),
            edge.edge_type,
            escape_str(&edge.to_id)
        ));
    }

    queries.push("END BATCH".to_string());

    queries
}

/// Combine multiple file batches into a single large batch for bulk loading.
pub fn merge_batches(batches: Vec<IngestBatch>) -> IngestBatch {
    let mut documents = Vec::new();
    let mut edges = Vec::new();
    let mut stats = IngestStats::default();
    let mut file_paths = Vec::new();
    let mut language = String::new();

    for batch in batches {
        documents.extend(batch.documents);
        edges.extend(batch.edges);
        stats.containers += batch.stats.containers;
        stats.functions += batch.stats.functions;
        stats.imports += batch.stats.imports;
        stats.calls += batch.stats.calls;
        stats.edges += batch.stats.edges;
        file_paths.push(batch.file_path);
        if language.is_empty() {
            language = batch.language;
        }
    }

    let file_path = if file_paths.len() == 1 {
        file_paths.into_iter().next().unwrap_or_default()
    } else {
        format!("[{} files]", file_paths.len())
    };

    IngestBatch {
        documents,
        edges,
        file_path,
        language,
        stats,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meta_ast::*;

    /// Build a sample MetaAST representing a module with 2 functions:
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
    fn prepare_batch_produces_correct_documents_and_edges() {
        let tree = sample_tree();
        let batch = prepare_batch(&tree, "lib/my_app.ex", "elixir");

        // Should have 3 documents: 1 container + 2 functions
        assert_eq!(batch.documents.len(), 3);

        // First document is the container
        assert_eq!(batch.documents[0].name, "MyApp");
        assert_eq!(batch.documents[0].kind, "module");
        assert_eq!(batch.documents[0].id, "lib/my_app.ex::module::MyApp");

        // Second document is the 'add' function
        assert_eq!(batch.documents[1].name, "add");
        assert_eq!(batch.documents[1].kind, "function_def");
        assert_eq!(batch.documents[1].id, "lib/my_app.ex::add/2");

        // Third document is the 'greet' function
        assert_eq!(batch.documents[2].name, "greet");
        assert_eq!(batch.documents[2].kind, "function_def");
        assert_eq!(batch.documents[2].id, "lib/my_app.ex::greet/1");

        // Check edges: 2 containment + 1 import + 1 call = 4
        assert_eq!(batch.edges.len(), 4);

        // Containment edges
        let contains_edges: Vec<_> = batch
            .edges
            .iter()
            .filter(|e| e.edge_type == "contains")
            .collect();
        assert_eq!(contains_edges.len(), 2);

        // Import edge
        let import_edges: Vec<_> = batch
            .edges
            .iter()
            .filter(|e| e.edge_type == "imports")
            .collect();
        assert_eq!(import_edges.len(), 1);
        assert_eq!(import_edges[0].to_id, "import::GenServer");

        // Call edge
        let call_edges: Vec<_> = batch
            .edges
            .iter()
            .filter(|e| e.edge_type == "calls")
            .collect();
        assert_eq!(call_edges.len(), 1);
        assert_eq!(call_edges[0].from_id, "lib/my_app.ex::greet/1");
        assert_eq!(call_edges[0].to_id, "call_target::IO.puts");

        // Stats
        assert_eq!(batch.stats.containers, 1);
        assert_eq!(batch.stats.functions, 2);
        assert_eq!(batch.stats.imports, 1);
        assert_eq!(batch.stats.calls, 1);
        assert_eq!(batch.stats.edges, 4);
    }

    #[test]
    fn generate_dllb_queries_produces_valid_batch() {
        let tree = sample_tree();
        let batch = prepare_batch(&tree, "lib/my_app.ex", "elixir");
        let queries = generate_dllb_queries(&batch);

        // Must start with BEGIN BATCH and end with END BATCH
        assert_eq!(queries.first().unwrap(), "BEGIN BATCH");
        assert_eq!(queries.last().unwrap(), "END BATCH");

        // Should have: 1 BEGIN + 3 CREATE + 4 RELATE + 1 END = 9
        assert_eq!(queries.len(), 9);

        // Check CREATE statements
        let creates: Vec<_> = queries.iter().filter(|q| q.starts_with("CREATE")).collect();
        assert_eq!(creates.len(), 3);
        assert!(creates[0].contains("ast_node:"));
        assert!(creates[0].contains("name = 'MyApp'"));
        assert!(creates[0].contains("kind = 'module'"));
        assert!(creates[0].contains("language = 'elixir'"));

        // Check RELATE statements
        let relates: Vec<_> = queries.iter().filter(|q| q.starts_with("RELATE")).collect();
        assert_eq!(relates.len(), 4);
        assert!(relates[0].contains("-> contains ->"));
    }

    #[test]
    fn merge_batches_combines_stats_correctly() {
        let tree = sample_tree();
        let batch1 = prepare_batch(&tree, "lib/my_app.ex", "elixir");
        let batch2 = prepare_batch(&tree, "lib/other.ex", "elixir");

        let b1_docs = batch1.documents.len();
        let b2_docs = batch2.documents.len();
        let b1_edges = batch1.edges.len();
        let b2_edges = batch2.edges.len();
        let b1_stats = batch1.stats.clone();
        let b2_stats = batch2.stats.clone();

        let merged = merge_batches(vec![batch1, batch2]);

        assert_eq!(merged.documents.len(), b1_docs + b2_docs);
        assert_eq!(merged.edges.len(), b1_edges + b2_edges);
        assert_eq!(
            merged.stats.containers,
            b1_stats.containers + b2_stats.containers
        );
        assert_eq!(
            merged.stats.functions,
            b1_stats.functions + b2_stats.functions
        );
        assert_eq!(merged.stats.imports, b1_stats.imports + b2_stats.imports);
        assert_eq!(merged.stats.calls, b1_stats.calls + b2_stats.calls);
        assert_eq!(merged.stats.edges, b1_stats.edges + b2_stats.edges);
        assert_eq!(merged.file_path, "[2 files]");
        assert_eq!(merged.language, "elixir");
    }

    #[test]
    fn ids_are_deterministic() {
        let tree = sample_tree();
        let batch1 = prepare_batch(&tree, "lib/my_app.ex", "elixir");
        let batch2 = prepare_batch(&tree, "lib/my_app.ex", "elixir");

        // Same input produces same IDs
        assert_eq!(batch1.documents.len(), batch2.documents.len());
        for (d1, d2) in batch1.documents.iter().zip(batch2.documents.iter()) {
            assert_eq!(d1.id, d2.id);
        }
        for (e1, e2) in batch1.edges.iter().zip(batch2.edges.iter()) {
            assert_eq!(e1.from_id, e2.from_id);
            assert_eq!(e1.to_id, e2.to_id);
            assert_eq!(e1.edge_type, e2.edge_type);
        }
    }

    #[test]
    fn escape_handles_single_quotes() {
        let tree = MetaNode::composite(
            NodeType::Container,
            vec![
                ("container_type".into(), MetaValue::Atom("module".into())),
                ("name".into(), MetaValue::String("O'Reilly".into())),
            ],
            vec![],
        );

        let batch = prepare_batch(&tree, "lib/test.ex", "elixir");
        let queries = generate_dllb_queries(&batch);

        // The name with a quote should be escaped
        let create = queries.iter().find(|q| q.starts_with("CREATE")).unwrap();
        assert!(create.contains("O''Reilly"));
    }
}
