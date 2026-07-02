//! Infrastructure adapter for the domain [`PcmCorrelator`] port.

use clip_sync::PcmCorrelator as ClipSyncPcmCorrelator;

use crate::domain::ports::PcmCorrelator;

/// Production GCC-PHAT correlator (wraps `clip_sync::FftCorrelator`).
pub struct FftCorrelator(pub clip_sync::FftCorrelator);

impl FftCorrelator {
    pub const fn new() -> Self {
        Self(clip_sync::FftCorrelator)
    }
}

impl PcmCorrelator for FftCorrelator {
    fn cross_correlate_lag(&self, a: &[f64], b: &[f64]) -> Option<(f64, f64)> {
        self.0.cross_correlate_lag(a, b)
    }

    fn segment_similarity(&self, a: &[f64], b: &[f64]) -> f64 {
        self.0.segment_similarity(a, b)
    }

    fn slide_template_scores(&self, template: &[f64], signal: &[f64]) -> Vec<f64> {
        self.0.slide_template_scores(template, signal)
    }
}
