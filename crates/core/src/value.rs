//! The `Value` enum represents any value storable in dllb.

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
