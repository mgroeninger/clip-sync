//! `constellation` slice of the gap_fingerprint measure folder split — item 3 of
//! `docs/dev/TEMP-constellation-fingerprint-plan.md` (§9 checklist, §10 sketch): wires the domain
//! onset picker + `L`-histogram lock test (`crate::domain::constellation`) into the fingerprint.

use crate::application::gap_fingerprint::schema::*;
use crate::domain::constellation::{
    evaluate_l_histogram, pick_onsets, LagHistogramParams, LockReason, PickerParams,
};
use crate::domain::policies::RefinedGapFrames;

use super::config::*;
use super::placement::*;

/// `(gap_index, side_tag)` → a redraw seed, deterministic and reproducible across runs (§10
/// resolution). `side_tag`: 0 = pre, 1 = post. Reuses `Lcg`'s own multiplier to stay in-family with
/// the domain module's PRNG rather than inventing a second constant.
fn constellation_seed(gap_index: usize, side_tag: u64) -> u64 {
    (gap_index as u64)
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(side_tag * 2 + 1)
}

/// Slice one track's interleaved window, pick onsets, and translate them to wire shape.
/// `seam_secs` is that track's own boundary time (A: the refined gap edge; B: the `b_mapped`
/// nominal of that edge) — `t_seam_ms` is measured against it, not against the window start.
/// `picker` is threaded in rather than rebuilt here: it's a pure function of `sample_rate` alone,
/// so the caller (`measure_constellation`) builds it once and both sides share it.
#[allow(clippy::too_many_arguments)]
fn side_from_window(
    interleaved: &[f32],
    ch: usize,
    picker: &PickerParams,
    frame_lo: usize,
    frame_hi: usize,
    rate: f64,
    seam_secs: f64,
    edge_pinned: bool,
) -> (ConstellationSide, Vec<f64>) {
    let lo_b = (frame_lo * ch).min(interleaved.len());
    let hi_b = (frame_hi * ch).min(interleaved.len()).max(lo_b);
    let slice = &interleaved[lo_b..hi_b];
    let onsets = pick_onsets(slice, ch, picker);
    let base_secs = frame_lo as f64 / rate;
    let points: Vec<ConstellationPoint> = onsets
        .iter()
        .map(|e| {
            let time_secs = base_secs + e.time_secs;
            ConstellationPoint {
                t_seam_ms: (time_secs - seam_secs) * 1000.0,
                time_secs,
                prominence: e.prominence,
            }
        })
        .collect();
    let times: Vec<f64> = points.iter().map(|p| p.time_secs).collect();
    let side = ConstellationSide {
        span_start_secs: base_secs,
        span_end_secs: frame_hi as f64 / rate,
        edge_pinned,
        n: points.len(),
        points,
    };
    (side, times)
}

/// One shoulder: pick both sides, then run the `L`-histogram lock test — unless one side's window
/// doesn't exist to attempt (degenerate/zero-width), in which case the shoulder is `Unmeasured`
/// without calling `evaluate_l_histogram` at all (§10 resolution: "not attempted" vs "attempted but
/// empty" — the latter is `evaluate_l_histogram`'s own `NoTrain`, reached below when both windows
/// exist but yield fewer than two onsets).
#[allow(clippy::too_many_arguments)]
fn measure_shoulder(
    a_samples: &[f32],
    b_haystack: &[f32],
    ch: usize,
    picker: &PickerParams,
    rate: f64,
    a_win: (usize, usize),
    a_edge_pinned: bool,
    b_win: (usize, usize),
    b_edge_pinned: bool,
    seam_a_secs: f64,
    seam_b_secs: f64,
    hist_params: &LagHistogramParams,
    redraw_seed: u64,
) -> ConstellationShoulder {
    let (a_lo, a_hi) = a_win;
    let (b_lo, b_hi) = b_win;
    let a_exists = a_hi > a_lo;
    let b_exists = b_hi > b_lo;

    let (a_side, a_times) = side_from_window(
        a_samples,
        ch,
        picker,
        a_lo,
        a_hi,
        rate,
        seam_a_secs,
        a_edge_pinned,
    );
    let (b_side, b_times) = side_from_window(
        b_haystack,
        ch,
        picker,
        b_lo,
        b_hi,
        rate,
        seam_b_secs,
        b_edge_pinned,
    );

    if !a_exists || !b_exists {
        return ConstellationShoulder {
            a: a_side,
            b: b_side,
            n_a: a_times.len(),
            n_b: b_times.len(),
            hist_peak_lag_ms: None,
            hist_peak_votes: None,
            hist_second_votes: None,
            redraw_p95_votes: None,
            reason: ConstellationReason::Unmeasured,
        };
    }

    let b_span = (b_lo as f64 / rate, b_hi as f64 / rate);
    let result = evaluate_l_histogram(&a_times, &b_times, b_span, hist_params, redraw_seed);
    let reason = match result.reason {
        LockReason::Locked => ConstellationReason::Locked,
        LockReason::NoTrain => ConstellationReason::NoTrain,
        LockReason::NotUnique => ConstellationReason::NotUnique,
        LockReason::EdgePinned => ConstellationReason::EdgePinned,
        // The null had no legal arrangement to compare against — no verdict ran (§10 resolution).
        LockReason::RedrawInfeasible => ConstellationReason::Unmeasured,
    };

    ConstellationShoulder {
        a: a_side,
        b: b_side,
        n_a: result.n_a,
        n_b: result.n_b,
        hist_peak_lag_ms: result.hist_peak_lag_ms,
        hist_peak_votes: result.hist_peak_votes,
        hist_second_votes: result.hist_second_votes,
        redraw_p95_votes: result.redraw_p95_votes,
        reason,
    }
}

/// Fingerprint measure: four windows (A pre/post, B pre/post haystack), pick, vote, stamp §5.
/// Pairwise-only (needs `b_haystack`) and Full-tier-only — call site gates both (item 3, §9).
///
/// B search span per plan §2 rule 7: the A window's own `b_mapped` nominal span, widened by
/// `lag_envelope_max_lag_ms` on both ends and clamped to the haystack — **not**
/// `fill_border_search_secs`/`SEAM_LOCAL_SEARCH_MS` (the Pearson band already refuted for this
/// population).
///
/// `context_frames` is the caller's own `(cfg.gap_signature_context_secs * rate).round()` — threaded
/// in rather than recomputed here, since `build_gap_fingerprint` already derives it once for the
/// intrinsic A-side measurements and this fn's window would otherwise be a second, easy-to-desync
/// copy of the same formula.
#[allow(clippy::too_many_arguments)]
pub(in crate::application::gap_fingerprint::measure) fn measure_constellation(
    gap_index: usize,
    a_samples: &[f32],
    b_haystack: &[f32],
    ch: usize,
    sample_rate: u32,
    rate: f64,
    refined: RefinedGapFrames,
    total_a: usize,
    context_frames: usize,
    cfg: &FingerprintConfig,
    gap_offset_secs: f64,
    b_extract_start_secs: f64,
) -> ConstellationSet {
    let picker_params = PickerParams::first_candidate(sample_rate);
    let hist_params = LagHistogramParams::first_candidate(&picker_params);
    let max_lag_frames = ((f64::from(cfg.lag_envelope_max_lag_ms) / 1000.0) * rate).round() as i64;
    let b_len_frames = b_haystack.len() / ch.max(1);

    let b_mapped = |a_frame: usize| -> usize {
        b_mapped_frame_in_haystack(a_frame, rate, gap_offset_secs, b_extract_start_secs)
    };

    // --- pre shoulder: A = [gap_start - context, gap_start) ---
    let a_pre_lo_i = refined.start_frame as i64 - context_frames as i64;
    let a_pre_lo = a_pre_lo_i.max(0) as usize;
    let a_pre_hi = refined.start_frame;
    let a_pre_edge_pinned = a_pre_lo_i < 0;

    let b_pre_seam_frame = b_mapped(a_pre_hi);
    let b_pre_lo_i = b_mapped(a_pre_lo) as i64 - max_lag_frames;
    let b_pre_hi_i = b_pre_seam_frame as i64 + max_lag_frames;
    let b_pre_lo = b_pre_lo_i.max(0) as usize;
    let b_pre_hi = b_pre_hi_i.max(0).min(b_len_frames as i64) as usize;
    let b_pre_edge_pinned = b_pre_lo_i < 0 || b_pre_hi_i > b_len_frames as i64;

    let pre = measure_shoulder(
        a_samples,
        b_haystack,
        ch,
        &picker_params,
        rate,
        (a_pre_lo, a_pre_hi),
        a_pre_edge_pinned,
        (b_pre_lo, b_pre_hi),
        b_pre_edge_pinned,
        a_pre_hi as f64 / rate,
        b_pre_seam_frame as f64 / rate,
        &hist_params,
        constellation_seed(gap_index, 0),
    );

    // --- post shoulder: A = [gap_end, gap_end + context) ---
    let a_post_lo = refined.end_frame;
    let a_post_hi_i = refined.end_frame as i64 + context_frames as i64;
    let a_post_hi = (a_post_hi_i.max(0) as usize).min(total_a);
    let a_post_edge_pinned = a_post_hi_i > total_a as i64;

    let b_post_seam_frame = b_mapped(a_post_lo);
    let b_post_lo_i = b_post_seam_frame as i64 - max_lag_frames;
    let b_post_hi_i = b_mapped(a_post_hi) as i64 + max_lag_frames;
    let b_post_lo = b_post_lo_i.max(0) as usize;
    let b_post_hi = b_post_hi_i.max(0).min(b_len_frames as i64) as usize;
    let b_post_edge_pinned = b_post_lo_i < 0 || b_post_hi_i > b_len_frames as i64;

    let post = measure_shoulder(
        a_samples,
        b_haystack,
        ch,
        &picker_params,
        rate,
        (a_post_lo, a_post_hi),
        a_post_edge_pinned,
        (b_post_lo, b_post_hi),
        b_post_edge_pinned,
        a_post_lo as f64 / rate,
        b_post_seam_frame as f64 / rate,
        &hist_params,
        constellation_seed(gap_index, 1),
    );

    ConstellationSet {
        bin_ms: picker_params.bin_ms,
        min_spacing_ms: hist_params.min_spacing_secs * 1000.0,
        context_secs: cfg.gap_signature_context_secs,
        max_lag_ms: cfg.lag_envelope_max_lag_ms,
        vote_bin_ms: hist_params.vote_bin_ms,
        redraw_trials: hist_params.redraw_trials,
        redraw_percentile: hist_params.redraw_percentile,
        pre,
        post,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(context_secs: f64, max_lag_ms: u32) -> FingerprintConfig {
        FingerprintConfig {
            gap_signature_context_secs: context_secs,
            lag_envelope_max_lag_ms: max_lag_ms,
            ..Default::default()
        }
    }

    fn ctx_frames(secs: f64, rate: u32) -> usize {
        (secs * f64::from(rate)).round() as usize
    }

    fn write_click(buf: &mut [f32], at: usize, width: usize, amp: f32) {
        let end = (at + width).min(buf.len());
        for s in &mut buf[at..end] {
            *s = amp;
        }
    }

    // --- constellation_seed ---

    #[test]
    fn constellation_seed_differs_by_side_and_by_gap_index() {
        assert_ne!(constellation_seed(0, 0), constellation_seed(0, 1));
        assert_ne!(constellation_seed(0, 0), constellation_seed(1, 0));
        assert_eq!(
            constellation_seed(5, 1),
            constellation_seed(5, 1),
            "pure function of (gap_index, side_tag): must be deterministic across calls"
        );
    }

    // --- measure_shoulder: "not attempted" vs "attempted but empty" ---

    #[test]
    fn measure_shoulder_is_unmeasured_when_a_window_is_zero_width() {
        let rate = 8_000.0;
        let a = vec![0f32; 1_000];
        let b = vec![0f32; 1_000];
        let picker_params = PickerParams::first_candidate(8_000);
        let hist_params = LagHistogramParams::first_candidate(&picker_params);
        let shoulder = measure_shoulder(
            &a,
            &b,
            1,
            &picker_params,
            rate,
            (100, 100),
            false, // zero-width A window: never reaches evaluate_l_histogram
            (50, 150),
            false,
            100.0 / rate,
            100.0 / rate,
            &hist_params,
            1,
        );
        assert_eq!(
            shoulder.reason,
            ConstellationReason::Unmeasured,
            "{shoulder:?}"
        );
        assert_eq!(shoulder.n_a, 0);
    }

    #[test]
    fn measure_shoulder_is_unmeasured_when_b_window_is_zero_width() {
        let rate = 8_000.0;
        let a = vec![0f32; 1_000];
        let b = vec![0f32; 1_000];
        let picker_params = PickerParams::first_candidate(8_000);
        let hist_params = LagHistogramParams::first_candidate(&picker_params);
        let shoulder = measure_shoulder(
            &a,
            &b,
            1,
            &picker_params,
            rate,
            (0, 200),
            false,
            (300, 300),
            false, // zero-width B window
            0.0,
            300.0 / rate,
            &hist_params,
            1,
        );
        assert_eq!(
            shoulder.reason,
            ConstellationReason::Unmeasured,
            "{shoulder:?}"
        );
        assert_eq!(shoulder.n_b, 0);
    }

    #[test]
    fn measure_shoulder_is_no_train_when_both_windows_exist_but_yield_no_onsets() {
        let rate = 8_000.0;
        let a = vec![0f32; 1_000]; // silence: pick_onsets finds nothing
        let b = vec![0f32; 1_000];
        let picker_params = PickerParams::first_candidate(8_000);
        let hist_params = LagHistogramParams::first_candidate(&picker_params);
        let shoulder = measure_shoulder(
            &a,
            &b,
            1,
            &picker_params,
            rate,
            (0, 200),
            false,
            (0, 200),
            false,
            200.0 / rate,
            200.0 / rate,
            &hist_params,
            1,
        );
        assert_eq!(
            shoulder.reason,
            ConstellationReason::NoTrain,
            "{shoulder:?}"
        );
    }

    // --- side_from_window: t_seam_ms sign convention ---

    #[test]
    fn side_from_window_reports_negative_t_seam_ms_before_the_seam_and_positive_after() {
        let rate = 8_000u32;
        let picker_params = PickerParams::first_candidate(rate);
        let mut buf = vec![0f32; 4_000];
        write_click(&mut buf, 3_000, 20, 0.9); // click at 3000/8000 = 0.375s

        let (side, _) = side_from_window(
            &buf,
            1,
            &picker_params,
            1_000,
            4_000,
            f64::from(rate),
            0.4375,
            false,
        );
        assert_eq!(side.n, 1, "{side:?}");
        let p = &side.points[0];
        assert!((p.time_secs - 0.375).abs() < 1e-9, "{p:?}");
        assert!(
            p.t_seam_ms < 0.0,
            "click before the seam must read as a negative t_seam_ms: {p:?}"
        );

        let (side, _) = side_from_window(
            &buf,
            1,
            &picker_params,
            1_000,
            4_000,
            f64::from(rate),
            0.2,
            false,
        );
        let p = &side.points[0];
        assert!(
            p.t_seam_ms > 0.0,
            "click after the seam must read as a positive t_seam_ms: {p:?}"
        );
    }

    // --- measure_constellation: A-side edge_pinned vs Unmeasured at file boundaries ---

    #[test]
    fn pre_shoulder_zero_width_at_file_start_is_unmeasured() {
        let rate = 8_000u32;
        let total_a = 2_000usize;
        let a = vec![0f32; total_a];
        let b = vec![0f32; total_a];
        let refined = RefinedGapFrames {
            start_frame: 0,
            end_frame: 500,
        };
        let c = cfg(0.05, 100); // context_frames = 400 at rate 8000
        let set = measure_constellation(
            0,
            &a,
            &b,
            1,
            rate,
            f64::from(rate),
            refined,
            total_a,
            ctx_frames(0.05, rate),
            &c,
            0.0,
            0.0,
        );
        assert_eq!(
            set.pre.reason,
            ConstellationReason::Unmeasured,
            "{:?}",
            set.pre
        );
        assert_eq!(set.pre.a.span_start_secs, set.pre.a.span_end_secs);
        assert!(set.pre.a.edge_pinned);
    }

    #[test]
    fn pre_shoulder_edge_pinned_but_attempted_when_the_window_partially_exists() {
        let rate = 8_000u32;
        let total_a = 2_000usize;
        let a = vec![0f32; total_a];
        let b = vec![0f32; total_a];
        // start_frame(200) < context_frames(400): the pre window is truncated at the file start but
        // is not zero-width, so this must still attempt the lock test (NoTrain, not Unmeasured).
        let refined = RefinedGapFrames {
            start_frame: 200,
            end_frame: 500,
        };
        let c = cfg(0.05, 100);
        let set = measure_constellation(
            0,
            &a,
            &b,
            1,
            rate,
            f64::from(rate),
            refined,
            total_a,
            ctx_frames(0.05, rate),
            &c,
            0.0,
            0.0,
        );
        assert!(set.pre.a.edge_pinned);
        assert_ne!(set.pre.a.span_start_secs, set.pre.a.span_end_secs);
        assert_eq!(
            set.pre.reason,
            ConstellationReason::NoTrain,
            "{:?}",
            set.pre
        );
    }

    #[test]
    fn post_shoulder_zero_width_at_file_end_is_unmeasured() {
        let rate = 8_000u32;
        let total_a = 2_000usize;
        let a = vec![0f32; total_a];
        let b = vec![0f32; total_a];
        let refined = RefinedGapFrames {
            start_frame: 1_500,
            end_frame: total_a, // gap runs to the very end of the file
        };
        let c = cfg(0.05, 100);
        let set = measure_constellation(
            0,
            &a,
            &b,
            1,
            rate,
            f64::from(rate),
            refined,
            total_a,
            ctx_frames(0.05, rate),
            &c,
            0.0,
            0.0,
        );
        assert_eq!(
            set.post.reason,
            ConstellationReason::Unmeasured,
            "{:?}",
            set.post
        );
        assert_eq!(set.post.a.span_start_secs, set.post.a.span_end_secs);
        assert!(set.post.a.edge_pinned);
    }

    #[test]
    fn post_shoulder_edge_pinned_but_attempted_when_the_window_partially_exists() {
        let rate = 8_000u32;
        let total_a = 2_000usize;
        let a = vec![0f32; total_a];
        let b = vec![0f32; total_a];
        // end_frame(1900) + context_frames(400) overshoots total_a(2000): truncated but non-zero.
        let refined = RefinedGapFrames {
            start_frame: 1_500,
            end_frame: 1_900,
        };
        let c = cfg(0.05, 100);
        let set = measure_constellation(
            0,
            &a,
            &b,
            1,
            rate,
            f64::from(rate),
            refined,
            total_a,
            ctx_frames(0.05, rate),
            &c,
            0.0,
            0.0,
        );
        assert!(set.post.a.edge_pinned);
        assert_ne!(set.post.a.span_start_secs, set.post.a.span_end_secs);
        assert_eq!(
            set.post.reason,
            ConstellationReason::NoTrain,
            "{:?}",
            set.post
        );
    }

    // --- measure_constellation: B search-span geometry (plan §2 rule 7) ---

    #[test]
    fn b_search_span_is_the_widened_nominal_span_when_the_haystack_is_ample() {
        let rate = 8_000u32;
        let total_a = 6_000usize;
        let a = vec![0f32; total_a];
        let b = vec![0f32; 5_000]; // ample: nothing clamps
        let refined = RefinedGapFrames {
            start_frame: 2_000,
            end_frame: 2_500,
        };
        // context_frames = 200 (0.025s @ 8000), max_lag_frames = 800 (100ms @ 8000).
        let c = cfg(0.025, 100);
        let set = measure_constellation(
            0,
            &a,
            &b,
            1,
            rate,
            f64::from(rate),
            refined,
            total_a,
            ctx_frames(0.025, rate),
            &c,
            0.0,
            0.0,
        );

        assert!(!set.pre.a.edge_pinned, "{:?}", set.pre.a);
        assert!(!set.pre.b.edge_pinned, "{:?}", set.pre.b);
        // A pre window: [1800, 2000). b_mapped is identity (offset=0, extract_start=0), widened by
        // max_lag_frames(800) on both ends of that span: [1800-800, 2000+800) = [1000, 2800).
        assert_eq!(set.pre.b.span_start_secs, 1_000.0 / f64::from(rate));
        assert_eq!(set.pre.b.span_end_secs, 2_800.0 / f64::from(rate));

        assert!(!set.post.a.edge_pinned, "{:?}", set.post.a);
        assert!(!set.post.b.edge_pinned, "{:?}", set.post.b);
        // A post window: [2500, 2700). Widened B span: [2500-800, 2700+800) = [1700, 3500).
        assert_eq!(set.post.b.span_start_secs, 1_700.0 / f64::from(rate));
        assert_eq!(set.post.b.span_end_secs, 3_500.0 / f64::from(rate));
    }

    #[test]
    fn b_search_span_clamps_and_flags_edge_pinned_at_haystack_boundaries() {
        let rate = 8_000u32;
        let total_a = 2_000usize;
        let a = vec![0f32; total_a];
        let b = vec![0f32; 1_000]; // short haystack: both ends of the widened span overshoot it
        let refined = RefinedGapFrames {
            start_frame: 300,
            end_frame: 1_700,
        };
        // context_frames = 200 (0.025s @ 8000): A pre window [100, 300), not itself edge-pinned.
        // max_lag_frames = 800 (100ms @ 8000): widened B span [100-800, 300+800) = [-700, 1100),
        // clamped to the 1000-frame haystack on both ends.
        let c = cfg(0.025, 100);
        let set = measure_constellation(
            0,
            &a,
            &b,
            1,
            rate,
            f64::from(rate),
            refined,
            total_a,
            ctx_frames(0.025, rate),
            &c,
            0.0,
            0.0,
        );

        assert!(
            !set.pre.a.edge_pinned,
            "A side itself is not clamped: {:?}",
            set.pre.a
        );
        assert!(set.pre.b.edge_pinned, "{:?}", set.pre.b);
        assert_eq!(set.pre.b.span_start_secs, 0.0);
        assert_eq!(set.pre.b.span_end_secs, 1_000.0 / f64::from(rate));
    }
}
