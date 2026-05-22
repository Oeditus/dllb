//! Serialization and deserialization of [`Document`] values.
//!
//! - **Internal storage**: MessagePack (compact binary, via `rmp-serde`)
//! - **Client I/O**: JSON (human-readable, via `serde_json`)
//!
//! The document's [`RecordId`] is NOT stored in the serialized bytes --
//! it lives in the KV key. Only the fields map is serialized.

use std::collections::BTreeMap;

use dllb_core::{Error, RecordId, Result, Value};

use crate::Document;

/// Serialize a document's fields to MessagePack bytes.
pub fn serialize(doc: &Document) -> Result<Vec<u8>> {
    rmp_serde::to_vec(&doc.fields).map_err(|e| Error::Serialization(e.to_string()))
}

/// Deserialize MessagePack bytes into a document, attaching the given ID.
pub fn deserialize(id: RecordId, bytes: &[u8]) -> Result<Document> {
    let fields: BTreeMap<String, Value> =
        rmp_serde::from_slice(bytes).map_err(|e| Error::Serialization(e.to_string()))?;
    Ok(Document { id, fields })
}

/// Serialize a document's fields to a JSON string.
pub fn to_json(doc: &Document) -> Result<String> {
    serde_json::to_string_pretty(&doc.fields).map_err(|e| Error::Serialization(e.to_string()))
}

/// Deserialize a JSON string into a document, attaching the given ID.
pub fn from_json(id: RecordId, json: &str) -> Result<Document> {
    let fields: BTreeMap<String, Value> =
        serde_json::from_str(json).map_err(|e| Error::Serialization(e.to_string()))?;
    Ok(Document { id, fields })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_doc() -> Document {
        Document::new(RecordId::new("user", "alice"))
            .with_field("name", Value::String("Alice".into()))
            .with_field("age", Value::Int(30))
            .with_field("active", Value::Bool(true))
    }

    #[test]
    fn msgpack_roundtrip() {
        let doc = sample_doc();
        let bytes = serialize(&doc).unwrap();
        let restored = deserialize(doc.id.clone(), &bytes).unwrap();
        assert_eq!(doc, restored);
    }

    #[test]
    fn json_roundtrip() {
        let doc = sample_doc();
        let json = to_json(&doc).unwrap();
        let restored = from_json(doc.id.clone(), &json).unwrap();
        assert_eq!(doc, restored);
    }

    #[test]
    fn vector_roundtrip() {
        let doc = Document::new(RecordId::new("node", "fn1"))
            .with_field("embedding", Value::Vector(vec![0.1, 0.2, 0.3]));
        let bytes = serialize(&doc).unwrap();
        let restored = deserialize(doc.id.clone(), &bytes).unwrap();
        assert_eq!(doc, restored);
    }
}
