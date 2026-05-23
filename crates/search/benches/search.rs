//! Benchmarks for `dllb-search`.
//!
//! Covers:
//! - Indexing throughput: add 100 / 1K documents + commit
//! - BM25 query latency: top-10 results across three distinct query strings
//!   against a pre-built index of 1K documents

use criterion::{BatchSize, BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use dllb_search::{AnalyzerConfig, FtsIndex};

// ---------------------------------------------------------------------------
// Corpus helpers
// ---------------------------------------------------------------------------

/// Generate a realistic sentence for document `i`.
fn doc_text(i: usize) -> String {
    let topics = [
        "distributed consensus algorithms",
        "graph traversal and pathfinding",
        "vector embeddings and similarity search",
        "full-text indexing with BM25 scoring",
        "key-value storage with MVCC transactions",
        "HNSW approximate nearest neighbor",
        "Rust async runtime and tokio",
        "B-tree range scans and prefix queries",
    ];
    let topic = topics[i % topics.len()];
    format!(
        "Document {i}: {topic}. \
         This record discusses techniques related to {topic} in the context \
         of multi-model database systems. Entry number {i}."
    )
}

// ---------------------------------------------------------------------------
// index benchmarks
// ---------------------------------------------------------------------------

fn bench_index(c: &mut Criterion) {
    let mut group = c.benchmark_group("search/index");

    for count in [100usize, 1_000] {
        group.bench_with_input(BenchmarkId::new("documents", count), &count, |b, &count| {
            b.iter_batched(
                || {
                    let dir = tempfile::tempdir().unwrap();
                    let index =
                        FtsIndex::open_or_create(dir.path(), AnalyzerConfig::Default).unwrap();
                    (dir, index)
                },
                |(dir, index)| {
                    for i in 0..count {
                        index
                            .index_document(&format!("doc:{i}"), &doc_text(i))
                            .unwrap();
                    }
                    index.commit().unwrap();
                    (dir, index) // deferred drop
                },
                BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// query benchmarks
// ---------------------------------------------------------------------------

fn bench_query(c: &mut Criterion) {
    let mut group = c.benchmark_group("search/query");

    // Build the index once outside criterion.
    let dir = tempfile::tempdir().unwrap();
    let index = FtsIndex::open_or_create(dir.path(), AnalyzerConfig::Default).unwrap();
    for i in 0..1_000usize {
        index
            .index_document(&format!("doc:{i}"), &doc_text(i))
            .unwrap();
    }
    index.commit().unwrap();

    let queries = [
        "distributed consensus",
        "vector embeddings similarity",
        "BM25 scoring",
    ];

    for query in queries {
        group.bench_with_input(BenchmarkId::new("top10", query), &query, |b, &query| {
            b.iter(|| black_box(index.search(black_box(query), 10).unwrap()));
        });
    }

    group.finish();
}

criterion_group!(benches, bench_index, bench_query);
criterion_main!(benches);
