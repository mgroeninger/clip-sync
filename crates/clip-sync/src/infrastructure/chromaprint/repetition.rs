use crate::application::config::ChromaprintPreset;
use crate::domain::{Fingerprint, RepetitionFinding};
use crate::infrastructure::chromaprint::config::configuration_for_preset;

/// Minimum candidate lag (fingerprint items) to skip trivial near-zero-lag matches.
const MIN_LAG_ITEMS: usize = 40;

/// Maximum mean Hamming distance (bits out of 32) below which a lag is treated as a genuine
/// repeat. The expected value for unrelated audio is ~16 (each bit is ~50/50); content that
/// genuinely repeats will drive the mean toward 0.
const AUTOCORR_SCORE_THRESHOLD: f64 = 12.0;

/// Expected mean bit-error for random, unrelated fingerprint items (32 bits × 0.5).
const AUTOCORR_BASELINE: f64 = 16.0;

/// Minimum comparison-window size (items) for a statistically reliable estimate.
const MIN_OVERLAP_ITEMS: usize = 50;

/// Detects internal repetition within a single prepared clip using sliding fingerprint
/// autocorrelation.
///
/// For each candidate lag `L` in `[MIN_LAG_ITEMS, N − MIN_OVERLAP_ITEMS]` the mean Hamming
/// distance between `fp[0..N-L]` and `fp[L..N]` is computed. If the minimum across all lags
/// falls below [`AUTOCORR_SCORE_THRESHOLD`], the repeat is reported with a confidence derived
/// from how far the score falls below the random baseline.
///
/// Unlike the previous half-vs-half approach, this detects repeats at any lag within the clip,
/// covering the scenario where a recording restarts part-way through (a short loop).
pub(crate) fn detect_clip_repetition(
    fingerprint: &Fingerprint,
    clip_duration_secs: f64,
    preset: ChromaprintPreset,
    min_confidence: f32,
) -> Option<RepetitionFinding> {
    if fingerprint.data.is_empty() {
        return None;
    }

    let config = configuration_for_preset(preset);
    let item_secs = f64::from(config.item_duration_in_seconds());

    // Clip must be long enough to test at least MIN_LAG_ITEMS with MIN_OVERLAP_ITEMS remaining.
    let min_clip_secs = (MIN_LAG_ITEMS + MIN_OVERLAP_ITEMS) as f64 * item_secs;
    if clip_duration_secs < min_clip_secs {
        return None;
    }

    let n = fingerprint.data.len();
    let max_lag = n.saturating_sub(MIN_OVERLAP_ITEMS);
    if max_lag < MIN_LAG_ITEMS {
        return None;
    }

    let mut best_lag = 0_usize;
    let mut best_score = f64::MAX;

    for lag in MIN_LAG_ITEMS..=max_lag {
        let overlap = n - lag;
        let mean_errors: f64 = fingerprint.data[..overlap]
            .iter()
            .zip(&fingerprint.data[lag..])
            .map(|(a, b)| f64::from((a ^ b).count_ones()))
            .sum::<f64>()
            / overlap as f64;

        if mean_errors < best_score {
            best_score = mean_errors;
            best_lag = lag;
        }
    }

    if best_score >= AUTOCORR_SCORE_THRESHOLD {
        return None;
    }

    let overlap = n - best_lag;
    let confidence = autocorr_confidence(best_score, overlap);
    if confidence < min_confidence {
        return None;
    }

    Some(RepetitionFinding {
        lag_secs: best_lag as f64 * item_secs,
        confidence,
        items_count: overlap,
    })
}

fn autocorr_confidence(mean_bit_error: f64, overlap_items: usize) -> f32 {
    let score_conf =
        ((AUTOCORR_BASELINE - mean_bit_error) / AUTOCORR_BASELINE).clamp(0.0, 1.0) as f32;
    let length_conf = (overlap_items as f32 / MIN_OVERLAP_ITEMS as f32).clamp(0.0, 1.0);
    (score_conf * length_conf).sqrt()
}

#[cfg(test)]
mod tests {
    use std::f32::consts::TAU;

    use super::*;
    use crate::application::ports::Fingerprinter;
    use crate::domain::{prepare_clip_for_fingerprint, MonoPcmClip, PcmPreparationOptions};
    use crate::infrastructure::chromaprint::ChromaprintFingerprinter;

    const SAMPLE_RATE: u32 = 44_100;
    const MIN_CONFIDENCE: f32 = 0.5;

    fn tone_samples(sample_rate: u32, start_index: u64, count: usize) -> Vec<i16> {
        (0..count)
            .map(|offset| {
                let index = start_index + offset as u64;
                let t = index as f32 / sample_rate as f32;
                ((TAU * 440.0 * t).sin() * (i16::MAX as f32 * 0.5)).round() as i16
            })
            .collect()
    }

    fn chirp_samples(sample_rate: u32, start_index: u64, count: usize) -> Vec<i16> {
        (0..count)
            .map(|offset| {
                let index = start_index + offset as u64;
                let t = index as f64 / f64::from(sample_rate);
                let freq = 300.0 + 400.0 * t;
                ((TAU as f64 * freq * t).sin() * (i16::MAX as f64 * 0.5)).round() as i16
            })
            .collect()
    }

    fn fingerprint_prepared(clip: &MonoPcmClip) -> Fingerprint {
        let prepared =
            prepare_clip_for_fingerprint(clip, PcmPreparationOptions::default()).unwrap();
        ChromaprintFingerprinter::default()
            .fingerprint(&prepared)
            .unwrap()
    }

    /// 60s clip: 10s 440 Hz tone at 0s and 30s, silence elsewhere. Lag = 30s = T/2.
    fn midpoint_repeat_clip() -> MonoPcmClip {
        let total = SAMPLE_RATE as usize * 60;
        let block = SAMPLE_RATE as usize * 10;
        let repeat_at = SAMPLE_RATE as usize * 30;
        let mut samples = vec![0_i16; total];
        let tone = tone_samples(SAMPLE_RATE, 0, block);
        samples[..block].copy_from_slice(&tone);
        samples[repeat_at..repeat_at + block].copy_from_slice(&tone);
        MonoPcmClip {
            sample_rate: SAMPLE_RATE,
            samples,
            decode_error_skips: 0,
            decoded_sample_count: None,
        }
    }

    /// 60s clip: 5s 440 Hz tone at 0s and 25s, silence elsewhere.
    /// Lag = 25s; midpoint = 30s; |25 − 30| = 5s exceeds the old 3s guard.
    fn non_midpoint_repeat_clip() -> MonoPcmClip {
        let total = SAMPLE_RATE as usize * 60;
        let block = SAMPLE_RATE as usize * 5;
        let repeat_at = SAMPLE_RATE as usize * 25;
        let mut samples = vec![0_i16; total];
        let tone = tone_samples(SAMPLE_RATE, 0, block);
        samples[..block].copy_from_slice(&tone);
        samples[repeat_at..repeat_at + block].copy_from_slice(&tone);
        MonoPcmClip {
            sample_rate: SAMPLE_RATE,
            samples,
            decode_error_skips: 0,
            decoded_sample_count: None,
        }
    }

    fn monotonic_chirp_clip(total_secs: u32) -> MonoPcmClip {
        MonoPcmClip {
            sample_rate: SAMPLE_RATE,
            samples: chirp_samples(SAMPLE_RATE, 0, SAMPLE_RATE as usize * total_secs as usize),
            decode_error_skips: 0,
            decoded_sample_count: None,
        }
    }

    #[test]
    fn detect_clip_repetition_finds_copied_block() {
        let clip = midpoint_repeat_clip();
        let duration = clip.samples.len() as f64 / f64::from(SAMPLE_RATE);
        let fp = fingerprint_prepared(&clip);

        let finding =
            detect_clip_repetition(&fp, duration, ChromaprintPreset::default(), MIN_CONFIDENCE)
                .expect("expected repetition finding");

        assert!(
            (finding.lag_secs - 30.0).abs() <= 3.0,
            "lag_secs={} (expected ~30s)",
            finding.lag_secs
        );
        assert!(finding.confidence >= MIN_CONFIDENCE, "confidence={}", finding.confidence);
    }

    #[test]
    fn detect_clip_repetition_finds_repeat_at_non_midpoint_lag() {
        // Tone at 0s and 25s → lag = 25s, clip midpoint = 30s, |25 − 30| = 5s.
        // The old half-vs-half algorithm's ±3s midpoint guard would reject this; autocorrelation
        // finds it regardless of where the lag falls within the clip.
        let clip = non_midpoint_repeat_clip();
        let fp = fingerprint_prepared(&clip);

        let finding =
            detect_clip_repetition(&fp, 60.0, ChromaprintPreset::default(), MIN_CONFIDENCE)
                .expect("expected repetition finding at ~25s");

        assert!(
            (finding.lag_secs - 25.0).abs() <= 2.0,
            "lag_secs={} expected ~25s",
            finding.lag_secs
        );
        assert!(finding.confidence >= MIN_CONFIDENCE, "confidence={}", finding.confidence);
    }

    #[test]
    fn detect_clip_repetition_none_on_chirp() {
        let clip = monotonic_chirp_clip(60);
        let fp = fingerprint_prepared(&clip);

        assert!(
            detect_clip_repetition(&fp, 60.0, ChromaprintPreset::default(), MIN_CONFIDENCE)
                .is_none(),
            "monotonic chirp should not trigger repetition detection"
        );
    }

    #[test]
    fn detect_clip_repetition_none_on_empty_fingerprint() {
        let empty = Fingerprint { data: vec![] };
        assert!(
            detect_clip_repetition(&empty, 60.0, ChromaprintPreset::default(), MIN_CONFIDENCE)
                .is_none()
        );
    }

    #[test]
    fn detect_clip_repetition_none_when_too_short() {
        let clip = midpoint_repeat_clip();
        let fp = fingerprint_prepared(&clip);
        // Duration below (MIN_LAG_ITEMS + MIN_OVERLAP_ITEMS) * item_secs triggers the early exit.
        assert!(
            detect_clip_repetition(&fp, 8.0, ChromaprintPreset::default(), MIN_CONFIDENCE)
                .is_none(),
            "clip duration too short for reliable autocorrelation"
        );
    }
}
