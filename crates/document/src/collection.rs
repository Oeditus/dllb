//! [`Collection`] -- the primary API for document CRUD operations.
//!
//! A collection is scoped to a namespace, database, and table. It holds
//! optional schema and index definitions and performs all operations
//! atomically via the underlying [`DllbStorage`].

use std::collections::BTreeMap;

use dllb_core::schema::TableDefinition;
use dllb_core::{Error, RecordId, Result, Value};
use dllb_storage::db::DllbStorage;
use dllb_storage::key;

use crate::document::Document;
use crate::index::{self, IndexDefinition};
use crate::serde::{deserialize, serialize};
use crate::validate::validate_document;

/// A single key/value put operation.
type PutOp = (Vec<u8>, Vec<u8>);
/// A batch of key/value put operations.
type PutOps = Vec<PutOp>;
/// A batch of keys to delete.
type DeleteOps = Vec<Vec<u8>>;

/// A document collection scoped to a namespace/database/table.
pub struct Collection<'s> {
    storage: &'s DllbStorage,
    ns: String,
    db: String,
    table: String,
    schema: Option<TableDefinition>,
    indexes: Vec<IndexDefinition>,
}

impl<'s> Collection<'s> {
    /// Create a new schemaless collection with no indexes.
    pub fn new(storage: &'s DllbStorage, ns: &str, db: &str, table: &str) -> Self {
        Self {
            storage,
            ns: ns.into(),
            db: db.into(),
            table: table.into(),
            schema: None,
            indexes: Vec::new(),
        }
    }

    /// Attach a schema for schemafull validation.
    pub fn with_schema(mut self, schema: TableDefinition) -> Self {
        self.schema = Some(schema);
        self
    }

    /// Register a secondary index.
    pub fn with_index(mut self, index: IndexDefinition) -> Self {
        self.indexes.push(index);
        self
    }

    // -------------------------------------------------------------------
    // Create
    // -------------------------------------------------------------------

    /// Create a document with an auto-generated UUID as the record ID.
    pub fn create(&self, doc: Document) -> Result<RecordId> {
        let id_str = doc.id.id.clone();
        self.create_inner(&id_str, doc)
    }

    /// Create a document with an explicit ID.
    pub fn create_with_id(&self, id: &str, mut doc: Document) -> Result<RecordId> {
        doc.id = RecordId::new(&self.table, id);
        self.create_inner(id, doc)
    }

    fn create_inner(&self, id: &str, doc: Document) -> Result<RecordId> {
        // Schema validation
        if let Some(schema) = &self.schema {
            validate_document(&doc, schema)?;
        }

        let doc_key = key::document_key(&self.ns, &self.db, &self.table, id);

        // Duplicate check
        if self.storage.contains(&doc_key)? {
            return Err(Error::Other(format!("record already exists: {id}")));
        }

        // Unique constraint checks
        for idx in &self.indexes {
            if idx.unique
                && let Some(field_name) = idx.fields.first()
                && let Some(value) = doc.fields.get(field_name)
            {
                index::check_unique_constraint(
                    self.storage,
                    &self.ns,
                    &self.db,
                    &self.table,
                    idx,
                    value,
                    id,
                )?;
            }
        }

        // Serialize
        let doc_bytes = serialize(&doc)?;

        // Build index entries
        let idx_entries =
            index::build_index_entries(&doc, &self.ns, &self.db, &self.table, &self.indexes)?;

        // Atomic batch write: document + all index entries
        let mut ops: Vec<(&[u8], &[u8])> = vec![(&doc_key, &doc_bytes)];
        for (k, v) in &idx_entries {
            ops.push((k.as_slice(), v.as_slice()));
        }
        self.storage.put_batch(&ops)?;

        Ok(doc.id)
    }

    /// Like [`create_with_id`](Self::create_with_id), but returns the KV
    /// operations without writing to storage.
    ///
    /// Returns `(record_id, put_ops)`. The caller is responsible for
    /// executing the puts (typically via `storage.put_batch`).
    pub fn create_to_ops(
        &self,
        id: &str,
        mut doc: Document,
    ) -> Result<(RecordId, PutOps)> {
        doc.id = RecordId::new(&self.table, id);

        if let Some(schema) = &self.schema {
            validate_document(&doc, schema)?;
        }

        let doc_key = key::document_key(&self.ns, &self.db, &self.table, id);
        let doc_bytes = serialize(&doc)?;

        let idx_entries =
            index::build_index_entries(&doc, &self.ns, &self.db, &self.table, &self.indexes)?;

        let mut ops: PutOps = Vec::with_capacity(1 + idx_entries.len());
        ops.push((doc_key, doc_bytes));
        for (k, v) in idx_entries {
            ops.push((k, v));
        }

        Ok((doc.id, ops))
    }

    /// Like [`upsert`](Self::upsert), but returns the KV operations without
    /// writing to storage.
    ///
    /// Returns `(record_id, was_created, put_ops, delete_ops)`. The caller
    /// must execute deletes before puts for correct index maintenance.
    pub fn upsert_to_ops(
        &self,
        id: &str,
        doc: Document,
        update_fields: BTreeMap<String, Value>,
    ) -> Result<(RecordId, bool, PutOps, DeleteOps)> {
        let doc_key = key::document_key(&self.ns, &self.db, &self.table, id);

        if self.storage.contains(&doc_key)? {
            // Record exists -- compute merge ops.
            let old = self
                .get(id)?
                .ok_or_else(|| Error::NotFound(format!("record not found: {id}")))?;

            let mut merged = old.fields.clone();
            for (k, v) in update_fields {
                merged.insert(k, v);
            }

            let new_doc = Document {
                id: old.id.clone(),
                fields: merged,
            };

            if let Some(schema) = &self.schema {
                validate_document(&new_doc, schema)?;
            }

            let new_bytes = serialize(&new_doc)?;

            let old_idx =
                index::build_index_entries(&old, &self.ns, &self.db, &self.table, &self.indexes)?;
            let new_idx = index::build_index_entries(
                &new_doc,
                &self.ns,
                &self.db,
                &self.table,
                &self.indexes,
            )?;

            let delete_ops: DeleteOps = old_idx.into_iter().map(|(k, _)| k).collect();

            let mut put_ops: PutOps = Vec::with_capacity(1 + new_idx.len());
            put_ops.push((doc_key, new_bytes));
            for (k, v) in new_idx {
                put_ops.push((k, v));
            }

            Ok((RecordId::new(&self.table, id), false, put_ops, delete_ops))
        } else {
            // No conflict -- compute create ops.
            let (record_id, put_ops) = self.create_to_ops(id, doc)?;
            Ok((record_id, true, put_ops, vec![]))
        }
    }

    // -------------------------------------------------------------------
    // Read
    // -------------------------------------------------------------------

    /// Get a document by its record ID string.
    pub fn get(&self, id: &str) -> Result<Option<Document>> {
        let doc_key = key::document_key(&self.ns, &self.db, &self.table, id);
        match self.storage.get(&doc_key)? {
            Some(bytes) => {
                let record_id = RecordId::new(&self.table, id);
                let doc = deserialize(record_id, &bytes)?;
                Ok(Some(doc))
            }
            None => Ok(None),
        }
    }

    /// Scan all documents in the collection.
    pub fn scan_all(&self) -> Result<Vec<Document>> {
        let prefix = key::table_prefix(&self.ns, &self.db, &self.table, key::tag::DOCUMENT);
        let entries = self.storage.prefix_scan(&prefix)?;
        let mut docs = Vec::with_capacity(entries.len());
        for (k, v) in entries {
            let parts = key::parse_key(&k)?;
            let id_str =
                std::str::from_utf8(parts.remainder).map_err(|e| Error::Storage(e.to_string()))?;
            let record_id = RecordId::new(&self.table, id_str);
            docs.push(deserialize(record_id, &v)?);
        }
        Ok(docs)
    }

    /// Count the number of documents in the collection.
    pub fn count(&self) -> Result<usize> {
        let prefix = key::table_prefix(&self.ns, &self.db, &self.table, key::tag::DOCUMENT);
        let entries = self.storage.prefix_scan(&prefix)?;
        Ok(entries.len())
    }

    // -------------------------------------------------------------------
    // Upsert (ON CONFLICT UPDATE)
    // -------------------------------------------------------------------

    /// Create a document or merge fields into it if it already exists.
    ///
    /// Returns `(record_id, was_created)` where `was_created` is `true` if the
    /// document was newly inserted, and `false` if an existing document was
    /// updated.
    pub fn upsert(
        &self,
        id: &str,
        doc: Document,
        update_fields: BTreeMap<String, Value>,
    ) -> Result<(RecordId, bool)> {
        let doc_key = key::document_key(&self.ns, &self.db, &self.table, id);

        if self.storage.contains(&doc_key)? {
            // Record exists -- merge the update fields.
            self.merge(id, update_fields)?;
            Ok((RecordId::new(&self.table, id), false))
        } else {
            // No conflict -- create normally.
            let created_id = self.create_inner(id, doc)?;
            Ok((created_id, true))
        }
    }

    // -------------------------------------------------------------------
    // Update
    // -------------------------------------------------------------------

    /// Replace all fields of an existing document.
    pub fn update(&self, id: &str, fields: BTreeMap<String, Value>) -> Result<()> {
        let old = self
            .get(id)?
            .ok_or_else(|| Error::NotFound(format!("record not found: {id}")))?;

        let new_doc = Document {
            id: old.id.clone(),
            fields,
        };

        if let Some(schema) = &self.schema {
            validate_document(&new_doc, schema)?;
        }

        self.replace_with_indexes(id, &old, &new_doc)
    }

    /// Merge fields into an existing document (partial update).
    ///
    /// Existing fields not in `fields` are preserved.
    pub fn merge(&self, id: &str, fields: BTreeMap<String, Value>) -> Result<()> {
        let old = self
            .get(id)?
            .ok_or_else(|| Error::NotFound(format!("record not found: {id}")))?;

        let mut merged = old.fields.clone();
        for (k, v) in fields {
            merged.insert(k, v);
        }

        let new_doc = Document {
            id: old.id.clone(),
            fields: merged,
        };

        if let Some(schema) = &self.schema {
            validate_document(&new_doc, schema)?;
        }

        self.replace_with_indexes(id, &old, &new_doc)
    }

    /// Internal: atomically replace a document and update its index entries.
    fn replace_with_indexes(&self, id: &str, old: &Document, new: &Document) -> Result<()> {
        let doc_key = key::document_key(&self.ns, &self.db, &self.table, id);
        let doc_bytes = serialize(new)?;

        // Old index entries to delete
        let old_idx =
            index::build_index_entries(old, &self.ns, &self.db, &self.table, &self.indexes)?;
        // New index entries to write
        let new_idx =
            index::build_index_entries(new, &self.ns, &self.db, &self.table, &self.indexes)?;

        // Delete old index entries
        let old_keys: Vec<&[u8]> = old_idx.iter().map(|(k, _)| k.as_slice()).collect();
        if !old_keys.is_empty() {
            self.storage.delete_batch(&old_keys)?;
        }

        // Write new document + new index entries
        let mut ops: Vec<(&[u8], &[u8])> = vec![(&doc_key, &doc_bytes)];
        for (k, v) in &new_idx {
            ops.push((k.as_slice(), v.as_slice()));
        }
        self.storage.put_batch(&ops)?;

        Ok(())
    }

    // -------------------------------------------------------------------
    // Delete
    // -------------------------------------------------------------------

    /// Delete a document by ID. Returns `true` if it existed.
    pub fn delete(&self, id: &str) -> Result<bool> {
        let doc = match self.get(id)? {
            Some(d) => d,
            None => return Ok(false),
        };

        let doc_key = key::document_key(&self.ns, &self.db, &self.table, id);

        // Index entries to delete
        let idx_entries =
            index::build_index_entries(&doc, &self.ns, &self.db, &self.table, &self.indexes)?;

        let mut keys_to_delete: Vec<&[u8]> = vec![&doc_key];
        for (k, _) in &idx_entries {
            keys_to_delete.push(k.as_slice());
        }
        self.storage.delete_batch(&keys_to_delete)?;

        Ok(true)
    }

    // -------------------------------------------------------------------
    // Index queries
    // -------------------------------------------------------------------

    /// Find documents by an index value.
    pub fn find_by_index(&self, index_name: &str, value: &Value) -> Result<Vec<Document>> {
        let ids = index::find_by_index(
            self.storage,
            &self.ns,
            &self.db,
            &self.table,
            index_name,
            value,
        )?;
        let mut docs = Vec::new();
        for id in ids {
            if let Some(doc) = self.get(&id)? {
                docs.push(doc);
            }
        }
        Ok(docs)
    }
}
