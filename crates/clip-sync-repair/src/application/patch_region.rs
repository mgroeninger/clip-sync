//! Structure match + seam gate for one A gap bracket (supports boundary extension retries).

use clip_sync::MultiChannelPcm;

use crate::domain::fill_mode::FillMode;
use crate::domain::repair_profile::FitBoundarySearch;
use crate::domain::gap_fill_fit::{
    boundary_search_step_frames, apply_residual_to_confidence, classify_fill_waveform_confidence,
    fit_candidate_ranking_score, match_gap_fill_unified_in_b_with_timeline, FillConfidence,
    ResidualGateError, UnifiedFillSearchInput, UnifiedFitWeights, WaveformSeamContext,
};
use crate::domain::residual_gate::ResidualGateMode;
use crate::domain::patch_anchor::AnchorSearchPrior;
use crate::domain::gap_seam_extend::{
    post_seam_extension_candidate, pre_seam_extension_candidate,
    short_gap_one_strong_seam_passes,
};
use crate::domain::gap_structure::{self, StructureMatchParams};
use crate::domain::gap_signature::{
    build_gap_signature, GapSignature, GapSignatureMode, StructureTimeline,
};
use crate::domain::gap_energy::EnergyTimeline;
use crate::domain::policies::{self, FillAlignment, GapBorderSpec, RefinedGapFrames};

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
}

pub(crate) struct SeamGateParams<'a> {
    pub a_pcm: &'a MultiChannelPcm,
    pub b_samples: &'a [i16],
    pub b_extract_start_secs: f64,
    pub refined_b_start_secs: f64,
    pub refined_b_end_secs: f64,
    pub channels: usize,
    pub sample_rate: u32,
    pub context_frames: usize,
    pub bin_frames: usize,
    pub seam_gate_frames: usize,
    pub border_frames: usize,
    pub border_standoff_frames: usize,
    pub search_radius_frames: usize,
    pub fill_length_slack_frames: usize,
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
    pub max_extend_frames: usize,
    pub step_frames: usize,
    pub gap_end_extend_on_post_seam_fail: bool,
    pub gap_start_extend_on_pre_seam_fail: bool,
    pub anchor_search_prior: Option<AnchorSearchPrior>,
    pub gap_signature_mode: GapSignatureMode,
    pub fit_boundary_search: FitBoundarySearch,
    /// P1 report-only: compute the residual/floor verdict per gap and attach it to the outcome/JSON.
    pub measure_residual: bool,
    /// Residual headroom gate mode (`off` = no gating; measurement still obeys `measure_residual`).
    pub residual_gate: ResidualGateMode,
    pub residual_floor_ok_db: f64,
    pub residual_headroom_margin_db: f64,
    pub residual_max_lag_frames: i64,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum SeamGateFailure {
    StructureAlignmentFailed,
    StructureBelowThreshold {
        pre: f64,
        post: f64,
    },
    WaveformBelowThreshold {
        pre: f64,
        post: f64,
        min: f32,
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
    if params.fill_mode == FillMode::Fit {
        evaluate_seam_gate_fit_joint(refined, params)
    } else {
        evaluate_seam_gate_legacy(refined, params)
    }
}

struct FitJointCandidate {
    outcome: SeamGateOutcome,
    ranking_score: f64,
    boundary_move: usize,
}

/// Reused B haystack mono, channels, and structure timelines for joint-grid candidates.
struct FitHaystackCache {
    b_mono: Vec<f64>,
    b_ch: Vec<Vec<f64>>,
    bool_timeline: gap_structure::ActivityTimeline,
    energy_timeline: EnergyTimeline,
}

impl FitHaystackCache {
    fn build(params: &SeamGateParams<'_>) -> Self {
        let channels = params.channels.max(1);
        let total_frames = params.b_samples.len() / channels;
        let bin_frames = params.bin_frames.max(1);
        Self {
            b_mono: policies::interleaved_to_mono(params.b_samples, channels),
            b_ch: policies::interleaved_to_channels(params.b_samples, channels),
            bool_timeline: gap_structure::ActivityTimeline::build(
                params.b_samples,
                channels,
                total_frames,
                bin_frames,
                params.silence_peak_fraction,
                params.absolute_silence_rms,
            ),
            energy_timeline: EnergyTimeline::build(
                params.b_samples,
                channels,
                total_frames,
                bin_frames,
                params.silence_peak_fraction,
                params.absolute_silence_rms,
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

fn record_fit_joint_candidate(
    best: &mut Option<FitJointCandidate>,
    best_below_floor: &mut Option<SeamGateFailure>,
    refined: RefinedGapFrames,
    baseline: RefinedGapFrames,
    params: &SeamGateParams<'_>,
    cache: &FitHaystackCache,
) {
    match evaluate_seam_gate_fit_candidate(refined, baseline, params, cache) {
        Ok((outcome, ranking_score)) => {
            let boundary_move = baseline.start_frame.abs_diff(refined.start_frame)
                + baseline.end_frame.abs_diff(refined.end_frame);
            let replace = best.as_ref().is_none_or(|current| {
                ranking_score > current.ranking_score + 1e-9
                    || (ranking_score >= current.ranking_score - 1e-9
                        && boundary_move < current.boundary_move)
            });
            if replace {
                *best = Some(FitJointCandidate {
                    outcome,
                    ranking_score,
                    boundary_move,
                });
            }
        }
        Err(SeamGateFailure::WaveformBelowThreshold { pre, post, min }) => {
            if best_below_floor.is_none() {
                *best_below_floor = Some(SeamGateFailure::WaveformBelowThreshold {
                    pre,
                    post,
                    min,
                });
            }
        }
        Err(fail @ SeamGateFailure::ResidualHeadroomExceeded { .. }) => {
            if best_below_floor.is_none() {
                *best_below_floor = Some(fail);
            }
        }
        Err(other) => {
            if best.is_none() && best_below_floor.is_none() {
                *best_below_floor = Some(other);
            }
        }
    }
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
    search == FitBoundarySearch::BaselineOnly
        && matches!(
            confidence,
            FillConfidence::High | FillConfidence::Marginal
        )
}

fn fit_haystack_secs(params: &SeamGateParams<'_>) -> f64 {
    let channels = params.channels.max(1);
    params.b_samples.len() as f64 / channels as f64 / f64::from(params.sample_rate)
}

fn evaluate_seam_gate_fit_joint(
    baseline: RefinedGapFrames,
    params: &SeamGateParams<'_>,
) -> Result<SeamGateOutcome, SeamGateFailure> {
    let step = boundary_search_step_frames(params.max_extend_frames, params.step_frames);
    let total_frames = params.a_pcm.samples.len() / params.channels.max(1);
    let haystack_secs = fit_haystack_secs(params);

    let start_min = if params.gap_start_extend_on_pre_seam_fail {
        baseline.start_frame.saturating_sub(params.max_extend_frames)
    } else {
        baseline.start_frame
    };
    let end_max = if params.gap_end_extend_on_post_seam_fail {
        (baseline.end_frame + params.max_extend_frames).min(total_frames)
    } else {
        baseline.end_frame
    };

    let mut best: Option<FitJointCandidate> = None;
    let mut best_below_floor: Option<SeamGateFailure> = None;
    let cache = FitHaystackCache::build(params);

    record_fit_joint_candidate(&mut best, &mut best_below_floor, baseline, baseline, params, &cache);
    let baseline_high = best
        .as_ref()
        .is_some_and(|c| c.outcome.confidence == FillConfidence::High);
    if baseline_high {
        let mut outcome = best.expect("baseline high").outcome;
        outcome.fit_haystack_secs = haystack_secs;
        return Ok(outcome);
    }

    if let Some(candidate) = &best {
        if accepts_baseline_without_boundary_grid(
            params.fit_boundary_search,
            candidate.outcome.confidence,
        ) {
            if candidate.outcome.confidence == FillConfidence::Marginal {
                tracing::warn!(
                    pre = candidate.outcome.report_pre,
                    post = candidate.outcome.report_post,
                    min = params.min_fill_correlation,
                    "marginal waveform seam patch (below min_fill_correlation)"
                );
            }
            let mut outcome = candidate.outcome.clone();
            outcome.fit_haystack_secs = haystack_secs;
            return Ok(outcome);
        }
    }
    if params.fit_boundary_search == FitBoundarySearch::BaselineOnly {
        return Err(best_below_floor.unwrap_or(SeamGateFailure::StructureAlignmentFailed));
    }

    let grid_cells = count_joint_boundary_grid_cells(baseline, start_min, end_max, step);

    let mut try_start = baseline.start_frame;
    while try_start >= start_min {
        let mut try_end = baseline.end_frame;
        while try_end <= end_max {
            if try_end > try_start
                && (try_start != baseline.start_frame || try_end != baseline.end_frame)
            {
                record_fit_joint_candidate(
                    &mut best,
                    &mut best_below_floor,
                    RefinedGapFrames {
                        start_frame: try_start,
                        end_frame: try_end,
                    },
                    baseline,
                    params,
                    &cache,
                );
                if best
                    .as_ref()
                    .is_some_and(|c| c.outcome.confidence == FillConfidence::High)
                {
                    return Ok(best.expect("high joint candidate").outcome);
                }
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

    if let Some(mut candidate) = best {
        if candidate.outcome.confidence == FillConfidence::Marginal {
            tracing::warn!(
                pre = candidate.outcome.report_pre,
                post = candidate.outcome.report_post,
                min = params.min_fill_correlation,
                "marginal waveform seam patch (below min_fill_correlation)"
            );
        }
        candidate.outcome.fit_used_boundary_grid = true;
        candidate.outcome.fit_boundary_grid_cells = Some(grid_cells);
        candidate.outcome.fit_haystack_secs = haystack_secs;
        return Ok(candidate.outcome);
    }

    Err(best_below_floor.unwrap_or(SeamGateFailure::StructureAlignmentFailed))
}

fn want_residual_measurement(params: &SeamGateParams<'_>) -> bool {
    params.measure_residual
        || params.residual_gate.is_active()
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
        a_samples: &params.a_pcm.samples,
        channels: params.channels,
        b_mono: &cache.b_mono,
        window,
        standoff_frames: params.border_standoff_frames,
        a_to_b_delta: nominal_delta,
        step_frames: window.max(1),
        max_walk_frames: params.sample_rate as usize * 3,
        absolute_silence_rms: params.absolute_silence_rms,
        max_lag_frames: params.residual_max_lag_frames,
    };
    let (chosen_pre, floor_pre) = policies::seam_chosen_and_floor(
        &floor_common(waveform_gate_frames),
        policies::SeamSide::Pre,
        refined.start_frame,
        refined.end_frame,
        chosen_delta,
    );
    let (chosen_post, floor_post) = if post_gate_frames == 0 {
        // Window 0 → `select_reference_window` abstains; skip spurious 1-frame post fit (L7).
        policies::seam_chosen_and_floor(
            &floor_common(0),
            policies::SeamSide::Post,
            refined.start_frame,
            refined.end_frame,
            chosen_delta,
        )
    } else {
        policies::seam_chosen_and_floor(
            &floor_common(post_gate_frames),
            policies::SeamSide::Post,
            refined.start_frame,
            refined.end_frame,
            chosen_delta,
        )
    };
    Some(policies::SeamResidualVerdict::from_parts_with_placement(
        &chosen_pre,
        &chosen_post,
        &floor_pre,
        &floor_post,
        params.residual_floor_ok_db,
        alignment_start_frame.abs_diff(offset_nominal_start) as u64,
        params.residual_max_lag_frames,
    ))
}

fn evaluate_seam_gate_fit_candidate(
    refined: RefinedGapFrames,
    baseline: RefinedGapFrames,
    params: &SeamGateParams<'_>,
    cache: &FitHaystackCache,
) -> Result<(SeamGateOutcome, f64), SeamGateFailure> {
    let gap_frames = refined.end_frame.saturating_sub(refined.start_frame);
    if gap_frames == 0 {
        return Err(SeamGateFailure::StructureAlignmentFailed);
    }

    let border_spec = GapBorderSpec {
        gap_start_frame: refined.start_frame,
        gap_end_frame: refined.end_frame,
        border_frames: params.border_frames,
        border_standoff_frames: params.border_standoff_frames,
        silence_peak_fraction: params.silence_peak_fraction,
        absolute_rms_floor: params.absolute_silence_rms,
    };
    let (a_pre_border, a_post_border) =
        policies::border_templates_for_gap(&params.a_pcm.samples, params.channels, &border_spec);
    let (a_pre_ch, a_post_ch) = policies::border_templates_per_channel_for_gap(
        &params.a_pcm.samples,
        params.channels,
        &border_spec,
    );

    let gap_secs = gap_frames as f64 / params.sample_rate as f64;
    let start_delta_secs = (refined.start_frame as i64 - baseline.start_frame as i64) as f64
        / params.sample_rate as f64;
    let end_delta_secs = (refined.end_frame as i64 - baseline.end_frame as i64) as f64
        / params.sample_rate as f64;
    let refined_b_start_secs = params.refined_b_start_secs + start_delta_secs;
    let refined_b_end_secs = params.refined_b_end_secs + end_delta_secs;

    let offset_nominal_start = ((refined_b_start_secs - params.b_extract_start_secs)
        * params.sample_rate as f64)
        .round() as usize;
    let gap_end_in_haystack = ((refined_b_end_secs - params.b_extract_start_secs)
        * params.sample_rate as f64)
        .round() as usize;

    let structure_params = StructureMatchParams {
        gap_frames,
        bin_frames: params.bin_frames.max(1),
        search_radius_frames: params.search_radius_frames,
        fill_length_slack_frames: params.fill_length_slack_frames,
        max_fine_adjustment_frames: gap_structure::structure_fine_polish_frames(params.bin_frames),
        silence_peak_fraction: params.silence_peak_fraction,
        absolute_silence_rms: params.absolute_silence_rms,
    };

    let signature = build_gap_signature(
        &params.a_pcm.samples,
        params.channels,
        refined.start_frame,
        refined.end_frame,
        params.context_frames,
        &structure_params,
        params.gap_signature_mode,
    );

    let waveform_gate_frames = params
        .seam_gate_frames
        .min(a_pre_border.len().max(1));
    let post_gate_frames = seam_post_gate_frames(params.seam_gate_frames, a_post_border.len());
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
        repeat_window_frames: params.border_frames.max(1),
        repeat_penalty_weight: params.fill_repeat_penalty_weight,
    };
    // Mode-coupled nominal bias: an energy-resolved signature is the signal that the alignment
    // nominal map may be wrong, so loosen the distance-from-nominal penalty (the penalty grows
    // linearly with distance, so this mainly frees far-off / drifted candidates). Bool keeps base.
    let nominal_bias_scale = match signature {
        GapSignature::Energy(_) => params.fill_fit_energy_nominal_bias_scale,
        GapSignature::Bool(_) => params.fill_fit_nominal_bias_scale,
    };
    let weights = UnifiedFitWeights {
        structure_weight: params.fill_fit_structure_weight,
        waveform_weight: params.fill_fit_waveform_weight,
        nominal_bias_scale,
        late_start_penalty_scale: params.fill_fit_late_start_penalty_scale,
    };
    let structure_timeline = cache.structure_timeline(&signature);
    let search_input = UnifiedFillSearchInput {
        signature: &signature,
        b_samples: params.b_samples,
        channels: params.channels,
        waveform: &waveform,
        nominal_fill_start: offset_nominal_start,
        nominal_fill_end: gap_end_in_haystack,
    };
    let unified = match_gap_fill_unified_in_b_with_timeline(
        &search_input,
        &structure_params,
        weights,
        &structure_timeline,
        params.anchor_search_prior,
    )
    .ok_or(SeamGateFailure::StructureAlignmentFailed)?;

    let alignment = unified.alignment;
    let structure_pre = unified.structure_pre;
    let structure_post = unified.structure_post;
    let structure_start_frame = alignment.start_frame;

    // Per-gap seam diagnostics (RUST_LOG=debug): which channels were scored and their
    // per-channel correlations at the winning placement — explains low pre/post on surround.
    if tracing::enabled!(tracing::Level::DEBUG) {
        let diag = policies::seam_channel_diagnostics(
            &templates,
            policies::SeamPlacement {
                start: alignment.start_frame,
                gap_frames,
                pre_window: waveform_gate_frames,
                post_window: post_gate_frames,
            },
        );
        tracing::debug!(
            start_frame = alignment.start_frame,
            seam_pre = alignment.pre_correlation,
            seam_post = alignment.post_correlation,
            structure_pre,
            structure_post,
            signature = signature.mode_label(),
            selected_channels = ?diag.selected,
            per_channel = ?diag.per_channel,
            mono_pre = diag.mono.0,
            mono_post = diag.mono.1,
            "fill seam channel diagnostics"
        );

    }

    let residual = measure_fit_residual_verdict(
        params,
        cache,
        refined,
        alignment.start_frame,
        offset_nominal_start,
        waveform_gate_frames,
        post_gate_frames,
    );
    if let Some(verdict) = residual {
        tracing::debug!(
            start_frame = alignment.start_frame,
            nominal_start = offset_nominal_start,
            seam_pre = alignment.pre_correlation,
            seam_post = alignment.post_correlation,
            chosen_pre_db = verdict.chosen_pre_db,
            chosen_post_db = verdict.chosen_post_db,
            floor_pre_db = verdict.floor_pre_db,
            floor_post_db = verdict.floor_post_db,
            headroom_db = verdict.worst_headroom_db(),
            informative = verdict.informative,
            residual_gate = ?params.residual_gate,
            "fill seam residual verdict"
        );
    }

    if !structure_passes_gate(
        structure_pre,
        structure_post,
        params.min_structure_match_score,
        gap_secs,
        params.short_gap_mean_correlation_secs,
    ) {
        return Err(SeamGateFailure::StructureBelowThreshold {
            pre: structure_pre,
            post: structure_post,
        });
    }

    let pre_corr = alignment.pre_correlation;
    let post_corr = alignment.post_correlation;
    let pearson = classify_fill_waveform_confidence(
        pre_corr,
        post_corr,
        params.min_fill_correlation,
        params.fill_marginal_margin,
        params.fill_absolute_floor,
    );

    let confidence = if params.residual_gate.is_active() {
        let verdict = residual.ok_or(SeamGateFailure::StructureAlignmentFailed)?;
        match apply_residual_to_confidence(
            pearson,
            &verdict,
            params.residual_headroom_margin_db,
            params.residual_gate.rescue_enabled(),
        ) {
            Ok(confidence) => confidence,
            Err(ResidualGateError::HeadroomExceeded { margin_db, .. }) => {
                return Err(SeamGateFailure::ResidualHeadroomExceeded {
                    pre: pre_corr,
                    post: post_corr,
                    residual: verdict,
                    margin_db,
                });
            }
            Err(ResidualGateError::PearsonBelowFloor(_)) => {
                return Err(SeamGateFailure::WaveformBelowThreshold {
                    pre: pre_corr,
                    post: post_corr,
                    min: params.fill_absolute_floor,
                });
            }
        }
    } else {
        pearson.map_err(|_| SeamGateFailure::WaveformBelowThreshold {
            pre: pre_corr,
            post: post_corr,
            min: params.fill_absolute_floor,
        })?
    };

    let boundary_move = baseline.start_frame.abs_diff(refined.start_frame)
        + baseline.end_frame.abs_diff(refined.end_frame);
    let ranking_score =
        fit_candidate_ranking_score(pre_corr.min(post_corr), boundary_move);

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

    let gap_secs = gap_frames as f64 / params.sample_rate as f64;
    let border_spec = GapBorderSpec {
        gap_start_frame: refined.start_frame,
        gap_end_frame: refined.end_frame,
        border_frames: params.border_frames,
        border_standoff_frames: params.border_standoff_frames,
        silence_peak_fraction: params.silence_peak_fraction,
        absolute_rms_floor: params.absolute_silence_rms,
    };
    let (a_pre_border, a_post_border) =
        policies::border_templates_for_gap(&params.a_pcm.samples, params.channels, &border_spec);
    let (a_pre_ch, a_post_ch) = policies::border_templates_per_channel_for_gap(
        &params.a_pcm.samples,
        params.channels,
        &border_spec,
    );
    let b_mono = policies::interleaved_to_mono(params.b_samples, params.channels);
    let b_ch = policies::interleaved_to_channels(params.b_samples, params.channels);

    let offset_nominal_start = ((params.refined_b_start_secs - params.b_extract_start_secs)
        * params.sample_rate as f64)
        .round() as usize;
    let gap_end_in_haystack = ((params.refined_b_end_secs - params.b_extract_start_secs)
        * params.sample_rate as f64)
        .round() as usize;

    let structure_params = StructureMatchParams {
        gap_frames,
        bin_frames: params.bin_frames.max(1),
        search_radius_frames: params.search_radius_frames,
        fill_length_slack_frames: params.fill_length_slack_frames,
        max_fine_adjustment_frames: gap_structure::structure_fine_polish_frames(params.bin_frames),
        silence_peak_fraction: params.silence_peak_fraction,
        absolute_silence_rms: params.absolute_silence_rms,
    };

    let signature = gap_structure::build_gap_context_signature(
        &params.a_pcm.samples,
        params.channels,
        refined.start_frame,
        refined.end_frame,
        params.context_frames,
        &structure_params,
    );

    let mut alignment = gap_structure::match_gap_structure_in_b(
        &signature,
        params.b_samples,
        params.channels,
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
        params.min_structure_match_score,
        gap_secs,
        params.short_gap_mean_correlation_secs,
    ) {
        return Err(SeamGateFailure::StructureBelowThreshold {
            pre: structure_pre,
            post: structure_post,
        });
    }

    let structure_trusted = !params.disable_structure_trust
        && structure_pre >= params.strong_structure_trust
        && structure_post >= params.strong_structure_trust;

    let (report_pre, report_post, patched_structure_trusted) = if structure_trusted {
        (structure_pre, structure_post, true)
    } else {
        let waveform_gate_frames = params
            .seam_gate_frames
            .min(a_pre_border.len().max(1));
        let post_gate_frames = seam_post_gate_frames(params.seam_gate_frames, a_post_border.len());
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

        let soften_waveform_gate = !params.disable_structure_trust
            && structure_pre >= params.partial_structure_waveform_soften
            && structure_post >= params.partial_structure_waveform_soften;
        let effective_min_corr = if soften_waveform_gate {
            params
                .min_fill_correlation
                .min(PARTIAL_WAVEFORM_MIN_CORRELATION)
        } else {
            params.min_fill_correlation
        };

        alignment.pre_correlation = pre_corr;
        alignment.post_correlation = post_corr;

        if !seams_pass_correlation_gate(
            &alignment,
            effective_min_corr,
            gap_secs,
            params.short_gap_mean_correlation_secs,
            params.short_gap_one_strong_seam_fallback,
            params.disable_structure_trust,
        ) {
            return Err(SeamGateFailure::WaveformBelowThreshold {
                pre: pre_corr,
                post: post_corr,
                min: effective_min_corr,
            });
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
) -> Result<SeamGateOutcome, SeamGateFailure> {
    let SeamGateFailure::WaveformBelowThreshold { pre, post, min } = initial_fail else {
        return Err(initial_fail);
    };

    if !post_seam_extension_candidate(pre, post, min) {
        return Err(initial_fail);
    }

    let total_frames = params.a_pcm.samples.len() / params.channels.max(1);
    let original_end = refined.end_frame;
    let max_end = (original_end + max_extend_frames).min(total_frames);
    if step_frames == 0 || original_end >= max_end {
        return Err(initial_fail);
    }

    let mut try_end = original_end;
    let mut last_fail = initial_fail;
    while try_end + step_frames <= max_end {
        try_end += step_frames;
        refined.end_frame = try_end;
        let refined_b_end_secs = try_end as f64 / params.sample_rate as f64 + gap_offset_secs;
        let try_params = SeamGateParams {
            refined_b_end_secs,
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
            Err(fail @ SeamGateFailure::WaveformBelowThreshold { .. }) => {
                last_fail = fail;
            }
            Err(other) => {
                refined.end_frame = original_end;
                return Err(other);
            }
        }
    }

    refined.end_frame = original_end;
    Err(last_fail)
}

/// Shift `refined.start_frame` earlier in steps when the pre seam failed waveform correlation.
pub(crate) fn try_extend_gap_start_for_pre_seam(
    refined: &mut RefinedGapFrames,
    gap_offset_secs: f64,
    params: &SeamGateParams<'_>,
    initial_fail: SeamGateFailure,
    max_extend_frames: usize,
    step_frames: usize,
) -> Result<SeamGateOutcome, SeamGateFailure> {
    let SeamGateFailure::WaveformBelowThreshold { pre, post, min } = initial_fail else {
        return Err(initial_fail);
    };

    if !pre_seam_extension_candidate(pre, post, min) {
        return Err(initial_fail);
    }

    let original_start = refined.start_frame;
    let min_start = original_start.saturating_sub(max_extend_frames);
    if step_frames == 0 || original_start <= min_start {
        return Err(initial_fail);
    }

    let mut try_start = original_start;
    let mut last_fail = initial_fail;
    while try_start >= step_frames && try_start - step_frames >= min_start {
        try_start -= step_frames;
        refined.start_frame = try_start;
        let refined_b_start_secs = try_start as f64 / params.sample_rate as f64 + gap_offset_secs;
        let try_params = SeamGateParams {
            refined_b_start_secs,
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
            Err(fail @ SeamGateFailure::WaveformBelowThreshold { .. }) => {
                last_fail = fail;
            }
            Err(other) => {
                refined.start_frame = original_start;
                return Err(other);
            }
        }
    }

    refined.start_frame = original_start;
    Err(last_fail)
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
    let SeamGateFailure::WaveformBelowThreshold { .. } = initial_fail else {
        return Err(initial_fail);
    };

    let step = retry.step_frames.max(1);
    let mut last_fail = initial_fail;

    if retry.gap_end_extend_on_post_seam_fail {
        match try_extend_gap_end_for_post_seam(
            refined,
            gap_offset_secs,
            params,
            last_fail,
            retry.max_extend_frames,
            step,
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
        );
    }

    Err(last_fail)
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
}
