//! Unified error types for the dllb database.
//!
//! Every crate in the workspace returns `dllb_core::Result<T>`, which uses
//! [`Error`] as the error type. External errors (redb, Tantivy, serialization)
//! are mapped into the appropriate variant at crate boundaries.

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("storage error: {0}")]
    Storage(String),

    #[error("serialization error: {0}")]
    Serialization(String),

    #[error("record not found: {0}")]
    NotFound(String),

    #[error("schema violation: {0}")]
    Schema(String),

    #[error("transaction conflict: {0}")]
    Conflict(String),

    #[error("query error: {0}")]
    Query(String),

    #[error("index error: {0}")]
    Index(String),

    #[error("dimension mismatch: expected {expected}, got {actual}")]
    DimensionMismatch { expected: usize, actual: usize },

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
