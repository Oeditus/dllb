//! Integration tests for dllb-vector.

use dllb_vector::{BruteForceIndex, DistanceMetric, HnswConfig, HnswIndex, VectorIndex};

fn random_vectors(n: usize, dim: usize) -> Vec<(String, Vec<f32>)> {
    use rand::Rng;
    let mut rng = rand::rng();
    (0..n)
        .map(|i| {
            let vec: Vec<f32> = (0..dim).map(|_| rng.random::<f32>()).collect();
            (format!("v{i}"), vec)
        })
        .collect()
}

// -------------------------------------------------------------------
// Brute-force
// -------------------------------------------------------------------

#[test]
fn brute_force_exact_knn() {
    let vectors = random_vectors(100, 32);
    let mut idx = BruteForceIndex::new(DistanceMetric::Euclidean);

    for (id, vec) in &vectors {
        idx.insert(id, vec.clone());
    }
    assert_eq!(idx.len(), 100);

    // Search for the first vector -- it should be its own nearest neighbor.
    let query = &vectors[0].1;
    let hits = idx.search(query, 5);
    assert_eq!(hits.len(), 5);
    assert_eq!(hits[0].id, "v0");
    assert!(hits[0].distance < 1e-5); // distance to itself ~ 0
}

#[test]
fn brute_force_remove() {
    let mut idx = BruteForceIndex::new(DistanceMetric::Cosine);
    idx.insert("a", vec![1.0, 0.0]);
    idx.insert("b", vec![0.0, 1.0]);

    assert!(idx.remove("a"));
    assert!(!idx.remove("a")); // already removed
    assert_eq!(idx.len(), 1);

    let hits = idx.search(&[1.0, 0.0], 10);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, "b");
}

#[test]
fn brute_force_empty_search() {
    let idx = BruteForceIndex::new(DistanceMetric::Euclidean);
    let hits = idx.search(&[1.0, 2.0, 3.0], 10);
    assert!(hits.is_empty());
}

// -------------------------------------------------------------------
// HNSW
// -------------------------------------------------------------------

#[test]
fn hnsw_empty_search() {
    let idx = HnswIndex::new(32, DistanceMetric::Cosine, HnswConfig::default());
    let hits = idx.search(&[0.0; 32], 10);
    assert!(hits.is_empty());
}

#[test]
fn hnsw_single_vector() {
    let mut idx = HnswIndex::new(3, DistanceMetric::Euclidean, HnswConfig::default());
    idx.insert("only", vec![1.0, 2.0, 3.0]);
    let hits = idx.search(&[1.0, 2.0, 3.0], 1);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, "only");
}

#[test]
fn hnsw_remove() {
    let mut idx = HnswIndex::new(3, DistanceMetric::Euclidean, HnswConfig::default());
    idx.insert("a", vec![1.0, 0.0, 0.0]);
    idx.insert("b", vec![0.0, 1.0, 0.0]);

    assert!(idx.remove("a"));
    assert_eq!(idx.len(), 1);

    let hits = idx.search(&[1.0, 0.0, 0.0], 10);
    // "a" should not appear (deleted).
    assert!(hits.iter().all(|h| h.id != "a"));
}

#[test]
fn hnsw_recall_500_vectors() {
    let dim = 32;
    let n = 500;
    let k = 10;
    let vectors = random_vectors(n, dim);

    // Build brute-force baseline.
    let mut bf = BruteForceIndex::new(DistanceMetric::Cosine);
    for (id, vec) in &vectors {
        bf.insert(id, vec.clone());
    }

    // Build HNSW.
    let config = HnswConfig {
        m: 16,
        ef_construction: 100,
        max_layers: 8,
    };
    let mut hnsw = HnswIndex::new(dim, DistanceMetric::Cosine, config);
    for (id, vec) in &vectors {
        hnsw.insert(id, vec.clone());
    }

    // Measure recall over 20 random queries.
    let queries = random_vectors(20, dim);
    let mut total_recall = 0.0;
    for (_, query) in &queries {
        let bf_results: Vec<String> = bf.search(query, k).iter().map(|h| h.id.clone()).collect();
        let hnsw_results: Vec<String> = hnsw
            .search_ef(query, k, 50)
            .iter()
            .map(|h| h.id.clone())
            .collect();

        let hits: usize = hnsw_results
            .iter()
            .filter(|id| bf_results.contains(id))
            .count();
        total_recall += hits as f64 / k as f64;
    }
    let avg_recall = total_recall / 20.0;
    assert!(
        avg_recall >= 0.6,
        "HNSW recall too low: {avg_recall:.2} (expected >= 0.6)"
    );
}

#[test]
fn hnsw_recall_1000_vectors() {
    let dim = 32;
    let n = 1000;
    let k = 10;
    let vectors = random_vectors(n, dim);

    let mut bf = BruteForceIndex::new(DistanceMetric::Euclidean);
    for (id, vec) in &vectors {
        bf.insert(id, vec.clone());
    }

    let config = HnswConfig {
        m: 16,
        ef_construction: 200,
        max_layers: 10,
    };
    let mut hnsw = HnswIndex::new(dim, DistanceMetric::Euclidean, config);
    for (id, vec) in &vectors {
        hnsw.insert(id, vec.clone());
    }

    let queries = random_vectors(20, dim);
    let mut total_recall = 0.0;
    for (_, query) in &queries {
        let bf_results: Vec<String> = bf.search(query, k).iter().map(|h| h.id.clone()).collect();
        let hnsw_results: Vec<String> = hnsw
            .search_ef(query, k, 100)
            .iter()
            .map(|h| h.id.clone())
            .collect();

        let hits: usize = hnsw_results
            .iter()
            .filter(|id| bf_results.contains(id))
            .count();
        total_recall += hits as f64 / k as f64;
    }
    let avg_recall = total_recall / 20.0;
    assert!(
        avg_recall >= 0.5,
        "HNSW recall too low: {avg_recall:.2} (expected >= 0.5)"
    );
}

// -------------------------------------------------------------------
// Multiple metrics
// -------------------------------------------------------------------

#[test]
fn different_metrics_different_ranking() {
    let mut cosine_idx = BruteForceIndex::new(DistanceMetric::Cosine);
    let mut euclidean_idx = BruteForceIndex::new(DistanceMetric::Euclidean);

    let vecs = vec![
        ("a", vec![1.0, 0.0]),
        ("b", vec![0.5, 0.5]),
        ("c", vec![10.0, 0.0]), // same direction as "a" but much larger
    ];
    for (id, v) in &vecs {
        cosine_idx.insert(id, v.clone());
        euclidean_idx.insert(id, v.clone());
    }

    let query = &[1.0, 0.0];

    let cos_hits = cosine_idx.search(query, 3);
    let euc_hits = euclidean_idx.search(query, 3);

    // Cosine: "a" and "c" should be equally close (same direction).
    // Euclidean: "a" is closest, "c" is far (magnitude 10).
    assert!(cos_hits[0].id == "a" || cos_hits[0].id == "c");
    assert_eq!(euc_hits[0].id, "a");
}
