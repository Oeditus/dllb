//! Benchmarks for `dllb-document`.
//!
//! Covers:
//! - Sequential document creation (100 and 1K documents per batch)
//! - Full-table scan (`scan_all`) over a pre-populated collection (100 / 1K docs)

use criterion::{BatchSize, BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use dllb_core::{RecordId, Value};
use dllb_document::{Collection, Document, IndexDefinition};
use dllb_storage::db::DllbStorage;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_doc(table: &str, i: usize) -> Document {
    Document::new(RecordId::new(table, format!("u{i:08}")))
        .with_field("name", Value::String(format!("User {i}")))
        .with_field("age", Value::Int(20 + (i % 60) as i64))
        .with_field("active", Value::Bool(i.is_multiple_of(2)))
}

// ---------------------------------------------------------------------------
// create benchmarks
// ---------------------------------------------------------------------------

fn bench_create(c: &mut Criterion) {
    let mut group = c.benchmark_group("document/create");

    for count in [100usize, 1_000] {
        group.bench_with_input(
            BenchmarkId::new("sequential", count),
            &count,
            |b, &count| {
                b.iter_batched(
                    || {
                        let dir = tempfile::tempdir().unwrap();
                        let store = DllbStorage::open(dir.path().join("bench.db")).unwrap();
                        (dir, store)
                    },
                    |(dir, store)| {
                        let coll = Collection::new(&store, "ns", "db", "users");
                        for i in 0..count {
                            coll.create(make_doc("users", i)).unwrap();
                        }
                        (dir, store) // deferred drop
                    },
                    BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// scan benchmarks
// ---------------------------------------------------------------------------

fn bench_scan(c: &mut Criterion) {
    let mut group = c.benchmark_group("document/scan");

    for count in [100usize, 1_000] {
        let dir = tempfile::tempdir().unwrap();
        let store = DllbStorage::open(dir.path().join("bench.db")).unwrap();
        let coll = Collection::new(&store, "ns", "db", "users");

        for i in 0..count {
            coll.create(make_doc("users", i)).unwrap();
        }

        group.bench_with_input(BenchmarkId::new("scan_all", count), &count, |b, _| {
            b.iter(|| black_box(coll.scan_all().unwrap()));
        });
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// indexed lookup benchmarks
// ---------------------------------------------------------------------------
//
// Contrasts answering an equality predicate by a full scan + filter (reads and
// deserializes every document) against probing a secondary index (reads only
// the matching index entries). This is the access path `SELECT ... WHERE` and
// `COUNT ... WHERE` now take when an index is defined.

fn bench_indexed_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("document/indexed_lookup");

    for count in [1_000usize, 10_000] {
        let dir = tempfile::tempdir().unwrap();
        let store = DllbStorage::open(dir.path().join("bench.db")).unwrap();
        let coll = Collection::new(&store, "ns", "db", "users").with_index(IndexDefinition {
            name: "by_age".into(),
            fields: vec!["age".into()],
            unique: false,
        });
        for i in 0..count {
            coll.create(make_doc("users", i)).unwrap();
        }
        // `make_doc` assigns age = 20 + (i % 60), so this value is present.
        let target = Value::Int(30);

        group.bench_with_input(
            BenchmarkId::new("full_scan_filter", count),
            &count,
            |b, _| {
                b.iter(|| {
                    let docs = coll.scan_all().unwrap();
                    let n = docs
                        .iter()
                        .filter(|d| d.get("age") == Some(&target))
                        .count();
                    black_box(n)
                });
            },
        );
        group.bench_with_input(BenchmarkId::new("index_probe", count), &count, |b, _| {
            b.iter(|| black_box(coll.find_ids_by_index("by_age", &target).unwrap()));
        });
    }

    group.finish();
}

criterion_group!(benches, bench_create, bench_scan, bench_indexed_lookup);
criterion_main!(benches);
