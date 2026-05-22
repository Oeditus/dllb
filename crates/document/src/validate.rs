//! Schema validation for documents.
//!
//! In **schemafull** mode, every document is validated against a
//! [`TableDefinition`] before being written. Required fields must be
//! present, field types must match, and no unknown fields are allowed.
//!
//! In **schemaless** mode, validation is a no-op.

use dllb_core::schema::{FieldType, SchemaMode, TableDefinition};
use dllb_core::{Error, Result, Value};

use crate::Document;

/// Validate a document against a table schema.
///
/// Returns `Ok(())` if the schema is `Schemaless` or if the document
/// conforms to the `Schemafull` definition.
pub fn validate_document(doc: &Document, schema: &TableDefinition) -> Result<()> {
    if schema.schema_mode == SchemaMode::Schemaless {
        return Ok(());
    }

    // Check required fields are present.
    for field_def in &schema.fields {
        if field_def.required && !doc.fields.contains_key(&field_def.name) {
            return Err(Error::Schema(format!(
                "missing required field: '{}'",
                field_def.name
            )));
        }
    }

    // Check no unknown fields.
    for field_name in doc.fields.keys() {
        if !schema.fields.iter().any(|f| f.name == *field_name) {
            return Err(Error::Schema(format!("unknown field: '{field_name}'")));
        }
    }

    // Type-check each present field.
    for field_def in &schema.fields {
        if let Some(value) = doc.fields.get(&field_def.name) {
            validate_field(value, &field_def.field_type, &field_def.name)?;
        }
    }

    Ok(())
}

/// Validate that a single value matches the expected field type.
pub fn validate_field(value: &Value, expected: &FieldType, field_name: &str) -> Result<()> {
    match (value, expected) {
        (Value::None, _) => Ok(()), // None is always valid (field not set)
        (Value::String(_), FieldType::String) => Ok(()),
        (Value::Int(_), FieldType::Int) => Ok(()),
        (Value::Float(_), FieldType::Float) => Ok(()),
        (Value::Bool(_), FieldType::Bool) => Ok(()),
        (Value::Bytes(_), FieldType::Bytes) => Ok(()),
        (Value::Array(_), FieldType::Array) => Ok(()),
        (Value::Object(_), FieldType::Object) => Ok(()),
        (Value::RecordId(_), FieldType::RecordId) => Ok(()),
        (Value::Vector(v), FieldType::Vector(dim)) => {
            if v.len() == *dim {
                Ok(())
            } else {
                Err(Error::DimensionMismatch {
                    expected: *dim,
                    actual: v.len(),
                })
            }
        }
        _ => Err(Error::Schema(format!(
            "field '{field_name}': expected {expected:?}, got {value:?}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dllb_core::RecordId;
    use dllb_core::schema::{FieldDefinition, SchemaMode, TableDefinition};

    fn user_schema() -> TableDefinition {
        TableDefinition {
            name: "user".into(),
            schema_mode: SchemaMode::Schemafull,
            fields: vec![
                FieldDefinition {
                    name: "name".into(),
                    field_type: FieldType::String,
                    required: true,
                },
                FieldDefinition {
                    name: "age".into(),
                    field_type: FieldType::Int,
                    required: false,
                },
                FieldDefinition {
                    name: "embedding".into(),
                    field_type: FieldType::Vector(3),
                    required: false,
                },
            ],
        }
    }

    #[test]
    fn valid_document_passes() {
        let doc = Document::new(RecordId::new("user", "a"))
            .with_field("name", Value::String("Alice".into()))
            .with_field("age", Value::Int(30));
        assert!(validate_document(&doc, &user_schema()).is_ok());
    }

    #[test]
    fn missing_required_field_fails() {
        let doc = Document::new(RecordId::new("user", "a")).with_field("age", Value::Int(30));
        let err = validate_document(&doc, &user_schema()).unwrap_err();
        assert!(err.to_string().contains("missing required field"));
    }

    #[test]
    fn wrong_type_fails() {
        let doc = Document::new(RecordId::new("user", "a")).with_field("name", Value::Int(42)); // should be String
        let err = validate_document(&doc, &user_schema()).unwrap_err();
        assert!(err.to_string().contains("expected"));
    }

    #[test]
    fn unknown_field_fails() {
        let doc = Document::new(RecordId::new("user", "a"))
            .with_field("name", Value::String("Alice".into()))
            .with_field("email", Value::String("a@b.c".into()));
        let err = validate_document(&doc, &user_schema()).unwrap_err();
        assert!(err.to_string().contains("unknown field"));
    }

    #[test]
    fn vector_dimension_mismatch_fails() {
        let doc = Document::new(RecordId::new("user", "a"))
            .with_field("name", Value::String("Alice".into()))
            .with_field("embedding", Value::Vector(vec![0.1, 0.2])); // expects 3
        let err = validate_document(&doc, &user_schema()).unwrap_err();
        assert!(err.to_string().contains("dimension mismatch"));
    }

    #[test]
    fn vector_correct_dimension_passes() {
        let doc = Document::new(RecordId::new("user", "a"))
            .with_field("name", Value::String("Alice".into()))
            .with_field("embedding", Value::Vector(vec![0.1, 0.2, 0.3]));
        assert!(validate_document(&doc, &user_schema()).is_ok());
    }

    #[test]
    fn schemaless_always_passes() {
        let schema = TableDefinition {
            name: "anything".into(),
            schema_mode: SchemaMode::Schemaless,
            fields: vec![],
        };
        let doc = Document::new(RecordId::new("x", "y"))
            .with_field("whatever", Value::String("fine".into()));
        assert!(validate_document(&doc, &schema).is_ok());
    }
}
