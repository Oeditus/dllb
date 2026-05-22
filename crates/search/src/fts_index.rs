//! [`FtsIndex`] -- wraps a single Tantivy index for one table/field.
//!
//! Each FtsIndex manages its own Tantivy schema (2 fields: `_id` + `_text`),
//! writer, and reader. BM25 scoring is used for ranking.

use std::path::Path;
use std::sync::Mutex;

use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{
    Field, IndexRecordOption, STORED, STRING, Schema, TextFieldIndexing, TextOptions,
    Value as TantivyValue,
};
use tantivy::{
    DocAddress, Index, IndexReader, IndexWriter, ReloadPolicy, Score, TantivyDocument, Term, doc,
};

use dllb_core::{Error, Result};

use crate::analyzer::{self, AnalyzerConfig};

/// A search result from a full-text query.
#[derive(Debug, Clone)]
pub struct SearchHit {
    /// The record ID of the matching document.
    pub id: String,
    /// BM25 relevance score.
    pub score: f32,
}

/// A full-text index for a single field of a single table.
pub struct FtsIndex {
    index: Index,
    reader: IndexReader,
    writer: Mutex<IndexWriter>,
    id_field: Field,
    text_field: Field,
}

impl FtsIndex {
    /// Open or create a Tantivy index at the given directory.
    pub fn open_or_create(path: &Path, config: AnalyzerConfig) -> Result<Self> {
        // Build text field options that use our custom analyzer.
        let text_indexing = TextFieldIndexing::default()
            .set_tokenizer(analyzer::ANALYZER_NAME)
            .set_index_option(IndexRecordOption::WithFreqsAndPositions);
        let text_options = TextOptions::default()
            .set_indexing_options(text_indexing)
            .set_stored();

        let mut schema_builder = Schema::builder();
        let id_field = schema_builder.add_text_field("_id", STRING | STORED);
        let text_field = schema_builder.add_text_field("_text", text_options);
        let schema = schema_builder.build();

        std::fs::create_dir_all(path).map_err(|e| Error::Storage(e.to_string()))?;
        let index = Index::create_in_dir(path, schema.clone())
            .or_else(|_| Index::open_in_dir(path))
            .map_err(|e| Error::Index(e.to_string()))?;

        // Register our analyzer so Tantivy can find it by name.
        let text_analyzer = analyzer::build_analyzer(&config);
        index
            .tokenizers()
            .register(analyzer::ANALYZER_NAME, text_analyzer);

        let writer = index
            .writer(15_000_000) // 15MB heap
            .map_err(|e| Error::Index(e.to_string()))?;

        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()
            .map_err(|e| Error::Index(e.to_string()))?;

        Ok(Self {
            index,
            reader,
            writer: Mutex::new(writer),
            id_field,
            text_field,
        })
    }

    /// Index a document's text content.
    pub fn index_document(&self, id: &str, text: &str) -> Result<()> {
        let writer = self
            .writer
            .lock()
            .map_err(|e| Error::Index(e.to_string()))?;
        writer
            .add_document(doc!(
                self.id_field => id,
                self.text_field => text,
            ))
            .map_err(|e| Error::Index(e.to_string()))?;
        Ok(())
    }

    /// Delete a document by its record ID.
    pub fn delete_document(&self, id: &str) -> Result<()> {
        let writer = self
            .writer
            .lock()
            .map_err(|e| Error::Index(e.to_string()))?;
        let term = Term::from_field_text(self.id_field, id);
        writer.delete_term(term);
        Ok(())
    }

    /// Update a document (delete + re-index).
    pub fn update_document(&self, id: &str, text: &str) -> Result<()> {
        self.delete_document(id)?;
        self.index_document(id, text)
    }

    /// Search for documents matching a query string.
    ///
    /// Returns results ranked by BM25 score (highest first).
    pub fn search(&self, query_str: &str, limit: usize) -> Result<Vec<SearchHit>> {
        let searcher = self.reader.searcher();
        let query_parser = QueryParser::for_index(&self.index, vec![self.text_field]);
        let query = query_parser
            .parse_query(query_str)
            .map_err(|e| Error::Query(e.to_string()))?;

        let top_docs: Vec<(Score, DocAddress)> = searcher
            .search(&query, &TopDocs::with_limit(limit).order_by_score())
            .map_err(|e| Error::Query(e.to_string()))?;

        let mut hits = Vec::with_capacity(top_docs.len());
        for (score, doc_address) in top_docs {
            let doc: TantivyDocument = searcher
                .doc(doc_address)
                .map_err(|e| Error::Query(e.to_string()))?;
            if let Some(id_value) = doc.get_first(self.id_field)
                && let Some(id_str) = id_value.as_str()
            {
                hits.push(SearchHit {
                    id: id_str.to_string(),
                    score,
                });
            }
        }
        Ok(hits)
    }

    /// Commit pending writes to disk and reload the reader.
    pub fn commit(&self) -> Result<()> {
        let mut writer = self
            .writer
            .lock()
            .map_err(|e| Error::Index(e.to_string()))?;
        writer.commit().map_err(|e| Error::Index(e.to_string()))?;
        self.reader
            .reload()
            .map_err(|e| Error::Index(e.to_string()))?;
        Ok(())
    }
}
