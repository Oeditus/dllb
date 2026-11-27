//! Dynamically-typed values storable in dllb.
//!
//! [`Value`] is the universal currency of the database: every document field,
//! edge property, and query result is a `Value`. The enum covers scalars
//! (bool, int, float, string), collections (array, object), references
//! (`RecordId`), binary blobs, and dense vector embeddings.
//!
//! # Conversion
//!
//! Common Rust types convert into `Value` via `From` impls:
//!
//! ```
//! use dllb_core::Value;
//!
//! let v: Value = 42i64.into();
//! let v: Value = "hello".to_string().into();
//! let v: Value = vec![0.1f32, 0.2, 0.3].into(); // Vector embedding
//! ```

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::RecordId;

/// A dynamically-typed value in the database.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Value {
    None,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Bytes(Vec<u8>),
    Array(Vec<Value>),
    Object(BTreeMap<String, Value>),
    RecordId(RecordId),
    /// A dense vector embedding (f32 per dimension).
    Vector(Vec<f32>),
}

impl Value {
    /// Returns `true` if this value is `None`.
    pub fn is_none(&self) -> bool {
        matches!(self, Value::None)
    }

    /// Try to interpret this value as an f32 vector slice.
    pub fn as_vector(&self) -> Option<&[f32]> {
        match self {
            Value::Vector(v) => Some(v.as_slice()),
            _ => None,
        }
    }
}

impl From<String> for Value {
    fn from(s: String) -> Self {
        Value::String(s)
    }
}

impl From<i64> for Value {
    fn from(n: i64) -> Self {
        Value::Int(n)
    }
}

impl From<f64> for Value {
    fn from(n: f64) -> Self {
        Value::Float(n)
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value::Bool(b)
    }
}

impl From<Vec<f32>> for Value {
    fn from(v: Vec<f32>) -> Self {
        Value::Vector(v)
    }
}

/// Human-readable rendering of a value.
///
/// This is the *display* form, not the wire form: `serde` still encodes the
/// enum with its variant tags because that is the MessagePack storage format.
/// Use this for CLI output, log lines, and error messages.
impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::None => f.write_str("NONE"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Int(n) => write!(f, "{n}"),
            Value::Float(x) => write!(f, "{x}"),
            Value::String(s) => write!(f, "{s}"),
            Value::Bytes(b) => write!(f, "<{} bytes>", b.len()),
            Value::RecordId(id) => write!(f, "{id}"),
            Value::Array(items) => {
                f.write_str("[")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{item}")?;
                }
                f.write_str("]")
            }
            Value::Object(map) => {
                f.write_str("{")?;
                for (i, (k, v)) in map.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{k}: {v}")?;
                }
                f.write_str("}")
            }
            Value::Vector(v) => {
                f.write_str("[")?;
                for (i, x) in v.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{x}")?;
                }
                f.write_str("]")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_renders_scalars() {
        assert_eq!(Value::None.to_string(), "NONE");
        assert_eq!(Value::Bool(true).to_string(), "true");
        assert_eq!(Value::Int(-7).to_string(), "-7");
        assert_eq!(Value::Float(1.5).to_string(), "1.5");
        assert_eq!(Value::String("hi".into()).to_string(), "hi");
        assert_eq!(Value::Bytes(vec![0; 3]).to_string(), "<3 bytes>");
        assert_eq!(
            Value::RecordId(RecordId::new("user", "alice")).to_string(),
            "user:alice"
        );
    }

    #[test]
    fn display_renders_collections() {
        assert_eq!(
            Value::Array(vec![Value::Int(1), Value::String("a".into())]).to_string(),
            "[1, a]"
        );
        assert_eq!(Value::Array(vec![]).to_string(), "[]");
        assert_eq!(Value::Vector(vec![0.5, 1.0]).to_string(), "[0.5, 1]");

        let mut map = BTreeMap::new();
        map.insert("k".to_string(), Value::Int(1));
        assert_eq!(Value::Object(map).to_string(), "{k: 1}");
    }

    #[test]
    fn display_nests() {
        let inner = Value::Array(vec![Value::Int(1)]);
        assert_eq!(Value::Array(vec![inner]).to_string(), "[[1]]");
    }
}
