//! Query executor: maps parsed [`Statement`]s to concrete crate API calls.

use std::collections::BTreeMap;

use dllb_core::{RecordId, Result, Value};
use dllb_document::{Collection, Document};
use dllb_graph::{Edge, EdgeStore, Traversal};
use dllb_storage::db::DllbStorage;

use crate::ast::*;

/// Result of executing a query.
#[derive(Debug)]
pub enum QueryResult {
    /// Statement executed successfully with no return value.
    Ok,
    /// A record was created.
    Created { id: RecordId },
    /// A record was deleted.
    Deleted { existed: bool },
    /// Rows returned from a SELECT.
    Rows(Vec<BTreeMap<String, Value>>),
}

/// Executes parsed statements against the storage layer.
pub struct QueryExecutor<'s> {
    storage: &'s DllbStorage,
    ns: String,
    db: String,
}

impl<'s> QueryExecutor<'s> {
    /// Create a new executor scoped to a namespace and database.
    pub fn new(storage: &'s DllbStorage, ns: &str, db: &str) -> Self {
        Self {
            storage,
            ns: ns.into(),
            db: db.into(),
        }
    }

    /// Execute a parsed statement.
    pub fn execute(&self, stmt: &Statement) -> Result<QueryResult> {
        match stmt {
            Statement::Create { table, id, fields } => {
                self.exec_create(table, id.as_deref(), fields)
            }
            Statement::Select {
                fields,
                from,
                filter,
            } => self.exec_select(fields, from, filter.as_ref()),
            Statement::Delete { table, id } => self.exec_delete(table, id),
            Statement::Relate {
                src,
                edge_type,
                dst,
                fields,
            } => self.exec_relate(src, edge_type, dst, fields),
        }
    }

    /// Convenience: parse + execute in one call.
    ///
    /// Returns the result together with the `OutcomeFormat` requested by
    /// the `OUTCOME` clause (defaults to JSON when omitted).
    pub fn run(&self, query: &str) -> Result<(QueryResult, crate::ast::OutcomeFormat)> {
        let q = crate::parser::parse(query)?;
        let result = self.execute(&q.statement)?;
        Ok((result, q.outcome))
    }

    // -------------------------------------------------------------------
    // Statement handlers
    // -------------------------------------------------------------------

    fn exec_create(
        &self,
        table: &str,
        id: Option<&str>,
        fields: &[(String, Literal)],
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

        let created_id = match id {
            Some(id) => coll.create_with_id(id, doc)?,
            None => coll.create(doc)?,
        };

        Ok(QueryResult::Created { id: created_id })
    }

    fn exec_select(
        &self,
        select_fields: &SelectFields,
        from: &FromTarget,
        filter: Option<&WhereClause>,
    ) -> Result<QueryResult> {
        // Graph traversal has its own execution path.
        if let SelectFields::Traversal(chain) = select_fields {
            return self.exec_traversal_select(chain, from, filter);
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

        // Project fields.
        let rows: Vec<BTreeMap<String, Value>> = filtered
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
    ) -> Result<QueryResult> {
        use crate::ast::TraversalDirection;

        // Collect starting vertex IDs.
        let mut current_ids: Vec<String> = match from {
            FromTarget::Record(r) => vec![r.id.clone()],
            FromTarget::Table(t) => {
                let coll = Collection::new(self.storage, &self.ns, &self.db, t);
                coll.scan_all()?.into_iter().map(|d| d.id.id).collect()
            }
        };

        // Follow each hop in sequence.
        for hop in &chain.hops {
            let es = EdgeStore::new(self.storage, &self.ns, &self.db, &hop.edge_type);
            let tv = Traversal::new(&es);
            let mut next_ids: Vec<String> = Vec::new();

            for id in &current_ids {
                let edges = match hop.direction {
                    TraversalDirection::Out => tv.outgoing_typed(id, &hop.edge_type)?,
                    TraversalDirection::In => tv.incoming_typed(id, &hop.edge_type)?,
                };
                for edge in edges {
                    let dest = match hop.direction {
                        TraversalDirection::Out => edge.dst,
                        TraversalDirection::In => edge.src,
                    };
                    // Deduplicate destinations.
                    if !next_ids.contains(&dest) {
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

        let mut rows: Vec<BTreeMap<String, Value>> = Vec::new();
        for id in &current_ids {
            let Some(doc) = coll.get(id)? else {
                continue;
            };
            // Apply WHERE filter on the destination document.
            if let Some(clause) = filter {
                if !matches_where(&doc, clause) {
                    continue;
                }
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

        Ok(QueryResult::Rows(rows))
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
