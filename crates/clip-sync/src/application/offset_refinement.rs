use crate::application::ports::{PcmCorrelator, Resampler};
use crate::domain::{ClipMatchEstimate, MonoPcmClip};

const REFINE_WINDOW_SECS: u32 = 20;
const MAX_REFINE_ADJUSTMENT_SECS: f64 = 1.0;

const DISCOVER_TEMPLATE_SECS: u32 = 10;
const MIN_DISCOVER_CLIP_SECS: u32 = 20;
const MIN_DISCOVER_CORRELATION: f64 = 0.35;
const SILENCE_PEAK_FRACTION: f32 = 0.01;
const DISCOVER_TEMPLATE_POINTS: usize = 8_000;
const DISCOVER_RIGHT_POINTS: usize = 48_000;
const DISCOVER_MIN_COARSE_OFFSET_SECS: f64 = 15.0;
const DISCOVER_COARSE_NEIGHBORHOOD_SECS: f64 = 1.0;
const DISCOVER_SEARCH_FRACTION: f64 = 0.35;
const DISCOVER_SEARCH_WIDE_FRACTION: f64 = 0.5;
const DISCOVER_SEARCH_CAP_SECS: f64 = 42.0;
const DISCOVER_SEARCH_WIDE_CAP_SECS: f64 = 60.0;
const DISCOVER_SEARCH_FLOOR_SECS: f64 = 10.0;
const DISCOVER_WIDEN_IF_COARSE_CORR_BELOW: f64 = 0.25;
const DISCOVER_SKIP_IF_COARSE_SCORE: f64 = 0.9;
const DISCOVER_COARSE_MIN_CORRELATION: f64 = 0.25;
const DISCOVER_SCORE_TIE_EPSILON: f64 = 0.05;
const MIN_LAG_SIGNAL_SUM: f64 = 1.0;

/// Sample indices into `left` / `right` where the same event satisfies `t_B = t_A + offset`.
pub fn aligned_slice_starts(offset_samples: i64) -> (usize, usize) {
    (
        usize::try_from((-offset_samples).max(0)).unwrap_or(0),
        usize::try_from(offset_samples.max(0)).unwrap_or(0),
    )
}

/// Test-only snapshot of [`should_discover_offset`] and its coarse-correlation input.
#[cfg(test)]
pub(crate) fn discover_gate_status(
    left: &MonoPcmClip,
    right: &MonoPcmClip,
    coarse_offset_secs: f64,
) -> (bool, Option<f64>) {
    let coarse_correlation = best_correlation_near_offset(
        left,
        right,
        coarse_offset_secs,
        DISCOVER_COARSE_NEIGHBORHOOD_SECS,
    );
    (
        should_discover_offset(left, right, coarse_offset_secs),
        coarse_correlation,
    )
}

fn should_discover_offset(
    left: &MonoPcmClip,
    right: &MonoPcmClip,
    coarse_offset_secs: f64,
) -> bool {
    if left.sample_rate == 0 {
        return false;
    }

    let clip_secs = left.samples.len() as f64 / f64::from(left.sample_rate);
    if clip_secs <= f64::from(MIN_DISCOVER_CLIP_SECS) {
        return false;
    }

    if coarse_offset_secs < DISCOVER_MIN_COARSE_OFFSET_SECS {
        return false;
    }

    best_correlation_near_offset(left, right, coarse_offset_secs, DISCOVER_COARSE_NEIGHBORHOOD_SECS)
        .is_none_or(|score| score < DISCOVER_SKIP_IF_COARSE_SCORE)
}

/// Fast Pearson scan over a downsampled ±`radius_secs` window (discover gate / widen probe).
fn best_correlation_near_offset(
    left: &MonoPcmClip,
    right: &MonoPcmClip,
    offset_secs: f64,
    radius_secs: f64,
) -> Option<f64> {
    if left.sample_rate != right.sample_rate || left.sample_rate == 0 {
        return None;
    }

    let rate = left.sample_rate;
    let left_start = first_audio_index(&left.samples);
    let template_samples = (rate as usize * DISCOVER_TEMPLATE_SECS as usize)
        .min(left.samples.len().saturating_sub(left_start))
        .min(right.samples.len());
    if template_samples < rate as usize {
        return None;
    }

    let max_start = right
        .samples
        .len()
        .saturating_sub(template_samples);
    let predicted_right = (left_start as f64 + offset_secs * f64::from(rate))
        .round()
        .clamp(0.0, max_start as f64) as usize;

    if radius_secs <= 0.0 {
        let template = samples_to_f64(&left.samples[left_start..left_start + template_samples]);
        let segment =
            samples_to_f64(&right.samples[predicted_right..predicted_right + template_samples]);
        return Some(normalized_correlation(&template, &segment));
    }

    let search_samples = (radius_secs * f64::from(rate)).round() as usize;
    let min_right = predicted_right.saturating_sub(search_samples);
    let max_right = (predicted_right + search_samples).min(max_start);
    let hay_end = (max_right + template_samples).min(right.samples.len());

    let template = samples_to_f64(&left.samples[left_start..left_start + template_samples]);
    let haystack = samples_to_f64(&right.samples[min_right..hay_end]);
    let downsample = choose_downsample(template_samples, haystack.len());
    let template_ds = downsample_f64(&template, downsample);
    let haystack_ds = downsample_f64(&haystack, downsample);
    if haystack_ds.len() < template_ds.len() || template_ds.is_empty() {
        return None;
    }

    let max_lag_ds = haystack_ds
        .len()
        .saturating_sub(template_ds.len())
        .min((max_right - min_right) / downsample);

    let mut best = f64::NEG_INFINITY;
    let mut lag = 0usize;
    while lag <= max_lag_ds {
        let score = normalized_correlation(
            &template_ds,
            &haystack_ds[lag..lag + template_ds.len()],
        );
        best = best.max(score);
        lag += 1;
    }

    if best.is_finite() {
        Some(best)
    } else {
        None
    }
}

/// Refine a coarse Chromaprint offset using PCM cross-correlation.
///
/// Searches a window around the coarse estimate on long clips (large leaders), then
/// fine-tunes with a short aligned cross-correlation.
pub fn refine_offset_estimate(
    left: &MonoPcmClip,
    right: &MonoPcmClip,
    coarse: ClipMatchEstimate,
    resampler: &dyn Resampler,
    correlator: &dyn PcmCorrelator,
) -> ClipMatchEstimate {
    if coarse.confidence <= 0.0 || left.sample_rate != right.sample_rate {
        return coarse;
    }

    let mut estimate = if should_discover_offset(left, right, coarse.offset_secs) {
        if let Some((discovered_offset, score)) =
            pcm_discover_offset(left, right, coarse.offset_secs)
        {
            ClipMatchEstimate {
                offset_secs: discovered_offset,
                confidence: coarse.confidence.max(correlation_to_confidence(score)),
            }
        } else {
            coarse
        }
    } else {
        coarse
    };

    let max_adjustment = max_refine_adjustment_secs(left);
    if let Some(adjustment) =
        pcm_lag_adjustment_secs(left, right, estimate.offset_secs, resampler, correlator)
    {
        if adjustment.abs() <= max_adjustment {
            estimate.offset_secs += adjustment;
        }
    }

    estimate
}

/// Refine offset near a prior estimate (e.g. start-clip Δ) using constrained PCM search.
///
/// Used for end-clip validation: detect drift within `search_radius_secs` without a full
/// Chromaprint rediscovery pass.
pub fn refine_offset_around_prior(
    left: &MonoPcmClip,
    right: &MonoPcmClip,
    prior: ClipMatchEstimate,
    search_radius_secs: f64,
    resampler: &dyn Resampler,
    correlator: &dyn PcmCorrelator,
) -> ClipMatchEstimate {
    if prior.confidence <= 0.0 || search_radius_secs <= 0.0 {
        return prior;
    }

    let mut estimate = if let Some((discovered_offset, score)) =
        pcm_search_near_offset(left, right, prior.offset_secs, search_radius_secs)
    {
        ClipMatchEstimate {
            offset_secs: discovered_offset,
            confidence: prior.confidence.max(correlation_to_confidence(score)),
        }
    } else {
        prior
    };

    if let Some((adjustment, _)) = pcm_cross_correlate_lag(
        left,
        right,
        estimate.offset_secs,
        REFINE_WINDOW_SECS,
        resampler,
        correlator,
    ) {
        if adjustment.abs() <= search_radius_secs {
            estimate.offset_secs += adjustment;
        }
    }

    estimate
}

/// Slide a template from `left` across `right`, searching near `center_offset_secs`.
///
/// Scores with local-window Pearson (`normalized_correlation`), not
/// [`PcmCorrelator::slide_template_scores`] (GCC-PHAT). The port remains for lag refine /
/// holdout; discover dropped PHAT in Jul 2026 so `DISCOVER_*` thresholds stay Pearson-scaled.
fn pcm_search_near_offset(
    left: &MonoPcmClip,
    right: &MonoPcmClip,
    center_offset_secs: f64,
    search_radius_secs: f64,
) -> Option<(f64, f64)> {
    if left.sample_rate != right.sample_rate || left.sample_rate == 0 || search_radius_secs <= 0.0
    {
        return None;
    }

    let rate = left.sample_rate;
    let left_start = first_audio_index(&left.samples);
    let template_samples = (rate as usize * DISCOVER_TEMPLATE_SECS as usize)
        .min(left.samples.len().saturating_sub(left_start))
        .min(right.samples.len());
    if template_samples < rate as usize {
        return None;
    }

    let template = samples_to_f64(&left.samples[left_start..left_start + template_samples]);
    let predicted_right_sample = (left_start as f64 + center_offset_secs * f64::from(rate))
        .round()
        .clamp(0.0, right.samples.len().saturating_sub(1) as f64) as usize;
    let search_samples = (search_radius_secs * f64::from(rate)).round() as usize;
    let min_right_sample = predicted_right_sample.saturating_sub(search_samples);
    let max_right_sample = (predicted_right_sample + search_samples).min(right.samples.len());
    let max_start = right.samples.len().saturating_sub(template_samples);
    let search_end = max_right_sample.min(max_start);

    let search_window = &right.samples[min_right_sample..search_end + template_samples];
    let downsample = choose_downsample(template_samples, search_window.len());
    let template_ds = downsample_f64(&template, downsample);
    let window_ds = downsample_f64(&samples_to_f64(search_window), downsample);
    if window_ds.len() < template_ds.len() || template_ds.is_empty() {
        return None;
    }

    let max_lag = window_ds.len().saturating_sub(template_ds.len());
    let slide_scores: Vec<f64> = (0..=max_lag)
        .map(|lag| {
            normalized_correlation(&template_ds, &window_ds[lag..lag + template_ds.len()]).abs()
        })
        .collect();
    let right_peak = right
        .samples
        .iter()
        .map(|sample| sample.unsigned_abs())
        .max()
        .unwrap_or(0);

    let mut best_sample = predicted_right_sample.min(max_start);
    let mut best_full_score = f64::NEG_INFINITY;
    let mut best_distance = f64::INFINITY;

    for (lag, score) in slide_scores.iter().enumerate() {
        let right_start = min_right_sample + lag.saturating_mul(downsample);
        if right_start > search_end {
            break;
        }
        let segment = &right.samples[right_start..right_start + template_samples];
        if !window_has_audio(segment, right_peak) {
            continue;
        }
        let offset_secs =
            right_start as f64 / f64::from(rate) - left_start as f64 / f64::from(rate);
        let distance = (offset_secs - center_offset_secs).abs();
        let better = *score > best_full_score + DISCOVER_SCORE_TIE_EPSILON
            || (*score >= best_full_score - DISCOVER_SCORE_TIE_EPSILON && distance < best_distance);
        if better {
            best_full_score = *score;
            best_sample = right_start;
            best_distance = distance;
        }
    }

    if best_full_score
        < discover_min_accept_score(
            &template,
            &right.samples[predicted_right_sample.min(max_start)
                ..predicted_right_sample.min(max_start) + template_samples],
            best_full_score,
        )
    {
        return None;
    }

    let offset_secs =
        best_sample as f64 / f64::from(rate) - left_start as f64 / f64::from(rate);
    Some((offset_secs, best_full_score))
}

/// Slide a template from `left` across `right`, searching near `coarse_offset_secs`.
/// Symmetric PCM discover search radius: clip-scaled policy, optionally widened when
/// coarse PCM correlation is poor, then clamped to where the template still fits.
fn discover_search_radius_secs(
    left: &MonoPcmClip,
    center_offset_secs: f64,
    widen: bool,
) -> f64 {
    let rate = f64::from(left.sample_rate);
    let clip_secs = left.samples.len() as f64 / rate;
    let left_start_secs = first_audio_index(&left.samples) as f64 / rate;
    let template_secs = f64::from(DISCOVER_TEMPLATE_SECS);
    let max_offset = (clip_secs - template_secs - left_start_secs).max(0.0);
    let min_offset = -left_start_secs;

    let fraction = if widen {
        DISCOVER_SEARCH_WIDE_FRACTION
    } else {
        DISCOVER_SEARCH_FRACTION
    };
    let cap = if widen {
        DISCOVER_SEARCH_WIDE_CAP_SECS
    } else {
        DISCOVER_SEARCH_CAP_SECS
    };
    let policy = (clip_secs * fraction).clamp(DISCOVER_SEARCH_FLOOR_SECS, cap);

    let upward = (max_offset - center_offset_secs).max(0.0);
    let downward = (center_offset_secs - min_offset).max(0.0);

    policy.min(upward).min(downward)
}

fn pcm_discover_offset(
    left: &MonoPcmClip,
    right: &MonoPcmClip,
    coarse_offset_secs: f64,
) -> Option<(f64, f64)> {
    if left.sample_rate != right.sample_rate || left.sample_rate == 0 {
        return None;
    }

    let rate = left.sample_rate;
    let min_samples = rate as usize * MIN_DISCOVER_CLIP_SECS as usize;
    if left.samples.len() < min_samples || right.samples.len() < min_samples {
        return None;
    }

    let coarse_corr = best_correlation_near_offset(
        left,
        right,
        coarse_offset_secs,
        DISCOVER_COARSE_NEIGHBORHOOD_SECS,
    );
    let widen = coarse_corr.is_none_or(|score| score < DISCOVER_WIDEN_IF_COARSE_CORR_BELOW);
    let search_secs = discover_search_radius_secs(left, coarse_offset_secs, widen);
    pcm_search_near_offset(left, right, coarse_offset_secs, search_secs)
}

fn max_refine_adjustment_secs(_left: &MonoPcmClip) -> f64 {
    MAX_REFINE_ADJUSTMENT_SECS
}

fn pcm_lag_adjustment_secs(
    left: &MonoPcmClip,
    right: &MonoPcmClip,
    coarse_offset_secs: f64,
    resampler: &dyn Resampler,
    correlator: &dyn PcmCorrelator,
) -> Option<f64> {
    pcm_cross_correlate_lag(
        left,
        right,
        coarse_offset_secs,
        REFINE_WINDOW_SECS,
        resampler,
        correlator,
    )
    .map(|(adjustment, _)| adjustment)
}

/// FFT cross-correlation lag adjustment between aligned PCM slices.
pub fn pcm_cross_correlate_lag(
    left: &MonoPcmClip,
    right: &MonoPcmClip,
    offset_secs: f64,
    window_secs: u32,
    resampler: &dyn Resampler,
    correlator: &dyn PcmCorrelator,
) -> Option<(f64, f64)> {
    if left.samples.is_empty() || right.samples.is_empty() {
        return None;
    }

    let target_rate = left.sample_rate.max(right.sample_rate);
    let left = if left.sample_rate == target_rate {
        left.clone()
    } else {
        resampler.resample_mono(left, target_rate)
    };
    let right = if right.sample_rate == target_rate {
        right.clone()
    } else {
        resampler.resample_mono(right, target_rate)
    };

    let window_samples = usize::min(
        target_rate as usize * window_secs as usize,
        left.samples.len().min(right.samples.len()),
    );
    if window_samples < target_rate as usize {
        return None;
    }

    let offset_samples = (offset_secs * f64::from(target_rate)).round() as i64;
    let (left_start, right_start) = aligned_slice_starts(offset_samples);

    if left_start + window_samples > left.samples.len()
        || right_start + window_samples > right.samples.len()
    {
        return None;
    }

    let left_f64: Vec<f64> = left.samples[left_start..left_start + window_samples]
        .iter()
        .map(|sample| f64::from(*sample))
        .collect();
    let right_f64: Vec<f64> = right.samples[right_start..right_start + window_samples]
        .iter()
        .map(|sample| f64::from(*sample))
        .collect();

    if left_f64.iter().map(|v| v.abs()).sum::<f64>() < MIN_LAG_SIGNAL_SUM
        || right_f64.iter().map(|v| v.abs()).sum::<f64>() < MIN_LAG_SIGNAL_SUM
    {
        return None;
    }

    let (lag_samples, peak_magnitude) = correlator.cross_correlate_lag(&left_f64, &right_f64)?;
    let adjustment = -lag_samples / f64::from(target_rate);
    Some((adjustment, peak_magnitude))
}

/// Refine lag on hold-out segments extracted at the discovery offset (lag ≈ 0).
pub fn refine_holdout_segment_lag(
    left: &MonoPcmClip,
    right: &MonoPcmClip,
    max_adjustment_secs: f64,
    resampler: &dyn Resampler,
    correlator: &dyn PcmCorrelator,
) -> Option<(f64, f64)> {
    if left.sample_rate == 0 || right.sample_rate == 0 {
        return None;
    }

    let window_secs = (left.samples.len() as f64 / f64::from(left.sample_rate))
        .min(right.samples.len() as f64 / f64::from(right.sample_rate))
        .floor()
        .max(1.0) as u32;

    let (adjustment, peak) =
        pcm_cross_correlate_lag(left, right, 0.0, window_secs, resampler, correlator)?;
    if adjustment.abs() > max_adjustment_secs {
        return None;
    }
    Some((adjustment, peak))
}

fn first_audio_index(samples: &[i16]) -> usize {
    if samples.is_empty() {
        return 0;
    }

    let peak = samples
        .iter()
        .map(|sample| sample.unsigned_abs())
        .max()
        .unwrap_or(0) as f32;
    let threshold = (peak * SILENCE_PEAK_FRACTION).max(64.0);
    samples
        .iter()
        .position(|sample| f32::from(sample.unsigned_abs()) >= threshold)
        .unwrap_or(0)
}

/// Skip discover candidates whose right-hand template window is still leading silence.
fn window_has_audio(samples: &[i16], clip_peak: u16) -> bool {
    if samples.is_empty() {
        return false;
    }
    let threshold = (f32::from(clip_peak) * SILENCE_PEAK_FRACTION).max(64.0);
    samples
        .iter()
        .any(|sample| f32::from(sample.unsigned_abs()) >= threshold)
}

fn discover_min_accept_score(
    template: &[f64],
    center_segment: &[i16],
    best_score: f64,
) -> f64 {
    let center_score = if center_segment.len() == template.len() {
        normalized_correlation(template, &samples_to_f64(center_segment)).abs()
    } else {
        0.0
    };
    if center_score < DISCOVER_WIDEN_IF_COARSE_CORR_BELOW {
        let uplifted = center_score + 0.08;
        uplifted.clamp(DISCOVER_COARSE_MIN_CORRELATION, MIN_DISCOVER_CORRELATION)
    } else {
        MIN_DISCOVER_CORRELATION.max(center_score + 0.05)
    }
    .min(best_score + 1.0)
}

fn samples_to_f64(samples: &[i16]) -> Vec<f64> {
    samples.iter().map(|sample| f64::from(*sample)).collect()
}

fn downsample_f64(samples: &[f64], factor: usize) -> Vec<f64> {
    if factor <= 1 {
        return samples.to_vec();
    }

    samples
        .chunks(factor)
        .map(|chunk| chunk.iter().sum::<f64>() / chunk.len() as f64)
        .collect()
}

fn choose_downsample(template_samples: usize, right_samples: usize) -> usize {
    let mut factor = 1usize;
    while template_samples / factor > DISCOVER_TEMPLATE_POINTS {
        factor += 1;
    }
    while right_samples / factor > DISCOVER_RIGHT_POINTS {
        factor += 1;
    }
    factor.max(1)
}

pub fn normalized_correlation(left: &[f64], right: &[f64]) -> f64 {
    debug_assert_eq!(left.len(), right.len());
    if left.is_empty() {
        return 0.0;
    }

    let left_mean = left.iter().sum::<f64>() / left.len() as f64;
    let right_mean = right.iter().sum::<f64>() / right.len() as f64;

    let mut numerator = 0.0;
    let mut left_var = 0.0;
    let mut right_var = 0.0;
    for (left_sample, right_sample) in left.iter().zip(right.iter()) {
        let left_delta = left_sample - left_mean;
        let right_delta = right_sample - right_mean;
        numerator += left_delta * right_delta;
        left_var += left_delta * left_delta;
        right_var += right_delta * right_delta;
    }

    let denominator = (left_var * right_var).sqrt();
    if denominator <= f64::EPSILON {
        0.0
    } else {
        (numerator / denominator).clamp(-1.0, 1.0)
    }
}

fn correlation_to_confidence(score: f64) -> f32 {
    ((score - MIN_DISCOVER_CORRELATION) / (1.0 - MIN_DISCOVER_CORRELATION))
        .clamp(0.0, 1.0) as f32
}

#[cfg(test)]
mod tests {
    use std::f32::consts::TAU;

    use super::*;
    use crate::domain::pcm_preparation::{prepare_clip_for_fingerprint, PcmPreparationOptions};
    use crate::infrastructure::correlation::FftCorrelator;
    use crate::infrastructure::resample::RubatoResampler;

    fn chirp_clip(sample_rate: u32, start_index: u64, seconds: u32) -> MonoPcmClip {
        let count = sample_rate as usize * seconds as usize;
        let rate = f64::from(sample_rate);
        let samples: Vec<i16> = (0..count)
            .map(|offset| {
                let index = start_index + offset as u64;
                let t = index as f64 / rate;
                let freq = 300.0 + 400.0 * t;
                ((TAU as f64 * freq * t).sin() * (i16::MAX as f64 * 0.5)).round() as i16
            })
            .collect();
        MonoPcmClip {
            sample_rate,
            samples,
            decode_error_skips: 0,
            decoded_sample_count: None,
        }
    }

    fn delayed_pair(sample_rate: u32, seconds: u32, offset_secs: u32) -> (MonoPcmClip, MonoPcmClip) {
        let delay_samples = u64::from(sample_rate) * u64::from(offset_secs);
        let total_samples = u64::from(sample_rate) * u64::from(seconds);
        let left = (0..total_samples).map(|index| chirp_sample(sample_rate, index));
        let right = (0..total_samples).map(|index| {
            if index < delay_samples {
                0
            } else {
                chirp_sample(sample_rate, index - delay_samples)
            }
        });
        (
            MonoPcmClip {
                sample_rate,
                samples: left.collect(),
                decode_error_skips: 0,
                decoded_sample_count: None,
            },
            MonoPcmClip {
                sample_rate,
                samples: right.collect(),
                decode_error_skips: 0,
                decoded_sample_count: None,
            },
        )
    }

    fn chirp_sample(sample_rate: u32, index: u64) -> i16 {
        let rate = f64::from(sample_rate);
        let t = index as f64 / rate;
        let freq = 300.0 + 400.0 * t;
        ((TAU as f64 * freq * t).sin() * (i16::MAX as f64 * 0.5)).round() as i16
    }

    fn tone_holdout_segment(sample_rate: u32, seconds: u32, freq_hz: f64) -> MonoPcmClip {
        let count = sample_rate as usize * seconds as usize;
        let rate = f64::from(sample_rate);
        let samples: Vec<i16> = (0..count)
            .map(|i| {
                let t = i as f64 / rate;
                ((TAU as f64 * freq_hz * t).sin() * (i16::MAX as f64 * 0.5)).round() as i16
            })
            .collect();
        MonoPcmClip {
            sample_rate,
            samples,
            decode_error_skips: 0,
            decoded_sample_count: None,
        }
    }

    /// Hold-out pair when discovery underestimated offset: right lags left by `lag_secs`.
    fn holdout_tone_pair_with_lag(
        sample_rate: u32,
        segment_secs: u32,
        lag_secs: f64,
    ) -> (MonoPcmClip, MonoPcmClip) {
        let left = tone_holdout_segment(sample_rate, segment_secs, 440.0);
        let lag_samples = (lag_secs * f64::from(sample_rate)).round() as usize;
        let mut right_samples = vec![0i16; lag_samples];
        right_samples.extend_from_slice(&left.samples[..left.samples.len().saturating_sub(lag_samples)]);
        right_samples.resize(left.samples.len(), 0);
        let right = MonoPcmClip {
            sample_rate,
            samples: right_samples,
            decode_error_skips: 0,
            decoded_sample_count: None,
        };
        (left, right)
    }

    #[test]
    fn refine_high_rate_segment_known_lag() {
        let sample_rate = 44_100;
        let lag_secs = 0.020;
        let (left, right) = holdout_tone_pair_with_lag(sample_rate, 3, lag_secs);
        let (adjustment, peak) =
            refine_holdout_segment_lag(&left, &right, 0.1, &RubatoResampler, &FftCorrelator).expect("adjustment");
        assert!((adjustment - lag_secs).abs() < 0.003, "adjustment={adjustment}");
        assert!(peak > 0.0);
    }

    #[test]
    fn refine_high_rate_respects_max_adjustment() {
        let sample_rate = 44_100;
        let lag_secs = 0.020;
        let (left, right) = holdout_tone_pair_with_lag(sample_rate, 3, lag_secs);
        assert!(
            refine_holdout_segment_lag(&left, &right, 0.1, &RubatoResampler, &FftCorrelator)
                .expect("within cap")
                .0
                .abs()
                > 0.0
        );
        assert!(refine_holdout_segment_lag(&left, &right, 0.010, &RubatoResampler, &FftCorrelator).is_none());
    }

    #[test]
    fn high_rate_holdout_corrects_typical_chromaprint_residual_at_44k() {
        let sample_rate = 44_100;
        let lag_secs = 3.0 - 2.971_428_573_131_561;
        let (left, right) = holdout_tone_pair_with_lag(sample_rate, 3, lag_secs);
        let (adjustment, peak) =
            refine_holdout_segment_lag(&left, &right, 0.1, &RubatoResampler, &FftCorrelator).expect("adjustment");
        assert!(peak > 0.0);
        assert!(
            (adjustment - lag_secs).abs() < 0.005,
            "adjustment={adjustment}, lag_secs={lag_secs}"
        );
    }

    #[test]
    fn pcm_cross_correlate_at_44k_corrects_chromaprint_coarse() {
        let sample_rate = 44_100;
        let (left, right) = delayed_pair(sample_rate, 120, 3);
        let coarse = 2.971_428_573_131_561;
        let (adjustment, peak) =
            pcm_cross_correlate_lag(&left, &right, coarse, 3, &RubatoResampler, &FftCorrelator).expect("adjustment");
        assert!(peak > 0.0);
        assert!(
            (coarse + adjustment - 3.0).abs() < 0.015,
            "coarse={coarse}, adjustment={adjustment}"
        );
    }

    #[test]
    fn aligned_slice_starts_positive_offset() {
        assert_eq!(aligned_slice_starts(3_000), (0, 3_000));
    }

    #[test]
    fn aligned_slice_starts_negative_offset() {
        assert_eq!(aligned_slice_starts(-5_000), (5_000, 0));
    }

    #[test]
    fn refine_around_prior_finds_offset_within_radius() {
        let sample_rate = 11_025;
        let (left, right) = delayed_pair(sample_rate, 60, 3);
        let options = PcmPreparationOptions {
            normalize_loudness: false,
            trim_silence: false,
            window_slide_secs: 0,
        };
        let left = prepare_clip_for_fingerprint(&left, options).unwrap();
        let right = prepare_clip_for_fingerprint(&right, options).unwrap();

        let prior = ClipMatchEstimate {
            offset_secs: 1.0,
            confidence: 0.9,
        };
        let refined = refine_offset_around_prior(&left, &right, prior, 5.0, &RubatoResampler, &FftCorrelator);
        assert!(
            (refined.offset_secs - 3.0).abs() < 0.15,
            "refined={}",
            refined.offset_secs
        );
    }

    #[test]
    fn discover_gate_finds_plausible_correlation_near_true_offset() {
        let sample_rate = 11_025;
        let (left, right) = {
            let count = sample_rate as usize * 60;
            let delay = sample_rate as usize * 15;
            let left: Vec<i16> = (0..count)
                .map(|i| {
                    let t = i as f32 / sample_rate as f32;
                    let freq = 300.0 + 400.0 * t;
                    ((TAU * freq * t).sin() * (i16::MAX as f32 * 0.5)).round() as i16
                })
                .collect();
            let mut right = vec![0i16; count];
            right[delay..].copy_from_slice(&left[..count - delay]);
            (
                MonoPcmClip {
                    sample_rate,
                    samples: left,
                    decode_error_skips: 0,
                    decoded_sample_count: None,
                },
                MonoPcmClip {
                    sample_rate,
                    samples: right,
                    decode_error_skips: 0,
                    decoded_sample_count: None,
                },
            )
        };

        let near = best_correlation_near_offset(&left, &right, 15.0, DISCOVER_COARSE_NEIGHBORHOOD_SECS)
            .expect("score");
        let far = best_correlation_near_offset(&left, &right, 45.0, DISCOVER_COARSE_NEIGHBORHOOD_SECS)
            .expect("score");
        assert!(near > far, "near={near}, far={far}");
        assert!(near > DISCOVER_WIDEN_IF_COARSE_CORR_BELOW, "near={near}");
    }

    #[test]
    fn pcm_lag_fixes_three_second_leader_at_11k() {
        let sample_rate = 11_025;
        let (left, right) = delayed_pair(sample_rate, 60, 3);
        let options = PcmPreparationOptions {
            normalize_loudness: false,
            trim_silence: false,
            window_slide_secs: 0,
        };
        let left = prepare_clip_for_fingerprint(&left, options).unwrap();
        let right = prepare_clip_for_fingerprint(&right, options).unwrap();

        let coarse = ClipMatchEstimate {
            offset_secs: 2.971_428_573_131_561,
            confidence: 0.8,
        };
        let refined = refine_offset_estimate(&left, &right, coarse, &RubatoResampler, &FftCorrelator);
        assert!(
            (refined.offset_secs - 3.0).abs() < 0.050,
            "refined={}",
            refined.offset_secs
        );
    }

    #[test]
    fn pcm_lag_fixes_five_second_b_ahead_at_11k() {
        let sample_rate = 11_025;
        let total_samples = u64::from(sample_rate) * 60;
        let lead_samples = u64::from(sample_rate) * 5;
        let left: Vec<i16> = (0..total_samples)
            .map(|index| {
                if index < lead_samples {
                    0
                } else {
                    chirp_sample(sample_rate, index - lead_samples)
                }
            })
            .collect();
        let right: Vec<i16> = (0..total_samples)
            .map(|index| chirp_sample(sample_rate, index))
            .collect();
        let left = MonoPcmClip {
            sample_rate,
            samples: left,
            decode_error_skips: 0,
            decoded_sample_count: None,
        };
        let right = MonoPcmClip {
            sample_rate,
            samples: right,
            decode_error_skips: 0,
            decoded_sample_count: None,
        };
        let options = PcmPreparationOptions {
            normalize_loudness: false,
            trim_silence: false,
            window_slide_secs: 0,
        };
        let left = prepare_clip_for_fingerprint(&left, options).unwrap();
        let right = prepare_clip_for_fingerprint(&right, options).unwrap();

        let coarse = ClipMatchEstimate {
            offset_secs: -4.952_380_952_380_952,
            confidence: 0.8,
        };
        let refined = refine_offset_estimate(&left, &right, coarse, &RubatoResampler, &FftCorrelator);
        assert!(
            (refined.offset_secs + 5.0).abs() < 0.050,
            "refined={}",
            refined.offset_secs
        );
    }

    #[test]
    fn pcm_cross_correlate_reports_zero_adjustment_when_already_aligned() {
        let sample_rate = 44_100;
        let (left, right) = delayed_pair(sample_rate, 10, 3);
        let (adjustment, peak) = pcm_cross_correlate_lag(&left, &right, 3.0, 3, &RubatoResampler, &FftCorrelator).expect("adjustment");
        assert!(adjustment.abs() < 1.0 / f64::from(sample_rate));
        assert!(peak > 0.0);
    }

    #[test]
    fn refinement_moves_offset_toward_true_lag() {
        let sample_rate = 44_100;
        let lead_secs = 2;
        let left = chirp_clip(sample_rate, 0, 20);
        let right = chirp_clip(sample_rate, lead_secs as u64 * sample_rate as u64, 20);

        let coarse = ClipMatchEstimate {
            offset_secs: -1.5,
            confidence: 0.8,
        };
        let refined = refine_offset_estimate(&left, &right, coarse, &RubatoResampler, &FftCorrelator);
        assert!(
            (refined.offset_secs + f64::from(lead_secs)).abs()
                < (coarse.offset_secs + f64::from(lead_secs)).abs() + 0.3,
            "refined={}",
            refined.offset_secs
        );
    }

    #[test]
    #[ignore = "slow: pcm_discover on 60s clips; cargo test pcm_discover_finds_thirty -- --ignored"]
    fn pcm_discover_finds_thirty_second_leader_on_sixty_second_clips() {
        let sample_rate = 11_025;
        let (left, right) = delayed_pair(sample_rate, 60, 30);
        let options = PcmPreparationOptions {
            normalize_loudness: false,
            trim_silence: false,
            window_slide_secs: 0,
        };
        let left = prepare_clip_for_fingerprint(&left, options).unwrap();
        let right = prepare_clip_for_fingerprint(&right, options).unwrap();

        let (offset, score) =
            pcm_discover_offset(&left, &right, 16.0).expect("discovered offset");
        assert!(score >= MIN_DISCOVER_CORRELATION, "score={score}");
        assert!(
            (offset - 30.0).abs() < 1.5,
            "offset={offset}, score={score}"
        );
    }

    #[test]
    #[ignore = "slow: pcm_discover on 60s clips; cargo test pcm_discover_finds_fifteen_second_leader -- --ignored --exact"]
    fn pcm_discover_finds_fifteen_second_leader() {
        let sample_rate = 11_025;
        let (left, right) = delayed_pair(sample_rate, 60, 15);
        let options = PcmPreparationOptions {
            normalize_loudness: false,
            trim_silence: false,
            window_slide_secs: 0,
        };
        let left = prepare_clip_for_fingerprint(&left, options).unwrap();
        let right = prepare_clip_for_fingerprint(&right, options).unwrap();

        let (offset, score) =
            pcm_discover_offset(&left, &right, 15.0).expect("discovered offset");
        assert!(score >= MIN_DISCOVER_CORRELATION, "score={score}");
        assert!(
            (offset - 15.0).abs() < 1.0,
            "offset={offset}, score={score}"
        );
    }

    #[test]
    fn pcm_discover_finds_fifteen_second_leader_without_normalization() {
        let sample_rate = 11_025;
        let (left, right) = delayed_pair(sample_rate, 60, 15);
        let options = PcmPreparationOptions {
            normalize_loudness: false,
            trim_silence: false,
            window_slide_secs: 0,
        };
        let left = prepare_clip_for_fingerprint(&left, options).unwrap();
        let right = prepare_clip_for_fingerprint(&right, options).unwrap();

        let coarse = ClipMatchEstimate {
            offset_secs: 14.980952389538288,
            confidence: 0.8,
        };
        let refined = refine_offset_estimate(&left, &right, coarse, &RubatoResampler, &FftCorrelator);
        assert!(
            (refined.offset_secs - 15.0).abs() < 1.0,
            "refined={}",
            refined.offset_secs
        );
    }

    #[test]
    fn discover_search_radius_covers_wav_leader_60s() {
        let sample_rate = 11_025;
        let (left, right) = delayed_pair(sample_rate, 120, 60);
        let corpus_prep = PcmPreparationOptions {
            normalize_loudness: false,
            trim_silence: false,
            window_slide_secs: 0,
        };
        let left_p = prepare_clip_for_fingerprint(&left, corpus_prep).unwrap();
        let _right_p = prepare_clip_for_fingerprint(&right, corpus_prep).unwrap();

        const COARSE: f64 = 32.438_095;
        const TRUE_OFFSET: f64 = 60.0;

        let radius_normal = discover_search_radius_secs(&left_p, COARSE, false);
        let radius_wide = discover_search_radius_secs(&left_p, COARSE, true);
        assert!(
            COARSE + radius_normal >= TRUE_OFFSET,
            "normal radius {radius_normal} too small from coarse {COARSE}"
        );
        assert!(
            COARSE + radius_wide >= TRUE_OFFSET,
            "wide radius {radius_wide} too small from coarse {COARSE}"
        );
        assert!(radius_normal >= 32.0, "expected physical clamp ~32s, got {radius_normal}");
    }

    #[test]
    fn discover_search_radius_keeps_tight_window_when_coarse_is_plausible() {
        let sample_rate = 11_025;
        let (left, _) = delayed_pair(sample_rate, 60, 30);
        let left_p = prepare_clip_for_fingerprint(
            &left,
            PcmPreparationOptions {
                normalize_loudness: false,
                trim_silence: false,
                window_slide_secs: 0,
            },
        )
        .unwrap();

        let radius = discover_search_radius_secs(&left_p, 16.0, false);
        assert!(
            radius <= DISCOVER_SEARCH_CAP_SECS,
            "radius {radius} should stay within normal cap"
        );
        assert!(16.0 + radius >= 30.0, "radius {radius} should still reach true 30s offset");
    }

    fn wav_leader_60s_prep_pair() -> (MonoPcmClip, MonoPcmClip, ClipMatchEstimate) {
        use crate::application::config::ChromaprintPreset;
        use crate::application::ports::{Aligner, Fingerprinter};
        use crate::infrastructure::chromaprint::{ChromaprintAligner, ChromaprintFingerprinter};

        const SAMPLE_RATE: u32 = 11_025;
        const CLIP_SECS: u32 = 120;
        const OFFSET_SECS: u32 = 60;

        let (left, right) = delayed_pair(SAMPLE_RATE, CLIP_SECS, OFFSET_SECS);
        let corpus_prep = PcmPreparationOptions {
            normalize_loudness: false,
            trim_silence: false,
            window_slide_secs: 0,
        };
        let left_p = prepare_clip_for_fingerprint(&left, corpus_prep).unwrap();
        let right_p = prepare_clip_for_fingerprint(&right, corpus_prep).unwrap();

        let preset = ChromaprintPreset::default();
        let fingerprinter = ChromaprintFingerprinter::new(preset);
        let aligner = ChromaprintAligner::new(preset);
        let fp_a = fingerprinter.fingerprint(&left_p).expect("fp a");
        let fp_b = fingerprinter.fingerprint(&right_p).expect("fp b");
        let coarse = aligner.find_offset(&fp_a, &fp_b).expect("chromaprint");
        (left_p, right_p, coarse)
    }

    /// Fast (~seconds): Chromaprint coarse offset + discover gate only.
    #[test]
    #[ignore = "diagnostic; cargo test diagnose_wav_leader_60s_coarse -- --ignored --nocapture"]
    fn diagnose_wav_leader_60s_coarse() {
        let (left_p, right_p, coarse) = wav_leader_60s_prep_pair();
        let (should_discover, coarse_corr) =
            discover_gate_status(&left_p, &right_p, coarse.offset_secs);
        let true_corr = best_correlation_near_offset(
            &left_p,
            &right_p,
            60.0,
            DISCOVER_COARSE_NEIGHBORHOOD_SECS,
        );

        eprintln!("=== wav_leader_60s coarse diagnostic (120s clip, 60s offset) ===");
        eprintln!(
            "chromaprint: offset={:.6}s confidence={:.4}",
            coarse.offset_secs, coarse.confidence
        );
        eprintln!(
            "pcm correlation @ coarse (±{DISCOVER_COARSE_NEIGHBORHOOD_SECS}s): {coarse_corr:?}"
        );
        eprintln!(
            "pcm correlation @ true 60s (±{DISCOVER_COARSE_NEIGHBORHOOD_SECS}s): {true_corr:?}"
        );
        eprintln!(
            "should_discover_offset: {should_discover} (skip discover when coarse corr >= {DISCOVER_SKIP_IF_COARSE_SCORE})"
        );
        let widen = coarse_corr.is_none_or(|score| score < DISCOVER_WIDEN_IF_COARSE_CORR_BELOW);
        let radius_normal = discover_search_radius_secs(&left_p, coarse.offset_secs, false);
        let radius_wide = discover_search_radius_secs(&left_p, coarse.offset_secs, true);
        eprintln!(
            "discover widen={widen} (coarse corr < {DISCOVER_WIDEN_IF_COARSE_CORR_BELOW}); radius normal={radius_normal:.2}s wide={radius_wide:.2}s"
        );
    }

    /// Slow (minutes): full refine + pcm_discover. Run only when coarse diagnostic warrants it.
    #[test]
    #[ignore = "slow diagnostic; cargo test diagnose_wav_leader_60s_full -- --ignored --nocapture"]
    fn diagnose_wav_leader_60s_full() {
        let (left_p, right_p, coarse) = wav_leader_60s_prep_pair();
        let refined = refine_offset_estimate(&left_p, &right_p, coarse, &RubatoResampler, &FftCorrelator);
        let discover = pcm_discover_offset(&left_p, &right_p, coarse.offset_secs);
        let lag_adj = pcm_lag_adjustment_secs(
            &left_p,
            &right_p,
            refined.offset_secs,
            &RubatoResampler,
            &FftCorrelator,
        );

        eprintln!("=== wav_leader_60s full diagnostic ===");
        eprintln!(
            "chromaprint coarse: offset={:.6}s confidence={:.4}",
            coarse.offset_secs, coarse.confidence
        );
        eprintln!("pcm_discover_offset from coarse: {discover:?}");
        eprintln!(
            "refine_offset_estimate final: offset={:.6}s confidence={:.4}",
            refined.offset_secs, refined.confidence
        );
        eprintln!("pcm_lag_adjustment at final: {lag_adj:?}");
    }

    #[test]
    #[ignore = "slow: pcm_discover on 120s clips; cargo test refine_recovers_sixty_second_leader -- --ignored"]
    fn refine_recovers_sixty_second_leader_on_120_second_clips() {
        let (left_p, right_p, coarse) = wav_leader_60s_prep_pair();
        let refined = refine_offset_estimate(&left_p, &right_p, coarse, &RubatoResampler, &FftCorrelator);
        assert!(
            (refined.offset_secs - 60.0).abs() < 1.5,
            "refined={}",
            refined.offset_secs
        );
    }

    #[test]
    #[ignore = "slow: discover+refine on 60s clips; cargo test refine_recovers_large -- --ignored"]
    fn refine_recovers_large_chromaprint_error() {
        let sample_rate = 11_025;
        let (left, right) = delayed_pair(sample_rate, 60, 30);
        let options = PcmPreparationOptions {
            normalize_loudness: false,
            trim_silence: false,
            window_slide_secs: 0,
        };
        let left = prepare_clip_for_fingerprint(&left, options).unwrap();
        let right = prepare_clip_for_fingerprint(&right, options).unwrap();

        let coarse = ClipMatchEstimate {
            offset_secs: 16.0,
            confidence: 0.8,
        };
        let refined = refine_offset_estimate(&left, &right, coarse, &RubatoResampler, &FftCorrelator);
        assert!(
            (refined.offset_secs - 30.0).abs() < 1.5,
            "refined={}",
            refined.offset_secs
        );
    }
}
