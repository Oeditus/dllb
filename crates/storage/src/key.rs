//! Binary key encoding for the unified KV keyspace.
//!
//! Type tags:
//! - `*` (0x2A) -- document record
//! - `~` (0x7E) -- graph edge pointer
//! - `+` (0x2B) -- index entry
//! - `!` (0x21) -- metadata

/// Type tag bytes used in the key encoding scheme.
pub mod tag {
    pub const DOCUMENT: u8 = b'*';
    pub const GRAPH_EDGE: u8 = b'~';
    pub const INDEX: u8 = b'+';
    pub const METADATA: u8 = b'!';
}

/// Separator byte between key segments.
pub const SEPARATOR: u8 = 0x00;
