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
