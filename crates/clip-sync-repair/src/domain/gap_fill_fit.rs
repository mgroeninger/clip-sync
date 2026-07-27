//! Waveform seam search and unified structure+waveform fit (Phase A/B/C).

use std::cell::Cell;
use std::sync::OnceLock;
use std::time::Instant;

use serde::Serialize;

use crate::domain::gap_signature::{
    score_post_for_signature, score_pre_for_signature, snap_fill_to_gap, GapSignature,
    StructureTimeline,
};
use crate::domain::gap_structure::{
    self, combined_structure_score, prefer_end, prefer_start, search_coarse_step,
    FillBracketPlacement, GapContextSignature, StructureMatchParams,
};
use crate::domain::patch_anchor::AnchorSearchPrior;
use crate::domain::policies::SeamResidualVerdict;
use crate::domain::pcm::interleaved_to_mono;
use crate::domain::policies::{
    fill_repeat_correlations, fill_repeat_correlations_band, fill_seam_correlations,
    fill_seam_correlations_band,
    fill_seam_correlations_with_channels, fill_splice_seam_correlations_interleaved,
    seam_score_channels, BorderSeamTemplates, FillAlignment, SeamPlacement, SeamTemplates,
    SpliceSeamContext,
};

const SCORE_TIE_EPSILON: f64 = 1e-9;
const LATE_START_PENALTY: f64 = 0.08;
/// Lever 1 belt threshold (§2.5): max |naive − FFT| seam value at the chosen winner before we treat the FFT
/// band as buggy and fall back to the naive refine for that gap. f64 FFT round-trip noise is ~1e-10; a real
/// porting bug diverges far more, so 1e-6 sits well above the noise and far below any signal.
const FFT_SEAM_DISCREPANCY_TOL: f64 = 1e-6;

/// The belt's agreement test, shared by the seam band (lever 1) and the repeat band (lever 1b(b)) so the two
/// cannot drift apart in how they treat the edges.
///
/// The `naive == band` arm is load-bearing, not a fast path: both bands legitimately produce infinities at
/// declined placements (the seam band's out-of-bounds `NEG_INFINITY`, the repeat band's empty-channel-set fold),
/// and `inf − inf` is `NaN`, which no threshold comparison can classify. Two identical infinities agree; a
/// `NaN` on either side matches neither arm and is therefore reported as a divergence — which is right, because
/// the naive path never produces one.
fn band_agrees_with_naive(naive: f64, band: f64) -> bool {
    naive == band || (naive - band).abs() <= FFT_SEAM_DISCREPANCY_TOL
}

const REPEAT_CORR_THRESHOLD: f64 = 0.55;
const REPEAT_SEAM_WEAK: f64 = 0.45;
const REPEAT_SUM_CEILING: f64 = 1.1;

/// Level F perf instrumentation (docs/dev/repair-perf.md §1): split ONE candidate evaluation into its
/// component costs. `unified_refine_*` is the pipeline's single largest exclusive cost and has no child spans,
/// so its 56.4% is a genuine leaf as far as the span tree can see — but the loop body is not uniform work. It
/// is four separable pieces, and which one dominates decides where the next optimization lever goes:
///
///  * `structure_us` — `score_pre_for_signature` + `score_post_for_signature` (the bool/energy timeline scan)
///  * `seam_us`      — the waveform seam pair: an O(1) FFT-band lookup on the lever-1 path, a full naive
///    Pearson (`waveform_seams_at_start`) on the fallback/coarse path
///  * `repeat_us`    — the naive `fill_repeat_correlations` Pearson inside the repeat penalty (timed only on
///    the `PerCandidate` path). Lever 1b(b) bands the refine-start repeat window: when
///    `use_fft_repeat_band` is on, this bucket drops toward zero in `unified_refine_start` and the FFT *build*
///    cost lands in `bracket_unified_search`'s exclusive time (same place as the seam-band build). Coarse,
///    fine-polish, and flag-off still pay the naive per-candidate rate here.
///  * `score_us`     — the rest of `unified_fit_score_with_repeat` (arithmetic, anchor prior), i.e. the
///    combining math with `repeat_us` already subtracted out.
///
/// Sub-spans are deliberately NOT used here: at ~330 µs/candidate and hundreds of candidates per call, a
/// per-candidate span enter/exit would be a measurable fraction of the thing being measured. These are plain
/// `Instant` deltas accumulated into the enclosing span's fields, ~2 clock reads per component per candidate.
///
/// Off unless `CLIP_SYNC_SPAN_TIMING` is set (the same gate the fmt subscriber uses to emit close lines), so
/// production runs pay one relaxed bool load per component and no clock reads at all.
#[derive(Default)]
struct CandidateTimers {
    structure_ns: Cell<u64>,
    seam_ns: Cell<u64>,
    repeat_ns: Cell<u64>,
    score_ns: Cell<u64>,
}

fn candidate_timing_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var_os("CLIP_SYNC_SPAN_TIMING").is_some())
}

impl CandidateTimers {
    /// Run `f`, adding its elapsed time to `bucket`. When timing is off this is `f()` and nothing else.
    fn time<T>(&self, bucket: fn(&Self) -> &Cell<u64>, f: impl FnOnce() -> T) -> T {
        if !candidate_timing_enabled() {
            return f();
        }
        let t0 = Instant::now();
        let out = f();
        let cell = bucket(self);
        cell.set(cell.get().saturating_add(t0.elapsed().as_nanos() as u64));
        out
    }

    /// Record the four buckets on `span` as microsecond fields, then reset them so the next phase's span
    /// reports its own split rather than a running total. Call before the span is dropped (a field recorded
    /// after close is lost), and only on a span that declared these fields `Empty`.
    ///
    /// `score_ns` is measured around the whole of `unified_fit_score_with_repeat`, which CONTAINS the repeat
    /// correlation, so `repeat_ns` is subtracted out here. The four reported buckets are then disjoint and
    /// sum to the instrumented part of the loop body (the remainder vs the span's `time.busy` is loop
    /// overhead, the bounds guards, and the clock reads themselves).
    fn record_on(&self, span: &tracing::Span) {
        if !candidate_timing_enabled() {
            return;
        }
        span.record("structure_us", self.structure_ns.get() / 1_000);
        span.record("seam_us", self.seam_ns.get() / 1_000);
        span.record("repeat_us", self.repeat_ns.get() / 1_000);
        span.record(
            "score_us",
            self.score_ns.get().saturating_sub(self.repeat_ns.get()) / 1_000,
        );
        self.structure_ns.set(0);
        self.seam_ns.set(0);
        self.repeat_ns.set(0);
        self.score_ns.set(0);
    }
}

/// The field set every candidate-loop span declares, so `CandidateTimers::record_on` always has somewhere to
/// write. `tracing` requires fields to be declared at span construction.
macro_rules! candidate_loop_span {
    ($name:expr) => {
        tracing::info_span!(
            $name,
            candidates = tracing::field::Empty,
            structure_us = tracing::field::Empty,
            seam_us = tracing::field::Empty,
            repeat_us = tracing::field::Empty,
            score_us = tracing::field::Empty,
        )
    };
}

/// Default hard skip floor for fit-mode waveform tiering (Phase C).
pub const DEFAULT_FILL_ABSOLUTE_FLOOR: f32 = 0.12;
/// Default band below `min_fill_correlation` for marginal patches (Phase C).
pub const DEFAULT_FILL_MARGINAL_MARGIN: f32 = 0.08;

/// Patch confidence from waveform seam scores (fit mode tiering).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FillConfidence {
    High,
    Marginal,
}

/// Hard skip floor honoring a lowered `min_fill_correlation` (e.g. gate disabled in tests).
pub fn effective_fill_absolute_floor(min_fill_correlation: f32, absolute_floor: f32) -> f32 {
    absolute_floor.min(min_fill_correlation)
}

/// True when structure is strong at anchor brackets but waveform Pearson is below production min.
pub fn anchor_trust_applies(
    structure_pre: f64,
    structure_post: f64,
    pearson_pre: f64,
    pearson_post: f64,
    strong_structure_trust: f64,
    min_fill_correlation: f32,
) -> bool {
    structure_pre >= strong_structure_trust
        && structure_post >= strong_structure_trust
        && pearson_pre.min(pearson_post) < f64::from(min_fill_correlation)
}

/// Classify waveform seam scores into patch confidence tiers (fit mode).
///
/// Returns `Err` when `min(pre, post)` is below the effective absolute floor.
pub fn classify_fill_waveform_confidence(
    pre: f64,
    post: f64,
    min_fill_correlation: f32,
    marginal_margin: f32,
    absolute_floor: f32,
) -> Result<FillConfidence, f64> {
    let min_score = pre.min(post);
    let hard_floor = f64::from(effective_fill_absolute_floor(
        min_fill_correlation,
        absolute_floor,
    ));
    if min_score < hard_floor {
        return Err(min_score);
    }
    if fit_mode_waveform_floor_passes(pre, post, min_fill_correlation) {
        return Ok(FillConfidence::High);
    }
    let marginal_floor = f64::from(min_fill_correlation - marginal_margin);
    if min_score >= marginal_floor {
        return Ok(FillConfidence::Marginal);
    }
    Err(min_score)
}

/// Why [`apply_residual_to_confidence`] rejected a candidate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ResidualGateError {
    PearsonBelowFloor(f64),
    HeadroomExceeded {
        headroom_db: f64,
        margin_db: f64,
    },
}

/// Compose Pearson waveform tiering with the residual headroom gate (fit mode).
///
/// Abstains (`NoOpinion`) when `!verdict.informative` or `verdict.beyond_lag_reach()` — returns
/// `pearson` unchanged.
pub fn apply_residual_to_confidence(
    pearson: Result<FillConfidence, f64>,
    verdict: &SeamResidualVerdict,
    margin_db: f64,
    rescue_enabled: bool,
) -> Result<FillConfidence, ResidualGateError> {
    if !verdict.informative || verdict.beyond_lag_reach() {
        return pearson.map_err(ResidualGateError::PearsonBelowFloor);
    }
    let headroom = verdict.worst_headroom_db();
    match pearson {
        Ok(_tier) if headroom > margin_db => Err(ResidualGateError::HeadroomExceeded {
            headroom_db: headroom,
            margin_db,
        }),
        Ok(tier) => Ok(tier),
        Err(_score) if headroom <= margin_db && rescue_enabled => Ok(FillConfidence::Marginal),
        Err(score) => Err(ResidualGateError::PearsonBelowFloor(score)),
    }
}

/// Penalty subtracted from ranking score per frame of A-boundary movement (Phase C).
pub const BOUNDARY_MOVE_PENALTY_PER_FRAME: f64 = 0.000_2;
/// Penalty per frame the anchor bracket center drifts from the scan-hole center (anchor seam).
pub const ANCHOR_CENTER_DRIFT_PENALTY_PER_FRAME: f64 = 0.000_15;

/// Cap grid steps per axis in joint A-boundary search (Phase C performance).
const MAX_BOUNDARY_GRID_STEPS: usize = 12;

pub fn boundary_search_step_frames(max_extend_frames: usize, step_frames: usize) -> usize {
    let step = step_frames.max(1);
    if max_extend_frames <= step.saturating_mul(MAX_BOUNDARY_GRID_STEPS) {
        return step;
    }
    (max_extend_frames / MAX_BOUNDARY_GRID_STEPS).max(step)
}

pub fn fit_candidate_ranking_score(min_waveform: f64, boundary_move_frames: usize) -> f64 {
    min_waveform - BOUNDARY_MOVE_PENALTY_PER_FRAME * boundary_move_frames as f64
}

/// Frame distance between scan-hole center and anchor-bracket center on A.
pub fn anchor_bracket_center_drift_frames(
    scan_hole: crate::domain::policies::RefinedGapFrames,
    refined: crate::domain::policies::RefinedGapFrames,
) -> usize {
    let scan_center = (scan_hole.start_frame + scan_hole.end_frame) / 2;
    let bracket_center = (refined.start_frame + refined.end_frame) / 2;
    scan_center.abs_diff(bracket_center)
}

/// Ranking penalty for anchor brackets that shift the editorial center away from the scan hole.
pub fn anchor_bracket_ranking_penalty(center_drift_frames: usize) -> f64 {
    ANCHOR_CENTER_DRIFT_PENALTY_PER_FRAME * center_drift_frames as f64
}

/// Joint-pool ranking for anchor-seam bracket candidates (waveform + boundary + center drift).
pub fn fit_anchor_candidate_ranking_score(
    min_waveform: f64,
    boundary_move_frames: usize,
    center_drift_frames: usize,
) -> f64 {
    fit_candidate_ranking_score(min_waveform, boundary_move_frames)
        - anchor_bracket_ranking_penalty(center_drift_frames)
}

/// Weights for unified fit scoring (Phase B).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UnifiedFitWeights {
    pub structure_weight: f64,
    pub waveform_weight: f64,
    /// Scales distance-from-nominal penalty inside the structure tier.
    pub nominal_bias_scale: f64,
    /// Scales the late-start penalty when `start > nominal_start`.
    pub late_start_penalty_scale: f64,
}

impl Default for UnifiedFitWeights {
    fn default() -> Self {
        Self {
            structure_weight: 0.35,
            waveform_weight: 0.65,
            nominal_bias_scale: 1.0,
            late_start_penalty_scale: 1.0,
        }
    }
}

impl UnifiedFitWeights {
    pub fn normalized(self) -> Self {
        let sum = self.structure_weight + self.waveform_weight;
        if sum <= 0.0 {
            return Self::default();
        }
        Self {
            structure_weight: self.structure_weight / sum,
            waveform_weight: self.waveform_weight / sum,
            nominal_bias_scale: self.nominal_bias_scale,
            late_start_penalty_scale: self.late_start_penalty_scale,
        }
    }
}

/// Waveform templates for unified scoring at each structure candidate.
pub struct WaveformSeamContext<'a> {
    pub templates: &'a SeamTemplates<'a>,
    pub gap_frames: usize,
    pub pre_window: usize,
    pub post_window: usize,
    pub b_total_frames: usize,
    /// Repeat-at-seam penalty (Phase D); `penalty_weight == 0` disables.
    pub repeat_window_frames: usize,
    pub repeat_penalty_weight: f64,
}

/// B-side haystack and nominal bracket for unified structure+waveform fill search.
pub struct UnifiedFillSearchInput<'a> {
    pub signature: &'a GapSignature,
    pub b_samples: &'a [f32],
    pub channels: usize,
    pub waveform: &'a WaveformSeamContext<'a>,
    pub nominal_fill_start: usize,
    pub nominal_fill_end: usize,
}

struct UnifiedSearchCtx<'a> {
    timeline: &'a StructureTimeline<'a>,
    waveform: &'a WaveformSeamContext<'a>,
    params: &'a StructureMatchParams,
    weights: UnifiedFitWeights,
    anchor_prior: Option<AnchorSearchPrior>,
    nominal_start: usize,
    nominal_end: usize,
    /// Placement-invariant seam channel selection, hoisted out of the per-candidate loop (perf lever 2).
    score_channels: &'a [usize],
    /// Lever 1 (§2.5): route the dense start-search *refine* seam correlations through the FFT band
    /// (`fill_seam_correlations_band`) instead of a per-candidate naive Pearson. Coarse stays naive; the FFT
    /// winner is verified against an exact naive re-score (belt) with a per-gap naive fallback on divergence.
    /// Off ⇒ byte-identical to pre-lever-1. Scoped to the production path only (the dump/oracle keeps naive,
    /// §2.4), so the public `match_gap_fill_unified_in_b` always passes `false`.
    use_fft_seam_search: bool,
    /// Lever 1b(b) (`TEMP-repeat-band-plan.md`): route the dense start-search *refine* repeat-window
    /// correlations through [`fill_repeat_correlations_band`] instead of a per-candidate naive Pearson. Same
    /// scoping rules as `use_fft_seam_search` (production path only, coarse and fine-polish stay naive). The two
    /// bands decline independently. On by default in production (`RepairConfig.fft_repeat_band`); dump/oracle
    /// always passes `false`.
    use_fft_repeat_band: bool,
}

fn fill_bracket_placement(
    start: usize,
    end: usize,
    nominal_start: usize,
    nominal_end: usize,
) -> FillBracketPlacement {
    FillBracketPlacement {
        start,
        end,
        nominal_start,
        nominal_end,
    }
}

/// Combined objective for one B fill bracket candidate.
pub fn unified_fit_score(
    structure_pre: f64,
    structure_post: f64,
    waveform_min: f64,
    placement: FillBracketPlacement,
    params: &StructureMatchParams,
    weights: UnifiedFitWeights,
) -> f64 {
    if !structure_pre.is_finite() || !structure_post.is_finite() {
        return f64::NEG_INFINITY;
    }
    let weights = weights.normalized();
    if weights.waveform_weight > 0.0 && !waveform_min.is_finite() {
        return f64::NEG_INFINITY;
    }
    let structure_combined = combined_structure_score(
        structure_pre,
        structure_post,
        placement,
        params,
        weights.nominal_bias_scale,
    );
    let mut score =
        weights.structure_weight * structure_combined + weights.waveform_weight * waveform_min;
    if placement.start > placement.nominal_start {
        let late_frac = (placement.start - placement.nominal_start) as f64
            / params.gap_frames.max(1) as f64;
        score -= LATE_START_PENALTY * late_frac * weights.late_start_penalty_scale;
    }
    score
}

/// The repeat-window correlation pair at one placement — the **expensive** half of the repeat penalty, and the
/// only half lever 1b(b) accelerates. Kept as a named type (not a bare `(f64, f64)`) so
/// [`RepeatPenaltySource::Banded`] cannot be confused with [`RepeatPenaltySource::Fixed`], which carries an
/// already-*combined* penalty. See `TEMP-repeat-band-plan.md` §3.
#[derive(Clone, Copy)]
struct RepeatCorrelations {
    pre: f64,
    post: f64,
}

/// Lever 1b(b): the expensive half, split out so the banded path can supply the same pair from an FFT band
/// lookup instead. Callers must honour the `repeat_penalty_weight <= 0.0 || repeat_window_frames == 0`
/// early-out themselves — it is **semantic**, not just a perf guard: with `repeat_window_frames == 0`,
/// `effective_repeat_window_frames` floors the window to 1 and would score a 1-frame Pearson rather than the
/// 0.0 penalty the disabled path is supposed to produce.
fn repeat_correlations_at_placement(
    ctx: &WaveformSeamContext<'_>,
    start: usize,
    timers: &CandidateTimers,
) -> RepeatCorrelations {
    let placement = SeamPlacement {
        start,
        gap_frames: ctx.gap_frames,
        pre_window: ctx.pre_window,
        post_window: ctx.post_window,
    };
    // Level F `repeat_us`: the un-banded per-candidate Pearson, timed on its own.
    let (pre, post) = timers.time(
        |t| &t.repeat_ns,
        || fill_repeat_correlations(ctx.templates, placement, ctx.repeat_window_frames),
    );
    RepeatCorrelations { pre, post }
}

/// The **cheap** half: the branch logic combining the repeat pair with the seam pair. Unchanged from the
/// original `repeat_penalty_at_placement` body. It is `wave_min`-dependent and therefore genuinely
/// per-candidate — the banded path must still run it, which is why `Banded` carries correlations rather than a
/// finished penalty.
///
/// Lever 1b(a) (TEMP-production-repair-perf-plan.md §2.5): the pre/post **seam** correlations are passed in —
/// the caller already computed them (`wave_min = pre_seam.min(post_seam)`) via `waveform_seams_at_start`
/// (naive) or the FFT band, so recomputing them here with a fresh `fill_seam_correlations` (which also re-ran
/// the hoisted `seam_score_channel_indices` scan per candidate — undoing lever 2) is pure waste. They are
/// byte-identical to the old `fill_seam_correlations` on the naive path (same
/// `fill_seam_correlations_with_channels` under the hood, same channel selection); on the FFT path they are the
/// band's ε-approximation, consistent with the `wave_min` the score already used. The repeat-window
/// correlation is a *different* window and is banded separately by lever 1b(b).
fn repeat_penalty_from_correlations(
    corr: RepeatCorrelations,
    pre_seam: f64,
    post_seam: f64,
) -> f64 {
    let wave_min = pre_seam.min(post_seam);
    let RepeatCorrelations {
        pre: repeat_pre,
        post: repeat_post,
    } = corr;
    let repeat_max = repeat_pre.max(repeat_post);
    let repeat_sum = repeat_pre + repeat_post;
    let asymmetric_post_dup = repeat_post > REPEAT_CORR_THRESHOLD
        && post_seam > REPEAT_CORR_THRESHOLD
        && post_seam - pre_seam > 0.35;
    if wave_min < REPEAT_SEAM_WEAK && repeat_max > REPEAT_CORR_THRESHOLD {
        repeat_max
    } else if asymmetric_post_dup {
        repeat_post
    } else if repeat_sum > REPEAT_SUM_CEILING && wave_min < REPEAT_SEAM_WEAK {
        repeat_sum * 0.5
    } else {
        0.0
    }
}

/// The un-banded composition of the two halves: the original `repeat_penalty_at_placement`, preserved verbatim
/// in behaviour. Holds the `repeat_penalty_weight <= 0.0 || repeat_window_frames == 0` early-out.
///
/// When [`build_repeat_band`] returns `None`, the refine loop passes `precomputed_repeat = None` and lands
/// here via [`RepeatPenaltySource::PerCandidate`] — that is the intended fallback for *both* `None` meanings
/// (zero-weight/zero-window early-out, where this fn returns 0.0 immediately, and a non-uniform band-edge
/// decline, where it pays the naive Pearson). The banded path applies the same early-out at *build* time so
/// a zero-weight bracket never constructs an FFT it would discard.
fn repeat_penalty_at_placement(
    ctx: &WaveformSeamContext<'_>,
    start: usize,
    pre_seam: f64,
    post_seam: f64,
    timers: &CandidateTimers,
) -> f64 {
    if ctx.repeat_penalty_weight <= 0.0 || ctx.repeat_window_frames == 0 {
        return 0.0;
    }
    let corr = repeat_correlations_at_placement(ctx, start, timers);
    repeat_penalty_from_correlations(corr, pre_seam, post_seam)
}

struct UnifiedFitCandidate {
    structure_pre: f64,
    structure_post: f64,
    /// The two seam correlations (`wave_min = wave_pre.min(wave_post)` is the waveform score term). Carried
    /// separately so `repeat_penalty_at_placement` reuses them instead of recomputing (lever 1b(a)).
    wave_pre: f64,
    wave_post: f64,
    placement: FillBracketPlacement,
}

/// Lever 1b(c) (docs/dev/repair-perf.md §1a): how a candidate loop supplies the repeat penalty.
///
/// `PerCandidate` is the general case — the penalty moves with the candidate, so it is recomputed.
/// `Fixed` is for a loop that has PROVEN the penalty loop-invariant and computed it once above the loop;
/// `unified_fit_score_with_repeat` then substitutes the value instead of recomputing an identical one.
/// The substitution is byte-identical: it replaces `repeat_penalty_at_placement(..)` with the f64 that call
/// would have returned, under the same `wf > 0.0 && score.is_finite()` guard and the same arithmetic.
///
/// `Banded` (lever 1b(b)) carries the **correlations**, not a penalty: the start-dependent repeat window is not
/// loop-invariant, so only the expensive Pearson pair can be precomputed (by
/// [`fill_repeat_correlations_band`]). The `wave_min` / `asymmetric_post_dup` branch still runs per candidate
/// via [`repeat_penalty_from_correlations`] — which is why this variant must not be conflated with `Fixed`.
#[derive(Clone, Copy)]
enum RepeatPenaltySource {
    PerCandidate,
    Banded(RepeatCorrelations),
    Fixed(f64),
}

fn unified_fit_score_with_repeat(
    candidate: UnifiedFitCandidate,
    params: &StructureMatchParams,
    weights: UnifiedFitWeights,
    waveform: &WaveformSeamContext<'_>,
    anchor_prior: Option<AnchorSearchPrior>,
    repeat_source: RepeatPenaltySource,
    timers: &CandidateTimers,
) -> f64 {
    let mut score = unified_fit_score(
        candidate.structure_pre,
        candidate.structure_post,
        candidate.wave_pre.min(candidate.wave_post),
        candidate.placement,
        params,
        weights,
    );
    if let Some(prior) = anchor_prior {
        score -= prior.penalty_at_start(candidate.placement.start);
    }
    let wf = weights.normalized().waveform_weight;
    if wf > 0.0 && score.is_finite() {
        let penalty = match repeat_source {
            RepeatPenaltySource::Fixed(penalty) => penalty,
            RepeatPenaltySource::Banded(corr) => repeat_penalty_from_correlations(
                corr,
                candidate.wave_pre,
                candidate.wave_post,
            ),
            RepeatPenaltySource::PerCandidate => repeat_penalty_at_placement(
                waveform,
                candidate.placement.start,
                candidate.wave_pre,
                candidate.wave_post,
                timers,
            ),
        };
        score -= waveform.repeat_penalty_weight * wf * penalty;
    }
    score
}

pub(crate) fn waveform_min_at_start(
    ctx: &WaveformSeamContext<'_>,
    start: usize,
    score_channels: &[usize],
) -> f64 {
    if !placement_in_bounds(
        start,
        ctx.gap_frames,
        ctx.pre_window,
        ctx.post_window,
        ctx.b_total_frames,
    ) {
        return f64::NEG_INFINITY;
    }
    let (pre, post) = fill_seam_correlations_with_channels(
        ctx.templates,
        SeamPlacement {
            start,
            gap_frames: ctx.gap_frames,
            pre_window: ctx.pre_window,
            post_window: ctx.post_window,
        },
        score_channels,
    );
    pre.min(post)
}

/// Lever 1 (§2.5): precompute the `(pre_seam, post_seam)` correlations that `waveform_seams_at_start` returns
/// (the score's waveform term is `pre.min(post)`) for every start in `[lo, hi]` via one FFT band pass per channel
/// ([`fill_seam_correlations_band`]), instead of a naive per-candidate Pearson. Returns `None` when the band
/// evaluator declines (a non-uniform band-edge case where the naive channel set would vary per start) — the
/// caller then scores that range the naive way. The `placement_in_bounds` NEG_INFINITY gate that
/// `waveform_seams_at_start` applies is intentionally NOT applied here; the caller re-applies it at lookup so
/// entries mirror the naive call exactly. Threaded pre/post also feed `repeat_penalty_at_placement` (lever 1b(a)).
fn build_wave_seam_band(
    ctx: &WaveformSeamContext<'_>,
    score_channels: &[usize],
    lo: usize,
    hi: usize,
) -> Option<Vec<(f64, f64)>> {
    fill_seam_correlations_band(
        ctx.templates,
        ctx.gap_frames,
        ctx.pre_window,
        ctx.post_window,
        score_channels,
        lo,
        hi,
    )
}

/// Lever 1b(b) (`TEMP-repeat-band-plan.md` §3): the repeat-window analogue of [`build_wave_seam_band`].
/// Precomputes the `(repeat_pre, repeat_post)` pair that [`repeat_correlations_at_placement`] returns for every
/// start in `[lo, hi]` via one FFT band pass per side per channel, instead of a naive per-candidate Pearson.
///
/// Two distinct `None` meanings, both safe for the caller to treat identically as "score this range naively":
/// the zero-weight/zero-window early-out (where the naive path also yields a 0.0 penalty), and the band
/// evaluator declining a non-uniform band edge. This band is **independent** of the seam band — either may be
/// `None` without forcing the other off the FFT path.
fn build_repeat_band(
    ctx: &WaveformSeamContext<'_>,
    lo: usize,
    hi: usize,
) -> Option<Vec<(f64, f64)>> {
    // Semantic, not just perf: `effective_repeat_window_frames` floors a 0 window to 1, so a band built with
    // `repeat_window_frames == 0` would score a 1-frame Pearson where the disabled path yields 0.0.
    if ctx.repeat_penalty_weight <= 0.0 || ctx.repeat_window_frames == 0 {
        return None;
    }
    fill_repeat_correlations_band(
        ctx.templates,
        ctx.gap_frames,
        ctx.pre_window,
        ctx.post_window,
        ctx.repeat_window_frames,
        lo,
        hi,
    )
}

/// Result of unified structure+waveform search (Phase B).
#[derive(Debug, Clone, PartialEq)]
pub struct UnifiedFillMatch {
    pub alignment: FillAlignment,
    pub structure_pre: f64,
    pub structure_post: f64,
}

/// Locate a B fill bracket by jointly ranking structure and waveform seam scores.
pub fn match_gap_fill_unified_in_b(
    input: &UnifiedFillSearchInput<'_>,
    params: &StructureMatchParams,
    weights: UnifiedFitWeights,
) -> Option<UnifiedFillMatch> {
    let channels = input.channels.max(1);
    let total_frames = input.b_samples.len() / channels;
    let bool_timeline;
    let energy_timeline;
    let structure_timeline = match input.signature {
        GapSignature::Bool(_) => {
            bool_timeline = gap_structure::ActivityTimeline::build(
                input.b_samples,
                channels,
                total_frames,
                params.bin_frames,
                params.silence_peak_fraction,
                params.absolute_silence_rms,
            );
            StructureTimeline::Bool(&bool_timeline)
        }
        GapSignature::Energy(_) => {
            energy_timeline = crate::domain::gap_energy::EnergyTimeline::build(
                input.b_samples,
                channels,
                total_frames,
                params.bin_frames,
                params.silence_peak_fraction,
                params.absolute_silence_rms,
            );
            StructureTimeline::Energy(&energy_timeline)
        }
    };
    // Public entry (dump/fixtures) always keeps the naive seam AND repeat correlations so the committed
    // corpus/golden stay byte-exact (§2.4). Only the production caller opts into either band, via `_with_timeline`.
    match_gap_fill_unified_in_b_with_timeline(
        input,
        params,
        weights,
        &structure_timeline,
        None,
        false,
        false,
    )
}

/// Like [`match_gap_fill_unified_in_b`] but reuses a pre-built structure timeline (joint grid perf).
/// `use_fft_seam_search` routes the dense start-search refine through the FFT seam band (lever 1, §2.5);
/// `false` is byte-identical to the pre-lever-1 naive search. `use_fft_repeat_band` does the same for the
/// repeat-window correlations (lever 1b(b)); the two are independent switches.
pub(crate) fn match_gap_fill_unified_in_b_with_timeline(
    input: &UnifiedFillSearchInput<'_>,
    params: &StructureMatchParams,
    weights: UnifiedFitWeights,
    structure_timeline: &StructureTimeline<'_>,
    anchor_prior: Option<AnchorSearchPrior>,
    use_fft_seam_search: bool,
    use_fft_repeat_band: bool,
) -> Option<UnifiedFillMatch> {
    if input.signature.is_empty() || params.gap_frames == 0 || params.bin_frames == 0 {
        return None;
    }

    let channels = input.channels.max(1);
    let total_frames = input.b_samples.len() / channels;
    let pre_span = match input.signature {
        GapSignature::Bool(sig) => sig.pre_bins.len() * params.bin_frames,
        GapSignature::Energy(sig) => sig.pre_energy.len() * params.bin_frames,
    };
    let post_span = match input.signature {
        GapSignature::Bool(sig) => sig.post_bins.len() * params.bin_frames,
        GapSignature::Energy(sig) => sig.post_energy.len() * params.bin_frames,
    };

    if pre_span == 0 || post_span == 0 {
        return None;
    }

    // Perf lever 2 (TEMP-production-repair-perf-plan.md §2.3): the seam channel selection depends only on
    // the A-side templates, not the placement, so compute it once here instead of per candidate inside
    // `fill_seam_correlations`.
    let score_channels = seam_score_channels(input.waveform.templates);
    let search = UnifiedSearchCtx {
        timeline: structure_timeline,
        waveform: input.waveform,
        params,
        weights,
        anchor_prior,
        nominal_start: input.nominal_fill_start,
        nominal_end: input.nominal_fill_end,
        score_channels: &score_channels,
        use_fft_seam_search,
        use_fft_repeat_band,
    };

    let (mut best_start, _) = unified_search_best_fill_start(
        input.signature,
        &search,
        pre_span,
        post_span,
        total_frames,
    )?;

    let (mut best_end, _) =
        unified_search_best_fill_end(input.signature, &search, best_start, post_span, total_frames)?;

    let matched_fill_len = best_end.saturating_sub(best_start);
    let polished_start =
        unified_fine_polish_start(input.signature, &search, best_start, best_end);
    best_start = polished_start;
    best_end = best_start + matched_fill_len;

    let (wave_pre, wave_post) =
        waveform_seams_at_start(input.waveform, best_start, &score_channels);

    let fill_frames = best_end.saturating_sub(best_start);
    if fill_frames == 0 {
        return None;
    }

    let mut alignment = FillAlignment {
        start_frame: best_start,
        fill_frames,
        pre_correlation: wave_pre,
        post_correlation: wave_post,
    };
    snap_fill_to_gap(&mut alignment, input.signature, structure_timeline, params);

    let structure_pre = score_pre_for_signature(
        input.signature,
        structure_timeline,
        alignment.start_frame,
        params,
    );
    let structure_post = score_post_for_signature(
        input.signature,
        structure_timeline,
        alignment.start_frame + alignment.fill_frames,
        params,
    );

    Some(UnifiedFillMatch {
        alignment,
        structure_pre,
        structure_post,
    })
}

fn waveform_seams_at_start(
    ctx: &WaveformSeamContext<'_>,
    start: usize,
    score_channels: &[usize],
) -> (f64, f64) {
    if !placement_in_bounds(
        start,
        ctx.gap_frames,
        ctx.pre_window,
        ctx.post_window,
        ctx.b_total_frames,
    ) {
        return (f64::NEG_INFINITY, f64::NEG_INFINITY);
    }
    fill_seam_correlations_with_channels(
        ctx.templates,
        SeamPlacement {
            start,
            gap_frames: ctx.gap_frames,
            pre_window: ctx.pre_window,
            post_window: ctx.post_window,
        },
        score_channels,
    )
}

fn unified_search_best_fill_start(
    signature: &GapSignature,
    ctx: &UnifiedSearchCtx<'_>,
    pre_span: usize,
    post_span: usize,
    total_frames: usize,
) -> Option<(usize, f64)> {
    let UnifiedSearchCtx {
        timeline,
        waveform,
        params,
        weights,
        anchor_prior,
        nominal_start,
        nominal_end,
        score_channels,
        use_fft_seam_search,
        use_fft_repeat_band,
    } = *ctx;
    let search_min = nominal_start.saturating_sub(params.search_radius_frames);
    let search_max = (nominal_start + params.search_radius_frames).min(total_frames);
    let span = search_max.saturating_sub(search_min);
    let coarse_step = search_coarse_step(params.bin_frames, span);

    let mut best_start = nominal_start;
    let mut best_score = f64::NEG_INFINITY;

    // Level F: one accumulator per PHASE, so the coarse and refine spans report their own component splits
    // rather than a running total. `consider` is shared, so the active accumulator is swapped between phases.
    let timers = CandidateTimers::default();

    // `consider` takes the `(pre_seam, post_seam)` correlations as `precomputed_wave`: `None` ⇒ compute them
    // naively in-line (the pre-lever-1 behaviour, exact), `Some((pre, post))` ⇒ use the FFT-band values the
    // refine loop looked up. Passing `None` everywhere is byte-identical to the old closure, so the flag-off path
    // is unchanged. The pair (not just the min) also feeds `repeat_penalty_at_placement` (lever 1b(a)).
    //
    // `precomputed_repeat` is the lever 1b(b) analogue for the repeat-window pair, and is INDEPENDENT of
    // `precomputed_wave`: either band may be absent without forcing the other onto the naive path.
    let consider = |start: usize,
                    precomputed_wave: Option<(f64, f64)>,
                    precomputed_repeat: Option<(f64, f64)>,
                    best_start: &mut usize,
                    best_score: &mut f64| {
        if start < pre_span || start > search_max {
            return;
        }
        let candidate_end = (start + params.gap_frames).min(total_frames);
        if candidate_end + post_span > total_frames {
            return;
        }
        // Level F `structure_us`: both timeline scans together — they are the same kind of work and always
        // run as a pair, so splitting pre from post would cost a clock read to say nothing new.
        let (pre_score, post_score) = timers.time(
            |t| &t.structure_ns,
            || {
                (
                    score_pre_for_signature(signature, timeline, start, params),
                    score_post_for_signature(signature, timeline, candidate_end, params),
                )
            },
        );
        // Level F `seam_us`: an O(1) band lookup when the caller supplied `precomputed_wave` (lever 1's FFT
        // path), a full naive Pearson otherwise. Timing both under one name is what makes the FFT path's
        // saving readable as a drop in this bucket.
        let (wave_pre, wave_post) = timers.time(
            |t| &t.seam_ns,
            || {
                precomputed_wave
                    .unwrap_or_else(|| waveform_seams_at_start(waveform, start, score_channels))
            },
        );
        // Level F `score_us`: the combining math. `repeat_us` is accumulated separately inside, so this
        // bucket is the score arithmetic with the repeat correlation already excluded.
        let score = timers.time(
            |t| &t.score_ns,
            || {
                unified_fit_score_with_repeat(
                    UnifiedFitCandidate {
                        structure_pre: pre_score,
                        structure_post: post_score,
                        wave_pre,
                        wave_post,
                        placement: fill_bracket_placement(
                            start,
                            candidate_end,
                            nominal_start,
                            nominal_end,
                        ),
                    },
                    params,
                    weights,
                    waveform,
                    anchor_prior,
                    // `start` moves here, so the repeat window moves with it — no hoist available (unlike the
                    // end search's lever 1b(c)). Lever 1b(b) instead precomputes the window's *correlations*
                    // for the whole refine range as an FFT band; the combining branch still runs per candidate.
                    match precomputed_repeat {
                        Some((pre, post)) => {
                            RepeatPenaltySource::Banded(RepeatCorrelations { pre, post })
                        }
                        None => RepeatPenaltySource::PerCandidate,
                    },
                    &timers,
                )
            },
        );
            let better = score > *best_score + SCORE_TIE_EPSILON
                || (score >= *best_score - SCORE_TIE_EPSILON
                    && prefer_start(start, *best_start, nominal_start));
            if better {
                *best_score = score;
                *best_start = start;
            }
        };

    // Perf instrumentation (Level E, TEMP-production-repair-perf-plan.md §2.3/§2.4): split each unified search
    // into the SPARSE coarse pass vs the DENSE integer refine — the distribution that decides prefix-sum vs FFT
    // for lever 1. `candidates` records attempts per phase (per-candidate cost = busy / candidates), and the
    // Level F `*_us` fields split one candidate into its components (see `CandidateTimers`). Emits only under
    // CLIP_SYNC_SPAN_TIMING (Level A); nests under `bracket_unified_search`.
    //
    // The `_start` / `_end` suffix matters (docs/dev/repair-perf.md §1): the start and end searches run
    // structurally DIFFERENT loop bodies — the end search hoists the pre structure score and both seam values
    // out of its loop (see `unified_search_best_fill_end`), so its per-candidate cost is far lower. Sharing one
    // span name made the harness's by-name roll-up average two populations, and the resulting per-candidate
    // figure described neither.
    //
    // Lever 1 (§2.5): the coarse pass stays NAIVE — it is sparse (4.8%, Level E) and, crucially, keeping it
    // naive means the coarse winner that anchors the refine window is bit-identical to today, so the FFT can
    // only ever move a placement *within* the ±coarse_step refine band, never relocate it grossly.
    let coarse_span = candidate_loop_span!("unified_coarse_start");
    let mut coarse_n = 0u64;
    {
        let _e = coarse_span.enter();
        if nominal_start >= pre_span {
            consider(nominal_start, None, None, &mut best_start, &mut best_score);
            coarse_n += 1;
        }
        let mut start = search_min;
        while start <= search_max {
            consider(start, None, None, &mut best_start, &mut best_score);
            coarse_n += 1;
            start = start.saturating_add(coarse_step);
        }
    }
    coarse_span.record("candidates", coarse_n);
    timers.record_on(&coarse_span);

    if !best_score.is_finite() {
        return None;
    }

    // Snapshot the coarse winner so the FFT belt can redo the refine naively from the same seed on divergence.
    let coarse_best_start = best_start;
    let coarse_best_score = best_score;

    let refine_min = best_start.saturating_sub(coarse_step).max(search_min);
    let refine_max = (best_start + coarse_step).min(search_max);

    // Lever 1 (§2.5): precompute the dense refine band's `(pre_seam, post_seam)` in one FFT pass per channel.
    // `None` ⇒ the band evaluator declined (non-uniform band edge) or the flag is off ⇒ fall back to naive.
    let wave_band = if use_fft_seam_search {
        build_wave_seam_band(waveform, score_channels, refine_min, refine_max)
    } else {
        None
    };
    // Lever 1b(b) (`TEMP-repeat-band-plan.md` §3): the same treatment for the repeat window, which the end
    // search hoists (1b(c)) but the start search cannot — the window moves with `start`. Independent of
    // `wave_band` in both directions: no shared decline, no shared fallback.
    let repeat_band = if use_fft_repeat_band {
        build_repeat_band(waveform, refine_min, refine_max)
    } else {
        None
    };
    let refine_span = candidate_loop_span!("unified_refine_start");
    let mut refine_n = 0u64;
    {
        let _e = refine_span.enter();
        for start in refine_min..=refine_max {
            // Band lookup mirrors `waveform_seams_at_start` exactly: apply its `placement_in_bounds`
            // NEG_INFINITY gate on top of the band pair (the band itself does not gate). Out of band ⇒ naive.
            let precomputed = wave_band.as_ref().map(|band| {
                if placement_in_bounds(
                    start,
                    waveform.gap_frames,
                    waveform.pre_window,
                    waveform.post_window,
                    waveform.b_total_frames,
                ) {
                    band[start - refine_min]
                } else {
                    (f64::NEG_INFINITY, f64::NEG_INFINITY)
                }
            });
            // No `placement_in_bounds` gate here, unlike the seam band: the naive counterpart
            // (`repeat_correlations_at_placement`) does not apply one either — it calls
            // `fill_repeat_correlations` directly, which does its own per-side length checks. The band mirrors
            // that, so a plain index is the exact analogue.
            let precomputed_repeat = repeat_band.as_ref().map(|band| band[start - refine_min]);
            consider(
                start,
                precomputed,
                precomputed_repeat,
                &mut best_start,
                &mut best_score,
            );
            refine_n += 1;
        }
    }
    refine_span.record("candidates", refine_n);
    timers.record_on(&refine_span);

    // Exact re-score belt + runtime monitor (§2.5): the FFT band only *finds where to look*; verify the chosen
    // winner's band value against an exact naive re-score. A divergence beyond FFT noise (≫ 1e-10) can only be
    // an FFT porting bug (lag convention / edge mask / normalization), so degrade THIS gap to the exact naive
    // refine — correct, unaccelerated — and warn, rather than shipping a bad placement. (The winner's *reported*
    // seam/structure scores are already re-derived naively downstream in
    // `match_gap_fill_unified_in_b_with_timeline`, so no separate reported-value re-score is needed here.)
    //
    // Lever 1b(b) (`TEMP-repeat-band-plan.md` §4) extends the belt to the repeat band. Each band is checked
    // independently — a divergence in either one condemns the winner, because both feed the same score — but
    // they share ONE fallback: the naive re-refine below passes `None` for both bands, so it is exact for
    // whichever diverged and for the other one too. Running it once is what keeps a double divergence from
    // paying for two full re-refines.
    let mut diverged = false;
    if wave_band.is_some() {
        let naive_wm = waveform_min_at_start(waveform, best_start, score_channels);
        let band_wm = wave_band.as_ref().map_or(f64::NEG_INFINITY, |band| {
            if placement_in_bounds(
                best_start,
                waveform.gap_frames,
                waveform.pre_window,
                waveform.post_window,
                waveform.b_total_frames,
            ) {
                let (pre, post) = band[best_start - refine_min];
                pre.min(post)
            } else {
                f64::NEG_INFINITY
            }
        });
        if !band_agrees_with_naive(naive_wm, band_wm) {
            tracing::warn!(
                best_start,
                naive_wm,
                band_wm,
                delta = (naive_wm - band_wm).abs(),
                "fft seam band diverged from naive at the winner — falling back to naive refine for this gap"
            );
            diverged = true;
        }
    }
    if let Some(band) = repeat_band.as_ref() {
        // Compared PRE-BRANCH — the correlation pair itself, not the penalty
        // `repeat_penalty_from_correlations` derives from it. The branch is a step function around
        // `REPEAT_CORR_THRESHOLD` / `REPEAT_SEAM_WEAK`, so a penalty-level comparison would read as agreement
        // for any FFT error that stays inside one branch and as a large disagreement for a 1e-12 error that
        // straddles a threshold. Neither tells you about the FFT. The correlations do.
        let naive = repeat_correlations_at_placement(waveform, best_start, &timers);
        let (band_pre, band_post) = band[best_start - refine_min];
        if !band_agrees_with_naive(naive.pre, band_pre)
            || !band_agrees_with_naive(naive.post, band_post)
        {
            tracing::warn!(
                best_start,
                naive_pre = naive.pre,
                naive_post = naive.post,
                band_pre,
                band_post,
                delta_pre = (naive.pre - band_pre).abs(),
                delta_post = (naive.post - band_post).abs(),
                "fft repeat band diverged from naive at the winner — falling back to naive refine for this gap"
            );
            diverged = true;
        }
    }
    if diverged {
        // Level F: this fallback re-refine's component times land in `timers` with no span left to record
        // them on, so they are discarded rather than misattributed to the next phase. That is deliberate —
        // the belt plus this fallback sit inside `bracket_unified_search`'s own time, measured at 0.3% of
        // root, so the loss is bounded and far below what the buckets are being used to decide.
        best_start = coarse_best_start;
        best_score = coarse_best_score;
        for start in refine_min..=refine_max {
            consider(start, None, None, &mut best_start, &mut best_score);
        }
    }

    Some((best_start, best_score))
}

fn unified_search_best_fill_end(
    signature: &GapSignature,
    ctx: &UnifiedSearchCtx<'_>,
    fill_start: usize,
    post_span: usize,
    total_frames: usize,
) -> Option<(usize, f64)> {
    let UnifiedSearchCtx {
        timeline,
        waveform,
        params,
        weights,
        nominal_end,
        score_channels,
        ..
    } = *ctx;
    let end_min = fill_start
        .saturating_add(params.gap_frames)
        .saturating_sub(params.fill_length_slack_frames);
    let end_max = (fill_start + params.gap_frames + params.fill_length_slack_frames)
        .min(total_frames);
    if end_min > end_max || end_min + post_span > total_frames {
        return None;
    }

    let span = end_max.saturating_sub(end_min);
    let coarse_step = search_coarse_step(params.bin_frames, span);

    let mut best_end = nominal_end.clamp(end_min, end_max);
    let mut best_score = f64::NEG_INFINITY;

    // Lever 1 (byte-identical cross-candidate reuse, TEMP-production-repair-perf-plan.md §2.5): in the END
    // search the pre seam and the waveform seams are anchored at the FIXED `fill_start`, so they are constant
    // across every `end` candidate. Compute them once here instead of per candidate — removes the per-channel
    // Pearson (`waveform_seams_at_start`) *and* the repeat penalty's seam reuse per end candidate.
    let const_pre_score = score_pre_for_signature(signature, timeline, fill_start, params);
    let (const_wave_pre, const_wave_post) =
        waveform_seams_at_start(waveform, fill_start, score_channels);

    // Level F (see `CandidateTimers`): the end search's `seam_us` is structurally ZERO — both seam values are
    // hoisted above, so no candidate pays for them. Its `structure_us` covers the post scan only. That is
    // exactly why the span names carry `_start` / `_end`: these buckets are not comparable across the two.
    let timers = CandidateTimers::default();

    // Lever 1b(c) (docs/dev/repair-perf.md §1a): the repeat penalty is likewise CONSTANT across this loop, so
    // it is hoisted too. An earlier note here claimed `end` "moves its window" and left it per-candidate; that
    // is wrong. `repeat_penalty_at_placement` reads only `waveform` (its `gap_frames` / `pre_window` /
    // `post_window` come from the context, NOT from the candidate), the placement's `start` — which here is
    // `fill_start` for every candidate, since `fill_bracket_placement(fill_start, end, ..)` puts `fill_start`
    // in `.start` — and the two seam values, already hoisted above. `end` never reaches it.
    //
    // The 2026-07-25 sweep measured what that cost: `repeat_us` was 99.8% of `unified_refine_end` and
    // `unified_coarse_end`, ~34% of total repair time, recomputing one identical f64 ~4.2M times.
    //
    // Timed on a throwaway accumulator so this one-off does not land in the first phase's `repeat_us`. Like
    // `const_pre_score` and the seam pair above, its cost now sits in `bracket_unified_search`'s own time.
    let const_repeat_penalty = repeat_penalty_at_placement(
        waveform,
        fill_start,
        const_wave_pre,
        const_wave_post,
        &CandidateTimers::default(),
    );
    let consider = |end: usize, best_end: &mut usize, best_score: &mut f64| {
        if end < end_min || end > end_max || end + post_span > total_frames {
            return;
        }
        let fill_len = end.saturating_sub(fill_start);
        let min_fill = (params.gap_frames / 4)
            .max(params.bin_frames)
            .max(1);
        let max_fill = params.gap_frames.saturating_add(params.fill_length_slack_frames);
        if fill_len < min_fill || fill_len > max_fill {
            return;
        }
        let post_score = timers.time(
            |t| &t.structure_ns,
            || score_post_for_signature(signature, timeline, end, params),
        );
        let score = timers.time(
            |t| &t.score_ns,
            || {
                unified_fit_score_with_repeat(
                    UnifiedFitCandidate {
                        structure_pre: const_pre_score,
                        structure_post: post_score,
                        wave_pre: const_wave_pre,
                        wave_post: const_wave_post,
                        placement: fill_bracket_placement(fill_start, end, fill_start, nominal_end),
                    },
                    params,
                    weights,
                    waveform,
                    None,
                    RepeatPenaltySource::Fixed(const_repeat_penalty),
                    &timers,
                )
            },
        );
        let better = score > *best_score + SCORE_TIE_EPSILON
            || (score >= *best_score - SCORE_TIE_EPSILON
                && prefer_end(end, *best_end, nominal_end));
        if better {
            *best_score = score;
            *best_end = end;
        }
    };

    // Level E (see start search): coarse (sparse) vs refine (dense) split. Names are suffixed `_end` so the
    // by-name roll-up keeps this loop's much cheaper candidates separate from the start search's.
    let coarse_span = candidate_loop_span!("unified_coarse_end");
    let mut coarse_n = 0u64;
    {
        let _e = coarse_span.enter();
        consider(best_end, &mut best_end, &mut best_score);
        coarse_n += 1;
        let mut end = end_min;
        while end <= end_max {
            consider(end, &mut best_end, &mut best_score);
            coarse_n += 1;
            end = end.saturating_add(coarse_step);
        }
    }
    coarse_span.record("candidates", coarse_n);
    timers.record_on(&coarse_span);

    if !best_score.is_finite() {
        return None;
    }

    let refine_min = best_end.saturating_sub(coarse_step).max(end_min);
    let refine_max = (best_end + coarse_step).min(end_max);
    let refine_span = candidate_loop_span!("unified_refine_end");
    let mut refine_n = 0u64;
    {
        let _e = refine_span.enter();
        for end in refine_min..=refine_max {
            consider(end, &mut best_end, &mut best_score);
            refine_n += 1;
        }
    }
    refine_span.record("candidates", refine_n);
    timers.record_on(&refine_span);

    Some((best_end, best_score))
}

fn unified_fine_polish_start(
    signature: &GapSignature,
    ctx: &UnifiedSearchCtx<'_>,
    start: usize,
    end: usize,
) -> usize {
    let UnifiedSearchCtx {
        timeline,
        waveform,
        params,
        weights,
        anchor_prior,
        nominal_start,
        nominal_end,
        score_channels,
        // Fine polish stays naive this PR (Level E: 2.5% of the search); FFT covers the refine. See §2.5 and
        // `TEMP-repeat-band-plan.md` §3 — both bands are scoped to the refine window for the same reason.
        use_fft_seam_search: _,
        use_fft_repeat_band: _,
    } = *ctx;
    if params.max_fine_adjustment_frames == 0 {
        return start;
    }

    let fill_len = end.saturating_sub(start).max(1);
    let mut best_start = start;
    let mut best_score = f64::NEG_INFINITY;

    // Level E: the fine polish is a small DENSE integer window (±max_fine_adjustment_frames). Its loop body is
    // the same four components as the start refine, and it is fully naive (no FFT band), so its Level F split
    // doubles as the un-accelerated control against `unified_refine_start`'s banded one.
    let polish_span = candidate_loop_span!("unified_fine_polish");
    let timers = CandidateTimers::default();
    let mut polish_n = 0u64;
    let polish_guard = polish_span.enter();
    for delta in -(params.max_fine_adjustment_frames as i64)
        ..=(params.max_fine_adjustment_frames as i64)
    {
        polish_n += 1;
        let candidate = start as i64 + delta;
        if candidate < 0 {
            continue;
        }
        let candidate = candidate as usize;
        let candidate_end = candidate + fill_len;
        let (pre_score, post_score) = timers.time(
            |t| &t.structure_ns,
            || {
                (
                    score_pre_for_signature(signature, timeline, candidate, params),
                    score_post_for_signature(signature, timeline, candidate_end, params),
                )
            },
        );
        let (wave_pre, wave_post) = timers.time(
            |t| &t.seam_ns,
            || waveform_seams_at_start(waveform, candidate, score_channels),
        );
        let score = timers.time(
            |t| &t.score_ns,
            || {
                unified_fit_score_with_repeat(
                    UnifiedFitCandidate {
                        structure_pre: pre_score,
                        structure_post: post_score,
                        wave_pre,
                        wave_post,
                        placement: fill_bracket_placement(
                            candidate,
                            candidate_end,
                            nominal_start,
                            nominal_end,
                        ),
                    },
                    params,
                    weights,
                    waveform,
                    anchor_prior,
                    // `candidate` moves the placement start, so the repeat window moves too.
                    RepeatPenaltySource::PerCandidate,
                    &timers,
                )
            },
        );
        let better = score > best_score + SCORE_TIE_EPSILON
            || (score >= best_score - SCORE_TIE_EPSILON
                && prefer_start(candidate, best_start, nominal_start));
        if better {
            best_score = score;
            best_start = candidate;
        }
    }
    drop(polish_guard);
    polish_span.record("candidates", polish_n);
    timers.record_on(&polish_span);

    best_start
}

/// Structure-only match for regression comparison in tests.
pub fn match_gap_structure_only_in_b(
    signature: &GapContextSignature,
    b_samples: &[f32],
    channels: usize,
    nominal_fill_start: usize,
    nominal_fill_end: usize,
    params: &StructureMatchParams,
) -> Option<FillAlignment> {
    gap_structure::match_gap_structure_in_b(
        signature,
        b_samples,
        channels,
        nominal_fill_start,
        nominal_fill_end,
        params,
    )
}


fn fill_anchor_better(
    head: (f64, f64),
    tail: (f64, f64),
) -> bool {
    let (pre_h, post_h) = head;
    let (pre_t, post_t) = tail;
    let min_h = pre_h.min(post_h);
    let min_t = pre_t.min(post_t);
    min_h > min_t + SCORE_TIE_EPSILON
        || (min_h >= min_t - SCORE_TIE_EPSILON && post_h > post_t + SCORE_TIE_EPSILON)
}

/// Fit a B bracket to A gap length (trim tail / pad tail).
pub fn fit_fill_to_gap_frames(samples: &[f32], channels: usize, target_frames: usize) -> Vec<f32> {
    let channels = channels.max(1);
    let source_frames = samples.len() / channels;
    if source_frames == target_frames {
        return samples.to_vec();
    }
    if source_frames == 0 {
        return vec![0.0f32; target_frames * channels];
    }

    if source_frames > target_frames {
        return samples[..target_frames * channels].to_vec();
    }

    let mut out = vec![0.0f32; target_frames * channels];
    out[..samples.len()].copy_from_slice(samples);
    out
}

/// Fallback frames guarding each end of a dual-fit bridge from the interior trim/pad point when no
/// caller-supplied guard is available. Production call sites must instead pass the border
/// re-validation window length (`seam_window_frames`) as the guard — a fixed small guard only keeps
/// the length edit off the shoulder seams, but the post-trim fill is re-scored by
/// `fill_splice_seam_correlations_interleaved` over a much larger window at each end, and a cut
/// inside that window corrupts the very edge the re-validation gate checks.
pub const DUALFIT_INTERIOR_EDGE_GUARD_FRAMES: usize = 64;

/// Start frame of the `window`-frame interior span with the lowest mean-square energy, kept ≥ `edge_guard`
/// frames from each end. The least-audible place to cut or hold.
fn lowest_energy_interior_start(fill: &[f32], channels: usize, window: usize, edge_guard: usize) -> usize {
    let ch = channels.max(1);
    let n = fill.len() / ch;
    let window = window.max(1);
    if n <= window + 2 * edge_guard {
        return edge_guard.min(n.saturating_sub(window));
    }
    let ms = |f: usize| -> f64 {
        let s = &fill[f * ch..(f + window) * ch];
        s.iter().map(|&x| (x as f64) * (x as f64)).sum::<f64>() / (s.len().max(1) as f64)
    };
    let (lo, hi) = (edge_guard, n - window - edge_guard);
    let mut best = lo;
    let mut best_ms = ms(lo);
    for f in (lo + 1)..=hi {
        let m = ms(f);
        if m < best_ms {
            best_ms = m;
            best = f;
        }
    }
    best
}

/// Reconcile a dual-fit **bridge** — B's content between the two seam-local-placed shoulders (length
/// = `gap ± step`) — to exactly `gap_frames`, editing **only the interior** at its **lowest-energy point**
/// (the least-audible splice). The shoulders keep their own lags (A3 §4); this is the interior length edit.
/// `bridge − gap > 0` ⇒ trim those interior frames; `< 0` ⇒ pad by holding the quietest interior frame.
/// *(A crossfade across the interior join is a D7 audibility refinement — the cut is already at min energy.)*
///
/// `edge_guard_frames` must be at least the border re-validation window length
/// (`seam_window_frames`) so the cut can never land inside either border-scoring window of the
/// resulting fill — otherwise the length edit corrupts the very edge the re-validation gate checks.
pub fn trim_at_lowest_energy_interior(
    bridge: &[f32],
    channels: usize,
    gap_frames: usize,
    edge_guard_frames: usize,
) -> Vec<f32> {
    let ch = channels.max(1);
    let b_frames = bridge.len() / ch;
    if b_frames == gap_frames || gap_frames == 0 || b_frames == 0 {
        return fit_fill_to_gap_frames(bridge, ch, gap_frames);
    }
    let guard = edge_guard_frames;
    if b_frames > gap_frames {
        let trim = b_frames - gap_frames;
        let cut = lowest_energy_interior_start(bridge, ch, trim, guard);
        let mut out = Vec::with_capacity(gap_frames * ch);
        out.extend_from_slice(&bridge[..cut * ch]);
        out.extend_from_slice(&bridge[(cut + trim) * ch..]);
        out
    } else {
        let pad = gap_frames - b_frames;
        let at = lowest_energy_interior_start(bridge, ch, 1, guard).min(b_frames - 1);
        let mut out = Vec::with_capacity(gap_frames * ch);
        out.extend_from_slice(&bridge[..(at + 1) * ch]);
        let held = &bridge[at * ch..(at + 1) * ch];
        for _ in 0..pad {
            out.extend_from_slice(held);
        }
        out.extend_from_slice(&bridge[(at + 1) * ch..]);
        out
    }
}

fn placement_repeat_post(
    fill_interleaved: &[f32],
    channels: usize,
    a_post: &[f64],
    a_post_ch: &[Vec<f64>],
    repeat_window_frames: usize,
    post_window: usize,
) -> f64 {
    let channels = channels.max(1);
    let b_mono = interleaved_to_mono(fill_interleaved, channels);
    let gap_frames = b_mono.len();
    if gap_frames == 0 {
        return 0.0;
    }
    let b_ch: Vec<Vec<f64>> = if channels > 1 && !a_post_ch.is_empty() {
        (0..channels)
            .map(|ch| {
                fill_interleaved
                    .iter()
                    .skip(ch)
                    .step_by(channels)
                    .map(|&s| s as f64)
                    .collect()
            })
            .collect()
    } else {
        vec![b_mono.clone()]
    };
    let (_, repeat_post) = fill_repeat_correlations(
        &SeamTemplates {
            a_pre: &[],
            a_post,
            a_pre_ch: &[],
            a_post_ch,
            b_mono: &b_mono,
            b_ch: &b_ch,
        },
        SeamPlacement {
            start: 0,
            gap_frames,
            pre_window: 0,
            post_window,
        },
        repeat_window_frames,
    );
    repeat_post
}

/// Greedily extend a short B bracket into contiguous B audio while `min(pre, post)` does not
/// fall and repeat-at-post stays bounded; zero-pad any remaining frames.
pub fn score_extend_short_fill_to_gap_frames(
    fill_interleaved: &[f32],
    extension_interleaved: &[f32],
    channels: usize,
    gap_frames: usize,
    borders: &BorderSeamTemplates<'_>,
    repeat_window_frames: usize,
    seam_ctx: SpliceSeamContext<'_>,
) -> Vec<f32> {
    let channels = channels.max(1);
    let source_frames = fill_interleaved.len() / channels;
    debug_assert!(source_frames < gap_frames);

    let ext_frames = extension_interleaved.len() / channels;
    let mut out = fill_interleaved.to_vec();
    let score_at_length = |samples: &[f32]| {
        let padded = fit_fill_to_gap_frames(samples, channels, gap_frames);
        let (pre, post) = fill_splice_seam_correlations_interleaved(
            &padded,
            channels,
            borders,
            seam_ctx,
        );
        (pre.min(post), padded)
    };
    let (mut best_min, _) = score_at_length(&out);
    let mut current_frames = source_frames;
    let mut prev_repeat = placement_repeat_post(
        &out,
        channels,
        borders.a_post,
        borders.a_post_ch,
        repeat_window_frames,
        borders.post_window,
    );

    while current_frames < gap_frames {
        let ext_frame = current_frames - source_frames;
        if ext_frame >= ext_frames {
            break;
        }
        let frame_start = ext_frame * channels;
        let mut trial = out.clone();
        trial.extend_from_slice(&extension_interleaved[frame_start..frame_start + channels]);

        let (min_score, _) = score_at_length(&trial);
        let next_repeat = placement_repeat_post(
            &trial,
            channels,
            borders.a_post,
            borders.a_post_ch,
            repeat_window_frames,
            borders.post_window,
        );

        let score_ok = min_score >= best_min - SCORE_TIE_EPSILON;
        let repeat_ok = next_repeat <= REPEAT_CORR_THRESHOLD
            && next_repeat <= prev_repeat + SCORE_TIE_EPSILON;

        if score_ok && repeat_ok {
            out = trial;
            best_min = min_score;
            prev_repeat = next_repeat;
            current_frames += 1;
        } else {
            break;
        }
    }

    fit_fill_to_gap_frames(&out, channels, gap_frames)
}

/// Fit-mode length adjust: score-based extend when short, dual-anchor trim when long.
pub fn fit_fill_length_for_gap(
    fill_interleaved: &[f32],
    extension_interleaved: &[f32],
    channels: usize,
    gap_frames: usize,
    borders: &BorderSeamTemplates<'_>,
    repeat_window_frames: usize,
    seam_ctx: SpliceSeamContext<'_>,
) -> Vec<f32> {
    let channels = channels.max(1);
    let source_frames = fill_interleaved.len() / channels;
    if source_frames > gap_frames {
        pick_fill_length_anchor(fill_interleaved, channels, gap_frames, borders, seam_ctx)
    } else if source_frames < gap_frames {
        score_extend_short_fill_to_gap_frames(
            fill_interleaved,
            extension_interleaved,
            channels,
            gap_frames,
            borders,
            repeat_window_frames,
            seam_ctx,
        )
    } else {
        fill_interleaved.to_vec()
    }
}

/// When B bracket exceeds A gap length, pick trim-head vs trim-tail by waveform seam score (fit mode).
pub fn pick_fill_length_anchor(
    fill_interleaved: &[f32],
    channels: usize,
    gap_frames: usize,
    borders: &BorderSeamTemplates<'_>,
    seam_ctx: SpliceSeamContext<'_>,
) -> Vec<f32> {
    let channels = channels.max(1);
    let source_frames = fill_interleaved.len() / channels;
    if source_frames <= gap_frames {
        return fit_fill_to_gap_frames(fill_interleaved, channels, gap_frames);
    }

    let trim_tail = fit_fill_to_gap_frames(fill_interleaved, channels, gap_frames);
    let skip_frames = source_frames - gap_frames;
    let skip_samples = skip_frames * channels;
    let trim_head = fill_interleaved[skip_samples..].to_vec();

    let seams_tail = fill_splice_seam_correlations_interleaved(
        &trim_tail,
        channels,
        borders,
        seam_ctx,
    );
    let seams_head = fill_splice_seam_correlations_interleaved(
        &trim_head,
        channels,
        borders,
        seam_ctx,
    );

    if fill_anchor_better(seams_head, seams_tail) {
        trim_head
    } else {
        trim_tail
    }
}


/// Frame step for waveform slide search (finer when the radius is small).
pub fn waveform_search_step(max_adjustment_frames: usize) -> usize {
    if max_adjustment_frames <= 512 {
        1
    } else {
        (max_adjustment_frames / 256).max(1)
    }
}

/// True when the best waveform seam scores meet the Pearson floor in fit mode.
pub fn fit_mode_waveform_floor_passes(pre: f64, post: f64, min_correlation: f32) -> bool {
    pre.min(post) >= f64::from(min_correlation)
}

/// Slide the B fill start to maximize `min(pre, post)` waveform Pearson around the structure match.
pub fn search_best_waveform_placement(
    templates: &SeamTemplates<'_>,
    structure_alignment: &FillAlignment,
    gap_frames: usize,
    max_adjustment_frames: usize,
    pre_window: usize,
    post_window: usize,
    b_total_frames: usize,
) -> FillAlignment {
    let structure_start = structure_alignment.start_frame;
    let step = waveform_search_step(max_adjustment_frames);
    let mut best_start = structure_start;
    let mut best_pre = f64::NEG_INFINITY;
    let mut best_post = f64::NEG_INFINITY;
    let mut best_score = f64::NEG_INFINITY;

    let search_min = structure_start.saturating_sub(max_adjustment_frames);
    let search_max = (structure_start + max_adjustment_frames).min(b_total_frames);

    let mut start = search_min;
    while start <= search_max {
        if placement_in_bounds(start, gap_frames, pre_window, post_window, b_total_frames) {
            let (pre, post) = fill_seam_correlations(
                templates,
                SeamPlacement {
                    start,
                    gap_frames,
                    pre_window,
                    post_window,
                },
            );
            let score = pre.min(post);
            let delta = start.abs_diff(structure_start);
            let best_delta = best_start.abs_diff(structure_start);
            let better = score > best_score + SCORE_TIE_EPSILON
                || (score >= best_score - SCORE_TIE_EPSILON
                    && (delta < best_delta
                        || (delta == best_delta && start < best_start)));
            if better {
                best_score = score;
                best_start = start;
                best_pre = pre;
                best_post = post;
            }
        }
        start += step;
    }

    if best_score.is_finite() {
        let refine_min = best_start.saturating_sub(step.saturating_sub(1));
        let refine_max = (best_start + step.saturating_sub(1)).min(search_max);
        for candidate in refine_min..=refine_max {
            if !placement_in_bounds(candidate, gap_frames, pre_window, post_window, b_total_frames)
            {
                continue;
            }
            let (pre, post) = fill_seam_correlations(
                templates,
                SeamPlacement {
                    start: candidate,
                    gap_frames,
                    pre_window,
                    post_window,
                },
            );
            let score = pre.min(post);
            let delta = candidate.abs_diff(structure_start);
            let best_delta = best_start.abs_diff(structure_start);
            let better = score > best_score + SCORE_TIE_EPSILON
                || (score >= best_score - SCORE_TIE_EPSILON
                    && (delta < best_delta
                        || (delta == best_delta && candidate < best_start)));
            if better {
                best_score = score;
                best_start = candidate;
                best_pre = pre;
                best_post = post;
            }
        }
    }

    FillAlignment {
        start_frame: best_start,
        fill_frames: structure_alignment.fill_frames,
        pre_correlation: best_pre,
        post_correlation: best_post,
    }
}

fn placement_in_bounds(
    start: usize,
    gap_frames: usize,
    pre_window: usize,
    post_window: usize,
    b_total_frames: usize,
) -> bool {
    pre_window > 0
        && post_window > 0
        && start >= pre_window
        && start + gap_frames + post_window <= b_total_frames
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::policies::{
        SeamFloorSource, SeamResidualVerdict,
    };

    #[test]
    fn interior_trim_cuts_in_the_quiet_valley() {
        // 1000 frames, loud everywhere except a quiet valley [400,600). Trimming 100 frames to reach 900
        // must remove only quiet frames — the loud energy is fully preserved, and the cut lands in the valley.
        let bridge: Vec<f32> = (0..1000)
            .map(|i| if (400..600).contains(&i) { 0.001 } else { 0.5 })
            .collect();
        let out = trim_at_lowest_energy_interior(&bridge, 1, 900, DUALFIT_INTERIOR_EDGE_GUARD_FRAMES);
        assert_eq!(out.len(), 900, "trimmed to gap length");
        let loud = |v: &[f32]| v.iter().filter(|&&x| x > 0.4).count();
        assert_eq!(loud(&bridge), loud(&out), "trim removed only quiet frames");
    }

    #[test]
    fn interior_trim_pad_and_equal_lengths() {
        let ch = 2;
        let bridge = vec![0.1f32; 500 * ch]; // 500 frames
        let guard = DUALFIT_INTERIOR_EDGE_GUARD_FRAMES;
        assert_eq!(trim_at_lowest_energy_interior(&bridge, ch, 500, guard).len(), 500 * ch, "equal");
        assert_eq!(trim_at_lowest_energy_interior(&bridge, ch, 450, guard).len(), 450 * ch, "trim");
        assert_eq!(trim_at_lowest_energy_interior(&bridge, ch, 600, guard).len(), 600 * ch, "pad");
    }

    fn verdict(informative: bool, headroom: f64) -> SeamResidualVerdict {
        let floor_db = if informative { -40.0 } else { -5.0 };
        let chosen_db = floor_db + headroom;
        SeamResidualVerdict {
            chosen_pre_db: chosen_db,
            chosen_post_db: chosen_db,
            floor_pre_db: floor_db,
            floor_post_db: floor_db,
            floor_source_pre: SeamFloorSource::Border,
            floor_source_post: SeamFloorSource::Border,
            informative,
            placement_slide_frames: 0,
            max_lag_frames: 0,
        }
    }

    #[test]
    fn apply_residual_abstains_when_beyond_lag_reach() {
        let mut v = verdict(true, 120.0);
        v.placement_slide_frames = 600;
        v.max_lag_frames = 480;
        assert!(v.beyond_lag_reach());
        let out = apply_residual_to_confidence(
            Ok(FillConfidence::High),
            &v,
            6.0,
            false,
        );
        assert_eq!(out, Ok(FillConfidence::High));
    }

    #[test]
    fn apply_residual_abstains_when_uninformative() {
        let pearson = Ok(FillConfidence::High);
        let out = apply_residual_to_confidence(
            pearson,
            &verdict(false, 100.0),
            6.0,
            false,
        );
        assert_eq!(out, Ok(FillConfidence::High));
    }

    #[test]
    fn apply_residual_vetoes_high_pearson_when_headroom_large() {
        let err = apply_residual_to_confidence(
            Ok(FillConfidence::High),
            &verdict(true, 20.0),
            6.0,
            false,
        )
        .unwrap_err();
        assert!(matches!(err, ResidualGateError::HeadroomExceeded { .. }));
    }

    #[test]
    fn apply_residual_rescues_dead_zone_when_enabled() {
        let out = apply_residual_to_confidence(
            Err(0.13),
            &verdict(true, 0.0),
            6.0,
            true,
        )
        .expect("rescue");
        assert_eq!(out, FillConfidence::Marginal);
    }

    #[test]
    fn apply_residual_leaves_dead_zone_when_headroom_high() {
        let err = apply_residual_to_confidence(
            Err(0.13),
            &verdict(true, 20.0),
            6.0,
            true,
        )
        .unwrap_err();
        assert!(matches!(err, ResidualGateError::PearsonBelowFloor(_)));
    }

    fn no_seam_ctx<'a>(a_samples: &'a [f32]) -> SpliceSeamContext<'a> {
        SpliceSeamContext {
            seam_cf: 0,
            gap_start_frame: 0,
            gap_end_frame: 0,
            a_samples,
            channels: 1,
            single_lag_alignment: true,
        }
    }

    fn test_borders<'a>(
        a_pre: &'a [f64],
        a_post: &'a [f64],
        a_pre_ch: &'a [Vec<f64>],
        a_post_ch: &'a [Vec<f64>],
        pre_window: usize,
        post_window: usize,
    ) -> BorderSeamTemplates<'a> {
        BorderSeamTemplates {
            a_pre,
            a_post,
            a_pre_ch,
            a_post_ch,
            pre_window,
            post_window,
        }
    }

    fn fit_candidate(
        structure_pre: f64,
        structure_post: f64,
        wave_min: f64,
        start: usize,
        end: usize,
        nominal_start: usize,
        nominal_end: usize,
    ) -> UnifiedFitCandidate {
        // Symmetric seams (pre == post == wave_min) for the score tests; the repeat-penalty tests still exercise
        // the weak-seam branch via `wave_min < REPEAT_SEAM_WEAK` + a high `repeat_max` from real templates.
        UnifiedFitCandidate {
            structure_pre,
            structure_post,
            wave_pre: wave_min,
            wave_post: wave_min,
            placement: fill_bracket_placement(start, end, nominal_start, nominal_end),
        }
    }

    use crate::domain::pcm::{interleaved_to_channels, interleaved_to_mono};

    fn sine_frame(frame: usize, rate: u32) -> f32 {
        let t = frame as f64 / f64::from(rate);
        (f64::sin(2.0 * std::f64::consts::PI * 440.0 * t) * 0.305) as f32
    }

    fn build_b_haystack_with_dropout_offset(
        rate: u32,
        pre_frames: usize,
        lead_in_frames: usize,
        gap_frames: usize,
        post_frames: usize,
    ) -> (Vec<f32>, usize) {
        let gap_start = pre_frames + lead_in_frames;
        let total = gap_start + gap_frames + post_frames;
        let mut samples = Vec::with_capacity(total);
        for frame in 0..total {
            let in_gap = frame >= gap_start && frame < gap_start + gap_frames;
            let sample = if in_gap {
                0.0f32
            } else {
                sine_frame(frame, rate)
            };
            samples.push(sample);
        }
        (samples, gap_start)
    }

    #[test]
    fn waveform_search_finds_offset_from_structure_nominal() {
        let rate = 48_000u32;
        let pre_frames = 200usize;
        let lead_in_frames = 3usize;
        let gap_frames = 48usize;
        let post_frames = 200usize;
        let (b_samples, true_gap_start) =
            build_b_haystack_with_dropout_offset(rate, pre_frames, lead_in_frames, gap_frames, post_frames);
        let b_mono = interleaved_to_mono(&b_samples, 1);
        let b_ch = interleaved_to_channels(&b_samples, 1);

        let pre_len = 64usize;
        let post_len = 64usize;
        let gap_start_on_a = pre_frames;
        let a_pre: Vec<f64> = (0..pre_len)
            .map(|i| sine_frame(gap_start_on_a - pre_len + i, rate) as f64)
            .collect();
        let a_post: Vec<f64> = (0..post_len)
            .map(|i| sine_frame(gap_start_on_a + gap_frames + i, rate) as f64)
            .collect();

        let structure_start = gap_start_on_a;
        let structure_alignment = FillAlignment {
            start_frame: structure_start,
            fill_frames: gap_frames,
            pre_correlation: 0.0,
            post_correlation: 0.0,
        };

        let templates = SeamTemplates {
            a_pre: &a_pre,
            a_post: &a_post,
            a_pre_ch: std::slice::from_ref(&a_pre),
            a_post_ch: std::slice::from_ref(&a_post),
            b_mono: &b_mono,
            b_ch: &b_ch,
        };

        let best = search_best_waveform_placement(
            &templates,
            &structure_alignment,
            gap_frames,
            8,
            pre_len,
            post_len,
            b_mono.len(),
        );

        assert_eq!(
            best.start_frame, true_gap_start,
            "expected waveform slide +{lead_in_frames}, got +{}",
            best.start_frame.saturating_sub(structure_start)
        );
        assert!(fit_mode_waveform_floor_passes(
            best.pre_correlation,
            best.post_correlation,
            0.5
        ));
    }

    #[test]
    fn fit_mode_floor_uses_min_of_seams() {
        assert!(fit_mode_waveform_floor_passes(0.5, 0.4, 0.35));
        assert!(!fit_mode_waveform_floor_passes(0.5, 0.2, 0.35));
    }

    #[test]
    fn classify_fill_waveform_confidence_tiers() {
        use super::FillConfidence;

        assert_eq!(
            classify_fill_waveform_confidence(0.5, 0.4, 0.35, 0.08, 0.12).unwrap(),
            FillConfidence::High
        );
        assert_eq!(
            classify_fill_waveform_confidence(0.30, 0.28, 0.35, 0.08, 0.12).unwrap(),
            FillConfidence::Marginal
        );
        assert!(classify_fill_waveform_confidence(0.10, 0.15, 0.35, 0.08, 0.12).is_err());
        // Gate disabled: hard floor follows min_fill_correlation.
        assert_eq!(
            classify_fill_waveform_confidence(0.05, 0.04, -1.0, 0.08, 0.12).unwrap(),
            FillConfidence::High
        );
    }

    #[test]
    fn fit_candidate_ranking_prefers_less_boundary_move_at_equal_waveform() {
        assert!(fit_candidate_ranking_score(0.5, 0) > fit_candidate_ranking_score(0.5, 100));
    }

    #[test]
    fn anchor_ranking_penalizes_center_drift_at_equal_waveform_and_boundary_move() {
        assert!(
            fit_anchor_candidate_ranking_score(0.5, 100, 0)
                > fit_anchor_candidate_ranking_score(0.5, 100, 500)
        );
    }

    #[test]
    fn anchor_center_drift_is_zero_when_bracket_matches_scan_hole() {
        use crate::domain::policies::RefinedGapFrames;
        let hole = RefinedGapFrames {
            start_frame: 100,
            end_frame: 200,
        };
        assert_eq!(anchor_bracket_center_drift_frames(hole, hole), 0);
    }

    #[test]
    fn waveform_search_step_is_one_for_small_radius() {
        assert_eq!(waveform_search_step(100), 1);
        assert!(waveform_search_step(10_000) > 1);
    }

    /// Lever 1 (§2.5) placement-diff: the FFT seam band (flag ON) must pick the SAME fill placement as the
    /// naive search (flag OFF). This is the integration-level guard the plan requires before the production
    /// default may flip on — it exercises the whole refine band + `placement_in_bounds` gate + belt path, not
    /// just the band evaluator (which its own unit test covers). Broadband noise ⇒ a single sharp seam peak.
    #[test]
    fn fft_seam_search_matches_naive_placement() {
        let mut seed = 0x00C0_FFEE_1234_5678u64;
        let mut rng = || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((seed >> 33) as f64 / (1u64 << 30) as f64) - 1.0
        };

        let bin_frames = 20usize;
        let gap_frames = 200usize;
        let pre_window = 96usize;
        let post_window = 96usize;
        let true_start = 1500usize;
        let total = 3000usize;

        // Loud broadband B everywhere ⇒ every timeline bin active ⇒ uniform structure score, so the waveform
        // seam term (the FFT-affected one) decides the placement. Plant A's borders so the pre seam ends at
        // `true_start` and the post seam starts at `true_start + gap_frames` — a unique correlation peak.
        let mut b_mono: Vec<f64> = (0..total).map(|_| rng() * 0.5).collect();
        let a_pre: Vec<f64> = (0..pre_window).map(|_| rng()).collect();
        let a_post: Vec<f64> = (0..post_window).map(|_| rng()).collect();
        b_mono[true_start - pre_window..true_start].copy_from_slice(&a_pre);
        b_mono[true_start + gap_frames..true_start + gap_frames + post_window]
            .copy_from_slice(&a_post);

        let b_samples: Vec<f32> = b_mono.iter().map(|&x| x as f32).collect();
        let b_ch = vec![b_mono.clone()];
        let templates = SeamTemplates {
            a_pre: &a_pre,
            a_post: &a_post,
            a_pre_ch: std::slice::from_ref(&a_pre),
            a_post_ch: std::slice::from_ref(&a_post),
            b_mono: &b_mono,
            b_ch: &b_ch,
        };
        let waveform = WaveformSeamContext {
            templates: &templates,
            gap_frames,
            pre_window,
            post_window,
            b_total_frames: total,
            repeat_window_frames: bin_frames,
            repeat_penalty_weight: 0.0,
        };
        let params = StructureMatchParams {
            gap_frames,
            bin_frames,
            search_radius_frames: 500,
            fill_length_slack_frames: 40,
            max_fine_adjustment_frames: 0,
            silence_peak_fraction: 0.01,
            absolute_silence_rms: 0.0,
        };
        let weights = UnifiedFitWeights {
            structure_weight: 0.1,
            waveform_weight: 0.9,
            ..Default::default()
        };
        let signature = GapSignature::Bool(GapContextSignature {
            pre_bins: vec![true; 8],
            post_bins: vec![true; 8],
        });
        let timeline = gap_structure::ActivityTimeline::build(
            &b_samples,
            1,
            total,
            bin_frames,
            params.silence_peak_fraction,
            params.absolute_silence_rms,
        );
        let structure_timeline = StructureTimeline::Bool(&timeline);
        let input = UnifiedFillSearchInput {
            signature: &signature,
            b_samples: &b_samples,
            channels: 1,
            waveform: &waveform,
            // Start off-true so the search genuinely has to move into the refine band.
            nominal_fill_start: true_start - 40,
            nominal_fill_end: true_start - 40 + gap_frames,
        };

        let naive = match_gap_fill_unified_in_b_with_timeline(
            &input,
            &params,
            weights,
            &structure_timeline,
            None,
            false,
            false,
        )
        .expect("naive search finds a fill");
        let fft = match_gap_fill_unified_in_b_with_timeline(
            &input,
            &params,
            weights,
            &structure_timeline,
            None,
            true,
            false,
        )
        .expect("fft search finds a fill");

        assert_eq!(
            naive.alignment.start_frame, fft.alignment.start_frame,
            "FFT seam band must pick the same placement as naive (naive={}, fft={})",
            naive.alignment.start_frame, fft.alignment.start_frame,
        );
        assert!(
            naive.alignment.start_frame.abs_diff(true_start) <= bin_frames,
            "search located the planted fill: start={} true={}",
            naive.alignment.start_frame,
            true_start,
        );
    }

    /// Lever 1b(b) placement-diff (plan §6 (2)): the FFT repeat band (flag ON) must pick the SAME fill
    /// placement as the naive per-candidate repeat correlation (flag OFF). The seam band is held OFF in both
    /// arms so a divergence can only be attributed to the repeat band.
    ///
    /// The templates carry **two channels of unequal border length**, which is the configuration §2.1 #2/#3
    /// are about: each channel derives its own `w`, while the outer channel gate is keyed on the *mono*
    /// window. A band that only reproduced the per-channel bounds would score these differently.
    #[test]
    fn fft_repeat_band_matches_naive_placement() {
        let mut seed = 0x00C0_FFEE_9876_5432u64;
        let mut rng = || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((seed >> 33) as f64 / (1u64 << 30) as f64) - 1.0
        };

        let bin_frames = 20usize;
        let gap_frames = 200usize;
        let pre_window = 96usize;
        let post_window = 96usize;
        let true_start = 1500usize;
        let total = 3000usize;

        let mut b_mono: Vec<f64> = (0..total).map(|_| rng() * 0.5).collect();
        let a_pre: Vec<f64> = (0..pre_window).map(|_| rng()).collect();
        let a_post: Vec<f64> = (0..post_window).map(|_| rng()).collect();
        b_mono[true_start - pre_window..true_start].copy_from_slice(&a_pre);
        b_mono[true_start + gap_frames..true_start + gap_frames + post_window]
            .copy_from_slice(&a_post);

        let b_samples: Vec<f32> = b_mono.iter().map(|&x| x as f32).collect();
        // Two B channels (same content, so the correlations stay well-conditioned) against two A-side border
        // templates of DIFFERENT lengths — the unequal-`w` case.
        let b_ch = vec![b_mono.clone(), b_mono.clone()];
        let a_pre_ch = vec![a_pre.clone(), a_pre[pre_window - 61..].to_vec()];
        let a_post_ch = vec![a_post.clone(), a_post[..57].to_vec()];
        let templates = SeamTemplates {
            a_pre: &a_pre,
            a_post: &a_post,
            a_pre_ch: &a_pre_ch,
            a_post_ch: &a_post_ch,
            b_mono: &b_mono,
            b_ch: &b_ch,
        };
        let waveform = WaveformSeamContext {
            templates: &templates,
            gap_frames,
            pre_window,
            post_window,
            b_total_frames: total,
            repeat_window_frames: bin_frames * 3,
            // Non-zero: with the production default weight the repeat term actually reaches the score, so a
            // banded/naive mismatch can move the winner. At 0.0 the band is never even built.
            repeat_penalty_weight: 0.4,
        };
        let params = StructureMatchParams {
            gap_frames,
            bin_frames,
            search_radius_frames: 500,
            fill_length_slack_frames: 40,
            max_fine_adjustment_frames: 0,
            silence_peak_fraction: 0.01,
            absolute_silence_rms: 0.0,
        };
        let weights = UnifiedFitWeights {
            structure_weight: 0.1,
            waveform_weight: 0.9,
            ..Default::default()
        };
        let signature = GapSignature::Bool(GapContextSignature {
            pre_bins: vec![true; 8],
            post_bins: vec![true; 8],
        });
        let timeline = gap_structure::ActivityTimeline::build(
            &b_samples,
            1,
            total,
            bin_frames,
            params.silence_peak_fraction,
            params.absolute_silence_rms,
        );
        let structure_timeline = StructureTimeline::Bool(&timeline);
        let input = UnifiedFillSearchInput {
            signature: &signature,
            b_samples: &b_samples,
            channels: 1,
            waveform: &waveform,
            nominal_fill_start: true_start - 40,
            nominal_fill_end: true_start - 40 + gap_frames,
        };

        let naive = match_gap_fill_unified_in_b_with_timeline(
            &input,
            &params,
            weights,
            &structure_timeline,
            None,
            false,
            false,
        )
        .expect("naive search finds a fill");
        let banded = match_gap_fill_unified_in_b_with_timeline(
            &input,
            &params,
            weights,
            &structure_timeline,
            None,
            false,
            true,
        )
        .expect("banded search finds a fill");

        assert_eq!(
            naive.alignment.start_frame, banded.alignment.start_frame,
            "FFT repeat band must pick the same placement as naive (naive={}, banded={})",
            naive.alignment.start_frame, banded.alignment.start_frame,
        );
        assert!(
            naive.alignment.start_frame.abs_diff(true_start) <= bin_frames,
            "search located the planted fill: start={} true={}",
            naive.alignment.start_frame,
            true_start,
        );
    }

    /// Lever 1b(b) §4: pin the belt's edge semantics, which are the part that cannot be read off the
    /// threshold. Both bands emit infinities at declined placements, and the naive path never emits `NaN` —
    /// so identical infinities must AGREE (`inf − inf` is `NaN`, which no threshold can classify) while a
    /// `NaN` from either side must DIVERGE rather than slip through a `NaN > tol == false` comparison.
    #[test]
    fn band_agreement_treats_infinities_as_equal_and_nan_as_divergence() {
        assert!(band_agrees_with_naive(0.5, 0.5));
        assert!(band_agrees_with_naive(0.5, 0.5 + FFT_SEAM_DISCREPANCY_TOL * 0.5));
        assert!(!band_agrees_with_naive(0.5, 0.5 + FFT_SEAM_DISCREPANCY_TOL * 10.0));

        assert!(band_agrees_with_naive(f64::NEG_INFINITY, f64::NEG_INFINITY));
        assert!(band_agrees_with_naive(f64::INFINITY, f64::INFINITY));
        // One side declined and the other did not: a real disagreement, not an edge to be excused.
        assert!(!band_agrees_with_naive(f64::NEG_INFINITY, 0.5));
        assert!(!band_agrees_with_naive(0.5, f64::NEG_INFINITY));
        assert!(!band_agrees_with_naive(f64::NEG_INFINITY, f64::INFINITY));

        assert!(!band_agrees_with_naive(f64::NAN, 0.5));
        assert!(!band_agrees_with_naive(0.5, f64::NAN));
        assert!(!band_agrees_with_naive(f64::NAN, f64::NAN));
    }

    #[test]
    fn unified_fit_weights_normalize_to_unit_sum() {
        let w = UnifiedFitWeights {
            structure_weight: 1.0,
            waveform_weight: 1.0,
            ..Default::default()
        }
        .normalized();
        assert!((w.structure_weight - 0.5).abs() < 1e-9);
        assert!((w.waveform_weight - 0.5).abs() < 1e-9);
    }

    #[test]
    fn unified_fit_score_favors_waveform_when_structure_differs_slightly() {
        use crate::domain::gap_structure::StructureMatchParams;

        let params = StructureMatchParams {
            gap_frames: 40,
            bin_frames: 20,
            search_radius_frames: 100,
            fill_length_slack_frames: 0,
            max_fine_adjustment_frames: 0,
            silence_peak_fraction: 0.01,
            absolute_silence_rms: 0.0,
        };
        let weights = UnifiedFitWeights {
            structure_weight: 0.2,
            waveform_weight: 0.8,
            ..Default::default()
        };
        let nominal_start = 200usize;
        let nominal_end = 240usize;
        let score_wrong = unified_fit_score(
            0.92,
            0.92,
            0.05,
            fill_bracket_placement(nominal_start, nominal_end, nominal_start, nominal_end),
            &params,
            weights,
        );
        let score_true = unified_fit_score(
            0.88,
            0.88,
            0.96,
            fill_bracket_placement(100, 140, nominal_start, nominal_end),
            &params,
            weights,
        );
        assert!(
            score_true > score_wrong,
            "waveform-heavy score should prefer true cycle (true={score_true}, wrong={score_wrong})"
        );
    }

    #[test]
    fn repeat_penalty_downranks_duplicate_fill_when_seams_weak() {
        use crate::domain::gap_structure::StructureMatchParams;

        let params = StructureMatchParams {
            gap_frames: 40,
            bin_frames: 20,
            search_radius_frames: 100,
            fill_length_slack_frames: 0,
            max_fine_adjustment_frames: 0,
            silence_peak_fraction: 0.01,
            absolute_silence_rms: 0.0,
        };
        let weights = UnifiedFitWeights::default();
        let a_pre: Vec<f64> = (0..16).map(|i| (i as f64 * 0.1).sin()).collect();
        let a_post: Vec<f64> = (0..16).map(|i| (i as f64 * 0.2).cos()).collect();
        let mut b_mono = vec![0.0f64; 200];
        b_mono[100..116].copy_from_slice(&a_pre[..16]);
        b_mono[140..156].copy_from_slice(&a_post[..16]);
        let b_ch = vec![b_mono.clone()];
        let templates = SeamTemplates {
            a_pre: &a_pre,
            a_post: &a_post,
            a_pre_ch: std::slice::from_ref(&a_pre),
            a_post_ch: std::slice::from_ref(&a_post),
            b_mono: &b_mono,
            b_ch: &b_ch,
        };
        let waveform = WaveformSeamContext {
            templates: &templates,
            gap_frames: 40,
            pre_window: 16,
            post_window: 16,
            b_total_frames: b_mono.len(),
            repeat_window_frames: 16,
            repeat_penalty_weight: 1.0,
        };
        let base = unified_fit_score_with_repeat(
            fit_candidate(0.9, 0.9, 0.2, 100, 140, 100, 140),
            &params,
            weights,
            &waveform,
            None,
            RepeatPenaltySource::PerCandidate,
            &CandidateTimers::default(),
        );
        let no_penalty = WaveformSeamContext {
            repeat_penalty_weight: 0.0,
            ..waveform
        };
        let without = unified_fit_score_with_repeat(
            fit_candidate(0.9, 0.9, 0.2, 100, 140, 100, 140),
            &params,
            weights,
            &no_penalty,
            None,
            RepeatPenaltySource::PerCandidate,
            &CandidateTimers::default(),
        );
        assert!(
            without > base,
            "repeat penalty should lower score when fill duplicates borders (with={without}, base={base})"
        );
    }

    /// Lever 1b(c): the end search hoists the repeat penalty above its candidate loop, which is only valid if
    /// the penalty does not depend on `end`. Pin that here — vary `end` across the loop's whole slack range and
    /// require `Fixed(penalty computed once at fill_start)` to be BIT-equal to `PerCandidate`. If someone ever
    /// makes the repeat window depend on the candidate end, this fails instead of silently changing placements.
    #[test]
    fn end_search_repeat_penalty_is_invariant_to_fill_end() {
        use crate::domain::gap_structure::StructureMatchParams;

        let params = StructureMatchParams {
            gap_frames: 40,
            bin_frames: 20,
            search_radius_frames: 100,
            // The end search's candidate range is `gap_frames ± fill_length_slack_frames`; a non-zero slack
            // is what makes "vary `end`" meaningful here.
            fill_length_slack_frames: 20,
            max_fine_adjustment_frames: 0,
            silence_peak_fraction: 0.01,
            absolute_silence_rms: 0.0,
        };
        let weights = UnifiedFitWeights::default();
        let a_pre: Vec<f64> = (0..16).map(|i| (i as f64 * 0.1).sin()).collect();
        let a_post: Vec<f64> = (0..16).map(|i| (i as f64 * 0.2).cos()).collect();
        let mut b_mono = vec![0.0f64; 200];
        // Duplicate both borders into the fill interior so the penalty is genuinely non-zero — an all-zero
        // penalty would make the two sources agree trivially and prove nothing.
        b_mono[100..116].copy_from_slice(&a_pre[..16]);
        b_mono[140..156].copy_from_slice(&a_post[..16]);
        let b_ch = vec![b_mono.clone()];
        let templates = SeamTemplates {
            a_pre: &a_pre,
            a_post: &a_post,
            a_pre_ch: std::slice::from_ref(&a_pre),
            a_post_ch: std::slice::from_ref(&a_post),
            b_mono: &b_mono,
            b_ch: &b_ch,
        };
        let waveform = WaveformSeamContext {
            templates: &templates,
            gap_frames: 40,
            pre_window: 16,
            post_window: 16,
            b_total_frames: b_mono.len(),
            repeat_window_frames: 16,
            repeat_penalty_weight: 1.0,
        };

        let fill_start = 100usize;
        let timers = CandidateTimers::default();
        let hoisted = repeat_penalty_at_placement(&waveform, fill_start, 0.2, 0.2, &timers);
        assert!(
            hoisted > 0.0,
            "fixture should produce a real repeat penalty, got {hoisted}"
        );

        for end in 120..=160 {
            // Exactly how `unified_search_best_fill_end` builds its candidate: placement start pinned to
            // `fill_start`, seams pinned to the hoisted constants, only `end` moving.
            let candidate = || UnifiedFitCandidate {
                structure_pre: 0.9,
                structure_post: 0.9,
                wave_pre: 0.2,
                wave_post: 0.2,
                placement: fill_bracket_placement(fill_start, end, fill_start, 140),
            };
            let per_candidate = unified_fit_score_with_repeat(
                candidate(),
                &params,
                weights,
                &waveform,
                None,
                RepeatPenaltySource::PerCandidate,
                &timers,
            );
            let fixed = unified_fit_score_with_repeat(
                candidate(),
                &params,
                weights,
                &waveform,
                None,
                RepeatPenaltySource::Fixed(hoisted),
                &timers,
            );
            assert_eq!(
                per_candidate.to_bits(),
                fixed.to_bits(),
                "hoisted repeat penalty diverged at end={end} \
                 (per_candidate={per_candidate}, fixed={fixed})"
            );
        }
    }

    #[test]
    fn pick_fill_length_anchor_prefers_better_seam_end() {
        let channels = 1usize;
        let gap_frames = 4usize;
        let pre_w = 2usize;
        let post_w = 2usize;
        let a_pre = vec![0.1, -0.1];
        let a_post = vec![0.6, -0.4];
        let post: Vec<f32> = a_post.iter().map(|&v| v as f32).collect();
        let fill = vec![0.0f32, 0.0, 0.0, 0.0, post[0], post[1]];
        let seam_ctx = no_seam_ctx(&[]);
        let picked = pick_fill_length_anchor(
            &fill,
            channels,
            gap_frames,
            &test_borders(&a_pre, &a_post, &[], &[], pre_w, post_w),
            seam_ctx,
        );
        assert_eq!(picked, vec![0.0, 0.0, post[0], post[1]]);
    }

    #[test]
    fn pick_fill_length_anchor_uses_min_seam_not_one_strong_side() {
        let channels = 1usize;
        let gap_frames = 4usize;
        let pre_w = 2usize;
        let post_w = 2usize;
        let a_pre: Vec<f64> = (0..pre_w)
            .map(|i| (i as f64 * 0.8).sin())
            .collect();
        let a_post: Vec<f64> = (0..post_w)
            .map(|i| ((i + 4) as f64 * 0.5).cos())
            .collect();
        let a_pre_ch = vec![a_pre.clone()];
        let a_post_ch = vec![a_post.clone()];
        let pre: Vec<f32> = a_pre.iter().map(|&v| v as f32).collect();
        let post: Vec<f32> = a_post.iter().map(|&v| v as f32).collect();
        let junk = vec![0.0f32, 0.0, 0.0, 0.0];
        let mut fill = pre;
        fill.extend(&junk);
        fill.extend(&post);
        let seam_ctx = no_seam_ctx(&[]);
        // Tail: strong pre, weak post → low min. Head: weak pre, strong post → higher min.
        let picked = pick_fill_length_anchor(
            &fill,
            channels,
            gap_frames,
            &test_borders(&a_pre, &a_post, &a_pre_ch, &a_post_ch, pre_w, post_w),
            seam_ctx,
        );
        assert_eq!(picked, vec![0.0, 0.0, post[0], post[1]]);
    }

    #[test]
    fn score_extend_stops_before_extension_degrades_post_seam() {
        let channels = 1usize;
        let gap_frames = 5usize;
        let pre_w = 2usize;
        let post_w = 2usize;
        let a_pre = vec![0.9, -0.9];
        let a_post = vec![0.8, 0.7];
        let fill = vec![0.9f32, -0.9f32];
        let extension = vec![0.75f32, 0.8f32, -1.0f32, -1.0f32];

        let blind = {
            let mut raw = fill.clone();
            raw.extend(&extension[..3]);
            fit_fill_to_gap_frames(&raw, channels, gap_frames)
        };
        let scored = score_extend_short_fill_to_gap_frames(
            &fill,
            &extension,
            channels,
            gap_frames,
            &test_borders(
                &a_pre,
                &a_post,
                std::slice::from_ref(&a_pre),
                std::slice::from_ref(&a_post),
                pre_w,
                post_w,
            ),
            pre_w,
            no_seam_ctx(&[]),
        );

        let extension_frames_used = |samples: &[f32]| {
            let mut end = samples.len();
            while end > fill.len() && samples[end - 1] == 0.0 {
                end -= 1;
            }
            (end - fill.len()) / channels
        };

        assert!(
            extension_frames_used(&scored) < extension_frames_used(&blind),
            "score extend should take less contiguous B audio than blind extend before padding"
        );
        assert_eq!(scored.len(), gap_frames * channels);
    }

    #[test]
    fn fit_fill_length_for_gap_delegates_short_bracket_to_score_extend() {
        let channels = 1usize;
        let gap_frames = 5usize;
        let window = 2usize;
        let a_pre = vec![0.9, -0.9];
        let a_post = vec![0.8, 0.7];
        let fill = vec![0.9f32, -0.9f32];
        let extension = vec![0.75f32, 0.8f32, -1.0f32, -1.0f32];
        let borders = test_borders(
            &a_pre,
            &a_post,
            std::slice::from_ref(&a_pre),
            std::slice::from_ref(&a_post),
            window,
            window,
        );
        let via_api = fit_fill_length_for_gap(
            &fill,
            &extension,
            channels,
            gap_frames,
            &borders,
            window,
            no_seam_ctx(&[]),
        );
        let direct = score_extend_short_fill_to_gap_frames(
            &fill,
            &extension,
            channels,
            gap_frames,
            &borders,
            window,
            no_seam_ctx(&[]),
        );
        assert_eq!(via_api, direct);
    }

    #[test]
    fn fill_anchor_better_prefers_higher_min_seam() {
        assert!(fill_anchor_better((0.5, 0.9), (0.4, 0.95)));
        assert!(!fill_anchor_better((0.4, 0.95), (0.5, 0.9)));
        assert!(fill_anchor_better((0.3, 0.8), (0.3, 0.5)));
    }

    #[test]
    fn unified_search_penalty_downranks_repeat_cycle() {
        use crate::domain::gap_structure::StructureMatchParams;

        let params = StructureMatchParams {
            gap_frames: 40,
            bin_frames: 20,
            search_radius_frames: 100,
            fill_length_slack_frames: 0,
            max_fine_adjustment_frames: 0,
            silence_peak_fraction: 0.01,
            absolute_silence_rms: 0.0,
        };
        let weights = UnifiedFitWeights::default();
        let a_pre: Vec<f64> = (0..16).map(|i| (i as f64 * 0.1).sin()).collect();
        let a_post: Vec<f64> = (0..16).map(|i| (i as f64 * 0.2).cos()).collect();
        let mut b_mono = vec![0.0f64; 200];
        b_mono[100..116].copy_from_slice(&a_pre[..16]);
        let b_ch = vec![b_mono.clone()];
        let templates = SeamTemplates {
            a_pre: &a_pre,
            a_post: &a_post,
            a_pre_ch: std::slice::from_ref(&a_pre),
            a_post_ch: std::slice::from_ref(&a_post),
            b_mono: &b_mono,
            b_ch: &b_ch,
        };
        let waveform_penalized = WaveformSeamContext {
            templates: &templates,
            gap_frames: 40,
            pre_window: 16,
            post_window: 16,
            b_total_frames: b_mono.len(),
            repeat_window_frames: 16,
            repeat_penalty_weight: 1.0,
        };
        let waveform_off = WaveformSeamContext {
            repeat_penalty_weight: 0.0,
            ..waveform_penalized
        };
        let nominal_start = 100usize;
        let nominal_end = 140usize;
        let with_penalty = unified_fit_score_with_repeat(
            fit_candidate(0.9, 0.9, 0.2, nominal_start, nominal_end, nominal_start, nominal_end),
            &params,
            weights,
            &waveform_penalized,
            None,
            RepeatPenaltySource::PerCandidate,
            &CandidateTimers::default(),
        );
        let without_penalty = unified_fit_score_with_repeat(
            fit_candidate(0.9, 0.9, 0.2, nominal_start, nominal_end, nominal_start, nominal_end),
            &params,
            weights,
            &waveform_off,
            None,
            RepeatPenaltySource::PerCandidate,
            &CandidateTimers::default(),
        );
        assert!(
            without_penalty > with_penalty,
            "repeat penalty should lower score (with={with_penalty}, without={without_penalty})"
        );
    }

    #[test]
    fn repeat_penalty_downranks_speech_in_fill_tail_on_one_second_gap() {
        use crate::domain::gap_structure::StructureMatchParams;

        let gap_frames = 48_000usize;
        let seam_window = 12_000usize;
        let border_frames = 96_000usize;
        let a_post: Vec<f64> = (0..seam_window)
            .map(|i| (i as f64 * 0.12).sin())
            .collect();
        let a_pre: Vec<f64> = vec![0.05; seam_window];

        let mut b_speech = vec![0.02f64; gap_frames + seam_window];
        b_speech[gap_frames - seam_window..gap_frames].copy_from_slice(&a_post);
        b_speech[gap_frames..gap_frames + seam_window].copy_from_slice(&a_post);

        let mut b_music = vec![0.02f64; gap_frames + seam_window];
        b_music[gap_frames..gap_frames + seam_window].copy_from_slice(&a_post);

        let params = StructureMatchParams {
            gap_frames,
            bin_frames: 2_400,
            search_radius_frames: 4_800,
            fill_length_slack_frames: 0,
            max_fine_adjustment_frames: 0,
            silence_peak_fraction: 0.01,
            absolute_silence_rms: 0.0,
        };
        let weights = UnifiedFitWeights::default();
        let nominal_start = 0usize;
        let nominal_end = gap_frames;

        let score_for = |b_mono: &[f64], wave_min: f64| {
            let b_ch = vec![b_mono.to_vec()];
            let templates = SeamTemplates {
                a_pre: &a_pre,
                a_post: &a_post,
                a_pre_ch: std::slice::from_ref(&a_pre),
                a_post_ch: std::slice::from_ref(&a_post),
                b_mono,
                b_ch: &b_ch,
            };
            let waveform = WaveformSeamContext {
                templates: &templates,
                gap_frames,
                pre_window: seam_window,
                post_window: seam_window,
                b_total_frames: b_mono.len(),
                repeat_window_frames: border_frames,
                repeat_penalty_weight: 0.4,
            };
            unified_fit_score_with_repeat(
                fit_candidate(0.85, 0.95, wave_min, nominal_start, nominal_end, nominal_start, nominal_end),
                &params,
                weights,
                &waveform,
                None,
                RepeatPenaltySource::PerCandidate,
                &CandidateTimers::default(),
            )
        };

        let speech_score = score_for(&b_speech, 0.31);
        let music_score = score_for(&b_music, 0.31);
        assert!(
            music_score > speech_score,
            "music-only fill should outrank speech-in-tail (music={music_score}, speech={speech_score})"
        );
    }
}
