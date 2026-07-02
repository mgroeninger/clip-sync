//! Structure match + seam gate for one A gap bracket (supports boundary extension retries).

use clip_sync::MultiChannelPcm;

use crate::domain::fill_mode::FillMode;
use crate::domain::repair_profile::FitBoundarySearch;
use crate::domain::gap_fill_fit::{
    anchor_trust_applies, boundary_search_step_frames, apply_residual_to_confidence,
    classify_fill_waveform_confidence, fit_anchor_candidate_ranking_score,
    fit_candidate_ranking_score,
    match_gap_fill_unified_in_b_with_timeline, FillConfidence, ResidualGateError,
    UnifiedFillMatch, UnifiedFillSearchInput, UnifiedFitWeights, WaveformSeamContext,
};
use crate::domain::residual_gate::ResidualGateMode;
use crate::domain::patch_anchor::AnchorSearchPrior;
use crate::domain::patch_result::{SeamScoreAttempt, SeamScoreSource};
use crate::domain::gap_seam_extend::{
    post_seam_extension_candidate, pre_seam_extension_candidate,
    short_gap_one_strong_seam_passes,
};
use crate::domain::gap_structure::{self, StructureMatchParams};
use crate::domain::gap_signature::{
    build_gap_signature, GapSignature, GapSignatureMode, StructureTimeline,
};
use crate::domain::gap_anchor_seam::{
    list_anchor_candidates_a, list_feasible_anchor_brackets, anchor_bracket_both_matchable,
    should_run_anchor_seam, AnchorBracket, AnchorSeamMode, AnchorSeamParams,
};
use crate::domain::gap_energy::EnergyTimeline;
use crate::domain::policies::{self, FillAlignment, GapBorderSpec, RefinedGapFrames};

use crate::application::fit_routing;

/// Pearson floor used when partial structure trust softens the waveform gate.
pub(crate) const PARTIAL_WAVEFORM_MIN_CORRELATION: f32 = 0.12;

/// Outcome of structure match and optional waveform seam gate.
#[derive(Clone)]
pub(crate) struct SeamGateOutcome {
    pub refined: RefinedGapFrames,
    pub alignment: FillAlignment,
    pub report_pre: f64,
    pub report_post: f64,
    pub structure_trusted: bool,
    pub structure_start_frame: usize,
    pub gap_frames: usize,
    pub confidence: FillConfidence,
    pub gap_start_adjust_frames: i64,
    pub gap_end_adjust_frames: i64,
    /// True when fit mode ran the joint A-boundary grid (not baseline-only short path).
    pub fit_used_boundary_grid: bool,
    /// Non-baseline grid cells evaluated when `fit_used_boundary_grid` (verbose diagnostics).
    pub fit_boundary_grid_cells: Option<u32>,
    /// B haystack duration in seconds (unified search window).
    pub fit_haystack_secs: f64,
    /// Residual/floor verdict (P1 report-only); `Some` when residual measurement is enabled.
    pub residual: Option<policies::SeamResidualVerdict>,
    /// True when the winning bracket came from editorial anchor search (not scan throat alone).
    pub anchor_seam_used: bool,
    /// Total frame movement from scan-refined baseline when `anchor_seam_used`.
    pub anchor_bracket_move_frames: usize,
    /// Structure-trusted patch at editorial anchors with weak throat Pearson (fit mode).
    pub anchor_trusted: bool,
}

/// Run-constant seam-gate inputs: built once per repair run (via [`SeamGateConfig::from_repair`])
/// and shared by reference across every gap's [`SeamGateParams`]. Holds tuning thresholds, modes,
/// and the frame counts that derive from run-level `secs × sample_rate` (no per-gap dependence).
/// The three `*_secs` fields feed [`derive_seam_gate_geometry`]'s per-gap frame math.
#[derive(Clone, Copy)]
#[doc(hidden)]
pub struct SeamGateConfig {
    pub channels: usize,
    pub sample_rate: u32,
    pub context_frames: usize,
    pub bin_frames: usize,
    pub border_standoff_frames: usize,
    pub search_radius_frames: usize,
    pub fill_length_slack_frames: usize,
    pub max_extend_frames: usize,
    pub step_frames: usize,
    pub residual_max_lag_frames: i64,
    pub normalize_window_secs: f64,
    pub min_border_discovery_secs: f64,
    pub fill_seam_search_secs: f64,
    pub silence_peak_fraction: f32,
    pub absolute_silence_rms: f32,
    pub min_structure_match_score: f32,
    pub strong_structure_trust: f64,
    pub disable_structure_trust: bool,
    pub partial_structure_waveform_soften: f64,
    pub min_fill_correlation: f32,
    pub short_gap_mean_correlation_secs: f64,
    pub short_gap_one_strong_seam_fallback: bool,
    pub fill_mode: FillMode,
    pub fill_fit_structure_weight: f64,
    pub fill_fit_waveform_weight: f64,
    pub fill_fit_nominal_bias_scale: f64,
    pub fill_fit_energy_nominal_bias_scale: f64,
    pub fill_fit_late_start_penalty_scale: f64,
    pub fill_marginal_margin: f32,
    pub fill_absolute_floor: f32,
    pub fill_repeat_penalty_weight: f64,
    pub gap_end_extend_on_post_seam_fail: bool,
    pub gap_start_extend_on_pre_seam_fail: bool,
    pub gap_signature_mode: GapSignatureMode,
    pub fit_boundary_search: FitBoundarySearch,
    pub anchor_seam_mode: AnchorSeamMode,
    pub max_anchor_bracket_secs: f64,
    pub max_anchors_per_side: usize,
    pub anchor_seam_min_prominence: f32,
    pub anchor_matchability: crate::domain::gap_anchor_seam::AnchorMatchabilityParams,
    /// P1 report-only: compute the residual/floor verdict per gap and attach it to the outcome/JSON.
    pub measure_residual: bool,
    /// Residual headroom gate mode (`off` = no gating; measurement still obeys `measure_residual`).
    pub residual_gate: ResidualGateMode,
    pub residual_floor_ok_db: f64,
    pub residual_headroom_margin_db: f64,
}

/// Per-gap seam-gate geometry: rebuilt for each gap (the audio borrows plus the B window and the
/// two `gap_frames`-derived frame counts `seam_gate_frames`/`border_frames`). See
/// [`derive_seam_gate_geometry`].
#[derive(Clone, Copy)]
#[doc(hidden)]
pub struct SeamGateGeometry<'a> {
    pub a_pcm: &'a MultiChannelPcm,
    pub b_samples: &'a [f32],
    pub b_extract_start_secs: f64,
    pub refined_b_start_secs: f64,
    pub refined_b_end_secs: f64,
    pub seam_gate_frames: usize,
    pub border_frames: usize,
    pub anchor_search_prior: Option<AnchorSearchPrior>,
}

#[doc(hidden)]
pub struct SeamGateParams<'a> {
    pub cfg: &'a SeamGateConfig,
    pub geom: SeamGateGeometry<'a>,
}

/// Build per-gap [`SeamGateGeometry`] from run-constant `cfg` + this gap's window. Computes
/// `seam_gate_frames`/`border_frames` from `gap_frames` so the oracle and production share one
/// path (see docs/TEMP-w5-anchor-rescue-diag-plan.md Phase 0).
#[allow(clippy::too_many_arguments)]
#[doc(hidden)]
pub fn derive_seam_gate_geometry<'a>(
    cfg: &SeamGateConfig,
    a_pcm: &'a MultiChannelPcm,
    b_samples: &'a [f32],
    b_extract_start_secs: f64,
    refined_b_start_secs: f64,
    refined_b_end_secs: f64,
    gap_frames: usize,
    anchor_search_prior: Option<AnchorSearchPrior>,
) -> SeamGateGeometry<'a> {
    let correlate_frames = crate::application::patch_audio::correlate_frames_for_gap(
        cfg.normalize_window_secs,
        cfg.min_border_discovery_secs,
        gap_frames,
        cfg.sample_rate,
    );
    let seam_gate_frames = crate::application::patch_audio::seam_gate_frames_for(
        correlate_frames,
        cfg.fill_seam_search_secs,
        cfg.sample_rate,
    );
    let border_frames =
        crate::application::patch_audio::border_frames_from_secs(cfg.normalize_window_secs, cfg.sample_rate)
            .min(correlate_frames);
    SeamGateGeometry {
        a_pcm,
        b_samples,
        b_extract_start_secs,
        refined_b_start_secs,
        refined_b_end_secs,
        seam_gate_frames,
        border_frames,
        anchor_search_prior,
    }
}

#[derive(Debug, Clone, Copy)]
#[doc(hidden)]
pub enum SeamGateFailure {
    StructureAlignmentFailed,
    StructureBelowThreshold {
        pre: f64,
        post: f64,
    },
    WaveformBelowThreshold {
        pre: f64,
        post: f64,
        min: f32,
        best_attempt: Option<SeamScoreAttempt>,
    },
    ResidualHeadroomExceeded {
        pre: f64,
        post: f64,
        residual: policies::SeamResidualVerdict,
        margin_db: f64,
    },
}

pub(crate) fn evaluate_seam_gate(
    refined: RefinedGapFrames,
    params: &SeamGateParams<'_>,
) -> Result<SeamGateOutcome, SeamGateFailure> {
    if params.cfg.fill_mode == FillMode::Fit {
        evaluate_seam_gate_fit_joint(refined, params)
    } else {
        evaluate_seam_gate_legacy(refined, params)
    }
}

fn waveform_below_threshold(pre: f64, post: f64, min: f32) -> SeamGateFailure {
    SeamGateFailure::WaveformBelowThreshold {
        pre,
        post,
        min,
        best_attempt: None,
    }
}

fn consider_waveform_attempt(
    best: &mut Option<SeamScoreAttempt>,
    pre: f64,
    post: f64,
    source: SeamScoreSource,
) {
    let min_score = pre.min(post);
    if !min_score.is_finite() {
        return;
    }
    let better = best
        .map(|b| min_score > b.min_pearson() + 1e-9)
        .unwrap_or(true);
    if better {
        *best = Some(SeamScoreAttempt {
            pre_correlation: pre,
            post_correlation: post,
            source,
        });
    }
}

fn outcome_score_source(outcome: &SeamGateOutcome, baseline: RefinedGapFrames) -> SeamScoreSource {
    if outcome.anchor_seam_used {
        SeamScoreSource::Anchor
    } else if outcome.refined == baseline {
        SeamScoreSource::Baseline
    } else {
        SeamScoreSource::Grid
    }
}

fn enrich_waveform_failure(
    fail: SeamGateFailure,
    best_waveform: &Option<SeamScoreAttempt>,
) -> SeamGateFailure {
    let SeamGateFailure::WaveformBelowThreshold { pre, post, min, .. } = fail else {
        return fail;
    };
    let reported_min = pre.min(post);
    let best_attempt = (*best_waveform).filter(|b| b.min_pearson() > reported_min + 1e-9);
    SeamGateFailure::WaveformBelowThreshold {
        pre,
        post,
        min,
        best_attempt,
    }
}

#[derive(Clone)]
struct FitJointCandidate {
    outcome: SeamGateOutcome,
    ranking_score: f64,
    boundary_move: usize,
}

impl FitJointCandidate {
    /// Project to the pure routing inputs (`fit_routing`); identity/residual/anchor flags stay here.
    fn score(&self) -> fit_routing::CandidateScore {
        fit_routing::CandidateScore {
            confidence: self.outcome.confidence,
            boundary_move: self.boundary_move,
            ranking_score: self.ranking_score,
        }
    }
}

/// Reused B haystack mono, channels, and structure timelines for joint-grid candidates.
#[doc(hidden)]
pub struct FitHaystackCache {
    b_mono: Vec<f64>,
    b_ch: Vec<Vec<f64>>,
    bool_timeline: gap_structure::ActivityTimeline,
    energy_timeline: EnergyTimeline,
}

impl FitHaystackCache {
    pub(crate) fn build(params: &SeamGateParams<'_>) -> Self {
        let channels = params.cfg.channels.max(1);
        let total_frames = params.geom.b_samples.len() / channels;
        let bin_frames = params.cfg.bin_frames.max(1);
        Self {
            b_mono: policies::interleaved_to_mono(params.geom.b_samples, channels),
            b_ch: policies::interleaved_to_channels(params.geom.b_samples, channels),
            bool_timeline: gap_structure::ActivityTimeline::build(
                params.geom.b_samples,
                channels,
                total_frames,
                bin_frames,
                params.cfg.silence_peak_fraction,
                params.cfg.absolute_silence_rms,
            ),
            energy_timeline: EnergyTimeline::build(
                params.geom.b_samples,
                channels,
                total_frames,
                bin_frames,
                params.cfg.silence_peak_fraction,
                params.cfg.absolute_silence_rms,
            ),
        }
    }

    fn structure_timeline<'a>(&'a self, signature: &GapSignature) -> StructureTimeline<'a> {
        match signature {
            GapSignature::Bool(_) => StructureTimeline::Bool(&self.bool_timeline),
            GapSignature::Energy(_) => StructureTimeline::Energy(&self.energy_timeline),
        }
    }
}

/// Non-audio knobs the fit-joint orchestration reads, so number tests can drive
/// `evaluate_seam_gate_fit_joint_core` without building a `SeamGateParams` (step 5, plan §9).
#[derive(Debug, Clone, Copy)]
struct FitJointConfig {
    fit_boundary_search: FitBoundarySearch,
    anchor_seam_mode: AnchorSeamMode,
    /// Grid sweep bounds (frame arithmetic; audio-independent).
    start_min: usize,
    end_max: usize,
    step: usize,
    /// B haystack seconds — stamped on the outcome as metadata.
    haystack_secs: f64,
}

/// The audio-touching operations the fit-joint orchestration needs, behind a seam so a fake can
/// drive the real precedence loop with scripted numbers (plan §9). The real impl ([`AudioFitSource`])
/// wraps the existing audio functions; production behaviour is unchanged.
trait FitCandidateSource {
    /// Score one A-side seam bracket → Pearson-confidence outcome + ranking, or a gate failure.
    fn score(
        &mut self,
        refined: RefinedGapFrames,
        anchor_seam_bracket: bool,
    ) -> Result<(SeamGateOutcome, f64), SeamGateFailure>;
    /// Editorial anchor brackets to try (empty = gate closed / mode off / none feasible).
    fn anchor_brackets(&mut self, baseline_pre: f64, baseline_post: f64) -> Vec<AnchorBracket>;
    /// Apply the residual/floor verdict at selection (identity when residual is off; may `Err` on veto).
    fn finalize_residual(
        &mut self,
        outcome: SeamGateOutcome,
    ) -> Result<SeamGateOutcome, SeamGateFailure>;
}

/// Production [`FitCandidateSource`]: scores against real A/B PCM via the existing functions.
struct AudioFitSource<'a> {
    params: &'a SeamGateParams<'a>,
    cache: &'a FitHaystackCache,
    baseline: RefinedGapFrames,
}

impl FitCandidateSource for AudioFitSource<'_> {
    fn score(
        &mut self,
        refined: RefinedGapFrames,
        anchor_seam_bracket: bool,
    ) -> Result<(SeamGateOutcome, f64), SeamGateFailure> {
        evaluate_seam_gate_fit_candidate(
            refined,
            self.baseline,
            self.params,
            self.cache,
            anchor_seam_bracket,
        )
    }

    fn anchor_brackets(&mut self, baseline_pre: f64, baseline_post: f64) -> Vec<AnchorBracket> {
        if self.params.cfg.anchor_seam_mode == AnchorSeamMode::Off {
            return Vec::new();
        }
        let anchor_params = anchor_seam_gate_params(self.params, self.baseline);
        let baseline_signature = build_gap_signature(
            &self.params.geom.a_pcm.samples,
            self.params.cfg.channels,
            self.baseline.start_frame,
            self.baseline.end_frame,
            self.params.cfg.context_frames,
            &anchor_params.structure,
            self.params.cfg.gap_signature_mode,
        );
        if !should_run_anchor_seam(
            self.params.cfg.anchor_seam_mode,
            baseline_pre,
            baseline_post,
            self.params.cfg.min_fill_correlation,
            self.params.cfg.fill_marginal_margin,
            baseline_signature.has_anchor_seam_contour(),
        ) {
            return Vec::new();
        }
        let candidates = list_anchor_candidates_a(
            &self.params.geom.a_pcm.samples,
            self.params.cfg.channels,
            self.baseline,
            &anchor_params,
        );
        list_feasible_anchor_brackets(&candidates, self.baseline, &anchor_params)
    }

    fn finalize_residual(
        &mut self,
        outcome: SeamGateOutcome,
    ) -> Result<SeamGateOutcome, SeamGateFailure> {
        finalize_fit_outcome_residual(outcome, self.baseline, self.params, self.cache)
    }
}

fn record_fit_joint_candidate_to_pool(
    pool: &mut Vec<FitJointCandidate>,
    recorded_failure: &mut Option<SeamGateFailure>,
    best_waveform: &mut Option<SeamScoreAttempt>,
    refined: RefinedGapFrames,
    baseline: RefinedGapFrames,
    source: &mut dyn FitCandidateSource,
    anchor_seam_bracket: bool,
    score_source: SeamScoreSource,
) {
    match source.score(refined, anchor_seam_bracket) {
        Ok((outcome, ranking_score)) => {
            consider_waveform_attempt(
                best_waveform,
                outcome.report_pre,
                outcome.report_post,
                score_source,
            );
            let boundary_move = baseline.start_frame.abs_diff(refined.start_frame)
                + baseline.end_frame.abs_diff(refined.end_frame);
            pool.push(FitJointCandidate {
                outcome,
                ranking_score,
                boundary_move,
            });
        }
        Err(SeamGateFailure::WaveformBelowThreshold { pre, post, min, .. }) => {
            consider_waveform_attempt(best_waveform, pre, post, score_source);
            if recorded_failure.is_none() {
                *recorded_failure = Some(waveform_below_threshold(pre, post, min));
            }
        }
        Err(fail @ SeamGateFailure::ResidualHeadroomExceeded { .. }) => {
            if recorded_failure.is_none() {
                *recorded_failure = Some(fail);
            }
        }
        Err(other) => {
            if pool.is_empty() && recorded_failure.is_none() {
                *recorded_failure = Some(other);
            }
        }
    }
}

/// The border spec for a gap at `refined` frames (shared by seam scoring, residual measurement, and
/// channel selection so they all see the same border window).
fn gap_border_spec(params: &SeamGateParams<'_>, refined: RefinedGapFrames) -> GapBorderSpec {
    GapBorderSpec {
        gap_start_frame: refined.start_frame,
        gap_end_frame: refined.end_frame,
        border_frames: params.geom.border_frames,
        border_standoff_frames: params.cfg.border_standoff_frames,
        silence_peak_fraction: params.cfg.silence_peak_fraction,
        absolute_rms_floor: params.cfg.absolute_silence_rms,
    }
}

fn fit_residual_geometry(
    refined: RefinedGapFrames,
    baseline: RefinedGapFrames,
    params: &SeamGateParams<'_>,
) -> (usize, usize, usize) {
    let border_spec = gap_border_spec(params, refined);
    let (a_pre_border, a_post_border) =
        policies::border_templates_for_gap(&params.geom.a_pcm.samples, params.cfg.channels, &border_spec);
    let start_delta_secs = (refined.start_frame as i64 - baseline.start_frame as i64) as f64
        / params.cfg.sample_rate as f64;
    let refined_b_start_secs = params.geom.refined_b_start_secs + start_delta_secs;
    let offset_nominal_start = ((refined_b_start_secs - params.geom.b_extract_start_secs)
        * params.cfg.sample_rate as f64)
        .round() as usize;
    let waveform_gate_frames = params
        .geom
        .seam_gate_frames
        .min(a_pre_border.len().max(1));
    let post_gate_frames = seam_post_gate_frames(params.geom.seam_gate_frames, a_post_border.len());
    (
        offset_nominal_start,
        waveform_gate_frames,
        post_gate_frames,
    )
}

fn log_residual_verdict_debug(
    params: &SeamGateParams<'_>,
    alignment_start_frame: usize,
    offset_nominal_start: usize,
    pre_corr: f64,
    post_corr: f64,
    verdict: &policies::SeamResidualVerdict,
) {
    tracing::debug!(
        start_frame = alignment_start_frame,
        nominal_start = offset_nominal_start,
        seam_pre = pre_corr,
        seam_post = post_corr,
        chosen_pre_db = verdict.chosen_pre_db,
        chosen_post_db = verdict.chosen_post_db,
        floor_pre_db = verdict.floor_pre_db,
        floor_post_db = verdict.floor_post_db,
        headroom_db = verdict.worst_headroom_db(),
        informative = verdict.informative,
        residual_gate = ?params.cfg.residual_gate,
        "fill seam residual verdict"
    );
}

fn finalize_fit_outcome_residual(
    mut outcome: SeamGateOutcome,
    baseline: RefinedGapFrames,
    params: &SeamGateParams<'_>,
    cache: &FitHaystackCache,
) -> Result<SeamGateOutcome, SeamGateFailure> {
    if !want_residual_measurement(params) {
        return Ok(outcome);
    }
    let (offset_nominal_start, waveform_gate_frames, post_gate_frames) =
        fit_residual_geometry(outcome.refined, baseline, params);
    let residual = measure_fit_residual_verdict(
        params,
        cache,
        outcome.refined,
        outcome.structure_start_frame,
        offset_nominal_start,
        waveform_gate_frames,
        post_gate_frames,
    );
    if let Some(ref verdict) = residual {
        log_residual_verdict_debug(
            params,
            outcome.structure_start_frame,
            offset_nominal_start,
            outcome.report_pre,
            outcome.report_post,
            verdict,
        );
    }
    if !params.cfg.residual_gate.is_active() {
        outcome.residual = residual;
        return Ok(outcome);
    }
    let verdict = residual.ok_or(SeamGateFailure::StructureAlignmentFailed)?;
    let pearson = classify_fill_waveform_confidence(
        outcome.report_pre,
        outcome.report_post,
        params.cfg.min_fill_correlation,
        params.cfg.fill_marginal_margin,
        params.cfg.fill_absolute_floor,
    );
    let confidence = match apply_residual_to_confidence(
        pearson,
        &verdict,
        params.cfg.residual_headroom_margin_db,
        params.cfg.residual_gate.rescue_enabled(),
    ) {
        Ok(confidence) => confidence,
        Err(ResidualGateError::HeadroomExceeded { margin_db, .. }) => {
            return Err(SeamGateFailure::ResidualHeadroomExceeded {
                pre: outcome.report_pre,
                post: outcome.report_post,
                residual: verdict,
                margin_db,
            });
        }
        Err(ResidualGateError::PearsonBelowFloor(_)) => {
            return Err(waveform_below_threshold(
                outcome.report_pre,
                outcome.report_post,
                params.cfg.fill_absolute_floor,
            ));
        }
    };
    outcome.confidence = confidence;
    outcome.residual = Some(verdict);
    Ok(outcome)
}

/// E2: under `baseline_only`, accept baseline without grid — unless `force` anchor should run on marginal.
fn baseline_accept_without_grid(
    search: FitBoundarySearch,
    confidence: FillConfidence,
    anchor_seam_mode: AnchorSeamMode,
) -> bool {
    if anchor_seam_mode == AnchorSeamMode::Force && confidence == FillConfidence::Marginal {
        return false;
    }
    accepts_baseline_without_boundary_grid(search, confidence)
}

fn try_finalize_high_joint_candidate(
    candidate: FitJointCandidate,
    source: &mut dyn FitCandidateSource,
    haystack_secs: f64,
    grid_cells: Option<u32>,
) -> Option<SeamGateOutcome> {
    if candidate.outcome.confidence != FillConfidence::High {
        return None;
    }
    let mut outcome = source.finalize_residual(candidate.outcome).ok()?;
    if outcome.confidence != FillConfidence::High {
        return None;
    }
    outcome.fit_haystack_secs = haystack_secs;
    if let Some(cells) = grid_cells {
        outcome.fit_used_boundary_grid = true;
        outcome.fit_boundary_grid_cells = Some(cells);
    }
    Some(outcome)
}

/// E6: after the full boundary grid, pick the best Pearson-`High` by ranking (not first in walk order).
fn try_finalize_best_grid_high(
    pool: &[FitJointCandidate],
    source: &mut dyn FitCandidateSource,
    haystack_secs: f64,
    grid_cells: u32,
) -> Option<SeamGateOutcome> {
    let candidate = best_high_joint_candidate(pool)?.clone();
    try_finalize_high_joint_candidate(candidate, source, haystack_secs, Some(grid_cells))
}

fn select_joint_fit_winner_with_residual(
    mut pool: Vec<FitJointCandidate>,
    mut recorded_failure: Option<SeamGateFailure>,
    mut best_waveform: Option<SeamScoreAttempt>,
    baseline: RefinedGapFrames,
    source: &mut dyn FitCandidateSource,
    haystack_secs: f64,
    grid_cells: Option<u32>,
) -> Result<SeamGateOutcome, SeamGateFailure> {
    pool.sort_by(|a, b| fit_routing::winner_cmp(&a.score(), &b.score()));
    for candidate in pool {
        let score_source = outcome_score_source(&candidate.outcome, baseline);
        match source.finalize_residual(candidate.outcome) {
            Ok(mut outcome) => {
                outcome.fit_haystack_secs = haystack_secs;
                if let Some(cells) = grid_cells {
                    outcome.fit_used_boundary_grid = true;
                    outcome.fit_boundary_grid_cells = Some(cells);
                }
                return Ok(outcome);
            }
            Err(fail @ SeamGateFailure::ResidualHeadroomExceeded { .. }) => {
                if recorded_failure.is_none() {
                    recorded_failure = Some(fail);
                }
            }
            Err(SeamGateFailure::WaveformBelowThreshold { pre, post, min, .. }) => {
                consider_waveform_attempt(&mut best_waveform, pre, post, score_source);
                if recorded_failure.is_none() {
                    recorded_failure = Some(waveform_below_threshold(pre, post, min));
                }
            }
            Err(other) => return Err(other),
        }
    }
    Err(enrich_waveform_failure(
        recorded_failure.unwrap_or(SeamGateFailure::StructureAlignmentFailed),
        &best_waveform,
    ))
}

/// Count non-baseline joint A-boundary grid cells (for verbose diagnostics).
pub(crate) fn count_joint_boundary_grid_cells(
    baseline: RefinedGapFrames,
    start_min: usize,
    end_max: usize,
    step: usize,
) -> u32 {
    let mut count = 0u32;
    let mut try_start = baseline.start_frame;
    while try_start >= start_min {
        let mut try_end = baseline.end_frame;
        while try_end <= end_max {
            if try_end > try_start
                && (try_start != baseline.start_frame || try_end != baseline.end_frame)
            {
                count += 1;
            }
            if try_end >= end_max {
                break;
            }
            try_end = (try_end + step).min(end_max);
        }
        if try_start <= start_min {
            break;
        }
        try_start = try_start.saturating_sub(step).max(start_min);
    }
    count
}

/// Whether fit mode may return after baseline unified search without running the grid.
pub(crate) fn accepts_baseline_without_boundary_grid(
    search: FitBoundarySearch,
    confidence: FillConfidence,
) -> bool {
    fit_routing::baseline_only_accepts(search, confidence)
}

fn fit_haystack_secs(params: &SeamGateParams<'_>) -> f64 {
    let channels = params.cfg.channels.max(1);
    params.geom.b_samples.len() as f64 / channels as f64 / f64::from(params.cfg.sample_rate)
}

fn baseline_seam_scores(
    baseline_candidate: Option<&FitJointCandidate>,
    recorded_failure: &Option<SeamGateFailure>,
) -> (f64, f64) {
    if let Some(c) = baseline_candidate {
        return (c.outcome.report_pre, c.outcome.report_post);
    }
    match recorded_failure {
        Some(SeamGateFailure::WaveformBelowThreshold { pre, post, .. }) => (*pre, *post),
        Some(SeamGateFailure::ResidualHeadroomExceeded { pre, post, .. }) => (*pre, *post),
        _ => (0.0, 0.0),
    }
}

fn anchor_seam_gate_params(params: &SeamGateParams<'_>, baseline: RefinedGapFrames) -> AnchorSeamParams {
    let gap_frames = baseline.end_frame.saturating_sub(baseline.start_frame);
    AnchorSeamParams {
        context_frames: params.cfg.context_frames,
        max_anchors_per_side: params.cfg.max_anchors_per_side,
        max_bracket_frames: (params.cfg.max_anchor_bracket_secs * f64::from(params.cfg.sample_rate))
            .round()
            .max(1.0) as usize,
        min_prominence: params.cfg.anchor_seam_min_prominence,
        structure: StructureMatchParams {
            gap_frames,
            bin_frames: params.cfg.bin_frames.max(1),
            search_radius_frames: params.cfg.search_radius_frames,
            fill_length_slack_frames: params.cfg.fill_length_slack_frames,
            max_fine_adjustment_frames: gap_structure::structure_fine_polish_frames(params.cfg.bin_frames),
            silence_peak_fraction: params.cfg.silence_peak_fraction,
            absolute_silence_rms: params.cfg.absolute_silence_rms,
        },
    }
}

fn mark_anchor_outcome(outcome: &mut SeamGateOutcome, move_frames: usize) {
    outcome.anchor_seam_used = true;
    outcome.anchor_bracket_move_frames = move_frames;
    if outcome.anchor_trusted {
        tracing::debug!(
            pre = outcome.report_pre,
            post = outcome.report_post,
            move_frames,
            "anchor-trusted patch at editorial seam"
        );
    }
}

fn joint_candidate_ranking_cmp(
    a: &&FitJointCandidate,
    b: &&FitJointCandidate,
) -> std::cmp::Ordering {
    fit_routing::selection_cmp(&a.score(), &b.score())
}

fn global_best_joint_candidate(pool: &[FitJointCandidate]) -> Option<&FitJointCandidate> {
    pool.iter().max_by(joint_candidate_ranking_cmp)
}

fn best_high_joint_candidate(pool: &[FitJointCandidate]) -> Option<&FitJointCandidate> {
    pool.iter()
        .filter(|c| fit_routing::terminates_high(c.outcome.confidence))
        .max_by(joint_candidate_ranking_cmp)
}

fn anchor_bracket_both_matchable_at_gate(
    templates: &policies::SeamTemplates<'_>,
    placement: policies::SeamPlacement,
    pre_window: usize,
    post_window: usize,
    params: &SeamGateParams<'_>,
) -> bool {
    static ANCHOR_XCORR: clip_sync::FftCorrelator = clip_sync::FftCorrelator;
    let max_lag = params
        .cfg
        .residual_max_lag_frames
        .clamp(0, i32::MAX as i64) as i32;
    let correlator = if max_lag > 0 {
        Some(&ANCHOR_XCORR)
    } else {
        None
    };
    anchor_bracket_both_matchable(
        templates,
        placement,
        pre_window,
        post_window,
        &params.cfg.anchor_matchability,
        correlator,
        max_lag,
    )
}

fn best_anchor_joint_candidate(pool: &[FitJointCandidate]) -> Option<&FitJointCandidate> {
    let global = global_best_joint_candidate(pool)?;
    if global.outcome.anchor_seam_used {
        Some(global)
    } else {
        None
    }
}

struct AnchorSeamJointSearchState<'a> {
    pool: &'a mut Vec<FitJointCandidate>,
    recorded_failure: &'a mut Option<SeamGateFailure>,
    best_waveform: &'a mut Option<SeamScoreAttempt>,
}

struct AnchorSeamJointSearchCtx {
    baseline: RefinedGapFrames,
    baseline_pre: f64,
    baseline_post: f64,
    fit_boundary_search: FitBoundarySearch,
    haystack_secs: f64,
}

fn try_anchor_seam_joint_search(
    state: &mut AnchorSeamJointSearchState<'_>,
    ctx: &AnchorSeamJointSearchCtx,
    source: &mut dyn FitCandidateSource,
) -> Result<Option<SeamGateOutcome>, SeamGateFailure> {
    let AnchorSeamJointSearchState {
        pool,
        recorded_failure,
        best_waveform,
    } = state;
    let &AnchorSeamJointSearchCtx {
        baseline,
        baseline_pre,
        baseline_post,
        fit_boundary_search,
        haystack_secs,
    } = ctx;

    // The source decides the gate (mode/contour) + enumeration; empty ⇒ anchor path not engaged.
    let brackets = source.anchor_brackets(baseline_pre, baseline_post);
    if brackets.is_empty() {
        return Ok(None);
    }

    tracing::debug!(brackets = brackets.len(), "anchor seam bracket search");

    for bracket in brackets {
        if bracket.refined == baseline {
            continue;
        }
        // `record_fit_joint_candidate_to_pool` only pushes on a passing gate; on failure the pool
        // is unchanged. Mark only the candidate we actually appended — otherwise a failed bracket
        // would stamp `anchor_seam_used` onto the prior entry (e.g. the baseline).
        let pool_len_before = pool.len();
        record_fit_joint_candidate_to_pool(
            pool,
            recorded_failure,
            best_waveform,
            bracket.refined,
            baseline,
            source,
            true,
            SeamScoreSource::Anchor,
        );
        if pool.len() > pool_len_before {
            if let Some(candidate) = pool.last_mut() {
                mark_anchor_outcome(&mut candidate.outcome, bracket.move_frames);
            }
        }
    }

    // E3: the best Pearson-High candidate, if it is an anchor bracket, terminates here (residual
    // confirmed by `try_finalize_high_joint_candidate`).
    if let Some(candidate) = best_high_joint_candidate(pool) {
        if candidate.outcome.anchor_seam_used {
            let candidate = candidate.clone();
            if let Some(outcome) =
                try_finalize_high_joint_candidate(candidate, source, haystack_secs, None)
            {
                return Ok(Some(outcome));
            }
        }
    }

    // E4: under baseline-only, an anchor bracket that ranks best overall is accepted without the grid.
    if let Some(candidate) = best_anchor_joint_candidate(pool) {
        if accepts_baseline_without_boundary_grid(
            fit_boundary_search,
            candidate.outcome.confidence,
        ) {
            let outcome = candidate.outcome.clone();
            let mut outcome = source.finalize_residual(outcome)?;
            outcome.fit_haystack_secs = haystack_secs;
            return Ok(Some(outcome));
        }
    }

    Ok(None)
}

fn evaluate_seam_gate_fit_joint(
    baseline: RefinedGapFrames,
    params: &SeamGateParams<'_>,
) -> Result<SeamGateOutcome, SeamGateFailure> {
    let step = boundary_search_step_frames(params.cfg.max_extend_frames, params.cfg.step_frames);
    let total_frames = params.geom.a_pcm.samples.len() / params.cfg.channels.max(1);
    let start_min = if params.cfg.gap_start_extend_on_pre_seam_fail {
        baseline.start_frame.saturating_sub(params.cfg.max_extend_frames)
    } else {
        baseline.start_frame
    };
    let end_max = if params.cfg.gap_end_extend_on_post_seam_fail {
        (baseline.end_frame + params.cfg.max_extend_frames).min(total_frames)
    } else {
        baseline.end_frame
    };
    let config = FitJointConfig {
        fit_boundary_search: params.cfg.fit_boundary_search,
        anchor_seam_mode: params.cfg.anchor_seam_mode,
        start_min,
        end_max,
        step,
        haystack_secs: fit_haystack_secs(params),
    };
    let cache = FitHaystackCache::build(params);
    let mut source = AudioFitSource {
        params,
        cache: &cache,
        baseline,
    };
    evaluate_seam_gate_fit_joint_core(baseline, &config, &mut source)
}

/// The fit-joint precedence loop (E1–E7), driven by a [`FitCandidateSource`] so it is testable with
/// scripted numbers (no audio). Production calls it via [`evaluate_seam_gate_fit_joint`] with an
/// [`AudioFitSource`]; tests use a scripted fake. See docs/archive/fit-routing-extraction-plan.md §9.
///
/// Single pool path (the `defer_residual` fork is gone): candidates score with Pearson confidence;
/// residual is applied at selection (`try_finalize_*` / `select_joint_fit_winner…`), a no-op when
/// residual measurement is disabled.
fn evaluate_seam_gate_fit_joint_core(
    baseline: RefinedGapFrames,
    config: &FitJointConfig,
    source: &mut dyn FitCandidateSource,
) -> Result<SeamGateOutcome, SeamGateFailure> {
    let mut pool: Vec<FitJointCandidate> = Vec::new();
    let mut recorded_failure: Option<SeamGateFailure> = None;
    let mut best_waveform: Option<SeamScoreAttempt> = None;

    record_fit_joint_candidate_to_pool(
        &mut pool,
        &mut recorded_failure,
        &mut best_waveform,
        baseline,
        baseline,
        source,
        false,
        SeamScoreSource::Baseline,
    );

    // E1: a High baseline short-circuits in any mode (residual confirmed by `try_finalize_high…`).
    if pool
        .first()
        .is_some_and(|c| fit_routing::terminates_high(c.outcome.confidence))
    {
        let candidate = pool.first().expect("baseline high").clone();
        if let Some(outcome) =
            try_finalize_high_joint_candidate(candidate, source, config.haystack_secs, None)
        {
            return Ok(outcome);
        }
    }

    // E2: under baseline-only, a High/Marginal baseline is accepted without the grid (the force rule
    // withholds a Marginal baseline so anchor search runs first).
    if let Some(candidate) = pool.first() {
        if baseline_accept_without_grid(
            config.fit_boundary_search,
            candidate.outcome.confidence,
            config.anchor_seam_mode,
        ) {
            return select_joint_fit_winner_with_residual(
                pool,
                recorded_failure,
                best_waveform,
                baseline,
                source,
                config.haystack_secs,
                None,
            );
        }
    }

    let (baseline_pre, baseline_post) = baseline_seam_scores(pool.first(), &recorded_failure);

    if let Some(outcome) = try_anchor_seam_joint_search(
        &mut AnchorSeamJointSearchState {
            pool: &mut pool,
            recorded_failure: &mut recorded_failure,
            best_waveform: &mut best_waveform,
        },
        &AnchorSeamJointSearchCtx {
            baseline,
            baseline_pre,
            baseline_post,
            fit_boundary_search: config.fit_boundary_search,
            haystack_secs: config.haystack_secs,
        },
        source,
    )? {
        return Ok(outcome);
    }

    // E5: baseline-only never runs the grid — pick the pool winner (or skip on recorded failure).
    if config.fit_boundary_search == FitBoundarySearch::BaselineOnly {
        return select_joint_fit_winner_with_residual(
            pool,
            recorded_failure,
            best_waveform,
            baseline,
            source,
            config.haystack_secs,
            None,
        );
    }

    let grid_cells =
        count_joint_boundary_grid_cells(baseline, config.start_min, config.end_max, config.step);

    let mut try_start = baseline.start_frame;
    while try_start >= config.start_min {
        let mut try_end = baseline.end_frame;
        while try_end <= config.end_max {
            if try_end > try_start
                && (try_start != baseline.start_frame || try_end != baseline.end_frame)
            {
                record_fit_joint_candidate_to_pool(
                    &mut pool,
                    &mut recorded_failure,
                    &mut best_waveform,
                    RefinedGapFrames {
                        start_frame: try_start,
                        end_frame: try_end,
                    },
                    baseline,
                    source,
                    false,
                    SeamScoreSource::Grid,
                );
            }
            if try_end >= config.end_max {
                break;
            }
            try_end = (try_end + config.step).min(config.end_max);
        }
        if try_start <= config.start_min {
            break;
        }
        try_start = try_start.saturating_sub(config.step).max(config.start_min);
    }

    // E6: best Pearson-High over the full grid (residual confirmed).
    if let Some(outcome) =
        try_finalize_best_grid_high(&pool, source, config.haystack_secs, grid_cells)
    {
        return Ok(outcome);
    }

    // E7: best of the pool, or skip with the recorded below-floor failure.
    select_joint_fit_winner_with_residual(
        pool,
        recorded_failure,
        best_waveform,
        baseline,
        source,
        config.haystack_secs,
        Some(grid_cells),
    )
}

fn want_residual_measurement(params: &SeamGateParams<'_>) -> bool {
    params.cfg.measure_residual
        || params.cfg.residual_gate.is_active()
        || tracing::enabled!(tracing::Level::DEBUG)
}

/// Post-side seam/residual window. Returns 0 when the trimmed post border template is empty so
/// residual measurement is skipped (L7: a forced 1-frame window can spuriously cancel).
fn seam_post_gate_frames(seam_gate_frames: usize, post_border_len: usize) -> usize {
    if post_border_len == 0 {
        0
    } else {
        seam_gate_frames.min(post_border_len).max(1)
    }
}

fn measure_fit_residual_verdict(
    params: &SeamGateParams<'_>,
    cache: &FitHaystackCache,
    refined: RefinedGapFrames,
    alignment_start_frame: usize,
    offset_nominal_start: usize,
    waveform_gate_frames: usize,
    post_gate_frames: usize,
) -> Option<policies::SeamResidualVerdict> {
    if !want_residual_measurement(params) {
        return None;
    }
    let chosen_delta = alignment_start_frame as i64 - refined.start_frame as i64;
    let nominal_delta = offset_nominal_start as i64 - refined.start_frame as i64;
    let floor_common = |window: usize| policies::SeamFloorParams {
        a_samples: &params.geom.a_pcm.samples,
        channels: params.cfg.channels,
        b_mono: &cache.b_mono,
        window,
        standoff_frames: params.cfg.border_standoff_frames,
        a_to_b_delta: nominal_delta,
        step_frames: window.max(1),
        max_walk_frames: params.cfg.sample_rate as usize * 3,
        absolute_silence_rms: params.cfg.absolute_silence_rms,
        max_lag_frames: params.cfg.residual_max_lag_frames,
    };
    let placement_slide = alignment_start_frame.abs_diff(offset_nominal_start) as u64;

    // Follow the same energy-selected channels as Pearson seam scoring so residual/floor is measured
    // on the channels that carry content (e.g. center-dominant 5.1), not a downmix diluted by quiet
    // surrounds. Selection is a pure function of (params, refined) — recomputed here, same border
    // spec Pearson uses (residual-channel-alignment-plan §4b). Empty ⇒ mono downmix path.
    let border_spec = gap_border_spec(params, refined);
    let selected: Vec<usize> =
        policies::selected_seam_channels(&params.geom.a_pcm.samples, params.cfg.channels, &border_spec)
            .into_iter()
            .filter(|&ch| ch < cache.b_ch.len())
            .collect();

    if selected.is_empty() {
        let (chosen_pre, floor_pre) = policies::seam_chosen_and_floor(
            &floor_common(waveform_gate_frames),
            policies::SeamSide::Pre,
            refined.start_frame,
            refined.end_frame,
            chosen_delta,
        );
        // Window 0 → `select_reference_window` abstains; skip spurious 1-frame post fit (L7).
        let (chosen_post, floor_post) = policies::seam_chosen_and_floor(
            &floor_common(post_gate_frames),
            policies::SeamSide::Post,
            refined.start_frame,
            refined.end_frame,
            chosen_delta,
        );
        return Some(policies::SeamResidualVerdict::from_parts_with_placement(
            &chosen_pre,
            &chosen_post,
            &floor_pre,
            &floor_post,
            params.cfg.residual_floor_ok_db,
            placement_slide,
            params.cfg.residual_max_lag_frames,
        ));
    }

    let pre = policies::seam_chosen_and_floor_multichannel(
        &floor_common(waveform_gate_frames),
        &cache.b_ch,
        &selected,
        policies::SeamSide::Pre,
        refined.start_frame,
        refined.end_frame,
        chosen_delta,
    );
    let post = policies::seam_chosen_and_floor_multichannel(
        &floor_common(post_gate_frames),
        &cache.b_ch,
        &selected,
        policies::SeamSide::Post,
        refined.start_frame,
        refined.end_frame,
        chosen_delta,
    );
    log_residual_channel_breakdown(refined.start_frame, &selected, &pre, &post);
    Some(policies::SeamResidualVerdict::from_channel_residuals(
        &pre,
        &post,
        params.cfg.residual_floor_ok_db,
        placement_slide,
        params.cfg.residual_max_lag_frames,
    ))
}

/// Residual headroom verdict at a given placement — **fingerprint** use (the same-source axis). Reuses
/// the production [`measure_fit_residual_verdict`] at the decision (throat) placement, with the throat
/// as its own baseline. Requires `params.cfg.measure_residual` (the fingerprint sets it on its cfg).
pub(crate) fn oracle_measure_residual(
    params: &SeamGateParams<'_>,
    cache: &FitHaystackCache,
    refined: RefinedGapFrames,
    structure_start_frame: usize,
) -> Option<policies::SeamResidualVerdict> {
    let (offset_nominal_start, waveform_gate_frames, post_gate_frames) =
        fit_residual_geometry(refined, refined, params);
    measure_fit_residual_verdict(
        params,
        cache,
        refined,
        structure_start_frame,
        offset_nominal_start,
        waveform_gate_frames,
        post_gate_frames,
    )
}

/// Debug-log per-channel residual headroom so surround/center mixes show which channels cancelled
/// (RUST_LOG=debug). Mirrors the `fill seam channel diagnostics` log on the scoring side.
fn log_residual_channel_breakdown(
    gap_start_frame: usize,
    selected: &[usize],
    pre: &[policies::SeamChannelResidual],
    post: &[policies::SeamChannelResidual],
) {
    if !tracing::enabled!(tracing::Level::DEBUG) {
        return;
    }
    let headroom = |c: &policies::SeamChannelResidual| (c.channel, c.chosen.residual_db - c.floor.residual_db);
    let pre_headroom: Vec<(usize, f64)> = pre.iter().map(headroom).collect();
    let post_headroom: Vec<(usize, f64)> = post.iter().map(headroom).collect();
    tracing::debug!(
        gap_start = gap_start_frame,
        selected_channels = ?selected,
        pre_channel_headroom_db = ?pre_headroom,
        post_channel_headroom_db = ?post_headroom,
        "fill residual channel breakdown"
    );
}

/// Oracle-only: build the unified haystack cache once for a cell, then reuse across candidates
/// (the cache depends only on `params`, not the candidate `refined`). W5 discovery, Phase 1.
#[doc(hidden)]
pub fn oracle_build_fit_cache(params: &SeamGateParams<'_>) -> FitHaystackCache {
    FitHaystackCache::build(params)
}

/// Oracle-only: score one fit candidate at the seam gate (W5 discovery, Phase 1). Runs the same
/// [`evaluate_seam_gate_fit_candidate`] production uses against a pre-built `cache`, returning gate
/// Pearson `(pre, post)` + confidence + ranking score, or the gate failure. See
/// docs/TEMP-w5-anchor-rescue-diag-plan.md §5.1b.
#[doc(hidden)]
pub fn oracle_score_fit_candidate(
    params: &SeamGateParams<'_>,
    cache: &FitHaystackCache,
    refined: RefinedGapFrames,
    baseline: RefinedGapFrames,
    anchor_seam_bracket: bool,
) -> Result<(f64, f64, FillConfidence, f64), SeamGateFailure> {
    let (outcome, ranking_score) =
        evaluate_seam_gate_fit_candidate(refined, baseline, params, cache, anchor_seam_bracket)?;
    Ok((
        outcome.report_pre,
        outcome.report_post,
        outcome.confidence,
        ranking_score,
    ))
}

/// Production-faithful joint-pool outcome for one oracle cell (W5 discovery, Phase 2). `patched`
/// is false when the whole gate skipped (E5). See docs/TEMP-w5-anchor-rescue-diag-plan.md §5.2.2.
#[doc(hidden)]
pub struct OracleJointOutcome {
    pub patched: bool,
    pub anchor_seam_used: bool,
    pub anchor_move_frames: usize,
}

/// Oracle-only: run the **full** fit-joint routing (E1–E7) on oracle-built `params` and report which
/// candidate won the pool — exactly what `PatchAudio` would decide, without decoding. Phase 2.
#[doc(hidden)]
pub fn oracle_evaluate_fit_joint(
    params: &SeamGateParams<'_>,
    baseline: RefinedGapFrames,
) -> OracleJointOutcome {
    match evaluate_seam_gate_fit_joint(baseline, params) {
        Ok(outcome) => OracleJointOutcome {
            patched: true,
            anchor_seam_used: outcome.anchor_seam_used,
            anchor_move_frames: outcome.anchor_bracket_move_frames,
        },
        Err(_) => OracleJointOutcome {
            patched: false,
            anchor_seam_used: false,
            anchor_move_frames: 0,
        },
    }
}

/// Oracle-only: would auto-mode anchor search run for this cell? Mirrors the gate inside
/// `AudioFitSource::anchor_brackets` (baseline contour + score floor). Phase 2 CSV column.
#[doc(hidden)]
pub fn oracle_anchor_seam_would_run(
    params: &SeamGateParams<'_>,
    baseline: RefinedGapFrames,
    baseline_pre: f64,
    baseline_post: f64,
) -> bool {
    if params.cfg.anchor_seam_mode == AnchorSeamMode::Off {
        return false;
    }
    let anchor_params = anchor_seam_gate_params(params, baseline);
    let baseline_signature = build_gap_signature(
        &params.geom.a_pcm.samples,
        params.cfg.channels,
        baseline.start_frame,
        baseline.end_frame,
        params.cfg.context_frames,
        &anchor_params.structure,
        params.cfg.gap_signature_mode,
    );
    should_run_anchor_seam(
        params.cfg.anchor_seam_mode,
        baseline_pre,
        baseline_post,
        params.cfg.min_fill_correlation,
        params.cfg.fill_marginal_margin,
        baseline_signature.has_anchor_seam_contour(),
    )
}

/// Outputs of the gate's structure-alignment step, **shared** by `evaluate_seam_gate_fit_candidate` and
/// the throat-placement read (`oracle_throat_structure_frame`) so registration metrics land at the SAME B
/// placement the gate scored — one code path, no second `place_on_b`, no divergence (review F1).
struct GateStructureAlign {
    a_pre_border: Vec<f64>,
    a_post_border: Vec<f64>,
    a_pre_ch: Vec<Vec<f64>>,
    a_post_ch: Vec<Vec<f64>>,
    gap_frames: usize,
    gap_secs: f64,
    waveform_gate_frames: usize,
    post_gate_frames: usize,
    unified: UnifiedFillMatch,
}

/// The gate's structure-alignment search for one `(refined, baseline)` candidate: builds the same border
/// templates / signature / waveform context the seam gate uses, runs the unified fill search, and returns
/// the placement (`unified.alignment.start_frame`) plus the windows the waveform gate needs. Fails only
/// when the search finds **no** placement (`StructureAlignmentFailed`); a weak-structure or weak-waveform
/// result still returns the placement, so a *skipped* gap's registration metrics can be read at the gate's
/// throat. This is the single source of the structure placement — `evaluate_seam_gate_fit_candidate` runs
/// the waveform/residual gate on top of it.
fn gate_structure_align(
    refined: RefinedGapFrames,
    baseline: RefinedGapFrames,
    params: &SeamGateParams<'_>,
    cache: &FitHaystackCache,
    anchor_seam_bracket: bool,
) -> Result<GateStructureAlign, SeamGateFailure> {
    let gap_frames = refined.end_frame.saturating_sub(refined.start_frame);
    if gap_frames == 0 {
        return Err(SeamGateFailure::StructureAlignmentFailed);
    }

    let border_spec = gap_border_spec(params, refined);
    let (a_pre_border, a_post_border) =
        policies::border_templates_for_gap(&params.geom.a_pcm.samples, params.cfg.channels, &border_spec);
    let (a_pre_ch, a_post_ch) = policies::border_templates_per_channel_for_gap(
        &params.geom.a_pcm.samples,
        params.cfg.channels,
        &border_spec,
    );

    let gap_secs = gap_frames as f64 / params.cfg.sample_rate as f64;
    let start_delta_secs = (refined.start_frame as i64 - baseline.start_frame as i64) as f64
        / params.cfg.sample_rate as f64;
    let end_delta_secs = (refined.end_frame as i64 - baseline.end_frame as i64) as f64
        / params.cfg.sample_rate as f64;
    let refined_b_start_secs = params.geom.refined_b_start_secs + start_delta_secs;
    let refined_b_end_secs = params.geom.refined_b_end_secs + end_delta_secs;

    let offset_nominal_start = ((refined_b_start_secs - params.geom.b_extract_start_secs)
        * params.cfg.sample_rate as f64)
        .round() as usize;
    let gap_end_in_haystack = ((refined_b_end_secs - params.geom.b_extract_start_secs)
        * params.cfg.sample_rate as f64)
        .round() as usize;

    let structure_params = StructureMatchParams {
        gap_frames,
        bin_frames: params.cfg.bin_frames.max(1),
        search_radius_frames: params.cfg.search_radius_frames,
        fill_length_slack_frames: params.cfg.fill_length_slack_frames,
        max_fine_adjustment_frames: gap_structure::structure_fine_polish_frames(params.cfg.bin_frames),
        silence_peak_fraction: params.cfg.silence_peak_fraction,
        absolute_silence_rms: params.cfg.absolute_silence_rms,
    };

    let signature = build_gap_signature(
        &params.geom.a_pcm.samples,
        params.cfg.channels,
        refined.start_frame,
        refined.end_frame,
        params.cfg.context_frames,
        &structure_params,
        params.cfg.gap_signature_mode,
    );

    let max_seam = params.geom.seam_gate_frames;
    let min_seam = (max_seam / 4).max(1);
    let waveform_gate_frames = if anchor_seam_bracket {
        policies::adaptive_seam_window_frames(
            a_pre_border.len(),
            min_seam,
            max_seam,
            policies::border_active_extent_frames(&a_pre_border),
        )
    } else {
        max_seam.min(a_pre_border.len().max(1))
    };
    let post_seam_cap = if anchor_seam_bracket {
        policies::adaptive_seam_window_frames(
            a_post_border.len(),
            min_seam,
            max_seam,
            policies::border_active_extent_frames(&a_post_border),
        )
    } else {
        params.geom.seam_gate_frames
    };
    let post_gate_frames = seam_post_gate_frames(post_seam_cap, a_post_border.len());
    let templates = policies::SeamTemplates {
        a_pre: &a_pre_border,
        a_post: &a_post_border,
        a_pre_ch: &a_pre_ch,
        a_post_ch: &a_post_ch,
        b_mono: &cache.b_mono,
        b_ch: &cache.b_ch,
    };
    let waveform = WaveformSeamContext {
        templates: &templates,
        gap_frames,
        pre_window: waveform_gate_frames,
        post_window: post_gate_frames,
        b_total_frames: cache.b_mono.len(),
        repeat_window_frames: params.geom.border_frames.max(1),
        repeat_penalty_weight: params.cfg.fill_repeat_penalty_weight,
    };
    // Mode-coupled nominal bias: an energy-resolved signature is the signal that the alignment
    // nominal map may be wrong, so loosen the distance-from-nominal penalty (the penalty grows
    // linearly with distance, so this mainly frees far-off / drifted candidates). Bool keeps base.
    let nominal_bias_scale = match signature {
        GapSignature::Energy(_) => params.cfg.fill_fit_energy_nominal_bias_scale,
        GapSignature::Bool(_) => params.cfg.fill_fit_nominal_bias_scale,
    };
    let weights = UnifiedFitWeights {
        structure_weight: params.cfg.fill_fit_structure_weight,
        waveform_weight: params.cfg.fill_fit_waveform_weight,
        nominal_bias_scale,
        late_start_penalty_scale: params.cfg.fill_fit_late_start_penalty_scale,
    };
    let structure_timeline = cache.structure_timeline(&signature);
    let search_input = UnifiedFillSearchInput {
        signature: &signature,
        b_samples: params.geom.b_samples,
        channels: params.cfg.channels,
        waveform: &waveform,
        nominal_fill_start: offset_nominal_start,
        nominal_fill_end: gap_end_in_haystack,
    };
    let unified = match_gap_fill_unified_in_b_with_timeline(
        &search_input,
        &structure_params,
        weights,
        &structure_timeline,
        params.geom.anchor_search_prior,
    )
    .ok_or(SeamGateFailure::StructureAlignmentFailed)?;

    // Per-gap seam diagnostics (RUST_LOG=debug): which channels were scored and their
    // per-channel correlations at the winning placement — explains low pre/post on surround.
    if tracing::enabled!(tracing::Level::DEBUG) {
        let diag = policies::seam_channel_diagnostics(
            &templates,
            policies::SeamPlacement {
                start: unified.alignment.start_frame,
                gap_frames,
                pre_window: waveform_gate_frames,
                post_window: post_gate_frames,
            },
        );
        tracing::debug!(
            start_frame = unified.alignment.start_frame,
            seam_pre = unified.alignment.pre_correlation,
            seam_post = unified.alignment.post_correlation,
            structure_pre = unified.structure_pre,
            structure_post = unified.structure_post,
            signature = signature.mode_label(),
            selected_channels = ?diag.selected,
            per_channel = ?diag.per_channel,
            mono_pre = diag.mono.0,
            mono_post = diag.mono.1,
            "fill seam channel diagnostics"
        );
    }

    Ok(GateStructureAlign {
        a_pre_border,
        a_post_border,
        a_pre_ch,
        a_post_ch,
        gap_frames,
        gap_secs,
        waveform_gate_frames,
        post_gate_frames,
        unified,
    })
}

/// Read the gate's zero-move **throat** placement (B haystack frame) for a gap — the placement the
/// decision **seam** (waveform @ lag 0) scores at. Registration metrics (`baseline_lag`, `seam_probe`,
/// `donor_interior`, `wide_envelope`, `splice`) are measured separately at **`b_mapped`**, not here.
/// `None` when the structure search finds no placement.
pub(crate) fn oracle_throat_structure_frame(
    params: &SeamGateParams<'_>,
    cache: &FitHaystackCache,
    refined: RefinedGapFrames,
) -> Option<usize> {
    gate_structure_align(refined, refined, params, cache, true)
        .ok()
        .map(|g| g.unified.alignment.start_frame)
}

fn evaluate_seam_gate_fit_candidate(
    refined: RefinedGapFrames,
    baseline: RefinedGapFrames,
    params: &SeamGateParams<'_>,
    cache: &FitHaystackCache,
    anchor_seam_bracket: bool,
) -> Result<(SeamGateOutcome, f64), SeamGateFailure> {
    let GateStructureAlign {
        a_pre_border,
        a_post_border,
        a_pre_ch,
        a_post_ch,
        gap_frames,
        gap_secs,
        waveform_gate_frames,
        post_gate_frames,
        unified,
    } = gate_structure_align(refined, baseline, params, cache, anchor_seam_bracket)?;
    let templates = policies::SeamTemplates {
        a_pre: &a_pre_border,
        a_post: &a_post_border,
        a_pre_ch: &a_pre_ch,
        a_post_ch: &a_post_ch,
        b_mono: &cache.b_mono,
        b_ch: &cache.b_ch,
    };
    let alignment = unified.alignment;
    let structure_pre = unified.structure_pre;
    let structure_post = unified.structure_post;
    let structure_start_frame = alignment.start_frame;

    // Residual/floor is probed once at selection (`finalize_fit_outcome_residual`), never per scored
    // candidate — keeping the grid/anchor cold path cheap. Scored candidates carry no verdict.
    let residual: Option<policies::SeamResidualVerdict> = None;

    if !structure_passes_gate(
        structure_pre,
        structure_post,
        params.cfg.min_structure_match_score,
        gap_secs,
        params.cfg.short_gap_mean_correlation_secs,
    ) {
        return Err(SeamGateFailure::StructureBelowThreshold {
            pre: structure_pre,
            post: structure_post,
        });
    }

    if anchor_seam_bracket {
        let placement = policies::SeamPlacement {
            start: alignment.start_frame,
            gap_frames,
            pre_window: waveform_gate_frames,
            post_window: post_gate_frames,
        };
        if !anchor_bracket_both_matchable_at_gate(
            &templates,
            placement,
            waveform_gate_frames,
            post_gate_frames,
            params,
        ) {
            return Err(waveform_below_threshold(
                alignment.pre_correlation,
                alignment.post_correlation,
                params.cfg.fill_absolute_floor,
            ));
        }
    }

    let pre_corr = alignment.pre_correlation;
    let post_corr = alignment.post_correlation;
    let pearson = classify_fill_waveform_confidence(
        pre_corr,
        post_corr,
        params.cfg.min_fill_correlation,
        params.cfg.fill_marginal_margin,
        params.cfg.fill_absolute_floor,
    );

    // Pearson-only confidence: residual is applied later at selection, so the residual-gated rescue
    // here just keeps a sub-floor Pearson alive as Marginal for the finalize step to confirm/veto.
    let confidence = if params.cfg.residual_gate.is_active() {
        match pearson {
            Ok(confidence) => confidence,
            Err(_) if params.cfg.residual_gate.rescue_enabled() => FillConfidence::Marginal,
            Err(_) => {
                return Err(waveform_below_threshold(
                    pre_corr,
                    post_corr,
                    params.cfg.fill_absolute_floor,
                ));
            }
        }
    } else {
        pearson.map_err(|_| {
            waveform_below_threshold(
                pre_corr,
                post_corr,
                params.cfg.fill_absolute_floor,
            )
        })?
    };

    let boundary_move = baseline.start_frame.abs_diff(refined.start_frame)
        + baseline.end_frame.abs_diff(refined.end_frame);
    let ranking_score = if anchor_seam_bracket {
        fit_anchor_candidate_ranking_score(
            pre_corr.min(post_corr),
            boundary_move,
            crate::domain::gap_fill_fit::anchor_bracket_center_drift_frames(baseline, refined),
        )
    } else {
        fit_candidate_ranking_score(pre_corr.min(post_corr), boundary_move)
    };
    let anchor_trusted = anchor_seam_bracket
        && anchor_trust_applies(
            structure_pre,
            structure_post,
            pre_corr,
            post_corr,
            params.cfg.strong_structure_trust,
            params.cfg.min_fill_correlation,
        );

    Ok((
        SeamGateOutcome {
            refined,
            alignment,
            report_pre: pre_corr,
            report_post: post_corr,
            structure_trusted: false,
            structure_start_frame,
            gap_frames,
            confidence,
            gap_start_adjust_frames: refined.start_frame as i64 - baseline.start_frame as i64,
            gap_end_adjust_frames: refined.end_frame as i64 - baseline.end_frame as i64,
            fit_used_boundary_grid: false,
            fit_boundary_grid_cells: None,
            fit_haystack_secs: fit_haystack_secs(params),
            residual,
            anchor_seam_used: false,
            anchor_bracket_move_frames: 0,
            anchor_trusted,
        },
        ranking_score,
    ))
}

fn evaluate_seam_gate_legacy(
    refined: RefinedGapFrames,
    params: &SeamGateParams<'_>,
) -> Result<SeamGateOutcome, SeamGateFailure> {
    let gap_frames = refined.end_frame.saturating_sub(refined.start_frame);
    if gap_frames == 0 {
        return Err(SeamGateFailure::StructureAlignmentFailed);
    }

    let gap_secs = gap_frames as f64 / params.cfg.sample_rate as f64;
    let border_spec = gap_border_spec(params, refined);
    let (a_pre_border, a_post_border) =
        policies::border_templates_for_gap(&params.geom.a_pcm.samples, params.cfg.channels, &border_spec);
    let (a_pre_ch, a_post_ch) = policies::border_templates_per_channel_for_gap(
        &params.geom.a_pcm.samples,
        params.cfg.channels,
        &border_spec,
    );
    let b_mono = policies::interleaved_to_mono(params.geom.b_samples, params.cfg.channels);
    let b_ch = policies::interleaved_to_channels(params.geom.b_samples, params.cfg.channels);

    let offset_nominal_start = ((params.geom.refined_b_start_secs - params.geom.b_extract_start_secs)
        * params.cfg.sample_rate as f64)
        .round() as usize;
    let gap_end_in_haystack = ((params.geom.refined_b_end_secs - params.geom.b_extract_start_secs)
        * params.cfg.sample_rate as f64)
        .round() as usize;

    let structure_params = StructureMatchParams {
        gap_frames,
        bin_frames: params.cfg.bin_frames.max(1),
        search_radius_frames: params.cfg.search_radius_frames,
        fill_length_slack_frames: params.cfg.fill_length_slack_frames,
        max_fine_adjustment_frames: gap_structure::structure_fine_polish_frames(params.cfg.bin_frames),
        silence_peak_fraction: params.cfg.silence_peak_fraction,
        absolute_silence_rms: params.cfg.absolute_silence_rms,
    };

    let signature = gap_structure::build_gap_context_signature(
        &params.geom.a_pcm.samples,
        params.cfg.channels,
        refined.start_frame,
        refined.end_frame,
        params.cfg.context_frames,
        &structure_params,
    );

    let mut alignment = gap_structure::match_gap_structure_in_b(
        &signature,
        params.geom.b_samples,
        params.cfg.channels,
        offset_nominal_start,
        gap_end_in_haystack,
        &structure_params,
    )
    .ok_or(SeamGateFailure::StructureAlignmentFailed)?;
    let structure_start_frame = alignment.start_frame;
    let structure_pre = alignment.pre_correlation;
    let structure_post = alignment.post_correlation;

    if !structure_passes_gate(
        structure_pre,
        structure_post,
        params.cfg.min_structure_match_score,
        gap_secs,
        params.cfg.short_gap_mean_correlation_secs,
    ) {
        return Err(SeamGateFailure::StructureBelowThreshold {
            pre: structure_pre,
            post: structure_post,
        });
    }

    let structure_trusted = !params.cfg.disable_structure_trust
        && structure_pre >= params.cfg.strong_structure_trust
        && structure_post >= params.cfg.strong_structure_trust;

    let (report_pre, report_post, patched_structure_trusted) = if structure_trusted {
        (structure_pre, structure_post, true)
    } else {
        let waveform_gate_frames = params
            .geom
            .seam_gate_frames
            .min(a_pre_border.len().max(1));
        let post_gate_frames = seam_post_gate_frames(params.geom.seam_gate_frames, a_post_border.len());
        let templates = policies::SeamTemplates {
            a_pre: &a_pre_border,
            a_post: &a_post_border,
            a_pre_ch: &a_pre_ch,
            a_post_ch: &a_post_ch,
            b_mono: &b_mono,
            b_ch: &b_ch,
        };

        let (pre_corr, post_corr) = policies::fill_seam_correlations(
            &templates,
            policies::SeamPlacement {
                start: alignment.start_frame,
                gap_frames,
                pre_window: waveform_gate_frames,
                post_window: post_gate_frames,
            },
        );

        let soften_waveform_gate = !params.cfg.disable_structure_trust
            && structure_pre >= params.cfg.partial_structure_waveform_soften
            && structure_post >= params.cfg.partial_structure_waveform_soften;
        let effective_min_corr = if soften_waveform_gate {
            params
                .cfg
                .min_fill_correlation
                .min(PARTIAL_WAVEFORM_MIN_CORRELATION)
        } else {
            params.cfg.min_fill_correlation
        };

        alignment.pre_correlation = pre_corr;
        alignment.post_correlation = post_corr;

        if !seams_pass_correlation_gate(
            &alignment,
            effective_min_corr,
            gap_secs,
            params.cfg.short_gap_mean_correlation_secs,
            params.cfg.short_gap_one_strong_seam_fallback,
            params.cfg.disable_structure_trust,
        ) {
            return Err(waveform_below_threshold(pre_corr, post_corr, effective_min_corr));
        }
        (pre_corr, post_corr, false)
    };

    Ok(SeamGateOutcome {
        refined,
        alignment,
        report_pre,
        report_post,
        structure_trusted: patched_structure_trusted,
        structure_start_frame,
        gap_frames,
        confidence: FillConfidence::High,
        gap_start_adjust_frames: 0,
        gap_end_adjust_frames: 0,
        fit_used_boundary_grid: false,
        fit_boundary_grid_cells: None,
        fit_haystack_secs: 0.0,
        // Legacy gate path does not compute the residual verdict (fit-mode only).
        residual: None,
        anchor_seam_used: false,
        anchor_bracket_move_frames: 0,
        anchor_trusted: false,
    })
}

/// Extend `refined.end_frame` in steps when the post seam failed waveform correlation.
pub(crate) fn try_extend_gap_end_for_post_seam(
    refined: &mut RefinedGapFrames,
    gap_offset_secs: f64,
    params: &SeamGateParams<'_>,
    initial_fail: SeamGateFailure,
    max_extend_frames: usize,
    step_frames: usize,
    best_waveform: &mut Option<SeamScoreAttempt>,
) -> Result<SeamGateOutcome, SeamGateFailure> {
    let SeamGateFailure::WaveformBelowThreshold { pre, post, min, .. } = initial_fail else {
        return Err(initial_fail);
    };

    if !post_seam_extension_candidate(pre, post, min) {
        return Err(enrich_waveform_failure(initial_fail, best_waveform));
    }

    let total_frames = params.geom.a_pcm.samples.len() / params.cfg.channels.max(1);
    let original_end = refined.end_frame;
    let max_end = (original_end + max_extend_frames).min(total_frames);
    if step_frames == 0 || original_end >= max_end {
        return Err(enrich_waveform_failure(initial_fail, best_waveform));
    }

    let mut try_end = original_end;
    let mut last_fail = initial_fail;
    while try_end + step_frames <= max_end {
        try_end += step_frames;
        refined.end_frame = try_end;
        let refined_b_end_secs = try_end as f64 / params.cfg.sample_rate as f64 + gap_offset_secs;
        let try_params = SeamGateParams {
            geom: SeamGateGeometry {
                refined_b_end_secs,
                ..params.geom
            },
            ..*params
        };
        match evaluate_seam_gate(*refined, &try_params) {
            Ok(outcome) => {
                tracing::debug!(
                    extended_frames = refined.end_frame - refined.start_frame,
                    pre = outcome.report_pre,
                    post = outcome.report_post,
                    "gap end extended for post-seam alignment"
                );
                return Ok(outcome);
            }
            Err(SeamGateFailure::WaveformBelowThreshold { pre, post, .. }) => {
                consider_waveform_attempt(
                    best_waveform,
                    pre,
                    post,
                    SeamScoreSource::Extension,
                );
                last_fail = waveform_below_threshold(pre, post, min);
            }
            Err(other) => {
                refined.end_frame = original_end;
                return Err(other);
            }
        }
    }

    refined.end_frame = original_end;
    Err(enrich_waveform_failure(last_fail, best_waveform))
}

/// Shift `refined.start_frame` earlier in steps when the pre seam failed waveform correlation.
pub(crate) fn try_extend_gap_start_for_pre_seam(
    refined: &mut RefinedGapFrames,
    gap_offset_secs: f64,
    params: &SeamGateParams<'_>,
    initial_fail: SeamGateFailure,
    max_extend_frames: usize,
    step_frames: usize,
    best_waveform: &mut Option<SeamScoreAttempt>,
) -> Result<SeamGateOutcome, SeamGateFailure> {
    let SeamGateFailure::WaveformBelowThreshold { pre, post, min, .. } = initial_fail else {
        return Err(initial_fail);
    };

    if !pre_seam_extension_candidate(pre, post, min) {
        return Err(enrich_waveform_failure(initial_fail, best_waveform));
    }

    let original_start = refined.start_frame;
    let min_start = original_start.saturating_sub(max_extend_frames);
    if step_frames == 0 || original_start <= min_start {
        return Err(enrich_waveform_failure(initial_fail, best_waveform));
    }

    let mut try_start = original_start;
    let mut last_fail = initial_fail;
    while try_start >= step_frames && try_start - step_frames >= min_start {
        try_start -= step_frames;
        refined.start_frame = try_start;
        let refined_b_start_secs = try_start as f64 / params.cfg.sample_rate as f64 + gap_offset_secs;
        let try_params = SeamGateParams {
            geom: SeamGateGeometry {
                refined_b_start_secs,
                ..params.geom
            },
            ..*params
        };
        match evaluate_seam_gate(*refined, &try_params) {
            Ok(outcome) => {
                tracing::debug!(
                    extended_frames = refined.end_frame - refined.start_frame,
                    pre = outcome.report_pre,
                    post = outcome.report_post,
                    "gap start extended for pre-seam alignment"
                );
                return Ok(outcome);
            }
            Err(SeamGateFailure::WaveformBelowThreshold { pre, post, .. }) => {
                consider_waveform_attempt(
                    best_waveform,
                    pre,
                    post,
                    SeamScoreSource::Extension,
                );
                last_fail = waveform_below_threshold(pre, post, min);
            }
            Err(other) => {
                refined.start_frame = original_start;
                return Err(other);
            }
        }
    }

    refined.start_frame = original_start;
    Err(enrich_waveform_failure(last_fail, best_waveform))
}

pub(crate) struct SeamExtensionRetry {
    pub max_extend_frames: usize,
    pub step_frames: usize,
    pub gap_end_extend_on_post_seam_fail: bool,
    pub gap_start_extend_on_pre_seam_fail: bool,
}

/// Retry waveform gate failures by extending the gap end, then the gap start.
pub(crate) fn retry_waveform_seam_extensions(
    refined: &mut RefinedGapFrames,
    gap_offset_secs: f64,
    params: &SeamGateParams<'_>,
    initial_fail: SeamGateFailure,
    retry: SeamExtensionRetry,
) -> Result<SeamGateOutcome, SeamGateFailure> {
    let SeamGateFailure::WaveformBelowThreshold { pre, post, min, .. } = initial_fail else {
        return Err(initial_fail);
    };

    let mut best_waveform = None;
    consider_waveform_attempt(
        &mut best_waveform,
        pre,
        post,
        SeamScoreSource::Baseline,
    );
    let step = retry.step_frames.max(1);
    let mut last_fail = waveform_below_threshold(pre, post, min);

    if retry.gap_end_extend_on_post_seam_fail {
        match try_extend_gap_end_for_post_seam(
            refined,
            gap_offset_secs,
            params,
            last_fail,
            retry.max_extend_frames,
            step,
            &mut best_waveform,
        ) {
            Ok(outcome) => return Ok(outcome),
            Err(fail) => last_fail = fail,
        }
    }

    if retry.gap_start_extend_on_pre_seam_fail {
        return try_extend_gap_start_for_pre_seam(
            refined,
            gap_offset_secs,
            params,
            last_fail,
            retry.max_extend_frames,
            step,
            &mut best_waveform,
        );
    }

    Err(enrich_waveform_failure(last_fail, &best_waveform))
}

fn structure_passes_gate(
    pre_score: f64,
    post_score: f64,
    min_structure_match_score: f32,
    gap_secs: f64,
    short_gap_mean_correlation_secs: f64,
) -> bool {
    let pre = pre_score as f32;
    let post = post_score as f32;
    if gap_secs <= short_gap_mean_correlation_secs {
        (pre + post) / 2.0 >= min_structure_match_score
    } else {
        pre >= min_structure_match_score && post >= min_structure_match_score
    }
}

fn seams_pass_correlation_gate(
    alignment: &FillAlignment,
    min_fill_correlation: f32,
    gap_secs: f64,
    short_gap_mean_correlation_secs: f64,
    short_gap_one_strong_seam_fallback: bool,
    require_both_seams: bool,
) -> bool {
    let pre = alignment.pre_correlation as f32;
    let post = alignment.post_correlation as f32;
    if require_both_seams {
        return pre >= min_fill_correlation && post >= min_fill_correlation;
    }
    if gap_secs <= short_gap_mean_correlation_secs {
        if (pre + post) / 2.0 >= min_fill_correlation {
            return true;
        }
        if short_gap_one_strong_seam_fallback {
            return short_gap_one_strong_seam_passes(pre, post, min_fill_correlation);
        }
        false
    } else {
        pre >= min_fill_correlation && post >= min_fill_correlation
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::gap_fill_fit::fit_mode_waveform_floor_passes;
    use crate::domain::policies::FillAlignment;

    #[test]
    fn fit_mode_rejects_asymmetric_seams_that_gate_one_strong_would_pass() {
        assert!(!fit_mode_waveform_floor_passes(0.998, -0.90, 0.12));
        assert!(seams_pass_correlation_gate(
            &FillAlignment {
                start_frame: 0,
                fill_frames: 48_000,
                pre_correlation: 0.998,
                post_correlation: -0.90,
            },
            0.12,
            1.0,
            2.0,
            true,
            false,
        ));
    }

    #[test]
    fn short_gap_passes_on_one_strong_seam_when_mean_fails() {
        let alignment = FillAlignment {
            start_frame: 0,
            fill_frames: 48_000,
            pre_correlation: -0.01,
            post_correlation: 0.17,
        };
        assert!(seams_pass_correlation_gate(
            &alignment,
            0.12,
            1.0,
            2.0,
            true,
            false,
        ));
    }

    #[test]
    fn short_gap_still_fails_when_both_seams_weak_and_fallback_on() {
        let alignment = FillAlignment {
            start_frame: 0,
            fill_frames: 48_000,
            pre_correlation: 0.06,
            post_correlation: -0.13,
        };
        assert!(!seams_pass_correlation_gate(
            &alignment,
            0.12,
            1.0,
            2.0,
            true,
            false,
        ));
    }

    #[test]
    fn waveform_gate_mean_fails_without_one_strong_for_asymmetric_seams() {
        let alignment = FillAlignment {
            start_frame: 0,
            fill_frames: 48_000,
            pre_correlation: 0.998,
            post_correlation: -0.90,
        };
        assert!(!seams_pass_correlation_gate(
            &alignment,
            0.12,
            1.0,
            2.0,
            false,
            false,
        ));
        assert!(seams_pass_correlation_gate(
            &alignment,
            0.12,
            1.0,
            2.0,
            true,
            false,
        ));
    }

    #[test]
    fn strict_waveform_gate_rejects_weak_post_even_when_mean_passes() {
        let alignment = FillAlignment {
            start_frame: 0,
            fill_frames: 48_000,
            pre_correlation: 0.25,
            post_correlation: 0.08,
        };
        assert!(seams_pass_correlation_gate(
            &alignment,
            0.12,
            1.0,
            2.0,
            false,
            false,
        ));
        assert!(!seams_pass_correlation_gate(
            &alignment,
            0.12,
            1.0,
            2.0,
            false,
            true,
        ));
    }

    #[test]
    fn baseline_only_accepts_marginal_without_grid() {
        assert!(accepts_baseline_without_boundary_grid(
            FitBoundarySearch::BaselineOnly,
            FillConfidence::Marginal,
        ));
    }

    #[test]
    fn force_anchor_defers_baseline_marginal_short_circuit() {
        assert!(!baseline_accept_without_grid(
            FitBoundarySearch::BaselineOnly,
            FillConfidence::Marginal,
            AnchorSeamMode::Force,
        ));
        assert!(baseline_accept_without_grid(
            FitBoundarySearch::BaselineOnly,
            FillConfidence::Marginal,
            AnchorSeamMode::Auto,
        ));
        assert!(baseline_accept_without_grid(
            FitBoundarySearch::BaselineOnly,
            FillConfidence::High,
            AnchorSeamMode::Force,
        ));
    }

    #[test]
    fn full_grid_does_not_short_circuit_marginal_baseline() {
        assert!(!accepts_baseline_without_boundary_grid(
            FitBoundarySearch::FullGrid,
            FillConfidence::Marginal,
        ));
    }

    #[test]
    fn baseline_only_accepts_high_without_grid() {
        assert!(accepts_baseline_without_boundary_grid(
            FitBoundarySearch::BaselineOnly,
            FillConfidence::High,
        ));
    }

    #[test]
    fn joint_boundary_grid_cell_count_positive_when_extension_enabled() {
        let baseline = RefinedGapFrames {
            start_frame: 48_000,
            end_frame: 96_000,
        };
        let start_min = 36_000;
        let end_max = 108_000;
        let step = 4_800;
        let cells = count_joint_boundary_grid_cells(baseline, start_min, end_max, step);
        assert!(cells > 0, "expected grid cells, got {cells}");
    }

    #[test]
    fn seam_post_gate_frames_zero_when_post_border_empty() {
        assert_eq!(seam_post_gate_frames(12_000, 0), 0);
        assert_eq!(seam_post_gate_frames(12_000, 8), 8);
        assert_eq!(seam_post_gate_frames(12_000, 96_000), 12_000);
    }

    // ---- structure_passes_gate: short gap accepts on the mean; long gap requires both sides ----
    // (Direct unit coverage of the structural twin of `seams_pass_correlation_gate`, which was only
    // exercised through the audio pipeline before.) min = 0.35, short-gap threshold = 2.0 s.

    #[test]
    fn structure_gate_short_gap_accepts_on_mean() {
        // gap_secs <= 2.0 → mean(pre,post) >= 0.35 is enough, even with one weak side.
        assert!(structure_passes_gate(0.60, 0.20, 0.35, 1.0, 2.0)); // mean 0.40 ≥ 0.35
        assert!(!structure_passes_gate(0.20, 0.20, 0.35, 1.0, 2.0)); // mean 0.20 < 0.35
        assert!(structure_passes_gate(0.35, 0.35, 0.35, 1.0, 2.0)); // mean exactly at floor
    }

    #[test]
    fn structure_gate_long_gap_requires_both_sides() {
        // gap_secs > 2.0 → BOTH sides must clear 0.35; the mean is irrelevant.
        assert!(structure_passes_gate(0.60, 0.50, 0.35, 5.0, 2.0)); // both ≥ 0.35
        assert!(!structure_passes_gate(0.60, 0.34, 0.35, 5.0, 2.0)); // post just under
        assert!(structure_passes_gate(0.35, 0.35, 0.35, 5.0, 2.0)); // both exactly at floor
    }

    #[test]
    fn structure_gate_short_vs_long_diverge_on_same_scores() {
        // The load-bearing branch: identical asymmetric scores pass as a short gap (mean) but fail
        // as a long gap (both-sides).
        let (pre, post) = (0.60, 0.20);
        assert!(structure_passes_gate(pre, post, 0.35, 1.0, 2.0));
        assert!(!structure_passes_gate(pre, post, 0.35, 5.0, 2.0));
    }

    #[test]
    fn structure_gate_threshold_boundary_is_short_gap_inclusive() {
        // gap_secs == short_gap threshold uses the mean branch (the `<=`).
        assert!(structure_passes_gate(0.50, 0.20, 0.35, 2.0, 2.0)); // mean 0.35 at the 2.0 s boundary
        assert!(!structure_passes_gate(0.50, 0.20, 0.35, 2.0001, 2.0)); // just past → both-sides → post fails
    }

    // ---- Step 4/5: number-driven orchestration tests (gap-type → FitCandidateSource script, §10) ----

    use crate::domain::gap_anchor_seam::{AnchorCandidate, AnchorSeamSide, AnchorSource};

    fn rgf(start_frame: usize, end_frame: usize) -> RefinedGapFrames {
        RefinedGapFrames {
            start_frame,
            end_frame,
        }
    }

    /// Minimal `SeamGateOutcome` from the routing-relevant numbers; other fields are inert defaults
    /// the orchestration never reads.
    fn scripted_outcome(
        refined: RefinedGapFrames,
        confidence: FillConfidence,
        pre: f64,
        post: f64,
    ) -> SeamGateOutcome {
        SeamGateOutcome {
            refined,
            alignment: FillAlignment {
                start_frame: refined.start_frame,
                fill_frames: refined.end_frame.saturating_sub(refined.start_frame),
                pre_correlation: pre,
                post_correlation: post,
            },
            report_pre: pre,
            report_post: post,
            structure_trusted: false,
            structure_start_frame: refined.start_frame,
            gap_frames: refined.end_frame.saturating_sub(refined.start_frame),
            confidence,
            gap_start_adjust_frames: 0,
            gap_end_adjust_frames: 0,
            fit_used_boundary_grid: false,
            fit_boundary_grid_cells: None,
            fit_haystack_secs: 0.0,
            residual: None,
            anchor_seam_used: false,
            anchor_bracket_move_frames: 0,
            anchor_trusted: false,
        }
    }

    fn ok_score(
        refined: RefinedGapFrames,
        confidence: FillConfidence,
        pre: f64,
        post: f64,
        ranking: f64,
    ) -> Result<(SeamGateOutcome, f64), SeamGateFailure> {
        Ok((scripted_outcome(refined, confidence, pre, post), ranking))
    }

    fn waveform_skip(pre: f64, post: f64) -> Result<(SeamGateOutcome, f64), SeamGateFailure> {
        Err(waveform_below_threshold(pre, post, 0.12))
    }

    /// An anchor bracket whose pre/post anchors are inert (routing reads only `refined`/`move_frames`).
    fn scripted_bracket(refined: RefinedGapFrames, move_frames: usize) -> AnchorBracket {
        let anchor = |frame, side| AnchorCandidate {
            frame,
            side,
            source: AnchorSource::EnergyPeak,
            prominence: 1.0,
            rms: 1.0,
        };
        AnchorBracket {
            refined,
            pre: anchor(refined.start_frame, AnchorSeamSide::Pre),
            post: anchor(refined.end_frame, AnchorSeamSide::Post),
            move_frames,
            center_drift_frames: 0,
        }
    }

    /// Scripted [`FitCandidateSource`] (plan §9): a `refined → result` table, scripted anchor
    /// brackets, and call counters — drives the real precedence loop with no audio.
    struct ScriptedFitSource {
        scores: Vec<(RefinedGapFrames, Result<(SeamGateOutcome, f64), SeamGateFailure>)>,
        brackets: Vec<AnchorBracket>,
        /// Candidates whose residual probe vetoes at selection (drives the fall-through path).
        finalize_vetoes: Vec<RefinedGapFrames>,
        score_calls: usize,
        anchor_calls: usize,
    }

    impl ScriptedFitSource {
        fn new() -> Self {
            Self {
                scores: Vec::new(),
                brackets: Vec::new(),
                finalize_vetoes: Vec::new(),
                score_calls: 0,
                anchor_calls: 0,
            }
        }
        fn score_at(
            mut self,
            refined: RefinedGapFrames,
            result: Result<(SeamGateOutcome, f64), SeamGateFailure>,
        ) -> Self {
            self.scores.push((refined, result));
            self
        }
        fn brackets(mut self, brackets: Vec<AnchorBracket>) -> Self {
            self.brackets = brackets;
            self
        }
        /// Mark a candidate so its residual probe vetoes at selection.
        fn veto_finalize(mut self, refined: RefinedGapFrames) -> Self {
            self.finalize_vetoes.push(refined);
            self
        }
    }

    impl FitCandidateSource for ScriptedFitSource {
        fn score(
            &mut self,
            refined: RefinedGapFrames,
            _anchor_seam_bracket: bool,
        ) -> Result<(SeamGateOutcome, f64), SeamGateFailure> {
            self.score_calls += 1;
            self.scores
                .iter()
                .find(|(r, _)| *r == refined)
                .map(|(_, res)| res.clone())
                .unwrap_or(Err(SeamGateFailure::StructureAlignmentFailed))
        }
        fn anchor_brackets(&mut self, _pre: f64, _post: f64) -> Vec<AnchorBracket> {
            self.anchor_calls += 1;
            self.brackets.clone()
        }
        fn finalize_residual(
            &mut self,
            outcome: SeamGateOutcome,
        ) -> Result<SeamGateOutcome, SeamGateFailure> {
            if self.finalize_vetoes.contains(&outcome.refined) {
                // A residual veto. `select_joint_fit_winner_with_residual` records this as
                // `best_below_floor` and walks on to the next candidate — the same fall-through arm
                // `ResidualHeadroomExceeded` takes, which needs no `SeamResidualVerdict` to construct.
                return Err(waveform_below_threshold(
                    outcome.report_pre,
                    outcome.report_post,
                    0.12,
                ));
            }
            Ok(outcome)
        }
    }

    fn config(
        fit_boundary_search: FitBoundarySearch,
        anchor_seam_mode: AnchorSeamMode,
    ) -> FitJointConfig {
        FitJointConfig {
            fit_boundary_search,
            anchor_seam_mode,
            start_min: 1_000,
            end_max: 2_000,
            step: 100,
            haystack_secs: 0.0,
        }
    }

    /// W1/E1: a High baseline short-circuits — anchor search is never even invoked.
    #[test]
    fn route_e1_baseline_high_short_circuits_anchor_never_invoked() {
        let baseline = rgf(1_000, 2_000);
        let mut source = ScriptedFitSource::new()
            .score_at(baseline, ok_score(baseline, FillConfidence::High, 0.99, 0.99, 0.99))
            .brackets(vec![scripted_bracket(rgf(800, 2_200), 400)]);
        let out = evaluate_seam_gate_fit_joint_core(
            baseline,
            &config(FitBoundarySearch::FullGrid, AnchorSeamMode::Force),
            &mut source,
        )
        .expect("E1 patch");
        assert_eq!(out.confidence, FillConfidence::High);
        assert!(!out.anchor_seam_used);
        assert_eq!(source.anchor_calls, 0, "anchor search must not run when baseline is High");
    }

    /// W2/E2: a Marginal baseline is accepted without grid or anchor under baseline-only.
    #[test]
    fn route_e2_baseline_marginal_accepts_under_baseline_only() {
        let baseline = rgf(1_000, 2_000);
        let mut source = ScriptedFitSource::new()
            .score_at(baseline, ok_score(baseline, FillConfidence::Marginal, 0.30, 0.32, 0.30));
        let out = evaluate_seam_gate_fit_joint_core(
            baseline,
            &config(FitBoundarySearch::BaselineOnly, AnchorSeamMode::Auto),
            &mut source,
        )
        .expect("E2 patch");
        assert_eq!(out.confidence, FillConfidence::Marginal);
        assert!(!out.anchor_seam_used);
        assert_eq!(source.anchor_calls, 0, "Marginal baseline accepts before anchor search");
    }

    /// W5/E5: symmetric-weak throat with no anchors → skip.
    /// W5 symmetric weak skip records the throat failure and the best anchor/grid attempt.
    #[test]
    fn skip_reports_best_waveform_when_anchor_scores_higher_but_still_fails() {
        let baseline = rgf(1_000, 2_000);
        let anchor = rgf(800, 2_200);
        let mut source = ScriptedFitSource::new()
            .score_at(baseline, waveform_skip(0.06, 0.06))
            .score_at(anchor, waveform_skip(0.18, 0.31))
            .brackets(vec![scripted_bracket(anchor, 400)]);
        let result = evaluate_seam_gate_fit_joint_core(
            baseline,
            &config(FitBoundarySearch::BaselineOnly, AnchorSeamMode::Force),
            &mut source,
        );
        let Err(SeamGateFailure::WaveformBelowThreshold {
            pre,
            post,
            best_attempt,
            ..
        }) = result
        else {
            panic!("expected waveform skip");
        };
        assert!((pre - 0.06).abs() < 1e-9 && (post - 0.06).abs() < 1e-9);
        let best = best_attempt.expect("best attempt across fall-through");
        assert!((best.pre_correlation - 0.18).abs() < 1e-9);
        assert!((best.post_correlation - 0.31).abs() < 1e-9);
        assert_eq!(best.source, SeamScoreSource::Anchor);
    }

    #[test]
    fn route_w5_symmetric_weak_skips() {
        let baseline = rgf(1_000, 2_000);
        let mut source =
            ScriptedFitSource::new().score_at(baseline, waveform_skip(0.14, 0.14));
        let result = evaluate_seam_gate_fit_joint_core(
            baseline,
            &config(FitBoundarySearch::BaselineOnly, AnchorSeamMode::Auto),
            &mut source,
        );
        assert!(
            matches!(result, Err(SeamGateFailure::WaveformBelowThreshold { .. })),
            "W5 must skip as a waveform-below-threshold failure"
        );
    }

    /// W5 + anchor rescue / E3: SAME weak throat as above, but one strong anchor bracket flips
    /// skip → patch. This is the original blind spot as a two-line diff.
    #[test]
    fn route_w5_anchor_rescue_e3() {
        let baseline = rgf(1_000, 2_000);
        let anchor = rgf(800, 2_200);
        let mut source = ScriptedFitSource::new()
            .score_at(baseline, waveform_skip(0.14, 0.14))
            .score_at(anchor, ok_score(anchor, FillConfidence::High, 0.55, 0.55, 0.31))
            .brackets(vec![scripted_bracket(anchor, 400)]);
        let out = evaluate_seam_gate_fit_joint_core(
            baseline,
            &config(FitBoundarySearch::BaselineOnly, AnchorSeamMode::Auto),
            &mut source,
        )
        .expect("E3 anchor rescue patch");
        assert_eq!(out.confidence, FillConfidence::High);
        assert!(out.anchor_seam_used, "the anchor path must be what patched this gap");
        assert!(out.anchor_bracket_move_frames > 0);
    }

    /// Anchor marginal rescue / E4: weak throat, the only anchor bracket scores Marginal (not High),
    /// so it is accepted under baseline-only without the grid.
    #[test]
    fn route_e4_anchor_marginal_accepts() {
        let baseline = rgf(1_000, 2_000);
        let anchor = rgf(800, 2_200);
        let mut source = ScriptedFitSource::new()
            .score_at(baseline, waveform_skip(0.14, 0.14))
            .score_at(anchor, ok_score(anchor, FillConfidence::Marginal, 0.30, 0.30, 0.20))
            .brackets(vec![scripted_bracket(anchor, 400)]);
        let out = evaluate_seam_gate_fit_joint_core(
            baseline,
            &config(FitBoundarySearch::BaselineOnly, AnchorSeamMode::Auto),
            &mut source,
        )
        .expect("E4 anchor marginal accept");
        assert_eq!(out.confidence, FillConfidence::Marginal);
        assert!(out.anchor_seam_used, "the Marginal anchor must be what patched this gap");
        assert!(out.anchor_bracket_move_frames > 0);
    }

    /// Force fall-through / E5: a Marginal baseline withheld by `force`, no better anchor, patches
    /// the marginal baseline (the 3b divergence, resolved to the defer/production behaviour).
    #[test]
    fn route_force_marginal_fallthrough_patches_marginal() {
        let baseline = rgf(1_000, 2_000);
        let mut source = ScriptedFitSource::new()
            .score_at(baseline, ok_score(baseline, FillConfidence::Marginal, 0.30, 0.30, 0.30));
        let out = evaluate_seam_gate_fit_joint_core(
            baseline,
            &config(FitBoundarySearch::BaselineOnly, AnchorSeamMode::Force),
            &mut source,
        )
        .expect("force fall-through patches the marginal baseline");
        assert_eq!(out.confidence, FillConfidence::Marginal);
        assert!(!out.anchor_seam_used);
        assert_eq!(source.anchor_calls, 1, "force must consult anchor search before falling back");
    }

    /// E6: full grid scored; a moved-edge cell scores High and wins.
    #[test]
    fn route_e6_grid_high_patches() {
        let baseline = rgf(1_000, 2_000);
        let cell = rgf(900, 2_000); // a feasible grid cell (start pushed out by `step`)
        let mut source = ScriptedFitSource::new()
            .score_at(baseline, waveform_skip(0.14, 0.14))
            .score_at(cell, ok_score(cell, FillConfidence::High, 0.50, 0.50, 0.50));
        let cfg = FitJointConfig {
            fit_boundary_search: FitBoundarySearch::FullGrid,
            anchor_seam_mode: AnchorSeamMode::Off,
            start_min: 900,
            end_max: 2_100,
            step: 100,
            haystack_secs: 0.0,
        };
        let out = evaluate_seam_gate_fit_joint_core(baseline, &cfg, &mut source).expect("E6 grid patch");
        assert_eq!(out.confidence, FillConfidence::High);
        assert!(out.fit_used_boundary_grid, "grid winner must be tagged boundary_grid");
        assert!(!out.anchor_seam_used);
    }

    /// Residual-veto fall-through (E6 → E7): the top-ranked grid `High` is vetoed at finalize, so the
    /// next-ranked candidate wins. Exercises the lazy residual walk in
    /// `select_joint_fit_winner_with_residual` + `try_finalize_best_grid_high`. Also pins that the
    /// full grid is scored (no early-exit) via the `score_calls` counter.
    #[test]
    fn route_residual_veto_falls_through_to_next_candidate() {
        let baseline = rgf(1_000, 2_000);
        let top = rgf(900, 2_000); // higher rank, but its residual probe vetoes
        let next = rgf(1_000, 2_100); // lower rank, passes residual
        let mut source = ScriptedFitSource::new()
            .score_at(baseline, waveform_skip(0.14, 0.14))
            .score_at(top, ok_score(top, FillConfidence::High, 0.60, 0.60, 0.60))
            .score_at(next, ok_score(next, FillConfidence::High, 0.50, 0.50, 0.50))
            .veto_finalize(top);
        let cfg = FitJointConfig {
            fit_boundary_search: FitBoundarySearch::FullGrid,
            anchor_seam_mode: AnchorSeamMode::Off,
            start_min: 900,
            end_max: 2_100,
            step: 100,
            haystack_secs: 0.0,
        };
        let out = evaluate_seam_gate_fit_joint_core(baseline, &cfg, &mut source)
            .expect("next-ranked candidate wins after the top is vetoed");
        assert_eq!(out.confidence, FillConfidence::High);
        assert_eq!(
            out.refined.start_frame, next.start_frame,
            "vetoed top (start=900) must fall through to the next-ranked candidate (start=1000)"
        );
        // Baseline + 3 feasible grid cells = 4; the full grid is scored, no early-exit on first High.
        assert_eq!(source.score_calls, 4, "full grid must be scored");
    }
}
