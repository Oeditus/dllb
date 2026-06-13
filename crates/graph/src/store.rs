//! [`EdgeStore`] -- CRUD operations for graph edges.
//!
//! Each edge is stored as two KV entries (outgoing + incoming reverse)
//! for bidirectional traversal. Edge properties are serialized into the
//! outgoing entry; the reverse entry's value is empty.

use std::collections::BTreeMap;

use dllb_core::{Error, Result, Value};
use dllb_storage::db::DllbStorage;
use dllb_storage::key;

use crate::edge::Edge;

/// Graph edge store scoped to a namespace/database/table.
pub struct EdgeStore<'s> {
    pub(crate) storage: &'s DllbStorage,
    pub(crate) ns: String,
    pub(crate) db: String,
    pub(crate) table: String,
}

impl<'s> EdgeStore<'s> {
    /// Create a new edge store.
    pub fn new(storage: &'s DllbStorage, ns: &str, db: &str, table: &str) -> Self {
        Self {
            storage,
            ns: ns.into(),
            db: db.into(),
            table: table.into(),
        }
    }

    /// Create a directed edge (RELATE).
    ///
    /// Writes both the outgoing and incoming (reverse) KV entries atomically.
    pub fn relate(&self, edge: &Edge) -> Result<()> {
        let props = serialize_props(&edge.properties)?;
        let out_key = key::graph_outgoing_key(
            &self.ns,
            &self.db,
            &self.table,
            &edge.src,
            &edge.edge_type,
            &edge.dst,
        );
        let in_key = key::graph_incoming_key(
            &self.ns,
            &self.db,
            &self.table,
            &edge.dst,
            &edge.edge_type,
            &edge.src,
        );
        self.storage
            .put_batch(&[(&out_key, &props), (&in_key, &[])])
    }

    /// Like [`relate`](Self::relate), but returns the KV operations without
    /// writing to storage.
    ///
    /// Returns the two put operations (outgoing + incoming keys).
    pub fn relate_to_ops(&self, edge: &Edge) -> Result<Vec<(Vec<u8>, Vec<u8>)>> {
        let props = serialize_props(&edge.properties)?;
        let out_key = key::graph_outgoing_key(
            &self.ns,
            &self.db,
            &self.table,
            &edge.src,
            &edge.edge_type,
            &edge.dst,
        );
        let in_key = key::graph_incoming_key(
            &self.ns,
            &self.db,
            &self.table,
            &edge.dst,
            &edge.edge_type,
            &edge.src,
        );
        Ok(vec![(out_key, props), (in_key, vec![])])
    }

    /// Get an edge by its (src, edge_type, dst) triple.
    pub fn get(&self, src: &str, edge_type: &str, dst: &str) -> Result<Option<Edge>> {
        let out_key = key::graph_outgoing_key(&self.ns, &self.db, &self.table, src, edge_type, dst);
        match self.storage.get(&out_key)? {
            Some(bytes) => {
                let properties = deserialize_props(&bytes)?;
                Ok(Some(Edge {
                    src: src.into(),
                    edge_type: edge_type.into(),
                    dst: dst.into(),
                    properties,
                }))
            }
            None => Ok(None),
        }
    }

    /// Delete an edge. Returns `true` if it existed.
    pub fn delete(&self, src: &str, edge_type: &str, dst: &str) -> Result<bool> {
        let out_key = key::graph_outgoing_key(&self.ns, &self.db, &self.table, src, edge_type, dst);
        let existed = self.storage.contains(&out_key)?;
        if existed {
            let in_key =
                key::graph_incoming_key(&self.ns, &self.db, &self.table, dst, edge_type, src);
            self.storage
                .delete_batch(&[out_key.as_slice(), in_key.as_slice()])?;
        }
        Ok(existed)
    }

    /// Scan all outgoing edges in this edge table.
    ///
    /// Returns `(src, dst, weight)` triples. Weight is taken from the
    /// `"weight"` edge property (float or int); defaults to `1.0` when
    /// absent. Incoming reverse-pointer entries are skipped automatically.
    pub fn scan_all_outgoing(&self) -> dllb_core::Result<Vec<(String, String, f64)>> {
        use dllb_storage::key::{self, tag};
        let prefix = key::table_prefix(&self.ns, &self.db, &self.table, tag::GRAPH_EDGE);
        let entries = self.storage.prefix_scan(&prefix)?;
        let mut edges = Vec::with_capacity(entries.len() / 2);

        for (k, v) in entries {
            let parts = key::parse_key(&k)?;
            // remainder = src\0<dir>edge_type\0dst
            let segs: Vec<&[u8]> = parts
                .remainder
                .splitn(3, |&b| b == key::SEPARATOR)
                .collect();
            if segs.len() < 3 {
                continue;
            }
            let dir_type = segs[1];
            if dir_type.is_empty() || dir_type[0] != key::dir::OUTGOING {
                continue; // skip incoming reverse pointers
            }
            let src = match std::str::from_utf8(segs[0]) {
                Ok(s) => s.to_string(),
                Err(_) => continue,
            };
            let dst = match std::str::from_utf8(segs[2]) {
                Ok(s) => s.to_string(),
                Err(_) => continue,
            };
            let weight = if v.is_empty() {
                1.0
            } else {
                let props: BTreeMap<String, dllb_core::Value> =
                    rmp_serde::from_slice(&v).unwrap_or_default();
                match props.get("weight") {
                    Some(dllb_core::Value::Float(f)) => *f,
                    Some(dllb_core::Value::Int(n)) => *n as f64,
                    _ => 1.0,
                }
            };
            edges.push((src, dst, weight));
        }
        Ok(edges)
    }

    /// Scan all outgoing edges as `(src, dst)` pairs, ignoring properties.
    ///
    /// Like [`scan_all_outgoing`](Self::scan_all_outgoing) but reads keys only:
    /// no value is fetched or deserialized. This is the right choice for
    /// weight-agnostic algorithms such as connected components, which would
    /// otherwise pay to deserialize every edge's properties just to discard
    /// them. Incoming reverse-pointer entries are skipped automatically.
    pub fn scan_all_edges(&self) -> dllb_core::Result<Vec<(String, String)>> {
        use dllb_storage::key::{self, tag};
        let prefix = key::table_prefix(&self.ns, &self.db, &self.table, tag::GRAPH_EDGE);
        let keys = self.storage.scan_prefix_keys(&prefix)?;
        let mut edges = Vec::with_capacity(keys.len() / 2);

        for k in keys {
            let parts = key::parse_key(&k)?;
            // remainder = src\0<dir>edge_type\0dst
            let segs: Vec<&[u8]> = parts
                .remainder
                .splitn(3, |&b| b == key::SEPARATOR)
                .collect();
            if segs.len() < 3 {
                continue;
            }
            let dir_type = segs[1];
            if dir_type.is_empty() || dir_type[0] != key::dir::OUTGOING {
                continue; // skip incoming reverse pointers
            }
            let src = match std::str::from_utf8(segs[0]) {
                Ok(s) => s.to_string(),
                Err(_) => continue,
            };
            let dst = match std::str::from_utf8(segs[2]) {
                Ok(s) => s.to_string(),
                Err(_) => continue,
            };
            edges.push((src, dst));
        }
        Ok(edges)
    }

    /// Scan all outgoing edges in this edge table as full [`Edge`] values.
    ///
    /// Like [`scan_all_outgoing`](Self::scan_all_outgoing) but also recovers
    /// the edge type and deserializes properties. Incoming reverse-pointer
    /// entries are skipped automatically.
    pub fn scan_all_outgoing_edges(&self) -> Result<Vec<Edge>> {
        use dllb_storage::key::{self, tag};
        let prefix = key::table_prefix(&self.ns, &self.db, &self.table, tag::GRAPH_EDGE);
        let entries = self.storage.prefix_scan(&prefix)?;
        let mut edges = Vec::with_capacity(entries.len() / 2);

        for (k, v) in entries {
            let parts = key::parse_key(&k)?;
            // remainder = src\0<dir>edge_type\0dst
            let segs: Vec<&[u8]> = parts
                .remainder
                .splitn(3, |&b| b == key::SEPARATOR)
                .collect();
            if segs.len() < 3 {
                continue;
            }
            let dir_type = segs[1];
            if dir_type.is_empty() || dir_type[0] != key::dir::OUTGOING {
                continue; // skip incoming reverse pointers
            }
            let src = match std::str::from_utf8(segs[0]) {
                Ok(s) => s.to_string(),
                Err(_) => continue,
            };
            // dir_type = <dir byte><edge_type bytes>
            let edge_type = match std::str::from_utf8(&dir_type[1..]) {
                Ok(s) => s.to_string(),
                Err(_) => continue,
            };
            let dst = match std::str::from_utf8(segs[2]) {
                Ok(s) => s.to_string(),
                Err(_) => continue,
            };
            let properties = deserialize_props(&v)?;
            edges.push(Edge {
                src,
                edge_type,
                dst,
                properties,
            });
        }
        Ok(edges)
    }

    /// Replace the properties of an existing edge.
    pub fn update_properties(
        &self,
        src: &str,
        edge_type: &str,
        dst: &str,
        props: BTreeMap<String, Value>,
    ) -> Result<()> {
        let out_key = key::graph_outgoing_key(&self.ns, &self.db, &self.table, src, edge_type, dst);
        if !self.storage.contains(&out_key)? {
            return Err(Error::NotFound(format!(
                "edge not found: {src}->{edge_type}->{dst}"
            )));
        }
        let bytes = serialize_props(&props)?;
        self.storage.put(&out_key, &bytes)
    }
}

// ---------------------------------------------------------------------------
// Property serialization (MessagePack)
// ---------------------------------------------------------------------------

fn serialize_props(props: &BTreeMap<String, Value>) -> Result<Vec<u8>> {
    if props.is_empty() {
        return Ok(vec![]);
    }
    rmp_serde::to_vec(props).map_err(|e| Error::Serialization(e.to_string()))
}

fn deserialize_props(bytes: &[u8]) -> Result<BTreeMap<String, Value>> {
    if bytes.is_empty() {
        return Ok(BTreeMap::new());
    }
    rmp_serde::from_slice(bytes).map_err(|e| Error::Serialization(e.to_string()))
}
