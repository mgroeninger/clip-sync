//! Shared seam-local lag search — the per-shoulder registration primitive used by **both** the diagnostic
//! `splice_dualfit` (fingerprint scan) and the production dual-fit repair (A3). One implementation so the
//! two can never drift — that divergence is exactly the bug class we fixed twice (seam-local vs 1 s-gross
//! placement; ledger A3).

use clip_sync::normalized_correlation;

/// Non-finite → 0.0 (a silent gap can cancel to `inf`/`NaN`).
fn finite(x: f64) -> f64 {
    if x.is_finite() {
        x
    } else {
        0.0
    }
}

/// Lag-correlation curve: normalized (Pearson) correlation of `a` against `b_ctx` at each integer lag in
/// `[-max_lag, max_lag]`. `base = max_lag + lag`, window `b_ctx[base..base+n]`; lags whose window runs past
/// `b_ctx` are skipped (edge mask), so `curve.len()` can be `< 2·max_lag + 1`.
pub fn lag_correlation_curve(a: &[f64], b_ctx: &[f64], max_lag: i64) -> Vec<(i64, f64)> {
    let n = a.len();
    if n == 0 || max_lag < 0 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity((2 * max_lag + 1) as usize);
    for lag in -max_lag..=max_lag {
        let base = max_lag + lag;
        if base < 0 {
            continue;
        }
        let base = base as usize;
        if base + n > b_ctx.len() {
            continue;
        }
        out.push((lag, normalized_correlation(a, &b_ctx[base..base + n])));
    }
    out
}

/// Best `(peak_r, peak_lag_frames, peak_z)` of one seam over a ±`max_lag` search around `anchor_start` (the
/// start of the seam's B window; `lag 0` aligns `a_seam` at `anchor_start`). `max_lag` auto-clamps to the
/// available B context (0 ⇒ a plain lag-0 score). `peak_z` = the peak's whole-curve z-score (the
/// periodicity/alias signal). `None` when the seam is too short or the window is out of range.
///
/// The peak / z extraction is identical to `summarize_lag_curve`'s (argmax by `r` with last-max tie-break;
/// `z = (peak − mean)/std` over the curve), so callers that migrate from that path see the same numbers.
pub fn seam_local_peak(
    a_seam: &[f64],
    b_mono: &[f64],
    anchor_start: usize,
    max_lag: usize,
) -> Option<(f64, i64, Option<f64>)> {
    let n = a_seam.len();
    if n < 8 || anchor_start + n > b_mono.len() {
        return None;
    }
    let max_lag = max_lag.min(anchor_start).min(b_mono.len() - (anchor_start + n));
    let b_ctx = &b_mono[anchor_start - max_lag..anchor_start + n + max_lag];
    let curve = lag_correlation_curve(a_seam, b_ctx, max_lag as i64);
    let &(peak_lag, peak_r) = curve
        .iter()
        .max_by(|(_, x), (_, y)| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal))?;
    let m = (curve.len() as f64).max(1.0);
    let mean = curve.iter().map(|(_, r)| *r).sum::<f64>() / m;
    let std = (curve.iter().map(|(_, r)| (r - mean).powi(2)).sum::<f64>() / m).sqrt();
    let peak_z = (std > 1e-9).then(|| (peak_r - mean) / std);
    Some((finite(peak_r), peak_lag, peak_z))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seam_local_peak_recovers_offset_seam() {
        // The seam's true match sits at a nonzero lag: a plain lag-0 score misses it, the ±max_lag search
        // recovers both the correlation and the lag. Broadband (noise) so a small shift genuinely decorrelates.
        let n = 2000usize;
        let mut seed = 0x1234_5678_9abc_def0u64;
        let mut rng = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((seed >> 33) as f64 / (1u64 << 30) as f64) - 1.0
        };
        let a: Vec<f64> = (0..n).map(|_| rng()).collect();
        let (anchor_start, true_lag, max_lag) = (600usize, 37i64, 200usize);
        let total = anchor_start + n + max_lag + 400;
        let mut b: Vec<f64> = (0..total).map(|_| rng() * 0.001).collect();
        let start = (anchor_start as i64 + true_lag) as usize;
        b[start..start + n].copy_from_slice(&a);

        let (r, lag, peak_z) = seam_local_peak(&a, &b, anchor_start, max_lag).expect("peak");
        assert!(r > 0.99, "seam recovers at its offset: r={r}");
        assert_eq!(lag, true_lag, "finds the seam-local lag, not lag 0");
        assert!(peak_z.is_some_and(|z| z > 5.0), "unique peak has a high z-score: {peak_z:?}");

        let lag0 = normalized_correlation(&a, &b[anchor_start..anchor_start + n]);
        assert!(lag0 < 0.5, "lag-0 score misses the offset seam: {lag0}");
    }
}
