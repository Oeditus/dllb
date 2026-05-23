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

criterion_group!(benches, bench_relate, bench_traverse);
criterion_main!(benches);
