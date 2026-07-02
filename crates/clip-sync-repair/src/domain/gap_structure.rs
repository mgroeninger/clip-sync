//! Structure-based gap fill alignment.
//!
//! Matches active/silent edit patterns in a window around A's dropout against B,
//! then optionally fine-tunes start position with the same representation.

use crate::domain::policies::{FillAlignment, is_silent_frame};

/// Binned active/silent pattern on each side of A's gap (index 0 = nearest the dropout).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GapContextSignature {
    pub pre_bins: Vec<bool>,
    pub post_bins: Vec<bool>,
}

/// Parameters for sliding a gap-context signature across B.
#[derive(Debug, Clone, Copy)]
pub struct StructureMatchParams {
    pub gap_frames: usize,
    pub bin_frames: usize,
    pub search_radius_frames: usize,
    pub fill_length_slack_frames: usize,
    pub max_fine_adjustment_frames: usize,
    pub silence_peak_fraction: f32,
    pub absolute_silence_rms: f32,
}

const SCORE_TIE_EPSILON: f64 = 1e-6;
const LENGTH_MISMATCH_PENALTY: f64 = 0.12;
const NOMINAL_BIAS_PER_BIN: f64 = 0.02;

/// Precomputed active/silent bins for an entire B haystack.
#[doc(hidden)]
pub struct ActivityTimeline {
    bins: Vec<bool>,
    bin_frames: usize,
}

impl ActivityTimeline {
    #[doc(hidden)]
    pub fn build(
        samples: &[f32],
        channels: usize,
        total_frames: usize,
        bin_frames: usize,
        silence_peak_fraction: f32,
        absolute_silence_rms: f32,
    ) -> Self {
        Self {
            bins: activity_bins(
                samples,
                channels,
                0,
                total_frames,
                bin_frames,
                silence_peak_fraction,
                absolute_silence_rms,
            ),
            bin_frames: bin_frames.max(1),
        }
    }

    fn bins_for_frames(&self, start_frame: usize, end_frame: usize) -> Option<&[bool]> {
        if start_frame >= end_frame {
            return None;
        }
        let start_bin = start_frame / self.bin_frames;
        let end_bin = end_frame.div_ceil(self.bin_frames);
        self.bins.get(start_bin..end_bin)
    }
}

/// Build a coarse active/silent signature around a gap on A's timeline.
///
/// Uses `bin_frames` and the silence thresholds from `params`; the other match-search fields
/// are not read here.
pub fn build_gap_context_signature(
    samples: &[f32],
    channels: usize,
    gap_start_frame: usize,
    gap_end_frame: usize,
    context_frames: usize,
    params: &StructureMatchParams,
) -> GapContextSignature {
    let silence_peak_fraction = params.silence_peak_fraction;
    let absolute_silence_rms = params.absolute_silence_rms;
    let channels = channels.max(1);
    let bin_frames = params.bin_frames.max(1);
    let total_frames = samples.len() / channels;

    let pre_start = gap_start_frame.saturating_sub(context_frames);
    let pre_end = gap_start_frame.min(total_frames);
    let post_start = gap_end_frame.min(total_frames);
    let post_end = (gap_end_frame + context_frames).min(total_frames);

    let pre_bins = activity_bins(
        samples,
        channels,
        pre_start,
        pre_end,
        bin_frames,
        silence_peak_fraction,
        absolute_silence_rms,
    );
    let post_bins = activity_bins(
        samples,
        channels,
        post_start,
        post_end,
        bin_frames,
        silence_peak_fraction,
        absolute_silence_rms,
    );

    GapContextSignature { pre_bins, post_bins }
}

/// Locate a B fill bracket by matching A's surrounding edit structure.
pub fn match_gap_structure_in_b(
    signature: &GapContextSignature,
    b_samples: &[f32],
    channels: usize,
    nominal_fill_start: usize,
    nominal_fill_end: usize,
    params: &StructureMatchParams,
) -> Option<FillAlignment> {
    if signature.pre_bins.is_empty()
        || signature.post_bins.is_empty()
        || params.gap_frames == 0
        || params.bin_frames == 0
    {
        return None;
    }

    let channels = channels.max(1);
    let total_frames = b_samples.len() / channels;
    let pre_span = signature.pre_bins.len() * params.bin_frames;
    let post_span = signature.post_bins.len() * params.bin_frames;

    if pre_span == 0 || post_span == 0 {
        return None;
    }

    let timeline = ActivityTimeline::build(
        b_samples,
        channels,
        total_frames,
        params.bin_frames,
        params.silence_peak_fraction,
        params.absolute_silence_rms,
    );

    let (mut best_start, _) = search_best_fill_start(
        signature,
        &timeline,
        nominal_fill_start,
        pre_span,
        total_frames,
        params,
    )?;

    let (mut best_end, _) = search_best_fill_end(
        signature,
        &timeline,
        best_start,
        nominal_fill_end,
        post_span,
        total_frames,
        params,
    )?;

    let matched_fill_len = best_end.saturating_sub(best_start);
    let polished_start = fine_polish_structure_start(
        signature,
        &timeline,
        best_start,
        best_end,
        nominal_fill_start,
        nominal_fill_end,
        params,
    );
    best_start = polished_start;
    best_end = best_start + matched_fill_len;
    let best_pre = score_pre_match(signature, &timeline, best_start, params);
    let best_post = score_post_match(signature, &timeline, best_end, params);

    let fill_frames = best_end.saturating_sub(best_start);
    if fill_frames == 0 {
        return None;
    }

    let mut alignment = FillAlignment {
        start_frame: best_start,
        fill_frames,
        pre_correlation: best_pre,
        post_correlation: best_post,
    };
    snap_structure_fill_to_gap(&mut alignment, signature, &timeline, params);

    Some(alignment)
}

pub(crate) fn snap_structure_fill_to_gap(
    alignment: &mut FillAlignment,
    signature: &GapContextSignature,
    timeline: &ActivityTimeline,
    params: &StructureMatchParams,
) {
    if alignment.fill_frames == params.gap_frames || params.gap_frames == 0 {
        return;
    }

    let preferred_end = alignment.start_frame.saturating_add(params.gap_frames);
    let snapped_post = score_post_match(signature, timeline, preferred_end, params);
    if !snapped_post.is_finite() {
        return;
    }
    const SNAP_TOLERANCE: f64 = 0.08;
    if snapped_post + SNAP_TOLERANCE >= alignment.post_correlation {
        alignment.fill_frames = params.gap_frames;
        alignment.post_correlation = snapped_post;
    }
}

fn fine_polish_structure_start(
    signature: &GapContextSignature,
    timeline: &ActivityTimeline,
    start: usize,
    end: usize,
    nominal_start: usize,
    nominal_end: usize,
    params: &StructureMatchParams,
) -> usize {
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
        let pre_score = score_pre_match(signature, timeline, candidate, params);
        let post_score = score_post_match(signature, timeline, candidate_end, params);
        if !pre_score.is_finite() || !post_score.is_finite() {
            continue;
        }
        let mut score = combined_structure_score(
            pre_score,
            post_score,
            FillBracketPlacement {
                start: candidate,
                end: candidate_end,
                nominal_start,
                nominal_end,
            },
            params,
            1.0,
        );
        if candidate > nominal_start {
            let late_frac =
                (candidate - nominal_start) as f64 / params.gap_frames.max(1) as f64;
            score -= 0.08 * late_frac;
        }
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

pub(crate) fn search_coarse_step(bin_frames: usize, span_frames: usize) -> usize {
    let bin_frames = bin_frames.max(1);
    let span_steps = (span_frames / bin_frames).max(1);
    (span_steps / 2_000).max(1) * bin_frames
}

/// Sample-level polish radius after coarse bin search — not the waveform slide window.
///
/// `max_fill_align_adjustment_secs` controls B waveform placement elsewhere; using that
/// value here caused multi-second exhaustive loops per fit candidate.
#[doc(hidden)]
pub fn structure_fine_polish_frames(bin_frames: usize) -> usize {
    bin_frames.clamp(1, 128)
}

fn search_best_fill_start(
    signature: &GapContextSignature,
    timeline: &ActivityTimeline,
    nominal_fill_start: usize,
    pre_span: usize,
    total_frames: usize,
    params: &StructureMatchParams,
) -> Option<(usize, f64)> {
    let search_min = nominal_fill_start.saturating_sub(params.search_radius_frames);
    let search_max = (nominal_fill_start + params.search_radius_frames).min(total_frames);
    let span = search_max.saturating_sub(search_min);
    let coarse_step = search_coarse_step(params.bin_frames, span);

    let mut best_start = nominal_fill_start;
    let mut best_pre = f64::NEG_INFINITY;
    let mut best_score = f64::NEG_INFINITY;

    let consider = |start: usize,
                        best_start: &mut usize,
                        best_pre: &mut f64,
                        best_score: &mut f64| {
        if start < pre_span || start > search_max {
            return;
        }
        let pre_score = score_pre_match(signature, timeline, start, params);
        if !pre_score.is_finite() {
            return;
        }
        let nominal_bias = NOMINAL_BIAS_PER_BIN
            * start.abs_diff(nominal_fill_start) as f64 / params.bin_frames.max(1) as f64;
        let score = pre_score - nominal_bias;
        let better = score > *best_score + SCORE_TIE_EPSILON
            || (score >= *best_score - SCORE_TIE_EPSILON
                && prefer_start(start, *best_start, nominal_fill_start));
        if better {
            *best_score = score;
            *best_pre = pre_score;
            *best_start = start;
        }
    };

    if nominal_fill_start >= pre_span {
        consider(
            nominal_fill_start,
            &mut best_start,
            &mut best_pre,
            &mut best_score,
        );
    }

    let mut start = search_min;
    while start <= search_max {
        consider(start, &mut best_start, &mut best_pre, &mut best_score);
        start = start.saturating_add(coarse_step);
    }

    if !best_pre.is_finite() {
        return None;
    }

    let refine_min = best_start.saturating_sub(coarse_step).max(search_min);
    let refine_max = (best_start + coarse_step).min(search_max);
    for start in refine_min..=refine_max {
        consider(start, &mut best_start, &mut best_pre, &mut best_score);
    }

    Some((best_start, best_pre))
}

fn search_best_fill_end(
    signature: &GapContextSignature,
    timeline: &ActivityTimeline,
    fill_start: usize,
    nominal_fill_end: usize,
    post_span: usize,
    total_frames: usize,
    params: &StructureMatchParams,
) -> Option<(usize, f64)> {
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

    let mut best_end = nominal_fill_end.clamp(end_min, end_max);
    let mut best_post = f64::NEG_INFINITY;
    let mut best_score = f64::NEG_INFINITY;

    let consider =
        |end: usize, best_end: &mut usize, best_post: &mut f64, best_score: &mut f64| {
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
            let post_score = score_post_match(signature, timeline, end, params);
            if !post_score.is_finite() {
                return;
            }
            let len_penalty = LENGTH_MISMATCH_PENALTY
                * fill_len.abs_diff(params.gap_frames) as f64 / params.gap_frames.max(1) as f64;
            let nominal_bias = NOMINAL_BIAS_PER_BIN
                * end.abs_diff(nominal_fill_end) as f64 / params.bin_frames.max(1) as f64;
            let score = post_score - len_penalty - nominal_bias;
            let better = score > *best_score + SCORE_TIE_EPSILON
                || (score >= *best_score - SCORE_TIE_EPSILON
                    && prefer_end(end, *best_end, nominal_fill_end));
            if better {
                *best_score = score;
                *best_post = post_score;
                *best_end = end;
            }
        };

    consider(best_end, &mut best_end, &mut best_post, &mut best_score);

    let mut end = end_min;
    while end <= end_max {
        consider(end, &mut best_end, &mut best_post, &mut best_score);
        end = end.saturating_add(coarse_step);
    }

    if !best_post.is_finite() {
        return None;
    }

    let refine_min = best_end.saturating_sub(coarse_step).max(end_min);
    let refine_max = (best_end + coarse_step).min(end_max);
    for end in refine_min..=refine_max {
        consider(end, &mut best_end, &mut best_post, &mut best_score);
    }

    Some((best_end, best_post))
}

/// Start/end frame bracket for a B-side fill candidate (actual + nominal).
#[derive(Debug, Clone, Copy)]
pub struct FillBracketPlacement {
    pub start: usize,
    pub end: usize,
    pub nominal_start: usize,
    pub nominal_end: usize,
}

pub(crate) fn combined_structure_score(
    pre_score: f64,
    post_score: f64,
    placement: FillBracketPlacement,
    params: &StructureMatchParams,
    nominal_bias_scale: f64,
) -> f64 {
    let FillBracketPlacement {
        start,
        end,
        nominal_start,
        nominal_end,
    } = placement;
    let fill_len = end.saturating_sub(start);
    let len_penalty = LENGTH_MISMATCH_PENALTY
        * fill_len.abs_diff(params.gap_frames) as f64 / params.gap_frames.max(1) as f64;
    let nominal_bias = NOMINAL_BIAS_PER_BIN
        * nominal_bias_scale
        * (start.abs_diff(nominal_start) + end.abs_diff(nominal_end)) as f64
            / params.bin_frames.max(1) as f64;
    pre_score + post_score - len_penalty - nominal_bias
}

pub(crate) fn prefer_start(candidate: usize, current: usize, nominal: usize) -> bool {
    let dc = candidate.abs_diff(nominal);
    let db = current.abs_diff(nominal);
    if dc != db {
        dc < db
    } else {
        candidate < current
    }
}

pub(crate) fn prefer_end(candidate: usize, current: usize, nominal: usize) -> bool {
    prefer_start(candidate, current, nominal)
}

#[doc(hidden)]
pub fn score_pre_match(
    signature: &GapContextSignature,
    timeline: &ActivityTimeline,
    fill_start: usize,
    params: &StructureMatchParams,
) -> f64 {
    let pre_span = signature.pre_bins.len() * params.bin_frames;
    if fill_start < pre_span {
        return f64::NEG_INFINITY;
    }
    let Some(b_bins) = timeline.bins_for_frames(fill_start - pre_span, fill_start) else {
        return f64::NEG_INFINITY;
    };
    bin_similarity(&signature.pre_bins, b_bins)
}

#[doc(hidden)]
pub fn score_post_match(
    signature: &GapContextSignature,
    timeline: &ActivityTimeline,
    fill_end: usize,
    params: &StructureMatchParams,
) -> f64 {
    let post_span = signature.post_bins.len() * params.bin_frames;
    let Some(b_bins) = timeline.bins_for_frames(fill_end, fill_end + post_span) else {
        return f64::NEG_INFINITY;
    };
    bin_similarity(&signature.post_bins, b_bins)
}

pub(crate) fn activity_bins(
    samples: &[f32],
    channels: usize,
    start_frame: usize,
    end_frame: usize,
    bin_frames: usize,
    silence_peak_fraction: f32,
    absolute_silence_rms: f32,
) -> Vec<bool> {
    let channels = channels.max(1);
    let bin_frames = bin_frames.max(1);
    if start_frame >= end_frame {
        return Vec::new();
    }

    let mut bins = Vec::new();
    let mut frame = start_frame;
    while frame < end_frame {
        let bin_end = (frame + bin_frames).min(end_frame);
        let silent_frames = (frame..bin_end)
            .filter(|&f| {
                is_silent_frame(
                    samples,
                    channels,
                    f,
                    silence_peak_fraction,
                    absolute_silence_rms,
                )
            })
            .count();
        let bin_len = bin_end - frame;
        bins.push(silent_frames * 2 < bin_len);
        frame = bin_end;
    }
    bins
}

fn bin_similarity(expected: &[bool], observed: &[bool]) -> f64 {
    let len = expected.len().min(observed.len());
    if len == 0 {
        return f64::NEG_INFINITY;
    }
    let matches = expected
        .iter()
        .zip(observed.iter())
        .take(len)
        .filter(|(a, b)| a == b)
        .count();
    matches as f64 / len as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_frame(samples: &mut Vec<f32>, channels: usize, frame: usize, active: bool) {
        let amp = if active { 8_000.0_f32 / 32767.0 } else { 0.0 };
        while samples.len() < (frame + 1) * channels {
            samples.push(0.0);
        }
        for ch in 0..channels {
            samples[frame * channels + ch] = amp;
        }
    }

    #[test]
    fn structure_fine_polish_frames_capped_independent_of_waveform_slide() {
        assert_eq!(structure_fine_polish_frames(3), 3);
        assert_eq!(structure_fine_polish_frames(2_400), 128);
    }

    #[test]
    fn structure_match_finds_dropout_with_shared_pause_pattern() {
        let channels = 1usize;
        let bin_frames = 10usize;
        let gap_frames = 30usize;
        let gap_start = 60usize;
        let gap_end = gap_start + gap_frames;

        let mut a = vec![0.0f32; 200];
        for f in 0..55 {
            write_frame(&mut a, channels, f, true);
        }
        for f in 55..gap_start {
            write_frame(&mut a, channels, f, false);
        }
        for f in gap_start..gap_end {
            write_frame(&mut a, channels, f, false);
        }
        for f in gap_end..150 {
            write_frame(&mut a, channels, f, true);
        }

        let mut b = vec![0.0f32; 200];
        for f in 0..55 {
            write_frame(&mut b, channels, f, true);
        }
        for f in 55..gap_start {
            write_frame(&mut b, channels, f, false);
        }
        for f in gap_start..gap_end {
            write_frame(&mut b, channels, f, true);
        }
        for f in gap_end..150 {
            write_frame(&mut b, channels, f, true);
        }

        let params = StructureMatchParams {
            gap_frames,
            bin_frames,
            search_radius_frames: 20,
            fill_length_slack_frames: 5,
            max_fine_adjustment_frames: 3,
            silence_peak_fraction: 0.01,
            absolute_silence_rms: 0.0,
        };

        let sig = build_gap_context_signature(&a, channels, gap_start, gap_end, 50, &params);

        let alignment = match_gap_structure_in_b(
            &sig,
            &b,
            channels,
            gap_start,
            gap_end,
            &params,
        )
        .expect("structure match");
        assert!(
            alignment.start_frame.abs_diff(gap_start) <= 3,
            "expected fill near {gap_start}, got {}",
            alignment.start_frame
        );
        assert!(alignment.pre_correlation > 0.7);
        assert!(alignment.post_correlation > 0.7);
    }

    #[test]
    fn structure_match_prefers_nominal_when_pattern_uniform() {
        let channels = 1usize;
        let bin_frames = 20usize;
        let gap_frames = 40usize;
        let mut a = vec![0.0f32; 180];
        for f in 0..180 {
            write_frame(&mut a, channels, f, !(60..100).contains(&f));
        }

        let mut b = vec![0.0f32; 180];
        for f in 0..180 {
            write_frame(&mut b, channels, f, true);
        }

        let params = StructureMatchParams {
            gap_frames,
            bin_frames,
            search_radius_frames: 30,
            fill_length_slack_frames: 5,
            max_fine_adjustment_frames: 3,
            silence_peak_fraction: 0.01,
            absolute_silence_rms: 0.0,
        };

        let sig = build_gap_context_signature(&a, channels, 60, 100, 50, &params);

        let alignment =
            match_gap_structure_in_b(&sig, &b, channels, 62, 102, &params).expect("match");
        assert!(
            alignment.start_frame.abs_diff(62) <= 5,
            "expected stay near nominal, got {}",
            alignment.start_frame
        );
    }
}
