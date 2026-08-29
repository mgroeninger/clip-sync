//! `lag` slice of the gap_fingerprint measure folder split.

use crate::application::gap_fingerprint::schema::*;

use crate::domain::policies::{
    border_edge_displacement_frames, border_templates_for_gap,
    border_templates_per_channel_for_gap, RefinedGapFrames,
};

use super::config::*;
use super::pcm::*;

/// Search knobs for a per-shoulder lag correlation sweep (`lag_side_sweep` / [`lag_pair`]).
#[derive(Debug, Clone, Copy)]
pub(in crate::application::gap_fingerprint::measure) struct LagSweepParams {
    pub(in crate::application::gap_fingerprint::measure) window: usize,
    pub(in crate::application::gap_fingerprint::measure) max_lag: i64,
    pub(in crate::application::gap_fingerprint::measure) sample_rate: u32,
    pub(in crate::application::gap_fingerprint::measure) channel: LagChannel,
}
impl LagSweepParams {
    fn win_ms(self) -> u32 {
        ((self.window as f64) * 1000.0 / f64::from(self.sample_rate.max(1))) as u32
    }

    fn max_lag_ms(self) -> u32 {
        let ml = self.max_lag.max(0) as usize;
        ((ml as f64) * 1000.0 / f64::from(self.sample_rate.max(1))) as u32
    }

    fn ml(self) -> usize {
        self.max_lag.max(0) as usize
    }
}
/// One shoulder of a [`lag_pair`] sweep: an A border template vs the B haystack around `anchor_frame`.
///
/// [`rescue_shoulder`] / [`envelope_shoulder`] reuse this bag. For rescue, `anchor_frame` is the
/// **unchained** envelope/nominal center (`post_nominal`, not `post_nominal + pre_shift`) and
/// `gross_lag_shift` is the sequential chain applied on top of that center — passing the already
/// chained post `LagSideSweep` from the nominal sweep would double-count `pre_shift`.
#[derive(Debug, Clone, Copy)]
pub(in crate::application::gap_fingerprint::measure) struct LagSideSweep<'a> {
    pub(in crate::application::gap_fingerprint::measure) a_border: &'a [f64],
    pub(in crate::application::gap_fingerprint::measure) b_signal: &'a [f64],
    pub(in crate::application::gap_fingerprint::measure) anchor_frame: usize,
    /// `true` for the pre shoulder (last `window` samples of `a_border`); `false` for post (first `window`).
    pub(in crate::application::gap_fingerprint::measure) pre_shoulder: bool,
    /// Samples added to each lag bin in the returned summary (sequential post: gross map to `b_mapped_end`).
    pub(in crate::application::gap_fingerprint::measure) gross_lag_shift: i64,
    /// This shoulder's template-edge displacement from the gap edge (`border_edge_displacement_frames`),
    /// recorded as `LagSummary::border_displacement_ms`. `None` where the caller builds no border
    /// templates (seam-local / dual-fit windows), which is what an absent field then means.
    pub(in crate::application::gap_fingerprint::measure) border_displacement_frames: Option<usize>,
}
/// Prominence of a seam's placement peak over its tallest rival within ±`max_lag`. `b_ctx` must be laid
/// out so lag 0 aligns `a_win` at the placement (`b_ctx[max_lag .. max_lag + a_win.len()]`). Low prominence
/// ⇒ the seam matches at many lags (periodic) ⇒ the dual-fit placement is not a unique registration.
pub(in crate::application::gap_fingerprint::measure) fn seam_prominence(
    a_win: &[f64],
    b_ctx: &[f64],
    max_lag: i64,
    sample_rate: u32,
) -> Option<f64> {
    let curve = lag_correlation_curve(a_win, b_ctx, max_lag);
    summarize_lag_curve(&curve, sample_rate, 0, 0, LagChannel::Mono).and_then(|s| s.prominence)
}
/// `lag_correlation_curve` + `seam_local_peak` moved to the shared `domain::seam_local` so the production
/// dual-fit repair (A3) and this diagnostic scan use one implementation (no drift). Re-exported here so the
/// existing call sites / tests keep their paths.
/// `LAG_EDGE_TOL_MS` and the `lag_is_edge_pinned` predicate moved with them: `SeamPeak::edge_pinned` and
/// `LagSummary::edge_pinned` are the same test, so it is one function in `domain`, not one per path.
pub use crate::domain::seam_local::{
    lag_correlation_curve, lag_correlation_curve_auto, lag_is_edge_pinned, seam_local_peak,
};
/// The coarse-envelope primitives moved to the shared `domain::lag_envelope` for the same reason
/// `seam_local` did: the production patch path and this diagnostic scan must use one implementation
/// (no drift). Re-exported here so the existing call sites / tests keep their paths.
pub use crate::domain::lag_envelope::{
    envelope_recenter_frames, envelope_shoulder, EnvelopeCfg, EnvelopePeak, EnvelopeSide,
    LAG_DECISION_RECOVERABLE_R,
};
/// Summarize a lag curve: lag-0 value, integer peak, parabolic-interpolated (fractional) peak, and a
/// [`LagVerdict`]. `None` for an empty curve.
pub fn summarize_lag_curve(
    curve: &[(i64, f64)],
    sample_rate: u32,
    window_ms: u32,
    max_lag_ms: u32,
    channel: LagChannel,
) -> Option<LagSummary> {
    if curve.is_empty() {
        return None;
    }
    // Correlation at gross lag 0 only when that sample was actually searched. A missing bin is
    // not 0.0 (`finite_corr(NaN)` used to fabricate a dead lag-0 on far recenter / chained post).
    let lag0_r = curve
        .iter()
        .find(|(l, _)| *l == 0)
        .map(|(_, r)| finite_corr(*r));
    let (pi, &(peak_lag, peak_r)) = curve
        .iter()
        .enumerate()
        .max_by(|(_, (_, x)), (_, (_, y))| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal))?;

    // Parabolic vertex through the integer peak and its two neighbours (when interior).
    let (frac_lag, frac_r) = if pi > 0 && pi < curve.len() - 1 {
        let y0 = curve[pi - 1].1;
        let y1 = peak_r;
        let y2 = curve[pi + 1].1;
        let denom = y0 - 2.0 * y1 + y2;
        if denom.abs() > 1e-12 {
            let xstar = (y0 - y2) / (2.0 * denom);
            let peak = y1 - (y2 - y0) * (y2 - y0) / (8.0 * denom);
            (peak_lag as f64 + xstar, peak)
        } else {
            (peak_lag as f64, peak_r)
        }
    } else {
        (peak_lag as f64, peak_r)
    };

    let rate = f64::from(sample_rate).max(1.0);
    // Robust uniqueness from curve shape: prominence + spacing to the tallest rival, and the peak's
    // z-score over the whole curve (§3.6a — the metric that separates registration from periodicity).
    let second = secondary_peak(curve, pi);
    let n = curve.len() as f64;
    let mean = curve.iter().map(|(_, r)| *r).sum::<f64>() / n.max(1.0);
    let std = (curve.iter().map(|(_, r)| (r - mean).powi(2)).sum::<f64>() / n.max(1.0)).sqrt();
    // Edge-pin: the peak lands at (or within tol of) the boundary of the *searched* lags. Read the
    // boundary from the curve itself (`lag_correlation_curve` masks high-side lags where the window runs
    // past B), so this is the true searched extent, not the nominal `±max_lag`. The predicate itself is
    // `domain::seam_local`'s, shared with `SeamPeak::edge_pinned` — one definition of "pinned".
    let lo_lag = curve.first().map(|(l, _)| *l).unwrap_or(peak_lag);
    let hi_lag = curve.last().map(|(l, _)| *l).unwrap_or(peak_lag);
    let edge_pinned = lag_is_edge_pinned(peak_lag, lo_lag, hi_lag, sample_rate);
    Some(LagSummary {
        window_ms,
        max_lag_ms,
        channel,
        lag0_r,
        peak_r: finite_corr(peak_r),
        second_peak_r: second.map(|(_, r)| r),
        peak_z: (std > 1e-9).then(|| (peak_r - mean) / std),
        prominence: second.map(|(_, r)| peak_r - r),
        top2_spacing_ms: second
            .map(|(lag, _)| (peak_lag - lag).unsigned_abs() as f64 * 1000.0 / rate),
        peak_lag_samples: peak_lag,
        frac_lag_samples: frac_lag,
        frac_lag_ms: frac_lag * 1000.0 / rate,
        edge_pinned: Some(edge_pinned),
        // Stamped by the caller that owns the border templates (`lag_side_sweep`); the curve alone
        // cannot know how far the template edge sits from the gap.
        border_displacement_ms: None,
        // Nominal sweep: the envelope has not run, so there is no template length to report. Set by
        // `stamp_rescue` on the paths that do consult it.
        envelope_window_ms: None,
        envelope_lag_ms: None,
        envelope_peak_r: None,
        envelope_edge_pinned: None,
        envelope_rescue: None,
        verdict: classify_lag(lag0_r, frac_r, peak_lag),
    })
}
/// The tallest **competing** local maximum `(lag, r)` that is not the main peak at `peak_index`. Within
/// the main peak's lobe the curve falls off monotonically (no local maxima), so a separate local maximum
/// is a genuine rival lag — the periodicity / ambiguity signal. `None` when the peak is unrivalled.
pub(in crate::application::gap_fingerprint::measure) fn secondary_peak(
    curve: &[(i64, f64)],
    peak_index: usize,
) -> Option<(i64, f64)> {
    let mut best: Option<(i64, f64)> = None;
    for i in 1..curve.len().saturating_sub(1) {
        if i == peak_index {
            continue;
        }
        let (lag, r) = curve[i];
        // Local maximum: ≥ both neighbours (≥ tolerates plateaus without double-counting the main lobe).
        if r >= curve[i - 1].1 && r >= curve[i + 1].1 {
            best = Some(match best {
                Some((_, br)) if br >= r => best.unwrap(),
                _ => (lag, r),
            });
        }
    }
    best
}
/// Verdict thresholds (see plan §4). `peak` is the parabolic-interpolated peak correlation.
/// `lag0_r` is `None` when lag 0 was not on the searched curve; offset-away then rests on
/// `peak_lag` alone (a far recenter always has `|peak_lag| ≫ 1`).
pub(in crate::application::gap_fingerprint::measure) fn classify_lag(
    lag0_r: Option<f64>,
    peak: f64,
    peak_lag: i64,
) -> LagVerdict {
    let recoverable = peak >= 0.5;
    let offset_away = peak_lag.abs() > 1 || lag0_r.is_some_and(|l0| (peak - l0) > 0.2);
    if recoverable && offset_away {
        LagVerdict::TimingOffset
    } else if peak < 0.3 {
        LagVerdict::Decorrelated
    } else {
        LagVerdict::Ambiguous
    }
}

// ---------------------------------------------------------------------------------------------
// Builder — assembles a fingerprint from decoded windows using public domain functions only
// (no patch-path coupling). Mirrors `tests/diag_anchor_quiet_gap.rs`.
// ---------------------------------------------------------------------------------------------
/// Correlate one A border shoulder against B over ±`params.max_lag` around `side.anchor_frame`.
pub(in crate::application::gap_fingerprint::measure) fn lag_side_sweep(
    side: LagSideSweep<'_>,
    params: LagSweepParams,
) -> Option<LagSummary> {
    let ml = params.ml();
    let w = params.window.min(side.a_border.len());
    let in_range = if side.pre_shoulder {
        w >= 8 && side.anchor_frame >= w + ml
    } else {
        w >= 8 && side.anchor_frame >= ml
    };
    if !in_range {
        return None;
    }
    let (lo, hi) = if side.pre_shoulder {
        let lo = side.anchor_frame - w - ml;
        let hi = (side.anchor_frame + ml).min(side.b_signal.len());
        (lo, hi)
    } else {
        let lo = side.anchor_frame - ml;
        let hi = (side.anchor_frame + w + ml).min(side.b_signal.len());
        (lo, hi)
    };
    if hi <= lo {
        return None;
    }
    let a_win = if side.pre_shoulder {
        &side.a_border[side.a_border.len() - w..]
    } else {
        &side.a_border[..w]
    };
    let curve = lag_correlation_curve_auto(a_win, &side.b_signal[lo..hi], params.max_lag);
    let mut summary = if side.gross_lag_shift == 0 {
        summarize_lag_curve(
            &curve,
            params.sample_rate,
            params.win_ms(),
            params.max_lag_ms(),
            params.channel,
        )
    } else {
        let gross_curve: Vec<(i64, f64)> = curve
            .into_iter()
            .map(|(l, r)| (l + side.gross_lag_shift, r))
            .collect();
        summarize_lag_curve(
            &gross_curve,
            params.sample_rate,
            params.win_ms(),
            params.max_lag_ms(),
            params.channel,
        )
    };
    if let (Some(s), Some(frames)) = (summary.as_mut(), side.border_displacement_frames) {
        s.border_displacement_ms = Some(frames as f64 * 1000.0 / f64::from(params.sample_rate));
    }
    summary
}
/// A shoulder is recoverable — and therefore untouchable by the envelope rescue — once its own
/// fine sweep clears `peak_r ≥ 0.5` (`classify_lag`'s recoverable line, `measure.rs:418`; the
/// nominal-first plan's triage predicate, **not** `LagSummary::is_landmark` — see "The
/// `is_landmark` trap", `docs/dev/archive/TEMP-lag-nominal-first-plan.md`).
pub(in crate::application::gap_fingerprint::measure) fn is_recoverable(s: &LagSummary) -> bool {
    s.peak_r >= LAG_DECISION_RECOVERABLE_R
}
pub(in crate::application::gap_fingerprint::measure) fn stamp_rescue(
    mut s: LagSummary,
    peak: Option<EnvelopePeak>,
    rescue: EnvelopeRescue,
    window_ms: f64,
) -> LagSummary {
    s.envelope_window_ms = Some(window_ms);
    if let Some(p) = peak {
        s.envelope_lag_ms = Some(p.lag_ms);
        s.envelope_peak_r = Some(p.peak_r);
        s.envelope_edge_pinned = Some(p.edge_pinned);
    }
    s.envelope_rescue = Some(rescue);
    s
}
/// Measure → triage → rescue → accept for one shoulder (`docs/dev/archive/TEMP-lag-nominal-first-plan.md`
/// § Change). `nominal` is that shoulder's zero-envelope-shift fine sweep, already run by the
/// caller. Alive shoulders (`is_recoverable`) are returned untouched — the envelope is never
/// consulted, so it can never move them. A dead shoulder is rescued only if the coarse envelope
/// proposes a center (`envelope_recenter_frames`'s guard) *and* the retry there is itself
/// recoverable; otherwise the honest nominal reading is kept with the declined candidate stamped
/// on the `envelope_*` fields.
///
/// `side.anchor_frame` is the unchained envelope center; `side.gross_lag_shift` is the sequential
/// chain (see [`LagSideSweep`]).
///
/// `wide_a_border` lazily supplies a *wider* A-side capture for the envelope peak search only —
/// called only once a shoulder is confirmed dead at nominal, never up front
/// (`docs/dev/archive/TEMP-lag-nominal-first-plan.md` § Change). The narrow nominal capture in `side.a_border`
/// is enough for the fine ±`max_lag` sweep (both here and the retry below); only
/// `envelope_shoulder`'s coarse search needs the wider reach. `None` — env off, no source supplied
/// (e.g. direct test calls), or the source returns nothing usable — falls back to `side.a_border`.
pub(in crate::application::gap_fingerprint::measure) fn rescue_shoulder(
    nominal: Option<LagSummary>,
    envelope: Option<EnvelopeCfg>,
    side: LagSideSweep<'_>,
    params: LagSweepParams,
    wide_a_border: Option<&dyn Fn() -> Option<Vec<f64>>>,
) -> Option<LagSummary> {
    let nominal = nominal?;
    let Some(env_cfg) = envelope else {
        return Some(nominal);
    };
    if is_recoverable(&nominal) {
        return Some(LagSummary {
            envelope_rescue: Some(EnvelopeRescue::NotConsulted),
            ..nominal
        });
    }
    let wide_border = wide_a_border.and_then(|f| f());
    let envelope_side = LagSideSweep {
        a_border: wide_border.as_deref().unwrap_or(side.a_border),
        ..side
    };
    // What the envelope will actually correlate — `envelope_shoulder` clamps the requested window
    // to the trimmed border, and on the `shared_covers_wide` path that border is the shared capture
    // rather than one cut to `wide_need`. Recorded on every path below so a reader never has to
    // infer it from the recipe (which cannot express either adjustment).
    let env_window_ms = (env_cfg.window.min(envelope_side.a_border.len()) as f64) * 1000.0
        / f64::from(params.sample_rate).max(1.0);
    let peak = envelope_shoulder(
        EnvelopeSide {
            a_border: envelope_side.a_border,
            b_signal: envelope_side.b_signal,
            anchor_frame: envelope_side.anchor_frame,
            pre_shoulder: envelope_side.pre_shoulder,
        },
        env_cfg,
        params.sample_rate,
    );
    let env_frames = envelope_recenter_frames(peak, params.sample_rate);
    if env_frames == 0 {
        return Some(stamp_rescue(
            nominal,
            peak,
            EnvelopeRescue::Declined,
            env_window_ms,
        ));
    }
    let rescued = lag_side_sweep(
        LagSideSweep {
            anchor_frame: (side.anchor_frame as i64 + side.gross_lag_shift + env_frames).max(0)
                as usize,
            gross_lag_shift: side.gross_lag_shift + env_frames,
            ..side
        },
        params,
    );
    match rescued {
        Some(r) if is_recoverable(&r) => Some(stamp_rescue(
            r,
            peak,
            EnvelopeRescue::Rescued,
            env_window_ms,
        )),
        _ => Some(stamp_rescue(
            nominal,
            peak,
            EnvelopeRescue::Declined,
            env_window_ms,
        )),
    }
}
/// Inputs for [`lag_pair`] — one A pre/post template pair vs a B haystack, plus the envelope
/// rescue knobs. Same grouping idea as [`LagAtPlacementInput`].
#[derive(Clone, Copy)]
pub(in crate::application::gap_fingerprint::measure) struct LagPairInput<'a> {
    pub(in crate::application::gap_fingerprint::measure) a_pre: &'a [f64],
    pub(in crate::application::gap_fingerprint::measure) a_post: &'a [f64],
    pub(in crate::application::gap_fingerprint::measure) b_signal: &'a [f64],
    pub(in crate::application::gap_fingerprint::measure) start_frame: usize,
    pub(in crate::application::gap_fingerprint::measure) gap_frames: usize,
    pub(in crate::application::gap_fingerprint::measure) params: LagSweepParams,
    pub(in crate::application::gap_fingerprint::measure) displacement: Option<(usize, usize)>,
    pub(in crate::application::gap_fingerprint::measure) envelope: Option<EnvelopeCfg>,
    /// Lazy wide-capture sources for [`rescue_shoulder`]'s envelope search — see its doc comment.
    /// `None` (env off, or a caller that never rescues, e.g. tests) falls back to the narrow
    /// `a_pre`/`a_post` already above.
    pub(in crate::application::gap_fingerprint::measure) pre_wide:
        Option<&'a dyn Fn() -> Option<Vec<f64>>>,
    pub(in crate::application::gap_fingerprint::measure) post_wide:
        Option<&'a dyn Fn() -> Option<Vec<f64>>>,
}
/// Measure → triage → rescue → accept ordering for one shoulder pair
/// (`docs/dev/archive/TEMP-lag-nominal-first-plan.md` § Change). Both shoulders are always swept at their
/// nominal (zero envelope shift) center first; `envelope` gates whether either may be rescued from
/// there. Only a dead shoulder (`peak_r < 0.5` at nominal) ever pays the coarse envelope pass.
pub(in crate::application::gap_fingerprint::measure) fn lag_pair(
    input: &LagPairInput<'_>,
) -> (Option<LagSummary>, Option<LagSummary>) {
    let LagPairInput {
        a_pre,
        a_post,
        b_signal,
        start_frame,
        gap_frames,
        params,
        displacement,
        envelope,
        pre_wide,
        post_wide,
    } = *input;
    // Map coordinates (docs/dev/archive/TEMP-lag-decision-map-coordinates-plan.md § Change, point 1): each
    // template's gap-facing edge sits `D` frames outward of the raw gap edge (walk-off + trim), so
    // the haystack anchor is shifted by that same `D` — pre anchor at `gap_start - D_pre`, post
    // (nominal, pre-chain) at `gap_end + D_post`. A perfectly registered pair then reads
    // `frac_lag_ms ≈ 0` on both shoulders instead of `∓D`.
    let (d_pre, d_post) = displacement.unwrap_or((0, 0));
    let pre_side = LagSideSweep {
        a_border: a_pre,
        b_signal,
        anchor_frame: (start_frame as i64 - d_pre as i64).max(0) as usize,
        pre_shoulder: true,
        gross_lag_shift: 0,
        border_displacement_frames: displacement.map(|(pre, _)| pre),
    };
    let pre_nominal = lag_side_sweep(pre_side, params);
    // Sequential per-shoulder registration (ledger A2): center the post search on
    // `post_nominal + round(L_pre)`, not the naive `post_nominal`, but only when the pre lock
    // clears the findings-§8 landmark bar (`LagSummary::is_landmark`) — a weak or ambiguous pre
    // must not become the prior for the post search (fail open to `post_nominal` otherwise). This
    // chain is always read from the **nominal** pre — never a rescued one
    // (`docs/dev/archive/TEMP-lag-nominal-first-plan.md` § Change, "Landmark chain stays anchored to the
    // nominal pass") — so a rescue on either shoulder can never move the other's search center.
    let post_nominal = start_frame + gap_frames + d_post;
    let pre_shift = pre_nominal
        .filter(LagSummary::is_landmark)
        .map(|p| p.frac_lag_samples.round() as i64)
        .unwrap_or(0);
    let post_base_nominal = (post_nominal as i64 + pre_shift).max(0) as usize;
    let post_nominal_summary = lag_side_sweep(
        LagSideSweep {
            a_border: a_post,
            b_signal,
            anchor_frame: post_base_nominal,
            pre_shoulder: false,
            gross_lag_shift: pre_shift,
            border_displacement_frames: displacement.map(|(_, post)| post),
        },
        params,
    );

    // Rescue post uses the *unchained* `post_nominal` as `anchor_frame` (see [`LagSideSweep`]).
    let pre = rescue_shoulder(pre_nominal, envelope, pre_side, params, pre_wide);
    let post = rescue_shoulder(
        post_nominal_summary,
        envelope,
        LagSideSweep {
            a_border: a_post,
            b_signal,
            anchor_frame: post_nominal,
            pre_shoulder: false,
            gross_lag_shift: pre_shift,
            border_displacement_frames: displacement.map(|(_, post)| post),
        },
        params,
        post_wide,
    );
    (pre, post)
}
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
    let ch = channels.max(1);
    let gap_frames = refined.end_frame.saturating_sub(refined.start_frame);
    let rate = f64::from(sample_rate).max(1.0);
    // §3.6a: uniqueness needs a ~1 s window. Discover that much border (plus the lag-search slack) so the
    // template isn't capped to the ~150 ms `bin_frames*3` used for the structure probe.
    let window = ((cfg.lag_window_secs * rate).round() as usize).max(8);
    let max_lag = ((cfg.lag_max_lag_ms as f64 / 1000.0) * rate).round() as i64;
    // One construction, shared with the patch path (`EnvelopeCfg::from_knobs`). `geometry` is the
    // same numbers on the disabled arm, which `wide_need` below still needs to size a capture.
    let envelope_cfg = EnvelopeCfg::from_knobs(
        cfg.lag_envelope_max_lag_ms,
        cfg.lag_envelope_window_secs,
        sample_rate,
    );
    let env_on = envelope_cfg.is_some();
    let env_geometry = EnvelopeCfg::geometry(
        cfg.lag_envelope_max_lag_ms,
        cfg.lag_envelope_window_secs,
        sample_rate,
    );
    let (env_window, env_lag) = (env_geometry.window, env_geometry.max_lag);
    // Matches the old unconditional formula's reach exactly (`env_window + env_lag`) so a rescued
    // shoulder's envelope search has the same slack it always had — including the walk-off margin
    // `gap_border_frame_range` needs when the near-gap fade runs longer than `env_window` alone.
    let wide_need = env_window + env_lag;
    // Nominal-first (`BorderCapture::Narrow`, the shipped default): the shared capture is sized to
    // the nominal fine sweep's own reach (`window + max_lag`), never `env_window + env_lag`. Widening
    // it unconditionally paid the wider capture's cost — and let its 12%-of-peak trim shift, moving
    // `border_displacement_ms` — on every gap edge, even when no shoulder is ever dead enough to
    // consult the envelope (`docs/dev/archive/TEMP-lag-nominal-first-plan.md` § Change). The wide
    // reach is instead fetched lazily, per shoulder, only inside `rescue_shoulder` — see
    // `pre_wide`/`post_wide` below.
    //
    // `BorderCapture::Wide` restores the pre-split reach as a diagnostic arm. It is gated on `env_on`
    // because that is exactly what the old `if env_on { ... }` guard did: with the envelope off there
    // is no wide search to serve, so the two arms must coincide.
    let narrow_need = window + max_lag.max(0) as usize;
    let border_need = match cfg.border_capture {
        BorderCapture::Wide if env_on => narrow_need.max(wide_need),
        BorderCapture::Wide | BorderCapture::Narrow => narrow_need,
    };
    let border_spec = probe_border_spec(&refined, border_need, cfg);
    let (a_pre, a_post) = border_templates_for_gap(a_samples, ch, &border_spec);
    let (a_pre_ch, a_post_ch) = border_templates_per_channel_for_gap(a_samples, ch, &border_spec);
    // When the shared capture is cut to exactly the envelope's reach, the lazy wide fetches below
    // are pure duplicated work: re-probing the same spec and re-cutting the same templates, up to
    // four times per gap (mono pre/post + selected-channel pre/post). Hand `rescue_shoulder` `None`
    // instead — its documented fallback is `side.a_border`, which then *is* the wide template. At
    // the defaults `Wide` sizes `border_need` to exactly `wide_need`, so it costs one capture, not
    // five.
    //
    // The test is `>=`, not `==`, and a longer capture is deliberately allowed to stand in. It is
    // not the same slice: `gap_border_frame_range` trims silence inward from the gap bounded by
    // `pre_start = gap_start - border_frames`, so a larger `border_frames` survives the trim with
    // more material. That is an advantage, not a hazard — `envelope_shoulder` clamps to
    // `cfg.window.min(a_border.len())`, so the extra length can only help it reach its full window.
    // Measured: at `--lag-envelope-window-secs 0.3 --lag-envelope-max-lag-ms 700` behind a 900 ms
    // silent tail, the shared 1.6 s border rescues at r = 0.996 where the envelope's own 1.0 s reach
    // has one bin and cannot correlate at all.
    //
    // The cost is provenance, not correctness: how much A material the envelope saw then depends on
    // `--border-capture`, which no recipe field describes. `LagSummary::envelope_window_ms` records
    // the template length actually correlated so a reader never has to infer it.
    let shared_covers_wide = border_need >= wide_need;
    let pre_wide_mono = || -> Option<Vec<f64>> {
        let spec = probe_border_spec(&refined, wide_need, cfg);
        let (pre, _post) = border_templates_for_gap(a_samples, ch, &spec);
        (!pre.is_empty()).then_some(pre)
    };
    let post_wide_mono = || -> Option<Vec<f64>> {
        let spec = probe_border_spec(&refined, wide_need, cfg);
        let (_pre, post) = border_templates_for_gap(a_samples, ch, &spec);
        (!post.is_empty()).then_some(post)
    };

    let sweep = LagSweepParams {
        window,
        max_lag,
        sample_rate,
        channel: LagChannel::Mono,
    };
    // Nominal-first: the envelope is only ever consulted by `rescue_shoulder` for a shoulder that
    // is dead at nominal, never up front (`docs/dev/archive/TEMP-lag-nominal-first-plan.md` § Change).
    // `envelope_cfg` is built above, next to the geometry `wide_need` shares.

    let mut out = LagFingerprint::default();
    let mut add = |pre: Option<LagSummary>, post: Option<LagSummary>| {
        if let Some(p) = pre {
            out.pre_anchor.push(p);
        }
        if let Some(p) = post {
            out.post_anchor.push(p);
        }
    };

    // The B anchors below are the mapped gap edges, but these templates start away from them (silence
    // walk-off + trim; standoff is [`PROBE_BORDER_STANDOFF_FRAMES`], not production's 0.35 s). Record
    // that displacement per shoulder so `frac_lag_ms` can be read against a known zero instead of an
    // assumed one — see `LagSummary::border_displacement_ms`.
    let mono_displacement =
        border_edge_displacement_frames(a_samples, ch, &border_spec, a_pre.len(), a_post.len());

    let (pre, post) = lag_pair(&LagPairInput {
        a_pre: &a_pre,
        a_post: &a_post,
        b_signal: b_mono,
        start_frame,
        gap_frames,
        params: sweep,
        displacement: Some(mono_displacement),
        envelope: envelope_cfg,
        pre_wide: (!shared_covers_wide).then_some(&pre_wide_mono as &dyn Fn() -> Option<Vec<f64>>),
        post_wide: (!shared_covers_wide)
            .then_some(&post_wide_mono as &dyn Fn() -> Option<Vec<f64>>),
    });
    add(pre, post);

    if let Some(sel) = selected {
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
            let pre_wide_ch = || -> Option<Vec<f64>> {
                let spec = probe_border_spec(&refined, wide_need, cfg);
                let (pre_ch, _post_ch) = border_templates_per_channel_for_gap(a_samples, ch, &spec);
                pre_ch.get(sel).filter(|v| !v.is_empty()).cloned()
            };
            let post_wide_ch = || -> Option<Vec<f64>> {
                let spec = probe_border_spec(&refined, wide_need, cfg);
                let (_pre_ch, post_ch) = border_templates_per_channel_for_gap(a_samples, ch, &spec);
                post_ch.get(sel).filter(|v| !v.is_empty()).cloned()
            };
            let (pre, post) = lag_pair(&LagPairInput {
                a_pre: &a_pre_ch[sel],
                a_post: &a_post_ch[sel],
                b_signal: &b_ch[sel],
                start_frame,
                gap_frames,
                params: LagSweepParams {
                    channel: LagChannel::Selected(sel),
                    ..sweep
                },
                // The 12 %-of-peak trim runs per buffer, so this channel's displacement is its own.
                displacement: Some(sel_disp),
                envelope: envelope_cfg,
                pre_wide: (!shared_covers_wide)
                    .then_some(&pre_wide_ch as &dyn Fn() -> Option<Vec<f64>>),
                post_wide: (!shared_covers_wide)
                    .then_some(&post_wide_ch as &dyn Fn() -> Option<Vec<f64>>),
            });
            add(pre, post);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::policies::RefinedGapFrames;

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
