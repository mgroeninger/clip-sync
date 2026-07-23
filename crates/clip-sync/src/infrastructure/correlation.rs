//! GCC-PHAT scoring helpers and FFT cross-correlation with sub-sample peak interpolation.
//!
//! [`PcmCorrelator::slide_template_scores`] is GCC-PHAT (level/EQ-robust). Fine lag on
//! already-aligned equal-length windows uses `cross_correlate` with parabolic peak fitting.
//! PCM **discover** search does not use this module — it slides Pearson in
//! `offset_refinement` so `DISCOVER_*` thresholds stay on that scale.

use cross_correlate::{Correlate, CrossCorrelationMode};
use rustfft::FftPlanner;

use crate::application::ports::PcmCorrelator;

const PHAT_EPSILON: f64 = 1e-12;

/// Production [`PcmCorrelator`]: GCC-PHAT slide/similarity + FFT lag with parabolic refine.
pub struct FftCorrelator;

impl PcmCorrelator for FftCorrelator {
    fn cross_correlate_lag(&self, a: &[f64], b: &[f64]) -> Option<(f64, f64)> {
        fft_cross_correlate_lag(a, b)
    }

    fn segment_similarity(&self, a: &[f64], b: &[f64]) -> f64 {
        gcc_phat_lag_zero_similarity(a, b)
    }

    fn slide_template_scores(&self, template: &[f64], signal: &[f64]) -> Vec<f64> {
        gcc_phat_slide_scores(template, signal)
    }
}

fn fft_cross_correlate_lag(a: &[f64], b: &[f64]) -> Option<(f64, f64)> {
    let correlation =
        Correlate::create_real_f64(a.len(), b.len(), CrossCorrelationMode::Full).ok()?;
    let corr = correlation.correlate_managed(a, b).ok()?;
    let peak_index = find_peak_index(&corr);
    let peak_value = corr[peak_index].abs();
    let fractional_index = parabolic_peak_index(&corr, peak_index);
    let center = (a.len() + b.len()).saturating_sub(1) as f64 / 2.0;
    let lag = fractional_index - center;
    Some((lag, peak_value))
}

fn to_complex_padded(signal: &[f64], n: usize) -> Vec<num_complex::Complex<f64>> {
    let mut buffer = signal
        .iter()
        .map(|sample| num_complex::Complex::new(*sample, 0.0))
        .collect::<Vec<_>>();
    buffer.resize(n, num_complex::Complex::new(0.0, 0.0));
    buffer
}

fn fft_len_for_correlation(a_len: usize, b_len: usize) -> usize {
    (a_len + b_len - 1).max(1).next_power_of_two()
}

fn gcc_phat_correlation(a: &[f64], b: &[f64]) -> Vec<f64> {
    fft_cross_correlation(a, b, true)
}

fn fft_cross_correlation(a: &[f64], b: &[f64], phat: bool) -> Vec<f64> {
    let out_len = a.len() + b.len().saturating_sub(1);
    if a.is_empty() || b.is_empty() {
        return Vec::new();
    }

    let n = fft_len_for_correlation(a.len(), b.len());
    let mut spectrum_a = to_complex_padded(a, n);
    let mut spectrum_b = to_complex_padded(b, n);

    let mut planner = FftPlanner::<f64>::new();
    let forward = planner.plan_fft_forward(n);
    let inverse = planner.plan_fft_inverse(n);

    forward.process(&mut spectrum_a);
    forward.process(&mut spectrum_b);

    for (sa, sb) in spectrum_a.iter_mut().zip(spectrum_b.iter()) {
        let cross = *sa * sb.conj();
        if phat {
            let mag = cross.norm() + PHAT_EPSILON;
            *sa = cross / mag;
        } else {
            *sa = cross;
        }
    }

    inverse.process(&mut spectrum_a);

    let scale = 1.0 / n as f64;
    spectrum_a
        .iter()
        .take(out_len)
        .map(|value| value.re * scale)
        .collect()
}

fn find_peak_index(values: &[f64]) -> usize {
    values
        .iter()
        .enumerate()
        .max_by(|(_, left), (_, right)| {
            left.abs()
                .partial_cmp(&right.abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(index, _)| index)
        .unwrap_or(0)
}

/// Parabolic fit around `peak_index`; returns a fractional index into `values`.
fn parabolic_peak_index(values: &[f64], peak_index: usize) -> f64 {
    if values.len() < 3 || peak_index == 0 || peak_index + 1 >= values.len() {
        return peak_index as f64;
    }

    let ym1 = values[peak_index - 1].abs();
    let y0 = values[peak_index].abs();
    let yp1 = values[peak_index + 1].abs();
    let denom = ym1 - 2.0 * y0 + yp1;
    if !denom.is_finite() || denom.abs() < PHAT_EPSILON {
        return peak_index as f64;
    }

    let delta = 0.5 * (ym1 - yp1) / denom;
    if !delta.is_finite() {
        return peak_index as f64;
    }

    (peak_index as f64 + delta.clamp(-1.0, 1.0)).clamp(0.0, (values.len() - 1) as f64)
}

fn gcc_phat_lag_zero_similarity(a: &[f64], b: &[f64]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let corr = gcc_phat_correlation(a, b);
    corr.first().copied().unwrap_or(0.0).abs()
}

/// Score every valid start index when sliding `template` across `signal` with one GCC-PHAT pass.
pub(crate) fn gcc_phat_slide_scores(template: &[f64], signal: &[f64]) -> Vec<f64> {
    if template.is_empty() || signal.len() < template.len() {
        return Vec::new();
    }

    let corr = gcc_phat_correlation(template, signal);
    let max_start = signal.len() - template.len();
    (0..=max_start)
        .map(|start| corr.get(start).copied().unwrap_or(0.0).abs())
        .collect()
}

#[cfg(test)]
mod tests {
    use std::f64::consts::TAU;

    use super::*;

    fn tone_samples(rate: u32, count: usize, freq_hz: f64) -> Vec<f64> {
        (0..count)
            .map(|i| {
                let t = i as f64 / f64::from(rate);
                (TAU * freq_hz * t).sin() * (i16::MAX as f64 * 0.5)
            })
            .collect()
    }

    #[test]
    fn parabolic_peak_refines_integer_peak() {
        let values = vec![0.1, 0.9, 1.0, 0.85, 0.2];
        let refined = parabolic_peak_index(&values, 2);
        assert!(
            refined > 1.85 && refined < 2.0,
            "expected peak slightly before index 2, got {refined}"
        );
    }

    #[test]
    fn fft_cross_correlate_finds_sub_sample_lag_on_tone_holdout() {
        let rate = 44_100;
        let lag_secs = 0.02;
        let count = rate as usize * 3;
        let lag_samples = (lag_secs * f64::from(rate)).round() as usize;
        let left = tone_samples(rate, count, 440.0);
        let mut right = vec![0.0; count];
        right[lag_samples..].copy_from_slice(&left[..count - lag_samples]);
        let (lag, peak) = fft_cross_correlate_lag(&left, &right).expect("lag");
        assert!(peak > 0.0);
        let lag_secs_est = -lag / f64::from(rate);
        assert!(
            (lag_secs_est - lag_secs).abs() < 0.003,
            "lag_secs_est={lag_secs_est}"
        );
    }

    #[test]
    fn gcc_phat_similarity_higher_when_aligned() {
        let rate = 44_100;
        let count = rate as usize * 2;
        let left = tone_samples(rate, count, 440.0);
        let right = left.clone();
        let aligned = FftCorrelator.segment_similarity(&left, &right);
        let misaligned = FftCorrelator.segment_similarity(&left, &right[100..count]);
        assert!(aligned > misaligned);
        assert!(aligned > 0.2);
    }
}
