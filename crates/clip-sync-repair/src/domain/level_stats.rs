//! Small robust order statistics over dB values, shared by the level readers.
//!
//! Every estimator here is **rank-based** (median / nearest-rank percentile / MAD) rather than a
//! mean: dB populations drawn from program audio are heavy-tailed at both ends — a single loud
//! transient or one misregistered block would move a mean and cannot move a median.
//!
//! Percentiles use **nearest-rank** (`ceil(p × n)`, 1-indexed, clamped) with no interpolation. The
//! convention is fixed and tested because these values are persisted in scan JSON: an
//! implementation-defined choice would become a silent cross-version difference.

/// Median of a set of dB values. `None` when empty.
pub fn median_db(mut vals: Vec<f64>) -> Option<f64> {
    if vals.is_empty() {
        return None;
    }
    sort_db(&mut vals);
    Some(median_of_sorted(&vals))
}

/// Nearest-rank percentile of a set of dB values, `p` in `[0, 1]`. `None` when empty.
pub fn percentile_db(mut vals: Vec<f64>, p: f64) -> Option<f64> {
    if vals.is_empty() {
        return None;
    }
    sort_db(&mut vals);
    Some(percentile_of_sorted(&vals, p))
}

/// Median absolute deviation about `center` — the spread companion to [`median_db`]. `None` when
/// empty. Reported raw (**not** scaled by 1.4826 to a normal-equivalent sigma): the underlying
/// distribution is not assumed normal, and the raw value is what a reader compares against a dB
/// threshold.
pub fn mad_db(vals: &[f64], center: f64) -> Option<f64> {
    if vals.is_empty() {
        return None;
    }
    median_db(vals.iter().map(|v| (v - center).abs()).collect())
}

/// Sort dB values ascending, treating incomparable values as equal (no `NaN` is expected from
/// `block_rms_db`, but a total order keeps the estimators total).
pub fn sort_db(vals: &mut [f64]) {
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
}

/// Median of an already-sorted slice. Lower-middle element for even lengths (no averaging), so the
/// result is always an observed value.
pub fn median_of_sorted(sorted: &[f64]) -> f64 {
    sorted[sorted.len() / 2]
}

/// Nearest-rank percentile of an already-sorted slice.
pub fn percentile_of_sorted(sorted: &[f64], p: f64) -> f64 {
    let n = sorted.len();
    let rank = (p.clamp(0.0, 1.0) * n as f64).ceil() as usize;
    sorted[rank.saturating_sub(1).min(n - 1)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn median_takes_the_lower_middle_for_even_lengths() {
        assert_eq!(median_db(vec![1.0, 2.0, 3.0, 4.0]), Some(3.0));
        assert_eq!(median_db(vec![3.0, 1.0, 2.0]), Some(2.0));
        assert_eq!(median_db(vec![]), None);
    }

    #[test]
    fn nearest_rank_percentile_is_an_observed_value() {
        let vals: Vec<f64> = (1..=10).map(f64::from).collect();
        assert_eq!(percentile_db(vals.clone(), 0.1), Some(1.0));
        assert_eq!(percentile_db(vals.clone(), 0.9), Some(9.0));
        assert_eq!(percentile_db(vals.clone(), 0.95), Some(10.0));
        // Boundaries never index out of range.
        assert_eq!(percentile_db(vals.clone(), 0.0), Some(1.0));
        assert_eq!(percentile_db(vals, 1.0), Some(10.0));
    }

    #[test]
    fn tiny_populations_are_total() {
        assert_eq!(percentile_db(vec![7.0], 0.1), Some(7.0));
        assert_eq!(percentile_db(vec![7.0], 0.9), Some(7.0));
        assert_eq!(percentile_db(vec![7.0, 9.0], 0.1), Some(7.0));
        assert_eq!(percentile_db(vec![7.0, 9.0], 0.9), Some(9.0));
        assert_eq!(percentile_db(vec![], 0.5), None);
    }

    #[test]
    fn mad_is_the_median_absolute_deviation() {
        assert_eq!(mad_db(&[1.0, 2.0, 3.0, 4.0, 100.0], 3.0), Some(1.0));
        assert_eq!(mad_db(&[], 0.0), None);
    }
}
