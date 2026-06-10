use crate::application::config::ChromaprintPreset;
use crate::domain::{Fingerprint, RepetitionFinding};
use crate::infrastructure::chromaprint::config::configuration_for_preset;

#[cfg(test)]
use std::cell::Cell;

#[cfg(test)]
thread_local! {
    static DETECT_COUNTING_ENABLED: Cell<bool> = const { Cell::new(false) };
    static DETECT_CLIP_REPETITION_CALLS: Cell<usize> = const { Cell::new(0) };
}

/// Enables per-thread detect-call counting and resets the counter. Only the test thread that
/// called this will record `detect_clip_repetition` invocations (safe under parallel `cargo test`).
#[cfg(test)]
pub(crate) fn test_reset_repetition_detect_calls() {
    DETECT_CLIP_REPETITION_CALLS.with(|counter| counter.set(0));
    DETECT_COUNTING_ENABLED.with(|enabled| enabled.set(true));
}

#[cfg(test)]
pub(crate) fn test_repetition_detect_calls() -> usize {
    let count = DETECT_CLIP_REPETITION_CALLS.with(|counter| counter.get());
    DETECT_COUNTING_ENABLED.with(|enabled| enabled.set(false));
    count
}

/// Minimum candidate lag (fingerprint items) to skip trivial near-zero-lag matches.
const MIN_LAG_ITEMS: usize = 40;

/// Maximum mean Hamming distance (bits out of 32) below which a lag is treated as a genuine
/// repeat. The expected value for unrelated audio is ~16 (each bit is ~50/50); content that
/// genuinely repeats will drive the mean toward 0.
/// Repeat candidates must beat this mean bit-error (random baseline is ~16).
const AUTOCORR_SCORE_THRESHOLD: f64 = 2.0;

/// Expected mean bit-error for random, unrelated fingerprint items (32 bits × 0.5).
const AUTOCORR_BASELINE: f64 = 16.0;

/// Minimum comparison-window size (items) for a statistically reliable estimate.
const MIN_OVERLAP_ITEMS: usize = 50;

/// Trailing trim must remove at least this much audio before short-lag artifact rejection applies.
const TAIL_TRIM_DETECT_MIN_SECS: f64 = 2.0;

/// Lags within `MIN_LAG_ITEMS + margin` after significant tail trim are often tone/silence edge artifacts.
const SHORT_LAG_AFTER_TRIM_ITEM_MARGIN: usize = 15;

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
    prepared_duration_secs: f64,
    preset: ChromaprintPreset,
    min_confidence: f32,
    source_duration_secs: f64,
) -> Option<RepetitionFinding> {
    #[cfg(test)]
    DETECT_COUNTING_ENABLED.with(|enabled| {
        if enabled.get() {
            DETECT_CLIP_REPETITION_CALLS.with(|counter| counter.set(counter.get() + 1));
        }
    });

    if fingerprint.data.is_empty() {
        return None;
    }

    let config = configuration_for_preset(preset);
    let item_secs = f64::from(config.item_duration_in_seconds());

    // Clip must be long enough to test at least MIN_LAG_ITEMS with MIN_OVERLAP_ITEMS remaining.
    let min_clip_secs = (MIN_LAG_ITEMS + MIN_OVERLAP_ITEMS) as f64 * item_secs;
    if prepared_duration_secs < min_clip_secs {
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

    if is_short_lag_after_tail_trim(
        best_lag,
        source_duration_secs,
        prepared_duration_secs,
    ) {
        return None;
    }

    let overlap = n - best_lag;
    let confidence = autocorr_confidence(best_score);
    if confidence < min_confidence {
        return None;
    }

    Some(RepetitionFinding {
        lag_secs: best_lag as f64 * item_secs,
        confidence,
        items_count: overlap,
    })
}

fn autocorr_confidence(mean_bit_error: f64) -> f32 {
    let score_conf =
        ((AUTOCORR_BASELINE - mean_bit_error) / AUTOCORR_BASELINE).clamp(0.0, 1.0) as f32;
    score_conf.sqrt()
}

fn is_short_lag_after_tail_trim(
    best_lag: usize,
    source_duration_secs: f64,
    prepared_duration_secs: f64,
) -> bool {
    let tail_trimmed =
        source_duration_secs - prepared_duration_secs >= TAIL_TRIM_DETECT_MIN_SECS;
    let short_lag =
        best_lag <= MIN_LAG_ITEMS.saturating_add(SHORT_LAG_AFTER_TRIM_ITEM_MARGIN);
    tail_trimmed && short_lag
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

    fn detect(
        fp: &Fingerprint,
        prepared_secs: f64,
        min_confidence: f32,
        source_secs: f64,
    ) -> Option<RepetitionFinding> {
        detect_clip_repetition(
            fp,
            prepared_secs,
            ChromaprintPreset::default(),
            min_confidence,
            source_secs,
        )
    }

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

    fn fingerprint_with_prep(clip: &MonoPcmClip, prep: PcmPreparationOptions) -> Fingerprint {
        let prepared = prepare_clip_for_fingerprint(clip, prep).unwrap();
        ChromaprintFingerprinter::default()
            .fingerprint(&prepared)
            .unwrap()
    }

    /// Full clip fingerprinted without trailing-silence trim (isolates detection from prep).
    fn fingerprint_untrimmed(clip: &MonoPcmClip) -> Fingerprint {
        fingerprint_with_prep(
            clip,
            PcmPreparationOptions {
                trim_silence: false,
                ..Default::default()
            },
        )
    }

    /// Matches default align prep: trim trailing silence + peak normalize.
    fn fingerprint_production_like(clip: &MonoPcmClip) -> Fingerprint {
        fingerprint_with_prep(clip, PcmPreparationOptions::default())
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

    /// 60s clip: 10s 440 Hz tone at 0s and 15s, silence elsewhere.
    /// Lag = 15s; midpoint = 30s; |15 − 30| = 15s exceeds the old 3s guard.
    /// Sized for default trim: repeat lag stays within searchable range after trailing trim.
    fn non_midpoint_repeat_clip() -> MonoPcmClip {
        let total = SAMPLE_RATE as usize * 60;
        let block = SAMPLE_RATE as usize * 10;
        let repeat_at = SAMPLE_RATE as usize * 15;
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

    /// 60s clip: 5s tone at 0s and 25s — true lag exceeds max_lag after default trim.
    fn distant_repeat_clip() -> MonoPcmClip {
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
        let fp = fingerprint_untrimmed(&clip);

        let finding = detect(&fp, duration, MIN_CONFIDENCE, duration).expect("expected repetition finding");

        assert!(
            (finding.lag_secs - 30.0).abs() <= 1.0,
            "lag_secs={} (expected ~30s)",
            finding.lag_secs
        );
        assert!(finding.confidence >= MIN_CONFIDENCE, "confidence={}", finding.confidence);
    }

    #[test]
    fn detect_clip_repetition_finds_repeat_at_non_midpoint_lag() {
        // Tone at 0s and 15s → lag = 15s. Autocorrelation finds repeats away from the midpoint.
        let clip = non_midpoint_repeat_clip();
        let fp = fingerprint_untrimmed(&clip);

        let finding = detect(&fp, 60.0, MIN_CONFIDENCE, 60.0).expect("expected repetition finding at ~15s");

        assert!(
            (finding.lag_secs - 15.0).abs() <= 2.0,
            "lag_secs={} expected ~15s",
            finding.lag_secs
        );
        assert!(finding.confidence >= MIN_CONFIDENCE, "confidence={}", finding.confidence);
    }

    #[test]
    fn detect_clip_repetition_finds_non_midpoint_with_production_prep() {
        let clip = non_midpoint_repeat_clip();
        let source_secs = clip.duration_secs();
        let prepared =
            prepare_clip_for_fingerprint(&clip, PcmPreparationOptions::default()).unwrap();
        let fp = ChromaprintFingerprinter::default()
            .fingerprint(&prepared)
            .unwrap();

        let finding = detect(
            &fp,
            prepared.duration_secs(),
            MIN_CONFIDENCE,
            source_secs,
        )
        .expect("expected repetition finding with default prep");

        assert!(
            (finding.lag_secs - 15.0).abs() <= 2.0,
            "lag_secs={} expected ~15s",
            finding.lag_secs
        );
    }

    #[test]
    fn detect_clip_repetition_trim_shortens_max_searchable_lag() {
        let clip = distant_repeat_clip();
        let config = configuration_for_preset(ChromaprintPreset::default());
        let item_secs = f64::from(config.item_duration_in_seconds());
        let true_lag_items = (25.0_f64 / item_secs).round() as usize;

        let fp_untrimmed = fingerprint_untrimmed(&clip);
        let max_lag_untrimmed = fp_untrimmed.data.len().saturating_sub(MIN_OVERLAP_ITEMS);
        assert!(
            true_lag_items <= max_lag_untrimmed,
            "fixture: true lag {true_lag_items} must be searchable without trim (max_lag={max_lag_untrimmed})"
        );

        let fp_trimmed = fingerprint_production_like(&clip);
        let max_lag_trimmed = fp_trimmed.data.len().saturating_sub(MIN_OVERLAP_ITEMS);
        assert!(
            true_lag_items > max_lag_trimmed,
            "trim must push true lag {true_lag_items} beyond max_lag {max_lag_trimmed}"
        );
    }

    #[test]
    fn detect_clip_repetition_rejects_short_lag_after_tail_trim() {
        // True 25s repeat is unsearchable after trim; short-lag tone/silence edges must not report.
        let clip = distant_repeat_clip();
        let source_secs = clip.duration_secs();
        let prepared =
            prepare_clip_for_fingerprint(&clip, PcmPreparationOptions::default()).unwrap();
        let fp = ChromaprintFingerprinter::default()
            .fingerprint(&prepared)
            .unwrap();
        assert!(
            detect(
                &fp,
                prepared.duration_secs(),
                MIN_CONFIDENCE,
                source_secs,
            )
            .is_none(),
            "short-lag detections after significant tail trim are edge artifacts"
        );
    }

    #[test]
    fn detect_clip_repetition_rejects_when_min_confidence_too_high() {
        let clip = midpoint_repeat_clip();
        let fp = fingerprint_untrimmed(&clip);
        let preset = ChromaprintPreset::default();

        assert!(
            detect(&fp, 60.0, MIN_CONFIDENCE, 60.0).is_some(),
            "default min_confidence should accept a strong repeat"
        );

        let finding = detect(&fp, 60.0, 0.0, 60.0).expect("fixture must produce a repetition finding");
        let threshold = finding.confidence.next_up();
        assert!(
            threshold > finding.confidence,
            "fixture confidence ({}) must be below f32::MAX to exercise the gate",
            finding.confidence
        );
        assert!(
            detect_clip_repetition(&fp, 60.0, preset, threshold, 60.0).is_none(),
            "threshold just above fixture confidence ({}) should reject",
            finding.confidence
        );
    }

    #[test]
    fn detect_clip_repetition_none_on_varied_content() {
        // Algorithm sanity: pseudorandom fingerprints have no repeat structure.
        let n_items = 463; // ~57s at item_secs ≈ 0.124s
        let mut x = 0xDEAD_BEEF_u32;
        let data: Vec<u32> = (0..n_items)
            .map(|_| {
                x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                x
            })
            .collect();
        let fp = Fingerprint { data };

        let config = configuration_for_preset(ChromaprintPreset::default());
        let item_secs = f64::from(config.item_duration_in_seconds());
        let duration = n_items as f64 * item_secs;

        assert!(
            detect(&fp, duration, MIN_CONFIDENCE, duration).is_none(),
            "pseudorandom fingerprint should not trigger detection"
        );
    }

    #[test]
    fn detect_clip_repetition_none_on_monotonic_chirp() {
        // End-to-end negative control: Chromaprint can collapse above-range chirp energy into
        // near-identical items (~41s false peak). Measured on Test2/default: global min bit_error
        // ≈ 2.51 vs AUTOCORR_SCORE_THRESHOLD 2.0 (~0.5 bit margin). Re-measure if upgrading
        // rusty-chromaprint or changing presets.
        let clip = monotonic_chirp_clip(60);
        let fp = fingerprint_untrimmed(&clip);

        assert!(
            detect(&fp, 60.0, MIN_CONFIDENCE, 60.0).is_none(),
            "monotonic chirp should not trigger repetition detection"
        );
    }

    #[test]
    fn detect_clip_repetition_corpus_repeated_segment_fixture() {
        use crate::application::testing::audio_fixtures::write_repeated_segment_wav_pair;
        use hound::WavReader;

        let temp = tempfile::tempdir().expect("tempdir");
        let (path_a, path_b) =
            write_repeated_segment_wav_pair(temp.path(), 11_025, 65, 3);

        let read_clip = |path: &std::path::Path| -> MonoPcmClip {
            let mut reader = WavReader::open(path).expect("wav");
            let samples: Vec<i16> = reader
                .samples()
                .take(11_025 * 60)
                .map(|sample| sample.expect("sample"))
                .collect();
            MonoPcmClip {
                sample_rate: 11_025,
                samples,
                decode_error_skips: 0,
                decoded_sample_count: None,
            }
        };

        let corpus_prep = PcmPreparationOptions {
            normalize_loudness: false,
            trim_silence: false,
            window_slide_secs: 0,
        };

        for (label, clip) in [("A", read_clip(&path_a)), ("B", read_clip(&path_b))] {
            let source_secs = clip.duration_secs();
            let prepared = prepare_clip_for_fingerprint(&clip, corpus_prep).unwrap();
            let fp = ChromaprintFingerprinter::default()
                .fingerprint(&prepared)
                .unwrap();
            let finding = detect(
                &fp,
                prepared.duration_secs(),
                MIN_CONFIDENCE,
                source_secs,
            );
            eprintln!("{label}: finding={finding:?}");
            let finding = finding.unwrap_or_else(|| {
                panic!("{label}: expected repetition finding on corpus repeated_segment fixture")
            });
            assert!(
                (28.0..=32.0).contains(&finding.lag_secs),
                "{label}: lag_secs={} expected [28, 32]",
                finding.lag_secs
            );
        }
    }

    #[test]
    fn detect_clip_repetition_none_when_too_short_or_empty() {
        let empty = Fingerprint { data: vec![] };
        assert!(
            detect(&empty, 60.0, MIN_CONFIDENCE, 60.0).is_none(),
            "empty fingerprint must not trigger detection"
        );

        let clip = midpoint_repeat_clip();
        let fp = fingerprint_untrimmed(&clip);
        assert!(
            detect(&fp, 8.0, MIN_CONFIDENCE, 8.0).is_none(),
            "clip duration too short for reliable autocorrelation"
        );
    }
}
