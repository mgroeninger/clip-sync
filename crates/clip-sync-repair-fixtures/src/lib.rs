//! Synthetic fixtures and oracle helpers for clip-sync-repair tier tests.
//!
//! Consumed by `clip-sync-repair-harness` and repair `tests/` binaries — not part of the
//! production repair library.

use clip_sync::ProgressReporter;

/// No-op progress sink for scan/patch helpers.
pub struct NoOpProgressReporter;

impl ProgressReporter for NoOpProgressReporter {
    fn phase(&self, _message: &str) {}

    fn progress(&self, _label: &str, _current: u64, _total: u64) {}
}

pub mod anchor_seam_diagnostic;
pub mod energy_signature_fixtures;
pub mod energy_signature_production;
pub mod fingerprint_corpus_fixtures;
pub mod gap_cell_fixtures;
pub mod gap_corpus_fixtures;
pub mod lossless_silence_pair;
pub mod patch_geometry_preview;
pub mod test_align;
pub mod w5_anchor_rescue_diag;
pub mod w5_timing_offset_diag;

pub use test_align::{
    no_op_alignment, oracle_injected_alignment, start_clip_alignment, zero_offset_alignment,
    NeverCalledAligner,
};
