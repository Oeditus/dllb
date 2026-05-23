//! Integration tests for the code intelligence layer.
//!
//! Demonstrates the full pipeline: build a MetaAST tree, extract
//! structural info, store as dllb documents, and create graph edges.

use dllb_code_intel::extract::{extract_calls, extract_functions, extract_imports};
use dllb_code_intel::meta_ast::*;
use dllb_code_intel::schemas;
use dllb_core::{RecordId, Value};
use dllb_document::{Collection, Document};
use dllb_graph::{Edge, EdgeStore, Traversal};
use dllb_storage::db::DllbStorage;

fn temp_storage() -> (tempfile::TempDir, DllbStorage) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("codeintel.redb");
    let storage = DllbStorage::open(&path).unwrap();
    (dir, storage)
}

/// Build a MetaAST for a module with two functions and an import.
fn sample_module() -> MetaNode {
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
fn full_pipeline_metaast_to_documents_and_edges() {
    let (_dir, storage) = temp_storage();
    let tree = sample_module();

    // -- Extract structural info --
    let functions = extract_functions(&tree);
    let imports = extract_imports(&tree);
    let calls = extract_calls(&tree);

    assert_eq!(functions.len(), 2);
    assert_eq!(imports.len(), 1);
    assert_eq!(calls, vec!["IO.puts"]);

    // -- Store as dllb documents --
    let coll = Collection::new(&storage, "ns", "db", "ast_node");

    // Store the module container.
    let module_name = tree.get_meta_str("name").unwrap();
    coll.create(
        Document::new(RecordId::new("ast_node", &format!("mod_{module_name}")))
            .with_field("name", Value::String(module_name.into()))
            .with_field("kind", Value::String("container".into()))
            .with_field("language", Value::String("elixir".into())),
    )
    .unwrap();

    // Store each function.
    for func in &functions {
        let id = format!("fn_{}_{}", module_name, func.name);
        coll.create(
            Document::new(RecordId::new("ast_node", &id))
                .with_field("name", Value::String(func.name.clone()))
                .with_field("kind", Value::String("function_def".into()))
                .with_field("language", Value::String("elixir".into()))
                .with_field(
                    "signature",
                    Value::String(format!("{}({})", func.name, func.params.join(", "))),
                ),
        )
        .unwrap();
    }

    assert_eq!(coll.count().unwrap(), 3); // 1 module + 2 functions

    // -- Create graph edges --
    // Module contains functions.
    let edges = EdgeStore::new(&storage, "ns", "db", schemas::EDGE_CONTAINS);
    for func in &functions {
        let src_id = format!("mod_{module_name}");
        let dst_id = format!("fn_{}_{}", module_name, func.name);
        edges
            .relate(&Edge::new(&src_id, schemas::EDGE_CONTAINS, &dst_id))
            .unwrap();
    }

    // Module imports GenServer.
    let import_edges = EdgeStore::new(&storage, "ns", "db", schemas::EDGE_IMPORTS);
    for imp in &imports {
        import_edges
            .relate(&Edge::new(
                &format!("mod_{module_name}"),
                schemas::EDGE_IMPORTS,
                &format!("ext_{}", imp.source),
            ))
            .unwrap();
    }

    // greet calls IO.puts.
    let call_edges = EdgeStore::new(&storage, "ns", "db", schemas::EDGE_CALLS);
    call_edges
        .relate(&Edge::new(
            "fn_MyApp_greet",
            schemas::EDGE_CALLS,
            "ext_IO.puts",
        ))
        .unwrap();

    // -- Verify graph traversal --
    let t = Traversal::new(&edges);
    let module_children = t.outgoing(&format!("mod_{module_name}")).unwrap();
    assert_eq!(module_children.len(), 2);

    let t_calls = Traversal::new(&call_edges);
    let greet_calls = t_calls.outgoing("fn_MyApp_greet").unwrap();
    assert_eq!(greet_calls.len(), 1);
    assert_eq!(greet_calls[0].dst, "ext_IO.puts");

    let t_imports = Traversal::new(&import_edges);
    let module_imports = t_imports.outgoing(&format!("mod_{module_name}")).unwrap();
    assert_eq!(module_imports.len(), 1);
    assert_eq!(module_imports[0].dst, "ext_GenServer");
}

#[test]
fn schema_validation_with_ast_node_schema() {
    let schema = schemas::ast_node_schema();

    // Verify the schema can be used with a Collection.
    let (_dir, storage) = temp_storage();
    let coll = Collection::new(&storage, "ns", "db", "ast_node").with_schema(schema);

    // Valid document.
    coll.create(
        Document::new(RecordId::new("ast_node", "fn_add"))
            .with_field("name", Value::String("add".into()))
            .with_field("kind", Value::String("function_def".into()))
            .with_field("language", Value::String("elixir".into())),
    )
    .unwrap();

    // Missing required field "name" -- should fail.
    let result = coll.create(
        Document::new(RecordId::new("ast_node", "bad"))
            .with_field("kind", Value::String("function_def".into()))
            .with_field("language", Value::String("elixir".into())),
    );
    assert!(result.is_err());
}
