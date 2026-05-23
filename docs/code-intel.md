# Code Intelligence Layer

This document covers the `dllb-code-intel` crate: MetaAST types,
code-aware tokenization, predefined schemas, and extraction utilities.

## Overview

`dllb-code-intel` is the Rust-native companion to the Elixir
[metastatic](https://github.com/Oeditus/metastatic) library. It provides
the type system and utilities for storing and querying program structure
in dllb.

Source code maps onto dllb's multi-model primitives:
- **Documents**: AST nodes (functions, modules, classes) with embeddings
- **Graph edges**: call graph, containment, imports, type references
- **Full-text**: source code indexed with code-aware tokenization
- **Vectors**: CodeBERT/StarCoder embeddings for semantic similarity

## MetaAST Types

The `meta_ast` module mirrors the MetaAST specification (METAST_SPEC.md)
with 38 node types across four layers:

| Layer | Count | Examples |
|-------|-------|---------|
| M2.1 Core | 18 | Literal, Variable, BinaryOp, FunctionCall, Block, Map |
| M2.2 Extended | 10 | Loop, Lambda, PatternMatch, Comprehension |
| M2.2s Structural | 8 | Container, FunctionDef, Import, TypeAnnotation |
| M2.3 Native | 1 | LanguageSpecific (escape hatch) |
| Special | 1 | Wildcard |

### MetaNode

```rust
let node = MetaNode::composite(
    NodeType::FunctionDef,
    vec![
        ("name".into(), MetaValue::String("add".into())),
        ("arity".into(), MetaValue::Int(2)),
        ("visibility".into(), MetaValue::Atom("public".into())),
    ],
    vec![body_node],
);

node.get_meta_str("name")   // Some("add")
node.layer()                 // Layer::Structural
node.node_type.atom_name()   // "function_def"
```

### Interop with metastatic

`NodeType::atom_name()` returns the Elixir atom name (e.g., `"function_def"`),
and `NodeType::from_atom("function_def")` parses it back. This enables
serialization/deserialization between the Elixir and Rust representations.

## Code-Aware Tokenizer

The `tokenizer` module splits source code into meaningful tokens for
full-text indexing:

```rust
use dllb_code_intel::code_tokenize;

code_tokenize("parseJSONData")    // ["parse", "json", "data"]
code_tokenize("parse_json_data")  // ["parse", "json", "data"]
code_tokenize("async fn fetch_user_profile(userId: String)")
// ["fetch", "user", "profile", "id", "string"]
// ("async", "fn" stripped as noise)
```

Features:
- camelCase splitting (`parseJSON` -> `parse`, `json`)
- snake_case splitting (`parse_json` -> `parse`, `json`)
- Lowercasing
- Noise keyword stripping (70+ keywords across languages)
- Single-character token removal

## Predefined Schemas

The `schemas` module provides factory functions for the standard
AST storage pattern:

```rust
let schema = ast_node_schema();
// 11 fields: name, kind, language, file_path, line_start, line_end,
// source_text, signature, docstring, source_embedding(768), structure_embedding(384)
```

### Edge Type Constants

| Constant | Value | Meaning |
|----------|-------|---------|
| `EDGE_CALLS` | `"calls"` | Function calls another function |
| `EDGE_CONTAINS` | `"contains"` | Module contains function |
| `EDGE_RETURNS` | `"returns"` | Function returns a type |
| `EDGE_IMPORTS` | `"imports"` | Module imports another |
| `EDGE_OVERRIDES` | `"overrides"` | Function overrides parent |
| `EDGE_EXEMPLIFIES` | `"exemplifies"` | Node exemplifies a pattern |

## Extraction Utilities

The `extract` module walks MetaAST trees and extracts structured info:

```rust
let tree = build_meta_ast_from_source(...);

let functions = extract_functions(&tree);  // Vec<FunctionInfo>
let imports = extract_imports(&tree);      // Vec<ImportInfo>
let variables = extract_variables(&tree);  // HashSet<String>
let calls = extract_calls(&tree);          // Vec<String>
let count = node_count(&tree);             // usize
let max_depth = depth(&tree);              // usize
```

These enable the pipeline:
1. Parse source code into MetaAST (via metastatic Elixir adapter)
2. Extract structural info in Rust
3. Store as dllb documents and graph edges
4. Index for full-text and vector search

## Testing

```bash
cargo test -p dllb-code-intel
```

18 tests: 16 unit (node types, layer classification, MetaNode construction,
tokenizer splits, schema validation, extraction from sample trees) +
2 integration (full pipeline MetaAST -> documents + edges, schema validation).
