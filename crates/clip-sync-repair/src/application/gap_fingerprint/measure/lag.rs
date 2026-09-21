//! `lag` slice of the gap_fingerprint measure folder split.

use crate::application::gap_fingerprint::schema::*;

use crate::domain::policies::{
    border_edge_displacement_frames, border_templates_per_channel_for_gap, GapBorderSpec,
    RefinedGapFrames,
};

use super::config::*;

use crate::domain::lag_decision::{
    lag_decision_geometry, lag_pair, probe_lag_border_spec, shoulder_lags_at_b_mapped,
    LagPairInput, LagSweepParams, ShoulderLagKnobs, WideBorder, WideBorderCut,
};

/// Inputs for [`lag_at_placement`] — A/B windows + gate placement for a single lag fingerprint.
pub(in crate::application::gap_fingerprint::measure) struct LagAtPlacementInput<'a> {
    pub(in crate::application::gap_fingerprint::measure) a_samples: &'a [f32],
    pub(in crate::application::gap_fingerprint::measure) channels: usize,
    pub(in crate::application::gap_fingerprint::measure) refined: RefinedGapFrames,
    pub(in crate::application::gap_fingerprint::measure) b_mono: &'a [f64],
    pub(in crate::application::gap_fingerprint::measure) b_ch: &'a [Vec<f64>],
    pub(in crate::application::gap_fingerprint::measure) selected: Option<usize>,
    pub(in crate::application::gap_fingerprint::measure) start_frame: usize,
    pub(in crate::application::gap_fingerprint::measure) cfg: &'a FingerprintConfig,
    pub(in crate::application::gap_fingerprint::measure) sample_rate: u32,
}
/// Lag fingerprint for a placement: A's kept border vs the B haystack swept over ±`max_lag`, on the
/// mono downmix **and** the gate-selected channel (where a multichannel failure lives).
pub(in crate::application::gap_fingerprint::measure) fn lag_at_placement(
    input: &LagAtPlacementInput<'_>,
) -> LagFingerprint {
    let LagAtPlacementInput {
        a_samples,
        channels,
        refined,
        b_mono,
        b_ch,
        selected,
        start_frame,
        cfg,
        sample_rate,
    } = *input;
    let knobs = ShoulderLagKnobs {
        sample_rate,
        lag_window_secs: cfg.lag_window_secs,
        lag_max_lag_ms: cfg.lag_max_lag_ms,
        lag_envelope_max_lag_ms: cfg.lag_envelope_max_lag_ms,
        lag_envelope_window_secs: cfg.lag_envelope_window_secs,
        border_capture: cfg.border_capture,
        silence_peak_fraction: cfg.silence_peak_fraction,
        absolute_silence_rms: cfg.absolute_silence_rms,
    };

    let mut out = LagFingerprint::default();
    let mut add = |pre: Option<LagSummary>, post: Option<LagSummary>| {
        if let Some(p) = pre {
            out.pre_anchor.push(p);
        }
        if let Some(p) = post {
            out.post_anchor.push(p);
        }
    };

    let (pre, post) =
        shoulder_lags_at_b_mapped(a_samples, channels, &refined, b_mono, start_frame, &knobs);
    add(pre, post);

    // Selected-channel pass stays here (dump-only). Same geometry as the mono caller so a
    // `--border-capture wide` dump cannot disagree with itself across channels.
    if let Some(sel) = selected {
        let ch = channels.max(1);
        let gap_frames = refined.end_frame.saturating_sub(refined.start_frame);
        let geo = lag_decision_geometry(&knobs);
        let border_spec = probe_lag_border_spec(
            &refined,
            geo.border_need,
            knobs.silence_peak_fraction,
            knobs.absolute_silence_rms,
        );
        let (a_pre_ch, a_post_ch) =
            border_templates_per_channel_for_gap(a_samples, ch, &border_spec);
        if sel < b_ch.len()
            && sel < a_pre_ch.len()
            && sel < a_post_ch.len()
            && !a_pre_ch[sel].is_empty()
        {
            let sel_disp = border_edge_displacement_frames(
                a_samples,
                ch,
                &border_spec,
                a_pre_ch[sel].len(),
                a_post_ch[sel].len(),
            );
            // Same helper as the mono path, so the selected-channel wide capture cannot be
            // anchored on a displacement measured from a different buffer.
            let wide_templates_ch = |spec: &GapBorderSpec| {
                let (pre_ch, post_ch) = border_templates_per_channel_for_gap(a_samples, ch, spec);
                (
                    pre_ch.get(sel).cloned().unwrap_or_default(),
                    post_ch.get(sel).cloned().unwrap_or_default(),
                )
            };
            let wide_cut = WideBorderCut {
                refined: &refined,
                wide_need: geo.wide_need,
                knobs: &knobs,
                a_samples,
                channels: ch,
            };
            let pre_wide_ch = || wide_cut.side(true, &wide_templates_ch);
            let post_wide_ch = || wide_cut.side(false, &wide_templates_ch);
            let (pre, post) = lag_pair(&LagPairInput {
                a_pre: &a_pre_ch[sel],
                a_post: &a_post_ch[sel],
                b_signal: &b_ch[sel],
                start_frame,
                gap_frames,
                params: LagSweepParams {
                    window: geo.window,
                    max_lag: geo.max_lag,
                    sample_rate,
                    channel: LagChannel::Selected(sel),
                },
                displacement: Some(sel_disp),
                envelope: geo.envelope_cfg,
                pre_wide: (!geo.shared_covers_wide)
                    .then_some(&pre_wide_ch as &dyn Fn() -> Option<WideBorder>),
                post_wide: (!geo.shared_covers_wide)
                    .then_some(&post_wide_ch as &dyn Fn() -> Option<WideBorder>),
            });
            add(pre, post);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::lag_decision::{
        is_recoverable, lag_side_sweep, rescue_shoulder, summarize_lag_curve, LagSideSweep,
    };
    use crate::domain::lag_envelope::{envelope_recenter_frames, EnvelopeCfg, EnvelopePeak};
    use crate::domain::policies::RefinedGapFrames;
    use crate::domain::seam_local::lag_correlation_curve;

    fn write_noise(buf: &mut [f32], start: usize, end: usize, seed: u64, amp: f32) {
        for (f, slot) in buf.iter_mut().enumerate().take(end).skip(start) {
            *slot = noise(seed, f) as f32 * amp;
        }
    }

    /// White noise with a slow unique envelope so 100 ms-bin correlation can lock (flat noise cannot).
    fn write_shaped_noise(
        buf: &mut [f32],
        start: usize,
        end: usize,
        seed: u64,
        amp: f32,
        rising: bool,
    ) {
        let span = (end - start).max(1) as f64;
        for (f, slot) in buf.iter_mut().enumerate().take(end).skip(start) {
            let t = (f - start) as f64 / span;
            let env = if rising { t } else { 1.0 - t };
            *slot = noise(seed, f) as f32 * amp * (0.2 + 0.8 * env) as f32;
        }
    }
    fn noise(seed: u64, i: usize) -> f64 {
        let mut z = ((seed << 32) | (i as u64 & 0xffff_ffff)).wrapping_add(0x9E37_79B9_7F4A_7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        ((z >> 40) as f64 / (1u64 << 24) as f64) * 2.0 - 1.0
    }

    fn base_noise(seed: u64, len: usize) -> Vec<f64> {
        (0..len).map(|i| noise(seed, i)).collect()
    }

    fn lerp(base: &[f64], x: f64) -> f64 {
        if x <= 0.0 {
            return base[0];
        }
        let i = x.floor() as usize;
        if i + 1 >= base.len() {
            return base[base.len() - 1];
        }
        let f = x - i as f64;
        base[i] * (1.0 - f) + base[i + 1] * f
    }

    /// Build (a, b_ctx) where B is the shared broadband signal delayed by `offset` samples (possibly
    /// fractional), so the lag curve should peak at lag ≈ `offset`.
    fn shifted_pair(offset: f64, n: usize, max_lag: i64) -> (Vec<f64>, Vec<f64>) {
        let ml = max_lag as f64;
        let a0 = (ml + offset.abs()).ceil() as usize + 4;
        let base = base_noise(1, a0 + n + max_lag as usize + 8);
        let a: Vec<f64> = (0..n).map(|i| base[a0 + i]).collect();
        let ctx_len = n + 2 * max_lag as usize;
        let b_ctx: Vec<f64> = (0..ctx_len)
            .map(|k| lerp(&base, a0 as f64 + (k as f64 - ml - offset)))
            .collect();
        (a, b_ctx)
    }

    #[test]
    fn lag_curve_finds_integer_offset() {
        let (a, b_ctx) = shifted_pair(17.0, 4000, 64);
        let curve = lag_correlation_curve(&a, &b_ctx, 64);
        let s = summarize_lag_curve(&curve, 48_000, 83, 1, LagChannel::Mono).expect("summary");
        assert_eq!(s.peak_lag_samples, 17, "peak at the true integer lag");
        assert!(
            s.peak_r > 0.95,
            "shared signal correlates ~1 at the lag, got {}",
            s.peak_r
        );
        assert!(
            s.lag0_r.expect("lag 0 is on this curve") < 0.5,
            "lag-0 is depressed by the offset, got {:?}",
            s.lag0_r
        );
        assert_eq!(s.verdict, LagVerdict::TimingOffset);
    }

    #[test]
    fn summarize_lag_curve_omits_lag0_r_when_zero_not_on_curve() {
        // Gross-shifted fine window: lags around +202 samples, no lag 0. Fabricating 0.0 here
        // was the far-recenter / chained-post foot-gun.
        let curve: Vec<(i64, f64)> = (200i64..=204)
            .map(|l| (l, 0.9 - (l - 202).unsigned_abs() as f64 * 0.02))
            .collect();
        let s = summarize_lag_curve(&curve, 48_000, 1000, 600, LagChannel::Mono).expect("summary");
        assert_eq!(s.lag0_r, None, "lag 0 was not searched; do not write 0.0");
        assert_eq!(s.verdict, LagVerdict::TimingOffset);
    }

    #[test]
    fn lag_curve_resolves_fractional_offset() {
        let (a, b_ctx) = shifted_pair(17.4, 4000, 64);
        let curve = lag_correlation_curve(&a, &b_ctx, 64);
        let s = summarize_lag_curve(&curve, 48_000, 83, 1, LagChannel::Mono).expect("summary");
        assert!(
            (s.frac_lag_samples - 17.4).abs() < 0.3,
            "parabolic lag {} should land near 17.4",
            s.frac_lag_samples
        );
        assert_eq!(s.verdict, LagVerdict::TimingOffset);
    }

    #[test]
    fn lag_curve_flat_for_decorrelated_sources() {
        let n = 4000;
        let max_lag = 64i64;
        let a = base_noise(1, n);
        let b_ctx = base_noise(2, n + 2 * max_lag as usize); // independent stream
        let curve = lag_correlation_curve(&a, &b_ctx, max_lag);
        let s = summarize_lag_curve(&curve, 48_000, 83, 1, LagChannel::Mono).expect("summary");
        assert!(
            s.peak_r < 0.3,
            "independent noise should not correlate at any lag, peak {}",
            s.peak_r
        );
        assert_eq!(s.verdict, LagVerdict::Decorrelated);
    }

    #[test]
    fn lag_summary_flags_competing_peak_for_periodic_curve() {
        // Unique peak: one hump, monotonic falloff → no rival local maximum.
        let unique: Vec<(i64, f64)> = (-5..=5)
            .map(|l| (l, 1.0 - 0.1 * (l as f64).abs()))
            .collect();
        let s = summarize_lag_curve(&unique, 48_000, 10, 1, LagChannel::Mono).expect("summary");
        assert!(
            s.second_peak_r.is_none_or(|r| r < s.peak_r - 0.3),
            "a unique peak should have no strong rival: {:?}",
            s.second_peak_r
        );

        // Periodic-like: two humps of similar height → a rival near peak_r (low uniqueness margin).
        let periodic = vec![
            (-4, 0.2),
            (-3, 0.9),
            (-2, 0.5),
            (-1, 0.3),
            (0, 0.85),
            (1, 0.4),
            (2, 0.2),
            (3, 0.1),
            (4, 0.05),
        ];
        let s2 = summarize_lag_curve(&periodic, 48_000, 10, 1, LagChannel::Mono).expect("summary");
        assert!(
            s2.second_peak_r.is_some_and(|r| r > 0.8),
            "a periodic curve should expose a competing peak near peak_r: {:?}",
            s2.second_peak_r
        );

        // Robust uniqueness fields: the unique (monotonic) curve has NO rival → prominence `None` (best
        // case); the periodic one has a rival near the peak → low prominence. peak_z stands out further for
        // the unique curve. Spacing is the lag gap to the rival.
        assert!(
            s.prominence.is_none(),
            "a unrivalled peak has no prominence value: {:?}",
            s.prominence
        );
        assert!(
            s2.prominence.is_some_and(|p| p < 0.1),
            "periodic: low prominence {:?}",
            s2.prominence
        );
        // peak_z is computed (finite, positive) whenever the curve has spread; its discriminating power is
        // validated on real flat-floor curves in the §3.6a experiment, not these broad toy humps.
        assert!(
            s.peak_z.is_some_and(|z| z.is_finite() && z > 0.0),
            "peak_z computed: {:?}",
            s.peak_z
        );
        assert!(
            s2.peak_z.is_some_and(|z| z.is_finite() && z > 0.0),
            "peak_z computed: {:?}",
            s2.peak_z
        );
        // periodic peak at lag 0, rival at lag −3 → spacing 3 samples = 62.5 µs at 48 kHz.
        assert!(
            s2.top2_spacing_ms
                .is_some_and(|ms| (ms - 3.0 * 1000.0 / 48_000.0).abs() < 1e-6),
            "spacing to the rival lag: {:?}",
            s2.top2_spacing_ms
        );
    }

    #[test]
    fn lag_curve_truncates_when_context_too_short() {
        // b_ctx shorter than a.len() + 2*max_lag → fewer than the full set of lags, no panic.
        let a = base_noise(1, 100);
        let b_ctx = base_noise(1, 120);
        let curve = lag_correlation_curve(&a, &b_ctx, 64);
        assert!(curve.len() < (2 * 64 + 1));
        assert!(curve.iter().all(|(_, r)| r.is_finite()));
    }

    /// A *perfectly* registered pair reads lag 0 on both shoulders. The A templates start `D_pre` /
    /// `D_post` away from the gap edges (fade trimmed at 12 % of peak); map coordinates
    /// (`docs/dev/archive/TEMP-lag-decision-map-coordinates-plan.md` § Change) shift the B haystack anchor by
    /// the same `D` so the sweep is centered on the template's true corresponding B-time, not the raw
    /// gap edge. `border_displacement_ms` stays on the wire as a record of that walk-off — it is no
    /// longer a reader-side correction to `frac_lag_ms`.
    #[test]
    fn lag_at_placement_reads_border_displacement_at_true_registration() {
        const RATE: u32 = 48_000;
        let d_pre = 200usize;
        let d_post = 150usize;
        let (gap_start, gap_end) = (4_200usize, 4_500usize);
        let total = 9_000usize;

        // Loud broadband either side, with a quiet fade into and out of the dropout: the fade is
        // above the silence floor (so it is not walked off) but below the 12 %-of-peak trim.
        let mut a = vec![0.0f32; total];
        write_noise(&mut a, 0, gap_start - d_pre, 7, 0.5);
        write_noise(&mut a, gap_start - d_pre, gap_start, 7, 0.5 * 0.02);
        write_noise(&mut a, gap_end, gap_end + d_post, 7, 0.5 * 0.02);
        write_noise(&mut a, gap_end + d_post, total, 7, 0.5);

        // B carries the same program at the same time — true registration is exactly 0.
        let b_mono: Vec<f64> = a.iter().map(|&v| f64::from(v)).collect();

        let config = FingerprintConfig {
            lag_window_secs: 0.01,      // 480 frames
            lag_max_lag_ms: 20,         // ±960 frames — comfortably past both displacements
            lag_envelope_max_lag_ms: 0, // isolate map-coordinates; this fixture is too short for ±2 s
            ..FingerprintConfig::default()
        };
        let out = lag_at_placement(&LagAtPlacementInput {
            a_samples: &a,
            channels: 1,
            refined: RefinedGapFrames {
                start_frame: gap_start,
                end_frame: gap_end,
            },
            b_mono: &b_mono,
            b_ch: &[],
            selected: None,
            start_frame: gap_start,
            cfg: &config,
            sample_rate: RATE,
        });

        let ms = |frames: usize| frames as f64 * 1000.0 / f64::from(RATE);
        let pre = out.pre_anchor.first().copied().expect("pre shoulder");
        let post = out.post_anchor.first().copied().expect("post shoulder");
        assert!(
            pre.peak_r > 0.95 && post.peak_r > 0.95,
            "both shoulders lock"
        );
        assert_eq!(pre.border_displacement_ms, Some(ms(d_pre)));
        assert_eq!(post.border_displacement_ms, Some(ms(d_post)));
        assert!(
            pre.frac_lag_ms.abs() < 0.05,
            "pre reads {} ms, not 0 — map coordinates should absorb D_pre",
            pre.frac_lag_ms
        );
        assert!(
            post.frac_lag_ms.abs() < 0.05,
            "post reads {} ms, not 0 — map coordinates should absorb D_post",
            post.frac_lag_ms
        );
    }

    fn lag_at_ms(rate: u32, ms: i64) -> usize {
        (ms.unsigned_abs() as f64 / 1000.0 * f64::from(rate)).round() as usize
    }

    /// 31/g7 class: unique pre near 0, unique post at −1160 ms. Recenter is per-shoulder —
    /// sharing one gap-level envelope would yank pre onto the post lag.
    #[test]
    fn lag_at_placement_envelope_recenter_is_per_shoulder() {
        const RATE: u32 = 48_000;
        let pre_len = lag_at_ms(RATE, 3000);
        let gap = lag_at_ms(RATE, 2000);
        let post_len = lag_at_ms(RATE, 3000);
        let post_lag = lag_at_ms(RATE, 1160);
        let gap_start = pre_len;
        let gap_end = pre_len + gap;
        let total = gap_end + post_len + lag_at_ms(RATE, 500);

        let mut a = vec![0.0f32; total];
        write_shaped_noise(&mut a, 0, gap_start, 11, 0.5, true);
        write_shaped_noise(&mut a, gap_end, gap_end + post_len, 23, 0.5, false);

        let mut b_mono = vec![0.0f64; total];
        for (i, &v) in a.iter().take(gap_start).enumerate() {
            b_mono[i] = f64::from(v);
        }
        let b_post_at = gap_end - post_lag;
        for i in 0..post_len {
            b_mono[b_post_at + i] = f64::from(a[gap_end + i]);
        }

        let out = lag_at_placement(&LagAtPlacementInput {
            a_samples: &a,
            channels: 1,
            refined: RefinedGapFrames {
                start_frame: gap_start,
                end_frame: gap_end,
            },
            b_mono: &b_mono,
            b_ch: &[],
            selected: None,
            start_frame: gap_start,
            cfg: &FingerprintConfig::default(),
            sample_rate: RATE,
        });
        let pre = out.pre_anchor.first().copied().expect("pre shoulder");
        let post = out.post_anchor.first().copied().expect("post shoulder");
        assert!(
            pre.peak_r > 0.8 && pre.frac_lag_ms.abs() < 50.0,
            "pre must stay the near-nominal landmark, got r={} lag={} ms",
            pre.peak_r,
            pre.frac_lag_ms
        );
        assert!(
            post.peak_r > 0.8 && (post.frac_lag_ms - (-1160.0)).abs() < 50.0,
            "post should unlock near -1160, got r={} lag={} ms",
            post.peak_r,
            post.frac_lag_ms
        );
        assert_eq!(post.edge_pinned, Some(false));
        assert_eq!(post.envelope_edge_pinned, Some(false));
        assert!(
            pre.lag0_r.is_some(),
            "pre stays near nominal, so lag 0 is on the fine curve"
        );
        // Post's envelope center is ~−1160 but 100 ms bins can leave lag 0 on the +600 ms edge
        // of the fine window. If it is present it must be the measured dead-at-b_mapped value,
        // not a fabricated 0.0 from a missing bin (that case is `summarize_lag_curve_omits_*`
        // and the shared-far test, where |center| > 600 ms).
        if let Some(l0) = post.lag0_r {
            assert!(
                l0.abs() < 0.2,
                "post lag 0 on the curve is dead at original b_mapped, got {l0}"
            );
        }
    }

    /// 31/g10 class: both shoulders true at ~−1153 ms. Off, both are dead at ±600; on, both lock.
    #[test]
    fn lag_at_placement_envelope_shared_far_unlocks_both() {
        const RATE: u32 = 48_000;
        let shift = lag_at_ms(RATE, 1153);
        let pre_len = lag_at_ms(RATE, 3000);
        let gap = lag_at_ms(RATE, 300);
        let post_len = lag_at_ms(RATE, 3000);
        // Pad so the −1153 ms B copy of pre still sits inside the haystack (2 s envelope + lag).
        let pad = shift + lag_at_ms(RATE, 500);
        let gap_start = pad + pre_len;
        let gap_end = gap_start + gap;
        let total = gap_end + post_len + shift;

        let mut a = vec![0.0f32; total];
        write_shaped_noise(&mut a, pad, gap_start, 13, 0.5, true);
        write_shaped_noise(&mut a, gap_end, gap_end + post_len, 17, 0.5, false);

        // B carries the same program 1153 ms earlier: A at t matches B at t − 1153.
        let b_mono: Vec<f64> = (0..total)
            .map(|i| {
                let j = i + shift;
                if j < a.len() {
                    f64::from(a[j])
                } else {
                    0.0
                }
            })
            .collect();

        let run = |env_ms: u32| {
            let config = FingerprintConfig {
                lag_envelope_max_lag_ms: env_ms,
                ..FingerprintConfig::default()
            };
            lag_at_placement(&LagAtPlacementInput {
                a_samples: &a,
                channels: 1,
                refined: RefinedGapFrames {
                    start_frame: gap_start,
                    end_frame: gap_end,
                },
                b_mono: &b_mono,
                b_ch: &[],
                selected: None,
                start_frame: gap_start,
                cfg: &config,
                sample_rate: RATE,
            })
        };

        let off = run(0);
        let pre_off = off.pre_anchor.first().copied().expect("pre off");
        let post_off = off.post_anchor.first().copied().expect("post off");
        assert!(
            pre_off.envelope_lag_ms.is_none() && post_off.envelope_lag_ms.is_none(),
            "knob 0 must omit envelope_*"
        );
        assert!(
            pre_off.frac_lag_ms.abs() < 600.0 && post_off.frac_lag_ms.abs() < 600.0,
            "off: fine window stays ±600 of nominal"
        );

        let on = run(2000);
        let pre = on.pre_anchor.first().copied().expect("pre on");
        let post = on.post_anchor.first().copied().expect("post on");
        assert!(
            pre.peak_r > 0.8 && (pre.frac_lag_ms - (-1153.0)).abs() < 50.0,
            "pre shared-far lock, got r={} lag={} ms",
            pre.peak_r,
            pre.frac_lag_ms
        );
        assert!(
            post.peak_r > 0.8 && (post.frac_lag_ms - (-1153.0)).abs() < 50.0,
            "post shared-far lock, got r={} lag={} ms",
            post.peak_r,
            post.frac_lag_ms
        );
        assert_eq!(pre.edge_pinned, Some(false));
        assert_eq!(post.edge_pinned, Some(false));
        assert_eq!(
            pre.lag0_r, None,
            "shared-far lock: original b_mapped is outside the fine window"
        );
        assert_eq!(post.lag0_r, None);
    }

    /// 26/g6 shape: a healthy nominal lock (`peak_r` well above 0.5) with a perfect decoy match
    /// inside the envelope's own search radius — if the envelope were consulted here it would
    /// lock onto the decoy immediately (identical content, correlation ~1). Nominal-first must
    /// never even look: a recoverable shoulder short-circuits before the envelope is touched.
    #[test]
    fn rescue_shoulder_never_consults_the_envelope_when_nominal_is_already_recoverable() {
        let n = 4000usize;
        let a_border: Vec<f64> = (0..n).map(|i| noise(7, i)).collect();
        let mut b_signal = vec![0.0f64; 20_000];
        let decoy_at = 12_000usize;
        for (i, &v) in a_border.iter().enumerate() {
            b_signal[decoy_at + i] = v;
        }
        let params = LagSweepParams {
            window: 2000,
            max_lag: 1000,
            sample_rate: 48_000,
            channel: LagChannel::Mono,
        };
        let envelope = Some(EnvelopeCfg {
            window: 2000,
            max_lag: 1000,
            bin: 100,
        });
        // Alive at nominal but *not* a landmark (`peak_z` well under 12) — the is_landmark trap:
        // triage must key off `peak_r`, not `is_landmark`, or this shape would be misread as dead.
        let nominal = LagSummary {
            window_ms: 1000,
            max_lag_ms: 600,
            channel: LagChannel::Mono,
            lag0_r: Some(0.55),
            peak_r: 0.55,
            second_peak_r: None,
            peak_z: Some(6.0),
            prominence: Some(0.1),
            top2_spacing_ms: None,
            peak_lag_samples: 0,
            frac_lag_samples: 0.0,
            frac_lag_ms: 0.0,
            edge_pinned: Some(false),
            border_displacement_ms: None,
            envelope_window_ms: None,
            envelope_border_displacement_ms: None,
            envelope_lag_ms: None,
            envelope_peak_r: None,
            envelope_edge_pinned: None,
            envelope_rescue: None,
            verdict: LagVerdict::TimingOffset,
        };
        assert!(
            !nominal.is_landmark(),
            "fixture must sit below the landmark bar to exercise the trap"
        );

        let out = rescue_shoulder(
            Some(nominal),
            envelope,
            LagSideSweep {
                a_border: &a_border,
                b_signal: &b_signal,
                anchor_frame: 11_500,
                pre_shoulder: true,
                gross_lag_shift: 0,
                border_displacement_frames: None,
                // No wide capture in this test, so the raw edge is the anchor itself.
                gap_edge_frame: 11_500,
            },
            params,
            None,
        )
        .expect("recoverable shoulder is always returned");

        assert_eq!(
            LagSummary {
                envelope_rescue: None,
                ..out
            },
            nominal,
            "a recoverable shoulder must be returned untouched — the envelope must never move \
             it, even with a perfect decoy match inside its search radius"
        );
        assert_eq!(out.envelope_rescue, Some(EnvelopeRescue::NotConsulted));
    }

    /// The `is_landmark` trap, narrowly: 26/g6's *real* nominal numbers (`peak_r` 0.997, `peak_z`
    /// only 6.13 — well under the landmark's `peak_z >= 12` bar) must still triage as alive.
    #[test]
    fn rescue_shoulder_triage_uses_peak_r_not_is_landmark() {
        let nominal = LagSummary {
            window_ms: 1000,
            max_lag_ms: 600,
            channel: LagChannel::Mono,
            lag0_r: Some(0.997),
            peak_r: 0.997,
            second_peak_r: Some(0.9),
            peak_z: Some(6.13),
            prominence: Some(0.097),
            top2_spacing_ms: Some(10.0),
            peak_lag_samples: -2,
            frac_lag_samples: -2.0,
            frac_lag_ms: -0.04,
            edge_pinned: Some(false),
            border_displacement_ms: None,
            envelope_window_ms: None,
            envelope_border_displacement_ms: None,
            envelope_lag_ms: None,
            envelope_peak_r: None,
            envelope_edge_pinned: None,
            envelope_rescue: None,
            verdict: LagVerdict::TimingOffset,
        };
        assert!(is_recoverable(&nominal), "peak_r 0.997 must be recoverable");
        assert!(
            !nominal.is_landmark(),
            "peak_z 6.13 must fail the landmark bar (>= 12)"
        );

        let out = rescue_shoulder(
            Some(nominal),
            Some(EnvelopeCfg {
                window: 100,
                max_lag: 100,
                bin: 10,
            }),
            LagSideSweep {
                a_border: &[0.0; 200],
                b_signal: &[0.0; 400],
                anchor_frame: 150,
                pre_shoulder: true,
                gross_lag_shift: 0,
                border_displacement_frames: None,
                // No wide capture in this test, so the raw edge is the anchor itself.
                gap_edge_frame: 150,
            },
            LagSweepParams {
                window: 100,
                max_lag: 50,
                sample_rate: 48_000,
                channel: LagChannel::Mono,
            },
            None,
        )
        .expect("recoverable shoulder is always returned");
        assert_eq!(out.envelope_rescue, Some(EnvelopeRescue::NotConsulted));
    }

    /// pair-50 shape: dead at nominal, and the envelope proposes nothing usable (unrelated noise
    /// on both sides — no real match at any lag, coarse or fine). The honest nominal reading must
    /// be kept, with the declined candidate stamped.
    #[test]
    fn rescue_shoulder_declines_a_still_dead_candidate() {
        let a_border: Vec<f64> = (0..2000).map(|i| noise(3, i)).collect();
        let b_signal: Vec<f64> = (0..10_000).map(|i| noise(9, i)).collect();
        let nominal = LagSummary {
            window_ms: 1000,
            max_lag_ms: 600,
            channel: LagChannel::Mono,
            lag0_r: Some(0.05),
            peak_r: 0.2,
            second_peak_r: Some(0.15),
            peak_z: Some(1.0),
            prominence: Some(0.05),
            top2_spacing_ms: Some(20.0),
            peak_lag_samples: 5,
            frac_lag_samples: 5.0,
            frac_lag_ms: 0.1,
            edge_pinned: Some(false),
            border_displacement_ms: None,
            envelope_window_ms: None,
            envelope_border_displacement_ms: None,
            envelope_lag_ms: None,
            envelope_peak_r: None,
            envelope_edge_pinned: None,
            envelope_rescue: None,
            verdict: LagVerdict::Decorrelated,
        };
        let params = LagSweepParams {
            window: 1000,
            max_lag: 600,
            sample_rate: 48_000,
            channel: LagChannel::Mono,
        };
        let envelope = Some(EnvelopeCfg {
            window: 1000,
            max_lag: 600,
            bin: 100,
        });

        let out = rescue_shoulder(
            Some(nominal),
            envelope,
            LagSideSweep {
                a_border: &a_border,
                b_signal: &b_signal,
                anchor_frame: 5000,
                pre_shoulder: true,
                gross_lag_shift: 0,
                border_displacement_frames: None,
                // No wide capture in this test, so the raw edge is the anchor itself.
                gap_edge_frame: 5000,
            },
            params,
            None,
        )
        .expect("dead shoulder is still returned");

        assert_eq!(out.envelope_rescue, Some(EnvelopeRescue::Declined));
        assert_eq!(
            out.peak_r, 0.2,
            "declined candidate keeps the honest nominal reading"
        );
        assert_eq!(out.verdict, LagVerdict::Decorrelated);
    }

    /// `lag_envelope_window_secs` is read by the rescue branch, and the window competes with the
    /// **B-side lead-in** — not with the shoulder — for the right to run.
    ///
    /// Same fixture as the border-capture test below: a 2500 ms pre shoulder whose true lag is
    /// −1160 ms, far outside the ±600 ms fine sweep, so only the envelope can find it. The A-side
    /// template is clamped to what the shoulder holds (`w = cfg.window.min(a_border.len())`), so a
    /// wide window is never refused for lack of A material. The binding guard is
    /// `anchor_frame < w + max_lag` in [`envelope_shoulder`]: the recenter needs `window + max_lag`
    /// of B *before* the gap, or it declines without correlating.
    ///
    /// Two off-default windows lose the rescue here, for **different** reasons, and neither is a
    /// defect:
    ///
    /// - `0.05` is below the 100 ms envelope bin, so no envelope can be built at all;
    /// - `4.0` needs 6000 ms of lead-in and the compact arm supplies 4160 ms.
    ///
    /// The lead-in pair is the point: the *same* 4.0 s window declines at 4160 ms of lead-in and
    /// rescues at 6660 ms. Widening does not disable the search on its own — it raises the lead-in
    /// the gap must sit behind, which bites only near the start of B. Production capture pads B by
    /// `gap_signature_context_secs + fill_border_search_secs + fill_align_margin_secs` (14 s at the
    /// defaults), so this guard has wide headroom on real dumps.
    ///
    /// Both refusals leave `envelope_peak_r` at `None`, so the observable cannot say which guard
    /// fired — the lead-in pair is what pins the cause. See BACKLOG § *Fill placement*,
    /// `env_frames == 0` conflates two verdicts.
    #[test]
    fn envelope_window_secs_is_read_and_trades_against_b_lead_in() {
        const RATE: u32 = 48_000;
        let pre_lag = lag_at_ms(RATE, 1160);
        let pre_len = lag_at_ms(RATE, 2500);
        let gap = lag_at_ms(RATE, 2000);
        let post_len = lag_at_ms(RATE, 3000);

        // `lead_in_ms` is the B material before the gap: `front_pad + pre_len`. The guard needs
        // `window + lag_envelope_max_lag_ms` (2000 ms) of it.
        let build = |pad_ms: i64| {
            let front_pad = pre_lag + lag_at_ms(RATE, pad_ms);
            let gap_start = front_pad + pre_len;
            let gap_end = gap_start + gap;
            let total = gap_end + post_len;

            let mut a = vec![0.0f32; total];
            write_shaped_noise(&mut a, front_pad, gap_start, 11, 0.5, true);
            write_shaped_noise(&mut a, gap_end, gap_end + post_len, 23, 0.5, false);

            let mut b_mono = vec![0.0f64; total];
            for i in 0..post_len {
                b_mono[gap_end + i] = f64::from(a[gap_end + i]);
            }
            let b_pre_at = gap_start - pre_lag - pre_len;
            for i in 0..pre_len {
                b_mono[b_pre_at + i] = f64::from(a[front_pad + i]);
            }
            (a, b_mono, gap_start, gap_end)
        };

        let run = |pad_ms: i64, secs: f64, capture: BorderCapture| {
            let (a, b_mono, gap_start, gap_end) = build(pad_ms);
            let config = FingerprintConfig {
                lag_envelope_window_secs: secs,
                border_capture: capture,
                ..FingerprintConfig::default()
            };
            lag_at_placement(&LagAtPlacementInput {
                a_samples: &a,
                channels: 1,
                refined: RefinedGapFrames {
                    start_frame: gap_start,
                    end_frame: gap_end,
                },
                b_mono: &b_mono,
                b_ch: &[],
                selected: None,
                start_frame: gap_start,
                cfg: &config,
                sample_rate: RATE,
            })
            .pre_anchor
            .first()
            .copied()
            .expect("pre shoulder")
        };

        // 500 ms of extra pad ⇒ 4160 ms of lead-in; 3000 ms ⇒ 6660 ms.
        const TIGHT: i64 = 500;
        const ROOMY: i64 = 3000;

        for capture in [BorderCapture::Narrow, BorderCapture::Wide] {
            let default = run(TIGHT, DEFAULT_LAG_ENVELOPE_WINDOW_SECS, capture);
            assert_eq!(
                default.envelope_rescue,
                Some(EnvelopeRescue::Rescued),
                "{capture:?}: the default window must still rescue"
            );
            assert!(
                (default.frac_lag_ms - (-1160.0)).abs() < 50.0,
                "{capture:?}: default-window lag drifted: {} ms",
                default.frac_lag_ms
            );

            // Below the 100 ms bin: no envelope is measurable, so the knob provably reaches the
            // rescue branch — this assertion fails if the field is declared but never read.
            let too_narrow = run(TIGHT, 0.05, capture);
            assert_eq!(
                too_narrow.envelope_peak_r, None,
                "{capture:?}: a sub-bin window cannot produce an envelope peak"
            );
            assert_ne!(
                too_narrow.envelope_rescue,
                Some(EnvelopeRescue::Rescued),
                "{capture:?}: a sub-bin window must not rescue"
            );

            // One window, two lead-ins. This pair is what identifies the guard: if the refusal
            // below were about shoulder length or A material, the roomy arm would decline too.
            let starved = run(TIGHT, 4.0, capture);
            assert_eq!(
                starved.envelope_peak_r, None,
                "{capture:?}: 4.0 s needs 6000 ms of lead-in and this arm has 4160 ms"
            );
            assert_ne!(
                starved.envelope_rescue,
                Some(EnvelopeRescue::Rescued),
                "{capture:?}: a starved lead-in must not rescue"
            );

            let fed = run(ROOMY, 4.0, capture);
            assert_eq!(
                fed.envelope_rescue,
                Some(EnvelopeRescue::Rescued),
                "{capture:?}: the same 4.0 s window rescues once the lead-in reaches 6660 ms"
            );
            assert!(
                (fed.frac_lag_ms - (-1160.0)).abs() < 50.0,
                "{capture:?}: wide-window lag drifted: {} ms",
                fed.frac_lag_ms
            );
        }
    }

    /// `shared_covers_wide` deliberately accepts a capture *longer* than the envelope's own reach,
    /// and `envelope_window_ms` is what makes that legible in the dump.
    ///
    /// The shared capture is not the same slice as one cut to `wide_need`: `gap_border_frame_range`
    /// trims silence inward from the gap bounded by `pre_start = gap_start - border_frames`, so a
    /// longer capture survives the trim with more material. `envelope_shoulder` then clamps to
    /// `cfg.window.min(a_border.len())`, so the extra length can only help it reach its full window.
    ///
    /// This is the arm the border-capture tests never reach: `narrow_need` (1.0 s window + 600 ms
    /// sweep = 1.6 s) *exceeds* `wide_need` (0.3 s window + 1000 ms envelope lag = 1.3 s), so on
    /// `Narrow` the shortcut fires and the envelope gets the 1.6 s border. A 1200 ms silent tail
    /// then decides the outcome, because each capture keeps only `border_need - tail`: the 1.3 s
    /// reach the envelope declared keeps 100 ms — one bin, not correlatable — while the 1.6 s
    /// border keeps 400 ms and the shoulder rescues at r ≈ 0.998.
    ///
    /// An `==` predicate declines this shoulder instead — measured: `envelope_peak_r` `None` and
    /// `envelope_window_ms` 100 ms. That was tried and rejected: it trades a correct rescue for a
    /// reproducibility property the recipe cannot express anyway. The cost is that the envelope's
    /// input depends on `--border-capture`, which is why `envelope_window_ms` records the template
    /// length actually correlated — here it is the field that tells the two outcomes apart.
    #[test]
    fn shared_capture_longer_than_the_envelope_reach_is_used_and_recorded() {
        const RATE: u32 = 48_000;
        // Outside the ±600 ms fine sweep (so the rescue is consulted) and comfortably inside the
        // 1000 ms envelope lag — a peak on the last searched bin sets `edge_pinned` and declines.
        let pre_lag = lag_at_ms(RATE, 650);
        let pre_len = lag_at_ms(RATE, 4000);
        let gap = lag_at_ms(RATE, 2000);
        let post_len = lag_at_ms(RATE, 3000);
        let front_pad = pre_lag + lag_at_ms(RATE, 6000);
        let gap_start = front_pad + pre_len;
        let gap_end = gap_start + gap;
        let total = gap_end + post_len;

        // Each capture survives the trim with `border_need - quiet_tail`. At 1200 ms that leaves
        // `wide_need` (1.3 s) a single 100 ms bin and `narrow_need` (1.6 s) a full 0.3 s window.
        let quiet_tail = lag_at_ms(RATE, 1200);
        let mut a = vec![0.0f32; total];
        write_shaped_noise(&mut a, front_pad, gap_start - quiet_tail, 11, 0.5, true);
        write_shaped_noise(&mut a, gap_end, gap_end + post_len, 23, 0.5, false);

        let mut b_mono = vec![0.0f64; total];
        for i in 0..post_len {
            b_mono[gap_end + i] = f64::from(a[gap_end + i]);
        }
        let b_pre_at = gap_start - pre_lag - pre_len;
        for i in 0..pre_len {
            b_mono[b_pre_at + i] = f64::from(a[front_pad + i]);
        }

        let config = FingerprintConfig {
            lag_envelope_window_secs: 0.3,
            lag_envelope_max_lag_ms: 1000,
            border_capture: BorderCapture::Narrow,
            ..FingerprintConfig::default()
        };
        let out = lag_at_placement(&LagAtPlacementInput {
            a_samples: &a,
            channels: 1,
            refined: RefinedGapFrames {
                start_frame: gap_start,
                end_frame: gap_end,
            },
            b_mono: &b_mono,
            b_ch: &[],
            selected: None,
            start_frame: gap_start,
            cfg: &config,
            sample_rate: RATE,
        })
        .pre_anchor
        .first()
        .copied()
        .expect("pre shoulder");

        assert_eq!(
            out.envelope_rescue,
            Some(EnvelopeRescue::Rescued),
            "the shared capture must still rescue this shoulder"
        );
        assert!(
            out.envelope_peak_r.is_some_and(|r| r > 0.9),
            "expected a strong envelope peak, got {:?}",
            out.envelope_peak_r
        );
        // The full 0.3 s window: the 1.6 s border kept 400 ms after the trim, so nothing clamped
        // it. Against the envelope's own 1.3 s reach this reads 100 ms — a single bin — and refuses.
        assert_eq!(
            out.envelope_window_ms,
            Some(300.0),
            "envelope_window_ms must record the template length actually correlated"
        );
    }

    /// The two [`BorderCapture`] arms, on the shape that motivated the knob: a shoulder dead at
    /// nominal that only the envelope can reach.
    ///
    /// Two contracts, and the second is the load-bearing one:
    /// 1. `Wide` is not a regression — it still rescues what `Narrow` rescues.
    /// 2. With the envelope **off** the arms must be bit-identical. `Wide` only widens to
    ///    `max(narrow, env_window + env_lag)` when `env_on`, mirroring the pre-2026-08-20
    ///    `if env_on { .. }` guard. Without that gate `--border-capture wide` would silently re-cut
    ///    every template on envelope-off dumps too — a corpus-wide re-baseline nobody asked for.
    #[test]
    fn border_capture_wide_matches_narrow_when_the_envelope_is_off() {
        const RATE: u32 = 48_000;
        let pre_lag = lag_at_ms(RATE, 1160);
        let pre_len = lag_at_ms(RATE, 2500);
        let gap = lag_at_ms(RATE, 2000);
        let post_len = lag_at_ms(RATE, 3000);
        let front_pad = pre_lag + lag_at_ms(RATE, 500);
        let gap_start = front_pad + pre_len;
        let gap_end = gap_start + gap;
        let total = gap_end + post_len;

        let mut a = vec![0.0f32; total];
        write_shaped_noise(&mut a, front_pad, gap_start, 11, 0.5, true);
        write_shaped_noise(&mut a, gap_end, gap_end + post_len, 23, 0.5, false);

        let mut b_mono = vec![0.0f64; total];
        for i in 0..post_len {
            b_mono[gap_end + i] = f64::from(a[gap_end + i]);
        }
        let b_pre_at = gap_start - pre_lag - pre_len;
        for i in 0..pre_len {
            b_mono[b_pre_at + i] = f64::from(a[front_pad + i]);
        }

        let run = |env_ms: u32, capture: BorderCapture| {
            let config = FingerprintConfig {
                lag_envelope_max_lag_ms: env_ms,
                border_capture: capture,
                ..FingerprintConfig::default()
            };
            lag_at_placement(&LagAtPlacementInput {
                a_samples: &a,
                channels: 1,
                refined: RefinedGapFrames {
                    start_frame: gap_start,
                    end_frame: gap_end,
                },
                b_mono: &b_mono,
                b_ch: &[],
                selected: None,
                start_frame: gap_start,
                cfg: &config,
                sample_rate: RATE,
            })
        };

        // 1. Envelope on: `Wide` rescues what `Narrow` rescues.
        let narrow_on = run(2000, BorderCapture::Narrow);
        let wide_on = run(2000, BorderCapture::Wide);
        let pre_narrow = narrow_on.pre_anchor.first().copied().expect("pre narrow");
        let pre_wide = wide_on.pre_anchor.first().copied().expect("pre wide");
        assert_eq!(pre_narrow.envelope_rescue, Some(EnvelopeRescue::Rescued));
        assert_eq!(
            pre_wide.envelope_rescue,
            Some(EnvelopeRescue::Rescued),
            "the wide arm must not lose a rescue the narrow arm makes"
        );
        assert!(
            (pre_wide.frac_lag_ms - (-1160.0)).abs() < 50.0,
            "wide arm lag drifted: {} ms",
            pre_wide.frac_lag_ms
        );

        // 2. Envelope off: the arms coincide, field for field.
        let narrow_off = run(0, BorderCapture::Narrow);
        let wide_off = run(0, BorderCapture::Wide);
        assert_eq!(
            narrow_off.pre_anchor, wide_off.pre_anchor,
            "border_capture must be inert when the envelope is off (pre)"
        );
        assert_eq!(
            narrow_off.post_anchor, wide_off.post_anchor,
            "border_capture must be inert when the envelope is off (post)"
        );
    }

    /// **Regression for [TEMP-rescue-wide-anchor-plan.md](docs/dev/archive/TEMP-rescue-wide-anchor-plan.md) § 4/§ 5.**
    /// The rescue branch used to correlate the lazily-built **wide** capture at the **shared
    /// narrow** capture's anchor, so its curve sat `D_wide − D_narrow` out of frame. It is now
    /// anchored on the capture it actually correlates.
    ///
    /// The border-capture test above cannot see this: its pre shoulder is uniform, so both captures
    /// trim to the same edge and the mismatch is zero. This fixture makes the two captures disagree
    /// the way 31/g8's do. A loud 3 s passage is followed by a quiet (never silent) 2 s that runs up
    /// to the gap:
    ///
    /// - the 1.6 s narrow capture is all quiet, its own peak is ~0.03, the 12 % trim keeps it — `D_narrow ≈ 0`;
    /// - the 4.0 s wide capture reaches the loud passage, its peak is ~0.5, and 12 % of that is
    ///   above the quiet material, so the trim eats all 2 s of it — `D_wide ≈ 2000 ms`.
    ///
    /// True lag is −1160 ms, outside the ±600 ms fine sweep, so only the envelope can find it.
    /// Correctly anchored it sits well inside the ±2000 ms reach; anchored 2000 ms late it would
    /// have to appear at −3160 ms, and no floor, statistic or wider rung can reach it.
    ///
    /// `Wide` is the control: there the shared capture *is* the wide one, `D_narrow = D_wide`, and
    /// the same shoulder rescues. The two arms disagreeing was the defect (measured 2026-09-19,
    /// § 4's Result: `Narrow` ran off its window at r 0.000, edge-pinned, and kept `decorrelated`);
    /// they must now agree, and the `Wide` arm must not have moved.
    #[test]
    fn rescue_wide_capture_is_anchored_in_its_own_frame() {
        use crate::domain::policies::border_templates_for_gap;

        const RATE: u32 = 48_000;
        const TRUE_LAG_MS: f64 = -1160.0;

        let pre_lag = lag_at_ms(RATE, 1160);
        let loud_len = lag_at_ms(RATE, 3000);
        let quiet_len = lag_at_ms(RATE, 2000);
        let pre_len = loud_len + quiet_len;
        let gap = lag_at_ms(RATE, 2000);
        let post_len = lag_at_ms(RATE, 3000);
        // Lead-in: the corrected pre anchor is `gap_start − D_wide`, and `envelope_shoulder` needs
        // `window + max_lag` (2 s + 2 s) of B before it.
        let front_pad = pre_lag + lag_at_ms(RATE, 2000);
        let gap_start = front_pad + pre_len;
        let gap_end = gap_start + gap;
        let total = gap_end + post_len;

        let mut a = vec![0.0f32; total];
        // Loud, shaped so a 100 ms-bin envelope can lock on it.
        write_shaped_noise(&mut a, front_pad, front_pad + loud_len, 11, 0.5, true);
        // Quiet but never silent: 0.03 is ~6x under the wide capture's 12 % floor (0.06) and ~8x
        // over the narrow capture's (0.0036).
        write_noise(&mut a, gap_start - quiet_len, gap_start, 17, 0.03);
        write_shaped_noise(&mut a, gap_end, gap_end + post_len, 23, 0.5, false);

        let mut b_mono = vec![0.0f64; total];
        for i in 0..post_len {
            b_mono[gap_end + i] = f64::from(a[gap_end + i]);
        }
        let b_pre_at = gap_start - pre_lag - pre_len;
        for i in 0..pre_len {
            b_mono[b_pre_at + i] = f64::from(a[front_pad + i]);
        }

        // 1. The fixture's premise: the two captures do not share an edge.
        let refined = RefinedGapFrames {
            start_frame: gap_start,
            end_frame: gap_end,
        };
        let displacement_ms = |need_ms: i64| {
            let spec = probe_lag_border_spec(&refined, lag_at_ms(RATE, need_ms), 0.01, 0.0);
            let (pre, post) = border_templates_for_gap(&a, 1, &spec);
            let (d_pre, _) = border_edge_displacement_frames(&a, 1, &spec, pre.len(), post.len());
            d_pre as f64 * 1000.0 / f64::from(RATE)
        };
        let d_narrow = displacement_ms(1600);
        let d_wide = displacement_ms(4000);
        assert!(
            d_narrow < 50.0,
            "narrow capture should keep the quiet shoulder, D_narrow = {d_narrow} ms"
        );
        assert!(
            d_wide > 1900.0,
            "wide capture should trim the quiet shoulder, D_wide = {d_wide} ms"
        );

        let run = |capture: BorderCapture| {
            let config = FingerprintConfig {
                border_capture: capture,
                ..FingerprintConfig::default()
            };
            lag_at_placement(&LagAtPlacementInput {
                a_samples: &a,
                channels: 1,
                refined,
                b_mono: &b_mono,
                b_ch: &[],
                selected: None,
                start_frame: gap_start,
                cfg: &config,
                sample_rate: RATE,
            })
            .pre_anchor
            .first()
            .copied()
            .expect("pre shoulder")
        };

        // 2. `Wide` — self-consistent, and the ground truth for this shoulder.
        let wide = run(BorderCapture::Wide);
        assert_eq!(
            wide.envelope_rescue,
            Some(EnvelopeRescue::Rescued),
            "wide arm must rescue: {wide:?}"
        );
        assert!(
            (wide.frac_lag_ms - TRUE_LAG_MS).abs() < 100.0,
            "wide arm lag drifted: {} ms",
            wide.frac_lag_ms
        );
        // On this arm the shared capture already covers the envelope's reach, so nothing is re-cut
        // and the field stays absent — the presence rule `LagSummary` documents.
        assert_eq!(
            wide.envelope_border_displacement_ms, None,
            "shared_covers_wide must not stamp an envelope displacement: {wide:?}"
        );

        // 3. `Narrow` — the shoulder the defect used to lose. Before the anchor fix the wide
        //    capture was correlated at the narrow capture's anchor, so the true peak sat at
        //    −3160 ms, outside the ±2000 ms reach, and no floor or guard could reach it. With the
        //    envelope anchored on `D_wide` the two arms now agree.
        let narrow = run(BorderCapture::Narrow);
        assert_eq!(
            narrow.envelope_rescue,
            Some(EnvelopeRescue::Rescued),
            "narrow arm must rescue once the envelope is anchored on its own capture: {narrow:?}"
        );
        assert!(
            (narrow.frac_lag_ms - TRUE_LAG_MS).abs() < 100.0,
            "narrow arm lag drifted: {} ms",
            narrow.frac_lag_ms
        );

        // 4. The re-anchoring is *recorded*, and it is the wide capture's displacement — not the
        //    shared capture's, which `border_displacement_ms` still reports.
        assert!(
            narrow
                .envelope_border_displacement_ms
                .is_some_and(|d| (d - d_wide).abs() < 50.0),
            "envelope displacement should be D_wide ({d_wide} ms): {narrow:?}"
        );
        assert!(
            narrow
                .border_displacement_ms
                .is_some_and(|d| (d - d_narrow).abs() < 50.0),
            "shared-capture displacement must stay D_narrow ({d_narrow} ms): {narrow:?}"
        );

        // 5. No translation term (plan § 5, point 3): the fine retry reads the *same* lag the
        //    coarse envelope proposed, to within one 100 ms bin. A reader's first instinct is that
        //    `D_wide - D_narrow` must be added back somewhere; it must not be.
        let envelope_lag = narrow.envelope_lag_ms.expect("envelope ran");
        assert!(
            (narrow.frac_lag_ms - envelope_lag).abs() < 100.0,
            "coarse and fine lag must share a frame: envelope {envelope_lag} ms vs fine {} ms",
            narrow.frac_lag_ms
        );
    }

    /// Chain-isolation shape: dead pre + alive post. The post's reported summary must be
    /// unaffected by whatever the pre's rescue does — bit-identical whether or not the envelope
    /// (and therefore the pre's rescue) runs at all. `pre_shift` is derived from the *nominal*
    /// pre only (`docs/dev/archive/TEMP-lag-nominal-first-plan.md` § Change, "Landmark chain stays
    /// anchored to the nominal pass"), so rescuing pre can never move post's own search center.
    #[test]
    fn lag_at_placement_post_unaffected_by_a_rescued_pre() {
        const RATE: u32 = 48_000;
        let pre_lag = lag_at_ms(RATE, 1160);
        let pre_len = lag_at_ms(RATE, 2500);
        let gap = lag_at_ms(RATE, 2000);
        let post_len = lag_at_ms(RATE, 3000);
        let front_pad = pre_lag + lag_at_ms(RATE, 500);
        let gap_start = front_pad + pre_len;
        let gap_end = gap_start + gap;
        let total = gap_end + post_len;

        let mut a = vec![0.0f32; total];
        write_shaped_noise(&mut a, front_pad, gap_start, 11, 0.5, true);
        write_shaped_noise(&mut a, gap_end, gap_end + post_len, 23, 0.5, false);

        let mut b_mono = vec![0.0f64; total];
        // Post: identical at nominal (alive, r ~= 1).
        for i in 0..post_len {
            b_mono[gap_end + i] = f64::from(a[gap_end + i]);
        }
        // Pre: true content sits `pre_lag` earlier than nominal — dead at nominal (outside the
        // ±600 ms fine window), rescuable via the envelope.
        let b_pre_at = gap_start - pre_lag - pre_len;
        for i in 0..pre_len {
            b_mono[b_pre_at + i] = f64::from(a[front_pad + i]);
        }

        let run = |env_ms: u32| {
            let config = FingerprintConfig {
                lag_envelope_max_lag_ms: env_ms,
                ..FingerprintConfig::default()
            };
            lag_at_placement(&LagAtPlacementInput {
                a_samples: &a,
                channels: 1,
                refined: RefinedGapFrames {
                    start_frame: gap_start,
                    end_frame: gap_end,
                },
                b_mono: &b_mono,
                b_ch: &[],
                selected: None,
                start_frame: gap_start,
                cfg: &config,
                sample_rate: RATE,
            })
        };

        let off = run(0);
        let on = run(2000);
        let pre_off = off.pre_anchor.first().copied().expect("pre off");
        let post_off = off.post_anchor.first().copied().expect("post off");
        let pre_on = on.pre_anchor.first().copied().expect("pre on");
        let post_on = on.post_anchor.first().copied().expect("post on");

        assert!(
            post_off.peak_r > 0.8,
            "post must be alive at nominal, got {}",
            post_off.peak_r
        );
        assert!(
            pre_off.peak_r < 0.5,
            "pre must be dead at nominal (env off), got {}",
            pre_off.peak_r
        );
        assert!(
            pre_on.peak_r > 0.8 && (pre_on.frac_lag_ms - (-1160.0)).abs() < 50.0,
            "pre must be rescued when the envelope is on, got r={} lag={} ms",
            pre_on.peak_r,
            pre_on.frac_lag_ms
        );
        assert_eq!(pre_on.envelope_rescue, Some(EnvelopeRescue::Rescued));
        assert_eq!(
            LagSummary {
                envelope_rescue: None,
                ..post_on
            },
            post_off,
            "post must be bit-identical whether or not the dead pre gets rescued"
        );
    }

    /// Pre-fix post search: center on `start_frame + gap_frames` with no pre-shift — stacks
    /// `L_pre + (D_B - D_A)` into one ±max_lag window (ledger A2).
    fn naive_lag_pair_post(
        a_post: &[f64],
        b_signal: &[f64],
        start_frame: usize,
        gap_frames: usize,
        params: LagSweepParams,
    ) -> Option<LagSummary> {
        lag_side_sweep(
            LagSideSweep {
                a_border: a_post,
                b_signal,
                anchor_frame: start_frame + gap_frames,
                pre_shoulder: false,
                gross_lag_shift: 0,
                border_displacement_frames: None,
                // No wide capture in this test, so the raw edge is the anchor itself.
                gap_edge_frame: start_frame + gap_frames,
            },
            params,
        )
    }

    /// Regression: when `|L_pre + (D_B - D_A)|` exceeds ±max_lag but `|D_B - D_A|` alone fits,
    /// sequential `lag_pair` finds the post shoulder while naive centering does not.
    #[test]
    fn lag_pair_sequential_decouples_pre_offset_from_bridge_mismatch() {
        const RATE: u32 = 48_000;
        let w = 4_000usize;
        let max_lag = 4_000i64;
        let ml = max_lag as usize;
        let start_frame = 30_000usize;
        let gap_frames = 20_000usize; // D_A — keep post burst out of the pre search window
        let l_pre_true = 3_000i64;
        let bridge_delta = 1_500i64; // |L_pre + bridge| = 4500 > max_lag; |bridge| = 1500 <= max_lag

        let b_post_match = start_frame as i64 + l_pre_true + gap_frames as i64 + bridge_delta;
        let naive_lag_needed = b_post_match - (start_frame + gap_frames) as i64;
        assert_eq!(naive_lag_needed, l_pre_true + bridge_delta);
        assert!(naive_lag_needed.unsigned_abs() > max_lag as u64);

        let mut b_signal = vec![0.0f64; 100_000];

        let (a_pre, pre_ctx) = shifted_pair(l_pre_true as f64, w, max_lag);
        let pre_lo = start_frame.saturating_sub(w + ml);
        for (i, &v) in pre_ctx.iter().enumerate() {
            if pre_lo + i < b_signal.len() {
                b_signal[pre_lo + i] = v;
            }
        }

        let (a_post, post_ctx) = shifted_pair(bridge_delta as f64, w, max_lag);
        let seq_post_base = start_frame as i64 + gap_frames as i64 + l_pre_true;
        let post_lo = (seq_post_base - ml as i64).max(0) as usize;
        for (i, &v) in post_ctx.iter().enumerate() {
            if post_lo + i < b_signal.len() {
                b_signal[post_lo + i] = v;
            }
        }

        let sweep = LagSweepParams {
            window: w,
            max_lag,
            sample_rate: RATE,
            channel: LagChannel::Mono,
        };

        let (pre, post_seq) = lag_pair(&LagPairInput {
            a_pre: &a_pre,
            a_post: &a_post,
            b_signal: &b_signal,
            start_frame,
            gap_frames,
            params: sweep,
            displacement: None,
            envelope: None,
            pre_wide: None,
            post_wide: None,
        });
        let pre = pre.expect("pre shoulder");
        let post_seq = post_seq.expect("sequential post shoulder");
        assert!(
            (pre.frac_lag_samples - l_pre_true as f64).abs() < 5.0,
            "pre lag {} should land near {l_pre_true}",
            pre.frac_lag_samples
        );
        assert!(pre.peak_r > 0.95, "pre peak_r {}", pre.peak_r);

        let gross_post_expected = l_pre_true + bridge_delta;
        assert!(
            (post_seq.frac_lag_samples - gross_post_expected as f64).abs() < 5.0,
            "gross post lag {} should land near {gross_post_expected}",
            post_seq.frac_lag_samples
        );
        assert!(
            post_seq.peak_r > 0.95,
            "sequential post peak_r {}",
            post_seq.peak_r
        );

        let post_naive = naive_lag_pair_post(&a_post, &b_signal, start_frame, gap_frames, sweep)
            .expect("naive post summary");
        assert!(
            post_naive.peak_r < post_seq.peak_r - 0.15,
            "naive post should be worse than sequential (naive r {} at lag {} vs sequential r {})",
            post_naive.peak_r,
            post_naive.frac_lag_samples,
            post_seq.peak_r
        );
    }

    /// Saturated envelope (50/g9 class): a coarse peak on the last searched bin fail-opens to
    /// `b_mapped` even when `peak_r` clears 0.70. Interior + high-r still recenters; a missing
    /// peak and an interior peak below `min_envelope_r` also fail open.
    #[test]
    fn envelope_recenter_frames_fail_opens_when_edge_pinned() {
        const RATE: u32 = 48_000;
        let expected_interior = (-1160.0 / 1000.0 * f64::from(RATE)).round() as i64;
        assert_eq!(
            envelope_recenter_frames(
                Some(EnvelopePeak {
                    lag_ms: -1160.0,
                    peak_r: 0.95,
                    edge_pinned: false,
                }),
                RATE
            ),
            expected_interior,
            "interior peak at r 0.95 must recenter"
        );
        assert_eq!(
            envelope_recenter_frames(
                Some(EnvelopePeak {
                    lag_ms: -2000.0,
                    peak_r: 0.95,
                    edge_pinned: true,
                }),
                RATE
            ),
            0,
            "edge-pinned peak must fail open even at r 0.95"
        );
        assert_eq!(
            envelope_recenter_frames(
                Some(EnvelopePeak {
                    lag_ms: -1160.0,
                    peak_r: 0.50,
                    edge_pinned: false,
                }),
                RATE
            ),
            0,
            "r below min_envelope_r (0.70) fail-opens even when interior"
        );
        assert_eq!(envelope_recenter_frames(None, RATE), 0);
    }

    /// 50/g9 class through `lag_at_placement`: unique post at exactly −2000 ms (the default
    /// envelope half-width). The coarse peak sits on the last bin, so rescue fail-opens; the
    /// fine sweep stays at nominal and does not inherit the far lock.
    #[test]
    fn lag_at_placement_envelope_edge_pinned_fail_opens() {
        const RATE: u32 = 48_000;
        let pre_len = lag_at_ms(RATE, 3000);
        let gap = lag_at_ms(RATE, 2000);
        let post_len = lag_at_ms(RATE, 3000);
        let post_lag = lag_at_ms(RATE, 2000);
        let gap_start = pre_len;
        let gap_end = pre_len + gap;
        let total = gap_end + post_len + lag_at_ms(RATE, 500);

        let mut a = vec![0.0f32; total];
        write_shaped_noise(&mut a, 0, gap_start, 11, 0.5, true);
        write_shaped_noise(&mut a, gap_end, gap_end + post_len, 23, 0.5, false);

        let mut b_mono = vec![0.0f64; total];
        for (i, &v) in a.iter().take(gap_start).enumerate() {
            b_mono[i] = f64::from(v);
        }
        let b_post_at = gap_end - post_lag;
        for i in 0..post_len {
            b_mono[b_post_at + i] = f64::from(a[gap_end + i]);
        }

        let out = lag_at_placement(&LagAtPlacementInput {
            a_samples: &a,
            channels: 1,
            refined: RefinedGapFrames {
                start_frame: gap_start,
                end_frame: gap_end,
            },
            b_mono: &b_mono,
            b_ch: &[],
            selected: None,
            start_frame: gap_start,
            cfg: &FingerprintConfig::default(),
            sample_rate: RATE,
        });
        let pre = out.pre_anchor.first().copied().expect("pre shoulder");
        let post = out.post_anchor.first().copied().expect("post shoulder");
        assert!(
            pre.peak_r > 0.8 && pre.frac_lag_ms.abs() < 50.0,
            "pre must stay the near-nominal landmark, got r={} lag={} ms",
            pre.peak_r,
            pre.frac_lag_ms
        );
        assert_eq!(post.envelope_edge_pinned, Some(true));
        assert_eq!(post.envelope_rescue, Some(EnvelopeRescue::Declined));
        assert!(
            post.envelope_lag_ms
                .is_some_and(|ms| (ms - (-2000.0)).abs() < 150.0),
            "coarse peak should sit on the −2000 ms bin, got {:?}",
            post.envelope_lag_ms
        );
        assert!(
            (post.frac_lag_ms - (-2000.0)).abs() > 500.0,
            "fail-open must not move the fine sweep onto the saturated peak, got {} ms",
            post.frac_lag_ms
        );
        assert!(
            !is_recoverable(&post),
            "nominal ±600 cannot see a −2000 ms lock, got r={}",
            post.peak_r
        );
    }
}
