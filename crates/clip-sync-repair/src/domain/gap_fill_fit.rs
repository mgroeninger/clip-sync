//! Waveform seam search and unified structure+waveform fit (Phase A/B/C).

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
use crate::domain::policies::{
    fill_repeat_correlations, fill_seam_correlations, fill_splice_seam_correlations_interleaved,
    interleaved_to_mono, BorderSeamTemplates, FillAlignment, SeamPlacement, SeamTemplates,
    SpliceSeamContext,
};

const SCORE_TIE_EPSILON: f64 = 1e-9;
const LATE_START_PENALTY: f64 = 0.08;
const REPEAT_CORR_THRESHOLD: f64 = 0.55;
const REPEAT_SEAM_WEAK: f64 = 0.45;
const REPEAT_SUM_CEILING: f64 = 1.1;

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
/// Abstains (`NoOpinion`) when `!verdict.informative` — returns `pearson` unchanged.
pub fn apply_residual_to_confidence(
    pearson: Result<FillConfidence, f64>,
    verdict: &SeamResidualVerdict,
    margin_db: f64,
    rescue_enabled: bool,
) -> Result<FillConfidence, ResidualGateError> {
    if !verdict.informative {
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
    pub b_samples: &'a [i16],
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

fn repeat_penalty_at_placement(
    ctx: &WaveformSeamContext<'_>,
    start: usize,
    wave_min: f64,
) -> f64 {
    if ctx.repeat_penalty_weight <= 0.0 || ctx.repeat_window_frames == 0 {
        return 0.0;
    }
    let placement = SeamPlacement {
        start,
        gap_frames: ctx.gap_frames,
        pre_window: ctx.pre_window,
        post_window: ctx.post_window,
    };
    let (repeat_pre, repeat_post) = fill_repeat_correlations(
        ctx.templates,
        placement,
        ctx.repeat_window_frames,
    );
    let (pre_seam, post_seam) = fill_seam_correlations(ctx.templates, placement);
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

struct UnifiedFitCandidate {
    structure_pre: f64,
    structure_post: f64,
    wave_min: f64,
    placement: FillBracketPlacement,
}

fn unified_fit_score_with_repeat(
    candidate: UnifiedFitCandidate,
    params: &StructureMatchParams,
    weights: UnifiedFitWeights,
    waveform: &WaveformSeamContext<'_>,
    anchor_prior: Option<AnchorSearchPrior>,
) -> f64 {
    let mut score = unified_fit_score(
        candidate.structure_pre,
        candidate.structure_post,
        candidate.wave_min,
        candidate.placement,
        params,
        weights,
    );
    if let Some(prior) = anchor_prior {
        score -= prior.penalty_at_start(candidate.placement.start);
    }
    let wf = weights.normalized().waveform_weight;
    if wf > 0.0 && score.is_finite() {
        score -= waveform.repeat_penalty_weight
            * wf
            * repeat_penalty_at_placement(waveform, candidate.placement.start, candidate.wave_min);
    }
    score
}

pub(crate) fn waveform_min_at_start(ctx: &WaveformSeamContext<'_>, start: usize) -> f64 {
    if !placement_in_bounds(
        start,
        ctx.gap_frames,
        ctx.pre_window,
        ctx.post_window,
        ctx.b_total_frames,
    ) {
        return f64::NEG_INFINITY;
    }
    let (pre, post) = fill_seam_correlations(
        ctx.templates,
        SeamPlacement {
            start,
            gap_frames: ctx.gap_frames,
            pre_window: ctx.pre_window,
            post_window: ctx.post_window,
        },
    );
    pre.min(post)
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
    match_gap_fill_unified_in_b_with_timeline(
        input,
        params,
        weights,
        &structure_timeline,
        None,
    )
}

/// Like [`match_gap_fill_unified_in_b`] but reuses a pre-built structure timeline (joint grid perf).
pub(crate) fn match_gap_fill_unified_in_b_with_timeline(
    input: &UnifiedFillSearchInput<'_>,
    params: &StructureMatchParams,
    weights: UnifiedFitWeights,
    structure_timeline: &StructureTimeline<'_>,
    anchor_prior: Option<AnchorSearchPrior>,
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

    let search = UnifiedSearchCtx {
        timeline: structure_timeline,
        waveform: input.waveform,
        params,
        weights,
        anchor_prior,
        nominal_start: input.nominal_fill_start,
        nominal_end: input.nominal_fill_end,
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

    let (wave_pre, wave_post) = waveform_seams_at_start(input.waveform, best_start);

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

fn waveform_seams_at_start(ctx: &WaveformSeamContext<'_>, start: usize) -> (f64, f64) {
    if !placement_in_bounds(
        start,
        ctx.gap_frames,
        ctx.pre_window,
        ctx.post_window,
        ctx.b_total_frames,
    ) {
        return (f64::NEG_INFINITY, f64::NEG_INFINITY);
    }
    fill_seam_correlations(
        ctx.templates,
        SeamPlacement {
            start,
            gap_frames: ctx.gap_frames,
            pre_window: ctx.pre_window,
            post_window: ctx.post_window,
        },
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
    } = *ctx;
    let search_min = nominal_start.saturating_sub(params.search_radius_frames);
    let search_max = (nominal_start + params.search_radius_frames).min(total_frames);
    let span = search_max.saturating_sub(search_min);
    let coarse_step = search_coarse_step(params.bin_frames, span);

    let mut best_start = nominal_start;
    let mut best_score = f64::NEG_INFINITY;

    let consider = |start: usize, best_start: &mut usize, best_score: &mut f64| {
        if start < pre_span || start > search_max {
            return;
        }
        let candidate_end = (start + params.gap_frames).min(total_frames);
        if candidate_end + post_span > total_frames {
            return;
        }
        let pre_score = score_pre_for_signature(signature, timeline, start, params);
        let post_score = score_post_for_signature(signature, timeline, candidate_end, params);
        let wave_min = waveform_min_at_start(waveform, start);
        let score = unified_fit_score_with_repeat(
            UnifiedFitCandidate {
                structure_pre: pre_score,
                structure_post: post_score,
                wave_min,
                placement: fill_bracket_placement(start, candidate_end, nominal_start, nominal_end),
            },
            params,
            weights,
            waveform,
            anchor_prior,
        );
        let better = score > *best_score + SCORE_TIE_EPSILON
            || (score >= *best_score - SCORE_TIE_EPSILON
                && prefer_start(start, *best_start, nominal_start));
        if better {
            *best_score = score;
            *best_start = start;
        }
    };

    if nominal_start >= pre_span {
        consider(nominal_start, &mut best_start, &mut best_score);
    }

    let mut start = search_min;
    while start <= search_max {
        consider(start, &mut best_start, &mut best_score);
        start = start.saturating_add(coarse_step);
    }

    if !best_score.is_finite() {
        return None;
    }

    let refine_min = best_start.saturating_sub(coarse_step).max(search_min);
    let refine_max = (best_start + coarse_step).min(search_max);
    for start in refine_min..=refine_max {
        consider(start, &mut best_start, &mut best_score);
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
        let pre_score = score_pre_for_signature(signature, timeline, fill_start, params);
        let post_score = score_post_for_signature(signature, timeline, end, params);
        let wave_min = waveform_min_at_start(waveform, fill_start);
        let score = unified_fit_score_with_repeat(
            UnifiedFitCandidate {
                structure_pre: pre_score,
                structure_post: post_score,
                wave_min,
                placement: fill_bracket_placement(fill_start, end, fill_start, nominal_end),
            },
            params,
            weights,
            waveform,
            None,
        );
        let better = score > *best_score + SCORE_TIE_EPSILON
            || (score >= *best_score - SCORE_TIE_EPSILON
                && prefer_end(end, *best_end, nominal_end));
        if better {
            *best_score = score;
            *best_end = end;
        }
    };

    consider(best_end, &mut best_end, &mut best_score);

    let mut end = end_min;
    while end <= end_max {
        consider(end, &mut best_end, &mut best_score);
        end = end.saturating_add(coarse_step);
    }

    if !best_score.is_finite() {
        return None;
    }

    let refine_min = best_end.saturating_sub(coarse_step).max(end_min);
    let refine_max = (best_end + coarse_step).min(end_max);
    for end in refine_min..=refine_max {
        consider(end, &mut best_end, &mut best_score);
    }

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
    } = *ctx;
    if params.max_fine_adjustment_frames == 0 {
        return start;
    }

    let fill_len = end.saturating_sub(start).max(1);
    let mut best_start = start;
    let mut best_score = f64::NEG_INFINITY;

    for delta in -(params.max_fine_adjustment_frames as i64)
        ..=(params.max_fine_adjustment_frames as i64)
    {
        let candidate = start as i64 + delta;
        if candidate < 0 {
            continue;
        }
        let candidate = candidate as usize;
        let candidate_end = candidate + fill_len;
        let pre_score = score_pre_for_signature(signature, timeline, candidate, params);
        let post_score = score_post_for_signature(signature, timeline, candidate_end, params);
        let wave_min = waveform_min_at_start(waveform, candidate);
        let score = unified_fit_score_with_repeat(
            UnifiedFitCandidate {
                structure_pre: pre_score,
                structure_post: post_score,
                wave_min,
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
        );
        let better = score > best_score + SCORE_TIE_EPSILON
            || (score >= best_score - SCORE_TIE_EPSILON
                && prefer_start(candidate, best_start, nominal_start));
        if better {
            best_score = score;
            best_start = candidate;
        }
    }

    best_start
}

/// Structure-only match for regression comparison in tests.
pub fn match_gap_structure_only_in_b(
    signature: &GapContextSignature,
    b_samples: &[i16],
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
pub fn fit_fill_to_gap_frames(samples: &[i16], channels: usize, target_frames: usize) -> Vec<i16> {
    let channels = channels.max(1);
    let source_frames = samples.len() / channels;
    if source_frames == target_frames {
        return samples.to_vec();
    }
    if source_frames == 0 {
        return vec![0i16; target_frames * channels];
    }

    if source_frames > target_frames {
        return samples[..target_frames * channels].to_vec();
    }

    let mut out = vec![0i16; target_frames * channels];
    out[..samples.len()].copy_from_slice(samples);
    out
}

fn placement_repeat_post(
    fill_interleaved: &[i16],
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
                    .map(|&s| f64::from(s))
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
    fill_interleaved: &[i16],
    extension_interleaved: &[i16],
    channels: usize,
    gap_frames: usize,
    borders: &BorderSeamTemplates<'_>,
    repeat_window_frames: usize,
    seam_ctx: SpliceSeamContext<'_>,
) -> Vec<i16> {
    let channels = channels.max(1);
    let source_frames = fill_interleaved.len() / channels;
    debug_assert!(source_frames < gap_frames);

    let ext_frames = extension_interleaved.len() / channels;
    let mut out = fill_interleaved.to_vec();
    let score_at_length = |samples: &[i16]| {
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
    fill_interleaved: &[i16],
    extension_interleaved: &[i16],
    channels: usize,
    gap_frames: usize,
    borders: &BorderSeamTemplates<'_>,
    repeat_window_frames: usize,
    seam_ctx: SpliceSeamContext<'_>,
) -> Vec<i16> {
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
    fill_interleaved: &[i16],
    channels: usize,
    gap_frames: usize,
    borders: &BorderSeamTemplates<'_>,
    seam_ctx: SpliceSeamContext<'_>,
) -> Vec<i16> {
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
        }
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

    fn no_seam_ctx<'a>(a_samples: &'a [i16]) -> SpliceSeamContext<'a> {
        SpliceSeamContext {
            seam_cf: 0,
            gap_start_frame: 0,
            gap_end_frame: 0,
            a_samples,
            channels: 1,
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
        UnifiedFitCandidate {
            structure_pre,
            structure_post,
            wave_min,
            placement: fill_bracket_placement(start, end, nominal_start, nominal_end),
        }
    }

    use crate::domain::policies::{interleaved_to_channels, interleaved_to_mono};

    fn sine_frame(frame: usize, rate: u32) -> i16 {
        let t = frame as f64 / f64::from(rate);
        (f64::sin(2.0 * std::f64::consts::PI * 440.0 * t) * 10_000.0) as i16
    }

    fn build_b_haystack_with_dropout_offset(
        rate: u32,
        pre_frames: usize,
        lead_in_frames: usize,
        gap_frames: usize,
        post_frames: usize,
    ) -> (Vec<i16>, usize) {
        let gap_start = pre_frames + lead_in_frames;
        let total = gap_start + gap_frames + post_frames;
        let mut samples = Vec::with_capacity(total);
        for frame in 0..total {
            let in_gap = frame >= gap_start && frame < gap_start + gap_frames;
            let sample = if in_gap {
                0i16
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
            .map(|i| f64::from(sine_frame(gap_start_on_a - pre_len + i, rate)))
            .collect();
        let a_post: Vec<f64> = (0..post_len)
            .map(|i| f64::from(sine_frame(gap_start_on_a + gap_frames + i, rate)))
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
    fn waveform_search_step_is_one_for_small_radius() {
        assert_eq!(waveform_search_step(100), 1);
        assert!(waveform_search_step(10_000) > 1);
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
        );
        assert!(
            without > base,
            "repeat penalty should lower score when fill duplicates borders (with={without}, base={base})"
        );
    }

    #[test]
    fn pick_fill_length_anchor_prefers_better_seam_end() {
        let channels = 1usize;
        let gap_frames = 4usize;
        let pre_w = 2usize;
        let post_w = 2usize;
        let a_pre = vec![0.1, -0.1];
        let a_post = vec![0.6, -0.4];
        let to_i16 = |v: f64| (v * 10_000.0).round() as i16;
        let post: Vec<i16> = a_post.iter().map(|&v| to_i16(v)).collect();
        let fill = vec![0i16, 0, 0, 0, post[0], post[1]];
        let seam_ctx = no_seam_ctx(&[]);
        let picked = pick_fill_length_anchor(
            &fill,
            channels,
            gap_frames,
            &test_borders(&a_pre, &a_post, &[], &[], pre_w, post_w),
            seam_ctx,
        );
        assert_eq!(picked, vec![0, 0, post[0], post[1]]);
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
        let to_i16 = |v: f64| (v * 10_000.0).round() as i16;
        let pre: Vec<i16> = a_pre.iter().map(|&v| to_i16(v)).collect();
        let post: Vec<i16> = a_post.iter().map(|&v| to_i16(v)).collect();
        let junk = vec![0i16, 0, 0, 0];
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
        assert_eq!(picked, vec![0, 0, post[0], post[1]]);
    }

    #[test]
    fn score_extend_stops_before_extension_degrades_post_seam() {
        let channels = 1usize;
        let gap_frames = 5usize;
        let pre_w = 2usize;
        let post_w = 2usize;
        let to_i16 = |v: f64| (v * 10_000.0).round() as i16;
        let a_pre = vec![0.9, -0.9];
        let a_post = vec![0.8, 0.7];
        let fill = vec![to_i16(0.9), to_i16(-0.9)];
        let extension = vec![
            to_i16(0.75),
            to_i16(0.8),
            to_i16(-1.0),
            to_i16(-1.0),
        ];

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

        let extension_frames_used = |samples: &[i16]| {
            let mut end = samples.len();
            while end > fill.len() && samples[end - 1] == 0 {
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
        let to_i16 = |v: f64| (v * 10_000.0).round() as i16;
        let a_pre = vec![0.9, -0.9];
        let a_post = vec![0.8, 0.7];
        let fill = vec![to_i16(0.9), to_i16(-0.9)];
        let extension = vec![
            to_i16(0.75),
            to_i16(0.8),
            to_i16(-1.0),
            to_i16(-1.0),
        ];
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
        );
        let without_penalty = unified_fit_score_with_repeat(
            fit_candidate(0.9, 0.9, 0.2, nominal_start, nominal_end, nominal_start, nominal_end),
            &params,
            weights,
            &waveform_off,
            None,
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
