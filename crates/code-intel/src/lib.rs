//! # dllb-code-intel
//!
//! AST/MetaAST code intelligence layer for dllb.
//!
//! Rust-native companion to the Elixir [metastatic](https://github.com/Oeditus/metastatic)
//! library. Provides MetaAST types faithful to the METAST_SPEC.md
//! specification, covering all 38 node types across four meta-modeling
//! layers (M2.1 Core, M2.2 Extended, M2.2s Structural, M2.3 Native).
//!
//! - [`meta_ast`]: `MetaNode`, `NodeType` (38 variants), `MetaValue`,
//!   `NodeChildren`, `Layer` -- the complete MetaAST type system
//! - [`tokenizer`]: code-aware tokenizer splitting camelCase/snake_case,
//!   stripping noise keywords, suitable for Tantivy full-text indexing
//! - [`schemas`]: predefined `TableDefinition` for AST nodes with 11
//!   fields (including vector embedding fields), and 6 edge type constants
//! - [`extract`]: tree walking, function/import/variable/call extraction

pub mod extract;
pub mod meta_ast;
pub mod schemas;
pub mod tokenizer;

pub use extract::{FunctionInfo, ImportInfo};
pub use meta_ast::{Layer, MetaNode, MetaValue, NodeChildren, NodeType};
pub use schemas::{ALL_EDGE_TYPES, ast_node_schema};
pub use tokenizer::code_tokenize;
