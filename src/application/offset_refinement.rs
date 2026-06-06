use cross_correlate::{Correlate, CrossCorrelationMode};

use crate::domain::{ClipMatchEstimate, MonoPcmClip};

const REFINE_WINDOW_SECS: u32 = 10;
const MAX_REFINE_ADJUSTMENT_SECS: f64 = 1.0;

/// Refine a coarse Chromaprint offset using short-window PCM cross-correlation
/// around the coarse alignment point.
pub fn refine_offset_estimate(
    left: &MonoPcmClip,
    right: &MonoPcmClip,
    coarse: ClipMatchEstimate,
) -> ClipMatchEstimate {
    if coarse.confidence <= 0.0 || left.sample_rate != right.sample_rate {
        return coarse;
    }

    let Some(adjustment_secs) = pcm_lag_adjustment_secs(left, right, coarse.offset_secs) else {
        return coarse;
    };

    if adjustment_secs.abs() > MAX_REFINE_ADJUSTMENT_SECS {
        return coarse;
    }

    ClipMatchEstimate {
        offset_secs: coarse.offset_secs + adjustment_secs,
        confidence: coarse.confidence,
    }
}

fn pcm_lag_adjustment_secs(
    left: &MonoPcmClip,
    right: &MonoPcmClip,
    coarse_offset_secs: f64,
) -> Option<f64> {
    let window_samples = usize::min(
        left.sample_rate as usize * REFINE_WINDOW_SECS as usize,
        left.samples.len().min(right.samples.len()),
    );
    if window_samples < left.sample_rate as usize {
        return None;
    }

    let offset_samples = (coarse_offset_secs * f64::from(left.sample_rate)).round() as i64;
    let left_start = usize::try_from(offset_samples.max(0)).unwrap_or(0);
    let right_start = usize::try_from((-offset_samples).max(0)).unwrap_or(0);

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

    if left_f64.iter().map(|v| v.abs()).sum::<f64>() < 1.0
        || right_f64.iter().map(|v| v.abs()).sum::<f64>() < 1.0
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
    let peak_index = corr
        .iter()
        .enumerate()
        .max_by(|(_, left), (_, right)| {
            left.abs()
                .partial_cmp(&right.abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })?
        .0;

    let center = corr.len() / 2;
    let lag_samples = peak_index as i64 - center as i64;
    Some(-lag_samples as f64 / f64::from(left.sample_rate))
}

#[cfg(test)]
mod tests {
    use std::f32::consts::TAU;

    use super::*;
    use crate::domain::MonoPcmClip;

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
        }
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
}
