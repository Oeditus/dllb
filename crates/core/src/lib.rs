//! # dllb-core
//!
//! Foundation types shared by every crate in the dllb workspace.
//!
//! This crate contains no business logic -- it defines the vocabulary that
//! all other crates speak:
//!
//! - [`Error`] / [`Result`] -- unified error type for storage, serialization,
//!   schema violations, transaction conflicts, query errors, and index errors.
//! - [`RecordId`] -- composite `table:id` identifier (e.g., `user:alice`).
//! - [`Value`] -- dynamically-typed enum covering all storable types, including
//!   `Vector(Vec<f32>)` for dense embeddings.
//! - [`schema`] -- `FieldType`, `SchemaMode`, `FieldDefinition`, `TableDefinition`.

pub mod error;
pub mod record_id;
pub mod schema;
pub mod value;

pub use error::{Error, Result};
pub use record_id::RecordId;
pub use value::Value;
