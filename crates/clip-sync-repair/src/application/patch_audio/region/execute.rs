//! Apply one already-characterized region spec: extract the B-side fill, splice it into A,
//! and return the resulting patch + outcome.
//!
//! Depends on `types`, `pcm`, and `bracket`. Must not import `characterize` — execution is the
//! layer characterize sits on top of, see the plan's cycle guards.

use crate::domain::gap_repair_spec::{GapRepairSpec, GapRepairStrategy, GapRepairVerdict};

use super::super::{FillWindowFrames, PatchAudioRequest};

use super::bracket::{
    execute_bracket_fill, execute_bracket_output, ExecuteBracketFillCtx, ExecuteBracketOutputCtx,
};
use super::pcm::slice_b_extract;
use super::types::{RegionPatch, RegionPatchContext, RegionPatchMedia, RegionPatchOutcome};

/// The executor entry: produce `(patch, outcome)` from a `GapRepairSpec` — the typed characterize→execute
/// handoff. Handles the two PATCH verdicts; `Skip` isn't *executed* (the loop filters skips and derives their
/// outcome from the spec, §2.5.5). Takes `spec` by value so the SilenceSplice fill moves out without a clone.
///
/// The bracket fill is assembled HERE, from the spec + `media` + `request` — characterize hands over a
/// decision, not PCM (F2). The media/request params are what that costs: the executor needs the decode
/// buffers back, plus the run-level knobs the windows are sized from. SilenceSplice still carries its `fill`
/// on the spec, by design: that PCM is synthesized from a decision the executor cannot re-make (§1).
pub(in crate::application::patch_audio) fn execute_region_spec(
    spec: GapRepairSpec,
    request: &PatchAudioRequest,
    media: &RegionPatchMedia<'_>,
    ctx: &RegionPatchContext,
) -> (Option<RegionPatch>, RegionPatchOutcome) {
    let &RegionPatchContext {
        channels,
        sample_rate,
        silence_peak_fraction,
        ..
    } = ctx;
    let GapRepairSpec {
        refined,
        crossfade_secs,
        b_extract,
        verdict,
        ..
    } = spec;
    match verdict {
        GapRepairVerdict::Patch(GapRepairStrategy::Bracket {
            alignment,
            window_gap_frames,
            structure_trusted,
            anchor_seam_used,
            anchor_bracket_move_frames,
            seam_pre,
            seam_post,
            confidence,
            gap_start_adjust_frames,
            gap_end_adjust_frames,
            fit_used_boundary_grid,
            fit_boundary_grid_cells,
            residual,
            normalize_gain,
            local_gain,
            fit_terms,
            registration_residual,
            ..
        }) => {
            // F1: re-derive the fill from the spec alone. The window trio comes from the *pre*-gate
            // `window_gap_frames` the spec carries (S0's helper, same call characterize made); the length the
            // fill is assembled TO is the *post*-gate gap on `refined`. Mixing those two up is the one way
            // this reconstruction can silently diverge, which is why they read from different fields here.
            let windows =
                FillWindowFrames::for_gap(&request.settings, window_gap_frames, sample_rate);
            let fill = execute_bracket_fill(ExecuteBracketFillCtx {
                alignment,
                b_samples: slice_b_extract(
                    media.b_samples_full,
                    channels,
                    b_extract.start_frame,
                    b_extract.end_frame,
                )
                .expect(
                    "a Bracket verdict's B extract window is the one characterize sliced non-empty",
                ),
                a_samples: &media.a_pcm.samples,
                a_frames: media.a_pcm.frames(),
                refined,
                channels,
                gap_frames: refined.end_frame.saturating_sub(refined.start_frame),
                fill_mode: request.fill_mode,
                border_frames: windows.border_frames,
                border_standoff_frames: (request.border_standoff_secs * sample_rate as f64).round()
                    as usize,
                silence_peak_fraction,
                absolute_silence_rms: request.absolute_silence_rms,
                seam_gate_frames: windows.seam_gate_frames,
                sample_rate,
                crossfade_secs,
            });
            let (patch, outcome) = execute_bracket_output(ExecuteBracketOutputCtx {
                fill,
                gain: normalize_gain,
                local_gain,
                refined,
                crossfade_secs,
                final_pre: seam_pre,
                final_post: seam_post,
                final_confidence: confidence,
                structure_trusted,
                gap_start_adjust_frames,
                gap_end_adjust_frames,
                fit_used_boundary_grid,
                fit_boundary_grid_cells,
                residual,
                anchor_seam_used,
                anchor_bracket_move_frames,
                fit_terms,
                alignment_start_frame: alignment.start_frame,
                offset_nominal_start: b_extract.b_mapped_start_frame,
                sample_rate,
                registration_residual: registration_residual.map(|r| *r),
            });
            (Some(patch), outcome)
        }
        GapRepairVerdict::Patch(GapRepairStrategy::SilenceSplice {
            fill,
            pre_seam_r,
            post_seam_r,
            residual,
            confidence,
            registration_residual,
            normalize_gain,
            local_gain,
            step_real,
            dual_fit_post_window_ms,
            ..
        }) => {
            // Dual-fit rescue: the fill is the synthesized bridge on the spec; seams sit at their own lags, so
            // there is no align/waveform slide and no structure trust.
            let patch = RegionPatch {
                b_samples: fill,
                gain: normalize_gain,
                a_start_frame: refined.start_frame,
                a_end_frame: refined.end_frame,
                crossfade_secs,
            };
            let outcome = RegionPatchOutcome::Patched {
                pre_correlation: pre_seam_r,
                post_correlation: post_seam_r,
                align_adjustment_secs: 0.0,
                structure_trusted: false,
                confidence,
                gap_start_adjust_frames: 0,
                gap_end_adjust_frames: 0,
                fit_used_boundary_grid: false,
                fit_boundary_grid_cells: None,
                residual,
                anchor_seam_used: false,
                anchor_bracket_move_frames: 0,
                dual_fit_used: true,
                dual_fit_step_real: Some(step_real),
                dual_fit_post_window_ms,
                // No unified search on this path — see the preview arm.
                fit_terms: None,
                // The product, matching the `gain` on the patch just above. Both halves ride the
                // strategy, so this and the preview-only `outcome_from_spec` read the same values.
                normalize_gain,
                local_gain,
                registration_residual: registration_residual.map(|r| *r),
            };
            (Some(patch), outcome)
        }
        GapRepairVerdict::Skip { .. } => {
            unreachable!(
                "skips are not executed — the loop derives their outcome from the spec (§2.5.5)"
            )
        }
    }
}
