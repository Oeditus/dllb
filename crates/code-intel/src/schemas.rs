//! Predefined schemas and edge type constants for storing AST data in dllb.
//!
//! Provides factory functions returning `TableDefinition` and edge type
//! constants for the standard AST storage pattern.

use dllb_core::schema::{FieldDefinition, FieldType, SchemaMode, TableDefinition};

/// Predefined edge type for function call relationships.
pub const EDGE_CALLS: &str = "calls";
/// Predefined edge type for containment (module contains function).
pub const EDGE_CONTAINS: &str = "contains";
/// Predefined edge type for return type relationships.
pub const EDGE_RETURNS: &str = "returns";
/// Predefined edge type for import/dependency relationships.
pub const EDGE_IMPORTS: &str = "imports";
/// Predefined edge type for override/implementation relationships.
pub const EDGE_OVERRIDES: &str = "overrides";
/// Predefined edge type for pattern exemplification.
pub const EDGE_EXEMPLIFIES: &str = "exemplifies";

/// All predefined edge types.
pub const ALL_EDGE_TYPES: &[&str] = &[
    EDGE_CALLS,
    EDGE_CONTAINS,
    EDGE_RETURNS,
    EDGE_IMPORTS,
    EDGE_OVERRIDES,
    EDGE_EXEMPLIFIES,
];

/// Create a schemafull table definition for AST nodes.
///
/// Fields:
/// - `name` (String, required) -- node name (function name, class name, etc.)
/// - `kind` (String, required) -- MetaAST node type atom name
/// - `language` (String, required) -- source language
/// - `file_path` (String) -- path to the source file
/// - `line_start` (Int) -- starting line number
/// - `line_end` (Int) -- ending line number
/// - `source_text` (String) -- raw source code text
/// - `signature` (String) -- function/method signature
/// - `docstring` (String) -- documentation string
/// - `source_embedding` (Vector(768)) -- embedding of source text
/// - `structure_embedding` (Vector(384)) -- embedding of AST structure
pub fn ast_node_schema() -> TableDefinition {
    TableDefinition {
        name: "ast_node".into(),
        schema_mode: SchemaMode::Schemafull,
        fields: vec![
            FieldDefinition {
                name: "name".into(),
                field_type: FieldType::String,
                required: true,
            },
            FieldDefinition {
                name: "kind".into(),
                field_type: FieldType::String,
                required: true,
            },
            FieldDefinition {
                name: "language".into(),
                field_type: FieldType::String,
                required: true,
            },
            FieldDefinition {
                name: "file_path".into(),
                field_type: FieldType::String,
                required: false,
            },
            FieldDefinition {
                name: "line_start".into(),
                field_type: FieldType::Int,
                required: false,
            },
            FieldDefinition {
                name: "line_end".into(),
                field_type: FieldType::Int,
                required: false,
            },
            FieldDefinition {
                name: "source_text".into(),
                field_type: FieldType::String,
                required: false,
            },
            FieldDefinition {
                name: "signature".into(),
                field_type: FieldType::String,
                required: false,
            },
            FieldDefinition {
                name: "docstring".into(),
                field_type: FieldType::String,
                required: false,
            },
            FieldDefinition {
                name: "source_embedding".into(),
                field_type: FieldType::Vector(768),
                required: false,
            },
            FieldDefinition {
                name: "structure_embedding".into(),
                field_type: FieldType::Vector(384),
                required: false,
            },
        ],
    }
}

/// The standard AST node kinds (MetaAST structural types that typically
/// become documents in the database).
pub const DOCUMENT_KINDS: &[&str] = &[
    "function_def",
    "container",
    "lambda",
    "property",
    "type_annotation",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_has_expected_fields() {
        let schema = ast_node_schema();
        assert_eq!(schema.name, "ast_node");
        assert_eq!(schema.schema_mode, SchemaMode::Schemafull);
        assert_eq!(schema.fields.len(), 11);

        let names: Vec<&str> = schema.fields.iter().map(|f| f.name.as_str()).collect();
        assert!(names.contains(&"name"));
        assert!(names.contains(&"kind"));
        assert!(names.contains(&"language"));
        assert!(names.contains(&"source_embedding"));
        assert!(names.contains(&"structure_embedding"));
    }

    #[test]
    fn edge_types_defined() {
        assert_eq!(ALL_EDGE_TYPES.len(), 6);
        assert!(ALL_EDGE_TYPES.contains(&"calls"));
        assert!(ALL_EDGE_TYPES.contains(&"contains"));
        assert!(ALL_EDGE_TYPES.contains(&"exemplifies"));
    }
}
