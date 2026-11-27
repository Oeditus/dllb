//! [`EdgeStore`] -- CRUD operations for graph edges.
//!
//! Each edge is stored as two KV entries (outgoing + incoming reverse)
//! for bidirectional traversal. Edge properties are serialized into the
//! outgoing entry; the reverse entry's value is empty.

use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock, RwLock};

use dllb_core::{Error, Result, Value};
use dllb_storage::db::DllbStorage;
use dllb_storage::key;

use crate::edge::Edge;

// ---------------------------------------------------------------------------
// Write-version registry
// ---------------------------------------------------------------------------
//
// Analytics results (`GRAPH COMMUNITIES`, `PAGERANK`, `CENTRALITY`,
// `COMPONENTS`) are cached and invalidated by comparing a per-edge-table
// monotonic counter. Bumping that counter here -- inside the only code that
// can mutate edges -- means no caller can forget to invalidate, including
// embedders that drive `EdgeStore` directly rather than through the query
// engine.

type TableKey = (String, String, String); // (ns, db, table)

/// Process-wide registry of per-edge-table write counters.
pub struct WriteVersions {
    map: RwLock<std::collections::HashMap<TableKey, Arc<std::sync::atomic::AtomicU64>>>,
}

impl Default for WriteVersions {
    fn default() -> Self {
        Self {
            map: RwLock::new(std::collections::HashMap::new()),
        }
    }
}

impl WriteVersions {
    /// Current version for the given table. Returns `0` if the table has
    /// never been written to.
    pub fn current(&self, ns: &str, db: &str, table: &str) -> u64 {
        let key = table_key(ns, db, table);
        self.map
            .read()
            .unwrap()
            .get(&key)
            .map(|c| c.load(std::sync::atomic::Ordering::Acquire))
            .unwrap_or(0)
    }

    /// Increment the write counter for `table` by 1.
    pub fn bump(&self, ns: &str, db: &str, table: &str) {
        let key = table_key(ns, db, table);

        // Fast path: the counter already exists -- bump without a write lock.
        {
            let guard = self.map.read().unwrap();
            if let Some(counter) = guard.get(&key) {
                counter.fetch_add(1, std::sync::atomic::Ordering::Release);
                return;
            }
        }

        // Slow path: first write to this table -- insert a new counter.
        let mut guard = self.map.write().unwrap();
        // Double-checked: another task may have inserted while we waited for
        // the write lock.
        guard
            .entry(key)
            .or_insert_with(|| Arc::new(std::sync::atomic::AtomicU64::new(0)))
            .fetch_add(1, std::sync::atomic::Ordering::Release);
    }
}

fn table_key(ns: &str, db: &str, table: &str) -> TableKey {
    (ns.into(), db.into(), table.into())
}

/// The process-wide [`WriteVersions`] instance that `EdgeStore` bumps.
///
/// A single global keeps the invariant enforceable without threading a handle
/// through every call site; the query engine reads the same instance so its
/// cache entries are invalidated by any writer in the process.
pub fn global_write_versions() -> Arc<WriteVersions> {
    static GLOBAL: OnceLock<Arc<WriteVersions>> = OnceLock::new();
    Arc::clone(GLOBAL.get_or_init(|| Arc::new(WriteVersions::default())))
}

/// Graph edge store scoped to a namespace/database/table.
pub struct EdgeStore<'s> {
    pub(crate) storage: &'s DllbStorage,
    pub(crate) ns: String,
    pub(crate) db: String,
    pub(crate) table: String,
    /// Registry bumped on every successful edge mutation. Defaults to the
    /// process-wide instance so cache invalidation cannot be skipped.
    versions: Arc<WriteVersions>,
}

impl<'s> EdgeStore<'s> {
    /// Create a new edge store.
    pub fn new(storage: &'s DllbStorage, ns: &str, db: &str, table: &str) -> Self {
        Self {
            storage,
            ns: ns.into(),
            db: db.into(),
            table: table.into(),
            versions: Arc::new(WriteVersions::default()),
        }
    }

    /// Create an edge store that reports mutations to a shared registry.
    ///
    /// Every writer that must invalidate a shared analytics cache (the query
    /// engine, the server) should use this constructor with
    /// [`global_write_versions`] so all connections observe the same counters.
    pub fn with_versions(
        storage: &'s DllbStorage,
        ns: &str,
        db: &str,
        table: &str,
        versions: Arc<WriteVersions>,
    ) -> Self {
        Self {
            storage,
            ns: ns.into(),
            db: db.into(),
            table: table.into(),
            versions,
        }
    }

    /// Bump this table's write counter, invalidating cached analytics.
    fn invalidate(&self) {
        self.versions.bump(&self.ns, &self.db, &self.table);
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
            .put_batch(&[(&out_key, &props), (&in_key, &[])])?;
        self.invalidate();
        Ok(())
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
            self.invalidate();
        }
        Ok(existed)
    }

    /// Scan all outgoing edges in this edge table.
    ///
    /// Returns `(src, dst, weight)` triples. Weight is taken from the
    /// `"weight"` edge property (float or int); defaults to `1.0` when
    /// absent. Incoming reverse-pointer entries are skipped automatically.
    pub fn scan_all_outgoing(&self) -> dllb_core::Result<Vec<(String, String, f64)>> {
        Ok(self
            .scan_edges(ScanMode::WithProperties)?
            .into_iter()
            .map(|edge| (edge.src, edge.dst, edge_weight(&edge.properties)))
            .collect())
    }

    /// Scan all outgoing edges as `(src, dst)` pairs, ignoring properties.
    ///
    /// Like [`scan_all_outgoing`](Self::scan_all_outgoing) but reads keys only:
    /// no value is fetched or deserialized. This is the right choice for
    /// weight-agnostic algorithms such as connected components, which would
    /// otherwise pay to deserialize every edge's properties just to discard
    /// them. Incoming reverse-pointer entries are skipped automatically.
    pub fn scan_all_edges(&self) -> dllb_core::Result<Vec<(String, String)>> {
        Ok(self
            .scan_edges(ScanMode::KeysOnly)?
            .into_iter()
            .map(|edge| (edge.src, edge.dst))
            .collect())
    }

    /// Scan all outgoing edges in this edge table as full [`Edge`] values.
    ///
    /// Like [`scan_all_outgoing`](Self::scan_all_outgoing) but also recovers
    /// the edge type and deserializes properties. Incoming reverse-pointer
    /// entries are skipped automatically.
    pub fn scan_all_outgoing_edges(&self) -> Result<Vec<Edge>> {
        self.scan_edges(ScanMode::WithProperties)
    }

    /// The single edge-scan implementation shared by the three public
    /// scanners above.
    ///
    /// Edge keys are `ns\0db\0table\0~src\0<dir><edge_type>\0dst`. The
    /// direction byte separates outgoing from incoming entries within a
    /// vertex's region, so a whole-table scan must filter on it: the incoming
    /// reverse pointers are duplicates of the outgoing edges, not extra ones.
    fn scan_edges(&self, mode: ScanMode) -> Result<Vec<Edge>> {
        use dllb_storage::key::tag;

        let prefix = key::table_prefix(&self.ns, &self.db, &self.table, tag::GRAPH_EDGE);

        // Keys-only callers never touch values, which is the whole point of
        // the mode: connectivity and traversal ignore edge properties.
        let entries: Vec<(Vec<u8>, Vec<u8>)> = match mode {
            ScanMode::KeysOnly => self
                .storage
                .scan_prefix_keys(&prefix)?
                .into_iter()
                .map(|k| (k, Vec::new()))
                .collect(),
            ScanMode::WithProperties => self.storage.prefix_scan(&prefix)?,
        };

        let mut edges = Vec::with_capacity(entries.len() / 2);
        for (k, v) in entries {
            let parts = key::parse_key(&k)?;
            // remainder = src\0<dir><edge_type>\0dst
            let segs: Vec<&[u8]> = parts
                .remainder
                .splitn(3, |&b| b == key::SEPARATOR)
                .collect();
            let [src, dir_type, dst] = segs.as_slice() else {
                continue;
            };
            // Skip incoming reverse pointers (and malformed empty segments).
            if dir_type.first() != Some(&key::dir::OUTGOING) {
                continue;
            }
            let (Ok(src), Ok(edge_type), Ok(dst)) = (
                std::str::from_utf8(src),
                std::str::from_utf8(&dir_type[1..]),
                std::str::from_utf8(dst),
            ) else {
                continue;
            };
            let properties = match mode {
                ScanMode::KeysOnly => BTreeMap::new(),
                ScanMode::WithProperties => deserialize_props(&v)?,
            };
            edges.push(Edge {
                src: src.to_string(),
                edge_type: edge_type.to_string(),
                dst: dst.to_string(),
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

/// How much of each edge entry a scan should materialize.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ScanMode {
    /// Read keys only; edge properties come back empty.
    KeysOnly,
    /// Read and deserialize each edge's MessagePack properties.
    WithProperties,
}

/// Extract the numeric `"weight"` property, defaulting to `1.0`.
fn edge_weight(properties: &BTreeMap<String, Value>) -> f64 {
    match properties.get("weight") {
        Some(Value::Float(f)) => *f,
        Some(Value::Int(n)) => *n as f64,
        _ => 1.0,
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
