//! Benchmarks for `dllb-document`.
//!
//! Covers:
//! - Sequential document creation (100 and 1K documents per batch)
//! - Full-table scan (`scan_all`) over a pre-populated collection (100 / 1K docs)

use criterion::{BatchSize, BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use dllb_core::{RecordId, Value};
use dllb_document::{Collection, Document};
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

criterion_group!(benches, bench_create, bench_scan);
criterion_main!(benches);
