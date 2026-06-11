//! Graph traversal engine.
//!
//! Traversals reduce to prefix scans over the sorted keyspace:
//! - Outgoing edges from vertex `v`: scan prefix `~v\0>`
//! - Incoming edges to vertex `v`: scan prefix `~v\0<`
//! - Multi-hop walks chain sequential prefix scans.

use std::collections::BTreeMap;

use dllb_core::{Error, Result, Value};
use dllb_storage::key;
use dllb_storage::key::SEPARATOR;

use crate::edge::Edge;
use crate::store::EdgeStore;

/// Direction of a traversal hop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Direction {
    /// Follow outgoing edges (`->`).
    Out,
    /// Follow incoming edges (`<-`).
    In,
}

/// Specification for a single hop in a multi-hop walk.
#[derive(Debug, Clone)]
pub struct HopSpec {
    pub direction: Direction,
    /// If `Some`, only follow edges of this type. If `None`, follow all types.
    pub edge_type: Option<String>,
}

/// Graph traversal engine operating on an [`EdgeStore`].
pub struct Traversal<'s> {
    store: &'s EdgeStore<'s>,
}

impl<'s> Traversal<'s> {
    pub fn new(store: &'s EdgeStore<'s>) -> Self {
        Self { store }
    }

    // -------------------------------------------------------------------
    // Single-hop: outgoing
    // -------------------------------------------------------------------

    /// All outgoing edges from `src`.
    pub fn outgoing(&self, src: &str) -> Result<Vec<Edge>> {
        let prefix =
            key::vertex_outgoing_prefix(&self.store.ns, &self.store.db, &self.store.table, src);
        self.scan_outgoing(src, &prefix)
    }

    /// Outgoing edges of a specific type from `src`.
    pub fn outgoing_typed(&self, src: &str, edge_type: &str) -> Result<Vec<Edge>> {
        let prefix = key::vertex_outgoing_typed_prefix(
            &self.store.ns,
            &self.store.db,
            &self.store.table,
            src,
            edge_type,
        );
        self.scan_outgoing(src, &prefix)
    }

    /// Outgoing edges from `src` matching a predicate on the edge.
    pub fn outgoing_filtered(
        &self,
        src: &str,
        filter: impl Fn(&Edge) -> bool,
    ) -> Result<Vec<Edge>> {
        Ok(self.outgoing(src)?.into_iter().filter(filter).collect())
    }

    fn scan_outgoing(&self, src: &str, prefix: &[u8]) -> Result<Vec<Edge>> {
        let entries = self.store.storage.prefix_scan(prefix)?;
        let mut edges = Vec::with_capacity(entries.len());
        for (k, v) in entries {
            let parts = key::parse_key(&k)?;
            let (edge_type, dst) = parse_directed_remainder(parts.remainder, true)?;
            let properties = deserialize_props(&v)?;
            edges.push(Edge {
                src: src.into(),
                edge_type,
                dst,
                properties,
            });
        }
        Ok(edges)
    }

    // -------------------------------------------------------------------
    // Single-hop: incoming
    // -------------------------------------------------------------------

    /// All incoming edges to `dst`.
    pub fn incoming(&self, dst: &str) -> Result<Vec<Edge>> {
        let prefix =
            key::vertex_incoming_prefix(&self.store.ns, &self.store.db, &self.store.table, dst);
        self.scan_incoming(dst, &prefix)
    }

    /// Incoming edges of a specific type to `dst`.
    pub fn incoming_typed(&self, dst: &str, edge_type: &str) -> Result<Vec<Edge>> {
        let prefix = key::vertex_incoming_typed_prefix(
            &self.store.ns,
            &self.store.db,
            &self.store.table,
            dst,
            edge_type,
        );
        self.scan_incoming(dst, &prefix)
    }

    fn scan_incoming(&self, dst: &str, prefix: &[u8]) -> Result<Vec<Edge>> {
        let entries = self.store.storage.prefix_scan(prefix)?;
        let mut edges = Vec::with_capacity(entries.len());
        for (k, _v) in entries {
            let parts = key::parse_key(&k)?;
            let (edge_type, src) = parse_directed_remainder(parts.remainder, false)?;
            // Incoming entries have empty values; fetch properties from outgoing.
            let properties = match self.store.get(&src, &edge_type, dst)? {
                Some(e) => e.properties,
                None => BTreeMap::new(),
            };
            edges.push(Edge {
                src,
                edge_type,
                dst: dst.into(),
                properties,
            });
        }
        Ok(edges)
    }

    // -------------------------------------------------------------------
    // Neighbor-only single-hop (IDs without edge properties)
    // -------------------------------------------------------------------
    //
    // These return only the adjacent vertex IDs, parsed directly from the
    // sorted keys. They never deserialize edge properties and -- crucially
    // for the incoming direction -- never issue the per-edge point lookup
    // that `scan_incoming` needs to populate `Edge::properties`. Prefer them
    // for pure reachability traversals (the dominant query-engine path),
    // where edge properties are irrelevant.

    /// All outgoing neighbor vertex IDs from `src` (any edge type).
    pub fn outgoing_neighbors(&self, src: &str) -> Result<Vec<String>> {
        let prefix =
            key::vertex_outgoing_prefix(&self.store.ns, &self.store.db, &self.store.table, src);
        self.scan_neighbors(&prefix)
    }

    /// Outgoing neighbor vertex IDs of a specific edge type from `src`.
    pub fn outgoing_neighbors_typed(&self, src: &str, edge_type: &str) -> Result<Vec<String>> {
        let prefix = key::vertex_outgoing_typed_prefix(
            &self.store.ns,
            &self.store.db,
            &self.store.table,
            src,
            edge_type,
        );
        self.scan_neighbors(&prefix)
    }

    /// All incoming neighbor vertex IDs to `dst` (any edge type).
    pub fn incoming_neighbors(&self, dst: &str) -> Result<Vec<String>> {
        let prefix =
            key::vertex_incoming_prefix(&self.store.ns, &self.store.db, &self.store.table, dst);
        self.scan_neighbors(&prefix)
    }

    /// Incoming neighbor vertex IDs of a specific edge type to `dst`.
    pub fn incoming_neighbors_typed(&self, dst: &str, edge_type: &str) -> Result<Vec<String>> {
        let prefix = key::vertex_incoming_typed_prefix(
            &self.store.ns,
            &self.store.db,
            &self.store.table,
            dst,
            edge_type,
        );
        self.scan_neighbors(&prefix)
    }

    /// Shared key-only scan: returns the "other vertex" of every edge under
    /// `prefix`, reading keys only (no value deserialization, no point gets).
    fn scan_neighbors(&self, prefix: &[u8]) -> Result<Vec<String>> {
        let keys = self.store.storage.scan_prefix_keys(prefix)?;
        let mut neighbors = Vec::with_capacity(keys.len());
        for k in keys {
            let parts = key::parse_key(&k)?;
            neighbors.push(parse_other_vertex(parts.remainder)?);
        }
        Ok(neighbors)
    }

    // -------------------------------------------------------------------
    // Multi-hop walk
    // -------------------------------------------------------------------

    /// Walk a multi-hop path starting from `start`.
    ///
    /// Returns all reachable vertex ID paths. Each inner `Vec<String>` is a
    /// path from `start` through each hop's destination.
    pub fn walk(&self, start: &str, hops: &[HopSpec]) -> Result<Vec<Vec<String>>> {
        let mut paths: Vec<Vec<String>> = vec![vec![start.into()]];

        for hop in hops {
            let mut next_paths = Vec::new();
            for path in &paths {
                let current = path.last().unwrap();
                let edges = match (&hop.direction, &hop.edge_type) {
                    (Direction::Out, Some(et)) => self.outgoing_typed(current, et)?,
                    (Direction::Out, None) => self.outgoing(current)?,
                    (Direction::In, Some(et)) => self.incoming_typed(current, et)?,
                    (Direction::In, None) => self.incoming(current)?,
                };
                for edge in edges {
                    let next_vertex = match hop.direction {
                        Direction::Out => edge.dst.clone(),
                        Direction::In => edge.src.clone(),
                    };
                    let mut new_path = path.clone();
                    new_path.push(next_vertex);
                    next_paths.push(new_path);
                }
            }
            paths = next_paths;
        }

        Ok(paths)
    }
}

// ---------------------------------------------------------------------------
// Key remainder parsing
// ---------------------------------------------------------------------------

/// Parse the remainder after the vertex_id separator in a graph edge key.
///
/// Remainder format: `<dir_byte><edge_type>\0<other_vertex>`
///
/// If `is_outgoing`, other_vertex = dst. If incoming, other_vertex = src.
fn parse_directed_remainder(remainder: &[u8], _is_outgoing: bool) -> Result<(String, String)> {
    // remainder starts with vertex_id\0<dir>edge_type\0other_vertex
    // But we've already consumed the vertex_id via prefix scan.
    // So remainder here (from parse_key) is: vertex_id\0<dir>edge_type\0other_vertex
    // We need to skip past vertex_id\0
    let mut parts = remainder.splitn(3, |&b| b == SEPARATOR);
    let _vertex_id = parts.next(); // already known from caller
    let dir_and_type = parts
        .next()
        .ok_or_else(|| Error::Storage("malformed graph edge key: missing edge_type".into()))?;
    let other = parts
        .next()
        .ok_or_else(|| Error::Storage("malformed graph edge key: missing other vertex".into()))?;

    // dir_and_type starts with direction byte (< or >), followed by edge_type
    if dir_and_type.is_empty() {
        return Err(Error::Storage(
            "malformed graph edge key: empty dir+type".into(),
        ));
    }
    let edge_type = std::str::from_utf8(&dir_and_type[1..])
        .map_err(|e| Error::Storage(e.to_string()))?
        .to_string();
    let other_vertex = std::str::from_utf8(other)
        .map_err(|e| Error::Storage(e.to_string()))?
        .to_string();

    Ok((edge_type, other_vertex))
}

/// Extract only the "other vertex" (third segment) from a graph edge key
/// remainder of the form `vertex_id\0<dir><edge_type>\0other_vertex`.
///
/// Cheaper than [`parse_directed_remainder`] when the edge type is not needed:
/// it skips the first two segments and never allocates the edge-type string.
fn parse_other_vertex(remainder: &[u8]) -> Result<String> {
    let mut parts = remainder.splitn(3, |&b| b == SEPARATOR);
    let _vertex_id = parts.next();
    let _dir_and_type = parts.next();
    let other = parts
        .next()
        .ok_or_else(|| Error::Storage("malformed graph edge key: missing other vertex".into()))?;
    std::str::from_utf8(other)
        .map(|s| s.to_string())
        .map_err(|e| Error::Storage(e.to_string()))
}

fn deserialize_props(bytes: &[u8]) -> Result<BTreeMap<String, Value>> {
    if bytes.is_empty() {
        return Ok(BTreeMap::new());
    }
    rmp_serde::from_slice(bytes).map_err(|e| Error::Serialization(e.to_string()))
}
