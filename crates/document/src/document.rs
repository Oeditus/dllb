//! The [`Document`] type: a record with a [`RecordId`] and arbitrary fields.
//!
//! # Examples
//!
//! ```
//! use dllb_core::{RecordId, Value};
//! use dllb_document::Document;
//!
//! let doc = Document::new(RecordId::new("user", "alice"))
//!     .with_field("name", Value::String("Alice".into()))
//!     .with_field("age", Value::Int(30));
//!
//! assert_eq!(doc.get("name"), Some(&Value::String("Alice".into())));
//! assert_eq!(doc.id.to_string(), "user:alice");
//! ```

use std::collections::BTreeMap;

use dllb_core::{RecordId, Value};

/// A document record in dllb.
///
/// Each document has a unique [`RecordId`] and a set of named fields.
/// Fields are dynamically typed via [`Value`].
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    pub id: RecordId,
    pub fields: BTreeMap<String, Value>,
}

impl Document {
    /// Create a new empty document with the given ID.
    pub fn new(id: RecordId) -> Self {
        Self {
            id,
            fields: BTreeMap::new(),
        }
    }

    /// Builder method: add a field and return self.
    pub fn with_field(mut self, name: impl Into<String>, value: Value) -> Self {
        self.fields.insert(name.into(), value);
        self
    }

    /// Get a field value by name.
    pub fn get(&self, field: &str) -> Option<&Value> {
        self.fields.get(field)
    }

    /// Set a field value (insert or overwrite).
    pub fn set(&mut self, field: impl Into<String>, value: Value) {
        self.fields.insert(field.into(), value);
    }

    /// Remove a field, returning its value if it existed.
    pub fn remove(&mut self, field: &str) -> Option<Value> {
        self.fields.remove(field)
    }

    /// Borrow the fields map.
    pub fn fields(&self) -> &BTreeMap<String, Value> {
        &self.fields
    }

    /// Consume the document and return the fields map.
    pub fn into_fields(self) -> BTreeMap<String, Value> {
        self.fields
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_and_accessors() {
        let mut doc = Document::new(RecordId::new("user", "alice"))
            .with_field("name", Value::String("Alice".into()))
            .with_field("age", Value::Int(30));

        assert_eq!(doc.get("name"), Some(&Value::String("Alice".into())));
        assert_eq!(doc.get("age"), Some(&Value::Int(30)));
        assert_eq!(doc.get("missing"), None);

        doc.set("age", Value::Int(31));
        assert_eq!(doc.get("age"), Some(&Value::Int(31)));

        let removed = doc.remove("name");
        assert_eq!(removed, Some(Value::String("Alice".into())));
        assert_eq!(doc.get("name"), None);
    }
}
