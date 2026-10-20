//! Decide what to do with one region: pick the anchor-search prior, run the bracket search and
//! seam gate, fall through to dual-fit rescue when the gate fails, and emit a region spec.
//!
//! Top of the folder's dependency DAG — it may import every sibling; nothing imports it.

use clip_sync::ProgressReporter;

use crate::application::patch_region::{
    derive_seam_gate_geometry, evaluate_seam_gate, retry_waveform_seam_extensions, SeamBWindow,
    SeamExtensionRetry, SeamGateDerived, SeamGateFailure, SeamGateOutcome, SeamGateParams,
};
use crate::domain::{
    b_stall::{gap_offset_with_stall_map, reported_stalls},
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
    gap_tags::{DualFitRoute, FillTierThresholds, GapTagsPatchContext},
    patch_anchor::{
        format_anchored_offset_verbose_line, interpolate_anchored_offset_secs, AnchorSearchPrior,
        PatchAnchorTable,
    },
    patch_result::{GapPatchSkipReason, ShouldersAbutLocks},
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
    build_dual_fit_input, finalize_dual_fit, registration_residual_from_shoulders,
    registration_route_taken, registration_shoulder_residual_ms, skip_or_dual_fit,
    try_dual_fit_rescue, DualFitAttempt, DualFitDecision, DualFitInputSpec, RouteKind,
};
use super::execute::execute_region_spec;
use super::pcm::{b_extract_frames, b_mapped_start_frame, slice_b_extract};
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
        silence_peak_fraction,
    } = ctx;

    // Perf instrumentation (Level B, TEMP-production-repair-perf-plan.md §0): time the whole characterize
    // pass per gap; the `char_*` child spans below split the shared-object rebuilds (b-extract / geometry /
    // dual-fit downmix) from the gate search. Emits only when `CLIP_SYNC_SPAN_TIMING` is set (Level A). This
    // span nests under `patch_audio` (pass 1) or `patch_anchored_retry` (the retry pass), so anchored-retry
    // characterize time stays bucketed separately per §0.
    let _characterize_span = tracing::info_span!("characterize").entered();

    let margin_secs = request.fill_align_margin_secs;
    let border_search_secs = request.fill_border_search_secs;
    let border_standoff_secs = request.border_standoff_secs;
    let fill_extract_tail_slack_secs = request.fill_extract_tail_slack_secs;
    let fill_seam_search_secs = request.fill_seam_search_secs;
    let gap_signature_context_secs = request.gap_signature_context_secs;
    let gap_signature_bin_ms = request.gap_signature_bin_ms;
    let min_structure_match_score = request.min_structure_match_score;
    let min_fill_correlation = request.min_fill_correlation;
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
    let stalls = reported_stalls(request.report.b_stall.as_ref());
    let stall_map_advance_secs = match fill_offset_mode {
        FillOffsetMode::Recommended => {
            crate::domain::b_stall::trusted_stall_advance_secs(stalls, region.a_start_secs)
        }
        FillOffsetMode::Interpolated | FillOffsetMode::AnchoredRetry => 0.0,
    };
    let gap_offset_secs = gap_offset_with_stall_map(
        fill_offset_mode,
        resolve_gap_offset_secs(
            &request.report.alignment,
            fill_offset_mode,
            gap_time_on_a,
            patch_anchors,
            anchored_retry_pass,
        ),
        region.b_start_secs - region.a_start_secs,
        stalls,
        region.a_start_secs,
    );

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

    let tag_ctx = GapTagsPatchContext {
        fill_mode: request.fill_mode,
        thresholds: FillTierThresholds {
            min_fill_correlation: request.min_fill_correlation,
            fill_marginal_margin: request.fill_marginal_margin,
            fill_absolute_floor: request.fill_absolute_floor,
        },
        signature_mode_label,
        // Placement facts, never written here: `region_outcome_gap_tags` reads them off a Patched
        // outcome, and a skip keeps these defaults.
        fit_used_boundary_grid: false,
        anchor_seam_used: false,
        anchor_bracket_move_frames: 0,
        zero_pad_frames: 0,
        anchor_trusted: false,
        dual_fit_used: false,
        dual_fit_step_real: None,
        dual_fit_post_window_ms: None,
        dual_fit_route: None,
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
            stall_map_advance_secs,
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
            SeamBWindow {
                b_extract_start_secs,
                refined_b_start_secs,
                refined_b_end_secs,
                gap_frames,
            },
            anchor_search_prior,
        )
    };
    let seam_params = SeamGateParams {
        settings: &request.settings,
        derived,
        geom,
    };

    // A3: build the dual-fit fallback input once, before the gate can mutate `refined` via
    // boundary-extension retry — dual-fit works on the base gap geometry.
    //
    // Built on EVERY run, not just `--dual-fit` ones: the seam locks below, their `clamped` tag and
    // `program_quiet_at_nominal` all read `templates` / `b_mono` / `a_gap_floor_db`, and the abut
    // skip is not a dual-fit feature. Consumption is still flag-gated (`dual_fit_eligible`), and
    // `BExtractWindow` is not serialized, so populating `b_mapped_start_frame` under `--no-dual-fit`
    // is internal-only. Cost of the change on that path: one B mono conversion plus the two ±600 ms
    // shoulder searches per gap.
    let dual_fit_input = {
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
    };

    // 8c: the B extract window for a dual-fit rescue spec — real geometry (`b_mapped_start` is the extract-
    // relative nominal gap start the dual-fit input already computed). Built here so `finalize_dual_fit` can
    // attach it; the executor's SilenceSplice arm doesn't read it, so this is PCM-neutral (byte-parity).
    let silence_splice_b_extract = BExtractWindow {
        start_frame: b_extract_start_frame,
        end_frame: b_extract_end_frame,
        b_mapped_start_frame: dual_fit_input
            .as_ref()
            .map(|d| d.templates.b_mapped_start())
            .unwrap_or(0),
    };

    // Early shoulder locks: measure both seams at (or, for an untrustworthy one, near) the nominal
    // `b_mapped` BEFORE the gate search, so a gap whose shoulders already abut is skipped without
    // paying for a fill search that cannot help it. Measured from the baseline `refined` for the
    // same reason `dual_fit_input` is — the gate may extend the boundaries afterwards.
    let seam_locks = dual_fit_input.as_ref().and_then(|df| {
        let _s = tracing::info_span!("char_seam_locks").entered();
        // A is fully decoded here, so the envelope's wider border is a plain slice; the closure form
        // exists for the fingerprint's B-side capture, which cannot slice.
        let env_cfg = crate::domain::lag_envelope::EnvelopeCfg::from_knobs(
            request.lag_envelope_max_lag_ms,
            request.lag_envelope_window_secs,
            sample_rate,
        );
        let wide_a = crate::domain::seam_locks::wide_a_slicer(
            &a_pcm.samples,
            channels,
            refined.start_frame,
            refined.end_frame,
            env_cfg.map(|c| c.window).unwrap_or(0),
        );
        crate::domain::seam_locks::nominal_first_seam_locks(
            &df.templates,
            &df.b_mono,
            df.params.max_lag_frames,
            crate::domain::seam_locks::EnvelopeSearch {
                cfg: env_cfg,
                wide_a: Some(&wide_a),
            },
            sample_rate,
            crate::domain::seam_locks::SeamGateFloors {
                min_fill_correlation: df.params.min_fill_correlation,
                fill_absolute_floor: df.params.fill_absolute_floor,
            },
            // Triage at the gate floor: a shoulder the gate would accept at its nominal lag is
            // then never a rescue candidate, so the envelope can only ever add a fill that is
            // declined today — it can never move an existing one. The fingerprint passes this same
            // value from its recipe, so the dump predicts the repair on this axis.
            df.params.min_fill_correlation,
        )
    });

    if let (Some(df), Some(locks)) = (dual_fit_input.as_ref(), seam_locks.as_ref()) {
        if locks.abut() {
            // Both shoulders lock confidently (`gate_pass`) and land on top of each other or crossed:
            // there is no donor bridge between them, so A's gap is introduced silence, not missing
            // content. Skip before `evaluate_seam_gate` — the fill search has nothing to find.
            let reason = GapPatchSkipReason::ShouldersAbut(ShouldersAbutLocks {
                pre_lag_ms: locks.pre.lag as f64 * 1000.0 / f64::from(sample_rate),
                post_lag_ms: locks.post.lag as f64 * 1000.0 / f64::from(sample_rate),
                bridge_frames: locks.bridge_frames,
                gap_frames,
                pre_z: locks.pre.z,
                post_z: locks.post.z,
                pre_envelope_rescued: locks.pre.rescued,
                post_envelope_rescued: locks.post.rescued,
                clamped: locks.clamped(),
                edge_pinned: locks.pre.edge_pinned || locks.post.edge_pinned,
                program_quiet: crate::domain::donor::program_quiet_at_nominal(
                    &df.b_mono,
                    df.templates.b_mapped_start(),
                    gap_frames,
                    df.params.a_gap_floor_db,
                    sample_rate,
                ),
            });
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
    }

    // Both halves or neither — `skip_or_dual_fit` has no use for one without the other.
    let dual_fit_attempt = || {
        dual_fit_input
            .as_ref()
            .zip(seam_locks.as_ref())
            .map(|(input, locks)| DualFitAttempt { input, locks })
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
                                dual_fit_attempt(),
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
                        dual_fit_attempt(),
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
                dual_fit_attempt(),
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
    // The gate's placement facts (boundary grid, anchor seam and trust, zero pad) ride the spec to
    // the outcome, and `region_outcome_gap_tags` reads them from there — `tag_ctx` is never written
    // with them, so a routed dual-fit patch or a post-gate skip cannot inherit the single-lag
    // fill's tags.

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

    let pre_gate_frames = seam_gate_frames.min(a_pre_border.len().max(1));
    let post_gate_frames = if a_post_border.is_empty() {
        0
    } else {
        seam_gate_frames.min(a_post_border.len()).max(1)
    };
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
        head_trim_frames,
        zero_pad_frames,
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
    // B ran out before the fill reached the gap. Those tail frames are not written — the executor
    // shortens the patch destination instead of padding them — so this many frames of the gap are
    // left unrepaired, carrying A's own material. Warned (not debugged) because it is incomplete
    // coverage of a gap reported as `Patched`, and reported here for the same L0 reason as the
    // sibling above: the primitive runs twice and only this call site speaks.
    if zero_pad_frames > 0 {
        tracing::warn!(
            a_start_secs,
            zero_pad_frames,
            "donor B exhausted before the A gap was filled; gap tail left unrepaired (not padded)"
        );
    }

    // S1 (docs/dev/TEMP-registration-residual-plan.md): independent shoulder-seam read of the placement
    // vs the fill actually written, record-only — no gate reads this. Same `seam_local_peak` primitive and
    // `offset_nominal_start` fiducial as `try_dual_fit`, reusing the already-decoded borders and B extract.
    //
    // Two corrections (2026-09-18, 47/g14) keep both sides of the subtraction in the same coordinates:
    // - **Anchors in map coordinates.** Each template's gap-facing edge sits `D` frames outward of the gap
    //   edge (silence walk-off + `border_standoff_frames` + low-energy trim), so the B anchors shift by the
    //   same `D` — exactly as `lag_decision` does. Unshifted, a perfectly registered shoulder reads `∓D`
    //   (≈350 ms at the default standoff), which is what the field mostly reported before.
    // - **Applied = the written fill's start.** Fit's trim-head anchor starts the fill `head_trim_frames`
    //   past `alignment.start_frame`; `slide_secs` alone misses that.
    //
    // `residual_border_standoff_secs` (diagnostic, 2026-09-19) rebuilds the measuring templates at their
    // own standoff so a `border_standoff_secs` sweep moves placement without moving the instrument —
    // otherwise every arm reads its residual with templates held off by the very value being swept.
    let registration_residual = {
        let residual_override = request.residual_border_standoff_secs;
        let residual_spec = GapBorderSpec {
            border_standoff_frames: residual_override.map_or(border_standoff_frames, |secs| {
                (secs * sample_rate as f64).round() as usize
            }),
            ..border_spec
        };
        let residual_templates = residual_override
            .map(|_| policies::border_templates_for_gap(&a_pcm.samples, channels, &residual_spec));
        let (a_pre_border, a_post_border) = match &residual_templates {
            Some((pre, post)) => (pre, post),
            None => (&a_pre_border, &a_post_border),
        };
        let (d_pre, d_post) = policies::border_edge_displacement_frames(
            &a_pcm.samples,
            channels,
            &residual_spec,
            a_pre_border.len(),
            a_post_border.len(),
        );
        // Same slice the dual-fit input already downmixed — reuse it rather than converting B twice.
        let b_mono_for_residual: std::borrow::Cow<'_, [f64]> = match dual_fit_input.as_ref() {
            Some(df) => std::borrow::Cow::Borrowed(&df.b_mono),
            None => std::borrow::Cow::Owned(crate::domain::pcm::interleaved_to_mono(
                b_samples, channels,
            )),
        };
        // Seam-scale, NOT `border_search_secs`: that is the B *haystack* slide radius (10 s by
        // default), and using it here argmaxed a ~0.25 s seam template over ~480k candidate lags
        // against a 0.35 floor with nothing to screen the winner.
        //
        // ONE continuous window spanning BOTH neighbourhoods — nominal ± r and the written fill ± r —
        // argmaxed together. Neither centre alone is right (2026-09-25):
        // - Nominal-centred fails when the map is off by more than a seam — a rigid ~+270 / ~+320 ms
        //   on pairs 13 and 18, −1.15 s on pair 31 — which the bracket slide absorbs: past the radius
        //   the read went sub-floor and vanished; near it, the seam locks' nominal-first gate accepted
        //   a low-prominence bump on the true peak's slope (15/15 such reads moved to the ±10 s lag).
        // - Fill-centred fails when the FILL is the misplaced thing: on 17/5687.5 (anchor seam,
        //   applied −539 ms) both shoulders sit near nominal at prominence 0.52–0.83, outside the
        //   fill's ±r, so the read went absent.
        // The span from `min(0, applied) − r` to `max(0, applied) + r` is centred halfway between the
        // two, so neither neighbourhood is searched twice. It adds only the lags between two places we
        // have reason to look, and `prominence` still flags a curve with rival peaks in both. The
        // placement locks keep `lock_seam`; only this record-only read spans. `seam_local_peak_spanning`
        // reports the lag from nominal, so `applied − shoulder_lag` keeps its meaning.
        let max_lag_for_residual =
            (request.residual_search_secs * sample_rate as f64).round() as usize;
        let confident_floor = f64::from(min_fill_correlation.max(request.fill_absolute_floor));
        let applied_frames =
            (alignment.start_frame + head_trim_frames) as i64 - offset_nominal_start as i64;
        let seam_peak_at = |template: &[f64], nominal_anchor: Option<usize>| {
            crate::domain::seam_local::seam_local_peak_spanning(
                template,
                &b_mono_for_residual,
                nominal_anchor?,
                applied_frames,
                max_lag_for_residual,
            )
        };
        let pre_peak = seam_peak_at(
            a_pre_border,
            offset_nominal_start.checked_sub(d_pre + a_pre_border.len()),
        );
        let post_peak = seam_peak_at(
            a_post_border,
            Some(offset_nominal_start + gap_frames + d_post),
        );
        let applied_ms = applied_frames as f64 * 1000.0 / sample_rate as f64;
        let shoulder_ms = |peak: Option<&crate::domain::seam_local::SeamPeak>| {
            peak.and_then(|p| {
                registration_shoulder_residual_ms(
                    p.r,
                    p.lag,
                    applied_ms,
                    sample_rate,
                    confident_floor,
                )
            })
        };
        let quality = crate::domain::patch_result::ShoulderQuality::from_peaks(
            pre_peak.as_ref(),
            post_peak.as_ref(),
            sample_rate,
        );
        registration_residual_from_shoulders(
            shoulder_ms(pre_peak.as_ref()),
            shoulder_ms(post_peak.as_ref()),
            applied_ms,
        )
        .map(|r| r.with_shoulder_quality(quality))
    };

    // Registration routing (docs/dev/archive/TEMP-dual-fit-registration-routing-plan.md § 5): the gate passed, but
    // the single-lag fill sits further off a licensed shoulder than the bound allows, so try dual-fit
    // first. A decline keeps the single-lag fill — routing never turns a patch into a skip (§ 2 inv. 1)
    // — and the rescue is validated by `try_dual_fit_rescue`'s own gate on its own seam scores (inv. 3).
    // Off (`None` bound) never reaches the measurement below, so the default path is untouched (inv. 2).
    if let (Some(df), Some(locks), Some(r)) = (
        dual_fit_input.as_ref(),
        seam_locks.as_ref(),
        registration_residual.as_ref(),
    ) {
        let crossfade_ms = seam_cf as f64 * 1000.0 / sample_rate as f64;
        if let Some(kind) = registration_route_taken(
            request.dual_fit,
            request.dual_fit_registration_bound_ms,
            request.dual_fit_registration_common_bound_ms,
            crossfade_ms,
            r.registration_residual_pre_ms,
            r.registration_residual_post_ms,
        ) {
            match try_dual_fit_rescue(request, region, df, locks) {
                Ok(rescue) => {
                    tracing::debug!(
                        gap = region.gap_index,
                        ?kind,
                        "registration routing: dual-fit replaced the single-lag fill"
                    );
                    return (
                        finalize_dual_fit(
                            DualFitDecision::Rescued(rescue),
                            region,
                            gap_offset_secs,
                            silence_splice_b_extract,
                            refined,
                        ),
                        GapTagsPatchContext {
                            dual_fit_route: Some(match kind {
                                RouteKind::Step => DualFitRoute::RegistrationStep,
                                RouteKind::Common => DualFitRoute::RegistrationCommon,
                            }),
                            ..tag_ctx
                        },
                    );
                }
                Err(_) => tracing::debug!(
                    gap = region.gap_index,
                    ?kind,
                    "registration routing: dual-fit declined; keeping the single-lag fill"
                ),
            }
        }
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
    // Hole-local half: the paired A-versus-B collar flanking THIS gap, stamped at scan time and
    // decided in `domain::local_gain`. It replaced the A-border-RMS-versus-whole-fill-RMS estimator
    // on 2026-08-31; nothing here walks the fill or the borders any more, which is why the
    // `compute_a_border_rms` / `rms_interleaved(&b_fill)` pair that used to sit at this spot is
    // gone, along with the per-run whole-A RMS that was only ever its silent-border fallback.
    // `normalize_window_secs` is untouched — it still sizes the splice's border geometry in
    // `geometry.rs` and the fingerprint noise collar, which are different jobs.
    let local_gain = request.local_gain(region.gap_index);
    let stacked = crate::domain::stack_fill_gain(program_gain, local_gain);
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
        gap_index: region.gap_index,
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
            local_gain,
            fit_terms,
            registration_residual: registration_residual.map(Box::new),
            zero_pad_frames,
        }),
        tags_ctx: GapRepairTags::default(),
    };
    (
        // F2: the fill is NOT handed over — `b_fill` was built for the reconciliation, and since
        // 2026-08-31 for nothing else (the gain no longer reads its RMS), and
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
            local_pair_collar_secs: None,
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
            b_stall: None,
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
            // Distinct from the hardcoded-0 stub this field used to carry: a leftover `gap_index: 0`
            // on the spec would still match a region at report index 0.
            gap_index: 7,
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
        assert_eq!(
            specs[0].gap_index, regions[0].gap_index,
            "the spec carries the region's report index, not the leftover 6b zero"
        );

        for (spec, region) in specs.iter().zip(regions.iter()) {
            assert_eq!(spec.gap_index, region.gap_index);
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
        let (residual, misplacement_ms) = matched_fixture_bracket_residual_ms(0.0, None);
        let residual = residual
            .expect("both shoulders are exact zero-lag matches, well above the confidence floor");
        assert!(
            misplacement_ms.abs() < 1.0,
            "fixture places the fill at zero lag"
        );
        assert!(
            residual.abs() < 1.0,
            "shoulders match at zero lag; residual should be ~0ms, got {residual}"
        );
    }

    /// Routing plan § 3 (2026-09-22): the bracket record keeps **both** shoulder reads, not only the
    /// selected one, and carries the minuend they share. On the standoff fixture both shoulders read
    /// lag 0 and the fill lands ~15 ms early, so each seam's residual and `registration_applied_ms` all
    /// equal that misplacement — which is the identity `residual = applied − shoulder_lag` per seam.
    #[test]
    fn bracket_registration_keeps_both_seams_and_the_applied_placement() {
        let (registration, misplacement_ms) = matched_fixture_bracket_registration(0.03, None);
        let r = registration.expect("both shoulders confident on the matched fixture");
        let pre = r
            .registration_residual_pre_ms
            .expect("pre shoulder is licensed");
        let post = r
            .registration_residual_post_ms
            .expect("post shoulder is licensed");
        assert!(
            (r.registration_applied_ms - misplacement_ms).abs() < 0.5,
            "applied {} vs written misplacement {misplacement_ms}",
            r.registration_applied_ms
        );
        for (side, v) in [("pre", pre), ("post", post)] {
            assert!(
                (v - misplacement_ms).abs() < 1.0,
                "{side} seam reads lag 0, so its residual is the misplacement: {v} vs {misplacement_ms}"
            );
        }
        let selected = if pre.abs() <= post.abs() { pre } else { post };
        assert_eq!(r.registration_residual_ms, selected);
    }

    /// 47/g14 (2026-09-18): the shoulder templates sit `border_standoff_secs` outward of the gap edges,
    /// so the residual's B anchors must shift by the same displacement. Before the fix the anchors sat
    /// at the raw gap edge and a perfectly registered shoulder read `∓standoff` (here ∓30 ms).
    ///
    /// The residual is pinned against the fill's **true** misplacement, not against zero: with a standoff
    /// this fixture's bracket search lands the fill ~15 ms early (seam r ≈ 0.07/0.09 — the zero floors let
    /// it through). Both shoulders read lag 0, so a correct residual reports exactly that misplacement;
    /// the unshifted anchors read +15 ms (the opposite sign) instead.
    #[test]
    fn bracket_registration_residual_tracks_true_misplacement_with_border_standoff() {
        let (residual, misplacement_ms) = matched_fixture_bracket_residual_ms(0.03, None);
        let residual = residual.expect(
            "both shoulders are exact zero-lag matches once anchored at the template edges",
        );
        assert!(
            (residual - misplacement_ms).abs() < 1.0,
            "residual must equal the fill's true offset from the zero-lag match              ({misplacement_ms} ms); got {residual} ms"
        );
    }

    /// `residual_border_standoff_secs` (2026-09-19): the measuring templates take their own standoff
    /// while placement keeps `border_standoff_secs`. Both directions: placement at 30 ms read by a
    /// flush (0 ms) instrument, and flush placement read by a 30 ms instrument. In each, placement must
    /// be exactly what it is without the override, and the residual must still equal the true
    /// misplacement — the instrument moved, the thing it measures did not.
    #[test]
    fn residual_border_standoff_override_moves_the_instrument_not_the_placement() {
        for (placement, instrument) in [(0.03, 0.0), (0.0, 0.03)] {
            let (_, baseline_misplacement) = matched_fixture_bracket_residual_ms(placement, None);
            let (residual, misplacement_ms) =
                matched_fixture_bracket_residual_ms(placement, Some(instrument));
            assert_eq!(
                misplacement_ms, baseline_misplacement,
                "placement {placement}: the override must not move the written fill"
            );
            let residual = residual.expect("both shoulders are exact zero-lag matches");
            assert!(
                (residual - misplacement_ms).abs() < 1.0,
                "placement {placement}, instrument {instrument}: residual {residual} ms must equal                  the true misplacement {misplacement_ms} ms"
            );
        }

        // The two cases above pass whether or not the override is read (a correct instrument reads
        // the true misplacement at any standoff). This one cannot: a 5 s instrument standoff leaves
        // the 1 s shoulders no template material, so the residual must vanish while placement,
        // which never sees the override, still patches exactly where it did.
        let (baseline_residual, baseline_misplacement) =
            matched_fixture_bracket_residual_ms(0.0, None);
        assert!(baseline_residual.is_some());
        let (residual, misplacement_ms) = matched_fixture_bracket_residual_ms(0.0, Some(5.0));
        assert_eq!(misplacement_ms, baseline_misplacement);
        assert_eq!(
            residual, None,
            "an instrument standoff wider than the shoulders must leave nothing to read"
        );
    }

    /// The bracket residual from the zero-lag matched fixture (A silent inside a 1 s gap, B the same
    /// band-limited noise throughout), with the given `border_standoff_secs`, paired with the written
    /// fill's true offset (ms) from that zero-lag match — known here because B is A's own signal.
    fn matched_fixture_bracket_residual_ms(
        border_standoff_secs: f64,
        residual_border_standoff_secs: Option<f64>,
    ) -> (Option<f64>, f64) {
        let (registration, misplacement_ms) = matched_fixture_bracket_registration(
            border_standoff_secs,
            residual_border_standoff_secs,
        );
        (
            registration.map(|r| r.registration_residual_ms),
            misplacement_ms,
        )
    }

    fn matched_fixture_bracket_registration(
        border_standoff_secs: f64,
        residual_border_standoff_secs: Option<f64>,
    ) -> (
        Option<crate::domain::patch_result::RegistrationResidual>,
        f64,
    ) {
        matched_fixture_bracket_registration_with(
            border_standoff_secs,
            residual_border_standoff_secs,
            0.0,
            None,
        )
    }

    /// 2026-09-25: the residual's search reaches the written fill, not just the nominal map. B runs
    /// 30 ms late of the map; the bracket slide absorbs it, and a ±10 ms residual search must still
    /// find both shoulders — at ~0 residual, unpinned. Centred on nominal alone, the same radius cannot
    /// reach the 30 ms shoulder and (the floors are zero here) reports an edge-pinned ~+20 ms: the
    /// corpus's rigid-map-error failure (pairs 13/18, ~+270/+320 ms at a 250 ms radius) in miniature.
    /// The mirror case (fill far from shoulders that sit at nominal) is
    /// `seam_local_peak_spanning_finds_a_shoulder_at_either_end`.
    #[test]
    fn bracket_registration_residual_is_centred_on_the_fill_not_the_map() {
        let (registration, applied_ms) =
            matched_fixture_bracket_registration_with(0.0, None, 0.03, Some(0.01));
        assert!(
            (applied_ms - 30.0).abs() < 1.0,
            "fixture must slide the fill onto B's 30 ms delay, got {applied_ms} ms"
        );
        let r = registration.expect("both shoulders sit inside the spanning window");
        for (side, v, pinned) in [
            (
                "pre",
                r.registration_residual_pre_ms,
                r.registration_residual_pre_edge_pinned,
            ),
            (
                "post",
                r.registration_residual_post_ms,
                r.registration_residual_post_edge_pinned,
            ),
        ] {
            let v = v.unwrap_or_else(|| panic!("{side} shoulder is licensed"));
            assert!(
                v.abs() < 1.0,
                "{side}: fill sits on the shoulder, residual should be ~0, got {v}"
            );
            assert_eq!(
                pinned,
                Some(false),
                "{side}: peak must not exhaust the radius"
            );
            assert_eq!(
                if side == "pre" {
                    r.registration_residual_pre_rescued
                } else {
                    r.registration_residual_post_rescued
                },
                None,
                "{side}: no envelope runs on the bracket read"
            );
        }
    }

    /// As [`matched_fixture_bracket_registration`], with B delayed `b_delay_secs` against the
    /// nominal map (B holds A's frame `j` at `j + delay`) and an optional `residual_search_secs`.
    fn matched_fixture_bracket_registration_with(
        border_standoff_secs: f64,
        residual_border_standoff_secs: Option<f64>,
        b_delay_secs: f64,
        residual_search_secs: Option<f64>,
    ) -> (
        Option<crate::domain::patch_result::RegistrationResidual>,
        f64,
    ) {
        use super::{characterize_all_regions, RegionPatchContext, RegionPatchMedia};
        use crate::domain::gap::Gap;
        use crate::domain::gap_fill::FillRegion;
        use crate::domain::gap_repair_spec::GapRepairStrategy;
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
        let delay = secs(b_delay_secs);
        let long: Vec<f32> = (0..total + delay).map(|_| rng()).collect();
        let noise = &long[delay..];
        let mut a = vec![0f32; total];
        let b = long[..total].to_vec();
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
            local_pair_collar_secs: None,
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
            b_stall: None,
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
            border_standoff_secs,
            residual_border_standoff_secs,
            max_anchor_bracket_secs: 0.2,
            max_anchors_per_side: 2,
            min_fill_correlation: 0.0,
            fill_absolute_floor: 0.0,
            normalize_window_secs: 0.05,
            residual_search_secs: residual_search_secs
                .unwrap_or(RepairConfig::default().residual_search_secs),
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
                    alignment,
                    registration_residual,
                    ..
                },
            ) => {
                // `fill_frames == gap_frames` in this fixture, so no head trim moves the written start.
                let misplacement_frames =
                    alignment.start_frame as f64 - specs[0].b_extract.b_mapped_start_frame as f64;
                (
                    registration_residual.as_deref().copied(),
                    misplacement_frames * 1000.0 / f64::from(rate),
                )
            }
            other => {
                panic!("expected a Patch(Bracket) verdict for a matched fixture, got {other:?}")
            }
        }
    }
}
