//! Domain ports — engines the repair domain depends on without naming concrete adapters.

/// GCC-PHAT cross-correlation engine for anchor-seam Tier-2 rescue.
pub trait PcmCorrelator {
    /// Full GCC-PHAT cross-correlation of `a` against `b`; returns the peak's lag in samples
    /// (fractional, sub-sample when the peak is interior) relative to the centered zero-lag
    /// position, and the peak magnitude. `None` when the engine cannot correlate these inputs.
    fn cross_correlate_lag(&self, a: &[f64], b: &[f64]) -> Option<(f64, f64)>;

    /// GCC-PHAT similarity for equal-length segments aligned at lag zero.
    fn segment_similarity(&self, a: &[f64], b: &[f64]) -> f64;

    /// GCC-PHAT score at every valid start index when sliding `template` across `signal`.
    fn slide_template_scores(&self, template: &[f64], signal: &[f64]) -> Vec<f64>;
}
