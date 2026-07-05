//! # dllb-code-intel
//!
//! AST/MetaAST code intelligence layer for dllb.
//!
//! Rust-native companion to the Elixir [metastatic](https://github.com/Oeditus/metastatic)
//! library. Provides MetaAST types faithful to the METAST_SPEC.md
//! specification, covering all 46 node types across four meta-modeling
//! layers (M2.1 Core, M2.2 Extended, M2.2s Structural, M2.3 Native).
//!
//! - [`meta_ast`]: `MetaNode`, `NodeType` (46 variants), `MetaValue`,
//!   `NodeChildren`, `Layer` -- the complete MetaAST type system
//! - [`tokenizer`]: code-aware tokenizer splitting camelCase/snake_case,
//!   stripping noise keywords, suitable for Tantivy full-text indexing
//! - [`schemas`]: predefined `TableDefinition` for AST nodes with 11
//!   fields (including vector embedding fields), and 6 edge type constants
//! - [`extract`]: tree walking, function/import/variable/call extraction

pub mod diff;
pub mod extract;
pub mod ingest;
pub mod meta_ast;
pub mod query_helpers;
pub mod schemas;
pub mod similarity;
pub mod tokenizer;

pub use diff::{AstChange, ChangeKind, DiffSummary, diff_trees};
pub use extract::{FunctionInfo, ImportInfo};
pub use ingest::{AstDocument, AstEdge, IngestBatch, IngestStats};
pub use meta_ast::{Layer, MetaNode, MetaValue, NodeChildren, NodeType};
pub use query_helpers::{
    ancestors, call_targets, complexity_estimate, containing_container, containing_function,
    find_by_name, find_by_type, find_parent, find_siblings, scope_at,
};
pub use schemas::{ALL_EDGE_TYPES, ast_node_schema};
pub use similarity::{
    ClonePair, find_clones, structural_similarity, subtree_hash, tree_fingerprint,
};
pub use tokenizer::code_tokenize;
