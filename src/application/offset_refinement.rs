use cross_correlate::{Correlate, CrossCorrelationMode};

use crate::domain::{resample_mono_pcm, ClipMatchEstimate, MonoPcmClip};

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
const DISCOVER_SEARCH_CAP_SECS: f64 = 21.0;
const DISCOVER_SEARCH_FLOOR_SECS: f64 = 10.0;
const DISCOVER_SKIP_IF_COARSE_SCORE: f64 = 0.9;
const DISCOVER_COARSE_MIN_CORRELATION: f64 = 0.25;
const DISCOVER_SCORE_TIE_EPSILON: f64 = 0.05;
const DISCOVER_FULL_REFINE_RADIUS_FACTOR: usize = 2;
const MIN_LAG_SIGNAL_SUM: f64 = 1.0;

/// Sample indices into `left` / `right` where the same event satisfies `t_B = t_A + offset`.
pub fn aligned_slice_starts(offset_samples: i64) -> (usize, usize) {
    (
        usize::try_from((-offset_samples).max(0)).unwrap_or(0),
        usize::try_from(offset_samples.max(0)).unwrap_or(0),
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

fn best_correlation_near_offset(
    left: &MonoPcmClip,
    right: &MonoPcmClip,
    offset_secs: f64,
    radius_secs: f64,
) -> Option<f64> {
    if radius_secs <= 0.0 {
        return correlation_at_offset(left, right, offset_secs);
    }

    let rate = f64::from(left.sample_rate);
    let center = (offset_secs * rate).round() as i64;
    let radius = (radius_secs * rate).round() as i64;
    let mut best = f64::NEG_INFINITY;
    for delta in -radius..=radius {
        if let Some(score) =
            correlation_at_offset(left, right, (center + delta) as f64 / rate)
        {
            best = best.max(score);
        }
    }

    if best.is_finite() {
        Some(best)
    } else {
        None
    }
}

fn correlation_at_offset(
    left: &MonoPcmClip,
    right: &MonoPcmClip,
    offset_secs: f64,
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

    let right_start = (left_start as f64 + offset_secs * f64::from(rate))
        .round()
        .clamp(0.0, right.samples.len().saturating_sub(template_samples) as f64) as usize;

    let template = samples_to_f64(&left.samples[left_start..left_start + template_samples]);
    let segment = samples_to_f64(&right.samples[right_start..right_start + template_samples]);
    Some(normalized_correlation(&template, &segment))
}

/// Refine a coarse Chromaprint offset using PCM cross-correlation.
///
/// Searches a window around the coarse estimate on long clips (large leaders), then
/// fine-tunes with a short aligned cross-correlation.
pub fn refine_offset_estimate(
    left: &MonoPcmClip,
    right: &MonoPcmClip,
    coarse: ClipMatchEstimate,
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
    if let Some(adjustment) = pcm_lag_adjustment_secs(left, right, estimate.offset_secs) {
        if adjustment.abs() <= max_adjustment {
            estimate.offset_secs += adjustment;
        }
    }

    estimate
}

/// Slide a template from `left` across `right`, searching near `coarse_offset_secs`.
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

    let left_start = first_audio_index(&left.samples);
    let template_samples = (rate as usize * DISCOVER_TEMPLATE_SECS as usize)
        .min(left.samples.len().saturating_sub(left_start))
        .min(right.samples.len());
    if template_samples < rate as usize {
        return None;
    }

    let template = samples_to_f64(&left.samples[left_start..left_start + template_samples]);
    let downsample = choose_downsample(template_samples, right.samples.len());
    let template_ds = downsample_f64(&template, downsample);
    let right_ds = downsample_f64(&samples_to_f64(&right.samples), downsample);
    if right_ds.len() < template_ds.len() || template_ds.is_empty() {
        return None;
    }

    let clip_secs = left.samples.len() as f64 / f64::from(rate);
    let search_secs = (clip_secs * DISCOVER_SEARCH_FRACTION)
        .min(DISCOVER_SEARCH_CAP_SECS)
        .max(DISCOVER_SEARCH_FLOOR_SECS);
    let predicted_right_sample = (left_start as f64 + coarse_offset_secs * f64::from(rate))
        .round()
        .clamp(0.0, right.samples.len().saturating_sub(1) as f64) as usize;
    let search_samples = (search_secs * f64::from(rate)).round() as usize;
    let min_right_sample = predicted_right_sample.saturating_sub(search_samples);
    let max_right_sample = (predicted_right_sample + search_samples).min(right.samples.len());
    let max_start = right.samples.len().saturating_sub(template_samples);
    let search_end = max_right_sample.min(max_start);

    let min_lag = (min_right_sample / downsample)
        .min(right_ds.len().saturating_sub(template_ds.len()));
    let max_lag = (max_right_sample / downsample)
        .min(right_ds.len().saturating_sub(template_ds.len()));

    let mut coarse_peaks: Vec<(usize, f64)> = Vec::new();
    let mut lag = min_lag;
    while lag <= max_lag {
        let score =
            normalized_correlation(&template_ds, &right_ds[lag..lag + template_ds.len()]);
        coarse_peaks.push((lag, score));
        lag += 1;
    }

    coarse_peaks.sort_by(|(lag_a, score_a), (lag_b, score_b)| {
        score_b
            .partial_cmp(score_a)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                let dist_a = (*lag_a as i64 * downsample as i64 - predicted_right_sample as i64).unsigned_abs();
                let dist_b = (*lag_b as i64 * downsample as i64 - predicted_right_sample as i64).unsigned_abs();
                dist_a.cmp(&dist_b)
            })
    });

    let mut candidates = vec![predicted_right_sample.min(max_start)];
    for (lag, score) in coarse_peaks.iter().take(3) {
        if *score >= DISCOVER_COARSE_MIN_CORRELATION {
            candidates.push(lag.saturating_mul(downsample).min(max_start));
        }
    }
    candidates.sort_unstable();
    candidates.dedup();

    let refine_radius = downsample.saturating_mul(DISCOVER_FULL_REFINE_RADIUS_FACTOR).max(1);
    let mut best_sample = predicted_right_sample.min(max_start);
    let mut best_full_score = f64::NEG_INFINITY;
    let mut best_distance = f64::INFINITY;

    for candidate in candidates {
        let refine_min = candidate
            .saturating_sub(refine_radius)
            .max(min_right_sample);
        let refine_max = (candidate + refine_radius).min(search_end);
        let mut right_start = refine_min;
        while right_start <= refine_max {
            let segment =
                samples_to_f64(&right.samples[right_start..right_start + template_samples]);
            let score = normalized_correlation(&template, &segment);
            let offset_secs =
                right_start as f64 / f64::from(rate) - left_start as f64 / f64::from(rate);
            let distance = (offset_secs - coarse_offset_secs).abs();
            let better = score > best_full_score + DISCOVER_SCORE_TIE_EPSILON
                || (score >= best_full_score - DISCOVER_SCORE_TIE_EPSILON
                    && distance < best_distance);
            if better {
                best_full_score = score;
                best_sample = right_start;
                best_distance = distance;
            }
            right_start += 1;
        }
    }

    if best_full_score < MIN_DISCOVER_CORRELATION {
        return None;
    }

    let offset_secs =
        best_sample as f64 / f64::from(rate) - left_start as f64 / f64::from(rate);
    Some((offset_secs, best_full_score))
}

fn max_refine_adjustment_secs(_left: &MonoPcmClip) -> f64 {
    MAX_REFINE_ADJUSTMENT_SECS
}

fn pcm_lag_adjustment_secs(
    left: &MonoPcmClip,
    right: &MonoPcmClip,
    coarse_offset_secs: f64,
) -> Option<f64> {
    pcm_cross_correlate_lag(left, right, coarse_offset_secs, REFINE_WINDOW_SECS)
        .map(|(adjustment, _)| adjustment)
}

/// FFT cross-correlation lag adjustment between aligned PCM slices.
pub fn pcm_cross_correlate_lag(
    left: &MonoPcmClip,
    right: &MonoPcmClip,
    offset_secs: f64,
    window_secs: u32,
) -> Option<(f64, f64)> {
    if left.samples.is_empty() || right.samples.is_empty() {
        return None;
    }

    let target_rate = left.sample_rate.max(right.sample_rate);
    let left = if left.sample_rate == target_rate {
        left.clone()
    } else {
        resample_mono_pcm(left, target_rate)
    };
    let right = if right.sample_rate == target_rate {
        right.clone()
    } else {
        resample_mono_pcm(right, target_rate)
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

    let correlation = Correlate::create_real_f64(
        left_f64.len(),
        right_f64.len(),
        CrossCorrelationMode::Valid,
    )
    .ok()?;
    let corr = correlation.correlate_managed(&left_f64, &right_f64).ok()?;
    let (peak_index, peak_value) = corr
        .iter()
        .enumerate()
        .max_by(|(_, left), (_, right)| {
            left.abs()
                .partial_cmp(&right.abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })?;

    let center = corr.len() / 2;
    let lag_samples = peak_index as i64 - center as i64;
    let adjustment = -lag_samples as f64 / f64::from(target_rate);
    Some((adjustment, peak_value.abs()))
}

/// Refine lag on hold-out segments extracted at the discovery offset (lag ≈ 0).
pub fn refine_holdout_segment_lag(
    left: &MonoPcmClip,
    right: &MonoPcmClip,
    max_adjustment_secs: f64,
) -> Option<(f64, f64)> {
    if left.sample_rate == 0 || right.sample_rate == 0 {
        return None;
    }

    let window_secs = (left.samples.len() as f64 / f64::from(left.sample_rate))
        .min(right.samples.len() as f64 / f64::from(right.sample_rate))
        .floor()
        .max(1.0) as u32;

    let (adjustment, peak) = pcm_cross_correlate_lag(left, right, 0.0, window_secs)?;
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

fn normalized_correlation(left: &[f64], right: &[f64]) -> f64 {
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

    fn prepare_pair(left: &MonoPcmClip, right: &MonoPcmClip) -> (MonoPcmClip, MonoPcmClip) {
        let options = PcmPreparationOptions {
            normalize_loudness: true,
            trim_silence: true,
            window_slide_secs: 0,
        };
        (
            prepare_clip_for_fingerprint(left, options).unwrap(),
            prepare_clip_for_fingerprint(right, options).unwrap(),
        )
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
        let refined = refine_offset_estimate(&left, &right, coarse);
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
        let refined = refine_offset_estimate(&left, &right, coarse);
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
        let (adjustment, peak) = pcm_cross_correlate_lag(&left, &right, 3.0, 3).expect("adjustment");
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
        let refined = refine_offset_estimate(&left, &right, coarse);
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
        let (left, right) = prepare_pair(&left, &right);

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
        let (left, right) = prepare_pair(&left, &right);

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
        let refined = refine_offset_estimate(&left, &right, coarse);
        assert!(
            (refined.offset_secs - 15.0).abs() < 1.0,
            "refined={}",
            refined.offset_secs
        );
    }

    #[test]
    #[ignore = "slow: discover+refine on 60s clips; cargo test refine_recovers_large -- --ignored"]
    fn refine_recovers_large_chromaprint_error() {
        let sample_rate = 11_025;
        let (left, right) = delayed_pair(sample_rate, 60, 30);
        let (left, right) = prepare_pair(&left, &right);

        let coarse = ClipMatchEstimate {
            offset_secs: 16.0,
            confidence: 0.8,
        };
        let refined = refine_offset_estimate(&left, &right, coarse);
        assert!(
            (refined.offset_secs - 30.0).abs() < 1.5,
            "refined={}",
            refined.offset_secs
        );
    }
}
