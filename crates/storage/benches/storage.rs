//! Benchmarks for `dllb-storage`.
//!
//! Covers the core hot paths:
//! - Single `put` (write transaction overhead + B-tree insert)
//! - Batch `put_batch` at 1K and 10K entries
//! - Single `get` (read transaction overhead + B-tree lookup)
//! - `prefix_scan` over 100 and 1K matching entries

use criterion::{BatchSize, BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use dllb_storage::db::DllbStorage;

// ---------------------------------------------------------------------------
// put benchmarks
// ---------------------------------------------------------------------------

fn bench_put(c: &mut Criterion) {
    let mut group = c.benchmark_group("storage/put");

    // Single put: overwrite the same key on every iteration to isolate
    // write-transaction + B-tree-update latency without unbounded DB growth.
    {
        let dir = tempfile::tempdir().unwrap();
        let store = DllbStorage::open(dir.path().join("bench.db")).unwrap();
        group.bench_function("single", |b| {
            b.iter(|| {
                store
                    .put(black_box(b"bench_key"), black_box(b"bench_value"))
                    .unwrap();
            });
        });
    }

    // Batch put: one `put_batch` call with N pairs on a fresh database.
    // Measures the cost of a single large atomic write transaction.
    for count in [1_000usize, 10_000] {
        group.bench_with_input(BenchmarkId::new("batch", count), &count, |b, &count| {
            b.iter_batched(
                || {
                    let dir = tempfile::tempdir().unwrap();
                    let store = DllbStorage::open(dir.path().join("bench.db")).unwrap();
                    let pairs: Vec<(Vec<u8>, Vec<u8>)> = (0..count)
                        .map(|i| (format!("key_{i:010}").into_bytes(), b"value".to_vec()))
                        .collect();
                    (dir, store, pairs)
                },
                |(dir, store, pairs)| {
                    let ops: Vec<(&[u8], &[u8])> = pairs
                        .iter()
                        .map(|(k, v)| (k.as_slice(), v.as_slice()))
                        .collect();
                    store.put_batch(black_box(&ops)).unwrap();
                    (dir, store, pairs) // deferred drop keeps TempDir alive during timing
                },
                BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// get benchmarks
// ---------------------------------------------------------------------------

fn bench_get(c: &mut Criterion) {
    let mut group = c.benchmark_group("storage/get");

    let dir = tempfile::tempdir().unwrap();
    let store = DllbStorage::open(dir.path().join("bench.db")).unwrap();

    // Pre-populate 1000 entries so the B-tree is non-trivial.
    let pairs: Vec<(Vec<u8>, Vec<u8>)> = (0..1_000usize)
        .map(|i| {
            (
                format!("key_{i:010}").into_bytes(),
                format!("value_{i}").into_bytes(),
            )
        })
        .collect();
    let ops: Vec<(&[u8], &[u8])> = pairs
        .iter()
        .map(|(k, v)| (k.as_slice(), v.as_slice()))
        .collect();
    store.put_batch(&ops).unwrap();

    // Read the middle entry to avoid branch-prediction advantages of
    // the first or last key in sorted order.
    group.bench_function("single", |b| {
        b.iter(|| store.get(black_box(b"key_0000000500")).unwrap());
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// scan benchmarks
// ---------------------------------------------------------------------------

fn bench_scan(c: &mut Criterion) {
    let mut group = c.benchmark_group("storage/scan");

    for count in [100usize, 1_000] {
        let dir = tempfile::tempdir().unwrap();
        let store = DllbStorage::open(dir.path().join("bench.db")).unwrap();

        // Insert N entries all sharing the same prefix.
        let pairs: Vec<(Vec<u8>, Vec<u8>)> = (0..count)
            .map(|i| (format!("pfx:{i:010}").into_bytes(), b"value".to_vec()))
            .collect();
        let ops: Vec<(&[u8], &[u8])> = pairs
            .iter()
            .map(|(k, v)| (k.as_slice(), v.as_slice()))
            .collect();
        store.put_batch(&ops).unwrap();

        group.bench_with_input(BenchmarkId::new("prefix_scan", count), &count, |b, _| {
            b.iter(|| store.prefix_scan(black_box(b"pfx:")).unwrap());
        });
    }

    group.finish();
}

criterion_group!(benches, bench_put, bench_get, bench_scan);
criterion_main!(benches);
