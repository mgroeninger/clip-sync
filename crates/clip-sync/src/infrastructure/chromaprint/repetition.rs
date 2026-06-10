use rusty_chromaprint::match_fingerprints;

use crate::application::config::ChromaprintPreset;
use crate::domain::{Fingerprint, RepetitionFinding};
use crate::infrastructure::chromaprint::config::configuration_for_preset;
use crate::infrastructure::chromaprint::matching::{
    segment_offset_items, select_best_nonzero_lag_segment,
};

/// Minimum internal lag (fingerprint items) before a segment counts as repetition.
const MIN_LAG_ITEMS: usize = 40;

/// Half-vs-half maps repeats near the clip midpoint; allow ±3 s tolerance.
const HALF_LAG_MIDPOINT_TOLERANCE_SECS: f64 = 3.0;

/// Detects internal repetition within a single prepared clip using half-vs-half fingerprint
/// comparison.
///
/// Limitations: detects repeats aligned near the clip midpoint. Repeats at other lags, or in
/// fingerprint-quiet segments (e.g. chirp-heavy audio), are not detected by this method.
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
    let min_lag_secs = item_secs * MIN_LAG_ITEMS as f64;

    if clip_duration_secs < 2.0 * min_lag_secs {
        return None;
    }

    let mid_secs = clip_duration_secs / 2.0;
    let mid_item = (mid_secs / item_secs).round() as usize;
    if mid_item == 0 || mid_item >= fingerprint.data.len() {
        return None;
    }

    let (left, right) = fingerprint.data.split_at(mid_item);
    let segments = match_fingerprints(left, right, &config).ok()?;

    let (segment, _ambiguous, confidence) =
        select_best_nonzero_lag_segment(&segments, min_confidence)?;

    let lag_items = segment_offset_items(segment).unsigned_abs() as f64;
    let lag_secs = mid_secs + lag_items * item_secs;

    if (lag_secs - mid_secs).abs() > HALF_LAG_MIDPOINT_TOLERANCE_SECS {
        return None;
    }

    Some(RepetitionFinding {
        lag_secs,
        confidence,
        items_count: segment.items_count,
    })
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

    /// 60s clip: 10s 440 Hz tone at 0s, same block copied to 30s, silence elsewhere.
    fn repeated_segment_clip() -> MonoPcmClip {
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
        let clip = repeated_segment_clip();
        let duration = clip.samples.len() as f64 / f64::from(SAMPLE_RATE);
        let fp = fingerprint_prepared(&clip);

        let finding = detect_clip_repetition(&fp, duration, ChromaprintPreset::default(), MIN_CONFIDENCE)
            .expect("expected repetition finding");

        assert!(
            (finding.lag_secs - 30.0).abs() <= 3.0,
            "lag_secs={} (expected ~30s)",
            finding.lag_secs
        );
        assert!(finding.confidence >= MIN_CONFIDENCE, "confidence={}", finding.confidence);
    }

    #[test]
    fn detect_clip_repetition_none_on_chirp() {
        let clip = monotonic_chirp_clip(60);
        let duration = 60.0_f64;
        let fp = fingerprint_prepared(&clip);

        assert!(
            detect_clip_repetition(&fp, duration, ChromaprintPreset::default(), MIN_CONFIDENCE).is_none(),
            "monotonic chirp should not trigger repetition detection"
        );
    }

    #[test]
    fn detect_clip_repetition_none_on_empty_fingerprint() {
        let empty = Fingerprint { data: vec![] };
        assert!(detect_clip_repetition(&empty, 60.0, ChromaprintPreset::default(), MIN_CONFIDENCE).is_none());
    }

    #[test]
    fn detect_clip_repetition_none_when_too_short() {
        let clip = repeated_segment_clip();
        // Use a duration below 2 * min_lag_secs (~10s); actual samples don't matter for this guard.
        let fp = fingerprint_prepared(&clip);
        assert!(
            detect_clip_repetition(&fp, 8.0, ChromaprintPreset::default(), MIN_CONFIDENCE).is_none(),
            "clip too short for half-vs-half detection"
        );
    }
}
