//! # dllb-code-intel
//!
//! AST/MetaAST code intelligence layer for dllb.
//!
//! This crate makes dllb a first-class store for program structure and
//! code embeddings. It provides:
//!
//! - Predefined schemas for AST nodes (function, class, module, trait)
//!   with fields for source text, signature, file path, and embeddings
//! - A code-aware tokenizer for Tantivy that splits on camelCase and
//!   snake_case boundaries (e.g., `parseJSON` -> `parse`, `json`)
//! - MetaAST: cross-repository structural pattern recognition, storing
//!   recurring patterns (builder, retry, observer) as documents and
//!   graph edges
//!
//! Source code maps onto the multi-model primitives:
//! - Documents: AST nodes with source_embedding and structure_embedding
//! - Graph edges: call graph, containment, imports, type references
//! - Full-text: source code indexed with the code-aware tokenizer
//! - Vectors: CodeBERT/StarCoder embeddings for semantic similarity
