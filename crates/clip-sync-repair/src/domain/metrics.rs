//! Pure numeric helpers — no external crate dependencies.

/// Pearson correlation of two equal-length vectors, normalized to [-1, 1].
pub fn normalized_correlation(left: &[f64], right: &[f64]) -> f64 {
    debug_assert_eq!(left.len(), right.len());
    if left.is_empty() {
        return 0.0;
    }

    let left_mean = left.iter().sum::<f64>() / left.len() as f64;
    let right_mean = right.iter().sum::<f64>() / right.len() as f64;

    let mut numerator = 0.0;
    let mut left_var = 0.0;
    let mut right_var = 0.0;
    for (left_sample, right_sample) in left.iter().zip(right.iter()) {
        let left_delta = left_sample - left_mean;
        let right_delta = right_sample - right_mean;
        numerator += left_delta * right_delta;
        left_var += left_delta * left_delta;
        right_var += right_delta * right_delta;
    }

    let denominator = (left_var * right_var).sqrt();
    if denominator <= f64::EPSILON {
        0.0
    } else {
        (numerator / denominator).clamp(-1.0, 1.0)
    }
}
