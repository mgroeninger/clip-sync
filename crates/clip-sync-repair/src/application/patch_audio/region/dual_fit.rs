//! Gate-fail rescue: build the dual-fit input, decide eligibility, measure the residual
//! verdict, and finalize into either a rescue or a skip spec.
//!
//! Depends on `types` (outcome / skip-spec construction) and `pcm` (gap floor, B start frame).
//! Must not import `bracket`, `execute`, or `characterize` — see the plan's cycle guards.

use clip_sync::ProgressReporter;

use crate::application::patch_region::SeamGateFailure;
use crate::domain::patch_result::RegistrationResidual;
use crate::domain::{
    gap_fill::FillRegion,
    gap_fill_fit::classify_fill_waveform_confidence,
    gap_repair_spec::{
        BExtractWindow, GapRepairSpec, GapRepairStrategy, GapRepairTags, GapRepairVerdict,
        SeamLocalTags,
    },
    gap_tags::GapTagsPatchContext,
    patch_result::GapPatchSkipReason,
    policies::{self, RefinedGapFrames},
};

use super::super::PatchAudioRequest;

use super::pcm::{a_gap_floor_db, b_mapped_start_frame};
use super::types::{seam_failure_outcome, skip_region_spec, RegionPatchOutcome};

/// Everything the A3 dual-fit algorithm needs, built once from the decoded window — only when `--dual-fit`
/// is on (so the off path pays nothing). `None` when the gap is too near a window edge for the seam borders.
pub(in crate::application::patch_audio::region) struct DualFitRepairInput<'a> {
    pub(in crate::application::patch_audio::region) params: crate::domain::dual_fit::DualFitParams,
    /// A's two raw seam borders + the nominal B geometry, cut once by
    /// [`crate::domain::seam_locks::SeamLockTemplates::build`] (which also carries the range guard
    /// this constructor used to spell out itself).
    pub(in crate::application::patch_audio::region) templates:
        crate::domain::seam_locks::SeamLockTemplates,
    pub(in crate::application::patch_audio::region) b_mono: Vec<f64>,
    pub(in crate::application::patch_audio::region) b_samples: &'a [f32],
    pub(in crate::application::patch_audio::region) a_start_frame: usize,
    pub(in crate::application::patch_audio::region) a_end_frame: usize,
    pub(in crate::application::patch_audio::region) crossfade_secs: f64,
    /// A's full decoded PCM — for the post-assembly residual measurement (parity with the ordinary
    /// path's residual gate).
    pub(in crate::application::patch_audio::region) a_samples: &'a [f32],
    /// For the post-assembly residual measurement (parity with the ordinary path's residual gate) —
    /// the reference-window walk-out standoff, same constant the ordinary path uses.
    pub(in crate::application::patch_audio::region) border_standoff_frames: usize,
}

/// Grouped inputs for [`build_dual_fit_input`] — the raw A/B buffers, resolved gap geometry, and
/// fill thresholds needed to assemble a [`DualFitRepairInput`]. Bundled into one value so the
/// constructor takes a single, order-safe parameter instead of a 13-wide positional list.
pub(in crate::application::patch_audio::region) struct DualFitInputSpec<'a> {
    pub(in crate::application::patch_audio::region) a_samples: &'a [f32],
    pub(in crate::application::patch_audio::region) b_samples: &'a [f32],
    pub(in crate::application::patch_audio::region) channels: usize,
    pub(in crate::application::patch_audio::region) sample_rate: u32,
    pub(in crate::application::patch_audio::region) refined: RefinedGapFrames,
    pub(in crate::application::patch_audio::region) refined_b_start_secs: f64,
    pub(in crate::application::patch_audio::region) b_extract_start_secs: f64,
    pub(in crate::application::patch_audio::region) gap_frames: usize,
    pub(in crate::application::patch_audio::region) fill_seam_search_secs: f64,
    pub(in crate::application::patch_audio::region) min_fill_correlation: f32,
    pub(in crate::application::patch_audio::region) fill_absolute_floor: f32,
    pub(in crate::application::patch_audio::region) crossfade_secs: f64,
    pub(in crate::application::patch_audio::region) border_standoff_frames: usize,
}

pub(in crate::application::patch_audio::region) fn build_dual_fit_input<'a>(
    spec: DualFitInputSpec<'a>,
) -> Option<DualFitRepairInput<'a>> {
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
    let b_mapped_start =
        b_mapped_start_frame(refined_b_start_secs, b_extract_start_secs, sample_rate);
    // Range guard + raw `mono(refined ± w)` borders, both now owned by `SeamLockTemplates::build`.
    let templates = crate::domain::seam_locks::SeamLockTemplates::build(
        a_samples,
        ch,
        refined.start_frame,
        refined.end_frame,
        b_mapped_start,
        w,
    )?;
    let b_mono: Vec<f64> = crate::domain::pcm::interleaved_to_mono(b_samples, ch);
    let a_gap_floor_db =
        a_gap_floor_db(a_samples, channels, refined.start_frame, refined.end_frame);

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
            edit_crossfade_frames: (crossfade_secs * sample_rate as f64).round() as usize,
        },
        templates,
        b_mono,
        b_samples,
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

/// The routing decision `characterize_region` acts on: [`registration_routes_to_dual_fit`] narrowed to
/// what is adopted so far.
/// - **Dual-fit must be on.** The config layer already drops the bound under `--no-dual-fit`, but a
///   request built directly must not route either.
/// - **Step-kind only** (plan § 6b). Common-mode is unadopted — § 4b left it out of scope below
///   300 ms and § 6d's gross bound is not yet decided — so a common-mode miss never routes, whatever the
///   bound. When § 6d lands it gets its own bound rather than reusing this one.
pub(in crate::application::patch_audio::region) fn registration_route_taken(
    dual_fit: bool,
    bound_ms: Option<f64>,
    crossfade_ms: f64,
    pre_ms: Option<f64>,
    post_ms: Option<f64>,
) -> Option<RouteKind> {
    if !dual_fit {
        return None;
    }
    match registration_routes_to_dual_fit(bound_ms, crossfade_ms, pre_ms, post_ms)? {
        RouteKind::Step => Some(RouteKind::Step),
        RouteKind::Common => None,
    }
}

/// A gap is step-kind when its shoulders disagree by more than this (routing plan § 0): no single lag
/// can satisfy both. At or below it the shoulders agree and the miss is common-mode.
const STEP_KIND_THRESHOLD_MS: f64 = 20.0;

/// Which kind of registration miss routed a gap to dual-fit (routing plan § 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::application::patch_audio::region) enum RouteKind {
    /// The shoulders disagree (a map step), so the two seam errors differ by the step.
    Step,
    /// The shoulders agree and the fill sits off both by about the same amount.
    Common,
}

/// Registration routing (routing plan § 5): should a gate-passing single-lag fill be tried as dual-fit
/// first? `Some(kind)` when both shoulders are licensed and the fill's far-seam error exceeds the bound.
///
/// `pre_ms` / `post_ms` are the per-shoulder `registration_residual_*_ms` values as the patch report
/// carries them — the **planned** frame, `None` for an unlicensed shoulder. The bound is quoted in the
/// **written** frame, which runs one crossfade width ahead of the plan (plan § 3), so this adds
/// `crossfade_ms` itself and a caller cannot forget it. The shift cancels in the step test.
///
/// Strict `>` against the bound. `None` bound, an unlicensed shoulder (invariant 4: one-sided
/// registration never routes) or a non-finite value never routes.
pub(in crate::application::patch_audio::region) fn registration_routes_to_dual_fit(
    bound_ms: Option<f64>,
    crossfade_ms: f64,
    pre_ms: Option<f64>,
    post_ms: Option<f64>,
) -> Option<RouteKind> {
    let bound = bound_ms?;
    let pre = pre_ms? + crossfade_ms;
    let post = post_ms? + crossfade_ms;
    if !(bound.is_finite() && pre.is_finite() && post.is_finite()) {
        return None;
    }
    if pre.abs().max(post.abs()) <= bound {
        return None;
    }
    Some(if (post - pre).abs() > STEP_KIND_THRESHOLD_MS {
        RouteKind::Step
    } else {
        RouteKind::Common
    })
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
    grown_post_window_frames: Option<usize>,
) -> Option<policies::SeamResidualVerdict> {
    if !(request.measure_residual
        || request.residual_gate.is_active()
        || tracing::enabled!(tracing::Level::DEBUG))
    {
        return None;
    }
    let nominal_delta = df.templates.b_mapped_start() as i64 - df.a_start_frame as i64;
    let max_lag_frames =
        crate::domain::residual_max_lag_frames(df.params.sample_rate, request.residual_lag_secs);
    let window = df.params.seam_window_frames;
    // Plan lock 2 (`docs/dev/archive/TEMP-dual-fit-grow-post-plan.md` §3.2) grows the residual **window** to
    // match the locked template, not the floor-search **stride**: `step_frames` stays the 250 ms gate
    // window on both shoulders, so a grown post residual still walks the same density
    // (`max_walk_frames / seam_window_frames` tiles) rather than a coarser one just because the
    // correlation window widened.
    let floor_common = |window: usize| policies::SeamFloorParams {
        a_samples: df.a_samples,
        channels: df.params.channels,
        b_mono: &df.b_mono,
        window,
        standoff_frames: df.border_standoff_frames,
        a_to_b_delta: nominal_delta,
        step_frames: df.params.seam_window_frames.max(1),
        max_walk_frames: df.params.sample_rate as usize * 3,
        absolute_silence_rms: request.absolute_silence_rms,
        max_lag_frames,
    };
    // A grown post lock widened the post shoulder's own seam window; the pre shoulder stays at the
    // 250 ms gate window regardless.
    let post_window = grown_post_window_frames.unwrap_or(window);
    let (chosen_pre, floor_pre) = policies::seam_chosen_and_floor(
        &floor_common(window),
        policies::SeamSide::Pre,
        df.a_start_frame,
        df.a_end_frame,
        nominal_delta + r.pre_lag,
    );
    let (chosen_post, floor_post) = policies::seam_chosen_and_floor(
        &floor_common(post_window),
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

/// S1 (`docs/dev/TEMP-registration-residual-plan.md`): a shoulder's independent seam-local read is
/// usable only when its peak `r` clears the same confidence floor the fit gate/`try_dual_fit` use.
/// Returns `applied_ms − shoulder_lag_ms` (signed) when confident, `None` otherwise.
pub(in crate::application::patch_audio::region) fn registration_shoulder_residual_ms(
    peak_r: f64,
    lag_frames: i64,
    applied_ms: f64,
    sample_rate: u32,
    confident_floor: f64,
) -> Option<f64> {
    if peak_r < confident_floor {
        return None;
    }
    let shoulder_lag_ms = lag_frames as f64 * 1000.0 / f64::from(sample_rate);
    Some(applied_ms - shoulder_lag_ms)
}

/// Keep both shoulders' reads and select the one that agrees more closely with the placement
/// (smallest `|residual|`, pre on a tie) as the historical `registration_residual_ms`. `None` when
/// neither is confident — record-only, never a fallback to an unconfident reading.
pub(in crate::application::patch_audio::region) fn registration_residual_from_shoulders(
    pre_ms: Option<f64>,
    post_ms: Option<f64>,
    applied_ms: f64,
) -> Option<RegistrationResidual> {
    let selected_ms = match (pre_ms, post_ms) {
        (Some(p), Some(q)) => {
            if p.abs() <= q.abs() {
                p
            } else {
                q
            }
        }
        (Some(p), None) => p,
        (None, Some(q)) => q,
        (None, None) => return None,
    };
    Some(RegistrationResidual {
        registration_residual_ms: selected_ms,
        registration_residual_pre_ms: pre_ms,
        registration_residual_post_ms: post_ms,
        registration_applied_ms: applied_ms,
        // Trust is attached by the caller, which holds the peaks; see
        // `RegistrationResidual::with_shoulder_quality`.
        registration_residual_pre_edge_pinned: None,
        registration_residual_post_edge_pinned: None,
        registration_residual_pre_z: None,
        registration_residual_post_z: None,
        registration_residual_pre_prominence: None,
        registration_residual_post_prominence: None,
        registration_residual_pre_rescued: None,
        registration_residual_post_rescued: None,
    })
}

/// A dual-fit attempt's two halves, which are only ever available together.
///
/// The locks are the ones `characterize_region` measured before the gate search — it needs them
/// there to decide `shoulders_abut` — so the skip decision and the fit read one set of lags, and the
/// gap pays one ±600 ms per-shoulder search rather than two. Bundled because there is no meaningful
/// state where one is present and the other is not: both are built from the same
/// [`build_dual_fit_input`] result.
pub(in crate::application::patch_audio::region) struct DualFitAttempt<'a> {
    pub(in crate::application::patch_audio::region) input: &'a DualFitRepairInput<'a>,
    pub(in crate::application::patch_audio::region) locks: &'a crate::domain::seam_locks::SeamLocks,
}

pub(in crate::application::patch_audio::region) fn skip_or_dual_fit(
    progress: &dyn ProgressReporter,
    request: &PatchAudioRequest,
    region: &FillRegion,
    fail: SeamGateFailure,
    min_structure_match_score: f32,
    tag_ctx: GapTagsPatchContext,
    attempt: Option<DualFitAttempt<'_>>,
) -> (DualFitDecision, GapTagsPatchContext) {
    let mut failure = fail;
    let mut dual_fit_attempt: Option<crate::domain::patch_result::SeamScoreAttempt> = None;
    if dual_fit_eligible(request.dual_fit, fail) {
        if let Some(DualFitAttempt { input: df, locks }) = attempt {
            match try_dual_fit_rescue(request, region, df, locks) {
                Ok(rescue) => return (DualFitDecision::Rescued(rescue), tag_ctx),
                Err(DualFitDecline::NoFit) => {}
                Err(DualFitDecline::Revalidation(scores)) => dual_fit_attempt = Some(scores),
                Err(DualFitDecline::HeadroomExceeded {
                    attempt: scores,
                    failure: headroom,
                }) => {
                    failure = headroom;
                    dual_fit_attempt = Some(scores);
                }
            }
        }
    }
    let (_, outcome, tag_ctx) = seam_failure_outcome(
        progress,
        region,
        failure,
        min_structure_match_score,
        tag_ctx,
        dual_fit_attempt,
    );
    (dual_fit_skipped(outcome), tag_ctx)
}

/// Why [`try_dual_fit_rescue`] did not produce a fill. `skip_or_dual_fit` reads it to pick the failure
/// it reports; a routed attempt (registration bound) ignores it and keeps the single-lag fill.
pub(in crate::application::patch_audio::region) enum DualFitDecline {
    /// The fit itself declined (no assembled bridge); no seam scores exist.
    NoFit,
    /// The assembled seams failed `classify_fill_waveform_confidence`.
    Revalidation(crate::domain::patch_result::SeamScoreAttempt),
    /// The residual headroom gate rejected the candidate; `failure` replaces the caller's.
    HeadroomExceeded {
        attempt: crate::domain::patch_result::SeamScoreAttempt,
        failure: SeamGateFailure,
    },
}

/// The dual-fit attempt proper: grow-post ladder, fit, seam re-validation, residual headroom gate and
/// `SilenceSplice` assembly. It owns no skip decision — the caller does — so it can be entered without
/// a `SeamGateFailure`.
pub(in crate::application::patch_audio::region) fn try_dual_fit_rescue(
    request: &PatchAudioRequest,
    region: &FillRegion,
    df: &DualFitRepairInput<'_>,
    locks: &crate::domain::seam_locks::SeamLocks,
) -> Result<Box<DualFitRescue>, DualFitDecline> {
    let local_gain = request.local_gain(region.gap_index);
    // Grow the post seam window before the fit is attempted (§0 lock 3 — grow before the
    // gate rejects, not on the result): a 250 ms post lock below the admit line is re-locked
    // at the shortest window in `POST_GROW_LADDER_MS` that stops the ladder
    // (`docs/dev/archive/TEMP-dual-fit-grow-post-plan.md` §3.2). The whole `GrownPostLock` is handed
    // to `dual_fit_from_locks_with_grown_post`, which applies the post-lock swap
    // (`b_post_seam`/`bridge_frames`/`gate_pass`) and threads its `a_post`/`window_frames`
    // itself — this caller never builds a parallel `(locks, a_post)` pair by hand, so it
    // cannot desync them. The pre lock and `SeamLockTemplates` (§0 lock 2) are untouched.
    let grown = if locks.post.r < crate::domain::seam_locks::POST_GROW_ADMIT_R {
        crate::domain::seam_locks::grow_post_lock(
            df.a_samples,
            df.params.channels,
            df.a_end_frame,
            &df.b_mono,
            df.templates.b_mapped_start(),
            df.params.gap_frames,
            locks.pre.lag,
            df.params.max_lag_frames,
            df.params.sample_rate,
            locks.post.lag,
        )
    } else {
        None
    };
    let Some(r) = crate::domain::dual_fit::dual_fit_from_locks_with_grown_post(
        &df.templates,
        locks,
        &df.b_mono,
        df.b_samples,
        &df.params,
        grown.as_ref(),
    ) else {
        return Err(DualFitDecline::NoFit);
    };
    // Re-validate the assembled seams. The seam scores are the ones `try_dual_fit`'s own
    // seam-local search already measured (`r.pre_seam_r`/`r.post_seam_r`): A's near-gap
    // border vs the B window ENDING at the pre shoulder / STARTING at the post shoulder —
    // the content just OUTSIDE the bridge that each shoulder matched. The interior length
    // edit (`jump_edit_to_gap`) only edits the bridge interior, guarded clear of
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
            let grown_post_window_frames = grown.as_ref().map(|g| g.window_frames);
            let residual =
                measure_dual_fit_residual_verdict(request, df, &r, grown_post_window_frames);
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
                    return Err(DualFitDecline::HeadroomExceeded {
                        attempt: crate::domain::patch_result::SeamScoreAttempt {
                            pre_correlation: splice_pre,
                            post_correlation: splice_post,
                            source: crate::domain::patch_result::SeamScoreSource::DualFit,
                        },
                        failure: SeamGateFailure::ResidualHeadroomExceeded {
                            pre: splice_pre,
                            post: splice_post,
                            residual: verdict,
                            margin_db,
                        },
                    });
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
                // GCC-PHAT experiment (`TEMP-phat-seam-scoring-plan.md`): diagnostic-only, never
                // computed on the production patch path.
                pre_seam_phat_r: None,
                post_seam_phat_r: None,
                pre_seam_phat_prom: None,
                post_seam_phat_prom: None,
                pre_seam_phat_z: None,
                post_seam_phat_z: None,
                pre_seam_lag_ms: None,
                post_seam_lag_ms: None,
                pre_seam_phat_lag_ms: None,
                post_seam_phat_lag_ms: None,
                // Provenance IS available on this path — the locks the fit was built from.
                // Recording it here is what lets a projected dump answer "was this shoulder
                // rescued / pinned at the search boundary?" without a decode.
                pre_seam_rescued: Some(locks.pre.rescued),
                post_seam_rescued: Some(locks.post.rescued),
                pre_seam_edge_pinned: Some(locks.pre.edge_pinned),
                post_seam_edge_pinned: Some(locks.post.edge_pinned),
                seams_clamped: Some(locks.clamped()),
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
            // S1: dual-fit has no single rigid "applied lag" (each shoulder is placed by its
            // own independent seam-local match) — `align_adjustment_secs` is recorded as 0.0
            // for this path (see the `outcome_from_spec`/`execute_region_spec` SilenceSplice
            // arms below), so `applied_ms = 0.0` keeps this field's convention consistent with
            // that recorded value rather than inventing a different baseline.
            let confident_floor = f64::from(
                request
                    .min_fill_correlation
                    .max(request.fill_absolute_floor),
            );
            let registration_residual = registration_residual_from_shoulders(
                registration_shoulder_residual_ms(
                    r.pre_seam_r,
                    r.pre_lag,
                    0.0,
                    df.params.sample_rate,
                    confident_floor,
                ),
                registration_shoulder_residual_ms(
                    r.post_seam_r,
                    r.post_lag,
                    0.0,
                    df.params.sample_rate,
                    confident_floor,
                ),
                0.0,
            )
            .map(|rr| {
                // This path's shoulders are the placement locks themselves (±600 ms fine
                // search, envelope recenter). The bracket path reads bare seam peaks over
                // a nominal-to-fill span at `residual_search_secs`, so `*_rescued`
                // exists only here, and the `applied` term differs between arms.
                rr.with_shoulder_quality(crate::domain::patch_result::ShoulderQuality::from_locks(
                    Some(&locks.pre),
                    Some(&locks.post),
                ))
            });
            // `--seam-extend-fill` for this arm. Cut here rather than at the splice: the
            // shoulder lags are anchored on nominal `b_mapped` in B-**extract** frames,
            // and this is the one site holding both them and the extract. The post
            // shoulder supplies the tail — using the pre shoulder plus `gap_frames`
            // would bake in the shoulder skew (`trim_frames`) at the seam.
            let seam_margin = if request.seam_extend_fill {
                let b_mapped_start = df.templates.b_mapped_start() as i64;
                let gap_frames = df.a_end_frame.saturating_sub(df.a_start_frame) as i64;
                super::pcm::seam_margin_from_b_shoulders(
                    df.b_samples,
                    df.params.channels,
                    b_mapped_start + r.pre_lag,
                    b_mapped_start + gap_frames + r.post_lag,
                    crate::domain::policies::effective_seam_crossfade_frames(
                        (df.crossfade_secs * df.params.sample_rate as f64) as usize,
                        df.a_start_frame,
                        df.a_end_frame,
                        df.a_samples.len() / df.params.channels.max(1),
                    ),
                )
            } else {
                None
            };
            Ok(Box::new(DualFitRescue {
                strategy: GapRepairStrategy::SilenceSplice {
                    fill: r.fill,
                    seam_margin,
                    pre_seam_r: splice_pre,
                    post_seam_r: splice_post,
                    pre_lag: r.pre_lag,
                    post_lag: r.post_lag,
                    trim_frames: r.trim_frames,
                    step_real: r.step_real,
                    residual,
                    confidence,
                    registration_residual: registration_residual.map(Box::new),
                    // Since 2026-08-31 dual-fit takes the hole-local half too:
                    // the paired collar reads A-versus-B blocks flanking the gap
                    // and never the fill, so nothing about a dual-fit interior is
                    // out of its reach. Both halves ride the strategy so this and
                    // `outcome_from_spec` cannot disagree about what was applied.
                    normalize_gain: crate::domain::stack_fill_gain(
                        request.program_gain.gain(),
                        local_gain,
                    )
                    .total,
                    local_gain,
                    dual_fit_post_window_ms: grown.as_ref().map(|g| g.window_ms),
                },
                tags_ctx,
                refined: RefinedGapFrames {
                    start_frame: df.a_start_frame,
                    end_frame: df.a_end_frame,
                },
                crossfade_secs: df.crossfade_secs,
            }))
        }
        Err(min_score) => {
            tracing::debug!(
                            a_start = df.a_start_frame,
                            pre = splice_pre,
                            post = splice_post,
                            min_score,
                            "dual-fit candidate failed re-validation at the assembled seam; falling back to skip"
                        );
            Err(DualFitDecline::Revalidation(
                crate::domain::patch_result::SeamScoreAttempt {
                    pre_correlation: splice_pre,
                    post_correlation: splice_post,
                    source: crate::domain::patch_result::SeamScoreSource::DualFit,
                },
            ))
        }
    }
}

/// What `skip_or_dual_fit` decided, **before** the region geometry is attached (8c). The dual-fit call site
/// lacks `b_extract` / `gap_offset`, so on a rescue it returns the SilenceSplice *strategy* plus the base
/// geometry it does own (from the dual-fit input); [`finalize_dual_fit`] — called from `characterize_region`,
/// which holds the region — wraps it into a full [`GapRepairSpec`]. This keeps geometry ownership in one
/// place (the decided 8c approach) instead of stamping inert placeholders. `gap_index` is copied from
/// `region` there, same as the bracket and skip constructors, not carried on this enum.
pub(in crate::application::patch_audio::region) enum DualFitDecision {
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
pub(in crate::application::patch_audio::region) struct DualFitRescue {
    strategy: GapRepairStrategy,
    tags_ctx: GapRepairTags,
    refined: RefinedGapFrames,
    crossfade_secs: f64,
}

/// Wrap a [`DualFitDecision`] into a [`GapRepairSpec`], attaching the region geometry the dual-fit call site
/// lacked (`gap_offset`, `b_extract`, `gap_index`). Index comes from `region`, same as the bracket spec and
/// [`skip_region_spec`] — not a SilenceSplice-only field and not deferred to fingerprint projection.
pub(in crate::application::patch_audio::region) fn finalize_dual_fit(
    decision: DualFitDecision,
    region: &FillRegion,
    gap_offset_secs: f64,
    b_extract: BExtractWindow,
    gap_refined: RefinedGapFrames,
) -> GapRepairSpec {
    match decision {
        DualFitDecision::Rescued(rescue) => {
            let DualFitRescue {
                strategy,
                tags_ctx,
                refined,
                crossfade_secs,
            } = *rescue;
            GapRepairSpec {
                gap_index: region.gap_index,
                a_start_secs: region.a_start_secs,
                a_end_secs: region.a_end_secs,
                gap_offset_secs,
                refined,
                b_extract,
                crossfade_secs,
                verdict: GapRepairVerdict::Patch(strategy),
                tags_ctx,
            }
        }
        DualFitDecision::Skipped { reason, residual } => skip_region_spec(
            reason,
            residual,
            region,
            gap_offset_secs,
            gap_refined,
            b_extract,
        ),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::gap::GapReport;
    use crate::domain::gap_fill_fit::FillConfidence;

    /// The production crossfade width; the predicate shifts planned residuals into the written frame.
    const CF: f64 = 10.0;

    fn routes(bound: Option<f64>, pre: Option<f64>, post: Option<f64>) -> Option<RouteKind> {
        registration_routes_to_dual_fit(bound, CF, pre, post)
    }

    #[test]
    fn registration_routes_step_and_common_mode_above_the_bound() {
        // 13/g16 as the report carries it (planned frame): written −72.8 / −109.8, a 37 ms step.
        assert_eq!(
            routes(Some(75.0), Some(-82.79), Some(-119.79)),
            Some(RouteKind::Step)
        );
        // 26/g6-like: written about −904 / −901, shoulders agree to 3 ms.
        assert_eq!(
            routes(Some(75.0), Some(-914.5), Some(-910.8)),
            Some(RouteKind::Common)
        );
    }

    #[test]
    fn registration_routing_needs_both_shoulders_licensed() {
        // Invariant 4: one-sided registration never routes, however large the licensed side is.
        assert_eq!(routes(Some(75.0), None, Some(-500.0)), None);
        assert_eq!(routes(Some(75.0), Some(-500.0), None), None);
        assert_eq!(routes(Some(75.0), None, None), None);
    }

    #[test]
    fn registration_routing_is_off_without_a_bound_or_on_non_finite_input() {
        assert_eq!(routes(None, Some(-500.0), Some(-500.0)), None);
        assert_eq!(routes(Some(f64::NAN), Some(-500.0), Some(-500.0)), None);
        assert_eq!(routes(Some(75.0), Some(f64::NAN), Some(-500.0)), None);
        assert_eq!(routes(Some(75.0), Some(-500.0), Some(f64::INFINITY)), None);
    }

    #[test]
    fn registration_routing_bound_is_strict() {
        // Planned 65 / 60 is written 75 / 70: exactly at a 75 ms bound, so it stays single-lag.
        assert_eq!(routes(Some(75.0), Some(65.0), Some(60.0)), None);
        // One tick under the bound routes; the far seam is the one that counts, whichever it is.
        assert!(routes(Some(74.99), Some(65.0), Some(60.0)).is_some());
        assert!(routes(Some(74.99), Some(60.0), Some(65.0)).is_some());
    }

    #[test]
    fn registration_routing_bound_is_in_the_written_frame() {
        // Late fill: planned 66 is written 76, so it clears a 75 ms bound it would not on the report
        // value alone.
        assert!(routes(Some(75.0), Some(66.0), Some(66.0)).is_some());
        // Early fill: planned −84 is written −74, so the same shift pulls it back under the bound.
        assert_eq!(routes(Some(75.0), Some(-84.0), Some(-84.0)), None);
    }

    #[test]
    fn registration_route_taken_is_step_kind_only() {
        let taken = |dual_fit, pre, post| {
            registration_route_taken(dual_fit, Some(75.0), CF, Some(pre), Some(post))
        };
        // 13/g16-like step: acted on.
        assert_eq!(taken(true, -82.79, -119.79), Some(RouteKind::Step));
        // 26/g6-like common-mode, well past the bound: the predicate sees it, the decision does not.
        assert_eq!(
            registration_routes_to_dual_fit(Some(75.0), CF, Some(-914.5), Some(-910.8)),
            Some(RouteKind::Common)
        );
        assert_eq!(taken(true, -914.5, -910.8), None);
    }

    #[test]
    fn registration_route_taken_needs_dual_fit_on() {
        assert_eq!(
            registration_route_taken(false, Some(75.0), CF, Some(-82.79), Some(-119.79)),
            None
        );
    }

    #[test]
    fn registration_kind_boundary_is_strict_at_twenty_ms() {
        // Written 60 / 80: a step of exactly 20 ms is still common-mode.
        assert_eq!(
            routes(Some(50.0), Some(50.0), Some(70.0)),
            Some(RouteKind::Common)
        );
        // Past 20 ms it is a step, in either direction.
        assert_eq!(
            routes(Some(50.0), Some(50.0), Some(70.5)),
            Some(RouteKind::Step)
        );
        assert_eq!(
            routes(Some(50.0), Some(70.5), Some(50.0)),
            Some(RouteKind::Step)
        );
    }

    #[test]
    fn dual_fit_eligible_excludes_structure_alignment_failed() {
        // A2: `StructureAlignmentFailed` means structure search never scored a bracket, so there is
        // nothing "exhausted" for dual-fit to rescue — it must fall through to the ordinary skip
        // regardless of whether `--dual-fit` is on.
        assert!(!dual_fit_eligible(
            true,
            SeamGateFailure::StructureAlignmentFailed
        ));
        assert!(!dual_fit_eligible(
            false,
            SeamGateFailure::StructureAlignmentFailed
        ));

        // Other seam-gate failure variants are scored-but-failed skips, so they DO qualify when
        // `--dual-fit` is on, and never qualify when it's off.
        let scored_but_failed = SeamGateFailure::StructureBelowThreshold {
            pre: 0.1,
            post: 0.1,
        };
        assert!(dual_fit_eligible(true, scored_but_failed));
        assert!(!dual_fit_eligible(false, scored_but_failed));
    }

    /// The echo must carry the value this run patched with, per field. The 2026-08-17 standoff sweep
    /// had no such record: four arms differing only by `--border-standoff-secs`, and nothing in the
    /// report to tell a real sweep from a flag that never applied. Distinct values per field so a
    /// copy-paste that echoes the neighbouring knob fails here.
    #[test]
    fn fill_geometry_echo_carries_the_values_this_run_patched_with() {
        let mut request = dual_fit_test_request(false, crate::domain::ResidualGateMode::Off);
        request.settings.border_standoff_secs = 0.11;
        request.settings.fill_border_search_secs = 0.22;
        request.settings.min_border_discovery_secs = 0.33;
        request.settings.fill_seam_search_secs = 0.44;

        request.settings.normalize_window_secs = 0.55;

        let echo = crate::application::patch_audio::fill_geometry_for(&request);

        // Asserting `echo.x == request.settings.x` would only prove the copy copies — it would pass
        // just as well if the patch path read a different field. So bind the echo to the code that
        // turns these settings into frames: `SeamGateDerived::from_repair` for the run-constant pair,
        // `FillWindowFrames::for_gap` for the per-gap trio. If the echo ever names a setting the
        // search does not use, these stop agreeing.
        let rate = 48_000u32;
        let derived =
            crate::application::patch_region::SeamGateDerived::from_repair(&request, rate, 1, 0.01);
        let secs_to_frames = |s: f64| (s * f64::from(rate)).round() as usize;
        assert_eq!(
            secs_to_frames(echo.border_standoff_secs),
            derived.border_standoff_frames,
            "the echoed standoff must be the one held off the gap edge"
        );
        assert_eq!(
            secs_to_frames(echo.fill_border_search_secs),
            derived.search_radius_frames,
            "the echoed border search must be the radius the fill search actually scans"
        );

        // A gap long enough that `border_frames` is not clamped by `correlate_frames`, so the echoed
        // normalize window is readable as the A-border template width it sizes.
        let gap_frames = rate as usize * 2;
        let windows = crate::application::patch_audio::geometry::FillWindowFrames::for_gap(
            &request.settings,
            gap_frames,
            rate,
        );
        assert_eq!(
            crate::application::patch_audio::geometry::correlate_frames_for_gap(
                echo.normalize_window_secs,
                echo.min_border_discovery_secs,
                gap_frames,
                rate,
            ),
            windows.correlate_frames,
            "the echoed normalize/discovery pair must reproduce the correlation window"
        );
        assert_eq!(
            crate::application::patch_audio::geometry::seam_gate_frames_for(
                windows.correlate_frames,
                echo.fill_seam_search_secs,
                rate,
            ),
            windows.seam_gate_frames,
            "the echoed seam search must reproduce the seam-gate cap"
        );
        assert_eq!(
            crate::application::patch_audio::geometry::border_frames_from_secs(
                echo.normalize_window_secs,
                rate
            ),
            windows.border_frames,
            "the echoed normalize window IS the A-border template width on this path"
        );
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
            local_pair_collar_secs: None,
            video_a: std::path::PathBuf::from("a.wav"),
            video_b: std::path::PathBuf::from("b.wav"),
            track_compatibility: None,
            alignment,
            gaps: Vec::new(),
            gap_equivalence: Vec::new(),
            gap_offset_agreement: None,
            decode_chunk_secs: 60,
            recipe: crate::domain::ScanRecipe::with_hold_blocks(1000, 0, 250, 0.01, 0.0),
            limit_fill_to_mapped_region: true,
            b_scanned_end_secs: None,
            b_scan_truncated: false,
            b_stall: None,
            audio_timeline_skew: None,
            program_levels: None,
        };
        let repair = crate::infrastructure::config::RepairConfig {
            residual_gate,
            absolute_silence_rms: 0.001,
            ..Default::default()
        };
        let mut request = repair
            .patch_settings()
            .into_request(report)
            .expect("default All gap selection");
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
        assert_eq!(
            request.gap_signature_mode,
            request.settings.gap_signature_mode
        );
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
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
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
            edit_crossfade_frames: 480,
        };
        let templates = crate::domain::seam_locks::SeamLockTemplates::from_mono(
            a_pre_mono.clone(),
            a_post_mono.clone(),
            gap,
            b_mapped_start,
            w,
        )
        .expect("fixture borders are exactly one seam window");
        let r =
            crate::domain::dual_fit::dual_fit_at_nominal(&templates, &b_mono, &b_samples, &params)
                .expect("dual-fit target");
        assert_eq!(
            r.pre_lag, 0,
            "pre shoulder matches at nominal lag in this fixture"
        );
        assert_eq!(
            r.post_lag, step,
            "post shoulder's seam-local match sits at +step in B"
        );

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
            templates,
            b_mono,
            b_samples: &b_samples,
            a_start_frame,
            a_end_frame,
            crossfade_secs: 0.02,
            a_samples: &a_samples,
            border_standoff_frames: 0,
        };

        let request = dual_fit_test_request(true, crate::domain::ResidualGateMode::VetoRescue);
        let verdict = measure_dual_fit_residual_verdict(&request, &df, &r, None)
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
        assert!(measure_dual_fit_residual_verdict(&off_request, &df, &r, None).is_none());
    }

    /// S1 (`docs/dev/TEMP-registration-residual-plan.md`) — `registration_shoulder_residual_ms`
    /// pins the sign convention (`applied_ms − shoulder_lag_ms`) and the confidence-floor cutoff:
    /// below the floor the shoulder is unusable and must not leak a stale reading.
    #[test]
    fn registration_shoulder_residual_ms_applies_sign_and_floor() {
        let sr = 48_000u32;
        // 240 frames at 48kHz = 5ms; applied placement is 100ms.
        let residual = registration_shoulder_residual_ms(0.9, 240, 100.0, sr, 0.3)
            .expect("peak above floor must yield a residual");
        assert!(
            (residual - 95.0).abs() < 1e-9,
            "applied_ms − shoulder_lag_ms: 100.0 − 5.0 = 95.0, got {residual}"
        );

        assert_eq!(
            registration_shoulder_residual_ms(0.29, 240, 100.0, sr, 0.3),
            None,
            "peak r just below the confidence floor must not produce a reading"
        );
        assert_eq!(
            registration_shoulder_residual_ms(0.3, 240, 100.0, sr, 0.3),
            Some(95.0),
            "peak r exactly at the floor is confident (>= not >)"
        );
    }

    /// `registration_residual_from_shoulders` never falls back to an unconfident reading, among two
    /// confident readings selects the one closer to the applied placement, and keeps both per-seam
    /// reads either way (the far seam is what the routing trigger reads).
    /// The trust siblings stay in lockstep with the `*_ms` keys they describe: a shoulder with no
    /// confident residual carries no `edge_pinned`/`z`, so a census cannot filter on a reading the
    /// report never made. (Contract: `docs/json-output.md` § Trust keys.)
    #[test]
    fn shoulder_quality_is_dropped_for_an_unlicensed_seam() {
        use crate::domain::patch_result::ShoulderQuality;
        let quality = ShoulderQuality {
            pre_edge_pinned: Some(true),
            post_edge_pinned: Some(false),
            pre_z: Some(1.5),
            post_z: Some(9.0),
            pre_prominence: Some(0.02),
            post_prominence: Some(0.4),
            pre_rescued: Some(true),
            post_rescued: Some(false),
        };
        // Only the post shoulder is confident, so only its trust keys survive.
        let r = registration_residual_from_shoulders(None, Some(4.0), 1.0)
            .expect("post confident")
            .with_shoulder_quality(quality);
        assert_eq!(r.registration_residual_pre_edge_pinned, None);
        assert_eq!(r.registration_residual_pre_z, None);
        assert_eq!(r.registration_residual_post_edge_pinned, Some(false));
        assert_eq!(r.registration_residual_post_z, Some(9.0));
        assert_eq!(r.registration_residual_pre_prominence, None);
        assert_eq!(r.registration_residual_pre_rescued, None);
        assert_eq!(r.registration_residual_post_prominence, Some(0.4));
        assert_eq!(r.registration_residual_post_rescued, Some(false));

        // Both confident: both sides carry their own trust, unswapped.
        let both = registration_residual_from_shoulders(Some(-2.0), Some(4.0), 1.0)
            .expect("both confident")
            .with_shoulder_quality(quality);
        assert_eq!(both.registration_residual_pre_edge_pinned, Some(true));
        assert_eq!(both.registration_residual_pre_z, Some(1.5));
        assert_eq!(both.registration_residual_post_edge_pinned, Some(false));
        assert_eq!(both.registration_residual_post_z, Some(9.0));
        assert_eq!(both.registration_residual_pre_prominence, Some(0.02));
        assert_eq!(both.registration_residual_pre_rescued, Some(true));
        assert_eq!(both.registration_residual_post_prominence, Some(0.4));
        assert_eq!(both.registration_residual_post_rescued, Some(false));
    }

    #[test]
    fn registration_residual_selects_smaller_magnitude_and_keeps_both_seams() {
        let sel = |pre, post| {
            registration_residual_from_shoulders(pre, post, 0.0).map(|r| r.registration_residual_ms)
        };
        assert_eq!(sel(None, None), None);
        assert_eq!(sel(Some(12.0), None), Some(12.0));
        assert_eq!(sel(None, Some(-7.0)), Some(-7.0));
        assert_eq!(
            sel(Some(-3.0), Some(9.0)),
            Some(-3.0),
            "smaller |residual| wins regardless of sign"
        );
        assert_eq!(
            sel(Some(5.0), Some(-5.0)),
            Some(5.0),
            "tie: pre (first argument) wins"
        );

        let r = registration_residual_from_shoulders(Some(-72.8), Some(-109.8), 163.3)
            .expect("both shoulders confident");
        assert_eq!(r.registration_residual_ms, -72.8);
        assert_eq!(r.registration_residual_pre_ms, Some(-72.8));
        assert_eq!(
            r.registration_residual_post_ms,
            Some(-109.8),
            "the far seam is kept, not discarded by the selection"
        );
        assert_eq!(r.registration_applied_ms, 163.3);

        let one =
            registration_residual_from_shoulders(None, Some(4.0), 1.0).expect("post confident");
        assert_eq!(
            (
                one.registration_residual_pre_ms,
                one.registration_residual_post_ms
            ),
            (None, Some(4.0)),
            "an unconfident seam stays None"
        );
    }

    /// Exit #4 (`docs/dev/TEMP-registration-residual-plan.md`): the dual-fit path's
    /// `registration_residual_ms` must be derivable purely from `try_dual_fit`'s own returned
    /// `pre_lag`/`post_lag`/`pre_seam_r`/`post_seam_r` — no new sweep. This reuses the exact
    /// synthetic fixture from `measure_dual_fit_residual_verdict_attaches_a_real_verdict` (pre
    /// shoulder matches at lag 0, post shoulder matches at `+step`) and confirms the composed
    /// helpers reproduce the hand-computed residual with `applied_ms = 0.0` (dual-fit's
    /// `align_adjustment_secs` convention).
    #[test]
    fn dual_fit_registration_residual_derives_from_try_dual_fit_lags_only() {
        let sr = 48_000u32;
        let w = 1200usize;
        let gap = 4000usize;
        let step = 200i64;

        let mut seed = 0xDEAD_BEEF_1234_5678u64;
        let mut rng = move || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
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
            channels: 1,
            sample_rate: sr,
            gap_frames: gap,
            seam_window_frames: w,
            max_lag_frames: (0.6 * sr as f64) as usize,
            min_fill_correlation: 0.35,
            fill_absolute_floor: 0.12,
            step_real_margin: 0.15,
            a_gap_floor_db: -60.0,
            edit_crossfade_frames: 480,
        };
        let templates = crate::domain::seam_locks::SeamLockTemplates::from_mono(
            a_pre_mono.clone(),
            a_post_mono.clone(),
            gap,
            b_mapped_start,
            w,
        )
        .expect("fixture borders are exactly one seam window");
        let r =
            crate::domain::dual_fit::dual_fit_at_nominal(&templates, &b_mono, &b_samples, &params)
                .expect("dual-fit target");

        let confident_floor = params.min_fill_correlation.max(params.fill_absolute_floor);
        let registration_residual = registration_residual_from_shoulders(
            registration_shoulder_residual_ms(r.pre_seam_r, r.pre_lag, 0.0, sr, confident_floor),
            registration_shoulder_residual_ms(r.post_seam_r, r.post_lag, 0.0, sr, confident_floor),
            0.0,
        )
        .map(|r| r.registration_residual_ms);

        // Pre matches at lag 0 (residual 0.0), post at +step frames (residual −step ms);
        // pre has the smaller magnitude and wins.
        assert_eq!(
            registration_residual,
            Some(0.0),
            "pre shoulder (lag 0, residual 0.0) has the smaller magnitude vs. post's \
             (−{step} frames), so it is selected: r={r:?}"
        );
    }
}
