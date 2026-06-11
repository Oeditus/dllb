//! Benchmarks for `dllb-graph`.
//!
//! Covers:
//! - Edge creation (`relate`) at 100 and 1K edges per batch
//! - Single-hop outgoing traversal on a star graph (fan-out 10 and 100)
//! - 2-hop typed walk on a linear chain of 100 nodes

use criterion::{BatchSize, BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use dllb_graph::{Direction, Edge, EdgeStore, HopSpec, Traversal};
use dllb_storage::db::DllbStorage;

// ---------------------------------------------------------------------------
// relate benchmarks
// ---------------------------------------------------------------------------

fn bench_relate(c: &mut Criterion) {
    let mut group = c.benchmark_group("graph/relate");

    for count in [100usize, 1_000] {
        group.bench_with_input(BenchmarkId::new("edges", count), &count, |b, &count| {
            b.iter_batched(
                || {
                    let dir = tempfile::tempdir().unwrap();
                    let store = DllbStorage::open(dir.path().join("bench.db")).unwrap();
                    let edges: Vec<Edge> = (0..count)
                        .map(|i| {
                            Edge::new(&format!("user:{i}"), "knows", &format!("user:{}", i + 1))
                        })
                        .collect();
                    (dir, store, edges)
                },
                |(dir, store, edges)| {
                    let es = EdgeStore::new(&store, "ns", "db", "rel");
                    for edge in &edges {
                        es.relate(black_box(edge)).unwrap();
                    }
                    (dir, store, edges) // deferred drop
                },
                BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// traversal benchmarks
// ---------------------------------------------------------------------------

fn bench_traverse(c: &mut Criterion) {
    let mut group = c.benchmark_group("graph/traverse");

    // Star graph: one hub connected to N leaves.
    // Measures prefix-scan latency as a function of fan-out.
    for fanout in [10usize, 100] {
        let dir = tempfile::tempdir().unwrap();
        let store = DllbStorage::open(dir.path().join("bench.db")).unwrap();
        let es = EdgeStore::new(&store, "ns", "db", "star");
        for i in 0..fanout {
            es.relate(&Edge::new("hub", "links", &format!("leaf:{i}")))
                .unwrap();
        }

        let traversal = Traversal::new(&es);
        group.bench_with_input(
            BenchmarkId::new("outgoing_fanout", fanout),
            &fanout,
            |b, _| {
                b.iter(|| black_box(traversal.outgoing(black_box("hub")).unwrap()));
            },
        );
    }

    // Linear chain: 0 -> 1 -> 2 -> ... -> 99.
    // 2-hop typed walk starting from node 0 produces 1 path ending at node 2.
    {
        let dir = tempfile::tempdir().unwrap();
        let store = DllbStorage::open(dir.path().join("bench.db")).unwrap();
        let es = EdgeStore::new(&store, "ns", "db", "chain");
        for i in 0..99usize {
            es.relate(&Edge::new(&format!("n{i}"), "next", &format!("n{}", i + 1)))
                .unwrap();
        }

        let traversal = Traversal::new(&es);
        let hops = vec![
            HopSpec {
                direction: Direction::Out,
                edge_type: Some("next".into()),
            },
            HopSpec {
                direction: Direction::Out,
                edge_type: Some("next".into()),
            },
        ];

        group.bench_function("walk_2hop", |b| {
            b.iter(|| black_box(traversal.walk(black_box("n0"), black_box(&hops)).unwrap()));
        });
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// neighbor-only retrieval benchmarks
// ---------------------------------------------------------------------------
//
// These exercise the optimized retrieval path used by the query engine:
// neighbor IDs are read straight from the sorted keys, with no edge-property
// deserialization and -- for the incoming direction -- no per-edge point
// lookup. Incoming fan-out is the headline case, since the previous
// `incoming`-based path issued one extra read transaction per edge.

fn bench_neighbors(c: &mut Criterion) {
    let mut group = c.benchmark_group("graph/neighbors");

    for fanout in [100usize, 1_000] {
        // Outgoing star: hub -> leaf:i
        let out_dir = tempfile::tempdir().unwrap();
        let out_store = DllbStorage::open(out_dir.path().join("bench.db")).unwrap();
        let out_es = EdgeStore::new(&out_store, "ns", "db", "out_star");
        // Incoming star: leaf:i -> hub  (so `hub` has `fanout` incoming edges)
        let in_dir = tempfile::tempdir().unwrap();
        let in_store = DllbStorage::open(in_dir.path().join("bench.db")).unwrap();
        let in_es = EdgeStore::new(&in_store, "ns", "db", "in_star");
        for i in 0..fanout {
            out_es
                .relate(&Edge::new("hub", "links", &format!("leaf:{i}")))
                .unwrap();
            in_es
                .relate(&Edge::new(&format!("leaf:{i}"), "links", "hub"))
                .unwrap();
        }

        let out_tv = Traversal::new(&out_es);
        group.bench_with_input(
            BenchmarkId::new("outgoing_neighbors", fanout),
            &fanout,
            |b, _| {
                b.iter(|| black_box(out_tv.outgoing_neighbors(black_box("hub")).unwrap()));
            },
        );

        let in_tv = Traversal::new(&in_es);
        group.bench_with_input(
            BenchmarkId::new("incoming_neighbors", fanout),
            &fanout,
            |b, _| {
                b.iter(|| black_box(in_tv.incoming_neighbors(black_box("hub")).unwrap()));
            },
        );
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// full edge-scan benchmark (community/component input)
// ---------------------------------------------------------------------------
//
// Compares the weighted scan (deserializes each edge's properties) against the
// weightless key-only scan that backs `GRAPH COMPONENTS`.

fn bench_scan_all(c: &mut Criterion) {
    let mut group = c.benchmark_group("graph/scan_all");

    let edge_count = 1_000usize;
    let dir = tempfile::tempdir().unwrap();
    let store = DllbStorage::open(dir.path().join("bench.db")).unwrap();
    let es = EdgeStore::new(&store, "ns", "db", "calls");
    for i in 0..edge_count {
        es.relate(
            &Edge::new(
                &format!("n{i}"),
                "calls",
                &format!("n{}", (i + 1) % edge_count),
            )
            .with_property("weight", dllb_core::Value::Float(1.0)),
        )
        .unwrap();
    }

    group.bench_function("weighted_1k", |b| {
        b.iter(|| black_box(es.scan_all_outgoing().unwrap()));
    });
    group.bench_function("weightless_1k", |b| {
        b.iter(|| black_box(es.scan_all_edges().unwrap()));
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_relate,
    bench_traverse,
    bench_neighbors,
    bench_scan_all
);
criterion_main!(benches);
