//! Decide what to do with one region: pick the anchor-search prior, run the bracket search and
//! seam gate, fall through to dual-fit rescue when the gate fails, and emit a region spec.
//!
//! Top of the folder's dependency DAG — it may import every sibling; nothing imports it.

use clip_sync::ProgressReporter;

use crate::application::patch_region::{
    derive_seam_gate_geometry, evaluate_seam_gate, retry_waveform_seam_extensions,
    SeamExtensionRetry, SeamGateDerived, SeamGateFailure, SeamGateOutcome, SeamGateParams,
};
use crate::domain::{
    fill_mode::FillMode,
    fill_offset::{resolve_gap_offset_secs, AnchoredRetryPass, FillOffsetMode},
    gap_extension_slack_secs,
    gap_fill::FillRegion,
    gap_fill_fit::{classify_fill_waveform_confidence, FillConfidence},
    gap_repair_spec::{
        BExtractWindow, GapRepairSpec, GapRepairStrategy, GapRepairTags, GapRepairVerdict,
    },
    gap_signature::build_gap_signature,
    gap_structure::StructureMatchParams,
    gap_tags::{FillTierThresholds, GapTagsPatchContext},
    patch_anchor::{
        format_anchored_offset_verbose_line, interpolate_anchored_offset_secs, AnchorSearchPrior,
        PatchAnchorTable,
    },
    patch_result::GapPatchSkipReason,
    policies::{self, GapBorderSpec, RefinedGapFrames},
};

use super::super::geometry::repair_patch_config_view;
use super::super::log::{
    log_gap_fill_plan_verbose, log_gap_fill_result_verbose, log_marginal_gap_fill,
    log_skip_gap_fill, GapFillPlanLog, GapFillResultLog, MarginalGapFillLog,
};
use super::super::{FillWindowFrames, PatchAudioRequest};

use super::bracket::{
    assemble_bracket_fill, execute_bracket_fill, BracketFill, BracketFillAssembly,
    ExecuteBracketFillCtx,
};
use super::dual_fit::{
    build_dual_fit_input, finalize_dual_fit, registration_shoulder_residual_ms,
    select_registration_residual_ms, skip_or_dual_fit, DualFitInputSpec,
};
use super::execute::execute_region_spec;
use super::pcm::{b_extract_frames, b_mapped_start_frame, compute_a_border_rms, slice_b_extract};
use super::types::{
    skip_outcome_from_fields, skip_region_spec, RegionPatch, RegionPatchContext, RegionPatchMedia,
    RegionPatchOpts, RegionPatchOutcome,
};

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
    if anchored_retry_pass != AnchoredRetryPass::Second {
        return None;
    }
    let gap_time_on_a = (region.a_start_secs + region.a_end_secs) / 2.0;
    let predicted_offset =
        interpolate_anchored_offset_secs(&request.report.alignment, gap_time_on_a, table)?;
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

/// **Characterize** one region (6b.3e): run geometry → gate → dual-fit/reconciliation and decide a
/// [`GapRepairSpec`] — `Patch` to execute, or `Skip` whose outcome is derived from the spec (§2.5.5). Does NOT
/// assemble the final patch; that is `execute_region_spec`, invoked by the `prepare_region_patch` shim. This
/// is the characterize→execute boundary (§2.5): all repair *decisions* live here; PCM *assembly* lives in the
/// executor.
///
/// It does assemble a bracket fill internally — the report-vs-splice reconciliation and the normalize gain are
/// decisions that can only be made against the actual bytes — but that `Vec` is scored and dropped (F2). No
/// PCM crosses the boundary on the Bracket path.
pub(in crate::application::patch_audio) fn characterize_region(
    progress: &dyn ProgressReporter,
    media: &RegionPatchMedia<'_>,
    region: &FillRegion,
    request: &PatchAudioRequest,
    ctx: &RegionPatchContext,
    opts: RegionPatchOpts<'_>,
) -> (GapRepairSpec, GapTagsPatchContext) {
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
    let border_standoff_secs = request.border_standoff_secs;
    let fill_extract_tail_slack_secs = request.fill_extract_tail_slack_secs;
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

    let context_frames = (gap_signature_context_secs * sample_rate as f64).round() as usize;
    let bin_frames = ((gap_signature_bin_ms as f64 / 1000.0) * sample_rate as f64).round() as usize;
    let search_radius_secs = border_search_secs.max(margin_secs);
    let extend_slack_secs = gap_extension_slack_secs(repair_patch_config_view(request));
    let b_extract_start_secs = (refined_b_start_secs
        - gap_signature_context_secs
        - search_radius_secs
        - margin_secs
        - extend_slack_secs)
        .max(0.0);
    let length_slack_secs = fill_extract_tail_slack_secs.max(margin_secs);
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
            skip_region_spec(
                GapPatchSkipReason::ZeroLengthGap,
                None,
                region,
                gap_offset_secs,
                refined,
                BExtractWindow {
                    start_frame: 0,
                    end_frame: 0,
                    b_mapped_start_frame: 0,
                },
            ),
            tag_ctx,
        );
    }

    // S0: the gap-derived window sizes, computed ONCE here from the pre-gate gap length and held fixed for
    // the rest of this gap — the gate may extend the gap boundaries afterwards, but these windows do not
    // follow it. `derive_seam_gate_geometry` and the executor derive the identical trio from the identical
    // input via this one helper.
    let window_gap_frames = gap_frames;
    let windows = FillWindowFrames::for_gap(&request.settings, window_gap_frames, sample_rate);
    let seam_gate_frames = windows.seam_gate_frames;
    // S1: compute the window frames ONCE — these exact values go on the spec, and the executor re-slices
    // from them. Deriving them here (instead of recomputing the same expressions at the spec site) is what
    // makes "the executor gets the buffer characterize used" true by construction rather than by coincidence.
    let (b_extract_start_frame, b_extract_end_frame) =
        b_extract_frames(sample_rate, b_extract_start_secs, b_extract_end_secs);
    let b_extract_result = {
        let _s = tracing::info_span!("char_b_extract").entered();
        slice_b_extract(
            b_samples_full,
            channels,
            b_extract_start_frame,
            b_extract_end_frame,
        )
    };
    let b_samples = match b_extract_result {
        Some(samples) => samples,
        None => {
            let reason = GapPatchSkipReason::BExtractFailed;
            log_skip_gap_fill(progress, region, &reason);
            return (
                skip_region_spec(
                    reason,
                    None,
                    region,
                    gap_offset_secs,
                    refined,
                    BExtractWindow {
                        start_frame: 0,
                        end_frame: 0,
                        b_mapped_start_frame: 0,
                    },
                ),
                tag_ctx,
            );
        }
    };

    let border_frames = windows.border_frames;
    let border_standoff_frames = (border_standoff_secs * sample_rate as f64).round() as usize;
    let search_radius_frames = (border_search_secs * sample_rate as f64).round() as usize;
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
        start_frame: b_extract_start_frame,
        end_frame: b_extract_end_frame,
        b_mapped_start_frame: dual_fit_input
            .as_ref()
            .map(|d| d.b_mapped_start)
            .unwrap_or(0),
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
                                finalize_dual_fit(
                                    decision,
                                    region,
                                    gap_offset_secs,
                                    silence_splice_b_extract,
                                    refined,
                                ),
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
                        finalize_dual_fit(
                            decision,
                            region,
                            gap_offset_secs,
                            silence_splice_b_extract,
                            refined,
                        ),
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
                finalize_dual_fit(
                    decision,
                    region,
                    gap_offset_secs,
                    silence_splice_b_extract,
                    refined,
                ),
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
        fit_terms,
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
                gap_index: region.gap_index,
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
        b_mapped_start_frame(refined_b_start_secs, b_extract_start_secs, sample_rate);

    // The same slide `execute_bracket_output` recomputes from the placement fields; kept here for the
    // verbose log, which runs before the spec is built.
    let slide_secs =
        (alignment.start_frame as f64 - offset_nominal_start as f64) / sample_rate as f64;

    // The executor derives the fill's target length from `spec.refined` rather than storing it. That is only
    // sound while the gate's reported `gap_frames` *is* its reported gap — pin it here so a future gate that
    // decouples them fails loudly in debug instead of silently assembling a differently-sized fill.
    debug_assert_eq!(
        gap_frames,
        refined.end_frame.saturating_sub(refined.start_frame),
        "gate gap_frames must match its own refined gap; the executor re-derives the fill length from it"
    );

    // The bracket spec's B window — built here (not at the spec site) so the `debug_assert_eq!` shadow below
    // can prove the executor's reconstruction against the very struct the spec will carry (S1).
    let bracket_b_extract = BExtractWindow {
        start_frame: b_extract_start_frame,
        end_frame: b_extract_end_frame,
        b_mapped_start_frame: offset_nominal_start,
    };

    let fill_start_sample = alignment.start_frame * channels;
    let b_fill_end_sample = fill_start_sample + alignment.fill_frames * channels;
    if b_fill_end_sample > b_samples.len() {
        let reason = GapPatchSkipReason::AlignedSegmentOutOfRange;
        log_skip_gap_fill(progress, region, &reason);
        return (
            skip_region_spec(
                reason,
                None,
                region,
                gap_offset_secs,
                refined,
                silence_splice_b_extract,
            ),
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
    let (a_pre_ch, a_post_ch) =
        policies::border_templates_per_channel_for_gap(&a_pcm.samples, channels, &border_spec);

    // S1 (docs/dev/TEMP-registration-residual-plan.md): independent shoulder-seam read of the placement
    // vs `slide_secs` (the applied placement), record-only — no gate reads this. Mirrors `try_dual_fit`'s
    // own anchor arithmetic exactly: same `seam_local_peak` primitive, same `offset_nominal_start`
    // fiducial, templates used as-returned (already trimmed for low-energy edges). Reuses the already-
    // decoded/bounded borders and B extract — no new decode, no new sweep window.
    let registration_residual_ms = {
        let b_mono_for_residual = crate::domain::pcm::interleaved_to_mono(b_samples, channels);
        let max_lag_for_residual = (border_search_secs * sample_rate as f64).round() as usize;
        let confident_floor = f64::from(min_fill_correlation.max(request.fill_absolute_floor));
        let pre_peak = offset_nominal_start
            .checked_sub(a_pre_border.len())
            .and_then(|anchor| {
                crate::domain::seam_local::seam_local_peak(
                    &a_pre_border,
                    &b_mono_for_residual,
                    anchor,
                    max_lag_for_residual,
                )
            });
        let post_peak = crate::domain::seam_local::seam_local_peak(
            &a_post_border,
            &b_mono_for_residual,
            offset_nominal_start + gap_frames,
            max_lag_for_residual,
        );
        let applied_ms = slide_secs * 1000.0;
        let shoulder_ms = |peak: Option<(f64, i64, Option<f64>)>| {
            peak.and_then(|(r, lag, _)| {
                registration_shoulder_residual_ms(r, lag, applied_ms, sample_rate, confident_floor)
            })
        };
        select_registration_residual_ms(shoulder_ms(pre_peak), shoulder_ms(post_peak))
    };
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
    // M0 perf instrumentation (archive/TEMP-patch-audio-bracket-fill-elimination-plan.md §3): time the fill
    // assembly itself. `char_gate_search` closes well before this point, so the assembly was previously
    // unmeasured — this span decides whether re-deriving the fill in execute needs a dedup hoist first.
    // `fill_mode` is request-level, so the Fit/Gate split comes from running one measurement per mode.
    let BracketFill {
        pcm: b_fill,
        extended_frames,
    } = {
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

    // 6b.3a shadow: prove the executor can re-derive this exact fill from the spec + the FULL decode buffers
    // alone (re-slicing the B extract, rebuilding borders independently). Guards the characterize→execute
    // split before 6b.3b removes the inline path. Debug-only; the release path is unchanged.
    //
    // S1: the B slice is re-derived from `bracket_b_extract` — the exact struct the spec carries — rather
    // than reusing characterize's `b_samples`. That closes the last step the shadow did not cover: without
    // it, the re-slice (and its clamp) would first run in production, on end-of-B gaps.
    debug_assert_eq!(
        execute_bracket_fill(ExecuteBracketFillCtx {
            alignment,
            b_samples: slice_b_extract(
                b_samples_full,
                channels,
                bracket_b_extract.start_frame,
                bracket_b_extract.end_frame,
            )
            .expect("bracket spec's B extract window re-slices (characterize just sliced it)"),
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

    // Whole-program master match, resolved once per run (`domain::program_gain`). Unity on a
    // refusal, so a run without a trusted offset is the pre-2026-08-26 path exactly. The two halves
    // compose in `stack_fill_gain` (plan § 5) — local is a residual on the already-matched fill.
    let program_gain = request.program_gain.gain();
    // The A-side border pool, and only ever A's: the local half reads A's shoulders, never B's
    // (plan § 2 — reading B's shoulders at the hole is the refuted idea, `settled.md` § A).
    let a_border_rms = if normalize_fill {
        compute_a_border_rms(
            a_pcm,
            refined.start_frame,
            refined.end_frame,
            normalize_window_secs,
            global_a_rms,
        )
    } else {
        0.0
    };
    // Both inputs are normalize-only, so both are gated the same way: `stack_fill_gain` discards
    // this whenever the flag is off, and off is the default — no reason to walk the fill for it.
    let b_fill_rms = if normalize_fill {
        policies::rms_interleaved(&b_fill)
    } else {
        0.0
    };
    let stacked = crate::domain::stack_fill_gain(
        program_gain,
        a_border_rms,
        b_fill_rms,
        max_fill_gain_db,
        normalize_fill,
    );
    // Boost-only: no production path may quiet a fill below the level it was assembled at. The
    // clamp lives in `compute_fill_gain`; this pins the property at the one site that consumes it,
    // so a future edit to the policy cannot silently reintroduce attenuation here. It is the
    // *local* half that is boost-only — `program_gain` attenuates a louder donor by design (plan
    // § 3), which is why `stacked.total` is not asserted.
    debug_assert!(
        stacked.local >= 1.0,
        "local normalize gain must never attenuate (boost-only policy), got {}",
        stacked.local
    );
    let gain = stacked.total;

    log_gap_fill_result_verbose(
        progress,
        &GapFillResultLog {
            b_search_start_secs: b_extract_start_secs,
            sample_rate,
            channels,
            fill_start_sample,
            fill_end_sample: b_fill_end_sample,
            slide_secs,
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
        b_extract: bracket_b_extract,
        crossfade_secs: region.crossfade_secs,
        verdict: GapRepairVerdict::Patch(GapRepairStrategy::Bracket {
            alignment,
            window_gap_frames,
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
            fit_terms,
            registration_residual_ms,
        }),
        tags_ctx: GapRepairTags::default(),
    };
    (
        // F2: the fill is NOT handed over — `b_fill` was built for the reconciliation and the gain above, and
        // dies here. The executor rebuilds its own from the spec.
        spec, tag_ctx,
    )
}

/// Characterize every planned region into a spec — the §2.5.5 multi-region loop over [`characterize_region`].
/// **Region-infallible:** exactly one spec per region, every failure folded into a `Skip` verdict (§2.5.7 #3),
/// so `specs.len() == regions.len()`. Drops the reporting extra (`GapTagsPatchContext`) — this yields the
/// repair *decisions* without executing PCM. First-pass anchors only (pass-2 anchored retry is out of scope
/// for this helper; production preview is `PatchAudio::preview`).
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
            let (spec, _tag_ctx) = characterize_region(
                progress,
                media,
                region,
                request,
                ctx,
                RegionPatchOpts {
                    anchored_retry_pass: AnchoredRetryPass::First,
                    patch_anchors: None,
                },
            );
            spec
        })
        .collect()
}

/// Thin shim (6b.3e): characterize the region, then execute if it's a patch. The former target of this name;
/// the split lives in [`characterize_region`] + [`execute_region_spec`]. Media/request/context are handed to
/// both passes: characterize to decide, execute to re-derive the fill PCM.
pub(in crate::application::patch_audio) fn prepare_region_patch(
    progress: &dyn ProgressReporter,
    media: &RegionPatchMedia<'_>,
    region: &FillRegion,
    request: &PatchAudioRequest,
    ctx: &RegionPatchContext,
    opts: RegionPatchOpts<'_>,
) -> (Option<RegionPatch>, RegionPatchOutcome, GapTagsPatchContext) {
    let (spec, tag_ctx) = characterize_region(progress, media, region, request, ctx, opts);
    let (patch, outcome) = if spec.verdict.is_patch() {
        execute_region_spec(spec, request, media, ctx)
    } else {
        // Field-level skip outcome — no verdict-matching helper (§3.2).
        let residual = spec.tags_ctx.gate.residual;
        match &spec.verdict {
            GapRepairVerdict::Skip { reason, .. } => {
                (None, skip_outcome_from_fields(reason, residual))
            }
            // `is_patch()` is the dispatch predicate; this arm is exhaustiveness-only. Re-enter the
            // executor so its existing Skip `unreachable!` remains the sole verdict assert.
            GapRepairVerdict::Patch(_) => execute_region_spec(spec, request, media, ctx),
        }
    };
    (patch, outcome, tag_ctx)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **8g.2 — `characterize_all_regions` yields one consistent spec per region.** Region-infallible
    /// (`specs.len() == regions.len()`), and each spec's verdict agrees with what the `prepare_region_patch`
    /// shim executes for that region: a `Patch` verdict ⟺ the executor emits a `RegionPatch`; a `Skip` verdict
    /// ⟺ a `Skipped` outcome. (Both go through the same `characterize_region`, so this pins the multi-region
    /// loop's spec extraction against the live shim.)
    #[test]
    fn characterize_all_regions_yields_one_consistent_spec_per_region() {
        use super::super::types::outcome_from_spec;
        use super::{
            characterize_all_regions, prepare_region_patch, AnchoredRetryPass, RegionPatchContext,
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
                *slot = (std::f64::consts::TAU * freq * (f as f64) / f64::from(rate)).sin() as f32
                    * 0.2;
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
                collar_level: None,
            }],
            gap_equivalence: Vec::new(),
            gap_offset_agreement: None,
            decode_chunk_secs: 30,
            recipe: crate::domain::ScanRecipe::with_hold_blocks(1000, 0, 20, 0.05, 0.0),
            limit_fill_to_mapped_region: false,
            b_scanned_end_secs: None,
            b_scan_truncated: false,
            audio_timeline_skew: None,
            program_levels: None,
        };
        let repair = RepairConfig {
            gap_signature_mode: GapSignatureMode::Energy,
            gap_signature_context_secs: 0.5,
            fill_border_search_secs: 0.05,
            fill_align_margin_secs: 0.02,
            fill_length_slack_secs: 0.1,
            fill_extract_tail_slack_secs: 0.1,
            fill_seam_search_secs: 0.05,
            border_standoff_secs: 0.0,
            max_anchor_bracket_secs: 0.2,
            max_anchors_per_side: 2,
            ..RepairConfig::default()
        };
        let request: PatchAudioRequest = repair
            .patch_settings()
            .into_request(report.clone())
            .expect("default All gap selection");

        let regions = vec![FillRegion {
            gap_index: 0,
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
        let media = RegionPatchMedia {
            b_samples_full: &b,
            a_pcm: &a_pcm,
        };
        let progress = NoOpProgressReporter;

        let specs = characterize_all_regions(&progress, &media, &regions, &request, &ctx);
        assert_eq!(
            specs.len(),
            regions.len(),
            "one spec per region (region-infallible)"
        );

        for (spec, region) in specs.iter().zip(regions.iter()) {
            let (patch, outcome, _tags) = prepare_region_patch(
                &progress,
                &media,
                region,
                &request,
                &ctx,
                RegionPatchOpts {
                    anchored_retry_pass: AnchoredRetryPass::First,
                    patch_anchors: None,
                },
            );
            let is_patch = spec.verdict.is_patch();
            assert_eq!(
                is_patch,
                patch.is_some(),
                "Patch verdict ⟺ executor emits a RegionPatch"
            );
            assert_eq!(
                !is_patch,
                matches!(outcome, RegionPatchOutcome::Skipped { .. }),
                "Skip verdict ⟺ Skipped outcome",
            );
            // Preview path: decision-only outcomes match the executor's dispositions (pass-1).
            let preview_outcome = outcome_from_spec(spec, rate);
            match (&preview_outcome, &outcome) {
                (RegionPatchOutcome::Patched { .. }, RegionPatchOutcome::Patched { .. })
                | (RegionPatchOutcome::Skipped { .. }, RegionPatchOutcome::Skipped { .. }) => {}
                _ => panic!("preview disposition diverged from execute for region"),
            }
        }
    }

    /// Exit #1 (`docs/dev/TEMP-registration-residual-plan.md`): bracket-path
    /// `registration_residual_ms` on a gap where B's content truly is continuous and matches A's
    /// shoulders at zero lag — the independent shoulder-local read must agree with the applied
    /// placement, landing a residual near 0. Reuses the exact fixture from
    /// `characterize_all_regions_yields_one_consistent_spec_per_region` (A silent inside the gap, B
    /// continuous 330Hz sine everywhere), which is by construction a zero-lag match on both
    /// shoulders.
    #[test]
    fn bracket_registration_residual_is_near_zero_on_a_matched_fixture() {
        use super::{characterize_all_regions, RegionPatchContext, RegionPatchMedia};
        use crate::domain::gap::Gap;
        use crate::domain::gap_fill::FillRegion;
        use crate::domain::gap_repair_spec::GapRepairStrategy;
        use crate::domain::policies;
        use crate::domain::{GapReport, GapSignatureMode, ScanAlignment};
        use crate::infrastructure::config::RepairConfig;
        use clip_sync::MultiChannelPcm;
        use clip_sync_repair_fixtures::NoOpProgressReporter;

        let rate = 48_000u32;
        let secs = |s: f64| (s * f64::from(rate)) as usize;
        let total = secs(3.0);
        let (g0, g1) = (secs(1.0), secs(2.0));

        // Band-limited noise, not a pure tone: a sine's autocorrelation is ambiguous at every
        // period, which can make the bracket search settle on the wrong lag. Noise gives an
        // unambiguous zero-lag match so the independent shoulder read and the applied placement
        // are guaranteed to agree.
        let mut seed = 0x1234_5678_9ABC_DEF0u64;
        let mut rng = move || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (((seed >> 33) as f64 / (1u64 << 30) as f64) - 1.0) as f32 * 0.2
        };
        let noise: Vec<f32> = (0..total).map(|_| rng()).collect();
        let mut a = vec![0f32; total];
        let b = noise.clone();
        a[..g0].copy_from_slice(&noise[..g0]);
        a[g1..total].copy_from_slice(&noise[g1..total]);

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
                collar_level: None,
            }],
            gap_equivalence: Vec::new(),
            gap_offset_agreement: None,
            decode_chunk_secs: 30,
            recipe: crate::domain::ScanRecipe::with_hold_blocks(1000, 0, 20, 0.05, 0.0),
            limit_fill_to_mapped_region: false,
            b_scanned_end_secs: None,
            b_scan_truncated: false,
            audio_timeline_skew: None,
            program_levels: None,
        };
        let repair = RepairConfig {
            gap_signature_mode: GapSignatureMode::Bool,
            gap_signature_context_secs: 0.5,
            fill_border_search_secs: 0.05,
            fill_align_margin_secs: 0.02,
            fill_length_slack_secs: 0.1,
            fill_extract_tail_slack_secs: 0.1,
            fill_seam_search_secs: 0.05,
            border_standoff_secs: 0.0,
            max_anchor_bracket_secs: 0.2,
            max_anchors_per_side: 2,
            min_fill_correlation: 0.0,
            fill_absolute_floor: 0.0,
            normalize_window_secs: 0.05,
            ..RepairConfig::default()
        };
        let request: PatchAudioRequest = repair
            .patch_settings()
            .into_request(report.clone())
            .expect("default All gap selection");

        let regions = vec![FillRegion {
            gap_index: 0,
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
        let media = RegionPatchMedia {
            b_samples_full: &b,
            a_pcm: &a_pcm,
        };
        let progress = NoOpProgressReporter;

        let specs = characterize_all_regions(&progress, &media, &regions, &request, &ctx);
        assert_eq!(specs.len(), 1);
        match &specs[0].verdict {
            crate::domain::gap_repair_spec::GapRepairVerdict::Patch(
                GapRepairStrategy::Bracket {
                    registration_residual_ms,
                    ..
                },
            ) => {
                let residual = registration_residual_ms.expect(
                    "both shoulders are exact zero-lag matches, well above the confidence floor",
                );
                assert!(
                    residual.abs() < 1.0,
                    "shoulders match at zero lag; residual should be ~0ms, got {residual}"
                );
            }
            other => {
                panic!("expected a Patch(Bracket) verdict for a matched fixture, got {other:?}")
            }
        }
    }
}
