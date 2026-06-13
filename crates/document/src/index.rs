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

/// The kind of secondary index.
///
/// `BTree` indexes live entirely in the redb keyspace and are maintained by
/// this crate. `FullText` and `Vector` indexes are backed by external
/// structures (Tantivy / HNSW) that live outside redb; the catalog only
/// records their definition, and the query layer maintains the structures.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum IndexKind {
    /// Sort-ordered B-tree index over one or more fields (the default).
    #[default]
    BTree,
    /// Full-text (BM25) index over a single text field, with a named analyzer.
    FullText { analyzer: String },
    /// Approximate-nearest-neighbor index over a single vector field.
    Vector { dim: usize, metric: String },
}

/// Definition of a secondary index on a collection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexDefinition {
    /// Index name (used in the KV key).
    pub name: String,
    /// Field names to index, in order. Multiple fields form a composite index
    /// (entries are keyed by the concatenated tuple, enabling leftmost-prefix
    /// and prefix-plus-range lookups). Full-text and vector indexes use a
    /// single field.
    pub fields: Vec<String>,
    /// If true, no two documents may have the same indexed value (B-tree only).
    pub unique: bool,
    /// The index kind. Defaults to `BTree` so catalog entries written before
    /// this field existed deserialize unchanged.
    #[serde(default)]
    pub kind: IndexKind,
}

impl IndexDefinition {
    /// Construct a B-tree index definition.
    pub fn btree(name: impl Into<String>, fields: Vec<String>, unique: bool) -> Self {
        Self {
            name: name.into(),
            fields,
            unique,
            kind: IndexKind::BTree,
        }
    }

    /// Whether this is a B-tree (redb-backed) index.
    pub fn is_btree(&self) -> bool {
        matches!(self.kind, IndexKind::BTree)
    }
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
/// region prefix (`ns\0db\0table\0+index_name\0`) and the number of indexed
/// fields. Walks past `field_count` `0x00 0x01` terminators (one per field's
/// value part); the id is everything after the last one.
fn id_from_entry(key: &[u8], region_len: usize, field_count: usize) -> Option<String> {
    let mut i = region_len;
    let mut seen_terms = 0usize;
    while i + 1 < key.len() {
        if key[i] == 0x00 {
            match key[i + 1] {
                0xFF => i += 2, // escaped 0x00, skip the pair
                0x01 => {
                    // End of one field's value part.
                    i += 2;
                    seen_terms += 1;
                    if seen_terms == field_count {
                        return std::str::from_utf8(&key[i..]).ok().map(String::from);
                    }
                }
                _ => return None, // malformed entry
            }
        } else {
            i += 1;
        }
    }
    None
}

/// Collect the indexed field values for `doc` in index-field order.
///
/// Returns `None` unless every indexed field is present and non-none -- a
/// composite entry requires the complete tuple, just as a single-field entry
/// requires its one field.
pub fn collect_index_values(doc: &Document, index: &IndexDefinition) -> Option<Vec<Value>> {
    if index.fields.is_empty() {
        return None;
    }
    let mut values = Vec::with_capacity(index.fields.len());
    for field in &index.fields {
        match doc.fields.get(field) {
            Some(v) if !v.is_none() => values.push(v.clone()),
            _ => return None,
        }
    }
    Some(values)
}

/// Concatenate the order-preserving key parts of a tuple of values.
///
/// Each part is self-delimiting (escaped + terminated), so the concatenation
/// sorts lexicographically by the tuple and field boundaries stay unambiguous.
pub fn encode_composite_key_part(values: &[Value]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    for v in values {
        out.extend_from_slice(&encode_index_key_part(v)?);
    }
    Ok(out)
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
        // Only B-tree indexes live in the redb keyspace; full-text and vector
        // indexes are maintained out-of-band by the query layer.
        if !idx.is_btree() {
            continue;
        }
        if idx.fields.is_empty() {
            return Err(Error::Index("index has no fields".into()));
        }
        // Index the full tuple; skip the entry unless every field is present.
        if let Some(values) = collect_index_values(doc, idx) {
            let key_part = encode_composite_key_part(&values)?;
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

/// A single range bound: the comparison value and whether it is inclusive.
pub type RangeBound = (Value, bool);

/// Find record IDs from an index, given equality values for the leading fields
/// and an optional range on the next field.
///
/// - `eq` holds the values for a leading prefix of the index's fields (it may
///   be empty, a strict prefix, or the whole tuple).
/// - `lower`/`upper` apply a range to the field that follows the `eq` prefix.
/// - `field_count` is the index's total field count, used to recover the id
///   past every field's terminator.
///
/// Results come back in ascending index order and never read documents.
#[allow(clippy::too_many_arguments)]
pub fn find_ids(
    storage: &DllbStorage,
    ns: &str,
    db: &str,
    table: &str,
    index_name: &str,
    eq: &[Value],
    lower: Option<&RangeBound>,
    upper: Option<&RangeBound>,
    field_count: usize,
) -> Result<Vec<String>> {
    let region = key::index_prefix(ns, db, table, index_name);

    // `base` = region + the equality-matched leading key parts. All matching
    // entries share it as a prefix.
    let mut base = region.clone();
    base.extend_from_slice(&encode_composite_key_part(eq)?);

    // Lower (inclusive start of the half-open scan).
    let start = match lower {
        None => base.clone(),
        Some((v, inclusive)) => {
            let mut p = base.clone();
            p.extend_from_slice(&encode_index_value_escaped(v)?);
            if *inclusive {
                p
            } else {
                p.extend_from_slice(&TERM);
                key::prefix_end(&p)
            }
        }
    };

    // Upper (exclusive end of the half-open scan).
    let end = match upper {
        None => key::prefix_end(&base),
        Some((v, inclusive)) => {
            let mut p = base.clone();
            p.extend_from_slice(&encode_index_value_escaped(v)?);
            if *inclusive {
                p.extend_from_slice(&TERM);
                key::prefix_end(&p)
            } else {
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
        if let Some(id) = id_from_entry(&k, region.len(), field_count) {
            ids.push(id);
        }
    }
    Ok(ids)
}

/// Find all record IDs matching a single-field index value (exact match).
pub fn find_by_index(
    storage: &DllbStorage,
    ns: &str,
    db: &str,
    table: &str,
    index_name: &str,
    value: &Value,
) -> Result<Vec<String>> {
    find_ids(
        storage,
        ns,
        db,
        table,
        index_name,
        std::slice::from_ref(value),
        None,
        None,
        1,
    )
}

/// Find all record IDs whose single-field indexed value falls within
/// `[lower, upper]` (each bound optional, with its own inclusive flag).
pub fn find_ids_by_range(
    storage: &DllbStorage,
    ns: &str,
    db: &str,
    table: &str,
    index_name: &str,
    lower: Option<&RangeBound>,
    upper: Option<&RangeBound>,
) -> Result<Vec<String>> {
    find_ids(storage, ns, db, table, index_name, &[], lower, upper, 1)
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

/// Check whether a unique index tuple already exists for a different record.
///
/// `values` is the full tuple of indexed field values (one per index field).
pub fn check_unique_constraint(
    storage: &DllbStorage,
    ns: &str,
    db: &str,
    table: &str,
    index: &IndexDefinition,
    values: &[Value],
    exclude_id: &str,
) -> Result<()> {
    if !index.unique {
        return Ok(());
    }
    let ids = find_ids(
        storage,
        ns,
        db,
        table,
        &index.name,
        values,
        None,
        None,
        index.fields.len(),
    )?;
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

    #[test]
    fn index_definition_back_compat_without_kind() {
        // Catalog entries written before `kind` existed are positional 3-tuples
        // (rmp-serde encodes structs as arrays). They must still deserialize,
        // with `kind` defaulting to BTree.
        let legacy = ("by_age".to_string(), vec!["age".to_string()], false);
        let bytes = rmp_serde::to_vec(&legacy).unwrap();
        let def: IndexDefinition = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(
            def,
            IndexDefinition::btree("by_age", vec!["age".into()], false)
        );
        assert_eq!(def.kind, IndexKind::BTree);
        assert!(def.is_btree());
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

    // -- composite key part ---------------------------------------------------

    fn ckp(values: &[Value]) -> Vec<u8> {
        encode_composite_key_part(values).unwrap()
    }

    #[test]
    fn composite_key_part_orders_by_tuple() {
        // (1,"b") < (1,"c") < (2,"a") -- ordered by the first field, then the
        // second, exactly like a SQL composite key.
        let a1b = ckp(&[Value::Int(1), Value::String("b".into())]);
        let a1c = ckp(&[Value::Int(1), Value::String("c".into())]);
        let a2a = ckp(&[Value::Int(2), Value::String("a".into())]);
        assert!(a1b < a1c);
        assert!(a1c < a2a);
    }

    #[test]
    fn composite_leading_part_is_a_byte_prefix() {
        // The single-field key part of the first value is a byte prefix of any
        // composite key part starting with that value -- this is what makes
        // leftmost-prefix scans work.
        let a1 = encode_index_key_part(&Value::Int(1)).unwrap();
        let a1b = ckp(&[Value::Int(1), Value::String("b".into())]);
        assert!(a1b.starts_with(&a1));
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
        assert_eq!(
            id_from_entry(&entry, region_len, 1).as_deref(),
            Some("alice")
        );
    }

    #[test]
    fn id_recovered_after_embedded_nul_string() {
        // A string containing 0x00 must terminate cleanly and still yield the id.
        let (entry, region_len) = entry_for("by_name", &Value::String("a\u{0}b".into()), "rec1");
        assert_eq!(
            id_from_entry(&entry, region_len, 1).as_deref(),
            Some("rec1")
        );
    }

    #[test]
    fn id_recovered_for_composite_entry() {
        let region = key::index_prefix("ns", "db", "user", "by_a_b");
        let mut entry = region.clone();
        entry.extend_from_slice(&ckp(&[Value::Int(1), Value::String("x".into())]));
        entry.extend_from_slice(b"rec1");
        // Two fields -> skip two terminators to reach the id.
        assert_eq!(
            id_from_entry(&entry, region.len(), 2).as_deref(),
            Some("rec1")
        );
        // The wrong field count stops at the first field boundary, not the id.
        assert_ne!(
            id_from_entry(&entry, region.len(), 1).as_deref(),
            Some("rec1")
        );
    }
}
