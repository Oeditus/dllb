//! Binary key encoding for the unified KV keyspace.
//!
//! All data models share a single sorted keyspace. Keys are structured as:
//!
//! ```text
//! [namespace][0x00][database][0x00][table][0x00][tag][remainder...]
//! ```
//!
//! Type tags distinguish data models:
//! - `!` (0x21) -- metadata (schema, table definitions)
//! - `*` (0x2A) -- document record
//! - `+` (0x2B) -- index entry (B-tree, HNSW, full-text)
//! - `~` (0x7E) -- graph edge pointer
//!
//! String segments must not contain `0x00` (the separator byte).

use dllb_core::{Error, Result};

/// Type tag bytes used in the key encoding scheme.
///
/// Byte values chosen so natural sort order is:
/// metadata (0x21) < document (0x2A) < index (0x2B) < graph_edge (0x7E).
pub mod tag {
    pub const METADATA: u8 = b'!'; // 0x21
    pub const DOCUMENT: u8 = b'*'; // 0x2A
    pub const INDEX: u8 = b'+'; // 0x2B
    pub const GRAPH_EDGE: u8 = b'~'; // 0x7E
}

/// Separator byte between key segments (null terminator).
pub const SEPARATOR: u8 = 0x00;

// ---------------------------------------------------------------------------
// KeyBuilder
// ---------------------------------------------------------------------------

/// Fluent builder for constructing binary keys segment by segment.
#[derive(Debug, Clone)]
pub struct KeyBuilder {
    buf: Vec<u8>,
}

impl KeyBuilder {
    pub fn new() -> Self {
        Self {
            buf: Vec::with_capacity(64),
        }
    }

    /// Append a namespace segment followed by the separator.
    pub fn namespace(self, ns: &str) -> Self {
        self.segment(ns.as_bytes())
    }

    /// Append a database segment followed by the separator.
    pub fn database(self, db: &str) -> Self {
        self.segment(db.as_bytes())
    }

    /// Append a table segment followed by the separator.
    pub fn table(self, table: &str) -> Self {
        self.segment(table.as_bytes())
    }

    /// Append a single type-tag byte.
    pub fn tag(mut self, tag: u8) -> Self {
        self.buf.push(tag);
        self
    }

    /// Append `data` followed by the separator byte.
    pub fn segment(mut self, data: &[u8]) -> Self {
        debug_assert!(
            !data.contains(&SEPARATOR),
            "key segment must not contain 0x00"
        );
        self.buf.extend_from_slice(data);
        self.buf.push(SEPARATOR);
        self
    }

    /// Append raw bytes without a trailing separator.
    pub fn raw(mut self, data: &[u8]) -> Self {
        self.buf.extend_from_slice(data);
        self
    }

    /// Consume the builder and return the encoded key.
    pub fn build(self) -> Vec<u8> {
        self.buf
    }
}

impl Default for KeyBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Convenience constructors
// ---------------------------------------------------------------------------

/// Build a document record key: `ns\0db\0table\0*id`.
pub fn document_key(ns: &str, db: &str, table: &str, id: &str) -> Vec<u8> {
    KeyBuilder::new()
        .namespace(ns)
        .database(db)
        .table(table)
        .tag(tag::DOCUMENT)
        .raw(id.as_bytes())
        .build()
}

/// Build a graph edge key (outgoing): `ns\0db\0table\0~src\0edge_type\0dst`.
pub fn graph_edge_key(
    ns: &str,
    db: &str,
    table: &str,
    src: &str,
    edge_type: &str,
    dst: &str,
) -> Vec<u8> {
    KeyBuilder::new()
        .namespace(ns)
        .database(db)
        .table(table)
        .tag(tag::GRAPH_EDGE)
        .segment(src.as_bytes())
        .segment(edge_type.as_bytes())
        .raw(dst.as_bytes())
        .build()
}

/// Build an index entry key: `ns\0db\0table\0+index_name\0field_value\0id`.
pub fn index_key(
    ns: &str,
    db: &str,
    table: &str,
    index_name: &str,
    field_val: &[u8],
    id: &str,
) -> Vec<u8> {
    KeyBuilder::new()
        .namespace(ns)
        .database(db)
        .table(table)
        .tag(tag::INDEX)
        .segment(index_name.as_bytes())
        .segment(field_val)
        .raw(id.as_bytes())
        .build()
}

/// Build a metadata key: `ns\0db\0table\0!`.
pub fn metadata_key(ns: &str, db: &str, table: &str) -> Vec<u8> {
    KeyBuilder::new()
        .namespace(ns)
        .database(db)
        .table(table)
        .tag(tag::METADATA)
        .build()
}

// ---------------------------------------------------------------------------
// Prefix helpers
// ---------------------------------------------------------------------------

/// Build a prefix for scanning all entries of a given tag within a table.
pub fn table_prefix(ns: &str, db: &str, table: &str, tag_byte: u8) -> Vec<u8> {
    KeyBuilder::new()
        .namespace(ns)
        .database(db)
        .table(table)
        .tag(tag_byte)
        .build()
}

/// Compute the exclusive end key for a prefix scan.
///
/// Increments the last byte; propagates carry if `0xFF`.
/// Returns an empty vec if the prefix is all `0xFF` (scan to end).
pub fn prefix_end(prefix: &[u8]) -> Vec<u8> {
    let mut end = prefix.to_vec();
    while let Some(last) = end.last_mut() {
        if *last < 0xFF {
            *last += 1;
            return end;
        }
        end.pop();
    }
    end
}

// ---------------------------------------------------------------------------
// KeyParser
// ---------------------------------------------------------------------------

/// Decoded segments of a key.
#[derive(Debug, PartialEq, Eq)]
pub struct KeyParts<'a> {
    pub namespace: &'a [u8],
    pub database: &'a [u8],
    pub table: &'a [u8],
    pub tag: u8,
    pub remainder: &'a [u8],
}

/// Parse a raw key into its constituent segments.
pub fn parse_key(key: &[u8]) -> Result<KeyParts<'_>> {
    let mut parts = key.splitn(4, |&b| b == SEPARATOR);

    let namespace = parts
        .next()
        .ok_or_else(|| Error::Storage("key too short: missing namespace".into()))?;
    let database = parts
        .next()
        .ok_or_else(|| Error::Storage("key too short: missing database".into()))?;
    let table = parts
        .next()
        .ok_or_else(|| Error::Storage("key too short: missing table".into()))?;
    let after_table = parts
        .next()
        .ok_or_else(|| Error::Storage("key too short: missing tag".into()))?;

    if after_table.is_empty() {
        return Err(Error::Storage("key too short: empty after table".into()));
    }

    Ok(KeyParts {
        namespace,
        database,
        table,
        tag: after_table[0],
        remainder: &after_table[1..],
    })
}

/// Validate that a string segment does not contain the separator byte.
pub fn validate_segment(s: &str) -> Result<()> {
    if s.as_bytes().contains(&SEPARATOR) {
        Err(Error::Other(format!(
            "key segment must not contain 0x00: {:?}",
            s
        )))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_key_roundtrip() {
        let key = document_key("ns", "db", "user", "alice");
        let parts = parse_key(&key).unwrap();
        assert_eq!(parts.namespace, b"ns");
        assert_eq!(parts.database, b"db");
        assert_eq!(parts.table, b"user");
        assert_eq!(parts.tag, tag::DOCUMENT);
        assert_eq!(parts.remainder, b"alice");
    }

    #[test]
    fn graph_edge_key_roundtrip() {
        let key = graph_edge_key("ns", "db", "edge", "alice", "knows", "bob");
        let parts = parse_key(&key).unwrap();
        assert_eq!(parts.tag, tag::GRAPH_EDGE);
        let segs: Vec<&[u8]> = parts.remainder.splitn(3, |&b| b == SEPARATOR).collect();
        assert_eq!(segs, vec![b"alice".as_slice(), b"knows", b"bob"]);
    }

    #[test]
    fn metadata_key_roundtrip() {
        let key = metadata_key("ns", "db", "user");
        let parts = parse_key(&key).unwrap();
        assert_eq!(parts.tag, tag::METADATA);
        assert!(parts.remainder.is_empty());
    }

    #[test]
    fn prefix_end_basic() {
        assert_eq!(prefix_end(b"abc"), b"abd");
        assert_eq!(prefix_end(b"ab\xff"), b"ac");
        assert_eq!(prefix_end(b"\xff\xff\xff"), b"");
        assert_eq!(prefix_end(b""), b"");
    }

    #[test]
    fn tag_sort_order() {
        assert!(tag::METADATA < tag::DOCUMENT);
        assert!(tag::DOCUMENT < tag::INDEX);
        assert!(tag::INDEX < tag::GRAPH_EDGE);
    }

    #[test]
    fn validate_segment_rejects_null() {
        assert!(validate_segment("hello").is_ok());
        assert!(validate_segment("hel\0lo").is_err());
    }

    #[test]
    fn parse_key_rejects_short_keys() {
        assert!(parse_key(b"").is_err());
        assert!(parse_key(b"ns").is_err());
        assert!(parse_key(b"ns\x00db").is_err());
    }
}
