use crate::application::patch_region::SeamGateDerived;
use crate::domain::RepairPatchConfigView;

use super::{PatchAudioRequest, PatchRequestSettings};

pub(super) fn repair_patch_config_view(request: &PatchAudioRequest) -> RepairPatchConfigView {
    RepairPatchConfigView {
        fill_mode: request.fill_mode,
        fit_boundary_search: request.fit_boundary_search,
        gap_end_extend_on_post_seam_fail: request.gap_end_extend_on_post_seam_fail,
        gap_start_extend_on_pre_seam_fail: request.gap_start_extend_on_pre_seam_fail,
        gap_end_extend_max_ms: request.gap_end_extend_max_ms,
        disable_structure_trust: request.disable_structure_trust,
        short_gap_one_strong_seam_fallback: request.short_gap_one_strong_seam_fallback,
        fill_anchor_search_prior_weight: request.fill_anchor_search_prior_weight,
        fill_anchor_retry_marginal: request.fill_anchor_retry_marginal,
        fill_offset_mode: request.fill_offset_mode,
        anchor_seam_mode: request.anchor_seam_mode,
    }
}

impl SeamGateDerived {
    /// Build run-constant derived seam-gate inputs from a repair request. Frame math only —
    /// policy is read from `request.settings` at use sites. `silence_peak_fraction` comes from
    /// the per-run patch/scan context.
    pub(crate) fn from_repair(
        request: &PatchAudioRequest,
        sample_rate: u32,
        channels: usize,
        silence_peak_fraction: f32,
    ) -> Self {
        let settings = &request.settings;
        let context_frames =
            (settings.gap_signature_context_secs * sample_rate as f64).round() as usize;
        let bin_frames =
            ((settings.gap_signature_bin_ms as f64 / 1000.0) * sample_rate as f64).round() as usize;
        let border_standoff_frames =
            (settings.border_standoff_secs * sample_rate as f64).round() as usize;
        let search_radius_frames =
            (settings.fill_border_search_secs * sample_rate as f64).round() as usize;
        let fill_length_slack_frames =
            (settings.fill_length_slack_secs * sample_rate as f64).round() as usize;
        let max_extend_frames =
            (settings.gap_end_extend_max_ms as f64 / 1000.0 * sample_rate as f64).round() as usize;
        let step_frames =
            (settings.gap_end_extend_step_ms as f64 / 1000.0 * sample_rate as f64).round() as usize;
        SeamGateDerived {
            channels,
            sample_rate,
            context_frames,
            bin_frames,
            border_standoff_frames,
            search_radius_frames,
            fill_length_slack_frames,
            max_extend_frames,
            step_frames,
            residual_max_lag_frames: crate::domain::residual_max_lag_frames(
                sample_rate,
                settings.residual_lag_secs,
            ),
            silence_peak_fraction,
            measure_residual: request.measure_residual,
            anchor_matchability:
                crate::domain::gap_anchor_seam::AnchorMatchabilityParams::from_repair_fields(
                    settings.anchor_seam_min_match_pearson,
                    settings.anchor_seam_min_xcorr_peak,
                    settings.anchor_seam_xcorr_ambiguous_band,
                ),
        }
    }
}

/// The three gap-length-derived window sizes that the seam gate and the bracket-fill assembly share.
///
/// **Single-sourced (S0).** These were computed by three call sites from the same inputs
/// (`characterize_region`, `derive_seam_gate_geometry`, and — once the executor re-derives the fill —
/// `execute_bracket_fill`). Hand-duplicating the expressions is the most likely way to break byte-parity
/// between the two passes, so they live here and every consumer calls [`FillWindowFrames::for_gap`].
///
/// **`gap_frames` is the gap length *before* the seam gate ran.** The gate may move the gap boundaries —
/// gate-mode seam-extension retry (`retry_waveform_seam_extensions`) and fit-mode boundary search both
/// do — but these windows are deliberately sized once from the original length and held fixed for the
/// rest of the gap's processing. Sizing them from the post-gate `refined` would silently change the
/// assembly on every gap the gate extended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FillWindowFrames {
    /// Seam correlation window sized to the gap.
    pub(crate) correlate_frames: usize,
    /// Cap for fine-align slide search and the seam correlation gate.
    pub(crate) seam_gate_frames: usize,
    /// A-border template length (never longer than `correlate_frames`).
    pub(crate) border_frames: usize,
}

impl FillWindowFrames {
    /// Derive the trio from a gap length. See the type docs for which gap length to pass.
    pub(crate) fn for_gap(
        settings: &PatchRequestSettings,
        gap_frames: usize,
        sample_rate: u32,
    ) -> Self {
        let correlate_frames = correlate_frames_for_gap(
            settings.normalize_window_secs,
            settings.min_border_discovery_secs,
            gap_frames,
            sample_rate,
        );
        FillWindowFrames {
            correlate_frames,
            seam_gate_frames: seam_gate_frames_for(
                correlate_frames,
                settings.fill_seam_search_secs,
                sample_rate,
            ),
            border_frames: border_frames_from_secs(settings.normalize_window_secs, sample_rate)
                .min(correlate_frames),
        }
    }
}

pub(crate) fn border_frames_from_secs(window_secs: f64, sample_rate: u32) -> usize {
    (window_secs * sample_rate as f64) as usize
}

/// Cap for fine-align slide search and seam correlation gate (frames).
pub(crate) fn seam_gate_frames_for(
    correlate_frames: usize,
    fill_seam_search_secs: f64,
    sample_rate: u32,
) -> usize {
    let cap = (fill_seam_search_secs * sample_rate as f64).round() as usize;
    correlate_frames.min(cap).max(1)
}

/// Seam correlation window sized to the gap (short gaps use shorter templates).
pub(crate) fn correlate_frames_for_gap(
    normalize_window_secs: f64,
    min_border_discovery_secs: f64,
    gap_frames: usize,
    sample_rate: u32,
) -> usize {
    let gap_secs = gap_frames as f64 / sample_rate as f64;
    let window_secs = normalize_window_secs
        .min(gap_secs * 0.45)
        .min(2.0)
        .max(min_border_discovery_secs)
        .max(0.25);
    ((window_secs * sample_rate as f64) as usize).max(1)
}
