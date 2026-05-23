//! Benchmarks for `dllb-vector`.
//!
//! Covers:
//! - Distance metric throughput: cosine / euclidean / dot-product at 128D and 768D
//! - HNSW index build time: insert 1K and 10K vectors
//! - HNSW KNN-10 query latency on a pre-built index of 1K / 10K vectors
//! - Brute-force exact KNN-10 baseline on 1K vectors

use std::time::Duration;

use criterion::{BatchSize, BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use dllb_vector::{
    BruteForceIndex, DistanceMetric, HnswConfig, HnswIndex, VectorIndex,
    distance::{cosine_distance, dot_product, euclidean_distance},
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Deterministic pseudo-random vector of length `dim`.
///
/// Uses a simple LCG so no external RNG dependency is needed in benches.
fn gen_vector(seed: usize, dim: usize) -> Vec<f32> {
    let mut state = seed.wrapping_mul(6_364_136).wrapping_add(1_442_695);
    (0..dim)
        .map(|_| {
            state = state.wrapping_mul(6_364_136).wrapping_add(1_442_695);
            (state % 1_000) as f32 / 1_000.0 - 0.5 // [-0.5, 0.5)
        })
        .collect()
}

fn build_hnsw(count: usize, dim: usize, config: HnswConfig) -> HnswIndex {
    let mut index = HnswIndex::new(dim, DistanceMetric::Cosine, config);
    for i in 0..count {
        index.insert(&format!("v{i}"), gen_vector(i, dim));
    }
    index
}

fn build_brute_force(count: usize, dim: usize) -> BruteForceIndex {
    let mut index = BruteForceIndex::new(DistanceMetric::Cosine);
    for i in 0..count {
        index.insert(&format!("v{i}"), gen_vector(i, dim));
    }
    index
}

// ---------------------------------------------------------------------------
// Distance metric benchmarks
// ---------------------------------------------------------------------------

fn bench_distance(c: &mut Criterion) {
    let mut group = c.benchmark_group("vector/distance");

    for dim in [128usize, 768] {
        let a = gen_vector(1, dim);
        let b = gen_vector(2, dim);

        group.bench_with_input(BenchmarkId::new("cosine", dim), &dim, |bench, _| {
            bench.iter(|| black_box(cosine_distance(black_box(&a), black_box(&b))));
        });
        group.bench_with_input(BenchmarkId::new("euclidean", dim), &dim, |bench, _| {
            bench.iter(|| black_box(euclidean_distance(black_box(&a), black_box(&b))));
        });
        group.bench_with_input(BenchmarkId::new("dot_product", dim), &dim, |bench, _| {
            bench.iter(|| black_box(dot_product(black_box(&a), black_box(&b))));
        });
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// HNSW insert benchmarks
// ---------------------------------------------------------------------------

fn bench_hnsw_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("vector/hnsw_insert");
    // These benchmarks build an entire index per sample; limit samples to keep
    // total wall time reasonable.
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(30));

    const DIM: usize = 128;

    for count in [1_000usize, 10_000] {
        group.bench_with_input(BenchmarkId::new("vectors", count), &count, |b, &count| {
            let vectors: Vec<Vec<f32>> = (0..count).map(|i| gen_vector(i, DIM)).collect();

            b.iter_batched(
                || vectors.clone(),
                |vecs| {
                    let mut index =
                        HnswIndex::new(DIM, DistanceMetric::Cosine, HnswConfig::default());
                    for (i, v) in vecs.iter().enumerate() {
                        index.insert(&format!("v{i}"), v.clone());
                    }
                    black_box(index)
                },
                BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// HNSW search benchmarks
// ---------------------------------------------------------------------------

fn bench_hnsw_search(c: &mut Criterion) {
    let mut group = c.benchmark_group("vector/hnsw_search");

    const DIM: usize = 128;
    let query = gen_vector(99_999, DIM);

    for count in [1_000usize, 10_000] {
        let index = build_hnsw(count, DIM, HnswConfig::default());

        group.bench_with_input(BenchmarkId::new("knn10", count), &count, |b, _| {
            b.iter(|| black_box(index.search(black_box(&query), 10)));
        });
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// Brute-force baseline benchmarks
// ---------------------------------------------------------------------------

fn bench_brute_force(c: &mut Criterion) {
    let mut group = c.benchmark_group("vector/brute_force");

    const DIM: usize = 128;
    let query = gen_vector(99_999, DIM);

    for count in [1_000usize, 10_000] {
        let index = build_brute_force(count, DIM);

        group.bench_with_input(BenchmarkId::new("knn10", count), &count, |b, _| {
            b.iter(|| black_box(index.search(black_box(&query), 10)));
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_distance,
    bench_hnsw_insert,
    bench_hnsw_search,
    bench_brute_force
);
criterion_main!(benches);
