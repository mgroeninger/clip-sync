//! Structure match + seam gate for one A gap bracket (supports boundary extension retries).

use clip_sync::MultiChannelPcm;

use crate::domain::fill_mode::FillMode;
use crate::domain::gap_fill_fit::{fit_mode_waveform_floor_passes, search_best_waveform_placement};
use crate::domain::gap_seam_extend::{
    post_seam_extension_candidate, pre_seam_extension_candidate,
    short_gap_one_strong_seam_passes,
};
use crate::domain::gap_structure::{self, StructureMatchParams};
use crate::domain::policies::{self, FillAlignment, GapBorderSpec, RefinedGapFrames};

/// Pearson floor used when partial structure trust softens the waveform gate.
pub(crate) const PARTIAL_WAVEFORM_MIN_CORRELATION: f32 = 0.12;

/// Outcome of structure match and optional waveform seam gate.
pub(crate) struct SeamGateOutcome {
    pub alignment: FillAlignment,
    pub report_pre: f64,
    pub report_post: f64,
    pub structure_trusted: bool,
    pub structure_start_frame: usize,
    pub gap_frames: usize,
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
    pub max_adjustment_frames: usize,
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
}

pub(crate) fn evaluate_seam_gate(
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
        max_fine_adjustment_frames: params.max_adjustment_frames,
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

    let structure_trusted = params.fill_mode == FillMode::Gate
        && !params.disable_structure_trust
        && structure_pre >= params.strong_structure_trust
        && structure_post >= params.strong_structure_trust;

    let (report_pre, report_post, patched_structure_trusted) = if structure_trusted {
        (structure_pre, structure_post, true)
    } else {
        let waveform_gate_frames = params
            .seam_gate_frames
            .min(a_pre_border.len().max(1));
        let post_gate_frames = params.seam_gate_frames.min(a_post_border.len()).max(1);
        let templates = policies::SeamTemplates {
            a_pre: &a_pre_border,
            a_post: &a_post_border,
            a_pre_ch: &a_pre_ch,
            a_post_ch: &a_post_ch,
            b_mono: &b_mono,
            b_ch: &b_ch,
        };

        match params.fill_mode {
            FillMode::Fit => {
                alignment = search_best_waveform_placement(
                    &templates,
                    &alignment,
                    gap_frames,
                    params.max_adjustment_frames,
                    waveform_gate_frames,
                    post_gate_frames,
                    b_mono.len(),
                );
                let pre_corr = alignment.pre_correlation;
                let post_corr = alignment.post_correlation;
                if !fit_mode_waveform_floor_passes(pre_corr, post_corr, params.min_fill_correlation)
                {
                    return Err(SeamGateFailure::WaveformBelowThreshold {
                        pre: pre_corr,
                        post: post_corr,
                        min: params.min_fill_correlation,
                    });
                }
                (pre_corr, post_corr, false)
            }
            FillMode::Gate => {
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
            }
        }
    };

    Ok(SeamGateOutcome {
        alignment,
        report_pre,
        report_post,
        structure_trusted: patched_structure_trusted,
        structure_start_frame,
        gap_frames,
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
}
