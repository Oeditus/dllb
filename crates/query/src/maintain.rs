//! Maintenance of the out-of-redb search structures (Tantivy full-text and
//! in-memory HNSW) after document mutations.
//!
//! B-tree indexes live in the redb keyspace and are maintained by
//! `dllb-document`; full-text and vector indexes live in [`SearchServices`]
//! and must be kept in step with every create, update, delete, and batch.
//! That lifecycle logic lives here rather than in the executor so the executor
//! can stay a statement dispatcher.

use std::sync::Arc;

use dllb_core::Result;
use dllb_document::{Collection, Document, IndexDefinition, IndexKind};
use dllb_storage::db::DllbStorage;

use crate::search::{self, SearchServices};

/// Keeps the full-text and vector indexes in step with stored documents.
///
/// All methods are no-ops when search services are disabled, so callers do
/// not need to branch on that themselves.
pub struct Maintainer<'a, 's> {
    storage: &'s DllbStorage,
    ns: &'a str,
    db: &'a str,
    services: Option<&'a Arc<SearchServices>>,
}

impl<'a, 's> Maintainer<'a, 's> {
    /// Build a maintainer for one namespace/database.
    ///
    /// `services` is `None` when the executor was constructed without them;
    /// every method then degrades to `Ok(())`.
    pub fn new(
        storage: &'s DllbStorage,
        ns: &'a str,
        db: &'a str,
        services: Option<&'a Arc<SearchServices>>,
    ) -> Self {
        Self {
            storage,
            ns,
            db,
            services,
        }
    }

    /// Whether search services are available.
    pub fn enabled(&self) -> bool {
        self.services.is_some()
    }

    /// Ensure a vector index is present in memory, rebuilding it from stored
    /// documents when necessary (HNSW is in-memory only, so a fresh process
    /// starts empty).
    pub fn ensure_vector_loaded(
        &self,
        table: &str,
        def: &IndexDefinition,
        dim: usize,
        metric: &str,
    ) -> Result<()> {
        let Some(services) = &self.services else {
            return Ok(());
        };
        let parsed_metric = search::parse_metric(metric)?;

        // Atomically fetch or create the entry to prevent race conditions on setup.
        let entry = services.get_or_create_vector(&def.name, dim, parsed_metric)?;

        // If it is already loaded, we can return immediately.
        if entry.loaded.load(std::sync::atomic::Ordering::SeqCst) {
            return Ok(());
        }

        // Lock to ensure only one thread performs the rebuild backfill.
        let _guard = entry
            .rebuild_lock
            .lock()
            .map_err(|_| dllb_core::Error::Index("vector rebuild lock poisoned".into()))?;

        // Double-check after acquiring lock.
        if entry.loaded.load(std::sync::atomic::Ordering::SeqCst) {
            return Ok(());
        }

        let Some(field) = def.fields.first() else {
            entry
                .loaded
                .store(true, std::sync::atomic::Ordering::SeqCst);
            return Ok(());
        };

        let coll = Collection::new(self.storage, self.ns, self.db, table);
        for doc in coll.scan_all()? {
            if let Some(v) = doc.fields.get(field).and_then(search::value_to_vector) {
                services.vector_insert(&def.name, &doc.id.id, v)?;
            }
        }

        entry
            .loaded
            .store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }

    /// Maintain full-text/vector indexes after a single-document mutation.
    ///
    /// `doc` is the post-write document for a create/update, or `None` for a
    /// delete. `defs` is the table's already-loaded catalog (so this adds no
    /// extra scan). No-op when search services are disabled. Full-text writes
    /// are buffered; the caller must call [`commit`](Self::commit).
    pub fn maintain(
        &self,
        table: &str,
        id: &str,
        doc: Option<&Document>,
        defs: &[IndexDefinition],
    ) -> Result<()> {
        let Some(services) = &self.services else {
            return Ok(());
        };
        for def in defs {
            let Some(field) = def.fields.first() else {
                continue;
            };
            match &def.kind {
                IndexKind::FullText { analyzer } => {
                    services.ensure_fulltext(table, field, analyzer)?;
                    match doc
                        .and_then(|d| d.fields.get(field))
                        .and_then(search::value_to_text)
                    {
                        Some(text) => services.fts_index(table, field, id, text)?,
                        None => services.fts_delete(table, field, id)?,
                    }
                }
                IndexKind::Vector { dim, metric } => {
                    match doc
                        .and_then(|d| d.fields.get(field))
                        .and_then(search::value_to_vector)
                    {
                        Some(v) => {
                            self.ensure_vector_loaded(table, def, *dim, metric)?;
                            services.vector_insert(&def.name, id, v)?;
                        }
                        // A delete (or a row that no longer carries a vector)
                        // need only touch an already-loaded index; an unloaded
                        // one reflects storage when next rebuilt.
                        None => {
                            if services.vector_exists(&def.name)? {
                                services.vector_remove(&def.name, id)?;
                            }
                        }
                    }
                }
                IndexKind::BTree => {}
            }
        }
        Ok(())
    }

    /// Commit buffered full-text writes so prior maintenance becomes visible
    /// to searches. No-op when services are disabled.
    pub fn commit(&self) -> Result<()> {
        if let Some(services) = &self.services {
            services.fts_commit()?;
        }
        Ok(())
    }

    /// `true` when a vector index of this name is registered in memory.
    pub fn vector_exists(&self, name: &str) -> Result<bool> {
        match self.services {
            Some(services) => services.vector_exists(name),
            None => Ok(false),
        }
    }
}
