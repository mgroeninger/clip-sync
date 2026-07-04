//! Shared seam-local lag search — the per-shoulder registration primitive used by **both** the diagnostic
//! `splice_dualfit` (fingerprint scan) and the production dual-fit repair (A3). One implementation so the
//! two can never drift — that divergence is exactly the bug class we fixed twice (seam-local vs 1 s-gross
//! placement; ledger A3).

use clip_sync::normalized_correlation;
use rustfft::{num_complex::Complex, FftPlanner};

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

/// **B1 — FFT-accelerated equivalent of [`lag_correlation_curve`].** Not yet wired into any caller (that's
/// perf-plan §3 step 4); this exists purely to be gated by the equivalence test below before the swap.
///
/// The numerator `sum_ab(base) = Σ a[i]·b_ctx[base+i]` is a linear cross-correlation, computed via one
/// forward FFT of each zero-padded signal, a conjugate-multiply, and one inverse FFT (`O(M log M)`,
/// `M = (n + nb).next_power_of_two()` — large enough that the implicit circular convolution never wraps
/// into the range of `base` we read). The Pearson **denominator** (sliding mean/var of the `b_ctx` window)
/// is NOT FFT-accelerated — it's exact via prefix sums, which is where the real precision floor comes from
/// (not the FFT). Reproduces the naive function's lag convention (`base = max_lag + lag`) and edge mask
/// (`base + n <= b_ctx.len()`) exactly, so `curve.len()` matches lag-for-lag.
pub fn lag_correlation_curve_fft(a: &[f64], b_ctx: &[f64], max_lag: i64) -> Vec<(i64, f64)> {
    let n = a.len();
    let nb = b_ctx.len();
    if n == 0 || max_lag < 0 || nb < n {
        return Vec::new();
    }

    // Prefix sums of b_ctx and b_ctx^2 → O(1) windowed sum/sum-of-squares for any base.
    let mut psum = vec![0.0f64; nb + 1];
    let mut psum2 = vec![0.0f64; nb + 1];
    for i in 0..nb {
        psum[i + 1] = psum[i] + b_ctx[i];
        psum2[i + 1] = psum2[i] + b_ctx[i] * b_ctx[i];
    }

    let mean_a = a.iter().sum::<f64>() / n as f64;
    let var_a: f64 = a.iter().map(|&x| (x - mean_a).powi(2)).sum();

    // Circular cross-correlation via FFT: IFFT(conj(FFT(a_pad)) .* FFT(b_pad))[k] = Σ a[i]·b_ctx[i+k] for
    // k in the valid (non-wrapping) range, since M >= n + nb - 1 zero-pads away any wraparound there.
    let m = (n + nb).next_power_of_two();
    let mut a_buf: Vec<Complex<f64>> =
        (0..m).map(|i| Complex::new(if i < n { a[i] } else { 0.0 }, 0.0)).collect();
    let mut b_buf: Vec<Complex<f64>> =
        (0..m).map(|i| Complex::new(if i < nb { b_ctx[i] } else { 0.0 }, 0.0)).collect();

    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(m);
    let ifft = planner.plan_fft_inverse(m);
    fft.process(&mut a_buf);
    fft.process(&mut b_buf);
    let mut prod: Vec<Complex<f64>> =
        a_buf.iter().zip(b_buf.iter()).map(|(x, y)| x.conj() * y).collect();
    ifft.process(&mut prod);
    let scale = 1.0 / m as f64;

    let max_base = nb - n;
    let mut out = Vec::with_capacity((2 * max_lag + 1).max(0) as usize);
    for lag in -max_lag..=max_lag {
        let base_i = max_lag + lag;
        if base_i < 0 {
            continue;
        }
        let base = base_i as usize;
        if base > max_base {
            continue;
        }
        let sum_ab = prod[base].re * scale;
        let sum_b = psum[base + n] - psum[base];
        let sum_b2 = psum2[base + n] - psum2[base];
        let var_b = sum_b2 - sum_b * sum_b / n as f64;
        let numerator = sum_ab - mean_a * sum_b;
        let denom = (var_a * var_b).sqrt();
        let r = if denom <= f64::EPSILON { 0.0 } else { (numerator / denom).clamp(-1.0, 1.0) };
        out.push((lag, finite(r)));
    }
    out
}

/// Cost crossover (in `n · (2·max_lag+1)` ops) for [`lag_correlation_curve_auto`]. Set well above the
/// small-probe regime (±30 ms seam-uniqueness / ±100 ms envelope bins, cost ~1e4) and well below the
/// production seam-local / baseline sweep regime (±600 ms over a 250 ms–1 s window, cost ~1e8–1e9), so the
/// boundary never sits near a real call site — perf-plan §3 step 4.
const FFT_CROSSOVER_OPS: u64 = 1_000_000;

/// Auto-select between [`lag_correlation_curve`] and [`lag_correlation_curve_fft`] by estimated naive cost
/// (`n · (2·max_lag+1)`). Equivalence is pinned by `fft_curve_matches_naive_*` below (ε = 1e-8), so callers
/// on the sweep side of the crossover get the FFT path for free; small probes stay on the naive path where
/// FFT setup (`FftPlanner`, zero-padding) would dominate.
pub fn lag_correlation_curve_auto(a: &[f64], b_ctx: &[f64], max_lag: i64) -> Vec<(i64, f64)> {
    let n = a.len() as u64;
    let width = (2 * max_lag.max(0) + 1) as u64;
    if n.saturating_mul(width) > FFT_CROSSOVER_OPS {
        lag_correlation_curve_fft(a, b_ctx, max_lag)
    } else {
        lag_correlation_curve(a, b_ctx, max_lag)
    }
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
    let curve = lag_correlation_curve_auto(a_seam, b_ctx, max_lag as i64);
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

    fn noise(seed: u64, n: usize, scale: f64) -> Vec<f64> {
        let mut seed = seed | 1;
        (0..n)
            .map(|_| {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                (((seed >> 33) as f64 / (1u64 << 30) as f64) - 1.0) * scale
            })
            .collect()
    }

    /// Per-lag ε for the FFT-vs-naive Pearson comparison. f64 rustfft round-trip error is ~1e-10 relative;
    /// this is intentionally tight — a real porting bug (wrong lag convention, wrong edge mask, wrong
    /// normalization) should trip it, only float-reassociation noise should not (perf-plan §4.7 B1).
    const FFT_EPS: f64 = 1e-8;

    fn assert_curves_equivalent(a: &[f64], b_ctx: &[f64], max_lag: i64, label: &str) {
        let naive = lag_correlation_curve(a, b_ctx, max_lag);
        let fft = lag_correlation_curve_fft(a, b_ctx, max_lag);
        assert_eq!(
            naive.len(),
            fft.len(),
            "{label}: curve length must match (same edge mask) — naive {} vs fft {}",
            naive.len(),
            fft.len()
        );
        for ((lag_n, r_n), (lag_f, r_f)) in naive.iter().zip(fft.iter()) {
            assert_eq!(lag_n, lag_f, "{label}: lag ordering must match");
            assert!(
                (r_n - r_f).abs() < FFT_EPS,
                "{label}: lag {lag_n} diverges — naive {r_n} vs fft {r_f} (Δ={})",
                (r_n - r_f).abs()
            );
        }
    }

    #[test]
    fn fft_curve_matches_naive_full_sweep() {
        // Scaled-down stand-in for the production ±600 ms / 1 s-window sweep (n ~ window, max_lag ~ n·1.2)
        // — full production size would make the naive comparator itself too slow for a unit test.
        let n = 2000usize;
        let max_lag = 2400i64;
        let a = noise(0xA5A5_1234, n, 1.0);
        let total = n + 2 * max_lag as usize + 37; // ragged tail exercises the edge mask on both sides
        let mut b_ctx = noise(0x99, total, 0.001);
        let true_lag = 900i64;
        let start = (max_lag + true_lag) as usize;
        b_ctx[start..start + n].copy_from_slice(&a);

        assert_curves_equivalent(&a, &b_ctx, max_lag, "full_sweep");

        // The FFT path must recover the same seam as the naive path recovers, not just agree numerically.
        let fft = lag_correlation_curve_fft(&a, &b_ctx, max_lag);
        let &(peak_lag, peak_r) = fft
            .iter()
            .max_by(|(_, x), (_, y)| x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal))
            .expect("non-empty curve");
        assert_eq!(peak_lag, true_lag, "fft curve peaks at the true lag");
        assert!(peak_r > 0.99, "fft curve peak correlation: {peak_r}");
    }

    #[test]
    fn fft_curve_matches_naive_small_probe() {
        // Scaled-down stand-in for the ±25 ms seam probe / 100 ms envelope bin — the small-`L` regime where
        // production keeps the naive path (auto-select by `n·L`, perf-plan §3 step 4).
        let n = 200usize;
        let max_lag = 30i64;
        let a = noise(0x1357_9bdf, n, 1.0);
        let total = n + 2 * max_lag as usize;
        let mut b_ctx = noise(0x2468, total, 0.001);
        let true_lag = 5i64;
        let start = (max_lag + true_lag) as usize;
        b_ctx[start..start + n].copy_from_slice(&a);

        assert_curves_equivalent(&a, &b_ctx, max_lag, "small_probe");
    }

    #[test]
    fn fft_curve_matches_naive_at_ragged_edges() {
        // `b_ctx` shorter than `n + 2*max_lag` on both sides forces the edge mask to trim differently at
        // each end — the exact class of off-by-one that would silently shift `peak_z` (whole-curve stat).
        let n = 500usize;
        let max_lag = 400i64;
        let a = noise(0x1111_2222, n, 1.0);
        // Only 900 samples of context instead of the full 500 + 800 = 1300 the unclipped sweep would want.
        let b_ctx = noise(0x3333_4444, 900, 0.5);
        assert_curves_equivalent(&a, &b_ctx, max_lag, "ragged_edges");
    }

    #[test]
    fn auto_picks_naive_for_small_probe_and_fft_for_full_sweep() {
        // Small probe (±30 ms seam-uniqueness class): auto must stay naive — verified indirectly by
        // checking it reproduces the naive curve exactly (no FFT round-trip noise at all).
        let n = 200usize;
        let max_lag = 30i64;
        let a = noise(0x1357_9bdf, n, 1.0);
        let total = n + 2 * max_lag as usize;
        let b_ctx = noise(0x2468, total, 0.001);
        let naive = lag_correlation_curve(&a, &b_ctx, max_lag);
        let auto = lag_correlation_curve_auto(&a, &b_ctx, max_lag);
        assert_eq!(naive, auto, "small probe: auto must take the naive path bit-for-bit");

        // Full sweep (production seam-local / baseline-lag class): auto must match naive within the FFT
        // epsilon (it took the FFT path), not bit-for-bit.
        let n = 2000usize;
        let max_lag = 2400i64;
        let a = noise(0xA5A5_1234, n, 1.0);
        let total = n + 2 * max_lag as usize + 37;
        let mut b_ctx = noise(0x99, total, 0.001);
        let true_lag = 900i64;
        let start = (max_lag + true_lag) as usize;
        b_ctx[start..start + n].copy_from_slice(&a);
        assert_curves_equivalent(&a, &b_ctx, max_lag, "auto_full_sweep_vs_naive");
        let auto = lag_correlation_curve_auto(&a, &b_ctx, max_lag);
        let fft = lag_correlation_curve_fft(&a, &b_ctx, max_lag);
        assert_eq!(auto, fft, "full sweep: auto must take the fft path bit-for-bit (same fn)");
    }

    #[test]
    fn fft_curve_derived_readouts_match_naive() {
        // Pins the *derived* readouts (peak_z, prominence, frac_lag_ms) the production floors (A5/C6) key
        // off of — not just the raw per-lag Pearson values (perf-plan §4.7 B1).
        use crate::application::gap_fingerprint::{summarize_lag_curve, LagChannel};

        let n = 1000usize;
        let max_lag = 1100i64;
        let a = noise(0xdead_beef, n, 1.0);
        let total = n + 2 * max_lag as usize;
        let mut b_ctx = noise(0xfeed_face, total, 0.02);
        let true_lag = -300i64;
        let start = (max_lag + true_lag) as usize;
        b_ctx[start..start + n].copy_from_slice(&a);

        let naive = lag_correlation_curve(&a, &b_ctx, max_lag);
        let fft = lag_correlation_curve_fft(&a, &b_ctx, max_lag);

        let sample_rate = 48_000u32;
        let summary_naive = summarize_lag_curve(&naive, sample_rate, 0, 0, LagChannel::Mono)
            .expect("naive summary");
        let summary_fft =
            summarize_lag_curve(&fft, sample_rate, 0, 0, LagChannel::Mono).expect("fft summary");

        assert_eq!(summary_naive.peak_lag_samples, summary_fft.peak_lag_samples);
        assert_eq!(summary_naive.edge_pinned, summary_fft.edge_pinned);
        assert_eq!(summary_naive.verdict, summary_fft.verdict);
        assert!(
            (summary_naive.frac_lag_ms - summary_fft.frac_lag_ms).abs() < 1e-6,
            "frac_lag_ms: naive {} vs fft {}",
            summary_naive.frac_lag_ms,
            summary_fft.frac_lag_ms
        );
        match (summary_naive.peak_z, summary_fft.peak_z) {
            (Some(zn), Some(zf)) => {
                assert!((zn - zf).abs() < 1e-6, "peak_z: naive {zn} vs fft {zf}")
            }
            (None, None) => {}
            other => panic!("peak_z presence mismatch: {other:?}"),
        }
        match (summary_naive.prominence, summary_fft.prominence) {
            (Some(pn), Some(pf)) => {
                assert!((pn - pf).abs() < 1e-6, "prominence: naive {pn} vs fft {pf}")
            }
            (None, None) => {}
            other => panic!("prominence presence mismatch: {other:?}"),
        }
    }
}
