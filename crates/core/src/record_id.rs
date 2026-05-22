//! Composite record identifiers: `table:id`.

use serde::{Deserialize, Serialize};
use std::fmt;

/// A composite record identifier in the form `table:id`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RecordId {
    pub table: String,
    pub id: String,
}

impl RecordId {
    pub fn new(table: impl Into<String>, id: impl Into<String>) -> Self {
        Self {
            table: table.into(),
            id: id.into(),
        }
    }

    /// Generate a new RecordId with a random UUID.
    pub fn generate(table: impl Into<String>) -> Self {
        Self {
            table: table.into(),
            id: uuid::Uuid::new_v4().to_string(),
        }
    }
}

impl fmt::Display for RecordId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.table, self.id)
    }
}

impl std::str::FromStr for RecordId {
    type Err = crate::Error;

    fn from_str(s: &str) -> crate::Result<Self> {
        let (table, id) = s
            .split_once(':')
            .ok_or_else(|| crate::Error::Other(format!("invalid record id: {s}")))?;
        Ok(Self::new(table, id))
    }
}
