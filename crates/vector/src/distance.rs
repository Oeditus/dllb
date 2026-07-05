//! Distance metrics for vector similarity search.
//!
//! All metrics return a non-negative distance where **lower is more similar**.
//! - Cosine distance = 1.0 - cosine_similarity (range [0, 2])
//! - Euclidean distance = L2 norm of the difference (range [0, inf))
//! - Dot product distance = -dot(a, b) (lower = higher similarity)

/// Supported distance metrics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DistanceMetric {
    Cosine,
    Euclidean,
    DotProduct,
}

/// Compute distance between two vectors using the given metric.
pub fn distance(a: &[f32], b: &[f32], metric: DistanceMetric) -> f32 {
    match metric {
        DistanceMetric::Cosine => cosine_distance(a, b),
        DistanceMetric::Euclidean => euclidean_distance(a, b),
        DistanceMetric::DotProduct => dot_product_distance(a, b),
    }
}

/// Cosine distance = 1.0 - cosine_similarity.
///
/// Returns 0.0 for identical directions, 1.0 for orthogonal, 2.0 for opposite.
/// Uses chunked accumulation (8-wide) to enable compiler auto-vectorization.
pub fn cosine_distance(a: &[f32], b: &[f32]) -> f32 {
    let chunks_a = a.chunks_exact(8);
    let chunks_b = b.chunks_exact(8);
    let remainder_a = chunks_a.remainder();
    let remainder_b = chunks_b.remainder();

    let mut dot_acc = [0.0f32; 8];
    let mut norm_a_acc = [0.0f32; 8];
    let mut norm_b_acc = [0.0f32; 8];

    for (ca, cb) in chunks_a.zip(chunks_b) {
        for i in 0..8 {
            dot_acc[i] += ca[i] * cb[i];
            norm_a_acc[i] += ca[i] * ca[i];
            norm_b_acc[i] += cb[i] * cb[i];
        }
    }

    let mut dot: f32 = dot_acc.iter().sum();
    let mut norm_a_sum: f32 = norm_a_acc.iter().sum();
    let mut norm_b_sum: f32 = norm_b_acc.iter().sum();

    for i in 0..remainder_a.len() {
        dot += remainder_a[i] * remainder_b[i];
        norm_a_sum += remainder_a[i] * remainder_a[i];
        norm_b_sum += remainder_b[i] * remainder_b[i];
    }

    let denom = norm_a_sum.sqrt() * norm_b_sum.sqrt();
    if denom == 0.0 {
        return 1.0; // zero vectors treated as orthogonal
    }
    1.0 - (dot / denom)
}

/// Euclidean (L2) distance.
/// Uses chunked accumulation (8-wide) to enable compiler auto-vectorization.
pub fn euclidean_distance(a: &[f32], b: &[f32]) -> f32 {
    let chunks_a = a.chunks_exact(8);
    let chunks_b = b.chunks_exact(8);
    let remainder_a = chunks_a.remainder();
    let remainder_b = chunks_b.remainder();

    let mut sum = [0.0f32; 8];
    for (ca, cb) in chunks_a.zip(chunks_b) {
        for i in 0..8 {
            let d = ca[i] - cb[i];
            sum[i] += d * d;
        }
    }

    let mut total: f32 = sum.iter().sum();
    for i in 0..remainder_a.len() {
        let d = remainder_a[i] - remainder_b[i];
        total += d * d;
    }
    total.sqrt()
}

/// Dot product of two vectors.
/// Uses chunked accumulation (8-wide) to enable compiler auto-vectorization.
pub fn dot_product(a: &[f32], b: &[f32]) -> f32 {
    let chunks_a = a.chunks_exact(8);
    let chunks_b = b.chunks_exact(8);
    let remainder_a = chunks_a.remainder();
    let remainder_b = chunks_b.remainder();

    let mut sum = [0.0f32; 8];
    for (ca, cb) in chunks_a.zip(chunks_b) {
        for i in 0..8 {
            sum[i] += ca[i] * cb[i];
        }
    }

    let mut total: f32 = sum.iter().sum();
    for i in 0..remainder_a.len() {
        total += remainder_a[i] * remainder_b[i];
    }
    total
}

/// Dot product distance = negative dot product (lower = more similar).
pub fn dot_product_distance(a: &[f32], b: &[f32]) -> f32 {
    -dot_product(a, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_identical() {
        let v = vec![1.0, 2.0, 3.0];
        let d = cosine_distance(&v, &v);
        assert!(d.abs() < 1e-5, "expected ~0, got {d}");
    }

    #[test]
    fn cosine_orthogonal() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        let d = cosine_distance(&a, &b);
        assert!((d - 1.0).abs() < 1e-5, "expected ~1, got {d}");
    }

    #[test]
    fn cosine_opposite() {
        let a = vec![1.0, 0.0];
        let b = vec![-1.0, 0.0];
        let d = cosine_distance(&a, &b);
        assert!((d - 2.0).abs() < 1e-5, "expected ~2, got {d}");
    }

    #[test]
    fn euclidean_identical() {
        let v = vec![1.0, 2.0, 3.0];
        assert!(euclidean_distance(&v, &v).abs() < 1e-5);
    }

    #[test]
    fn euclidean_known() {
        let a = vec![0.0, 0.0];
        let b = vec![3.0, 4.0];
        let d = euclidean_distance(&a, &b);
        assert!((d - 5.0).abs() < 1e-5, "expected 5.0, got {d}");
    }

    #[test]
    fn dot_product_known() {
        let a = vec![1.0, 2.0, 3.0];
        let b = vec![4.0, 5.0, 6.0];
        // 1*4 + 2*5 + 3*6 = 32
        assert!((dot_product(&a, &b) - 32.0).abs() < 1e-5);
    }

    #[test]
    fn dot_product_distance_higher_similarity_lower_value() {
        let a = vec![1.0, 0.0];
        let similar = vec![0.9, 0.1];
        let dissimilar = vec![-0.9, 0.1];
        let d_sim = dot_product_distance(&a, &similar);
        let d_dis = dot_product_distance(&a, &dissimilar);
        assert!(d_sim < d_dis);
    }
}
