//! FFT cross-correlation adapter over the `cross_correlate` crate.
//!
//! Extracted from `application/offset_refinement.rs` by the layer-purity plan — the
//! application layer reaches the engine only through the [`PcmCorrelator`] port.

use cross_correlate::{Correlate, CrossCorrelationMode};

use crate::application::ports::PcmCorrelator;

/// Production [`PcmCorrelator`] backed by `cross_correlate`'s real-valued FFT engine.
pub struct FftCorrelator;

impl PcmCorrelator for FftCorrelator {
    fn cross_correlate_lag(&self, a: &[f64], b: &[f64]) -> Option<(isize, f64)> {
        let correlation =
            Correlate::create_real_f64(a.len(), b.len(), CrossCorrelationMode::Full).ok()?;
        let corr = correlation.correlate_managed(a, b).ok()?;
        let (peak_index, peak_value) = corr
            .iter()
            .enumerate()
            .max_by(|(_, left), (_, right)| {
                left.abs()
                    .partial_cmp(&right.abs())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })?;

        let center = (a.len() + b.len()).saturating_sub(1) / 2;
        Some((peak_index as isize - center as isize, peak_value.abs()))
    }
}
