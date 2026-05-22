//! # dllb-core
//!
//! Shared types for the dllb multi-model database: RecordId, Value, Error, Schema.

pub mod error;
pub mod record_id;
pub mod schema;
pub mod value;

pub use error::{Error, Result};
pub use record_id::RecordId;
pub use value::Value;
