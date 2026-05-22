//! # dllb-query
//!
//! Query engine for dllb.
//!
//! Provides a SQL-like declarative language (inspired by SurrealQL) with:
//!
//! - **Tokenizer**: splits input into keyword/ident/literal/symbol tokens
//! - **Parser**: hand-written recursive descent producing an AST
//! - **Executor**: maps AST nodes to crate API calls (Collection, EdgeStore)
//!
//! Supported statements: CREATE, SELECT (with WHERE), DELETE, RELATE.
//! The `QueryExecutor::run()` method provides a single-call parse+execute.

pub mod ast;
pub mod executor;
pub mod format;
pub mod parser;
pub mod tokenizer;

pub use executor::{QueryExecutor, QueryResult};
pub use format::{format_error, format_result};
pub use parser::parse;
