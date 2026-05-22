//! Composite record identifiers in the form `table:id`.
//!
//! Every record in dllb is uniquely identified by a [`RecordId`] combining
//! the table name and a per-record identifier. IDs can be user-supplied
//! strings or auto-generated UUIDs.
//!
//! # Examples
//!
//! ```
//! use dllb_core::RecordId;
//!
//! let explicit = RecordId::new("user", "alice");
//! assert_eq!(explicit.to_string(), "user:alice");
//!
//! let generated = RecordId::generate("user");
//! assert!(generated.to_string().starts_with("user:"));
//!
//! let parsed: RecordId = "product:widget".parse().unwrap();
//! assert_eq!(parsed.table, "product");
//! ```

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
