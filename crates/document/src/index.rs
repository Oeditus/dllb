//! Secondary B-tree indexes for documents.
//!
//! Index entries are stored as KV pairs alongside documents:
//! - Key: `ns\0db\0table\0+index_name\0encoded_value\0record_id`
//! - Value: empty `[]`
//!
//! Field values are encoded to bytes that preserve sort order so that
//! range scans over index entries return results in field-value order.

use serde::{Deserialize, Serialize};

use dllb_core::{Error, Result, Value};
use dllb_storage::db::DllbStorage;
use dllb_storage::key;

use crate::Document;

/// Definition of a secondary index on a collection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexDefinition {
    /// Index name (used in the KV key).
    pub name: String,
    /// Field names to index (single-field indexes for now).
    pub fields: Vec<String>,
    /// If true, no two documents may have the same indexed value.
    pub unique: bool,
}

// ---------------------------------------------------------------------------
// Sort-preserving value encoding
// ---------------------------------------------------------------------------

/// Encode a [`Value`] into bytes that preserve sort order.
///
/// Supported types: String, Int, Float, Bool.
/// Returns an error for unsupported types.
pub fn encode_index_value(value: &Value) -> Result<Vec<u8>> {
    match value {
        Value::String(s) => Ok(s.as_bytes().to_vec()),
        Value::Int(n) => Ok(encode_i64(*n)),
        Value::Float(f) => Ok(encode_f64(*f)),
        Value::Bool(b) => Ok(vec![if *b { 0x01 } else { 0x00 }]),
        other => Err(Error::Index(format!("type not indexable: {other:?}"))),
    }
}

/// Encode an i64 to 8 bytes preserving sort order.
///
/// Flip the sign bit so that negative numbers sort before positive.
fn encode_i64(n: i64) -> Vec<u8> {
    let unsigned = (n as u64) ^ (1u64 << 63);
    unsigned.to_be_bytes().to_vec()
}

/// Encode an f64 to 8 bytes preserving sort order.
///
/// For positive floats: flip the sign bit.
/// For negative floats: flip all bits.
fn encode_f64(f: f64) -> Vec<u8> {
    let bits = f.to_bits();
    let encoded = if f.is_sign_negative() {
        !bits // flip all bits for negatives
    } else {
        bits ^ (1u64 << 63) // flip sign bit for positives
    };
    encoded.to_be_bytes().to_vec()
}

// ---------------------------------------------------------------------------
// Index entry construction
// ---------------------------------------------------------------------------

/// Build all index KV entries for a document given a set of index definitions.
///
/// Returns a list of `(key, value)` pairs where value is always empty.
pub fn build_index_entries(
    doc: &Document,
    ns: &str,
    db: &str,
    table: &str,
    indexes: &[IndexDefinition],
) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
    let mut entries = Vec::new();
    let id = &doc.id.id;

    for idx in indexes {
        // For now: single-field indexes only (use first field).
        let field_name = idx
            .fields
            .first()
            .ok_or_else(|| Error::Index("index has no fields".into()))?;

        if let Some(value) = doc.fields.get(field_name)
            && !value.is_none()
        {
            let encoded = encode_index_value(value)?;
            // Build key manually because encoded value may contain 0x00
            // (e.g., big-endian i64). Use raw() to avoid the segment
            // assertion, and separate value from record ID with a 0xFF
            // marker byte.
            let k = key::KeyBuilder::new()
                .namespace(ns)
                .database(db)
                .table(table)
                .tag(key::tag::INDEX)
                .segment(idx.name.as_bytes())
                .raw(&encoded)
                .raw(&[0xFF]) // marker separating value from ID
                .raw(id.as_bytes())
                .build();
            entries.push((k, vec![]));
        }
    }

    Ok(entries)
}

/// Find all record IDs matching a given index value.
pub fn find_by_index(
    storage: &DllbStorage,
    ns: &str,
    db: &str,
    table: &str,
    index_name: &str,
    value: &Value,
) -> Result<Vec<String>> {
    let encoded = encode_index_value(value)?;
    // Build a prefix that matches the index name + encoded value + 0xFF marker.
    let prefix = key::KeyBuilder::new()
        .namespace(ns)
        .database(db)
        .table(table)
        .tag(key::tag::INDEX)
        .segment(index_name.as_bytes())
        .raw(&encoded)
        .raw(&[0xFF]) // same marker used in build_index_entries
        .build();

    let entries = storage.prefix_scan(&prefix)?;
    let mut ids = Vec::new();
    for (k, _) in entries {
        // The key ends with: ...encoded_value 0xFF record_id
        // We know the prefix length, so the record ID is everything after it.
        let id_bytes = &k[prefix.len()..];
        if let Ok(id) = std::str::from_utf8(id_bytes) {
            ids.push(id.to_string());
        }
    }
    Ok(ids)
}

// ---------------------------------------------------------------------------
// Index catalog persistence
// ---------------------------------------------------------------------------
//
// Index definitions are persisted in the metadata keyspace so that they
// survive restarts and are visible to every connection. Each definition is
// a MessagePack-serialized `IndexDefinition` stored under
// `ns\0db\0table\0!idx\0<index_name>`.

/// Build the catalog `(key, value)` pair for an index definition.
///
/// Exposed so callers can include catalog persistence in a larger atomic
/// `write_batch` (e.g. `DEFINE INDEX` writing the definition together with
/// all backfilled entries in one transaction).
pub fn index_definition_kv(
    ns: &str,
    db: &str,
    table: &str,
    def: &IndexDefinition,
) -> Result<(Vec<u8>, Vec<u8>)> {
    let key = key::index_catalog_key(ns, db, table, &def.name);
    let bytes = rmp_serde::to_vec(def).map_err(|e| Error::Serialization(e.to_string()))?;
    Ok((key, bytes))
}

/// Persist (create or overwrite) an index definition in the catalog.
pub fn save_index_definition(
    storage: &DllbStorage,
    ns: &str,
    db: &str,
    table: &str,
    def: &IndexDefinition,
) -> Result<()> {
    let (key, bytes) = index_definition_kv(ns, db, table, def)?;
    storage.put(&key, &bytes)
}

/// Remove an index definition from the catalog. Returns `true` if it existed.
pub fn remove_index_definition(
    storage: &DllbStorage,
    ns: &str,
    db: &str,
    table: &str,
    index_name: &str,
) -> Result<bool> {
    let key = key::index_catalog_key(ns, db, table, index_name);
    let existed = storage.contains(&key)?;
    if existed {
        storage.delete(&key)?;
    }
    Ok(existed)
}

/// Load all index definitions registered for a table.
pub fn load_index_definitions(
    storage: &DllbStorage,
    ns: &str,
    db: &str,
    table: &str,
) -> Result<Vec<IndexDefinition>> {
    let prefix = key::index_catalog_prefix(ns, db, table);
    let entries = storage.prefix_scan(&prefix)?;
    let mut defs = Vec::with_capacity(entries.len());
    for (_k, v) in entries {
        let def: IndexDefinition =
            rmp_serde::from_slice(&v).map_err(|e| Error::Serialization(e.to_string()))?;
        defs.push(def);
    }
    Ok(defs)
}

/// Check if a unique index value already exists for a different record.
pub fn check_unique_constraint(
    storage: &DllbStorage,
    ns: &str,
    db: &str,
    table: &str,
    index: &IndexDefinition,
    value: &Value,
    exclude_id: &str,
) -> Result<()> {
    if !index.unique {
        return Ok(());
    }
    let ids = find_by_index(storage, ns, db, table, &index.name, value)?;
    for id in &ids {
        if id != exclude_id {
            return Err(Error::Schema(format!(
                "unique constraint violated on index '{}': value already exists for record '{}'",
                index.name, id
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn i64_sort_order() {
        let neg = encode_i64(-100);
        let zero = encode_i64(0);
        let pos = encode_i64(100);
        assert!(neg < zero);
        assert!(zero < pos);
    }

    #[test]
    fn f64_sort_order() {
        let neg = encode_f64(-1.5);
        let zero = encode_f64(0.0);
        let pos = encode_f64(1.5);
        assert!(neg < zero);
        assert!(zero < pos);
    }

    #[test]
    fn bool_sort_order() {
        let f = encode_index_value(&Value::Bool(false)).unwrap();
        let t = encode_index_value(&Value::Bool(true)).unwrap();
        assert!(f < t);
    }

    #[test]
    fn string_encoding() {
        let a = encode_index_value(&Value::String("apple".into())).unwrap();
        let b = encode_index_value(&Value::String("banana".into())).unwrap();
        assert!(a < b);
    }

    #[test]
    fn unsupported_type_errors() {
        assert!(encode_index_value(&Value::Array(vec![])).is_err());
    }
}
