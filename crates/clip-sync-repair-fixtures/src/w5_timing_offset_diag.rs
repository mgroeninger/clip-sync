//! W5 **timing-offset** recoverability diagnostic (Phase C).
//! See `docs/archive/TEMP-w5-timing-offset-diag-plan.md` §5 Phase C.
//!
//! The g003 class is a seam that is dead at lag 0 yet recovers under a multi-ms shift (verdict
//! `timing_offset`). This module measures that recovery directly — pre/post seam [`LagSummary`] via the
//! production fingerprint lag API ([`lag_correlation_curve`] / [`summarize_lag_curve`]) — and sweeps it
//! over `(seam_offset_ms, drift_ppm)` to map **where recoverability breaks**: the gross offset leaving
//! the bounded lag search, or the within-window drift smearing the recovered peak below the
//! `peak ≥ 0.5` floor. Shared by the Phase B self-validation test and the `diag_w5_timing_offset`
//! binary so both characterize the fixture against the same discriminator.

use clip_sync_repair::application::gap_fingerprint::{
    lag_correlation_curve, summarize_lag_curve, LagChannel, LagSummary, LagVerdict,
};
use crate::energy_signature_fixtures::{
    build_w5_timing_offset_seam, EnergySignatureFixture,
};

/// Default sample rate for the timing-offset fixture sweep.
pub const SR: u32 = 48_000;
/// Collar flanking the gap (must exceed `window + 2·max_lag` so the shifted probe stays on collar).
pub const COLLAR_SECS: f64 = 0.3;
/// Anchor-burst offset (envelope only; matches the g003 geometry shape).
pub const PEAK_OFFSET_SECS: f64 = 1.0;

/// Human string for a lag verdict (matches the fingerprint JSON tags).
pub fn verdict_str(v: LagVerdict) -> &'static str {
    match v {
        LagVerdict::TimingOffset => "timing_offset",
        LagVerdict::Decorrelated => "decorrelated",
        LagVerdict::Ambiguous => "ambiguous",
    }
}

/// Pre/post seam lag summaries for one timing-offset fixture.
#[derive(Debug, Clone, Copy)]
pub struct W5SeamLag {
    pub pre: LagSummary,
    pub post: LagSummary,
}

fn to_f64(samples: &[f32]) -> Vec<f64> {
    samples.iter().map(|&s| f64::from(s)).collect()
}

/// Lag summary for an A window `[a_win_start, a_win_start + w)` vs B, swept over ±`max_lag`. Mirrors
/// [`lag_correlation_curve`]'s convention: `b_ctx` starts `max_lag` before the A window so curve lag
/// `L` compares `a[a_win_start + k]` with `b[a_win_start + L + k]` (positive `L` ⇒ B delayed). `None`
/// if the windows fall outside the buffers.
fn seam_lag(a: &[f64], b: &[f64], a_win_start: usize, w: usize, max_lag: i64) -> Option<LagSummary> {
    let ml = max_lag.max(0) as usize;
    if a_win_start < ml || a_win_start + w + ml > a.len() || a_win_start + w + ml > b.len() {
        return None;
    }
    let a_win = &a[a_win_start..a_win_start + w];
    let b_ctx = &b[a_win_start - ml..a_win_start + w + ml];
    let curve = lag_correlation_curve(a_win, b_ctx, max_lag);
    let win_ms = (w as f64 * 1000.0 / f64::from(SR)) as u32;
    let max_lag_ms = (ml as f64 * 1000.0 / f64::from(SR)) as u32;
    summarize_lag_curve(&curve, SR, win_ms, max_lag_ms, LagChannel::Mono)
}

/// Compute pre/post seam lag for a timing-offset fixture. Windows sit inside the collar with a
/// `max_lag` guard so the shifted B window never crosses into the throat fill. The caller must build
/// the fixture with `collar_secs ≥ window_secs + 2·max_lag_secs`. `None` if a window is out of range.
pub fn w5_timing_offset_seam_lag(
    fixture: &EnergySignatureFixture,
    window_secs: f64,
    max_lag_secs: f64,
) -> Option<W5SeamLag> {
    let a = to_f64(&fixture.a_samples);
    let b = to_f64(&fixture.b_samples);
    let w = (window_secs * f64::from(SR)) as usize;
    let max_lag = (max_lag_secs * f64::from(SR)) as i64;
    let guard = max_lag.max(0) as usize;
    let pre = seam_lag(&a, &b, fixture.gap_start.checked_sub(guard + w)?, w, max_lag)?;
    let post = seam_lag(&a, &b, fixture.gap_end + guard, w, max_lag)?;
    Some(W5SeamLag { pre, post })
}

/// One recoverability-sweep cell: the fixture parameters plus the measured pre/post seam lag.
#[derive(Debug, Clone, Copy)]
pub struct W5TimingOffsetCell {
    pub seam_offset_ms: f64,
    pub drift_ppm: f64,
    pub lag: Option<W5SeamLag>,
}

impl W5TimingOffsetCell {
    /// Recoverable when **both** seams read `timing_offset` (the gate could recover the seam with a
    /// bounded shift). `false` when either degrades to `ambiguous`/`decorrelated` — offset beyond the
    /// search range, or drift smearing the peak below the 0.5 floor.
    pub fn recoverable(&self) -> bool {
        self.lag.is_some_and(|l| {
            l.pre.verdict == LagVerdict::TimingOffset && l.post.verdict == LagVerdict::TimingOffset
        })
    }
}

/// Window / max-lag the sweep probes with. Kept fixed so the only axes are `(offset, drift)`.
pub const SWEEP_WINDOW_SECS: f64 = 0.08;
pub const SWEEP_MAX_LAG_SECS: f64 = 0.05; // ±50 ms brackets the swept offsets

/// Evaluate one `(seam_offset_ms, drift_ppm)` cell: build the fixture, measure pre/post seam lag.
pub fn evaluate_w5_timing_offset_cell(seam_offset_ms: f64, drift_ppm: f64) -> W5TimingOffsetCell {
    let fixture = build_w5_timing_offset_seam(
        SR,
        1,
        PEAK_OFFSET_SECS,
        COLLAR_SECS,
        seam_offset_ms,
        drift_ppm,
    );
    let lag = w5_timing_offset_seam_lag(&fixture, SWEEP_WINDOW_SECS, SWEEP_MAX_LAG_SECS);
    W5TimingOffsetCell {
        seam_offset_ms,
        drift_ppm,
        lag,
    }
}

/// Cartesian sweep over offsets × drifts (recoverability map).
pub fn w5_timing_offset_grid(offsets_ms: &[f64], drifts_ppm: &[f64]) -> Vec<W5TimingOffsetCell> {
    let mut out = Vec::with_capacity(offsets_ms.len() * drifts_ppm.len());
    for &off in offsets_ms {
        for &drift in drifts_ppm {
            out.push(evaluate_w5_timing_offset_cell(off, drift));
        }
    }
    out
}

/// Default sweep axes: `seam_offset_ms` spanning g003's ~16 ms out to beyond the 50 ms search range,
/// `drift_ppm` (negative = g003 direction, `|pre| > |post|`) from none to a peak-smearing skew.
pub fn w5_timing_offset_grid_default() -> Vec<W5TimingOffsetCell> {
    let offsets = [2.0, 4.0, 8.0, 12.0, 16.0, 24.0, 32.0, 48.0];
    let drifts = [0.0, -1_500.0, -3_000.0, -4_500.0, -9_000.0, -18_000.0, -36_000.0];
    w5_timing_offset_grid(&offsets, &drifts)
}

const CSV_HEADER: &str = "seam_offset_ms,drift_ppm,pre_lag0_r,pre_peak_r,pre_frac_lag_ms,pre_verdict,\
post_lag0_r,post_peak_r,post_frac_lag_ms,post_verdict,recoverable";

fn lag_cols(l: Option<&LagSummary>) -> String {
    match l {
        Some(s) => format!(
            "{:.4},{:.4},{:.3},{}",
            s.lag0_r,
            s.peak_r,
            s.frac_lag_ms,
            verdict_str(s.verdict)
        ),
        None => ",,,".to_string(),
    }
}

/// CSV for a recoverability sweep, one row per cell.
pub fn w5_timing_offset_csv(cells: &[W5TimingOffsetCell]) -> String {
    let mut out = String::from(CSV_HEADER);
    out.push('\n');
    for c in cells {
        let (pre, post) = c
            .lag
            .map(|l| (Some(l.pre), Some(l.post)))
            .unwrap_or((None, None));
        out.push_str(&format!(
            "{:.1},{:.0},{},{},{}\n",
            c.seam_offset_ms,
            c.drift_ppm,
            lag_cols(pre.as_ref()),
            lag_cols(post.as_ref()),
            c.recoverable(),
        ));
    }
    out
}
