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
// Order-preserving, self-delimiting key part
// ---------------------------------------------------------------------------
//
// An index entry key is `region + key_part(value) + id`. The key part must be
// order-preserving (so range scans are correct even when one value is a byte
// prefix of another) and self-delimiting (so the trailing id can be recovered
// and never blurs value boundaries).
//
// Scheme: escape every 0x00 byte of the per-type encoding as `0x00 0xFF`, then
// append the terminator `0x00 0x01`. Since a real 0x00 is always followed by
// 0xFF, the terminator is unambiguous, and a shorter value sorts before a
// longer one that extends it (the terminator's 0x00 is the smallest byte).

/// Two-byte value terminator. Strictly less than any escaped data byte pair
/// beginning with `0x00` (`0x00 0xFF`), so equal-prefix values order by length.
const TERM: [u8; 2] = [0x00, 0x01];

/// Append `raw` to `out`, escaping `0x00` as `0x00 0xFF`.
fn escape_into(raw: &[u8], out: &mut Vec<u8>) {
    for &b in raw {
        out.push(b);
        if b == 0x00 {
            out.push(0xFF);
        }
    }
}

/// Escaped per-type encoding of `value` *without* the terminator.
///
/// Used to build range-scan bounds.
pub fn encode_index_value_escaped(value: &Value) -> Result<Vec<u8>> {
    let raw = encode_index_value(value)?;
    let mut out = Vec::with_capacity(raw.len() + 2);
    escape_into(&raw, &mut out);
    Ok(out)
}

/// Full order-preserving key part for `value`: escaped encoding + terminator.
pub fn encode_index_key_part(value: &Value) -> Result<Vec<u8>> {
    let mut out = encode_index_value_escaped(value)?;
    out.extend_from_slice(&TERM);
    Ok(out)
}

/// Recover the record id from an index entry key, given the length of the
/// region prefix (`ns\0db\0table\0+index_name\0`). Walks the escaped value to
/// its `0x00 0x01` terminator; the id is everything after it.
fn id_from_entry(key: &[u8], region_len: usize) -> Option<String> {
    let mut i = region_len;
    while i + 1 < key.len() {
        if key[i] == 0x00 {
            match key[i + 1] {
                0xFF => i += 2, // escaped 0x00, skip the pair
                0x01 => {
                    // terminator: id follows
                    return std::str::from_utf8(&key[i + 2..]).ok().map(String::from);
                }
                _ => return None, // malformed entry
            }
        } else {
            i += 1;
        }
    }
    None
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
            // The key part is an order-preserving, self-delimiting encoding of
            // the value (escaped + terminated), so range scans are correct and
            // the trailing id is unambiguous.
            let key_part = encode_index_key_part(value)?;
            let k = key::KeyBuilder::new()
                .namespace(ns)
                .database(db)
                .table(table)
                .tag(key::tag::INDEX)
                .segment(idx.name.as_bytes())
                .raw(&key_part)
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
    // Exact match: the full (terminated) key part is a unique prefix for this
    // value, and the id is whatever follows it.
    let key_part = encode_index_key_part(value)?;
    let prefix = key::KeyBuilder::new()
        .namespace(ns)
        .database(db)
        .table(table)
        .tag(key::tag::INDEX)
        .segment(index_name.as_bytes())
        .raw(&key_part)
        .build();

    let entries = storage.prefix_scan(&prefix)?;
    let mut ids = Vec::new();
    for (k, _) in entries {
        let id_bytes = &k[prefix.len()..];
        if let Ok(id) = std::str::from_utf8(id_bytes) {
            ids.push(id.to_string());
        }
    }
    Ok(ids)
}

/// A single range bound: the comparison value and whether it is inclusive.
pub type RangeBound = (Value, bool);

/// Find all record IDs whose indexed value falls within `[lower, upper]`
/// (each bound optional, with its own inclusive flag).
///
/// Results are returned in ascending index-value order. The scan reads only
/// the matching index entries, never the documents.
pub fn find_ids_by_range(
    storage: &DllbStorage,
    ns: &str,
    db: &str,
    table: &str,
    index_name: &str,
    lower: Option<&RangeBound>,
    upper: Option<&RangeBound>,
) -> Result<Vec<String>> {
    let region = key::index_prefix(ns, db, table, index_name);

    // Lower (inclusive start of the half-open scan).
    let start = match lower {
        None => region.clone(),
        Some((v, inclusive)) => {
            let p = escaped_with_region(&region, v)?;
            if *inclusive {
                // `>= v`: entries for v begin at `region + P(v)`.
                p
            } else {
                // `> v`: skip every entry equal to v (they share `P(v)+TERM`).
                let mut with_term = p;
                with_term.extend_from_slice(&TERM);
                key::prefix_end(&with_term)
            }
        }
    };

    // Upper (exclusive end of the half-open scan).
    let end = match upper {
        None => key::prefix_end(&region),
        Some((v, inclusive)) => {
            let p = escaped_with_region(&region, v)?;
            if *inclusive {
                // `<= v`: include every entry equal to v.
                let mut with_term = p;
                with_term.extend_from_slice(&TERM);
                key::prefix_end(&with_term)
            } else {
                // `< v`: stop before any entry equal to v.
                p
            }
        }
    };

    if start >= end {
        return Ok(Vec::new());
    }

    let entries = storage.scan(&start, &end)?;
    let mut ids = Vec::with_capacity(entries.len());
    for (k, _) in entries {
        if let Some(id) = id_from_entry(&k, region.len()) {
            ids.push(id);
        }
    }
    Ok(ids)
}

/// `region + escaped(value)` (no terminator) -- the building block for range
/// scan bounds.
fn escaped_with_region(region: &[u8], value: &Value) -> Result<Vec<u8>> {
    let escaped = encode_index_value_escaped(value)?;
    let mut out = Vec::with_capacity(region.len() + escaped.len());
    out.extend_from_slice(region);
    out.extend_from_slice(&escaped);
    Ok(out)
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

    // -- order-preserving key part --------------------------------------------

    fn kp(v: &Value) -> Vec<u8> {
        encode_index_key_part(v).unwrap()
    }

    #[test]
    fn key_part_orders_shared_prefix_strings() {
        // A prefix must sort before the string that extends it -- the case the
        // old 0xFF separator got wrong.
        assert!(kp(&Value::String("ab".into())) < kp(&Value::String("abc".into())));
        assert!(kp(&Value::String("abc".into())) < kp(&Value::String("abd".into())));
        assert!(kp(&Value::String("a".into())) < kp(&Value::String("ab".into())));
    }

    #[test]
    fn key_part_orders_match_int_order() {
        let vals = [-1000i64, -100, -1, 0, 1, 100, 1000];
        for w in vals.windows(2) {
            assert!(
                kp(&Value::Int(w[0])) < kp(&Value::Int(w[1])),
                "{} should encode before {}",
                w[0],
                w[1]
            );
        }
    }

    #[test]
    fn key_part_orders_match_float_order() {
        let vals = [-2.5f64, -1.0, 0.0, 1.5, 9.9];
        for w in vals.windows(2) {
            assert!(kp(&Value::Float(w[0])) < kp(&Value::Float(w[1])));
        }
    }

    #[test]
    fn key_part_orders_bool() {
        assert!(kp(&Value::Bool(false)) < kp(&Value::Bool(true)));
    }

    // -- id recovery ----------------------------------------------------------

    fn entry_for(index_name: &str, value: &Value, id: &str) -> (Vec<u8>, usize) {
        let region = key::index_prefix("ns", "db", "user", index_name);
        let mut entry = region.clone();
        entry.extend_from_slice(&encode_index_key_part(value).unwrap());
        entry.extend_from_slice(id.as_bytes());
        (entry, region.len())
    }

    #[test]
    fn id_recovered_for_int_value() {
        let (entry, region_len) = entry_for("by_age", &Value::Int(-5), "alice");
        assert_eq!(id_from_entry(&entry, region_len).as_deref(), Some("alice"));
    }

    #[test]
    fn id_recovered_after_embedded_nul_string() {
        // A string containing 0x00 must terminate cleanly and still yield the id.
        let (entry, region_len) = entry_for("by_name", &Value::String("a\u{0}b".into()), "rec1");
        assert_eq!(id_from_entry(&entry, region_len).as_deref(), Some("rec1"));
    }
}
