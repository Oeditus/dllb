//! # dllb-query
//!
//! Query engine for dllb.
//!
//! Provides a SQL-like declarative language (inspired by SurrealQL) with:
//!
//! - **Parser**: tokenizer and recursive-descent parser producing an AST
//! - **Planner**: converts the AST into a logical plan (scan, filter,
//!   project, join, sort, limit)
//! - **Optimizer**: index selection (B-tree, full-text, HNSW, graph
//!   traversal), predicate pushdown, limit pushdown
//! - **Executor**: streaming, batched execution with cross-model support
//!   -- a single query can combine document filters, graph traversals,
//!   full-text matches, and vector KNN in one statement
//!
//! Hybrid ranking uses reciprocal rank fusion (RRF) or weighted linear
//! combination of vector/text/graph scores.
