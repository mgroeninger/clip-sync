//! Gap border refinement, templates, and seam-channel selection.
//!
//! Turns a raw gap into the A-side templates the seam gate scores against: refined `[start, end)`
//! frames, per-channel border templates, and the channel subset carrying border energy. Builds on
//! `silence::is_silent_frame`, `seam_splice`'s low-energy trim, and `seam_scoring`'s energy-based
//! channel selection.
use crate::domain::pcm::{interleaved_to_channels, interleaved_to_mono};

use super::seam_scoring::{seam_score_channel_indices, template_mean_square};
use super::seam_splice::{trim_low_energy_prefix, trim_low_energy_suffix};
use super::silence::is_silent_frame;

/// Result of bracketing a B fill between matched pre/post borders.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FillAlignment {
    /// Frame index into the extended B buffer where the fill should start.
    pub start_frame: usize,
    /// B-derived fill length in frames (`post` border starts at `start_frame + fill_frames`).
    pub fill_frames: usize,
    pub pre_correlation: f64,
    pub post_correlation: f64,
}

/// Refined gap boundaries on A's PCM timeline (frame indices, `[start, end)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefinedGapFrames {
    pub start_frame: usize,
    pub end_frame: usize,
}

fn silent_run(
    samples: &[f32],
    channels: usize,
    start_frame: usize,
    run_frames: usize,
    silence_peak_fraction: f32,
    absolute_rms_floor: f32,
) -> bool {
    (start_frame..start_frame + run_frames).all(|frame| {
        is_silent_frame(
            samples,
            channels,
            frame,
            silence_peak_fraction,
            absolute_rms_floor,
        )
    })
}

/// Tighten a reported gap against A's decoded PCM.
///
/// - Retracts `start` through leading silence before the reported boundary (scanner block
///   quantization can start the run slightly late).
/// - Advances `start` past leading non-silent frames (scanner started the run too early).
/// - Extends `end` through trailing silence (scanner closed the run too early).
pub fn refine_gap_frames(
    samples: &[f32],
    channels: usize,
    start_frame: usize,
    end_frame: usize,
    silence_peak_fraction: f32,
    absolute_rms_floor: f32,
    max_refine_frames: usize,
) -> RefinedGapFrames {
    let channels = channels.max(1);
    let total_frames = samples.len() / channels;
    let mut start = start_frame.min(total_frames);
    let mut end = end_frame.min(total_frames);
    if start >= end {
        return RefinedGapFrames {
            start_frame: start,
            end_frame: end,
        };
    }

    let mut budget = max_refine_frames;
    while start > 0
        && budget > 0
        && is_silent_frame(
            samples,
            channels,
            start - 1,
            silence_peak_fraction,
            absolute_rms_floor,
        )
    {
        start -= 1;
        budget -= 1;
    }

    // Peel at most `max_refine_frames` of leading non-silent audio before the reported gap.
    // Cap at `start_frame + max_refine` so a noisy dropout interior cannot push `start` all the
    // way to `end_frame` (block-quantized gaps are typically misaligned by <250 ms).
    let max_start = (start_frame + max_refine_frames).min(end_frame);
    let confirm_frames = (max_refine_frames / 15).clamp(4, 4096);
    while start + confirm_frames <= max_start && budget > 0 {
        if silent_run(
            samples,
            channels,
            start,
            confirm_frames,
            silence_peak_fraction,
            absolute_rms_floor,
        ) {
            break;
        }
        start += 1;
        budget -= 1;
    }

    budget = max_refine_frames;
    while end < total_frames
        && budget > 0
        && is_silent_frame(
            samples,
            channels,
            end,
            silence_peak_fraction,
            absolute_rms_floor,
        )
    {
        end += 1;
        budget -= 1;
    }

    RefinedGapFrames {
        start_frame: start,
        end_frame: end.max(start),
    }
}

struct GapBorderFrameRange {
    pre_start: usize,
    pre_end: usize,
    post_start: usize,
    post_end: usize,
}

/// Gap bounds, template sizing, and silence thresholds shared by the border-template builders.
#[derive(Clone, Copy)]
pub struct GapBorderSpec {
    pub gap_start_frame: usize,
    pub gap_end_frame: usize,
    pub border_frames: usize,
    pub border_standoff_frames: usize,
    pub silence_peak_fraction: f32,
    pub absolute_rms_floor: f32,
}

fn gap_border_frame_range(
    samples: &[f32],
    channels: usize,
    spec: &GapBorderSpec,
) -> GapBorderFrameRange {
    let GapBorderSpec {
        gap_start_frame,
        gap_end_frame,
        border_frames,
        border_standoff_frames,
        silence_peak_fraction,
        absolute_rms_floor,
    } = *spec;
    let channels = channels.max(1);
    let total_frames = samples.len() / channels;

    let pre_start = gap_start_frame.saturating_sub(border_frames);
    let mut pre_end = gap_start_frame.min(total_frames);
    while pre_end > pre_start
        && is_silent_frame(
            samples,
            channels,
            pre_end - 1,
            silence_peak_fraction,
            absolute_rms_floor,
        )
    {
        pre_end -= 1;
    }

    let post_end = (gap_end_frame + border_frames).min(total_frames);
    let mut post_start = gap_end_frame.min(total_frames);
    while post_start < post_end
        && is_silent_frame(
            samples,
            channels,
            post_start,
            silence_peak_fraction,
            absolute_rms_floor,
        )
    {
        post_start += 1;
    }

    // Skip audio immediately adjacent to the dropout (A-side templates only).
    if border_standoff_frames > 0 {
        pre_end = pre_end
            .saturating_sub(border_standoff_frames)
            .max(pre_start);
        post_start = (post_start + border_standoff_frames).min(post_end);
    }

    GapBorderFrameRange {
        pre_start,
        pre_end,
        post_start,
        post_end,
    }
}

/// Build mono border templates for seam correlation, skipping silence adjacent to the gap.
pub fn border_templates_for_gap(
    samples: &[f32],
    channels: usize,
    spec: &GapBorderSpec,
) -> (Vec<f64>, Vec<f64>) {
    let channels = channels.max(1);
    let range = gap_border_frame_range(samples, channels, spec);

    let mut pre_mono = if range.pre_end > range.pre_start {
        interleaved_to_mono(
            &samples[range.pre_start * channels..range.pre_end * channels],
            channels,
        )
    } else {
        Vec::new()
    };
    let mut post_mono = if range.post_end > range.post_start {
        interleaved_to_mono(
            &samples[range.post_start * channels..range.post_end * channels],
            channels,
        )
    } else {
        Vec::new()
    };

    pre_mono = trim_low_energy_suffix(&pre_mono);
    post_mono = trim_low_energy_prefix(&post_mono);

    (pre_mono, post_mono)
}

/// Count of samples above 12% of border peak (matches trim_low_energy threshold).
pub(crate) fn border_active_extent_frames(border: &[f64]) -> usize {
    if border.is_empty() {
        return 0;
    }
    let peak = border.iter().map(|s| s.abs()).fold(0.0f64, f64::max);
    if peak <= f64::EPSILON {
        return 0;
    }
    let floor = peak * 0.12;
    border.iter().filter(|s| s.abs() >= floor).count()
}

/// Adaptive seam window: cap at active extent, bounded by min/max seam frames.
pub(crate) fn adaptive_seam_window_frames(
    border_len: usize,
    min_frames: usize,
    max_frames: usize,
    active_extent_frames: usize,
) -> usize {
    if border_len == 0 {
        return 0;
    }
    let min_frames = min_frames.max(1);
    let max_frames = max_frames.max(min_frames);
    let target = active_extent_frames.max(min_frames).min(max_frames);
    target.min(border_len).max(1)
}

/// Per-channel border templates (same frame ranges as [`border_templates_for_gap`]).
pub fn border_templates_per_channel_for_gap(
    samples: &[f32],
    channels: usize,
    spec: &GapBorderSpec,
) -> (Vec<Vec<f64>>, Vec<Vec<f64>>) {
    let channels = channels.max(1);
    let range = gap_border_frame_range(samples, channels, spec);

    let mut pre_ch = if range.pre_end > range.pre_start {
        interleaved_to_channels(
            &samples[range.pre_start * channels..range.pre_end * channels],
            channels,
        )
    } else {
        vec![Vec::new(); channels]
    };
    let mut post_ch = if range.post_end > range.post_start {
        interleaved_to_channels(
            &samples[range.post_start * channels..range.post_end * channels],
            channels,
        )
    } else {
        vec![Vec::new(); channels]
    };

    for ch in &mut pre_ch {
        *ch = trim_low_energy_suffix(ch);
    }
    for ch in &mut post_ch {
        *ch = trim_low_energy_prefix(ch);
    }

    (pre_ch, post_ch)
}

/// The **loudest** energy-passing seam channel — argmax of per-channel border energy (max of pre/post
/// mean-square), or `None` when every channel is near-silent. Use this for a single-channel
/// *representative* (e.g. the lag/probe diagnostic) instead of `selected_seam_channels().first()`, which
/// returns the **lowest-index** passing channel — index order, an arbitrary pick that lands on L over a
/// louder C in a center-dominant mix. The gate's multichannel decision still uses the full
/// `selected_seam_channels` set; this only changes which single channel a diagnostic follows.
pub fn loudest_seam_channel(
    a_samples: &[f32],
    channels: usize,
    spec: &GapBorderSpec,
) -> Option<usize> {
    let (a_pre_ch, a_post_ch) = border_templates_per_channel_for_gap(a_samples, channels, spec);
    let n = a_pre_ch.len().min(a_post_ch.len());
    (0..n)
        .map(|ch| {
            (
                ch,
                template_mean_square(&a_pre_ch[ch]).max(template_mean_square(&a_post_ch[ch])),
            )
        })
        .filter(|&(_, e)| e > f64::EPSILON)
        .max_by(|(_, x), (_, y)| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(ch, _)| ch)
}

/// Energy-selected seam channels for a gap, computed from the same per-channel border templates
/// Pearson scores — the single entry point so residual/floor measurement follows the *same*
/// channels as seam scoring (see `seam-scoring.md`). Returns empty when every channel is
/// near-silent, so the caller falls back to the mono downmix.
pub fn selected_seam_channels(
    a_samples: &[f32],
    channels: usize,
    spec: &GapBorderSpec,
) -> Vec<usize> {
    let (a_pre_ch, a_post_ch) = border_templates_per_channel_for_gap(a_samples, channels, spec);
    seam_score_channel_indices(&a_pre_ch, &a_post_ch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::policies::{seam_channel_diagnostics, SeamPlacement, SeamTemplates};

    #[test]
    fn refine_gap_frames_retracts_through_leading_silence_before_reported_start() {
        let channels = 2usize;
        let loud = 8_000.0_f32 / 32767.0;
        // [loud 5][silent 10][loud 5] — reported gap starts two frames late.
        let mut samples = Vec::new();
        for _ in 0..5 {
            samples.extend([loud, loud]);
        }
        for _ in 0..10 {
            samples.extend([0.0f32, 0.0f32]);
        }
        for _ in 0..5 {
            samples.extend([loud, loud]);
        }

        let refined = refine_gap_frames(&samples, channels, 7, 14, 0.01, 0.0, 10);
        assert_eq!(refined.start_frame, 5);
        assert_eq!(refined.end_frame, 15);
    }

    #[test]
    fn refine_gap_frames_advances_past_leading_audio_and_extends_trailing_silence() {
        let channels = 2usize;
        let loud = 8_000.0_f32 / 32767.0;
        // [loud 5][silent 10][loud 5] — reported gap starts one frame too early and ends one frame too early.
        let mut samples = Vec::new();
        for _ in 0..5 {
            samples.extend([loud, loud]);
        }
        for _ in 0..10 {
            samples.extend([0.0f32, 0.0f32]);
        }
        for _ in 0..5 {
            samples.extend([loud, loud]);
        }

        let refined = refine_gap_frames(&samples, channels, 4, 14, 0.01, 0.0, 10);
        assert_eq!(refined.start_frame, 5);
        assert_eq!(refined.end_frame, 15);
    }

    #[test]
    fn refine_gap_frames_caps_start_advance_before_reported_end() {
        let channels = 1usize;
        let mut samples = Vec::new();
        // Leading audio (2 frames), low-level dropout (8 frames), trailing audio.
        let loud = 8_000.0_f32 / 32767.0;
        samples.extend([loud; 2]);
        samples.extend([3.0_f32 / 32767.0; 8]);
        samples.extend([loud; 2]);

        let refined = refine_gap_frames(&samples, channels, 2, 10, 0.01, 0.0, 20);
        assert!(
            refined.start_frame < 10,
            "start should not advance all the way to reported end"
        );
        assert_eq!(refined.end_frame, 10);
    }

    fn test_border_spec(
        gap_start_frame: usize,
        gap_end_frame: usize,
        border_frames: usize,
        border_standoff_frames: usize,
    ) -> GapBorderSpec {
        GapBorderSpec {
            gap_start_frame,
            gap_end_frame,
            border_frames,
            border_standoff_frames,
            silence_peak_fraction: 0.01,
            absolute_rms_floor: 0.0,
        }
    }

    #[test]
    fn border_templates_trim_quiet_fade_before_dropout() {
        let channels = 1usize;
        let loud = 8_000.0_f32 / 32767.0;
        let low = 200.0_f32 / 32767.0;
        let mut samples = Vec::new();
        samples.extend(vec![loud; 8]);
        samples.extend(vec![low; 4]);
        samples.extend(vec![0.0f32; 4]);
        samples.extend(vec![low; 4]);
        samples.extend(vec![loud; 8]);

        let (pre, post) =
            border_templates_for_gap(&samples, channels, &test_border_spec(16, 20, 12, 0));
        assert!(!pre.is_empty());
        assert!(pre.iter().all(|&v| v.abs() > 1_000.0 / 32767.0));
        assert!(!post.is_empty());
        assert!(post.iter().all(|&v| v.abs() > 1_000.0 / 32767.0));
    }

    #[test]
    fn border_standoff_excludes_audio_adjacent_to_dropout() {
        let channels = 1usize;
        let loud = 8_000.0_f32 / 32767.0;
        let mut samples = vec![loud; 20];
        samples.extend(vec![0.0f32; 5]);
        samples.extend(vec![loud; 20]);

        let (pre_no, _) =
            border_templates_for_gap(&samples, channels, &test_border_spec(20, 25, 15, 0));
        let (pre_standoff, _) =
            border_templates_for_gap(&samples, channels, &test_border_spec(20, 25, 15, 5));
        assert_eq!(pre_no.len(), 15);
        assert_eq!(pre_standoff.len(), 10);
    }

    #[test]
    fn border_templates_for_gap_skip_adjacent_silence() {
        let channels = 1usize;
        let loud = 8_000.0_f32 / 32767.0;
        let mut samples = vec![loud; 5];
        samples.extend(vec![0.0f32; 5]);
        samples.extend(vec![loud; 5]);

        let (pre, post) =
            border_templates_for_gap(&samples, channels, &test_border_spec(5, 10, 5, 0));
        assert_eq!(pre.len(), 5);
        assert!(pre.iter().all(|&v| (v - loud as f64).abs() < 0.001));
        assert_eq!(post.len(), 5);
        assert!(post.iter().all(|&v| (v - loud as f64).abs() < 0.001));
    }

    // ---- Per-channel (multichannel) residual ----------------------------------------------------

    /// Build interleaved f32 A samples (normalized) from per-channel f64 timelines (raw level).
    fn interleave_a(channels_f64: &[Vec<f64>], norm: f64) -> Vec<f32> {
        let channels = channels_f64.len();
        let total = channels_f64[0].len();
        let mut out = vec![0.0f32; total * channels];
        for frame in 0..total {
            for (ch, timeline) in channels_f64.iter().enumerate() {
                out[frame * channels + ch] = (timeline[frame] / norm) as f32;
            }
        }
        out
    }

    /// Pearson (`seam_channel_diagnostics`) and residual (`selected_seam_channels`) must agree on
    /// which A-side channels carry border energy for a gap.
    fn assert_channel_selection_parity(
        a_samples: &[f32],
        channels: usize,
        spec: &GapBorderSpec,
        b_ch: &[Vec<f64>],
        placement: SeamPlacement,
    ) {
        let (a_pre, a_post) = border_templates_for_gap(a_samples, channels, spec);
        let (a_pre_ch, a_post_ch) = border_templates_per_channel_for_gap(a_samples, channels, spec);
        let b_mono: Vec<f64> = b_ch
            .first()
            .map(|_| {
                (0..b_ch[0].len())
                    .map(|i| b_ch.iter().map(|ch| ch[i]).sum::<f64>() / b_ch.len() as f64)
                    .collect()
            })
            .unwrap_or_default();
        let templates = SeamTemplates {
            a_pre: &a_pre,
            a_post: &a_post,
            a_pre_ch: &a_pre_ch,
            a_post_ch: &a_post_ch,
            b_mono: &b_mono,
            b_ch,
        };
        let pearson = seam_channel_diagnostics(&templates, placement).selected;
        let residual = selected_seam_channels(a_samples, channels, spec);
        assert_eq!(
            pearson, residual,
            "Pearson diagnostics and residual must select the same channels"
        );
    }

    #[test]
    fn selected_seam_channels_matches_pearson_diagnostics() {
        let gap_start = 800usize;
        let gap_end = 1000usize;
        let border_frames = 128usize;
        let standoff = 16usize;
        let window = 128usize;
        let start = gap_start;
        let gap_frames = gap_end - gap_start;
        let placement = || SeamPlacement {
            start,
            gap_frames,
            pre_window: window,
            post_window: window,
        };

        // Stereo, equal energy on both channels → both selected.
        {
            let total = 2000usize;
            let channels = 2usize;
            let l: Vec<f64> = (0..total)
                .map(|i| (i as f64 * 0.17).sin() * 4000.0)
                .collect();
            let r: Vec<f64> = (0..total)
                .map(|i| (i as f64 * 0.4).cos() * 4000.0)
                .collect();
            let a_samples = interleave_a(&[l.clone(), r.clone()], 4000.0);
            let b_ch = vec![l, r];
            let spec = test_border_spec(gap_start, gap_end, border_frames, standoff);
            assert_channel_selection_parity(&a_samples, channels, &spec, &b_ch, placement());
            assert_eq!(
                selected_seam_channels(&a_samples, channels, &spec),
                vec![0, 1]
            );
        }

        // Center-dominant 3ch: only FC crosses the ~20 dB energy gate in the border templates.
        {
            let total = 2000usize;
            let channels = 3usize;
            let fc_b: Vec<f64> = (0..total)
                .map(|i| (i as f64 * 0.17).sin() * 4000.0 + (i as f64 * 0.4).cos() * 1500.0)
                .collect();
            let fl_b: Vec<f64> = (0..total)
                .map(|i| (i as f64 * 0.53).sin() * 2000.0)
                .collect();
            let fr_b: Vec<f64> = (0..total)
                .map(|i| (i as f64 * 0.91).cos() * 2000.0)
                .collect();
            let b_ch = vec![fl_b.clone(), fr_b.clone(), fc_b.clone()];
            let fc_a: Vec<f64> = fc_b.iter().map(|s| s * 0.5).collect();
            // Fronts well below FC in the border region (>20 dB down) so only the center is selected.
            let fl_a: Vec<f64> = vec![5.0; total];
            let fr_a: Vec<f64> = vec![5.0; total];
            let a_samples = interleave_a(&[fl_a, fr_a, fc_a], 4000.0);
            let spec = test_border_spec(gap_start, gap_end, border_frames, standoff);
            assert_channel_selection_parity(&a_samples, channels, &spec, &b_ch, placement());
            assert_eq!(selected_seam_channels(&a_samples, channels, &spec), vec![2]);
        }

        // Near-silent borders → empty selection (mono fallback for both paths).
        {
            let total = 2000usize;
            let channels = 2usize;
            let a_samples = vec![0.0f32; total * channels];
            let b_ch = vec![vec![0.0f64; total], vec![0.0f64; total]];
            let spec = test_border_spec(gap_start, gap_end, border_frames, standoff);
            assert_channel_selection_parity(&a_samples, channels, &spec, &b_ch, placement());
            assert!(selected_seam_channels(&a_samples, channels, &spec).is_empty());
        }
    }

    #[test]
    fn loudest_seam_channel_picks_by_energy_not_index() {
        let (gap_start, gap_end, border_frames, standoff) =
            (800usize, 1000usize, 128usize, 16usize);
        let total = 2000usize;
        let channels = 3usize;
        let spec = test_border_spec(gap_start, gap_end, border_frames, standoff);

        // ch0 (L) moderate, ch1 (R) silent, ch2 (C) loudest. Both 0 and 2 pass the −20 dB energy gate,
        // so `selected_seam_channels` returns [0, 2] in index order and `.first()` = 0 (the *quieter*
        // channel). `loudest_seam_channel` must instead return 2 (the channel that carries the level).
        let l_a: Vec<f64> = (0..total)
            .map(|i| (i as f64 * 0.17).sin() * 2000.0)
            .collect();
        let r_a: Vec<f64> = vec![5.0; total];
        let c_a: Vec<f64> = (0..total)
            .map(|i| (i as f64 * 0.31).sin() * 4000.0)
            .collect();
        let a_samples = interleave_a(&[l_a, r_a, c_a], 4000.0);

        assert_eq!(
            selected_seam_channels(&a_samples, channels, &spec),
            vec![0, 2]
        );
        assert_eq!(
            selected_seam_channels(&a_samples, channels, &spec)
                .first()
                .copied(),
            Some(0),
            ".first() picks the lowest-index passing channel (the quieter L)"
        );
        assert_eq!(
            loudest_seam_channel(&a_samples, channels, &spec),
            Some(2),
            "loudest_seam_channel follows the level to the center channel"
        );
    }
}
