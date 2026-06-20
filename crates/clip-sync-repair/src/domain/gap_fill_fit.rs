//! Waveform seam search for fit-mode gap fill (Phase A).

use crate::domain::policies::{
    fill_seam_correlations, FillAlignment, SeamPlacement, SeamTemplates,
};

const SCORE_TIE_EPSILON: f64 = 1e-9;

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
            a_pre_ch: &[a_pre.clone()],
            a_post_ch: &[a_post.clone()],
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
    fn waveform_search_step_is_one_for_small_radius() {
        assert_eq!(waveform_search_step(100), 1);
        assert!(waveform_search_step(10_000) > 1);
    }
}
