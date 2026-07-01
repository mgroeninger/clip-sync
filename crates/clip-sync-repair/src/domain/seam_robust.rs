//! Cross-codec-robust seam similarity metrics — **diagnostic data only** (not wired to the gate).
//!
//! Two lossy encodings of one master agree poorly *sample-for-sample* (Pearson@0 low), yet agree on
//! lower-resolution structure that codecs preserve: **R2** the low-frequency waveform, **R4** the
//! magnitude spectrum (phase-invariant). Measured at the gate's throat placement, these test whether a
//! dead waveform seam is a *validator mismatch* (robust-high while Pearson-low) rather than wrong
//! content. See `docs/TEMP-cross-codec-seam-impl-plan.md` §4.

use clip_sync::normalized_correlation;
use rustfft::{num_complex::Complex, FftPlanner};

/// Default low-pass cutoff (Hz) for R2 — keeps the low-frequency waveform codecs preserve.
pub const BANDLIMITED_CUTOFF_HZ: f64 = 300.0;

/// **R2** — band-limited Pearson: low-pass both windows to `cutoff_hz`, then lag-0 Pearson. Removes the
/// high-frequency detail codecs alter most; shared low-frequency waveform survives. `NaN` for tiny input.
pub fn bandlimited_pearson(a: &[f64], b: &[f64], sample_rate: u32, cutoff_hz: f64) -> f64 {
    let n = a.len().min(b.len());
    if n < 8 {
        return f64::NAN;
    }
    let la = lowpass(&a[..n], sample_rate, cutoff_hz);
    let lb = lowpass(&b[..n], sample_rate, cutoff_hz);
    normalized_correlation(&la, &lb)
}

/// **R4** — magnitude-spectrum correlation: Pearson of the Hann-windowed rFFT magnitudes of each window.
/// Phase-invariant ⇒ robust to small time shifts and to codec phase differences. `NaN` for tiny input.
pub fn spectrum_correlation(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len().min(b.len());
    if n < 8 {
        return f64::NAN;
    }
    let ma = magnitude_spectrum(&a[..n]);
    let mb = magnitude_spectrum(&b[..n]);
    normalized_correlation(&ma, &mb)
}

/// Zero-phase low-pass via a centered boxcar of length `≈ sample_rate / (2·cutoff)` (O(n) prefix sums).
fn lowpass(x: &[f64], sample_rate: u32, cutoff_hz: f64) -> Vec<f64> {
    let win = ((f64::from(sample_rate) / (2.0 * cutoff_hz.max(1.0))).round() as usize).max(1);
    if win <= 1 || x.len() < win {
        return x.to_vec();
    }
    let mut psum = Vec::with_capacity(x.len() + 1);
    psum.push(0.0);
    for &v in x {
        psum.push(psum.last().unwrap() + v);
    }
    let half = win / 2;
    (0..x.len())
        .map(|i| {
            let lo = i.saturating_sub(half);
            let hi = (i + half + 1).min(x.len());
            (psum[hi] - psum[lo]) / (hi - lo) as f64
        })
        .collect()
}

/// Hann-windowed real FFT magnitude spectrum (bins `0..N/2`), `N` = next power of two ≥ `x.len()`.
fn magnitude_spectrum(x: &[f64]) -> Vec<f64> {
    let n = x.len().next_power_of_two().max(2);
    let denom = (x.len() as f64 - 1.0).max(1.0);
    let mut buf: Vec<Complex<f64>> = (0..n)
        .map(|i| {
            let v = if i < x.len() {
                let h = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / denom).cos();
                x[i] * h
            } else {
                0.0
            };
            Complex::new(v, 0.0)
        })
        .collect();
    FftPlanner::new().plan_fft_forward(n).process(&mut buf);
    buf[..n / 2].iter().map(|c| c.norm()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(freq: f64, n: usize, sr: u32) -> Vec<f64> {
        (0..n).map(|i| (2.0 * std::f64::consts::PI * freq * i as f64 / f64::from(sr)).sin()).collect()
    }

    fn noise(seed: u64, n: usize) -> Vec<f64> {
        let mut s = seed | 1;
        (0..n)
            .map(|_| {
                s ^= s << 13;
                s ^= s >> 7;
                s ^= s << 17;
                ((s >> 33) as f64 / (1u64 << 31) as f64) - 0.5
            })
            .collect()
    }

    #[test]
    fn identical_windows_score_high() {
        let a = tone(220.0, 4096, 48_000);
        assert!(bandlimited_pearson(&a, &a, 48_000, BANDLIMITED_CUTOFF_HZ) > 0.99);
        assert!(spectrum_correlation(&a, &a) > 0.99);
    }

    #[test]
    fn independent_noise_scores_low() {
        let a = noise(1, 4096);
        let b = noise(2, 4096);
        assert!(bandlimited_pearson(&a, &b, 48_000, BANDLIMITED_CUTOFF_HZ).abs() < 0.5);
        assert!(spectrum_correlation(&a, &b) < 0.5);
    }

    #[test]
    fn spectrum_is_shift_invariant() {
        // Same content shifted by a few samples: waveform Pearson drops, magnitude spectrum holds.
        let base = tone(440.0, 4096, 48_000);
        let shifted = &base[10..];
        let aligned = &base[..shifted.len()];
        assert!(spectrum_correlation(aligned, shifted) > 0.95);
    }
}
