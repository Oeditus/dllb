//! Query executor: maps parsed [`Statement`]s to concrete crate API calls.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use dllb_core::{RecordId, Result, Value};
use dllb_document::{Collection, Document};
use dllb_graph::{CommunityOptions, Edge, EdgeStore, Traversal, connected_components};
use dllb_graph::{community::Algorithm, detect_communities_weighted};
use dllb_storage::db::DllbStorage;

use crate::ast::*;
use crate::cache::{CacheKey, CacheKind, ComputeCache, WriteVersions};

/// Result of executing a query.
#[derive(Debug)]
pub enum QueryResult {
    /// Statement executed successfully with no return value.
    Ok,
    /// A record was created.
    Created { id: RecordId },
    /// An existing record was updated (e.g. via `ON CONFLICT UPDATE`).
    Updated { id: RecordId },
    /// A record was deleted.
    Deleted { existed: bool },
    /// Rows returned from a SELECT.
    Rows(Vec<BTreeMap<String, Value>>),
    /// Communities returned from a `GRAPH COMMUNITIES` statement.
    Communities {
        algorithm: String,
        groups: Vec<BTreeMap<String, Value>>,
    },
    /// A pre-formatted response served from the compute cache.
    ///
    /// The payload is already serialised in the `OutcomeFormat` that was
    /// requested and is returned as-is by all `format_result_*` functions.
    CachedResponse(String),
    /// Result of a `BEGIN BATCH ... END BATCH` block.
    Batch {
        count: usize,
        created: usize,
        updated: usize,
    },
    /// Number of rows affected by an `UPDATE` statement.
    Update { matched: usize },
    /// Row count from a `COUNT` statement.
    Count { count: usize },
    /// Connected-components summary from a `GRAPH COMPONENTS` statement.
    ///
    /// `count` is the number of components, `largest` the size of the biggest
    /// component, and `nodes` the total number of vertices seen in the edge
    /// list. Member lists are intentionally omitted to keep the response small.
    Components {
        count: i64,
        largest: i64,
        nodes: i64,
    },
}

/// Executes parsed statements against the storage layer.
pub struct QueryExecutor<'s> {
    storage: &'s DllbStorage,
    ns: String,
    db: String,
    /// Shared compute-result cache. Each call to `run` consults this before
    /// executing and stores the result after a cache miss.
    cache: Arc<ComputeCache>,
    /// Per-table write-version counters used for cache invalidation.
    versions: Arc<WriteVersions>,
}

impl<'s> QueryExecutor<'s> {
    /// Create a new executor with a private (non-shared) cache.
    ///
    /// Suitable for tests and one-off uses where cross-request caching is
    /// not needed. For the production server, prefer [`Self::new_with_cache`].
    pub fn new(storage: &'s DllbStorage, ns: &str, db: &str) -> Self {
        Self {
            storage,
            ns: ns.into(),
            db: db.into(),
            cache: Arc::new(ComputeCache::default()),
            versions: Arc::new(WriteVersions::default()),
        }
    }

    /// Create an executor that shares a process-wide cache and version map.
    ///
    /// All connection handlers should share the same `Arc<ComputeCache>` and
    /// `Arc<WriteVersions>` so that a cache entry built by one connection is
    /// served to all subsequent connections, and a write on one connection
    /// invalidates the cache for every connection.
    pub fn new_with_cache(
        storage: &'s DllbStorage,
        ns: &str,
        db: &str,
        cache: Arc<ComputeCache>,
        versions: Arc<WriteVersions>,
    ) -> Self {
        Self {
            storage,
            ns: ns.into(),
            db: db.into(),
            cache,
            versions,
        }
    }

    /// Execute a parsed statement.
    pub fn execute(&self, stmt: &Statement) -> Result<QueryResult> {
        match stmt {
            Statement::Create {
                table,
                id,
                fields,
                on_conflict,
            } => self.exec_create(table, id.as_deref(), fields, on_conflict.as_ref()),
            Statement::Select {
                fields,
                from,
                filter,
                limit,
            } => self.exec_select(fields, from, filter.as_ref(), *limit),
            Statement::Delete { table, id } => self.exec_delete(table, id),
            Statement::Relate {
                src,
                edge_type,
                dst,
                fields,
            } => self.exec_relate(src, edge_type, dst, fields),
            Statement::GraphCommunities {
                table,
                algorithm,
                max_iterations,
                resolution,
            } => self.exec_graph_communities(table, algorithm, *max_iterations, *resolution),
            Statement::Update {
                target,
                fields,
                filter,
            } => self.exec_update(target, fields, filter.as_ref()),
            Statement::Count { table, filter } => self.exec_count(table, filter.as_ref()),
            Statement::GraphComponents { table } => self.exec_graph_components(table),
        }
    }

    /// Convenience: parse + execute in one call.
    ///
    /// Returns the result together with the `OutcomeFormat` requested by
    /// the `OUTCOME` clause (defaults to JSON when omitted).
    ///
    /// For cacheable statements (`GRAPH COMMUNITIES`) this checks the
    /// [`ComputeCache`] before executing. On a miss it computes, formats,
    /// and stores the result. On a hit it returns a
    /// [`QueryResult::CachedResponse`] that bypasses serialisation.
    pub fn run(&self, query: &str) -> Result<(QueryResult, crate::ast::OutcomeFormat)> {
        let q = crate::parser::parse(query)?;

        // -- Cache read-through -----------------------------------------------
        if let Some(payload) = self.try_cache_hit(&q.statement, q.outcome) {
            return Ok((QueryResult::CachedResponse(payload), q.outcome));
        }

        let result = self.execute(&q.statement)?;

        // -- Post-execute: bump version or populate cache --------------------
        self.post_execute(&q.statement, &result, q.outcome);

        Ok((result, q.outcome))
    }

    // -------------------------------------------------------------------
    // Cache helpers
    // -------------------------------------------------------------------

    /// Check the cache for a pre-computed result. Returns the formatted
    /// payload string on a hit, or `None` on a miss or for non-cacheable
    /// statements.
    fn try_cache_hit(&self, stmt: &Statement, outcome: OutcomeFormat) -> Option<String> {
        match stmt {
            Statement::GraphCommunities {
                table,
                algorithm,
                max_iterations,
                resolution,
            } => {
                let kind =
                    CacheKind::communities(algo_str(algorithm), *max_iterations, *resolution);
                let key = CacheKey::new(&self.ns, &self.db, table, kind, outcome);
                let version = self.versions.current(&self.ns, &self.db, table);
                self.cache.get(&key, version)
            }
            Statement::GraphComponents { table } => {
                let key = CacheKey::new(&self.ns, &self.db, table, CacheKind::Components, outcome);
                let version = self.versions.current(&self.ns, &self.db, table);
                self.cache.get(&key, version)
            }
            _ => None,
        }
    }

    /// Post-execution side effects:
    /// - `RELATE` bumps the write version for the edge table.
    /// - `GRAPH COMMUNITIES` stores the formatted result in the cache.
    fn post_execute(&self, stmt: &Statement, result: &QueryResult, outcome: OutcomeFormat) {
        match (stmt, result) {
            // Bump write version whenever an edge is successfully written.
            (Statement::Relate { edge_type, .. }, QueryResult::Ok) => {
                self.versions.bump(&self.ns, &self.db, edge_type);
            }

            // Populate cache after a successful communities computation.
            (
                Statement::GraphCommunities {
                    table,
                    algorithm,
                    max_iterations,
                    resolution,
                },
                QueryResult::Communities { .. },
            ) => {
                let kind =
                    CacheKind::communities(algo_str(algorithm), *max_iterations, *resolution);
                let key = CacheKey::new(&self.ns, &self.db, table, kind, outcome);
                // Read version *after* computation so the cached entry
                // reflects the write state at time of result, not before.
                let version = self.versions.current(&self.ns, &self.db, table);
                let payload = crate::format::format_result(result, outcome);
                self.cache.insert(key, payload, version);
            }

            // Populate cache after a successful components computation.
            (Statement::GraphComponents { table }, QueryResult::Components { .. }) => {
                let key = CacheKey::new(&self.ns, &self.db, table, CacheKind::Components, outcome);
                let version = self.versions.current(&self.ns, &self.db, table);
                let payload = crate::format::format_result(result, outcome);
                self.cache.insert(key, payload, version);
            }

            _ => {}
        }
    }

    // -------------------------------------------------------------------
    // Statement handlers
    // -------------------------------------------------------------------

    fn exec_create(
        &self,
        table: &str,
        id: Option<&str>,
        fields: &[(String, Literal)],
        on_conflict: Option<&crate::ast::OnConflict>,
    ) -> Result<QueryResult> {
        let coll = Collection::new(self.storage, &self.ns, &self.db, table);

        let record_id = match id {
            Some(id) => RecordId::new(table, id),
            None => RecordId::generate(table),
        };

        let mut doc = Document::new(record_id.clone());
        for (name, lit) in fields {
            doc.set(name, lit.to_value());
        }

        // ON CONFLICT UPDATE requires an explicit ID to be meaningful.
        if let Some(conflict) = on_conflict {
            let id_str = id.ok_or_else(|| {
                dllb_core::Error::Query("ON CONFLICT UPDATE requires an explicit record ID".into())
            })?;

            let update_fields: std::collections::BTreeMap<String, Value> = match conflict {
                crate::ast::OnConflict::Update => {
                    // Reuse the CREATE fields for the update.
                    fields
                        .iter()
                        .map(|(k, v)| (k.clone(), v.to_value()))
                        .collect()
                }
                crate::ast::OnConflict::UpdateSet(update_set) => {
                    // Use the explicit ON CONFLICT UPDATE SET fields.
                    update_set
                        .iter()
                        .map(|(k, v)| (k.clone(), v.to_value()))
                        .collect()
                }
            };

            let (result_id, was_created) = coll.upsert(id_str, doc, update_fields)?;
            if was_created {
                Ok(QueryResult::Created { id: result_id })
            } else {
                Ok(QueryResult::Updated { id: result_id })
            }
        } else {
            let created_id = match id {
                Some(id) => coll.create_with_id(id, doc)?,
                None => coll.create(doc)?,
            };
            Ok(QueryResult::Created { id: created_id })
        }
    }

    fn exec_select(
        &self,
        select_fields: &SelectFields,
        from: &FromTarget,
        filter: Option<&WhereClause>,
        limit: Option<u64>,
    ) -> Result<QueryResult> {
        // Graph traversal has its own execution path.
        if let SelectFields::Traversal(chain) = select_fields {
            return self.exec_traversal_select(chain, from, filter, limit);
        }

        let (table, docs) = match from {
            FromTarget::Table(table) => {
                let coll = Collection::new(self.storage, &self.ns, &self.db, table);
                (table.as_str(), coll.scan_all()?)
            }
            FromTarget::Record(r) => {
                let coll = Collection::new(self.storage, &self.ns, &self.db, &r.table);
                let docs = match coll.get(&r.id)? {
                    Some(d) => vec![d],
                    None => vec![],
                };
                (r.table.as_str(), docs)
            }
        };

        // Apply WHERE filter.
        let filtered: Vec<Document> = match filter {
            Some(clause) => docs
                .into_iter()
                .filter(|d| matches_where(d, clause))
                .collect(),
            None => docs,
        };

        // Apply LIMIT.
        let limited: Vec<Document> = match limit {
            Some(n) => filtered.into_iter().take(n as usize).collect(),
            None => filtered,
        };

        // Project fields.
        let rows: Vec<BTreeMap<String, Value>> = limited
            .into_iter()
            .map(|doc| {
                let mut row = match select_fields {
                    SelectFields::All => doc.fields.clone(),
                    SelectFields::Named(names) => {
                        let mut m = BTreeMap::new();
                        for name in names {
                            if let Some(v) = doc.fields.get(name) {
                                m.insert(name.clone(), v.clone());
                            }
                        }
                        m
                    }
                    SelectFields::Traversal(_) => unreachable!(),
                };
                // Always include id.
                row.insert("id".into(), Value::String(format!("{table}:{}", doc.id.id)));
                row
            })
            .collect();

        Ok(QueryResult::Rows(rows))
    }

    /// Execute a graph traversal SELECT.
    ///
    /// Follows each hop in the chain starting from the `from` source(s),
    /// then fetches the final destination records and applies the WHERE filter.
    fn exec_traversal_select(
        &self,
        chain: &crate::ast::TraversalChain,
        from: &FromTarget,
        filter: Option<&WhereClause>,
        limit: Option<u64>,
    ) -> Result<QueryResult> {
        use crate::ast::TraversalDirection;

        // Collect starting vertex IDs. For a whole-table source we only need
        // the IDs, so scan keys (no document bodies are deserialized).
        let mut current_ids: Vec<String> = match from {
            FromTarget::Record(r) => vec![r.id.clone()],
            FromTarget::Table(t) => {
                let coll = Collection::new(self.storage, &self.ns, &self.db, t);
                coll.scan_ids()?
            }
        };

        // Follow each hop in sequence. Hops use neighbor-only scans (no edge
        // properties are read), and destinations are deduplicated with a set
        // plus an insertion-ordered vector -- O(1) amortized per neighbor
        // instead of the previous O(n) `Vec::contains` scan per neighbor.
        for hop in &chain.hops {
            let es = EdgeStore::new(self.storage, &self.ns, &self.db, &hop.edge_type);
            let tv = Traversal::new(&es);
            let mut next_ids: Vec<String> = Vec::new();
            let mut seen: HashSet<String> = HashSet::new();

            for id in &current_ids {
                let neighbors = match hop.direction {
                    TraversalDirection::Out => tv.outgoing_neighbors_typed(id, &hop.edge_type)?,
                    TraversalDirection::In => tv.incoming_neighbors_typed(id, &hop.edge_type)?,
                };
                for dest in neighbors {
                    if seen.insert(dest.clone()) {
                        next_ids.push(dest);
                    }
                }
            }
            current_ids = next_ids;
        }

        // Resolve final destination records.
        let final_table = match chain.hops.last() {
            Some(h) => h.dest_table.as_str(),
            None => return Ok(QueryResult::Rows(vec![])),
        };
        let coll = Collection::new(self.storage, &self.ns, &self.db, final_table);

        // Fetch all destination documents in a single read transaction
        // (preserving traversal order; missing records are skipped).
        let docs = coll.get_many(&current_ids)?;

        let mut rows: Vec<BTreeMap<String, Value>> = Vec::with_capacity(docs.len());
        for doc in docs {
            // Apply WHERE filter on the destination document.
            if let Some(clause) = filter
                && !matches_where(&doc, clause)
            {
                continue;
            }
            let mut row = match &chain.projection {
                None => doc.fields.clone(),
                Some(field) => {
                    let mut m = BTreeMap::new();
                    if let Some(v) = doc.fields.get(field) {
                        m.insert(field.clone(), v.clone());
                    }
                    m
                }
            };
            row.insert(
                "id".into(),
                Value::String(format!("{final_table}:{}", doc.id.id)),
            );
            rows.push(row);
        }

        // Apply LIMIT.
        if let Some(n) = limit {
            rows.truncate(n as usize);
        }

        Ok(QueryResult::Rows(rows))
    }

    fn exec_graph_communities(
        &self,
        table: &str,
        algorithm: &crate::ast::CommunityAlgorithm,
        max_iterations: usize,
        resolution: f64,
    ) -> Result<QueryResult> {
        let store = EdgeStore::new(self.storage, &self.ns, &self.db, table);
        let edges = store.scan_all_outgoing()?;

        let algo = match algorithm {
            crate::ast::CommunityAlgorithm::Louvain => Algorithm::Louvain,
            crate::ast::CommunityAlgorithm::LabelPropagation => Algorithm::LabelPropagation,
        };

        let opts = CommunityOptions {
            algorithm: algo,
            max_iterations,
            resolution,
            ..CommunityOptions::default()
        };

        let result = detect_communities_weighted(&edges, &opts);

        // Serialise into rows: one row per community.
        let mut groups: Vec<BTreeMap<String, Value>> = result
            .groups
            .into_iter()
            .map(|(comm_id, members)| {
                let size = members.len() as i64;
                let members_val = Value::Array(members.into_iter().map(Value::String).collect());
                let mut row = BTreeMap::new();
                row.insert("id".into(), Value::String(comm_id));
                row.insert("size".into(), Value::Int(size));
                row.insert("members".into(), members_val);
                row
            })
            .collect();

        // Sort by descending size for a consistent, useful output order.
        groups.sort_by(|a, b| {
            let sa = match a.get("size") {
                Some(Value::Int(n)) => *n,
                _ => 0,
            };
            let sb = match b.get("size") {
                Some(Value::Int(n)) => *n,
                _ => 0,
            };
            sb.cmp(&sa)
        });

        let algo_name = match algorithm {
            crate::ast::CommunityAlgorithm::Louvain => "louvain",
            crate::ast::CommunityAlgorithm::LabelPropagation => "lp",
        }
        .to_string();

        Ok(QueryResult::Communities {
            algorithm: algo_name,
            groups,
        })
    }

    fn exec_update(
        &self,
        target: &FromTarget,
        fields: &[(String, Literal)],
        filter: Option<&WhereClause>,
    ) -> Result<QueryResult> {
        let update_fields: BTreeMap<String, Value> = fields
            .iter()
            .map(|(k, v)| (k.clone(), v.to_value()))
            .collect();

        match target {
            // Single-record update: merge fields if the record exists.
            FromTarget::Record(r) => {
                let coll = Collection::new(self.storage, &self.ns, &self.db, &r.table);
                if coll.get(&r.id)?.is_some() {
                    coll.merge(&r.id, update_fields)?;
                    Ok(QueryResult::Update { matched: 1 })
                } else {
                    Ok(QueryResult::Update { matched: 0 })
                }
            }
            // Table update: merge fields into every row matching the filter.
            FromTarget::Table(table) => {
                let coll = Collection::new(self.storage, &self.ns, &self.db, table);
                let docs = coll.scan_all()?;
                let mut matched = 0usize;
                for doc in docs {
                    let keep = match filter {
                        Some(clause) => matches_where(&doc, clause),
                        None => true,
                    };
                    if keep {
                        coll.merge(&doc.id.id, update_fields.clone())?;
                        matched += 1;
                    }
                }
                Ok(QueryResult::Update { matched })
            }
        }
    }

    fn exec_count(&self, table: &str, filter: Option<&WhereClause>) -> Result<QueryResult> {
        let coll = Collection::new(self.storage, &self.ns, &self.db, table);
        let count = match filter {
            None => coll.count()?,
            Some(clause) => coll
                .scan_all()?
                .iter()
                .filter(|d| matches_where(d, clause))
                .count(),
        };
        Ok(QueryResult::Count { count })
    }

    fn exec_graph_components(&self, table: &str) -> Result<QueryResult> {
        let store = EdgeStore::new(self.storage, &self.ns, &self.db, table);
        // Connectivity ignores weights, so use the key-only edge scan that
        // never deserializes edge properties.
        let edges = store.scan_all_edges()?;

        let comps = connected_components(&edges);

        let count = comps.len() as i64;
        let mut largest = 0i64;
        let mut nodes = 0i64;
        for members in comps.groups.values() {
            let size = members.len() as i64;
            nodes += size;
            if size > largest {
                largest = size;
            }
        }

        Ok(QueryResult::Components {
            count,
            largest,
            nodes,
        })
    }

    // -------------------------------------------------------------------
    // Batch execution
    // -------------------------------------------------------------------

    /// Execute multiple statements in a single storage transaction.
    ///
    /// All KV operations are collected first, then applied in one atomic
    /// `write_batch` call. This eliminates the per-statement write-commit
    /// overhead that dominates bulk ingestion workloads.
    ///
    /// Only `CREATE` (with optional `ON CONFLICT UPDATE`) and `RELATE`
    /// statements are supported inside a batch. SELECT, DELETE, and
    /// GRAPH COMMUNITIES are rejected.
    pub fn execute_batch(&self, stmts: &[Statement]) -> Result<QueryResult> {
        let mut all_puts: Vec<(Vec<u8>, Vec<u8>)> = Vec::with_capacity(stmts.len() * 2);
        let mut all_deletes: Vec<Vec<u8>> = Vec::new();
        let mut created: usize = 0;
        let mut updated: usize = 0;

        for stmt in stmts {
            match stmt {
                Statement::Create {
                    table,
                    id,
                    fields,
                    on_conflict,
                } => {
                    let coll = Collection::new(self.storage, &self.ns, &self.db, table);
                    let mut doc = Document::new(RecordId::generate(table));
                    for (name, lit) in fields {
                        doc.set(name, lit.to_value());
                    }

                    if let Some(conflict) = on_conflict {
                        let id_str = id.as_deref().ok_or_else(|| {
                            dllb_core::Error::Query(
                                "ON CONFLICT UPDATE requires an explicit record ID".into(),
                            )
                        })?;

                        let update_fields: std::collections::BTreeMap<String, Value> =
                            match conflict {
                                crate::ast::OnConflict::Update => fields
                                    .iter()
                                    .map(|(k, v)| (k.clone(), v.to_value()))
                                    .collect(),
                                crate::ast::OnConflict::UpdateSet(update_set) => update_set
                                    .iter()
                                    .map(|(k, v)| (k.clone(), v.to_value()))
                                    .collect(),
                            };

                        let (_rid, was_created, puts, deletes) =
                            coll.upsert_to_ops(id_str, doc, update_fields)?;
                        all_puts.extend(puts);
                        all_deletes.extend(deletes);
                        if was_created {
                            created += 1;
                        } else {
                            updated += 1;
                        }
                    } else {
                        let id_str = match id {
                            Some(s) => s.clone(),
                            None => doc.id.id.clone(),
                        };
                        let (_rid, puts) = coll.create_to_ops(&id_str, doc)?;
                        all_puts.extend(puts);
                        created += 1;
                    }
                }
                Statement::Relate {
                    src,
                    edge_type,
                    dst,
                    fields,
                } => {
                    let store = EdgeStore::new(self.storage, &self.ns, &self.db, edge_type);
                    let mut edge = Edge::new(&src.id, edge_type, &dst.id);
                    for (name, lit) in fields {
                        edge = edge.with_property(name, lit.to_value());
                    }
                    let puts = store.relate_to_ops(&edge)?;
                    all_puts.extend(puts);
                }
                _ => {
                    return Err(dllb_core::Error::Query(
                        "only CREATE and RELATE statements are supported inside BEGIN BATCH".into(),
                    ));
                }
            }
        }

        // Single atomic write.
        let put_refs: Vec<(&[u8], &[u8])> = all_puts
            .iter()
            .map(|(k, v)| (k.as_slice(), v.as_slice()))
            .collect();
        let del_refs: Vec<&[u8]> = all_deletes.iter().map(|k| k.as_slice()).collect();
        self.storage.write_batch(&put_refs, &del_refs)?;

        let count = stmts.len();

        // Bump write versions for any edge types touched.
        for stmt in stmts {
            if let Statement::Relate { edge_type, .. } = stmt {
                self.versions.bump(&self.ns, &self.db, edge_type);
            }
        }

        Ok(QueryResult::Batch {
            count,
            created,
            updated,
        })
    }

    fn exec_delete(&self, table: &str, id: &str) -> Result<QueryResult> {
        let coll = Collection::new(self.storage, &self.ns, &self.db, table);
        let existed = coll.delete(id)?;
        Ok(QueryResult::Deleted { existed })
    }

    fn exec_relate(
        &self,
        src: &RecordRef,
        edge_type: &str,
        dst: &RecordRef,
        fields: &[(String, Literal)],
    ) -> Result<QueryResult> {
        let store = EdgeStore::new(self.storage, &self.ns, &self.db, edge_type);

        let mut edge = Edge::new(&src.id, edge_type, &dst.id);
        for (name, lit) in fields {
            edge = edge.with_property(name, lit.to_value());
        }
        store.relate(&edge)?;

        Ok(QueryResult::Ok)
    }
}

// ---------------------------------------------------------------------------
// Module-level helpers
// ---------------------------------------------------------------------------

/// Map a `CommunityAlgorithm` to the string used in cache keys.
#[inline]
fn algo_str(algo: &crate::ast::CommunityAlgorithm) -> &'static str {
    match algo {
        crate::ast::CommunityAlgorithm::Louvain => "louvain",
        crate::ast::CommunityAlgorithm::LabelPropagation => "lp",
    }
}

// ---------------------------------------------------------------------------
// WHERE evaluation helpers
// ---------------------------------------------------------------------------

/// Evaluate a [`WhereClause`] against a document.
fn matches_where(doc: &Document, clause: &WhereClause) -> bool {
    match clause {
        WhereClause::Cmp { field, op, value } => {
            let Some(doc_val) = doc.fields.get(field) else {
                return false;
            };
            let target = value.to_value();
            match op {
                CmpOp::Eq => doc_val == &target,
                CmpOp::Ne => doc_val != &target,
                CmpOp::Gt => {
                    matches!(
                        cmp_values(doc_val, &target),
                        Some(std::cmp::Ordering::Greater)
                    )
                }
                CmpOp::Lt => {
                    matches!(cmp_values(doc_val, &target), Some(std::cmp::Ordering::Less))
                }
                CmpOp::Gte => matches!(
                    cmp_values(doc_val, &target),
                    Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)
                ),
                CmpOp::Lte => matches!(
                    cmp_values(doc_val, &target),
                    Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
                ),
            }
        }
        WhereClause::And(left, right) => matches_where(doc, left) && matches_where(doc, right),
        WhereClause::IsNull { field, negated } => {
            // A field is "set" when present and not `Value::None`.
            let is_set = doc.fields.get(field).is_some_and(|v| !v.is_none());
            // `IS NONE` matches unset fields; `IS NOT NONE` matches set fields.
            if *negated { is_set } else { !is_set }
        }
    }
}

/// Compare two [`Value`]s for ordering.
///
/// Returns `None` for incomparable type combinations (e.g. string vs int).
/// Cross-type Int/Float comparison is supported.
fn cmp_values(a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Some(x.cmp(y)),
        (Value::Float(x), Value::Float(y)) => x.partial_cmp(y),
        (Value::Int(x), Value::Float(y)) => (*x as f64).partial_cmp(y),
        (Value::Float(x), Value::Int(y)) => x.partial_cmp(&(*y as f64)),
        (Value::String(x), Value::String(y)) => Some(x.cmp(y)),
        _ => None,
    }
}
