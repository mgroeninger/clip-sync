use std::time::Duration;

use crate::domain::{DomainError, MonoPcmClip};

/// Peak amplitude target for normalization (~−3 dBFS).
const NORMALIZE_TARGET_PEAK: f32 = i16::MAX as f32 * 0.707;
/// Fraction of peak used as silence threshold when trimming trailing silence.
const SILENCE_PEAK_FRACTION: f32 = 0.01;
/// Minimum RMS (0–1 of full scale) required after preparation.
const MIN_RMS_FRACTION: f32 = 0.002;
/// Minimum retained audio after preparation (seconds).
const MIN_RETAINED_SECS: f64 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PcmPreparationOptions {
    pub normalize_loudness: bool,
    pub trim_silence: bool,
    pub window_slide_secs: u32,
}

impl Default for PcmPreparationOptions {
    fn default() -> Self {
        Self {
            normalize_loudness: true,
            trim_silence: true,
            window_slide_secs: 30,
        }
    }
}

/// Expand a planned window so a shorter sub-clip can be slid to peak energy.
pub fn expand_window_for_slide(
    window: &crate::domain::ClipWindow,
    slide_secs: u32,
    duration: Duration,
) -> crate::domain::ClipWindow {
    if slide_secs == 0 {
        return *window;
    }

    let slide = Duration::from_secs(u64::from(slide_secs));
    let start = window.start.saturating_sub(slide);
    let end = (window.end + slide).min(duration);
    crate::domain::ClipWindow::new(start, end, window.label)
}

/// Pick aligned sub-clips of `target_duration` using peak energy in `left` only,
/// so both clips stay on the same relative timeline.
pub fn select_aligned_subclip_pair(
    left: &MonoPcmClip,
    right: &MonoPcmClip,
    target_duration: Duration,
) -> (MonoPcmClip, MonoPcmClip) {
    let target_samples = left
        .sample_rate
        .saturating_mul(target_duration.as_secs() as u32)
        .max(1) as usize;

    if left.samples.len() <= target_samples && right.samples.len() <= target_samples {
        return (left.clone(), right.clone());
    }

    let best_start = find_peak_subclip_start(&left.samples, target_samples);
    // Peak is chosen from `left` only; clamp per side so a shorter `right`
    // (tail truncation / decode shortfall) cannot panic on the slice.
    let left_start = best_start.min(left.samples.len());
    let right_start = best_start.min(right.samples.len());
    let left_end = (best_start + target_samples).min(left.samples.len());
    let right_end = (best_start + target_samples).min(right.samples.len());

    let left_clip = MonoPcmClip {
        sample_rate: left.sample_rate,
        samples: left.samples[left_start..left_end].to_vec(),
        decode_error_skips: left.decode_error_skips,
        decoded_sample_count: None,
    };
    let right_clip = MonoPcmClip {
        sample_rate: right.sample_rate,
        samples: right.samples[right_start..right_end].to_vec(),
        decode_error_skips: right.decode_error_skips,
        decoded_sample_count: None,
    };

    (left_clip, right_clip)
}

fn find_peak_subclip_start(samples: &[i16], target_samples: usize) -> usize {
    if samples.len() <= target_samples {
        return 0;
    }

    let mut best_start = 0usize;
    let mut best_rms = 0.0f32;
    let step = (target_samples / 8).max(1);
    let mut start = 0usize;

    while start + target_samples <= samples.len() {
        let rms = rms_of_slice(&samples[start..start + target_samples]);
        if rms > best_rms {
            best_rms = rms;
            best_start = start;
        }
        start += step;
    }

    best_start
}

pub fn prepare_clip_for_fingerprint(
    clip: &MonoPcmClip,
    options: PcmPreparationOptions,
) -> Result<MonoPcmClip, DomainError> {
    if clip.samples.is_empty() {
        return Err(DomainError::EmptyClip);
    }

    let mut prepared = clip.clone();

    if options.trim_silence {
        prepared = trim_trailing_silence(&prepared);
    }

    if options.normalize_loudness {
        prepared = peak_normalize(&prepared);
    }

    if !has_sufficient_audio(&prepared) {
        return Err(DomainError::InsufficientAudio);
    }

    Ok(prepared)
}

pub fn has_sufficient_audio(clip: &MonoPcmClip) -> bool {
    if clip.samples.is_empty() {
        return false;
    }

    let min_samples = (f64::from(clip.sample_rate) * MIN_RETAINED_SECS).ceil() as usize;
    if clip.samples.len() < min_samples {
        return false;
    }

    rms_of_slice(&clip.samples) >= MIN_RMS_FRACTION * i16::MAX as f32
}

/// Trim only trailing silence. Leading silence must be preserved so inter-file
/// offsets are not collapsed when each file has different pre-roll padding.
fn trim_trailing_silence(clip: &MonoPcmClip) -> MonoPcmClip {
    if clip.samples.is_empty() {
        return clip.clone();
    }

    let peak = clip
        .samples
        .iter()
        .map(|sample| sample.unsigned_abs())
        .max()
        .unwrap_or(0) as f32;
    let threshold = (peak * SILENCE_PEAK_FRACTION).max(64.0);

    let end = clip
        .samples
        .iter()
        .rposition(|&sample| f32::from(sample.unsigned_abs()) >= threshold)
        .map(|index| index + 1)
        .unwrap_or(clip.samples.len());

    if end == 0 || end == clip.samples.len() {
        return clip.clone();
    }

    MonoPcmClip {
        sample_rate: clip.sample_rate,
        samples: clip.samples[..end].to_vec(),
        decode_error_skips: clip.decode_error_skips,
        decoded_sample_count: clip.decoded_sample_count,
    }
}

fn peak_normalize(clip: &MonoPcmClip) -> MonoPcmClip {
    if clip.samples.is_empty() {
        return clip.clone();
    }

    let peak = clip
        .samples
        .iter()
        .map(|sample| sample.unsigned_abs())
        .max()
        .unwrap_or(0) as f32;
    if peak <= 1.0 {
        return clip.clone();
    }

    let scale = NORMALIZE_TARGET_PEAK / peak;
    let samples = clip
        .samples
        .iter()
        .map(|sample| {
            (f32::from(*sample) * scale)
                .round()
                .clamp(i16::MIN as f32, i16::MAX as f32) as i16
        })
        .collect();

    MonoPcmClip {
        sample_rate: clip.sample_rate,
        samples,
        decode_error_skips: clip.decode_error_skips,
        decoded_sample_count: clip.decoded_sample_count,
    }
}

fn rms_of_slice(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }

    let sum_sq: f64 = samples
        .iter()
        .map(|sample| {
            let value = f64::from(*sample);
            value * value
        })
        .sum();
    (sum_sq / samples.len() as f64).sqrt() as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trim_preserves_leading_silence() {
        let clip = MonoPcmClip {
            sample_rate: 1_000,
            samples: {
                let mut samples = vec![0_i16; 500];
                samples.extend((0..400).map(|i| (i as i16 % 100) * 100));
                samples.extend(vec![0_i16; 100]);
                samples
            },
            decode_error_skips: 0,
            decoded_sample_count: None,
        };

        let trimmed = trim_trailing_silence(&clip);
        assert_eq!(trimmed.samples.len(), 900);
        assert_eq!(trimmed.samples[0], 0);
        assert_eq!(trimmed.samples[499], 0);
    }

    #[test]
    fn trim_removes_trailing_silence() {
        let clip = MonoPcmClip {
            sample_rate: 1_000,
            samples: {
                let mut samples = vec![5_000_i16; 500];
                samples.extend(vec![0_i16; 500]);
                samples
            },
            decode_error_skips: 0,
            decoded_sample_count: None,
        };

        let trimmed = trim_trailing_silence(&clip);
        assert_eq!(trimmed.samples.len(), 500);
        assert_eq!(trimmed.samples[499], 5_000);
    }

    #[test]
    fn peak_normalize_scales_to_target() {
        let clip = MonoPcmClip {
            sample_rate: 1_000,
            samples: vec![1_000; 1_000],
            decode_error_skips: 0,
            decoded_sample_count: None,
        };
        let normalized = peak_normalize(&clip);
        let peak = normalized
            .samples
            .iter()
            .map(|s| s.unsigned_abs())
            .max()
            .unwrap();
        assert!((peak as i32 - NORMALIZE_TARGET_PEAK as i32).abs() <= 2);
    }

    #[test]
    fn aligned_subclip_pair_uses_left_energy_profile() {
        let mut left_samples = vec![10_i16; 2_000];
        for sample in &mut left_samples[800..1_200] {
            *sample = 5_000;
        }
        let right_samples = vec![7_i16; 2_000];
        let left = MonoPcmClip {
            sample_rate: 1_000,
            samples: left_samples,
            decode_error_skips: 0,
            decoded_sample_count: None,
        };
        let right = MonoPcmClip {
            sample_rate: 1_000,
            samples: right_samples,
            decode_error_skips: 0,
            decoded_sample_count: None,
        };
        let (left_clip, right_clip) =
            select_aligned_subclip_pair(&left, &right, Duration::from_secs(1));
        assert_eq!(left_clip.samples.len(), 1_000);
        assert_eq!(right_clip.samples.len(), 1_000);
        assert!(left_clip.samples.iter().any(|s| *s > 1_000));
    }

    #[test]
    fn aligned_subclip_pair_clamps_when_right_shorter_than_peak_start() {
        // Left peak near the end; right is truncated short of that peak start.
        // Must not panic; right slice is empty (start clamped to len).
        let mut left_samples = vec![10_i16; 4_000];
        for sample in &mut left_samples[3_000..4_000] {
            *sample = 8_000;
        }
        let left = MonoPcmClip {
            sample_rate: 1_000,
            samples: left_samples,
            decode_error_skips: 0,
            decoded_sample_count: None,
        };
        let right = MonoPcmClip {
            sample_rate: 1_000,
            samples: vec![7_i16; 500],
            decode_error_skips: 0,
            decoded_sample_count: None,
        };
        let (left_clip, right_clip) =
            select_aligned_subclip_pair(&left, &right, Duration::from_secs(1));
        assert!(!left_clip.samples.is_empty());
        assert!(right_clip.samples.is_empty());
    }

    #[test]
    fn rejects_silent_clip() {
        let clip = MonoPcmClip {
            sample_rate: 44_100,
            samples: vec![0; 44_100],
            decode_error_skips: 0,
            decoded_sample_count: None,
        };
        assert!(prepare_clip_for_fingerprint(&clip, PcmPreparationOptions::default()).is_err());
    }
}
