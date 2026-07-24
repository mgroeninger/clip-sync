//! Seam crossfade splice and shared trim / crossfade-length helpers.
//!
//! Leaf module of `domain::policies` — no intra-crate dependencies. Owns the crossfade actually
//! applied at a gap seam and its effective length; the low-energy trim helpers are `pub(crate)`
//! for `gap_borders`, which must build templates from the same full-level audio the splice sees.

/// Effective crossfade length at gap seams (shared by splice and splice-aware scoring).
pub fn effective_seam_crossfade_frames(
    crossfade_frames: usize,
    gap_start_frame: usize,
    gap_end_frame: usize,
    total_a_frames: usize,
) -> usize {
    let gap_frames = gap_end_frame.saturating_sub(gap_start_frame);
    let pre_available = gap_start_frame;
    let post_available = total_a_frames.saturating_sub(gap_end_frame);
    crossfade_frames
        .min(gap_frames / 2)
        .min(pre_available)
        .min(post_available)
}

/// Drop quiet tail samples (e.g. fade into a dropout) so seam templates use full-level audio.
pub(crate) fn trim_low_energy_suffix(samples: &[f64]) -> Vec<f64> {
    if samples.is_empty() {
        return Vec::new();
    }
    let peak = samples.iter().map(|s| s.abs()).fold(0.0f64, f64::max);
    if peak <= f64::EPSILON {
        return Vec::new();
    }
    let threshold = peak * 0.12;
    let mut end = samples.len();
    while end > 0 && samples[end - 1].abs() < threshold {
        end -= 1;
    }
    samples[..end].to_vec()
}

/// Drop quiet head samples (e.g. fade out of a dropout) so seam templates use full-level audio.
pub(crate) fn trim_low_energy_prefix(samples: &[f64]) -> Vec<f64> {
    if samples.is_empty() {
        return Vec::new();
    }
    let peak = samples.iter().map(|s| s.abs()).fold(0.0f64, f64::max);
    if peak <= f64::EPSILON {
        return Vec::new();
    }
    let threshold = peak * 0.12;
    let mut start = 0usize;
    while start < samples.len() && samples[start].abs() < threshold {
        start += 1;
    }
    samples[start..].to_vec()
}

pub(crate) fn blend_samples(a: f32, b: f32, a_weight: f32, b_weight: f32) -> f32 {
    (a_weight * a + b_weight * b).clamp(-1.0, 1.0)
}

/// Splice `b_fill` into `a_samples` at the gap, crossfading against A's real border audio.
///
/// Pre-seam: equal-power crossfade bleeds the fill head into the last `cf` pre-gap frames only;
/// the gap interior starts at full `b_fill[cf]` so there is no silence ramp inside the dropout.
/// Post-seam: blends fill tail with post-gap head across the boundary (same value on both sides).
///
/// `gap_start_frame` / `gap_end_frame` are frame indices (not interleaved sample indices).
pub fn apply_seam_crossfade(
    a_samples: &mut [f32],
    b_fill: &[f32],
    channels: usize,
    gap_start_frame: usize,
    gap_end_frame: usize,
    crossfade_frames: usize,
) {
    let channels = channels.max(1);
    let total_frames = a_samples.len() / channels;
    let gap_frames = gap_end_frame.saturating_sub(gap_start_frame);
    if gap_frames == 0 || b_fill.len() < gap_frames * channels {
        return;
    }

    let cf = effective_seam_crossfade_frames(
        crossfade_frames,
        gap_start_frame,
        gap_end_frame,
        total_frames,
    );

    if cf == 0 {
        for frame in gap_start_frame..gap_end_frame {
            for ch in 0..channels {
                let idx = frame * channels + ch;
                let b_idx = (frame - gap_start_frame) * channels + ch;
                a_samples[idx] = b_fill[b_idx];
            }
        }
        return;
    }

    // Pre-seam: crossfade fill head into pre-gap tail only (gap starts at full b_fill[cf]).
    for i in 0..cf {
        let t = i as f32 / cf as f32;
        let a_w = (t * std::f32::consts::FRAC_PI_2).cos();
        let b_w = (t * std::f32::consts::FRAC_PI_2).sin();
        let pre_frame = gap_start_frame - cf + i;
        for ch in 0..channels {
            let pre_idx = pre_frame * channels + ch;
            let a_val = a_samples[pre_idx];
            let b_val = b_fill[i * channels + ch];
            a_samples[pre_idx] = blend_samples(a_val, b_val, a_w, b_w);
        }
    }

    // Gap interior: pure fill (first `cf` B frames were consumed in the pre-seam bleed).
    for frame in gap_start_frame..(gap_end_frame - cf) {
        for ch in 0..channels {
            let a_idx = frame * channels + ch;
            let b_idx = (frame - gap_start_frame + cf) * channels + ch;
            a_samples[a_idx] = b_fill[b_idx];
        }
    }

    // Fade-out: blend fill tail with A's post-gap head across the seam.
    for i in 0..cf {
        let t = i as f32 / cf as f32;
        let b_w = (t * std::f32::consts::FRAC_PI_2).cos();
        let a_w = (t * std::f32::consts::FRAC_PI_2).sin();
        let b_frame = gap_frames - cf + i;
        for ch in 0..channels {
            let b_val = b_fill[b_frame * channels + ch];
            let post_idx = (gap_end_frame + i) * channels + ch;
            let a_val = a_samples[post_idx];
            let blended = blend_samples(a_val, b_val, a_w, b_w);
            let gap_idx = (gap_end_frame - cf + i) * channels + ch;
            a_samples[gap_idx] = blended;
            a_samples[post_idx] = blended;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::policies::{fill_splice_seam_correlations, SpliceSeamContext};

    #[test]
    fn fill_splice_seam_correlations_uses_crossfade_offset() {
        let pre_window = 2usize;
        let post_window = 2usize;
        let cf = 2usize;
        let gap_start = 4usize;
        let gap_end = 6usize;
        // A: ramp into gap; B fill matches the pre/post windows at splice time.
        let a_samples: Vec<f32> = [100.0, 200.0, 300.0, 400.0, 0.0, 0.0, 500.0, 600.0, 0.0, 0.0]
            .iter()
            .map(|&v| v / 32767.0)
            .collect();
        let fill = vec![300.0, 400.0, 0.0, 0.0, 500.0, 600.0];
        let a_pre = vec![1.0, 0.5];
        let a_post = vec![0.8, -0.6];
        let ctx = SpliceSeamContext {
            seam_cf: cf,
            gap_start_frame: gap_start,
            gap_end_frame: gap_end,
            a_samples: &a_samples,
            channels: 1,
            single_lag_alignment: true,
        };

        let (pre_cf, post_cf) = fill_splice_seam_correlations(
            &fill,
            &a_pre,
            &a_post,
            pre_window,
            post_window,
            ctx,
        );
        let (pre_no_cf, post_no_cf) = fill_splice_seam_correlations(
            &fill,
            &a_pre,
            &a_post,
            pre_window,
            post_window,
            SpliceSeamContext {
                seam_cf: 0,
                gap_start_frame: gap_start,
                gap_end_frame: gap_end,
                a_samples: &a_samples,
                channels: 1,
                single_lag_alignment: true,
            },
        );

        assert!(pre_cf > pre_no_cf + 0.5, "pre should score bleed tail on A timeline");
        assert!(post_cf > post_no_cf + 0.5, "post should score fade head on A timeline");
        assert!(pre_cf > 0.9 && post_cf > 0.9);
    }

    /// A **dual-fit** fill's shoulders are independently matched at their own seam-local lag — the
    /// fill is NOT expected to sit at lag 0 against A's raw neighboring samples the way an ordinary
    /// rigid-lag splice fill is. Regression for the 2026-07-03 (`7a26a17`) / 2026-07-05 production
    /// bug: with `single_lag_alignment: true`, the crossfade-window branch compares the fill's own
    /// head/tail against raw A at the literal gap boundary and collapses to a strongly NEGATIVE
    /// correlation even though the fill matches the border template (what `try_dual_fit`'s own
    /// seam-local search validated) almost perfectly. `single_lag_alignment: false` must bypass that
    /// branch and score the border template instead.
    #[test]
    fn splice_seam_correlation_ignores_crossfade_lag0_window_when_not_single_lag_aligned() {
        let pre_window = 4usize;
        let post_window = 4usize;
        let cf = 2usize;
        let gap_start = 10usize;
        let gap_end = 14usize;
        let total = 20usize;

        // Raw A around the gap boundary: a short spike-then-drop right at the edge, unrelated to
        // the fill's own trend — this is what a genuine per-shoulder lag looks like: the fill's
        // content was matched further along B, not against A's literal immediate neighbor.
        let mut a_samples = vec![0.0f32; total];
        a_samples[gap_start - 2] = 100.0 / 32767.0;
        a_samples[gap_start - 1] = 0.0;
        a_samples[gap_end] = 0.0;
        a_samples[gap_end + 1] = 100.0 / 32767.0;

        // The fill's own head/tail ramp — matches the border templates (what seam-local search
        // validated) almost perfectly, but is monotonically opposite the raw A spike-then-drop above.
        let fill = vec![10.0, 20.0, 30.0, 40.0, 0.0, 0.0, 0.0, 0.0, 40.0, 30.0, 20.0, 10.0];
        let a_pre = vec![1.0, 2.0, 3.0, 4.0];
        let a_post = vec![4.0, 3.0, 2.0, 1.0];

        let dual_fit_ctx = SpliceSeamContext {
            seam_cf: cf,
            gap_start_frame: gap_start,
            gap_end_frame: gap_end,
            a_samples: &a_samples,
            channels: 1,
            single_lag_alignment: false,
        };
        let (pre_border, post_border) = fill_splice_seam_correlations(
            &fill,
            &a_pre,
            &a_post,
            pre_window,
            post_window,
            dual_fit_ctx,
        );
        assert!(
            pre_border > 0.9 && post_border > 0.9,
            "dual-fit fill must be scored against the border template it was actually matched to: pre={pre_border} post={post_border}"
        );

        let single_lag_ctx = SpliceSeamContext {
            seam_cf: cf,
            gap_start_frame: gap_start,
            gap_end_frame: gap_end,
            a_samples: &a_samples,
            channels: 1,
            single_lag_alignment: true,
        };
        let (pre_lag0, post_lag0) = fill_splice_seam_correlations(
            &fill,
            &a_pre,
            &a_post,
            pre_window,
            post_window,
            single_lag_ctx,
        );
        assert!(
            pre_lag0 < 0.0 && post_lag0 < 0.0,
            "sanity check: the lag-0 crossfade window must actually diverge from the border score \
             for this fixture (pre={pre_lag0} post={post_lag0}) — otherwise this test isn't exercising the bug"
        );
    }

    #[test]
    fn apply_seam_crossfade_bleeds_fill_head_into_pre_gap_tail() {
        // Layout: [pre-border loud][gap silent][post-border loud]
        let cf = 4usize;
        let gap_start = 10usize;
        let gap_end = 20usize;
        let gap_frames = gap_end - gap_start;
        let loud: f32 = 8_000.0 / 32767.0;
        let fill_level: f32 = 4_000.0 / 32767.0;

        let mut a = vec![0.0f32; 30];
        for s in &mut a[0..gap_start] {
            *s = loud;
        }
        for s in &mut a[gap_end..] {
            *s = loud;
        }

        let b_fill = vec![fill_level; gap_frames];
        apply_seam_crossfade(&mut a, &b_fill, 1, gap_start, gap_end, cf);

        assert!(
            (a[gap_start - cf - 1] - loud).abs() < 1e-5,
            "pre-gap audio before the crossfade window should be untouched"
        );
        assert!(
            (a[gap_start - cf] - loud).abs() < 1e-5,
            "crossfade should start from pure pre-gap audio"
        );
        assert!(
            (a[gap_start] - fill_level).abs() < 1e-5,
            "gap should start at full fill level, not a silence ramp"
        );
        assert!((a[gap_start + 1] - fill_level).abs() < 1e-5, "gap interior should be pure fill");
        assert!(
            a[gap_start - 1] > 3_000.0 / 32767.0,
            "pre-gap tail should bleed into fill before the gap boundary"
        );
    }

    #[test]
    fn apply_seam_crossfade_is_continuous_at_pre_seam() {
        let cf = 4usize;
        let gap_start = 10usize;
        let gap_end = 20usize;
        let gap_frames = gap_end - gap_start;
        let loud: f32 = 8_000.0 / 32767.0;
        let fill_level: f32 = 4_000.0 / 32767.0;

        let mut a = vec![0.0f32; 30];
        for s in &mut a[0..gap_start] {
            *s = loud;
        }
        for s in &mut a[gap_end..] {
            *s = loud;
        }

        let b_fill = vec![fill_level; gap_frames];
        apply_seam_crossfade(&mut a, &b_fill, 1, gap_start, gap_end, cf);

        let diff = (a[gap_start] - a[gap_start - 1]).abs();
        assert!(
            diff <= 4_500.0 / 32767.0,
            "jump of {diff} across pre seam ({} -> {})",
            a[gap_start - 1],
            a[gap_start]
        );
    }
}
