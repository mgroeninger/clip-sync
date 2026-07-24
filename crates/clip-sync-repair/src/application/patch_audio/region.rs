use std::collections::HashMap;

use clip_sync::{MultiChannelPcm, ProgressReporter};

use crate::application::patch_region::{
    derive_seam_gate_geometry, evaluate_seam_gate, retry_waveform_seam_extensions,
    SeamExtensionRetry, SeamGateDerived, SeamGateFailure, SeamGateOutcome, SeamGateParams,
};
use crate::domain::{
    fill_offset::{resolve_gap_offset_secs, AnchoredRetryPass, FillOffsetMode},
    fill_mode::FillMode,
    gap_extension_slack_secs,
    gap_fill::{FillRegion, GapFillPlan},
    gap_fill_fit::{
        classify_fill_waveform_confidence, fit_fill_length_for_gap, fit_fill_to_gap_frames,
        FillConfidence,
    },
    gap_signature::build_gap_signature,
    gap_structure::StructureMatchParams,
    gap_tags::{
        derive_gap_tags_from_patch_outcome, derive_gap_tags_from_status, FillTierThresholds,
        GapPatchTierInput, GapTags, GapTagsPatchContext,
    },
    patch_anchor::{
        format_anchored_offset_verbose_line, interpolate_anchored_offset_secs, AnchorSearchPrior,
        PatchAnchorTable,
    },
    patch_result::{
        residual_summary_scalar_fields, GapFillSkipReason, GapPatchOutcome, GapPatchSkipReason,
        GapPatchStatus,
    },
    policies::{self, GapBorderSpec, RefinedGapFrames},
    gap_repair_spec::{
        BExtractWindow, GapRepairSpec, GapRepairStrategy, GapRepairTags, GapRepairVerdict,
        SeamLocalTags,
    },
    Gap,
};

use super::geometry::repair_patch_config_view;
use super::log::{
    gap_key, log_gap_fill_plan_verbose, log_gap_fill_result_verbose, log_marginal_gap_fill,
    log_skip_gap_fill, GapFillPlanLog, GapFillResultLog, MarginalGapFillLog,
};
use super::{
    border_frames_from_secs, correlate_frames_for_gap, seam_gate_frames_for, PatchAudioRequest,
};

// Collected B segment ready to splice into A.
#[derive(Debug, PartialEq)]
pub(super) struct RegionPatch {
    pub(super) b_samples: Vec<f32>,
    pub(super) gain: f32,
    pub(super) a_start_frame: usize,
    pub(super) a_end_frame: usize,
    pub(super) crossfade_secs: f64,
}
#[derive(Debug, PartialEq)]
pub(super) enum RegionPatchOutcome {
    Patched {
        pre_correlation: f64,
        post_correlation: f64,
        align_adjustment_secs: f64,
        waveform_adjustment_secs: f64,
        structure_trusted: bool,
        confidence: FillConfidence,
        gap_start_adjust_frames: i64,
        gap_end_adjust_frames: i64,
        fit_used_boundary_grid: bool,
        fit_boundary_grid_cells: Option<u32>,
        residual: Option<policies::SeamResidualVerdict>,
        anchor_seam_used: bool,
        anchor_bracket_move_frames: usize,
        dual_fit_used: bool,
    },
    Skipped {
        reason: GapPatchSkipReason,
        residual: Option<policies::SeamResidualVerdict>,
    },
}
pub(super) fn skipped_patch(reason: GapPatchSkipReason) -> RegionPatchOutcome {
    RegionPatchOutcome::Skipped {
        reason,
        residual: None,
    }
}

fn skipped_patch_with_residual(
    reason: GapPatchSkipReason,
    residual: Option<policies::SeamResidualVerdict>,
) -> RegionPatchOutcome {
    RegionPatchOutcome::Skipped { reason, residual }
}

pub(super) fn record_patch_gap_span(span: &tracing::Span, outcome: &RegionPatchOutcome) {
    match outcome {
        RegionPatchOutcome::Patched {
            confidence,
            fit_used_boundary_grid,
            fit_boundary_grid_cells,
            ..
        } => {
            span.record("outcome", "patched");
            span.record(
                "confidence",
                match confidence {
                    FillConfidence::High => "high",
                    FillConfidence::Marginal => "marginal",
                },
            );
            span.record("boundary_grid", *fit_used_boundary_grid);
            if let Some(cells) = fit_boundary_grid_cells {
                span.record("grid_cells", cells);
            }
        }
        RegionPatchOutcome::Skipped { reason, .. } => {
            span.record("outcome", "skipped");
            span.record("skip_reason", format!("{reason:?}"));
        }
    }
}
fn anchor_search_prior_for_gap(
    request: &PatchAudioRequest,
    patch_anchors: Option<&PatchAnchorTable>,
    anchored_retry_pass: AnchoredRetryPass,
    region: &FillRegion,
    b_extract_start_secs: f64,
    search_radius_frames: usize,
    sample_rate: u32,
) -> Option<AnchorSearchPrior> {
    if request.fill_mode != FillMode::Fit || request.fill_anchor_search_prior_weight <= 0.0 {
        return None;
    }
    let table = patch_anchors.filter(|t| !t.is_empty())?;
    if anchored_retry_pass != AnchoredRetryPass::Second
        && request.fill_offset_mode != FillOffsetMode::Anchored
    {
        return None;
    }
    let gap_time_on_a = (region.a_start_secs + region.a_end_secs) / 2.0;
    let predicted_offset = interpolate_anchored_offset_secs(
        &request.report.alignment,
        gap_time_on_a,
        table,
    )?;
    let predicted_b_start = region.a_start_secs + predicted_offset;
    let predicted_start_frame = ((predicted_b_start - b_extract_start_secs) * sample_rate as f64)
        .round()
        .max(0.0) as usize;
    Some(AnchorSearchPrior {
        predicted_start_frame,
        weight: request.fill_anchor_search_prior_weight,
        search_radius_frames,
    })
}
pub(super) struct RegionPatchMedia<'a> {
    pub(super) b_samples_full: &'a [f32],
    pub(super) a_pcm: &'a MultiChannelPcm,
}

pub(super) struct RegionPatchOpts<'a> {
    pub(super) anchored_retry_pass: AnchoredRetryPass,
    pub(super) patch_anchors: Option<&'a PatchAnchorTable>,
}
pub(super) fn outcomes_in_report_order(
    gaps: &[Gap],
    plan: &GapFillPlan,
    region_results: &[(f64, f64, RegionPatchOutcome, GapTags)],
    fill_mode: FillMode,
    thresholds: FillTierThresholds,
) -> Vec<GapPatchOutcome> {
    let mut status_by_gap: HashMap<(u64, u64), GapPatchStatus> = HashMap::new();
    let mut tags_by_gap: HashMap<(u64, u64), GapTags> = HashMap::new();
    let mut residual_by_gap: HashMap<(u64, u64), policies::SeamResidualVerdict> = HashMap::new();

    for skip in &plan.skipped {
        let key = gap_key(skip.a_start_secs, skip.a_end_secs);
        let status = GapPatchStatus::NotPlanned {
            reason: skip.reason.clone(),
        };
        let tags = derive_gap_tags_from_status(&status, fill_mode, thresholds);
        status_by_gap.insert(key, status);
        tags_by_gap.insert(key, tags);
    }

    for (a_start, a_end, outcome, tags) in region_results {
        let status = match outcome {
            RegionPatchOutcome::Patched {
                pre_correlation,
                post_correlation,
                align_adjustment_secs,
                waveform_adjustment_secs,
                structure_trusted,
                confidence,
                gap_start_adjust_frames,
                gap_end_adjust_frames,
                residual,
                anchor_seam_used,
                anchor_bracket_move_frames,
                dual_fit_used,
                ..
            } => {
                if let Some(verdict) = residual {
                    residual_by_gap.insert(gap_key(*a_start, *a_end), *verdict);
                }
                let (residual_db, floor_db, headroom_db) =
                    residual_summary_scalar_fields(residual.as_ref());
                GapPatchStatus::Patched {
                    pre_correlation: *pre_correlation,
                    post_correlation: *post_correlation,
                    align_adjustment_secs: *align_adjustment_secs,
                    waveform_adjustment_secs: *waveform_adjustment_secs,
                    structure_trusted: *structure_trusted,
                    confidence: *confidence,
                    gap_start_adjust_frames: *gap_start_adjust_frames,
                    gap_end_adjust_frames: *gap_end_adjust_frames,
                    residual_db,
                    floor_db,
                    headroom_db,
                    anchor_seam_used: *anchor_seam_used,
                    anchor_bracket_move_frames: *anchor_bracket_move_frames,
                    dual_fit_used: *dual_fit_used,
                }
            }
            RegionPatchOutcome::Skipped { reason, residual } => {
                if let Some(verdict) = residual {
                    residual_by_gap.insert(gap_key(*a_start, *a_end), *verdict);
                }
                GapPatchStatus::Skipped {
                    reason: reason.clone(),
                }
            }
        };
        let key = gap_key(*a_start, *a_end);
        status_by_gap.insert(key, status);
        tags_by_gap.insert(key, tags.clone());
    }

    gaps
        .iter()
        .map(|gap| {
            let key = gap_key(gap.video_a_start_secs, gap.video_a_end_secs);
            let status = status_by_gap.remove(&key).unwrap_or(GapPatchStatus::NotPlanned {
                reason: GapFillSkipReason::NotFillable,
            });
            let tags = tags_by_gap.remove(&key).unwrap_or_else(|| {
                derive_gap_tags_from_status(&status, fill_mode, thresholds)
            });
            GapPatchOutcome::new(
                gap.video_a_start_secs,
                gap.video_a_end_secs,
                status,
                tags,
            )
            .with_residual(residual_by_gap.remove(&key))
        })
        .collect()
}

/// Per-run values derived once in `execute` and shared by every fill region.
pub(super) struct RegionPatchContext {
    pub(super) channels: usize,
    pub(super) sample_rate: u32,
    pub(super) max_refine_frames: usize,
    pub(super) global_a_rms: f32,
    /// From the scan report (matches the thresholds the gaps were detected with).
    pub(super) silence_peak_fraction: f32,
}

pub(super) fn region_outcome_gap_tags(
    outcome: &RegionPatchOutcome,
    mut tag_ctx: GapTagsPatchContext,
) -> GapTags {
    if let RegionPatchOutcome::Patched {
        anchor_seam_used,
        anchor_bracket_move_frames,
        dual_fit_used,
        ..
    } = outcome
    {
        tag_ctx.anchor_seam_used = *anchor_seam_used;
        tag_ctx.anchor_bracket_move_frames = *anchor_bracket_move_frames;
        tag_ctx.dual_fit_used = *dual_fit_used;
    }
    tag_ctx.residual = match outcome {
        RegionPatchOutcome::Patched { residual, .. } | RegionPatchOutcome::Skipped { residual, .. } => {
            *residual
        }
    };
    let input = match outcome {
        RegionPatchOutcome::Patched {
            pre_correlation,
            post_correlation,
            confidence,
            ..
        } => GapPatchTierInput::Patched {
            pre: *pre_correlation,
            post: *post_correlation,
            confidence: *confidence,
        },
        RegionPatchOutcome::Skipped { reason, .. } => GapPatchTierInput::Skipped(reason),
    };
    derive_gap_tags_from_patch_outcome(&input, tag_ctx)
}

fn seam_failure_outcome(
    progress: &dyn ProgressReporter,
    request: &PatchAudioRequest,
    region: &FillRegion,
    fail: SeamGateFailure,
    min_structure_match_score: f32,
    tag_ctx: GapTagsPatchContext,
    dual_fit_attempt: Option<crate::domain::patch_result::SeamScoreAttempt>,
) -> (Option<RegionPatch>, RegionPatchOutcome, GapTagsPatchContext) {
    let (reason, residual) = match fail {
        SeamGateFailure::StructureAlignmentFailed => {
            (GapPatchSkipReason::BoundaryAlignmentFailed, None)
        }
        SeamGateFailure::StructureBelowThreshold { pre, post } => (
            GapPatchSkipReason::CorrelationBelowThreshold {
                pre_correlation: pre,
                post_correlation: post,
                min_correlation: min_structure_match_score,
                best_attempt: crate::domain::patch_result::better_seam_score_attempt(
                    None,
                    dual_fit_attempt,
                ),
            },
            None,
        ),
        SeamGateFailure::WaveformBelowThreshold {
            pre,
            post,
            min,
            best_attempt,
        } => (
            GapPatchSkipReason::CorrelationBelowThreshold {
                pre_correlation: pre,
                post_correlation: post,
                min_correlation: min,
                best_attempt: crate::domain::patch_result::better_seam_score_attempt(
                    best_attempt,
                    dual_fit_attempt,
                ),
            },
            None,
        ),
        SeamGateFailure::ResidualHeadroomExceeded {
            pre,
            post,
            residual,
            margin_db,
        } => (
            GapPatchSkipReason::ResidualHeadroomExceeded {
                pre_correlation: pre,
                post_correlation: post,
                headroom_db: residual.worst_headroom_db(),
                floor_pre_db: residual.floor_pre_db,
                floor_post_db: residual.floor_post_db,
                margin_db,
            },
            Some(residual),
        ),
    };
    log_skip_gap_fill(
        progress,
        &request.report.gaps,
        region.a_start_secs,
        region.a_end_secs,
        &reason,
    );
    let outcome = if let Some(residual) = residual {
        skipped_patch_with_residual(reason, Some(residual))
    } else {
        skipped_patch(reason)
    };
    (None, outcome, tag_ctx)
}

/// A-side gap-interior RMS floor (dB) — approximation of the fingerprint `levels.gap_floor_db`.
fn a_gap_floor_db(a_samples: &[f32], channels: usize, gap_start: usize, gap_end: usize) -> f64 {
    let ch = channels.max(1);
    let lo = gap_start.min(gap_end);
    let hi = gap_start.max(gap_end);
    if lo >= hi {
        return -120.0;
    }
    let sum_sq: f64 = (lo..hi)
        .map(|f| {
            let base = f * ch;
            let m = a_samples[base..base + ch].iter().map(|&x| x as f64).sum::<f64>() / ch as f64;
            m * m
        })
        .sum();
    let rms = (sum_sq / (hi - lo) as f64).sqrt();
    if rms <= 1e-9 {
        -120.0
    } else {
        20.0 * rms.log10()
    }
}

fn b_mapped_start_frame(
    refined_b_start_secs: f64,
    b_extract_start_secs: f64,
    sample_rate: u32,
) -> usize {
    (((refined_b_start_secs - b_extract_start_secs) * sample_rate as f64).round() as i64).max(0) as usize
}

/// Everything the A3 dual-fit algorithm needs, built once from the decoded window — only when `--dual-fit`
/// is on (so the off path pays nothing). `None` when the gap is too near a window edge for the seam borders.
struct DualFitRepairInput<'a> {
    params: crate::domain::dual_fit::DualFitParams,
    a_pre_mono: Vec<f64>,
    a_post_mono: Vec<f64>,
    b_mono: Vec<f64>,
    b_samples: &'a [f32],
    b_mapped_start: usize,
    a_start_frame: usize,
    a_end_frame: usize,
    crossfade_secs: f64,
    /// A's full decoded PCM — for the post-assembly residual measurement (parity with the ordinary
    /// path's residual gate).
    a_samples: &'a [f32],
    /// For the post-assembly residual measurement (parity with the ordinary path's residual gate) —
    /// the reference-window walk-out standoff, same constant the ordinary path uses.
    border_standoff_frames: usize,
}

/// Grouped inputs for [`build_dual_fit_input`] — the raw A/B buffers, resolved gap geometry, and
/// fill thresholds needed to assemble a [`DualFitRepairInput`]. Bundled into one value so the
/// constructor takes a single, order-safe parameter instead of a 13-wide positional list.
struct DualFitInputSpec<'a> {
    a_samples: &'a [f32],
    b_samples: &'a [f32],
    channels: usize,
    sample_rate: u32,
    refined: RefinedGapFrames,
    refined_b_start_secs: f64,
    b_extract_start_secs: f64,
    gap_frames: usize,
    fill_seam_search_secs: f64,
    min_fill_correlation: f32,
    fill_absolute_floor: f32,
    crossfade_secs: f64,
    border_standoff_frames: usize,
}

fn build_dual_fit_input<'a>(spec: DualFitInputSpec<'a>) -> Option<DualFitRepairInput<'a>> {
    let DualFitInputSpec {
        a_samples,
        b_samples,
        channels,
        sample_rate,
        refined,
        refined_b_start_secs,
        b_extract_start_secs,
        gap_frames,
        fill_seam_search_secs,
        min_fill_correlation,
        fill_absolute_floor,
        crossfade_secs,
        border_standoff_frames,
    } = spec;
    let ch = channels.max(1);
    let w = ((fill_seam_search_secs * sample_rate as f64).round() as usize).max(8);
    let a_frames = a_samples.len() / ch;
    if refined.start_frame < w || refined.end_frame + w > a_frames || gap_frames == 0 {
        return None;
    }
    let mono = |lo: usize, hi: usize| -> Vec<f64> {
        (lo..hi)
            .map(|f| {
                let base = f * ch;
                a_samples[base..base + ch].iter().map(|&x| x as f64).sum::<f64>() / ch as f64
            })
            .collect()
    };
    let a_pre_mono = mono(refined.start_frame - w, refined.start_frame);
    let a_post_mono = mono(refined.end_frame, refined.end_frame + w);
    let b_mono: Vec<f64> = b_samples
        .chunks(ch)
        .map(|fr| fr.iter().map(|&x| x as f64).sum::<f64>() / ch as f64)
        .collect();
    let b_mapped_start = b_mapped_start_frame(refined_b_start_secs, b_extract_start_secs, sample_rate);
    let a_gap_floor_db = a_gap_floor_db(a_samples, channels, refined.start_frame, refined.end_frame);

    Some(DualFitRepairInput {
        params: crate::domain::dual_fit::DualFitParams {
            channels: ch,
            sample_rate,
            gap_frames,
            seam_window_frames: w,
            max_lag_frames: (0.6 * sample_rate as f64) as usize,
            min_fill_correlation: min_fill_correlation as f64,
            fill_absolute_floor: fill_absolute_floor as f64,
            step_real_margin: 0.15,
            a_gap_floor_db,
        },
        a_pre_mono,
        a_post_mono,
        b_mono,
        b_samples,
        b_mapped_start,
        a_start_frame: refined.start_frame,
        a_end_frame: refined.end_frame,
        crossfade_secs,
        a_samples,
        border_standoff_frames,
    })
}

/// Flag-gated **A3 dual-fit fallback** at the seam-gate skip. With `--dual-fit` off (default) this is exactly
/// [`seam_failure_outcome`] — the existing skip, **byte-identical** (D6). With it on, a bracket-exhausted
/// skip gets a dual-fit attempt (§5.2: per-shoulder seam-local fit → interior trim → `dualfit_target`); on
/// success it returns a `Patched` fill instead of the skip.
///
/// **`StructureAlignmentFailed` is excluded** — that variant means structure search never scored a single
/// candidate bracket, so there is nothing "exhausted" (§5.2 step 1 / doc `G6` requires `bracket_exhausted`,
/// i.e. brackets were scored and all failed). Dual-fit only ever attempts a rescue on a *scored-but-failed*
/// skip.
///
/// The assembled/trimmed fill is re-validated with the **unchanged** production gate (§5.2 step 3): the
/// pre-trim seam-local scores from `try_dual_fit` only prove the *shoulders* are viable, not the spliced
/// result, so we re-score `r.fill` against the real A border templates with
/// `fill_splice_seam_correlations_interleaved` and classify it with the same
/// `classify_fill_waveform_confidence` every other fill path uses. No loosening: a fill that doesn't clear
/// the floors here falls back to the skip.
/// `StructureAlignmentFailed` never qualifies for dual-fit rescue (see doc comment on
/// [`skip_or_dual_fit`]) — pulled out as a pure predicate so the exclusion is unit-testable
/// without constructing a full [`DualFitRepairInput`]/[`PatchAudioRequest`].
fn dual_fit_eligible(request_dual_fit: bool, fail: SeamGateFailure) -> bool {
    request_dual_fit && !matches!(fail, SeamGateFailure::StructureAlignmentFailed)
}

/// Residual-gate parity for dual-fit (§ residual-gate bypass fix): the ordinary path measures one
/// `SeamResidualVerdict` per gap using a single `chosen_delta`, valid because it assumes one rigid
/// A/B placement across the whole splice. Dual-fit matches its two shoulders independently
/// (`r.pre_lag`/`r.post_lag`), so this reuses the same low-level primitives
/// (`policies::seam_chosen_and_floor`/`SeamResidualVerdict::from_parts_with_placement`) with two
/// distinct `chosen_delta` values, one per shoulder, derived from `nominal_delta + {pre,post}_lag`.
fn measure_dual_fit_residual_verdict(
    request: &PatchAudioRequest,
    df: &DualFitRepairInput<'_>,
    r: &crate::domain::dual_fit::DualFitResult,
) -> Option<policies::SeamResidualVerdict> {
    if !(request.measure_residual
        || request.residual_gate.is_active()
        || tracing::enabled!(tracing::Level::DEBUG))
    {
        return None;
    }
    let nominal_delta = df.b_mapped_start as i64 - df.a_start_frame as i64;
    let max_lag_frames =
        crate::domain::residual_max_lag_frames(df.params.sample_rate, request.residual_lag_secs);
    let floor_common = |window: usize| policies::SeamFloorParams {
        a_samples: df.a_samples,
        channels: df.params.channels,
        b_mono: &df.b_mono,
        window,
        standoff_frames: df.border_standoff_frames,
        a_to_b_delta: nominal_delta,
        step_frames: window.max(1),
        max_walk_frames: df.params.sample_rate as usize * 3,
        absolute_silence_rms: request.absolute_silence_rms,
        max_lag_frames,
    };
    let window = df.params.seam_window_frames;
    let (chosen_pre, floor_pre) = policies::seam_chosen_and_floor(
        &floor_common(window),
        policies::SeamSide::Pre,
        df.a_start_frame,
        df.a_end_frame,
        nominal_delta + r.pre_lag,
    );
    let (chosen_post, floor_post) = policies::seam_chosen_and_floor(
        &floor_common(window),
        policies::SeamSide::Post,
        df.a_start_frame,
        df.a_end_frame,
        nominal_delta + r.post_lag,
    );
    let placement_slide = r.pre_lag.abs_diff(r.post_lag);
    Some(policies::SeamResidualVerdict::from_parts_with_placement(
        &chosen_pre,
        &chosen_post,
        &floor_pre,
        &floor_post,
        request.residual_floor_ok_db,
        placement_slide,
        max_lag_frames,
    ))
}

fn skip_or_dual_fit(
    progress: &dyn ProgressReporter,
    request: &PatchAudioRequest,
    region: &FillRegion,
    fail: SeamGateFailure,
    min_structure_match_score: f32,
    tag_ctx: GapTagsPatchContext,
    dual_fit: Option<&DualFitRepairInput<'_>>,
) -> (DualFitDecision, GapTagsPatchContext) {
    let mut dual_fit_attempt: Option<crate::domain::patch_result::SeamScoreAttempt> = None;
    if dual_fit_eligible(request.dual_fit, fail) {
        if let Some(df) = dual_fit {
            if let Some(r) = crate::domain::dual_fit::try_dual_fit(
                &df.a_pre_mono,
                &df.a_post_mono,
                &df.b_mono,
                df.b_samples,
                df.b_mapped_start,
                &df.params,
            ) {
                // Re-validate the assembled seams. The seam scores are the ones `try_dual_fit`'s own
                // seam-local search already measured (`r.pre_seam_r`/`r.post_seam_r`): A's near-gap
                // border vs the B window ENDING at the pre shoulder / STARTING at the post shoulder —
                // the content just OUTSIDE the bridge that each shoulder matched. The interior trim
                // (`trim_at_lowest_energy_interior`) only edits the bridge interior, guarded clear of
                // `seam_window_frames` from each end, so the assembled seams equal the ones dual-fit
                // fit — nothing to re-measure.
                //
                // We deliberately do NOT route this through `fill_splice_seam_correlations_interleaved`:
                // its border branch compares A's border against the fill's own head/tail
                // (`fill[..w]`/`fill[len-w..]`), which for a dual-fit fill is the B window on the INSIDE
                // of the shoulder — i.e. the chunk of B immediately ADJACENT to (not overlapping) the
                // window each seam actually matched. For broadband audio those adjacent windows
                // correlate at ~0, so a perfect dual-fit fill scored a false ~0 and was skipped. That
                // scorer is built for the rigid single-lag splice (where the fill head overlaps A's
                // pre-gap region); it has no access to the B content outside the bridge that dual-fit's
                // per-shoulder seam-local match relies on.
                let (splice_pre, splice_post) = (r.pre_seam_r, r.post_seam_r);
                match classify_fill_waveform_confidence(
                    splice_pre,
                    splice_post,
                    request.min_fill_correlation,
                    request.fill_marginal_margin,
                    request.fill_absolute_floor,
                ) {
                    Ok(confidence) => {
                        let residual = measure_dual_fit_residual_verdict(request, df, &r);
                        let gated_confidence = match &residual {
                            Some(verdict) => match crate::domain::gap_fill_fit::apply_residual_to_confidence(
                                Ok(confidence),
                                verdict,
                                request.residual_headroom_margin_db,
                                request.residual_gate.rescue_enabled(),
                            ) {
                                Ok(tier) => Ok(tier),
                                Err(crate::domain::gap_fill_fit::ResidualGateError::HeadroomExceeded {
                                    headroom_db,
                                    margin_db,
                                }) => Err((headroom_db, margin_db, *verdict)),
                                Err(crate::domain::gap_fill_fit::ResidualGateError::PearsonBelowFloor(_)) => {
                                    Ok(confidence)
                                }
                            },
                            None => Ok(confidence),
                        };
                        let confidence = match gated_confidence {
                            Ok(tier) => tier,
                            Err((headroom_db, margin_db, verdict)) => {
                                tracing::debug!(
                                    a_start = df.a_start_frame,
                                    headroom_db,
                                    margin_db,
                                    "dual-fit candidate rejected by residual headroom gate; falling back to skip"
                                );
                                dual_fit_attempt = Some(crate::domain::patch_result::SeamScoreAttempt {
                                    pre_correlation: splice_pre,
                                    post_correlation: splice_post,
                                    source: crate::domain::patch_result::SeamScoreSource::DualFit,
                                });
                                let (_, outcome, tag_ctx) = seam_failure_outcome(
                                    progress,
                                    request,
                                    region,
                                    SeamGateFailure::ResidualHeadroomExceeded {
                                        pre: splice_pre,
                                        post: splice_post,
                                        residual: verdict,
                                        margin_db,
                                    },
                                    min_structure_match_score,
                                    tag_ctx,
                                    dual_fit_attempt,
                                );
                                return (dual_fit_skipped(outcome), tag_ctx);
                            }
                        };
                        tracing::debug!(
                            a_start = df.a_start_frame,
                            pre = splice_pre,
                            post = splice_post,
                            trim = r.trim_frames,
                            ?confidence,
                            "dual-fit rescued a bracket-exhausted skip"
                        );
                        // 6b.3d/e: build a `Patch(SilenceSplice)` spec and hand it back for the shim to
                        // execute (characterize decides, execute runs — §2.5). The synthesized bridge PCM
                        // lives on the spec; geometry comes from the dual-fit input (`refined` = the gap
                        // frames). `b_extract` / `gap_offset` are still inert for this arm (the executor
                        // reads only `refined` + `crossfade` + the SilenceSplice fields) — known-tracked (f),
                        // fixed at Fingerprint-unification 8c.
                        //
                        // 8a: populate the dual-fit D/R payload (`seam_local` + `donor_aligned`) from the ONE
                        // `DualFitResult` `r` — single-source (A7 / §2.5.2a): the tag seam scores ARE the
                        // strategy's, copied not re-measured. `gate_pass` is trivially true here (we only reach
                        // this build after `try_dual_fit` passed its own `smin ≥ floors` gate and the assembled
                        // seam re-validated Ok). Uniqueness validators stay `None` (Tier-3, production path).
                        let seam_local = SeamLocalTags {
                            pre_seam_r: r.pre_seam_r,
                            post_seam_r: r.post_seam_r,
                            post_seam_global_r: r.post_seam_global_r,
                            trim_frames: r.trim_frames,
                            gate_pass: true,
                            pre_lag: r.pre_lag,
                            post_lag: r.post_lag,
                            pre_seam_prom: None,
                            post_seam_prom: None,
                            pre_seam_z: None,
                            post_seam_z: None,
                        };
                        // Single-source invariant (§2.5.7 #2): the tag seams equal the strategy seams because
                        // both are `r.{pre,post}_seam_r` (splice_pre/splice_post are those same values, L1654).
                        debug_assert_eq!(seam_local.pre_seam_r, splice_pre);
                        debug_assert_eq!(seam_local.post_seam_r, splice_post);
                        let tags_ctx = GapRepairTags {
                            seam_local: Some(seam_local),
                            donor_aligned: Some(r.aligned_donor),
                            ..GapRepairTags::default()
                        };
                        // 8c: return the SilenceSplice *strategy* + base geometry (from the dual-fit input) +
                        // tags. `characterize_region` (which holds the region geometry) wraps this into the full
                        // `GapRepairSpec` with real `b_extract` / `gap_offset` — this call site lacks them, and
                        // returning inert placeholders here was the note-(f) trap. `refined` / `crossfade` come
                        // from `df` (the pre-gate base geometry dual-fit worked on).
                        return (
                            DualFitDecision::Rescued(Box::new(DualFitRescue {
                                strategy: GapRepairStrategy::SilenceSplice {
                                    fill: r.fill,
                                    pre_seam_r: splice_pre,
                                    post_seam_r: splice_post,
                                    pre_lag: r.pre_lag,
                                    post_lag: r.post_lag,
                                    trim_frames: r.trim_frames,
                                    residual,
                                    confidence,
                                },
                                tags_ctx,
                                refined: RefinedGapFrames {
                                    start_frame: df.a_start_frame,
                                    end_frame: df.a_end_frame,
                                },
                                crossfade_secs: df.crossfade_secs,
                            })),
                            tag_ctx,
                        );
                    }
                    Err(min_score) => {
                        tracing::debug!(
                            a_start = df.a_start_frame,
                            pre = splice_pre,
                            post = splice_post,
                            min_score,
                            "dual-fit candidate failed re-validation at the assembled seam; falling back to skip"
                        );
                        dual_fit_attempt = Some(crate::domain::patch_result::SeamScoreAttempt {
                            pre_correlation: splice_pre,
                            post_correlation: splice_post,
                            source: crate::domain::patch_result::SeamScoreSource::DualFit,
                        });
                    }
                }
            }
        }
    }
    let (_, outcome, tag_ctx) = seam_failure_outcome(
        progress,
        request,
        region,
        fail,
        min_structure_match_score,
        tag_ctx,
        dual_fit_attempt,
    );
    (dual_fit_skipped(outcome), tag_ctx)
}

/// Inputs to [`assemble_bracket_fill`] — the sliced B fill (`b_fill_raw` + contiguous `b_extension`), the
/// A-border templates for Fit-mode length fitting, and the seam context.
struct BracketFillAssembly<'a> {
    b_fill_raw: Vec<f32>,
    b_extension: &'a [f32],
    channels: usize,
    gap_frames: usize,
    fill_mode: FillMode,
    a_pre_border: &'a [f64],
    a_post_border: &'a [f64],
    a_pre_ch: &'a [Vec<f64>],
    a_post_ch: &'a [Vec<f64>],
    pre_gate_frames: usize,
    post_gate_frames: usize,
    repeat_window_frames: usize,
    seam_ctx: policies::SpliceSeamContext<'a>,
}

/// The assembled fill plus the one reportable fact about how it was built. Returning the fact instead of
/// logging it (L0) is what makes [`assemble_bracket_fill`] safe to call twice: a pure function called from
/// both passes yields the same value twice, and only the reporting call site speaks. Reporting from inside
/// the primitive would double-emit once the executor re-derives — and would describe characterize's fill,
/// which is discarded, rather than the executor's, which is written.
struct BracketFill {
    pcm: Vec<f32>,
    /// Gate mode only: frames appended from contiguous B because the bracket was shorter than the A gap.
    /// `None` in Fit mode (length fitting handles it) and when no extension was needed or available.
    extended_frames: Option<usize>,
}

/// Assemble the bracket fill PCM to exactly `gap_frames` from the sliced B: **Fit** length-fits against the A
/// borders ([`fit_fill_length_for_gap`]), **Gate** extends from contiguous B then tail-trims
/// ([`fit_fill_to_gap_frames`]). Extracted verbatim from `prepare_region_patch` (6b.3a) so the current inline
/// path and the future `execute_region_spec` share ONE PCM primitive — byte-identical by construction. The
/// executor reconstructs these inputs deterministically from the spec: `b_fill_raw`/`b_extension` re-sliced
/// from the decode buffer via `FillAlignment`, and (Fit mode only) the A-border templates + `seam_ctx` rebuilt
/// from A geometry — never re-deciding.
///
/// **Side-effect-free (L0).** No logging, no spans: see [`BracketFill`].
fn assemble_bracket_fill(inp: BracketFillAssembly<'_>) -> BracketFill {
    let BracketFillAssembly {
        b_fill_raw,
        b_extension,
        channels,
        gap_frames,
        fill_mode,
        a_pre_border,
        a_post_border,
        a_pre_ch,
        a_post_ch,
        pre_gate_frames,
        post_gate_frames,
        repeat_window_frames,
        seam_ctx,
    } = inp;
    // `source_frames` is derived here (not a field) so a caller can't pass one inconsistent with `b_fill_raw`.
    let source_frames = b_fill_raw.len() / channels;
    if fill_mode == FillMode::Fit {
        let borders = policies::BorderSeamTemplates {
            a_pre: a_pre_border,
            a_post: a_post_border,
            a_pre_ch,
            a_post_ch,
            pre_window: pre_gate_frames,
            post_window: post_gate_frames,
        };
        BracketFill {
            pcm: fit_fill_length_for_gap(
                &b_fill_raw,
                b_extension,
                channels,
                gap_frames,
                &borders,
                repeat_window_frames,
                seam_ctx,
            ),
            extended_frames: None,
        }
    } else {
        let mut gate_fill = b_fill_raw;
        let mut extended_frames = None;
        if source_frames < gap_frames {
            let need_samples = (gap_frames - source_frames) * channels;
            let extend_to = need_samples.min(b_extension.len());
            if extend_to > 0 {
                gate_fill.extend_from_slice(&b_extension[..extend_to]);
                extended_frames = Some(extend_to / channels);
            }
        }
        BracketFill {
            pcm: fit_fill_to_gap_frames(&gate_fill, channels, gap_frames),
            extended_frames,
        }
    }
}

/// Everything the executor needs to reconstruct a bracket fill from the spec — the `FillAlignment` indices,
/// the decode buffers, and the A geometry/config to rebuild the border templates.
struct ExecuteBracketFillCtx<'a> {
    alignment: policies::FillAlignment,
    b_samples: &'a [f32],
    a_samples: &'a [f32],
    a_frames: usize,
    refined: RefinedGapFrames,
    channels: usize,
    gap_frames: usize,
    fill_mode: FillMode,
    border_frames: usize,
    border_standoff_frames: usize,
    silence_peak_fraction: f32,
    absolute_silence_rms: f32,
    seam_gate_frames: usize,
    sample_rate: u32,
    crossfade_secs: f64,
}

/// The executor's bracket-fill reconstruction (6b.3a). Re-derives everything the fill needs from the spec's
/// [`FillAlignment`] + the decode buffers + A geometry — **independently of characterize** — then assembles
/// the PCM via the shared [`assemble_bracket_fill`]. This is the "assemble twice" design: characterize
/// assembles a fill to score the report-vs-splice reconciliation and discards it; the executor re-assembles
/// from the spec for the splice (borders rebuilt from scratch — deduped later by the step-8 hoists).
///
/// **Currently exercised only by the fill `debug_assert_eq!` shadow** (the live fill is still the inline
/// `assemble_bracket_fill`, shared with the reconciliation). It becomes the live fill path at 6b.3c, when
/// `execute_region_spec` re-assembles from the spec in a separate execute step.
fn execute_bracket_fill(ctx: ExecuteBracketFillCtx<'_>) -> Vec<f32> {
    let ExecuteBracketFillCtx {
        alignment,
        b_samples,
        a_samples,
        a_frames,
        refined,
        channels,
        gap_frames,
        fill_mode,
        border_frames,
        border_standoff_frames,
        silence_peak_fraction,
        absolute_silence_rms,
        seam_gate_frames,
        sample_rate,
        crossfade_secs,
    } = ctx;
    // M0 (§3): the executor-side cost — border rebuild + assembly. In debug builds this nests under the
    // `debug_assert_eq!` shadow rather than the live path; measure on a release-profile run.
    let _s = tracing::info_span!("exec_fill_assembly").entered();
    let fill_start_sample = alignment.start_frame * channels;
    let b_fill_end_sample = fill_start_sample + alignment.fill_frames * channels;
    let b_fill_raw = b_samples[fill_start_sample..b_fill_end_sample].to_vec();
    let b_extension = if b_fill_end_sample < b_samples.len() {
        &b_samples[b_fill_end_sample..]
    } else {
        &[][..]
    };
    let border_spec = GapBorderSpec {
        gap_start_frame: refined.start_frame,
        gap_end_frame: refined.end_frame,
        border_frames,
        border_standoff_frames,
        silence_peak_fraction,
        absolute_rms_floor: absolute_silence_rms,
    };
    let (a_pre_border, a_post_border) =
        policies::border_templates_for_gap(a_samples, channels, &border_spec);
    let (a_pre_ch, a_post_ch) =
        policies::border_templates_per_channel_for_gap(a_samples, channels, &border_spec);
    let pre_gate_frames = seam_gate_frames.min(a_pre_border.len().max(1));
    let post_gate_frames = if a_post_border.is_empty() {
        0
    } else {
        seam_gate_frames.min(a_post_border.len()).max(1)
    };
    let repeat_window_frames = border_frames.max(1);
    let seam_cf = policies::effective_seam_crossfade_frames(
        (crossfade_secs * sample_rate as f64) as usize,
        refined.start_frame,
        refined.end_frame,
        a_frames,
    );
    let seam_ctx = policies::SpliceSeamContext {
        seam_cf,
        gap_start_frame: refined.start_frame,
        gap_end_frame: refined.end_frame,
        a_samples,
        channels,
        single_lag_alignment: true,
    };
    assemble_bracket_fill(BracketFillAssembly {
        b_fill_raw,
        b_extension,
        channels,
        gap_frames,
        fill_mode,
        a_pre_border: &a_pre_border,
        a_post_border: &a_post_border,
        a_pre_ch: &a_pre_ch,
        a_post_ch: &a_post_ch,
        pre_gate_frames,
        post_gate_frames,
        repeat_window_frames,
        seam_ctx,
    })
    .pcm
}

/// Resolved values the executor assembles a bracket `(RegionPatch, RegionPatchOutcome)` from. Everything
/// here is either read from the spec (gain, seam correlations, confidence, flags) or reconstructed (`fill`
/// via [`execute_bracket_fill`], slides recomputed from the geometry fields).
struct ExecuteBracketOutputCtx {
    fill: Vec<f32>,
    gain: f32,
    refined: RefinedGapFrames,
    crossfade_secs: f64,
    final_pre: f64,
    final_post: f64,
    final_confidence: FillConfidence,
    structure_trusted: bool,
    gap_start_adjust_frames: i64,
    gap_end_adjust_frames: i64,
    fit_used_boundary_grid: bool,
    fit_boundary_grid_cells: Option<u32>,
    residual: Option<policies::SeamResidualVerdict>,
    anchor_seam_used: bool,
    anchor_bracket_move_frames: usize,
    // Slide geometry — recomputed here from EXACT frame values (no float round-trip). `offset_nominal_start`
    // is the nominal gap start within the B extract window, stored on the spec as `b_extract.b_mapped_start_frame`
    // (deriving it back from `b_extract_start_secs` would be lossy by up to 1/sr — a byte-parity break).
    structure_start_frame: usize,
    alignment_start_frame: usize,
    offset_nominal_start: usize,
    sample_rate: u32,
}

/// The executor's bracket `(RegionPatch, RegionPatchOutcome)` assembly. Recomputes the geometry slides
/// independently and builds the structs from the resolved/spec values — `dual_fit_used` is always `false`
/// for a bracket. **The authoritative bracket output path since 6b.3b** (was shadow-validated byte-identical
/// across the full suite in 6b.3a before the inline construction was removed).
fn execute_bracket_output(ctx: ExecuteBracketOutputCtx) -> (RegionPatch, RegionPatchOutcome) {
    let structure_slide_secs =
        (ctx.structure_start_frame as f64 - ctx.offset_nominal_start as f64) / ctx.sample_rate as f64;
    let waveform_slide_secs =
        (ctx.alignment_start_frame as f64 - ctx.structure_start_frame as f64) / ctx.sample_rate as f64;
    let align_adjustment_secs = structure_slide_secs + waveform_slide_secs;
    let patch = RegionPatch {
        b_samples: ctx.fill,
        gain: ctx.gain,
        a_start_frame: ctx.refined.start_frame,
        a_end_frame: ctx.refined.end_frame,
        crossfade_secs: ctx.crossfade_secs,
    };
    let outcome = RegionPatchOutcome::Patched {
        pre_correlation: ctx.final_pre,
        post_correlation: ctx.final_post,
        align_adjustment_secs,
        waveform_adjustment_secs: waveform_slide_secs,
        structure_trusted: ctx.structure_trusted,
        confidence: ctx.final_confidence,
        gap_start_adjust_frames: ctx.gap_start_adjust_frames,
        gap_end_adjust_frames: ctx.gap_end_adjust_frames,
        fit_used_boundary_grid: ctx.fit_used_boundary_grid,
        fit_boundary_grid_cells: ctx.fit_boundary_grid_cells,
        residual: ctx.residual,
        anchor_seam_used: ctx.anchor_seam_used,
        anchor_bracket_move_frames: ctx.anchor_bracket_move_frames,
        dual_fit_used: false,
    };
    (patch, outcome)
}

/// What `characterize_region` decided for one region (6b.3e). A `Patch` carries the spec to execute plus the
/// transitional pre-assembled bracket fill (`None` for dual-fit, whose fill is on the spec); a `Skip` carries
/// its already-built outcome — skips are not executed (§2.5.5), so there is nothing for the executor to do.
/// (Skips are not yet full `Skip`-verdict specs; the fingerprint projection at step 8 will need that, tracked
/// separately.)
// A transient per-region value, built once and matched immediately — boxing the large `Patch` variant would
// add a pointless heap alloc, so the size disparity is deliberate.
#[allow(clippy::large_enum_variant)]
pub(super) enum RegionCharacterization {
    Patch {
        spec: GapRepairSpec,
        bracket_fill: Option<Vec<f32>>,
    },
    /// A reasoned/mechanical skip — now a full `Skip`-verdict [`GapRepairSpec`] (8d), not a bare outcome, so
    /// every region yields a projectable spec (§2.5.5). The loop/shim derives the [`RegionPatchOutcome`] from
    /// it via [`skip_outcome_from_spec`]; downstream reporting/tiering is byte-identical.
    Skip(GapRepairSpec),
}

/// What `skip_or_dual_fit` decided, **before** the region geometry is attached (8c). The dual-fit call site
/// lacks `b_extract` / `gap_offset` / `gap_index`, so on a rescue it returns the SilenceSplice *strategy* plus
/// the base geometry it does own (from the dual-fit input); [`finalize_dual_fit`] — called from
/// `characterize_region`, which holds the region geometry — wraps it into a full [`GapRepairSpec`]. This keeps
/// geometry ownership in one place (the decided 8c approach) instead of stamping inert placeholders.
enum DualFitDecision {
    /// Dual-fit rescued the gap: the SilenceSplice strategy + its D/R tags + the base A geometry
    /// (`refined`/`crossfade` from the pre-gate dual-fit input). Boxed — the payload is large relative to
    /// [`DualFitDecision::Skipped`] (clippy `large_enum_variant`).
    Rescued(Box<DualFitRescue>),
    /// Dual-fit declined or was ineligible — the skip `reason` + gate `residual`. `finalize_dual_fit` wraps it
    /// into a `Skip`-verdict spec with the region geometry (8c/8d), same as the rescue arm.
    Skipped {
        reason: GapPatchSkipReason,
        residual: Option<policies::SeamResidualVerdict>,
    },
}

/// The dual-fit rescue payload — everything `skip_or_dual_fit` owns before `characterize_region` attaches the
/// region geometry (8c).
struct DualFitRescue {
    strategy: GapRepairStrategy,
    tags_ctx: GapRepairTags,
    refined: RefinedGapFrames,
    crossfade_secs: f64,
}

/// Wrap a [`DualFitDecision`] into a [`RegionCharacterization`], attaching the region geometry the dual-fit
/// call site lacked (`gap_offset`, `b_extract`). `gap_index` stays `0` — assigned at the fingerprint
/// projection (8e/8f), same as the bracket path — so it is not SilenceSplice-specific inert geometry.
fn finalize_dual_fit(
    decision: DualFitDecision,
    region: &FillRegion,
    gap_offset_secs: f64,
    b_extract: BExtractWindow,
    gap_refined: RefinedGapFrames,
) -> RegionCharacterization {
    match decision {
        DualFitDecision::Rescued(rescue) => {
            let DualFitRescue { strategy, tags_ctx, refined, crossfade_secs } = *rescue;
            let spec = GapRepairSpec {
                gap_index: 0,
                a_start_secs: region.a_start_secs,
                a_end_secs: region.a_end_secs,
                gap_offset_secs,
                refined,
                b_extract,
                crossfade_secs,
                verdict: GapRepairVerdict::Patch(strategy),
                tags_ctx,
            };
            RegionCharacterization::Patch { spec, bracket_fill: None }
        }
        DualFitDecision::Skipped { reason, residual } => {
            RegionCharacterization::Skip(skip_region_spec(
                reason, residual, region, gap_offset_secs, gap_refined, b_extract,
            ))
        }
    }
}

/// Build a `Skip`-verdict [`GapRepairSpec`] (8d). Carries the `reason` (in the verdict) and `residual` (in
/// `tags_ctx.gate.residual`) so [`skip_outcome_from_spec`] reproduces the identical [`RegionPatchOutcome`].
/// The cell is derived from the reason + whatever tags are available (`skip_cell_from_tags`); on the decline
/// path tags are partial (no `seam_local`) until 8f, so a class-3/4 decline reports as `Decorrelated` for now
/// — cell correctness is a projection (8f) concern, not an outcome one (§8b guard).
fn skip_region_spec(
    reason: GapPatchSkipReason,
    residual: Option<policies::SeamResidualVerdict>,
    region: &FillRegion,
    gap_offset_secs: f64,
    refined: RefinedGapFrames,
    b_extract: BExtractWindow,
) -> GapRepairSpec {
    let mut tags_ctx = GapRepairTags::default();
    tags_ctx.gate.residual = residual;
    let cell = crate::domain::gap_repair_spec::skip_cell_from_tags(&reason, &tags_ctx);
    GapRepairSpec {
        gap_index: 0,
        a_start_secs: region.a_start_secs,
        a_end_secs: region.a_end_secs,
        gap_offset_secs,
        refined,
        b_extract,
        crossfade_secs: region.crossfade_secs,
        verdict: GapRepairVerdict::skip_with_cell(cell, reason),
        tags_ctx,
    }
}

/// Reproduce the [`RegionPatchOutcome::Skipped`] for a `Skip`-verdict spec (8d) — `reason` from the verdict,
/// `residual` from `tags_ctx.gate.residual`, the two fields `RegionPatchOutcome::Skipped` carries. Byte-
/// identical to the pre-8d `skipped_patch` / `seam_failure_outcome` outputs.
pub(super) fn skip_outcome_from_spec(spec: &GapRepairSpec) -> RegionPatchOutcome {
    match &spec.verdict {
        GapRepairVerdict::Skip { reason, .. } => RegionPatchOutcome::Skipped {
            reason: reason.clone(),
            residual: spec.tags_ctx.gate.residual,
        },
        GapRepairVerdict::Patch(_) => {
            unreachable!("skip_outcome_from_spec called on a Patch-verdict spec")
        }
    }
}

/// Reproduce the [`RegionPatchOutcome::Patched`] the executor would emit for a `Patch`-verdict spec —
/// decision fields only, no PCM assembly. Used by `--repair-preview` (characterize without execute).
pub(super) fn patched_outcome_from_spec(spec: &GapRepairSpec, sample_rate: u32) -> RegionPatchOutcome {
    match &spec.verdict {
        GapRepairVerdict::Patch(GapRepairStrategy::Bracket {
            alignment,
            structure_start_frame,
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
            ..
        }) => {
            let offset_nominal_start = spec.b_extract.b_mapped_start_frame;
            let structure_slide_secs = (*structure_start_frame as f64 - offset_nominal_start as f64)
                / sample_rate as f64;
            let waveform_slide_secs = (alignment.start_frame as f64 - *structure_start_frame as f64)
                / sample_rate as f64;
            RegionPatchOutcome::Patched {
                pre_correlation: *seam_pre,
                post_correlation: *seam_post,
                align_adjustment_secs: structure_slide_secs + waveform_slide_secs,
                waveform_adjustment_secs: waveform_slide_secs,
                structure_trusted: *structure_trusted,
                confidence: *confidence,
                gap_start_adjust_frames: *gap_start_adjust_frames,
                gap_end_adjust_frames: *gap_end_adjust_frames,
                fit_used_boundary_grid: *fit_used_boundary_grid,
                fit_boundary_grid_cells: *fit_boundary_grid_cells,
                residual: *residual,
                anchor_seam_used: *anchor_seam_used,
                anchor_bracket_move_frames: *anchor_bracket_move_frames,
                dual_fit_used: false,
            }
        }
        GapRepairVerdict::Patch(GapRepairStrategy::SilenceSplice {
            pre_seam_r,
            post_seam_r,
            residual,
            confidence,
            ..
        }) => RegionPatchOutcome::Patched {
            pre_correlation: *pre_seam_r,
            post_correlation: *post_seam_r,
            align_adjustment_secs: 0.0,
            waveform_adjustment_secs: 0.0,
            structure_trusted: false,
            confidence: *confidence,
            gap_start_adjust_frames: 0,
            gap_end_adjust_frames: 0,
            fit_used_boundary_grid: false,
            fit_boundary_grid_cells: None,
            residual: *residual,
            anchor_seam_used: false,
            anchor_bracket_move_frames: 0,
            dual_fit_used: true,
        },
        GapRepairVerdict::Skip { .. } => {
            unreachable!("patched_outcome_from_spec called on a Skip-verdict spec")
        }
    }
}

/// Decision-only outcome from a characterization (preview / report path — no PCM).
pub(super) fn outcome_from_characterization(
    characterization: &RegionCharacterization,
    sample_rate: u32,
) -> RegionPatchOutcome {
    match characterization {
        RegionCharacterization::Patch { spec, .. } => patched_outcome_from_spec(spec, sample_rate),
        RegionCharacterization::Skip(spec) => skip_outcome_from_spec(spec),
    }
}

/// Adapt a `seam_failure_outcome` result (always a `Skipped`) into a [`DualFitDecision::Skipped`] so
/// `finalize_dual_fit` can wrap it into a `Skip`-verdict spec with the region geometry (8d).
fn dual_fit_skipped(outcome: RegionPatchOutcome) -> DualFitDecision {
    match outcome {
        RegionPatchOutcome::Skipped { reason, residual } => {
            DualFitDecision::Skipped { reason, residual }
        }
        RegionPatchOutcome::Patched { .. } => {
            unreachable!("seam_failure_outcome always yields a skip outcome")
        }
    }
}

/// The executor entry: produce `(patch, outcome)` from a `GapRepairSpec` — the typed characterize→execute
/// handoff. Handles the two PATCH verdicts; `Skip` isn't *executed* (the loop filters skips and derives their
/// outcome from the spec, §2.5.5). `bracket_fill` is the transitional pre-assembled bracket fill (6b.3c) —
/// `Some` for `Bracket`, `None` for `SilenceSplice` (whose fill is on the spec); 6c re-derives the bracket
/// fill from `FillAlignment` and drops this param. Takes `spec` by value so the SilenceSplice fill moves out
/// without a clone.
pub(super) fn execute_region_spec(
    spec: GapRepairSpec,
    bracket_fill: Option<Vec<f32>>,
    sample_rate: u32,
) -> (Option<RegionPatch>, RegionPatchOutcome) {
    let GapRepairSpec { refined, crossfade_secs, b_extract, verdict, .. } = spec;
    match verdict {
        GapRepairVerdict::Patch(GapRepairStrategy::Bracket {
            alignment,
            structure_start_frame,
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
            ..
        }) => {
            let (patch, outcome) = execute_bracket_output(ExecuteBracketOutputCtx {
                fill: bracket_fill.expect("bracket verdict requires a bracket fill until 6c re-derivation"),
                gain: normalize_gain,
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
                structure_start_frame,
                alignment_start_frame: alignment.start_frame,
                offset_nominal_start: b_extract.b_mapped_start_frame,
                sample_rate,
            });
            (Some(patch), outcome)
        }
        GapRepairVerdict::Patch(GapRepairStrategy::SilenceSplice {
            fill,
            pre_seam_r,
            post_seam_r,
            residual,
            confidence,
            ..
        }) => {
            // Dual-fit rescue: the fill is the synthesized bridge on the spec; seams sit at their own lags, so
            // there is no align/waveform slide and no structure trust.
            let patch = RegionPatch {
                b_samples: fill,
                gain: 1.0,
                a_start_frame: refined.start_frame,
                a_end_frame: refined.end_frame,
                crossfade_secs,
            };
            let outcome = RegionPatchOutcome::Patched {
                pre_correlation: pre_seam_r,
                post_correlation: post_seam_r,
                align_adjustment_secs: 0.0,
                waveform_adjustment_secs: 0.0,
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
            };
            (Some(patch), outcome)
        }
        GapRepairVerdict::Skip { .. } => {
            unreachable!("skips are not executed — the loop derives their outcome from the spec (§2.5.5)")
        }
    }
}

/// **Characterize** one region (6b.3e): run geometry → gate → dual-fit/reconciliation and decide a
/// [`RegionCharacterization`] — a `Patch` spec to execute, or an already-built `Skip` outcome. Does NOT
/// assemble the final patch; that is `execute_region_spec`, invoked by the `prepare_region_patch` shim. This
/// is the characterize→execute boundary (§2.5): all repair *decisions* live here; PCM *assembly* lives in the
/// executor.
pub(super) fn characterize_region(
    progress: &dyn ProgressReporter,
    media: &RegionPatchMedia<'_>,
    region: &FillRegion,
    request: &PatchAudioRequest,
    ctx: &RegionPatchContext,
    opts: RegionPatchOpts<'_>,
) -> (RegionCharacterization, GapTagsPatchContext) {
    let RegionPatchMedia {
        b_samples_full,
        a_pcm,
    } = *media;
    let RegionPatchOpts {
        anchored_retry_pass,
        patch_anchors,
    } = opts;
    let &RegionPatchContext {
        channels,
        sample_rate,
        max_refine_frames,
        global_a_rms,
        silence_peak_fraction,
    } = ctx;

    // Perf instrumentation (Level B, TEMP-production-repair-perf-plan.md §0): time the whole characterize
    // pass per gap; the `char_*` child spans below split the shared-object rebuilds (b-extract / geometry /
    // dual-fit downmix) from the gate search. Emits only when `CLIP_SYNC_SPAN_TIMING` is set (Level A). This
    // span nests under `patch_audio` (pass 1) or `patch_anchored_retry` (the retry pass), so anchored-retry
    // characterize time stays bucketed separately per §0.
    let _characterize_span = tracing::info_span!("characterize").entered();

    let normalize_window_secs = request.normalize_window_secs;
    let margin_secs = request.fill_align_margin_secs;
    let border_search_secs = request.fill_border_search_secs;
    let min_border_discovery_secs = request.min_border_discovery_secs;
    let border_standoff_secs = request.border_standoff_secs;
    let fill_length_slack_secs = request.fill_length_slack_secs;
    let fill_seam_search_secs = request.fill_seam_search_secs;
    let gap_signature_context_secs = request.gap_signature_context_secs;
    let gap_signature_bin_ms = request.gap_signature_bin_ms;
    let min_structure_match_score = request.min_structure_match_score;
    let min_fill_correlation = request.min_fill_correlation;
    let normalize_fill = request.normalize_fill;
    let max_fill_gain_db = request.max_fill_gain_db;
    let absolute_silence_rms = request.absolute_silence_rms;
    let fill_offset_mode = request.fill_offset_mode;
    let gap_end_extend_on_post_seam_fail = request.gap_end_extend_on_post_seam_fail;
    let gap_start_extend_on_pre_seam_fail = request.gap_start_extend_on_pre_seam_fail;
    let gap_end_extend_max_ms = request.gap_end_extend_max_ms;
    let gap_end_extend_step_ms = request.gap_end_extend_step_ms;

    debug_assert!(
        region.b_start_secs >= 0.0,
        "fill plan must not include gaps with negative B start"
    );

    let gap_time_on_a = (region.a_start_secs + region.a_end_secs) / 2.0;
    let gap_offset_secs = resolve_gap_offset_secs(
        &request.report.alignment,
        fill_offset_mode,
        gap_time_on_a,
        patch_anchors,
        anchored_retry_pass,
    )
    .unwrap_or(region.b_start_secs - region.a_start_secs);

    if anchored_retry_pass == AnchoredRetryPass::Second {
        if let Some(table) = patch_anchors.filter(|t| !t.is_empty()) {
            progress.phase_verbose(&format_anchored_offset_verbose_line(
                gap_offset_secs,
                &request.report.alignment,
                gap_time_on_a,
                table,
            ));
        }
    }

    let reported_start_frame = (region.a_start_secs * sample_rate as f64) as usize;
    let reported_end_frame = (region.a_end_secs * sample_rate as f64) as usize;
    let mut refined: RefinedGapFrames = policies::refine_gap_frames(
        &a_pcm.samples,
        channels,
        reported_start_frame,
        reported_end_frame,
        silence_peak_fraction,
        absolute_silence_rms,
        max_refine_frames,
    );

    let refined_a_start_secs = refined.start_frame as f64 / sample_rate as f64;
    let refined_a_end_secs = refined.end_frame as f64 / sample_rate as f64;
    let refined_b_start_secs = refined_a_start_secs + gap_offset_secs;
    let refined_b_end_secs = refined_a_end_secs + gap_offset_secs;
    let a_start_secs = refined_a_start_secs;

    if !matches!(
        fill_offset_mode,
        FillOffsetMode::Recommended
            | FillOffsetMode::AnchoredRetry if anchored_retry_pass == AnchoredRetryPass::First
    ) {
        tracing::debug!(
            a_start_secs,
            gap_offset_secs,
            mode = ?fill_offset_mode,
            "using per-gap fill offset"
        );
    }

    let context_frames =
        (gap_signature_context_secs * sample_rate as f64).round() as usize;
    let bin_frames =
        ((gap_signature_bin_ms as f64 / 1000.0) * sample_rate as f64).round() as usize;
    let search_radius_secs = border_search_secs.max(margin_secs);
    let extend_slack_secs = gap_extension_slack_secs(repair_patch_config_view(request));
    let b_extract_start_secs = (refined_b_start_secs
        - gap_signature_context_secs
        - search_radius_secs
        - margin_secs
        - extend_slack_secs)
        .max(0.0);
    let length_slack_secs = fill_length_slack_secs.max(margin_secs);
    let b_extract_end_secs = refined_b_end_secs
        + gap_signature_context_secs
        + search_radius_secs
        + length_slack_secs
        + margin_secs
        + extend_slack_secs;

    let gap_frames_preview = refined.end_frame.saturating_sub(refined.start_frame);
    let signature_mode_label = if request.fill_mode == FillMode::Fit && gap_frames_preview > 0 {
        let structure_params = StructureMatchParams {
            gap_frames: gap_frames_preview,
            bin_frames: bin_frames.max(1),
            search_radius_frames: 0,
            fill_length_slack_frames: 0,
            max_fine_adjustment_frames: 0,
            silence_peak_fraction,
            absolute_silence_rms,
        };
        build_gap_signature(
            &a_pcm.samples,
            channels,
            refined.start_frame,
            refined.end_frame,
            context_frames,
            &structure_params,
            request.gap_signature_mode,
        )
        .mode_label()
    } else {
        "bool"
    };

    let mut tag_ctx = GapTagsPatchContext {
        fill_mode: request.fill_mode,
        thresholds: FillTierThresholds {
            min_fill_correlation: request.min_fill_correlation,
            fill_marginal_margin: request.fill_marginal_margin,
            fill_absolute_floor: request.fill_absolute_floor,
        },
        signature_mode_label,
        fit_used_boundary_grid: false,
        anchor_seam_used: false,
        anchor_bracket_move_frames: 0,
        anchor_trusted: false,
        dual_fit_used: false,
        residual: None,
        residual_headroom_margin_db: request.residual_headroom_margin_db,
    };

    log_gap_fill_plan_verbose(
        progress,
        &GapFillPlanLog {
            scan_a_start_secs: region.a_start_secs,
            scan_a_end_secs: region.a_end_secs,
            refined_a_start_secs,
            refined_a_end_secs,
            gap_offset_secs,
            fill_offset_mode,
            mapped_b_start_secs: refined_b_start_secs,
            mapped_b_end_secs: refined_b_end_secs,
            b_search_start_secs: b_extract_start_secs,
            b_search_end_secs: b_extract_end_secs,
            signature_mode_label,
        },
    );

    let gap_frames = refined.end_frame.saturating_sub(refined.start_frame);
    if gap_frames == 0 {
        // Pre-placement mechanical skip: no B window established yet (`b_extract` is the not-measured zero
        // window — full geometry for such skips is an 8f concern; the outcome ignores it).
        return (
            RegionCharacterization::Skip(skip_region_spec(
                GapPatchSkipReason::ZeroLengthGap, None, region, gap_offset_secs, refined,
                BExtractWindow { start_frame: 0, end_frame: 0, b_mapped_start_frame: 0 },
            )),
            tag_ctx,
        );
    }

    let correlate_frames = correlate_frames_for_gap(
        normalize_window_secs,
        min_border_discovery_secs,
        gap_frames,
        sample_rate,
    );
    let seam_gate_frames =
        seam_gate_frames_for(correlate_frames, fill_seam_search_secs, sample_rate);
    let b_extract_result = {
        let _s = tracing::info_span!("char_b_extract").entered();
        slice_b_segment(
            b_samples_full,
            channels,
            sample_rate,
            b_extract_start_secs,
            b_extract_end_secs,
        )
    };
    let b_samples = match b_extract_result {
        Some(samples) => samples,
        None => {
            let reason = GapPatchSkipReason::BExtractFailed;
            log_skip_gap_fill(
                progress,
                &request.report.gaps,
                region.a_start_secs,
                region.a_end_secs,
                &reason,
            );
            return (
                RegionCharacterization::Skip(skip_region_spec(
                    reason, None, region, gap_offset_secs, refined,
                    BExtractWindow { start_frame: 0, end_frame: 0, b_mapped_start_frame: 0 },
                )),
                tag_ctx,
            );
        }
    };

    let border_frames = border_frames_from_secs(normalize_window_secs, sample_rate)
        .min(correlate_frames);
    let border_standoff_frames =
        (border_standoff_secs * sample_rate as f64).round() as usize;
    let search_radius_frames =
        (border_search_secs * sample_rate as f64).round() as usize;
    let max_extend_frames =
        (gap_end_extend_max_ms as f64 / 1000.0 * sample_rate as f64).round() as usize;
    let step_frames =
        (gap_end_extend_step_ms as f64 / 1000.0 * sample_rate as f64).round() as usize;

    let derived =
        SeamGateDerived::from_repair(request, sample_rate, channels, silence_peak_fraction);
    let anchor_search_prior = anchor_search_prior_for_gap(
        request,
        patch_anchors,
        anchored_retry_pass,
        region,
        b_extract_start_secs,
        search_radius_frames,
        sample_rate,
    );
    let geom = {
        let _s = tracing::info_span!("char_geometry").entered();
        derive_seam_gate_geometry(
            &request.settings,
            &derived,
            a_pcm,
            b_samples,
            b_extract_start_secs,
            refined_b_start_secs,
            refined_b_end_secs,
            gap_frames,
            anchor_search_prior,
        )
    };
    let seam_params = SeamGateParams {
        settings: &request.settings,
        derived,
        geom,
    };

    // A3: build the dual-fit fallback input once (only when `--dual-fit` is on), before the gate can mutate
    // `refined` via boundary-extension retry — dual-fit works on the base gap geometry.
    let dual_fit_input = request
        .dual_fit
        .then(|| {
            let _s = tracing::info_span!("char_dual_fit_input").entered();
            build_dual_fit_input(DualFitInputSpec {
                a_samples: &a_pcm.samples,
                b_samples,
                channels,
                sample_rate,
                refined,
                refined_b_start_secs,
                b_extract_start_secs,
                gap_frames,
                fill_seam_search_secs,
                min_fill_correlation,
                fill_absolute_floor: request.fill_absolute_floor,
                crossfade_secs: region.crossfade_secs,
                border_standoff_frames: derived.border_standoff_frames,
            })
        })
        .flatten();

    // 8c: the B extract window for a dual-fit rescue spec — real geometry (`b_mapped_start` is the extract-
    // relative nominal gap start the dual-fit input already computed). Built here so `finalize_dual_fit` can
    // attach it; the executor's SilenceSplice arm doesn't read it, so this is PCM-neutral (byte-parity).
    let silence_splice_b_extract = BExtractWindow {
        start_frame: (b_extract_start_secs * sample_rate as f64).round() as usize,
        end_frame: (b_extract_end_secs * sample_rate as f64).round() as usize,
        b_mapped_start_frame: dual_fit_input.as_ref().map(|d| d.b_mapped_start).unwrap_or(0),
    };

    let gate_result = {
        let _s = tracing::info_span!("char_gate_search").entered();
        evaluate_seam_gate(refined, &seam_params)
    };
    let gate_outcome = match gate_result {
        Ok(outcome) => outcome,
        Err(fail)
            if request.fill_mode == crate::domain::FillMode::Gate
                && (gap_end_extend_on_post_seam_fail || gap_start_extend_on_pre_seam_fail) =>
        {
            match fail {
                SeamGateFailure::WaveformBelowThreshold { .. } => {
                    match retry_waveform_seam_extensions(
                        &mut refined,
                        gap_offset_secs,
                        &seam_params,
                        fail,
                        SeamExtensionRetry {
                            max_extend_frames,
                            step_frames: step_frames.max(1),
                            gap_end_extend_on_post_seam_fail,
                            gap_start_extend_on_pre_seam_fail,
                        },
                    ) {
                        Ok(outcome) => outcome,
                        Err(retry_fail) => {
                            let (decision, tag_ctx) = skip_or_dual_fit(
                                progress,
                                request,
                                region,
                                retry_fail,
                                min_structure_match_score,
                                tag_ctx,
                                dual_fit_input.as_ref(),
                            );
                            return (
                                finalize_dual_fit(decision, region, gap_offset_secs, silence_splice_b_extract, refined),
                                tag_ctx,
                            );
                        }
                    }
                }
                other => {
                    let (decision, tag_ctx) = skip_or_dual_fit(
                        progress,
                        request,
                        region,
                        other,
                        min_structure_match_score,
                        tag_ctx,
                        dual_fit_input.as_ref(),
                    );
                    return (
                        finalize_dual_fit(decision, region, gap_offset_secs, silence_splice_b_extract, refined),
                        tag_ctx,
                    );
                }
            }
        }
        Err(other) => {
            let (decision, tag_ctx) = skip_or_dual_fit(
                progress,
                request,
                region,
                other,
                min_structure_match_score,
                tag_ctx,
                dual_fit_input.as_ref(),
            );
            return (
                finalize_dual_fit(decision, region, gap_offset_secs, silence_splice_b_extract, refined),
                tag_ctx,
            );
        }
    };

    let SeamGateOutcome {
        refined,
        alignment,
        report_pre,
        report_post,
        structure_trusted: patched_structure_trusted,
        structure_start_frame,
        gap_frames,
        confidence,
        gap_start_adjust_frames,
        gap_end_adjust_frames,
        fit_used_boundary_grid,
        fit_boundary_grid_cells,
        fit_haystack_secs,
        residual: residual_verdict,
        anchor_seam_used,
        anchor_bracket_move_frames,
        anchor_trusted,
        ..
    } = gate_outcome;
    tag_ctx.fit_used_boundary_grid = fit_used_boundary_grid;
    tag_ctx.anchor_seam_used = anchor_seam_used;
    tag_ctx.anchor_bracket_move_frames = anchor_bracket_move_frames;
    tag_ctx.anchor_trusted = anchor_trusted;

    if confidence == FillConfidence::Marginal {
        log_marginal_gap_fill(
            progress,
            &MarginalGapFillLog {
                gaps: &request.report.gaps,
                a_start_secs: region.a_start_secs,
                a_end_secs: region.a_end_secs,
                pre: report_pre,
                post: report_post,
                min: request.min_fill_correlation,
                anchor_seam: anchor_seam_used,
            },
        );
    }

    let refined_b_start_secs = refined.start_frame as f64 / sample_rate as f64 + gap_offset_secs;

    let offset_nominal_start =
        ((refined_b_start_secs - b_extract_start_secs) * sample_rate as f64).round() as usize;

    let structure_slide_secs = (structure_start_frame as f64 - offset_nominal_start as f64)
        / sample_rate as f64;
    let waveform_slide_secs =
        (alignment.start_frame as f64 - structure_start_frame as f64) / sample_rate as f64;
    // `align_adjustment_secs` (= structure + waveform slide) is recomputed inside `execute_bracket_output`
    // from the placement fields; `structure_slide_secs`/`waveform_slide_secs` remain for the verbose log.

    let fill_start_sample = alignment.start_frame * channels;
    let b_fill_end_sample = fill_start_sample + alignment.fill_frames * channels;
    if b_fill_end_sample > b_samples.len() {
        let reason = GapPatchSkipReason::AlignedSegmentOutOfRange;
        log_skip_gap_fill(
            progress,
            &request.report.gaps,
            region.a_start_secs,
            region.a_end_secs,
            &reason,
        );
        return (
            RegionCharacterization::Skip(skip_region_spec(
                reason, None, region, gap_offset_secs, refined, silence_splice_b_extract,
            )),
            tag_ctx,
        );
    }

    let b_fill_raw = b_samples[fill_start_sample..b_fill_end_sample].to_vec();
    let source_frames = b_fill_raw.len() / channels;
    let b_extension = if b_fill_end_sample < b_samples.len() {
        &b_samples[b_fill_end_sample..]
    } else {
        &[][..]
    };
    if source_frames > gap_frames {
        tracing::debug!(
            a_start_secs,
            b_fill_frames = source_frames,
            a_gap_frames = gap_frames,
            "B fill longer than A gap; choosing trim anchor (fit) or trimming tail (gate)"
        );
    }
    let border_spec = GapBorderSpec {
        gap_start_frame: refined.start_frame,
        gap_end_frame: refined.end_frame,
        border_frames,
        border_standoff_frames,
        silence_peak_fraction,
        absolute_rms_floor: absolute_silence_rms,
    };
    let (a_pre_border, a_post_border) =
        policies::border_templates_for_gap(&a_pcm.samples, channels, &border_spec);
    let (a_pre_ch, a_post_ch) = policies::border_templates_per_channel_for_gap(
        &a_pcm.samples,
        channels,
        &border_spec,
    );
    let pre_gate_frames = seam_gate_frames.min(a_pre_border.len().max(1));
    let post_gate_frames = if a_post_border.is_empty() {
        0
    } else {
        seam_gate_frames.min(a_post_border.len()).max(1)
    };
    let repeat_window_frames = border_frames.max(1);
    let seam_cf = policies::effective_seam_crossfade_frames(
        (region.crossfade_secs * sample_rate as f64) as usize,
        refined.start_frame,
        refined.end_frame,
        a_pcm.frames(),
    );
    let seam_ctx = policies::SpliceSeamContext {
        seam_cf,
        gap_start_frame: refined.start_frame,
        gap_end_frame: refined.end_frame,
        a_samples: &a_pcm.samples,
        channels,
        single_lag_alignment: true,
    };
    // M0 perf instrumentation (TEMP-patch-audio-bracket-fill-elimination-plan.md §3): time the fill
    // assembly itself. `char_gate_search` closes well before this point, so the assembly was previously
    // unmeasured — this span decides whether re-deriving the fill in execute needs a dedup hoist first.
    // `fill_mode` is request-level, so the Fit/Gate split comes from running one measurement per mode.
    let BracketFill { pcm: b_fill, extended_frames } = {
        let _s = tracing::info_span!("char_fill_assembly").entered();
        assemble_bracket_fill(BracketFillAssembly {
            b_fill_raw,
            b_extension,
            channels,
            gap_frames,
            fill_mode: request.fill_mode,
            a_pre_border: &a_pre_border,
            a_post_border: &a_post_border,
            a_pre_ch: &a_pre_ch,
            a_post_ch: &a_post_ch,
            pre_gate_frames,
            post_gate_frames,
            repeat_window_frames,
            seam_ctx,
        })
    };
    // L0: the primitive returns the fact, the caller reports it — same message and fields as the emission
    // that used to live inside `assemble_bracket_fill`, so the log stream is unchanged. Reporting here (not
    // in the primitive) is what lets the executor re-derive the fill without double-emitting.
    if let Some(extended_frames) = extended_frames {
        tracing::debug!(
            a_start_secs,
            extended_frames,
            "B bracket shorter than A gap; extended from contiguous B audio (gate)"
        );
    }

    // 6b.3a shadow: prove the executor can re-derive this exact fill from the spec's `FillAlignment` +
    // decode buffers alone (rebuilding borders independently). Guards the characterize→execute split before
    // 6b.3b removes the inline path. Debug-only; the release path is unchanged.
    debug_assert_eq!(
        execute_bracket_fill(ExecuteBracketFillCtx {
            alignment,
            b_samples,
            a_samples: &a_pcm.samples,
            a_frames: a_pcm.frames(),
            refined,
            channels,
            gap_frames,
            fill_mode: request.fill_mode,
            border_frames,
            border_standoff_frames,
            silence_peak_fraction,
            absolute_silence_rms,
            seam_gate_frames,
            sample_rate,
            crossfade_secs: region.crossfade_secs,
        }),
        b_fill,
        "6b.3a: executor bracket-fill reconstruction must byte-match the inline fill"
    );

    let gain = if normalize_fill {
        let border_rms = compute_a_border_rms(
            a_pcm,
            refined.start_frame,
            refined.end_frame,
            normalize_window_secs,
            global_a_rms,
        );
        let b_rms = policies::rms_interleaved(&b_fill);
        policies::compute_fill_gain(border_rms, b_rms, max_fill_gain_db)
    } else {
        1.0f32
    };

    log_gap_fill_result_verbose(
        progress,
        &GapFillResultLog {
            b_search_start_secs: b_extract_start_secs,
            sample_rate,
            channels,
            fill_start_sample,
            fill_end_sample: b_fill_end_sample,
            structure_slide_secs,
            waveform_slide_secs,
            fit_used_boundary_grid,
            fit_boundary_grid_cells,
            fit_haystack_secs,
            report_pre,
            report_post,
            confidence,
        },
    );

    // Report-vs-splice reconciliation (§2.5.7 #4). `used_splice` is captured for the spec's placement
    // provenance (§4.3): false = the gate-throat report seam won, true = the assembled-fill splice seam.
    let (final_pre, final_post, final_confidence, used_splice) = if patched_structure_trusted {
        (report_pre, report_post, confidence, false)
    } else if request.fill_mode == FillMode::Fit {
        let (splice_pre, splice_post) = policies::fill_splice_seam_correlations_interleaved(
            &b_fill,
            channels,
            &policies::BorderSeamTemplates {
                a_pre: &a_pre_border,
                a_post: &a_post_border,
                a_pre_ch: &a_pre_ch,
                a_post_ch: &a_post_ch,
                pre_window: pre_gate_frames,
                post_window: post_gate_frames,
            },
            seam_ctx,
        );
        let gate_min = report_pre.min(report_post);
        let splice_min = splice_pre.min(splice_post);
        let use_splice = splice_min >= gate_min;
        let (pre, post) = if use_splice {
            (splice_pre, splice_post)
        } else {
            (report_pre, report_post)
        };
        let splice_confidence = if use_splice {
            classify_fill_waveform_confidence(
                splice_pre,
                splice_post,
                min_fill_correlation,
                request.fill_marginal_margin,
                request.fill_absolute_floor,
            )
            .unwrap_or(confidence)
        } else {
            confidence
        };
        (pre, post, splice_confidence, use_splice)
    } else {
        (report_pre, report_post, confidence, false)
    };

    // 6b.3c: build the typed `GapRepairSpec` — the characterize→execute handoff — and route through the
    // executor. The Bracket strategy carries the resolved repair-params; geometry (incl. the exact
    // `offset_nominal_start` as `b_mapped_start_frame`) is on the spec, so the executor reads everything from
    // it. `tags_ctx` is PARTIAL in 6b (the executor ignores it; step 8 populates the D/R payload). `b_fill`
    // is handed over for now — 6c re-derives it from `FillAlignment` when the passes split.
    let spec = GapRepairSpec {
        gap_index: 0, // not projected in 6b; the fingerprint index is assigned at step 8
        a_start_secs: region.a_start_secs,
        a_end_secs: region.a_end_secs,
        gap_offset_secs,
        refined,
        b_extract: BExtractWindow {
            start_frame: (b_extract_start_secs * sample_rate as f64).round() as usize,
            end_frame: (b_extract_end_secs * sample_rate as f64).round() as usize,
            b_mapped_start_frame: offset_nominal_start,
        },
        crossfade_secs: region.crossfade_secs,
        verdict: GapRepairVerdict::Patch(GapRepairStrategy::Bracket {
            alignment,
            structure_start_frame,
            structure_trusted: patched_structure_trusted,
            anchor_seam_used,
            anchor_bracket_move_frames,
            anchor_trusted,
            seam_pre: final_pre,
            seam_post: final_post,
            used_splice,
            confidence: final_confidence,
            gap_start_adjust_frames,
            gap_end_adjust_frames,
            fit_used_boundary_grid,
            fit_boundary_grid_cells,
            residual: residual_verdict,
            normalize_gain: gain,
        }),
        tags_ctx: GapRepairTags::default(),
    };
    (
        RegionCharacterization::Patch { spec, bracket_fill: Some(b_fill) },
        tag_ctx,
    )
}

/// Characterize every planned region into a spec — the §2.5.5 multi-region loop over [`characterize_region`].
/// **Region-infallible:** exactly one spec per region, every failure folded into a `Skip` verdict (§2.5.7 #3),
/// so `specs.len() == regions.len()`. Drops the executor/reporting extras (`bracket_fill` /
/// `GapTagsPatchContext`) — this yields the repair *decisions* without executing PCM. First-pass anchors only
/// (pass-2 anchored retry is out of scope for this helper; production preview is `PatchAudio::preview`).
///
/// Test-only: production `--repair-preview` runs the same loop inside `PatchAudio::preview` (with progress /
/// plan wiring). Kept here to pin region-infallible + disposition parity against `prepare_region_patch`.
#[cfg(test)]
fn characterize_all_regions(
    progress: &dyn ProgressReporter,
    media: &RegionPatchMedia<'_>,
    regions: &[FillRegion],
    request: &PatchAudioRequest,
    ctx: &RegionPatchContext,
) -> Vec<GapRepairSpec> {
    regions
        .iter()
        .map(|region| {
            let (characterization, _tag_ctx) = characterize_region(
                progress,
                media,
                region,
                request,
                ctx,
                RegionPatchOpts { anchored_retry_pass: AnchoredRetryPass::First, patch_anchors: None },
            );
            match characterization {
                RegionCharacterization::Patch { spec, .. } => spec,
                RegionCharacterization::Skip(spec) => spec,
            }
        })
        .collect()
}

/// Thin shim (6b.3e): characterize the region, then execute if it's a patch. The former target of this name;
/// the split lives in [`characterize_region`] + [`execute_region_spec`]. `sample_rate` for the executor comes
/// from the region context.
pub(super) fn prepare_region_patch(
    progress: &dyn ProgressReporter,
    media: &RegionPatchMedia<'_>,
    region: &FillRegion,
    request: &PatchAudioRequest,
    ctx: &RegionPatchContext,
    opts: RegionPatchOpts<'_>,
) -> (Option<RegionPatch>, RegionPatchOutcome, GapTagsPatchContext) {
    let sample_rate = ctx.sample_rate;
    let (characterization, tag_ctx) =
        characterize_region(progress, media, region, request, ctx, opts);
    match characterization {
        RegionCharacterization::Patch { spec, bracket_fill } => {
            let (patch, outcome) = execute_region_spec(spec, bracket_fill, sample_rate);
            (patch, outcome, tag_ctx)
        }
        RegionCharacterization::Skip(spec) => (None, skip_outcome_from_spec(&spec), tag_ctx),
    }
}

fn slice_b_segment(
    b_samples: &[f32],
    channels: usize,
    sample_rate: u32,
    start_secs: f64,
    end_secs: f64,
) -> Option<&[f32]> {
    let channels = channels.max(1);
    let start_frame = (start_secs * sample_rate as f64).round() as usize;
    let end_frame = ((end_secs * sample_rate as f64).round() as usize)
        .min(b_samples.len() / channels);
    if start_frame >= end_frame {
        return None;
    }
    Some(&b_samples[start_frame * channels..end_frame * channels])
}

/// Compute RMS of the A samples bordering the gap region.
///
/// Looks at `window_secs` before the gap start and `window_secs` after the gap end.
/// Returns `fallback` when the computed RMS is zero.
fn compute_a_border_rms(
    a_pcm: &MultiChannelPcm,
    gap_start_frame: usize,
    gap_end_frame: usize,
    window_secs: f64,
    fallback: f32,
) -> f32 {
    let channels = a_pcm.channels as usize;
    let sample_rate = a_pcm.sample_rate;
    let window_frames = border_frames_from_secs(window_secs, sample_rate);

    let pre_start = gap_start_frame.saturating_sub(window_frames);
    let pre_end = gap_start_frame;
    let post_start = gap_end_frame;
    let post_end = (gap_end_frame + window_frames).min(a_pcm.samples.len() / channels);

    let pre_samples = &a_pcm.samples[pre_start * channels..pre_end * channels];
    let post_samples = &a_pcm.samples[post_start * channels..post_end * channels];

    let total = pre_samples.len() + post_samples.len();
    if total == 0 {
        return fallback;
    }

    let sum_sq: f64 = pre_samples
        .iter()
        .chain(post_samples.iter())
        .map(|&s| {
            let v = s as f64;
            v * v
        })
        .sum();

    let rms = (sum_sq / total as f64).sqrt() as f32;
    if rms == 0.0 { fallback } else { rms }
}

/// Splice B samples into A's interleaved sample buffer at the gap location.
pub(super) fn splice_into_a(
    a_samples: &mut [f32],
    b_samples: &[f32],
    channels: usize,
    gap_start_frame: usize,
    gap_end_frame: usize,
    crossfade_secs: f64,
    sample_rate: u32,
) {
    let channels = channels.max(1);

    if gap_start_frame * channels >= a_samples.len()
        || gap_end_frame * channels > a_samples.len()
    {
        tracing::warn!(
            gap_start_frame,
            gap_end_frame,
            a_len = a_samples.len() / channels,
            "splice_into_a: region out of range, skipping"
        );
        return;
    }

    if gap_start_frame >= gap_end_frame {
        return;
    }

    let gap_frames = gap_end_frame.saturating_sub(gap_start_frame);
    let needed_samples = gap_frames * channels;
    if b_samples.len() < needed_samples {
        tracing::warn!(
            gap_start_frame,
            gap_end_frame,
            b_len = b_samples.len(),
            needed_samples,
            "splice_into_a: B fill shorter than gap, skipping"
        );
        return;
    }

    let crossfade_frames = (crossfade_secs * sample_rate as f64) as usize;

    policies::apply_seam_crossfade(
        a_samples,
        b_samples,
        channels,
        gap_start_frame,
        gap_end_frame,
        crossfade_frames,
    );
}

#[cfg(test)]
mod tests {
    use super::{
        dual_fit_eligible, measure_dual_fit_residual_verdict, DualFitRepairInput, PatchAudioRequest,
        SeamGateFailure,
    };
    use crate::domain::gap::GapReport;
    use crate::domain::gap_fill_fit::{fit_fill_to_gap_frames, FillConfidence};

    /// **8g.2 — `characterize_all_regions` yields one consistent spec per region.** Region-infallible
    /// (`specs.len() == regions.len()`), and each spec's verdict agrees with what the `prepare_region_patch`
    /// shim executes for that region: a `Patch` verdict ⟺ the executor emits a `RegionPatch`; a `Skip` verdict
    /// ⟺ a `Skipped` outcome. (Both go through the same `characterize_region`, so this pins the multi-region
    /// loop's spec extraction against the live shim.)
    #[test]
    fn characterize_all_regions_yields_one_consistent_spec_per_region() {
        use super::{
            characterize_all_regions, patched_outcome_from_spec, prepare_region_patch,
            skip_outcome_from_spec, AnchoredRetryPass, GapRepairVerdict, RegionPatchContext,
            RegionPatchMedia, RegionPatchOpts, RegionPatchOutcome,
        };
        use crate::domain::gap::Gap;
        use crate::domain::gap_fill::FillRegion;
        use crate::domain::policies;
        use crate::domain::{GapReport, GapSignatureMode, ScanAlignment};
        use crate::infrastructure::config::RepairConfig;
        use clip_sync::MultiChannelPcm;
        use clip_sync_repair_fixtures::NoOpProgressReporter;

        let rate = 48_000u32;
        let secs = |s: f64| (s * f64::from(rate)) as usize;
        let total = secs(3.0);
        let (g0, g1) = (secs(1.0), secs(2.0));
        let sine = |buf: &mut [f32], start: usize, end: usize, freq: f64| {
            for (f, slot) in buf.iter_mut().enumerate().take(end.min(total)).skip(start) {
                *slot = (std::f64::consts::TAU * freq * (f as f64) / f64::from(rate)).sin() as f32 * 0.2;
            }
        };
        // A: content outside the gap, silent inside. B: content everywhere (fill available in the gap).
        let mut a = vec![0f32; total];
        let mut b = vec![0f32; total];
        sine(&mut a, 0, g0, 330.0);
        sine(&mut a, g1, total, 440.0);
        sine(&mut b, 0, total, 330.0);

        let a_pcm = MultiChannelPcm {
            sample_rate: rate,
            channels: 1,
            samples: a,
            decode_error_skips: 0,
            decoded_frame_count: None,
            compressed_bytes: None,
            source_bit_depth: None,
        };
        let report = GapReport {
            video_a: Default::default(),
            video_b: Default::default(),
            track_compatibility: None,
            alignment: ScanAlignment {
                clips: vec![],
                start_aligned: true,
                end_aligned: None,
                recommended_offset_secs: None,
                offsets_consistent: true,
                offset_drift_secs: None,
                start_overlap: None,
                query_reference_mode: false,
            },
            gaps: vec![Gap {
                video_a_start_secs: g0 as f64 / f64::from(rate),
                video_a_end_secs: g1 as f64 / f64::from(rate),
                video_b_start_secs: Some(g0 as f64 / f64::from(rate)),
                video_b_end_secs: Some(g1 as f64 / f64::from(rate)),
                b_has_energy: true,
            }],
            gap_equivalence: Vec::new(),
            gap_offset_agreement: None,
            decode_chunk_secs: 30,
            scan_block_ms: 20,
            silence_peak_fraction: 0.05,
            limit_fill_to_mapped_region: false,
            audio_timeline_skew: None,
        };
        let repair = RepairConfig {
            gap_signature_mode: GapSignatureMode::Energy,
            gap_signature_context_secs: 0.5,
            fill_border_search_secs: 0.05,
            fill_align_margin_secs: 0.02,
            fill_length_slack_secs: 0.1,
            fill_seam_search_secs: 0.05,
            border_standoff_secs: 0.0,
            max_anchor_bracket_secs: 0.2,
            max_anchors_per_side: 2,
            ..RepairConfig::default()
        };
        let request: PatchAudioRequest = repair.patch_settings().into_request(report.clone());

        let regions = vec![FillRegion {
            a_start_secs: g0 as f64 / f64::from(rate),
            a_end_secs: g1 as f64 / f64::from(rate),
            b_start_secs: g0 as f64 / f64::from(rate),
            b_end_secs: g1 as f64 / f64::from(rate),
            gain: 1.0,
            crossfade_secs: 0.01,
        }];
        let ctx = RegionPatchContext {
            channels: 1,
            sample_rate: rate,
            max_refine_frames: (0.05 * f64::from(rate)) as usize,
            global_a_rms: policies::rms_interleaved(&a_pcm.samples),
            silence_peak_fraction: 0.05,
        };
        let media = RegionPatchMedia { b_samples_full: &b, a_pcm: &a_pcm };
        let progress = NoOpProgressReporter;

        let specs = characterize_all_regions(&progress, &media, &regions, &request, &ctx);
        assert_eq!(specs.len(), regions.len(), "one spec per region (region-infallible)");

        for (spec, region) in specs.iter().zip(regions.iter()) {
            let (patch, outcome, _tags) = prepare_region_patch(
                &progress,
                &media,
                region,
                &request,
                &ctx,
                RegionPatchOpts { anchored_retry_pass: AnchoredRetryPass::First, patch_anchors: None },
            );
            let is_patch = matches!(spec.verdict, GapRepairVerdict::Patch(_));
            assert_eq!(is_patch, patch.is_some(), "Patch verdict ⟺ executor emits a RegionPatch");
            assert_eq!(
                !is_patch,
                matches!(outcome, RegionPatchOutcome::Skipped { .. }),
                "Skip verdict ⟺ Skipped outcome",
            );
            // Preview path: decision-only outcomes match the executor's dispositions (pass-1).
            let preview_outcome = if is_patch {
                patched_outcome_from_spec(spec, rate)
            } else {
                skip_outcome_from_spec(spec)
            };
            match (&preview_outcome, &outcome) {
                (RegionPatchOutcome::Patched { .. }, RegionPatchOutcome::Patched { .. })
                | (RegionPatchOutcome::Skipped { .. }, RegionPatchOutcome::Skipped { .. }) => {}
                _ => panic!("preview disposition diverged from execute for region"),
            }
        }
    }

    #[test]
    fn dual_fit_eligible_excludes_structure_alignment_failed() {
        // A2: `StructureAlignmentFailed` means structure search never scored a bracket, so there is
        // nothing "exhausted" for dual-fit to rescue — it must fall through to the ordinary skip
        // regardless of whether `--dual-fit` is on.
        assert!(!dual_fit_eligible(true, SeamGateFailure::StructureAlignmentFailed));
        assert!(!dual_fit_eligible(false, SeamGateFailure::StructureAlignmentFailed));

        // Other seam-gate failure variants are scored-but-failed skips, so they DO qualify when
        // `--dual-fit` is on, and never qualify when it's off.
        let scored_but_failed = SeamGateFailure::StructureBelowThreshold { pre: 0.1, post: 0.1 };
        assert!(dual_fit_eligible(true, scored_but_failed));
        assert!(!dual_fit_eligible(false, scored_but_failed));
    }

    #[test]
    fn fit_fill_trims_tail_without_resampling() {
        let channels = 2usize;
        let mut samples = Vec::new();
        for frame in 0..10i32 {
            samples.push(frame as f32 * 100.0 / 32767.0);
            samples.push(frame as f32 * 100.0 / 32767.0);
        }
        let fitted = fit_fill_to_gap_frames(&samples, channels, 6);
        assert_eq!(fitted.len(), 12);
        assert!((fitted[0] - 0.0).abs() < 1e-6);
        assert!((fitted[1] - 0.0).abs() < 1e-6);
        assert!((fitted[10] - 500.0 / 32767.0).abs() < 1e-5);
        assert!((fitted[11] - 500.0 / 32767.0).abs() < 1e-5);
    }

    #[test]
    fn fit_fill_zero_pads_short_source() {
        let v1 = 1000.0_f32 / 32767.0;
        let v2 = 2000.0_f32 / 32767.0;
        let samples = vec![v1, v1, v2, v2];
        let fitted = fit_fill_to_gap_frames(&samples, 2, 4);
        assert_eq!(fitted, vec![v1, v1, v2, v2, 0.0, 0.0, 0.0, 0.0]);
    }

    fn dual_fit_test_request(
        measure_residual: bool,
        residual_gate: crate::domain::ResidualGateMode,
    ) -> PatchAudioRequest {
        use crate::domain::align::{AlignedClip, ClipRole, ScanAlignment};

        let alignment = ScanAlignment {
            clips: vec![AlignedClip {
                role: ClipRole::Start,
                window_start_secs: 0.0,
                window_end_secs: 60.0,
                aligned: true,
                offset_secs: Some(0.0),
                confidence: 0.9,
                video_a_decode_skips: 0,
                video_b_decode_skips: 0,
                video_b_window_start_secs: None,
                video_b_window_end_secs: None,
            }],
            start_aligned: true,
            end_aligned: None,
            recommended_offset_secs: Some(0.0),
            offsets_consistent: true,
            offset_drift_secs: None,
            start_overlap: None,
            query_reference_mode: false,
        };
        let report = GapReport {
            video_a: std::path::PathBuf::from("a.wav"),
            video_b: std::path::PathBuf::from("b.wav"),
            track_compatibility: None,
            alignment,
            gaps: Vec::new(),
            gap_equivalence: Vec::new(),
            gap_offset_agreement: None,
            decode_chunk_secs: 60,
            scan_block_ms: 250,
            silence_peak_fraction: 0.01,
            limit_fill_to_mapped_region: true,
            audio_timeline_skew: None,
        };
        let repair = crate::infrastructure::config::RepairConfig {
            residual_gate,
            absolute_silence_rms: 0.001,
            ..Default::default()
        };
        let mut request = repair.patch_settings().into_request(report);
        request.measure_residual = measure_residual;
        request
    }

    /// Guard for the P0 `Deref` layering
    /// (`docs/dev/archive/TEMP-repair-config-bundles-plan.md` §3, invariants 1–2).
    ///
    /// [`PatchAudioRequest`] exposes patch policy by `Deref` to its embedded
    /// [`PatchRequestSettings`], so `request.fill_mode` and `request.settings.fill_mode` must
    /// always name the same value. The hazard this catches: if a field is ever added directly to
    /// `PatchAudioRequest` whose name collides with a settings field, the inherent field silently
    /// wins at every one of the ~160 `request.<knob>` read sites — no compile error, and no other
    /// test in the suite would notice. Asserts one field per knob family (plan §5), then that a
    /// write through `settings` is observable through the `Deref`, which is what distinguishes a
    /// borrow of the embedded struct from a shadowing copy taken at construction.
    #[test]
    fn deref_reads_reach_embedded_settings_and_are_not_shadowed() {
        let mut request = dual_fit_test_request(false, crate::domain::ResidualGateMode::Off);

        assert_eq!(request.fill_mode, request.settings.fill_mode);
        assert_eq!(
            request.min_fill_correlation,
            request.settings.min_fill_correlation
        );
        assert_eq!(
            request.fill_border_search_secs,
            request.settings.fill_border_search_secs
        );
        assert_eq!(
            request.fill_fit_structure_weight,
            request.settings.fill_fit_structure_weight
        );
        assert_eq!(
            request.gap_end_extend_max_ms,
            request.settings.gap_end_extend_max_ms
        );
        assert_eq!(
            request.fill_anchor_min_correlation,
            request.settings.fill_anchor_min_correlation
        );
        assert_eq!(request.anchor_seam_mode, request.settings.anchor_seam_mode);
        assert_eq!(request.gap_signature_mode, request.settings.gap_signature_mode);
        assert_eq!(request.residual_gate, request.settings.residual_gate);
        assert_eq!(request.normalize_fill, request.settings.normalize_fill);
        assert_eq!(request.dual_fit, request.settings.dual_fit);
        assert_eq!(
            request.skip_equivalent_gaps,
            request.settings.skip_equivalent_gaps
        );

        let flipped = !request.settings.dual_fit;
        request.settings.dual_fit = flipped;
        assert_eq!(
            request.dual_fit, flipped,
            "Deref must observe writes to settings, not a copy"
        );

        let bumped = request.settings.fill_border_search_secs + 1.0;
        request.settings.fill_border_search_secs = bumped;
        assert_eq!(request.fill_border_search_secs, bumped);
    }

    /// `into_request` must leave the report-only residual measurement off: it is a per-run opt-in
    /// with no config key, and the only field callers assign on a request after construction.
    #[test]
    fn into_request_defaults_measure_residual_off() {
        let request = dual_fit_test_request(false, crate::domain::ResidualGateMode::Off);
        assert!(
            !request.measure_residual,
            "measure_residual must default off; callers opt in explicitly"
        );
    }

    /// Regression for the residual-gate bypass fix: dual-fit's success path used to hardcode
    /// `residual: None` on every rescued patch, silently bypassing `--residual-gate` regardless of
    /// whether it was active for the rest of the run. `measure_dual_fit_residual_verdict` gives
    /// dual-fit its own residual measurement (two independent `chosen_delta`s, one per shoulder,
    /// since dual-fit — unlike the ordinary rigid splice — matches its two shoulders at
    /// independent lags). This builds a synthetic same-master gap (mirrors
    /// `dual_fit::tests::recovers_a_stepped_silence_splice`) where A's raw pre/post border windows
    /// are literal copies of the B content at the placement `try_dual_fit` actually matched, so the
    /// chosen probe should cancel almost perfectly against the floor and the gate must not reject.
    #[test]
    fn measure_dual_fit_residual_verdict_attaches_a_real_verdict() {
        let sr = 48_000u32;
        let ch = 1usize;
        let w = 1200usize;
        let gap = 4000usize;
        let step = 200i64;

        let mut seed = 0xDEAD_BEEF_1234_5678u64;
        let mut rng = move || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((seed >> 33) as f64 / (1u64 << 30) as f64) - 1.0
        };
        let bn = 40_000usize;
        let b_mono: Vec<f64> = (0..bn).map(|_| rng()).collect();
        let b_samples: Vec<f32> = b_mono.iter().map(|&x| x as f32).collect();

        let b_mapped_start = 10_000usize;
        let a_pre_mono: Vec<f64> = b_mono[b_mapped_start - w..b_mapped_start].to_vec();
        let post_src = b_mapped_start + gap + step as usize;
        let a_post_mono: Vec<f64> = b_mono[post_src..post_src + w].to_vec();

        let params = crate::domain::dual_fit::DualFitParams {
            channels: ch,
            sample_rate: sr,
            gap_frames: gap,
            seam_window_frames: w,
            max_lag_frames: (0.6 * sr as f64) as usize,
            min_fill_correlation: 0.35,
            fill_absolute_floor: 0.12,
            step_real_margin: 0.15,
            a_gap_floor_db: -60.0,
        };
        let r = crate::domain::dual_fit::try_dual_fit(
            &a_pre_mono,
            &a_post_mono,
            &b_mono,
            &b_samples,
            b_mapped_start,
            &params,
        )
        .expect("dual-fit target");
        assert_eq!(r.pre_lag, 0, "pre shoulder matches at nominal lag in this fixture");
        assert_eq!(r.post_lag, step, "post shoulder's seam-local match sits at +step in B");

        // A's raw audio around the (arbitrary, far-away) gap position: the pre/post border windows
        // are literal copies of the B content at the matched placement, so chosen == floor.
        let a_start_frame = 500_000usize;
        let a_end_frame = a_start_frame + gap;
        let mut a_samples = vec![0.0f32; a_end_frame + w];
        for (i, &v) in a_pre_mono.iter().enumerate() {
            a_samples[a_start_frame - w + i] = v as f32;
        }
        for (i, &v) in a_post_mono.iter().enumerate() {
            a_samples[a_end_frame + i] = v as f32;
        }

        let df = DualFitRepairInput {
            params,
            a_pre_mono,
            a_post_mono,
            b_mono,
            b_samples: &b_samples,
            b_mapped_start,
            a_start_frame,
            a_end_frame,
            crossfade_secs: 0.02,
            a_samples: &a_samples,
            border_standoff_frames: 0,
        };

        let request = dual_fit_test_request(true, crate::domain::ResidualGateMode::VetoRescue);
        let verdict = measure_dual_fit_residual_verdict(&request, &df, &r)
            .expect("residual measurement must run when measure_residual is set");
        assert!(
            verdict.informative,
            "both sides found energetic reference windows; verdict must be informative: {verdict:?}"
        );
        let headroom = verdict.worst_headroom_db();
        assert!(
            headroom < 1.0,
            "chosen placement is a literal copy of the matched B content, so headroom vs. the \
             nominal floor should be ~0 dB, not a rejection-worthy value: {headroom}"
        );
        let gated = crate::domain::gap_fill_fit::apply_residual_to_confidence(
            Ok(FillConfidence::High),
            &verdict,
            request.residual_headroom_margin_db,
            request.residual_gate.rescue_enabled(),
        );
        assert_eq!(
            gated,
            Ok(FillConfidence::High),
            "a well-cancelling dual-fit candidate must not be rejected by the residual gate"
        );

        // Off by default: no residual measurement, matching the ordinary path's `want_residual_measurement`.
        let off_request = dual_fit_test_request(false, crate::domain::ResidualGateMode::Off);
        assert!(measure_dual_fit_residual_verdict(&off_request, &df, &r).is_none());
    }
}
