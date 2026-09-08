//! Mono Full-tier `lag_decision` construction, shared by the fingerprint dump and the
//! injected-time pre-pass (`docs/dev/TEMP-injected-time-detector-plan.md` § 5).
//!
//! Moved from `application::gap_fingerprint` so production repair can call the same
//! `shoulder_lags_at_b_mapped` the dump uses for mono `lag_decision`, without importing
//! the measure slice or inventing fill-border templates. **Move only for the primitives;
//! the production caller is new.**

use serde::{Deserialize, Serialize};

use crate::domain::lag_envelope::{
    envelope_recenter_frames, envelope_shoulder, EnvelopeCfg, EnvelopePeak, EnvelopeSide,
    LAG_DECISION_RECOVERABLE_R,
};
use crate::domain::policies::{
    border_edge_displacement_frames, border_templates_for_gap, GapBorderSpec, RefinedGapFrames,
};
use crate::domain::seam_local::{
    lag_correlation_curve, lag_correlation_curve_auto, lag_is_edge_pinned,
};

fn finite_corr(x: f64) -> f64 {
    if x.is_finite() {
        x
    } else {
        0.0
    }
}

/// Which signal the lag curve was measured on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LagChannel {
    Mono,
    /// The gate-selected channel index.
    Selected(usize),
}

/// Verdict from a lag curve (see `docs/dev/archive/TEMP-gap-fingerprint-plan.md` §4 thresholds).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LagVerdict {
    /// A shift recovers strong correlation — recoverable timing offset (read `frac_lag_ms`).
    TimingOffset,
    /// No shift recovers correlation — sources genuinely differ.
    Decorrelated,
    /// Partial; weak shared content.
    Ambiguous,
}

/// Parabolic-interpolated peak of a lag-correlation sweep plus a verdict.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LagSummary {
    pub window_ms: u32,
    pub max_lag_ms: u32,
    pub channel: LagChannel,
    /// Correlation at gross lag 0 (original `b_mapped`) — what the seam currently sees.
    ///
    /// Absent when the searched window does not contain lag 0: a far envelope recenter or a
    /// sequential post whose chain shifted the fine window past nominal. Missing is **not** 0.0 —
    /// that sample was not measured. Older corpora always wrote a number (including a fabricated
    /// `0.0` on gross-shifted shoulders).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub lag0_r: Option<f64>,
    /// Best correlation at an integer lag.
    pub peak_r: f64,
    /// **Uniqueness guard:** the highest *competing* local-maximum correlation away from the main peak.
    /// A value near `peak_r` means the match is ambiguous — periodic content (tones, music) peaks at
    /// many lags, so a high `peak_r` may be a false same-source positive. `peak_r − second_peak_r` is
    /// the margin by which the chosen lag wins. `None` for fingerprints written before this field.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub second_peak_r: Option<f64>,
    /// **Robust uniqueness:** z-score of the peak over the whole lag curve, `(peak_r − mean)/std`. Unlike
    /// `second_peak_r` (one rival), this measures how far the peak stands out from the *entire* lag
    /// landscape — the metric the §3.6a experiment found separates real registration from periodic
    /// ambiguity at a 1 s window (ambiguous ≈ 6, unique ≥ 15). `None` when the curve has no spread.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub peak_z: Option<f64>,
    /// `peak_r − second_peak_r` — prominence of the chosen lag over the tallest competing local max.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub prominence: Option<f64>,
    /// `|lag(peak) − lag(second peak)|` in ms — spacing to the tallest rival. A spacing that recurs across
    /// gaps is the content's periodicity period (judge the peak *given* it); scattered ⇒ unique.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub top2_spacing_ms: Option<f64>,
    pub peak_lag_samples: i64,
    /// Parabolic-interpolated (possibly fractional) lag of the peak.
    pub frac_lag_samples: f64,
    pub frac_lag_ms: f64,
    /// **Search-exhausted guard:** the integer peak sits at (or within `LAG_EDGE_TOL_MS` of) the edge of
    /// the searched lag range, so the true optimum may lie *beyond* `±max_lag`. `frac_lag_ms` / `peak_r`
    /// are then a lower bound clipped by the window, not the real registration — GIGO for `splice.step_ms`
    /// (ledger A5/C6). `None` for fingerprints written before this field. Widen the sweep to clear it.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub edge_pinned: Option<bool>,
    /// **Template walk-off (ms), diagnostic only.** This shoulder's A template does not start at the
    /// gap edge: the silence walk-off, the standoff, and the 12 %-of-peak trim pull its gap-facing
    /// edge outward by this much (`border_edge_displacement_frames`; always ≥ 0). The B search window
    /// is shifted by exactly this amount before the sweep runs (map coordinates — see
    /// `docs/dev/archive/TEMP-lag-decision-map-coordinates-plan.md` § Change), so a *perfectly* registered pair
    /// reads `frac_lag_ms ≈ 0` on **both** shoulders; this field is no longer a reader-side correction
    /// to `frac_lag_ms`, only a record of how far the template itself sat from the throat. `None` for
    /// fingerprints written before this field, and on paths that do not build border templates.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub border_displacement_ms: Option<f64>,
    /// **A-template length the envelope actually correlated (ms).** The recipe's
    /// `lag_envelope_window_secs` records what was *asked for*; this records what was *used*, which
    /// is `min(requested_window, trimmed_a_border)`. They part company for two reasons, and a reader
    /// cannot tell which from the recipe alone:
    ///
    /// - the silence walk-off and 12 %-of-peak trim left the shoulder shorter than the window
    ///   (the same mechanism behind [`Self::border_displacement_ms`]);
    /// - the shared A capture already reached the envelope's need, so the rescue reused it instead
    ///   of re-cutting one at exactly `env_window + env_lag`. That capture is sized by
    ///   `--border-capture` and can be *longer*, which survives the trim with more material.
    ///   Measured case: it hands the envelope 1.6 s where the envelope's own reach would have
    ///   yielded 100 ms, turning a one-bin refusal into an r = 0.996 rescue.
    ///
    /// The field does not say *which* of the two applied — reconstructing that needs the border
    /// geometry, not the recipe. What it does give a reader is the quantity that explains a weak or
    /// refused envelope directly: a value below the recipe's window means the template was short,
    /// and one at ~100 ms means a single bin, which cannot correlate at all.
    ///
    /// Same presence rule as [`Self::envelope_lag_ms`]: set whenever the envelope ran (`rescued` /
    /// `declined`), absent on `not_consulted`, when `lag_envelope_max_lag_ms` is 0, and on corpora
    /// written before this field. Unlike `envelope_lag_ms` it is still set when the envelope refused
    /// before correlating, which is exactly when knowing the template length explains the refusal.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub envelope_window_ms: Option<f64>,
    /// Coarse 100 ms-bin envelope peak at `b_mapped` (ms). Present on `rescued` / `declined`
    /// shoulders (the envelope ran). Absent on `not_consulted` (alive at nominal — envelope never
    /// run), when `lag_envelope_max_lag_ms` is 0, and on corpora from before this field.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub envelope_lag_ms: Option<f64>,
    /// Peak Pearson of that coarse envelope. A dead-at-nominal shoulder retries at this peak iff
    /// it is ≥ scan `min_envelope_r` (0.70) **and** [`Self::envelope_edge_pinned`] is false.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub envelope_peak_r: Option<f64>,
    /// The coarse envelope peak sits on the last bin of ±`lag_envelope_max_lag_ms` — fail open
    /// to `b_mapped` for this shoulder (saturated window, not a 31/g7-specific guard).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub envelope_edge_pinned: Option<bool>,
    /// Which branch of the nominal-first measure → triage → rescue → accept ordering produced
    /// this summary (`docs/dev/archive/TEMP-lag-nominal-first-plan.md` § Change). `None` only when the
    /// envelope mechanism is off (`lag_envelope_max_lag_ms == 0`). Unlike `envelope_lag_ms`, this
    /// is still `Some` (`NotConsulted`) when the mechanism is on but the shoulder was already
    /// recoverable at nominal, since the envelope was never run to produce a peak.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub envelope_rescue: Option<EnvelopeRescue>,
    pub verdict: LagVerdict,
}

/// Which branch of the nominal-first rescue ordering a shoulder's [`LagSummary`] came from. See
/// [`LagSummary::envelope_rescue`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnvelopeRescue {
    /// Recoverable at nominal (`peak_r` ≥ 0.5) — the envelope was never consulted; this shoulder
    /// cannot be moved by it.
    NotConsulted,
    /// Non-recoverable at nominal; the coarse envelope proposed a center and the retry there was
    /// itself recoverable, so the retry replaced the nominal reading.
    Rescued,
    /// Non-recoverable at nominal; either the envelope guard did not propose a center (`peak_r` <
    /// 0.70 or edge-pinned) or the retry was still non-recoverable. The honest nominal reading
    /// was kept.
    Declined,
}

/// How wide the **shared** A border capture behind `lag_decision` was cut.
///
/// This is not "how much context the envelope gets" — that is `lag_envelope_max_lag_ms`, which
/// already sizes the per-shoulder lazy wide capture. This knob sizes the capture that *every*
/// shoulder's template is trimmed out of, so it also moves `border_displacement_ms` (the silence
/// walk-off + 12 %-of-peak trim run over the capture, `border_edge_displacement_frames`) and with
/// it the zero that `frac_lag_ms` is read against. Changing it re-baselines the whole corpus, not
/// just the shoulders that reach the envelope: a 505-shoulder A/B moved 161 displacements by >1 ms
/// while flipping exactly one verdict. Treat a cross-arm comparison of `frac_lag_ms` as invalid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BorderCapture {
    /// Capture sized to the nominal fine sweep's own reach (`lag_window_secs + lag_max_lag_ms`).
    /// The shipped default since the 2026-08-20 nominal-first split: a shoulder recoverable at
    /// nominal never pays for reach it does not use, and the wide reach is fetched lazily per
    /// shoulder inside the rescue branch.
    #[default]
    Narrow,
    /// Capture widened to the envelope's reach as well
    /// (`max(lag_window_secs + lag_max_lag_ms, 2 s + lag_envelope_max_lag_ms)`), matching the
    /// pre-split behaviour. `lag_envelope_max_lag_ms = 0` (envelope off) leaves it narrow, exactly
    /// as the old `if env_on` guard did.
    ///
    /// Diagnostic arm. Its one known behavioural edge over `Narrow` is a shoulder that is dead at
    /// nominal *and* needs a far rescue, where the narrow capture's trim can cost the true lock
    /// (31/g8 pre: Narrow envelope +800 ms / `D` 13 ms; Wide keeps −1146.1 / `D` 1984 ms).
    Wide,
}

/// Peak correlation floor for [`LagSummary::is_landmark`] — findings §8's "unique" bar
/// (`docs/dev/archive/TEMP-31g7-model-vs-findings.md` § 8: "one unique (`peak_r ≥ 0.8`, `peak_z ≥ 12`)").
pub const LAG_LANDMARK_MIN_PEAK_R: f64 = 0.8;
/// Peak z-score floor for [`LagSummary::is_landmark`] — see [`LAG_LANDMARK_MIN_PEAK_R`].
pub const LAG_LANDMARK_MIN_PEAK_Z: f64 = 12.0;

impl LagSummary {
    /// Is this shoulder's lock confident enough to chain the *other* shoulder's search off it?
    ///
    /// Findings §8's bar, ported from the Python census prototype
    /// (`docs/dev/archive/TEMP-lag-decision-map-coordinates-plan.md` § Change, point 3): a weak or ambiguous
    /// pre lock must not become the prior for the post search — that is how 31/g10's post reached a
    /// false −1152 ms under a 600 ms max lag by chaining off a `peak_r` 0.158 pre. `peak_z` absent
    /// (curve had no spread) counts as not a landmark, not an exemption.
    pub fn is_landmark(&self) -> bool {
        self.peak_r >= LAG_LANDMARK_MIN_PEAK_R
            && self.peak_z.is_some_and(|z| z >= LAG_LANDMARK_MIN_PEAK_Z)
    }
}

/// Search knobs for a per-shoulder lag correlation sweep (`lag_side_sweep` / [`lag_pair`]).
#[derive(Debug, Clone, Copy)]
pub(crate) struct LagSweepParams {
    pub(crate) window: usize,
    pub(crate) max_lag: i64,
    pub(crate) sample_rate: u32,
    pub(crate) channel: LagChannel,
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
pub(crate) struct LagSideSweep<'a> {
    pub(crate) a_border: &'a [f64],
    pub(crate) b_signal: &'a [f64],
    pub(crate) anchor_frame: usize,
    /// `true` for the pre shoulder (last `window` samples of `a_border`); `false` for post (first `window`).
    pub(crate) pre_shoulder: bool,
    /// Samples added to each lag bin in the returned summary (sequential post: gross map to `b_mapped_end`).
    pub(crate) gross_lag_shift: i64,
    /// This shoulder's template-edge displacement from the gap edge (`border_edge_displacement_frames`),
    /// recorded as `LagSummary::border_displacement_ms`. `None` where the caller builds no border
    /// templates (seam-local / dual-fit windows), which is what an absent field then means.
    pub(crate) border_displacement_frames: Option<usize>,
}
/// Prominence of a seam's placement peak over its tallest rival within ±`max_lag`. `b_ctx` must be laid
/// out so lag 0 aligns `a_win` at the placement (`b_ctx[max_lag .. max_lag + a_win.len()]`). Low prominence
/// ⇒ the seam matches at many lags (periodic) ⇒ the dual-fit placement is not a unique registration.
pub(crate) fn seam_prominence(
    a_win: &[f64],
    b_ctx: &[f64],
    max_lag: i64,
    sample_rate: u32,
) -> Option<f64> {
    let curve = lag_correlation_curve(a_win, b_ctx, max_lag);
    summarize_lag_curve(&curve, sample_rate, 0, 0, LagChannel::Mono).and_then(|s| s.prominence)
}
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
pub(crate) fn secondary_peak(curve: &[(i64, f64)], peak_index: usize) -> Option<(i64, f64)> {
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
pub(crate) fn classify_lag(lag0_r: Option<f64>, peak: f64, peak_lag: i64) -> LagVerdict {
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
pub(crate) fn lag_side_sweep(side: LagSideSweep<'_>, params: LagSweepParams) -> Option<LagSummary> {
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
pub(crate) fn is_recoverable(s: &LagSummary) -> bool {
    s.peak_r >= LAG_DECISION_RECOVERABLE_R
}
pub(crate) fn stamp_rescue(
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
pub(crate) fn rescue_shoulder(
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
pub(crate) struct LagPairInput<'a> {
    pub(crate) a_pre: &'a [f64],
    pub(crate) a_post: &'a [f64],
    pub(crate) b_signal: &'a [f64],
    pub(crate) start_frame: usize,
    pub(crate) gap_frames: usize,
    pub(crate) params: LagSweepParams,
    pub(crate) displacement: Option<(usize, usize)>,
    pub(crate) envelope: Option<EnvelopeCfg>,
    /// Lazy wide-capture sources for [`rescue_shoulder`]'s envelope search — see its doc comment.
    /// `None` (env off, or a caller that never rescues, e.g. tests) falls back to the narrow
    /// `a_pre`/`a_post` already above.
    pub(crate) pre_wide: Option<&'a dyn Fn() -> Option<Vec<f64>>>,
    pub(crate) post_wide: Option<&'a dyn Fn() -> Option<Vec<f64>>>,
}
/// Measure → triage → rescue → accept ordering for one shoulder pair
/// (`docs/dev/archive/TEMP-lag-nominal-first-plan.md` § Change). Both shoulders are always swept at their
/// nominal (zero envelope shift) center first; `envelope` gates whether either may be rescued from
/// there. Only a dead shoulder (`peak_r < 0.5` at nominal) ever pays the coarse envelope pass.
pub(crate) fn lag_pair(input: &LagPairInput<'_>) -> (Option<LagSummary>, Option<LagSummary>) {
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

/// Fine-sweep window used by Full-tier `lag_decision` and the injected-time pre-pass.
/// `FingerprintConfig::from_request` hardcodes this; it is not a CLI/config key.
pub const LAG_DECISION_WINDOW_SECS: f64 = 1.0;
/// Fine-sweep half-width (ms) used by Full-tier `lag_decision` and the injected-time pre-pass.
pub const LAG_DECISION_MAX_LAG_MS: u32 = 600;

/// Border standoff applied to every probe `GapBorderSpec` — deliberately 0, and deliberately
/// **not** `RepairConfig.border_standoff_secs` (0.35). Production's fill standoff would shift
/// every `frac_lag_ms` and invalidate the injected-time (T, N) pin.
///
/// Moved here from fingerprint `measure/pcm.rs` so that crate is not the production source of
/// truth (`docs/dev/TEMP-injected-time-detector-plan.md` § 5).
pub const PROBE_BORDER_STANDOFF_FRAMES: usize = 0;

/// Seconds form of [`PROBE_BORDER_STANDOFF_FRAMES`] at the rate the probe applied it.
pub fn probe_border_standoff_secs(sample_rate: u32) -> f64 {
    PROBE_BORDER_STANDOFF_FRAMES as f64 / f64::from(sample_rate).max(1.0)
}

/// Probe (registration) borders, not production fill borders. Standoff is always
/// [`PROBE_BORDER_STANDOFF_FRAMES`].
pub fn probe_lag_border_spec(
    refined: &RefinedGapFrames,
    border_frames: usize,
    silence_peak_fraction: f32,
    absolute_silence_rms: f32,
) -> GapBorderSpec {
    GapBorderSpec {
        gap_start_frame: refined.start_frame,
        gap_end_frame: refined.end_frame,
        border_frames,
        border_standoff_frames: PROBE_BORDER_STANDOFF_FRAMES,
        silence_peak_fraction,
        absolute_rms_floor: absolute_silence_rms,
    }
}

/// B-frame index in the decoded B haystack for the gross A→B map at `a_start_frame`.
///
/// `b_extract_start_secs` is 0 when the haystack is the full decoded B track.
pub fn b_mapped_frame_in_haystack(
    a_start_frame: usize,
    sample_rate: f64,
    gap_offset_secs: f64,
    b_extract_start_secs: f64,
) -> usize {
    (((a_start_frame as f64 / sample_rate + gap_offset_secs - b_extract_start_secs) * sample_rate)
        .round()
        .max(0.0)) as usize
}

/// Search knobs for [`shoulder_lags_at_b_mapped`]. Production always uses
/// [`ShoulderLagKnobs::production`]; the fingerprint wrapper copies `FingerprintConfig`.
#[derive(Debug, Clone, Copy)]
pub struct ShoulderLagKnobs {
    pub sample_rate: u32,
    pub lag_window_secs: f64,
    pub lag_max_lag_ms: u32,
    pub lag_envelope_max_lag_ms: u32,
    pub lag_envelope_window_secs: f64,
    pub border_capture: BorderCapture,
    pub silence_peak_fraction: f32,
    pub absolute_silence_rms: f32,
}

impl ShoulderLagKnobs {
    /// Production pre-pass: Full-tier fine sweep (1 s / 600 ms), Narrow capture, envelope from
    /// the shared request knobs. Never reads `--border-capture`.
    pub fn production(
        sample_rate: u32,
        silence_peak_fraction: f32,
        absolute_silence_rms: f32,
        lag_envelope_max_lag_ms: u32,
        lag_envelope_window_secs: f64,
    ) -> Self {
        Self {
            sample_rate,
            lag_window_secs: LAG_DECISION_WINDOW_SECS,
            lag_max_lag_ms: LAG_DECISION_MAX_LAG_MS,
            lag_envelope_max_lag_ms,
            lag_envelope_window_secs,
            border_capture: BorderCapture::Narrow,
            silence_peak_fraction,
            absolute_silence_rms,
        }
    }
}

/// Window / envelope geometry shared by the mono caller and the fingerprint selected-channel wrapper.
#[derive(Debug, Clone, Copy)]
pub struct LagDecisionGeometry {
    pub window: usize,
    pub max_lag: i64,
    pub envelope_cfg: Option<EnvelopeCfg>,
    pub wide_need: usize,
    pub border_need: usize,
    pub shared_covers_wide: bool,
}

pub fn lag_decision_geometry(knobs: &ShoulderLagKnobs) -> LagDecisionGeometry {
    let rate = f64::from(knobs.sample_rate).max(1.0);
    let window = ((knobs.lag_window_secs * rate).round() as usize).max(8);
    let max_lag = ((knobs.lag_max_lag_ms as f64 / 1000.0) * rate).round() as i64;
    let envelope_cfg = EnvelopeCfg::from_knobs(
        knobs.lag_envelope_max_lag_ms,
        knobs.lag_envelope_window_secs,
        knobs.sample_rate,
    );
    let env_on = envelope_cfg.is_some();
    let env_geometry = EnvelopeCfg::geometry(
        knobs.lag_envelope_max_lag_ms,
        knobs.lag_envelope_window_secs,
        knobs.sample_rate,
    );
    let wide_need = env_geometry.window + env_geometry.max_lag;
    let narrow_need = window + max_lag.max(0) as usize;
    let border_need = match knobs.border_capture {
        BorderCapture::Wide if env_on => narrow_need.max(wide_need),
        BorderCapture::Wide | BorderCapture::Narrow => narrow_need,
    };
    LagDecisionGeometry {
        window,
        max_lag,
        envelope_cfg,
        wide_need,
        border_need,
        shared_covers_wide: border_need >= wide_need,
    }
}

/// Mono half of Full-tier `lag_at_placement`: probe borders at standoff 0, then [`lag_pair`].
pub fn shoulder_lags_at_b_mapped(
    a_samples: &[f32],
    channels: usize,
    refined: &RefinedGapFrames,
    b_mono: &[f64],
    start_frame: usize,
    knobs: &ShoulderLagKnobs,
) -> (Option<LagSummary>, Option<LagSummary>) {
    let ch = channels.max(1);
    let gap_frames = refined.end_frame.saturating_sub(refined.start_frame);
    let geo = lag_decision_geometry(knobs);
    let border_spec = probe_lag_border_spec(
        refined,
        geo.border_need,
        knobs.silence_peak_fraction,
        knobs.absolute_silence_rms,
    );
    let (a_pre, a_post) = border_templates_for_gap(a_samples, ch, &border_spec);
    let pre_wide_mono = || -> Option<Vec<f64>> {
        let spec = probe_lag_border_spec(
            refined,
            geo.wide_need,
            knobs.silence_peak_fraction,
            knobs.absolute_silence_rms,
        );
        let (pre, _post) = border_templates_for_gap(a_samples, ch, &spec);
        (!pre.is_empty()).then_some(pre)
    };
    let post_wide_mono = || -> Option<Vec<f64>> {
        let spec = probe_lag_border_spec(
            refined,
            geo.wide_need,
            knobs.silence_peak_fraction,
            knobs.absolute_silence_rms,
        );
        let (_pre, post) = border_templates_for_gap(a_samples, ch, &spec);
        (!post.is_empty()).then_some(post)
    };
    let sweep = LagSweepParams {
        window: geo.window,
        max_lag: geo.max_lag,
        sample_rate: knobs.sample_rate,
        channel: LagChannel::Mono,
    };
    let mono_displacement =
        border_edge_displacement_frames(a_samples, ch, &border_spec, a_pre.len(), a_post.len());
    lag_pair(&LagPairInput {
        a_pre: &a_pre,
        a_post: &a_post,
        b_signal: b_mono,
        start_frame,
        gap_frames,
        params: sweep,
        displacement: Some(mono_displacement),
        envelope: geo.envelope_cfg,
        pre_wide: (!geo.shared_covers_wide)
            .then_some(&pre_wide_mono as &dyn Fn() -> Option<Vec<f64>>),
        post_wide: (!geo.shared_covers_wide)
            .then_some(&post_wide_mono as &dyn Fn() -> Option<Vec<f64>>),
    })
}
