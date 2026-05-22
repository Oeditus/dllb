//! Schema definitions: table schemas, field types, index definitions.

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
