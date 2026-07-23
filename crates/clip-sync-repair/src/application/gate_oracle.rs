//! Gate-oracle helpers for the `clip-sync-repair-fixtures` crate.
//!
//! Re-exports production seam-gate internals for the `clip-sync-repair-fixtures` crate.
#![doc(hidden)]

pub use crate::application::patch_region::{
    derive_seam_gate_geometry, oracle_anchor_seam_would_run, oracle_build_fit_cache,
    oracle_evaluate_fit_joint, oracle_score_fit_candidate, OracleJointOutcome, SeamGateDerived,
    SeamGateFailure, SeamGateParams,
};

/// Build run-constant derived seam-gate inputs from a repair request (same as production).
pub fn seam_gate_derived_from_repair(
    request: &crate::application::PatchAudioRequest,
    sample_rate: u32,
    channels: usize,
    silence_peak_fraction: f32,
) -> SeamGateDerived {
    SeamGateDerived::from_repair(request, sample_rate, channels, silence_peak_fraction)
}
