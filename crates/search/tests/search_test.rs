//! Integration tests for dllb-search.

use dllb_search::{AnalyzerConfig, FtsIndex, FtsManager, Language};

fn temp_index(config: AnalyzerConfig) -> (tempfile::TempDir, FtsIndex) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("idx");
    let idx = FtsIndex::open_or_create(&path, config).unwrap();
    (dir, idx)
}

// -------------------------------------------------------------------
// FtsIndex basics
// -------------------------------------------------------------------

#[test]
fn index_and_search() {
    let (_dir, idx) = temp_index(AnalyzerConfig::Default);

    idx.index_document("doc1", "the quick brown fox jumps over the lazy dog")
        .unwrap();
    idx.index_document("doc2", "a fast brown car drove past the sleeping cat")
        .unwrap();
    idx.index_document("doc3", "quantum computing is the future of technology")
        .unwrap();
    idx.commit().unwrap();

    let hits = idx.search("brown fox", 10).unwrap();
    assert!(!hits.is_empty());
    // doc1 should be the top hit (has both "brown" and "fox").
    assert_eq!(hits[0].id, "doc1");
}

#[test]
fn bm25_ranking() {
    let (_dir, idx) = temp_index(AnalyzerConfig::Default);

    // doc1 has "rust" once in a long text.
    idx.index_document(
        "doc1",
        "this is a long document about many topics not related to rust programming at all really",
    )
    .unwrap();
    // doc2 has "rust" multiple times.
    idx.index_document(
        "doc2",
        "rust rust rust is a great language for systems programming in rust",
    )
    .unwrap();
    idx.commit().unwrap();

    let hits = idx.search("rust", 10).unwrap();
    assert_eq!(hits.len(), 2);
    // doc2 should rank higher (higher term frequency).
    assert_eq!(hits[0].id, "doc2");
    assert!(hits[0].score > hits[1].score);
}

#[test]
fn delete_document() {
    let (_dir, idx) = temp_index(AnalyzerConfig::Default);

    idx.index_document("doc1", "hello world").unwrap();
    idx.commit().unwrap();
    assert_eq!(idx.search("hello", 10).unwrap().len(), 1);

    idx.delete_document("doc1").unwrap();
    idx.commit().unwrap();
    assert!(idx.search("hello", 10).unwrap().is_empty());
}

#[test]
fn update_document() {
    let (_dir, idx) = temp_index(AnalyzerConfig::Default);

    idx.index_document("doc1", "old content about databases")
        .unwrap();
    idx.commit().unwrap();
    assert_eq!(idx.search("databases", 10).unwrap().len(), 1);

    idx.update_document("doc1", "new content about compilers")
        .unwrap();
    idx.commit().unwrap();

    // Old text should not match.
    assert!(idx.search("databases", 10).unwrap().is_empty());
    // New text should match.
    assert_eq!(idx.search("compilers", 10).unwrap().len(), 1);
}

#[test]
fn search_no_results() {
    let (_dir, idx) = temp_index(AnalyzerConfig::Default);

    idx.index_document("doc1", "hello world").unwrap();
    idx.commit().unwrap();

    let hits = idx.search("nonexistent", 10).unwrap();
    assert!(hits.is_empty());
}

#[test]
fn english_stemming() {
    let (_dir, idx) = temp_index(AnalyzerConfig::Language(Language::English));

    idx.index_document("doc1", "the runners were running quickly")
        .unwrap();
    idx.commit().unwrap();

    // "run" should match "running" and "runners" via stemming.
    let hits = idx.search("run", 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, "doc1");
}

// -------------------------------------------------------------------
// FtsManager
// -------------------------------------------------------------------

#[test]
fn manager_multi_index_isolation() {
    let dir = tempfile::tempdir().unwrap();
    let mut mgr = FtsManager::new(dir.path());

    mgr.define_index("article", "title", AnalyzerConfig::Default)
        .unwrap();
    mgr.define_index("article", "body", AnalyzerConfig::Default)
        .unwrap();

    mgr.index_document("article", "title", "doc1", "Graph Databases Explained")
        .unwrap();
    mgr.index_document(
        "article",
        "body",
        "doc1",
        "This is the body text about graphs",
    )
    .unwrap();
    mgr.commit_all().unwrap();

    // Searching title should find "Graph" but not "body".
    let title_hits = mgr.search("article", "title", "graph", 10).unwrap();
    assert_eq!(title_hits.len(), 1);

    // Searching body should find "graphs" but not "explained".
    let body_hits = mgr.search("article", "body", "graphs", 10).unwrap();
    assert_eq!(body_hits.len(), 1);

    let no_hits = mgr.search("article", "title", "body", 10).unwrap();
    assert!(no_hits.is_empty());
}

#[test]
fn manager_undefined_index_errors() {
    let dir = tempfile::tempdir().unwrap();
    let mgr = FtsManager::new(dir.path());

    let err = mgr.search("nosuch", "field", "query", 10).unwrap_err();
    assert!(err.to_string().contains("no FTS index defined"));
}

#[test]
fn persistence_across_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("persist_idx");

    // First open: create and index.
    {
        let idx = FtsIndex::open_or_create(&path, AnalyzerConfig::Default).unwrap();
        idx.index_document("doc1", "persistent data survives")
            .unwrap();
        idx.commit().unwrap();
    }

    // Second open: data should still be there.
    {
        let idx = FtsIndex::open_or_create(&path, AnalyzerConfig::Default).unwrap();
        let hits = idx.search("persistent", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "doc1");
    }
}
