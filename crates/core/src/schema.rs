//! Schema definitions for dllb tables and fields.
//!
//! dllb supports two schema modes:
//!
//! - **Schemaless**: any fields can be stored; no validation on write.
//! - **Schemafull**: all fields must be declared with types; writes are
//!   validated against the schema.
//!
//! The [`FieldType::Vector`] variant carries a fixed dimensionality,
//! enabling the database to validate embedding dimensions at write time.

use serde::{Deserialize, Serialize};

/// The type of a field in a schemafull table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldType {
    String,
    Int,
    Float,
    Bool,
    Bytes,
    Array,
    Object,
    RecordId,
    /// A dense vector with fixed dimensionality.
    Vector(usize),
}

/// Whether a table enforces a schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SchemaMode {
    /// No schema enforcement; arbitrary fields allowed.
    Schemaless,
    /// All fields must be declared with types.
    Schemafull,
}

/// A field definition within a schemafull table.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldDefinition {
    pub name: String,
    pub field_type: FieldType,
    pub required: bool,
}

/// A table definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableDefinition {
    pub name: String,
    pub schema_mode: SchemaMode,
    pub fields: Vec<FieldDefinition>,
}
