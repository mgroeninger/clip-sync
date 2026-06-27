//! Hidden test helpers (integration + acceptance tests).

use clip_sync::ProgressReporter;

/// No-op progress sink for scan/patch helpers (avoids `clip-sync/test-utils` in lib builds).
pub struct NoOpProgressReporter;

impl ProgressReporter for NoOpProgressReporter {
    fn phase(&self, _message: &str) {}

    fn progress(&self, _label: &str, _current: u64, _total: u64) {}
}

pub mod anchor_seam_diagnostic;
pub mod energy_signature_fixtures;
pub mod energy_signature_production;
pub mod gap_corpus_fixtures;
pub mod patch_geometry_preview;
pub mod w5_anchor_rescue_diag;
