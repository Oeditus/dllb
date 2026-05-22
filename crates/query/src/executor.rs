//! Query executor: maps parsed [`Statement`]s to concrete crate API calls.

use std::collections::BTreeMap;

use dllb_core::{RecordId, Result, Value};
use dllb_document::{Collection, Document};
use dllb_graph::{Edge, EdgeStore};
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
    pub fn run(&self, query: &str) -> Result<QueryResult> {
        let stmt = crate::parser::parse(query)?;
        self.execute(&stmt)
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
            Some(WhereClause::Eq { field, value }) => {
                let target = value.to_value();
                docs.into_iter()
                    .filter(|d| d.get(field) == Some(&target))
                    .collect()
            }
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
                };
                // Always include id.
                row.insert("id".into(), Value::String(format!("{table}:{}", doc.id.id)));
                row
            })
            .collect();

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
