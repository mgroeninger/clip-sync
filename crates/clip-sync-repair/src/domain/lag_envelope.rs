//! Coarse RMS-envelope registration — the "where is this shoulder *really*?" primitive behind the
//! nominal-first rescue ordering (`docs/dev/archive/TEMP-lag-nominal-first-plan.md`).
//!
//! A fine waveform sweep only sees what is inside its own `max_lag`. When a shoulder's true lag is
//! outside that window the sweep reads noise, and no amount of re-searching in-window recovers it.
//! The coarse envelope — 100 ms RMS bins over a wide window — is cheap enough to search much
//! further, and precise enough to *recenter* the fine sweep so its narrow window lands on the right
//! place. That is the whole mechanism: coarse to find, fine to measure.
//!
//! Moved here from `application::gap_fingerprint::measure::lag` (which re-exports every item, so
//! existing paths and tests are unchanged) so that the production patch path and the fingerprint
//! diagnostic share one implementation and can never drift — the same reason
//! [`crate::domain::seam_local`] exists. **Move only: no behavior change.**

use crate::domain::gap_equivalence::DonorRegistrationParams;
use crate::domain::seam_local::lag_correlation_curve;

/// A shoulder is recoverable — and therefore untouchable by the envelope rescue — once its own fine
/// sweep clears `peak_r ≥ 0.5` (`classify_lag`'s recoverable line; the nominal-first plan's triage
/// predicate, **not** `LagSummary::is_landmark` — see "The `is_landmark` trap",
/// `docs/dev/archive/TEMP-lag-nominal-first-plan.md`).
///
/// One number, two callers: the fingerprint's `is_recoverable` and the patch path's seam-lock
/// triage.
pub const SEAM_LOCK_RECOVERABLE_R: f64 = 0.5;

/// Non-finite → 0.0 (a silent gap can cancel to `inf`/`NaN`).
fn finite_corr(x: f64) -> f64 {
    if x.is_finite() {
        x
    } else {
        0.0
    }
}

/// Coarse 100 ms-bin envelope peak at one shoulder's `b_mapped` (before the fine sweep).
#[derive(Clone, Copy, Debug)]
pub struct EnvelopePeak {
    pub lag_ms: f64,
    pub peak_r: f64,
    pub edge_pinned: bool,
}

/// Geometry of the coarse search: how much A to correlate (`window`), how far to look
/// (`max_lag`), and the bin size the envelope is built at (`bin`) — all in frames.
#[derive(Clone, Copy, Debug)]
pub struct EnvelopeCfg {
    pub window: usize,
    pub max_lag: usize,
    pub bin: usize,
}

/// One shoulder's inputs to the coarse search.
///
/// Deliberately narrower than the fingerprint's `LagSideSweep`, which carries two further fields
/// (`gross_lag_shift`, `border_displacement_frames`) that belong to the *fine* sweep and its wire
/// schema — the envelope never reads them. `lag.rs` builds this from its `LagSideSweep` at the one
/// call site.
pub struct EnvelopeSide<'a> {
    pub a_border: &'a [f64],
    pub b_signal: &'a [f64],
    pub anchor_frame: usize,
    /// `true` for the pre shoulder (last `window` samples of `a_border`); `false` for post (first
    /// `window`).
    pub pre_shoulder: bool,
}

/// `~10 ms`-bin RMS envelope — the encoding-/small-shift-robust representation for the seam probe.
pub fn fine_rms_envelope(x: &[f64], bin: usize) -> Vec<f64> {
    let bin = bin.max(1);
    x.chunks(bin)
        .map(|c| (c.iter().map(|v| v * v).sum::<f64>() / c.len() as f64).sqrt())
        .collect()
}

/// Correlate A's envelope against B's over `±max_lag_samples`, in `env_bin`-sized bins.
///
/// `edge_pinned` marks a peak sitting on the first or last searched bin: the true peak may lie
/// outside the searched window, so the reading is saturated rather than located.
pub fn envelope_peak_at(
    a_win: &[f64],
    b_ctx: &[f64],
    sample_rate: u32,
    env_bin: usize,
    max_lag_samples: usize,
) -> Option<EnvelopePeak> {
    if a_win.len() < env_bin.max(8) {
        return None;
    }
    let rate = f64::from(sample_rate).max(1.0);
    let ea = fine_rms_envelope(a_win, env_bin);
    let eb = fine_rms_envelope(b_ctx, env_bin);
    if ea.len() < 3 {
        return None;
    }
    let env_max_lag = (max_lag_samples / env_bin.max(1)) as i64;
    if env_max_lag < 1 {
        return None;
    }
    let curve = lag_correlation_curve(&ea, &eb, env_max_lag);
    if curve.is_empty() {
        return None;
    }
    let &(peak_lag_bin, peak_r) = curve
        .iter()
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))?;
    let lo = curve.first().map(|(l, _)| *l).unwrap_or(peak_lag_bin);
    let hi = curve.last().map(|(l, _)| *l).unwrap_or(peak_lag_bin);
    // ≥1 bin inside the searched coarse window; a peak on the last searched bin is saturated.
    let edge_pinned = peak_lag_bin == lo || peak_lag_bin == hi;
    Some(EnvelopePeak {
        lag_ms: peak_lag_bin as f64 * env_bin as f64 * 1000.0 / rate,
        peak_r: finite_corr(peak_r),
        edge_pinned,
    })
}

/// Coarse envelope peak for one shoulder, slicing A's template and B's haystack from `side`.
///
/// The requested `cfg.window` is clamped to the border actually available, so a caller that could
/// not supply the full width still gets an honest (narrower) reading rather than `None`.
pub fn envelope_shoulder(
    side: EnvelopeSide<'_>,
    cfg: EnvelopeCfg,
    sample_rate: u32,
) -> Option<EnvelopePeak> {
    let w = cfg.window.min(side.a_border.len());
    if w < cfg.bin.max(8) {
        return None;
    }
    let max_lag = cfg.max_lag;
    let (a_win, lo, hi) = if side.pre_shoulder {
        if side.anchor_frame < w + max_lag {
            return None;
        }
        let lo = side.anchor_frame - w - max_lag;
        let hi = (side.anchor_frame + max_lag).min(side.b_signal.len());
        (&side.a_border[side.a_border.len() - w..], lo, hi)
    } else {
        if side.anchor_frame < max_lag {
            return None;
        }
        let lo = side.anchor_frame.saturating_sub(max_lag);
        let hi = (side.anchor_frame + w + max_lag).min(side.b_signal.len());
        (&side.a_border[..w], lo, hi)
    };
    if hi <= lo {
        return None;
    }
    envelope_peak_at(a_win, &side.b_signal[lo..hi], sample_rate, cfg.bin, max_lag)
}

/// Frames to recenter the fine sweep by, or `0` for "the envelope proposes nothing usable".
///
/// The two guards are what keep a coarse peak from moving a fine sweep onto a worse place: the peak
/// must clear `min_envelope_r`, and it must not be `edge_pinned` (a saturated reading locates
/// nothing).
pub fn envelope_recenter_frames(peak: Option<EnvelopePeak>, sample_rate: u32) -> i64 {
    let Some(p) = peak else {
        return 0;
    };
    let min_r = DonorRegistrationParams::default().min_envelope_r;
    if p.peak_r >= min_r && !p.edge_pinned {
        (p.lag_ms / 1000.0 * f64::from(sample_rate)).round() as i64
    } else {
        0
    }
}
