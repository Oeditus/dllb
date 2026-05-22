//! The [`Edge`] type: a directed relationship between two vertices.
//!
//! Edges carry a source vertex ID, destination vertex ID, an edge type
//! label, and optional properties (arbitrary key-value fields).

use std::collections::BTreeMap;

use dllb_core::Value;

/// A directed graph edge with optional properties.
#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    pub src: String,
    pub edge_type: String,
    pub dst: String,
    pub properties: BTreeMap<String, Value>,
}

impl Edge {
    /// Create a new edge with no properties.
    pub fn new(src: &str, edge_type: &str, dst: &str) -> Self {
        Self {
            src: src.into(),
            edge_type: edge_type.into(),
            dst: dst.into(),
            properties: BTreeMap::new(),
        }
    }

    /// Builder: add a property and return self.
    pub fn with_property(mut self, name: &str, value: Value) -> Self {
        self.properties.insert(name.into(), value);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder() {
        let e = Edge::new("alice", "knows", "bob")
            .with_property("since", Value::Int(2020))
            .with_property("weight", Value::Float(0.9));
        assert_eq!(e.src, "alice");
        assert_eq!(e.edge_type, "knows");
        assert_eq!(e.dst, "bob");
        assert_eq!(e.properties.len(), 2);
    }
}
