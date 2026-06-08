//! Phase 0 spike: validate Chromaprint approaches for internal repetition detection.
//!
//! Outcome (2026-06-06): **full self-match rejected**; **half-vs-half with midpoint guard** passes
//! spike tests. See [docs/TEMP-clip-self-repetition-plan.md](../../../docs/TEMP-clip-self-repetition-plan.md).
//! Phase 1 promotes helpers into `matching.rs` and `repetition.rs`.

use rusty_chromaprint::{match_fingerprints, Segment};

use crate::application::config::ChromaprintPreset;
use crate::domain::Fingerprint;
use crate::infrastructure::chromaprint::config::{
    configuration_for_preset, MATCH_SCORE_THRESHOLD, MIN_RELIABLE_ITEMS,
};

/// Minimum internal lag (fingerprint items) before a segment counts as repetition.
pub const MIN_LAG_ITEMS: isize = 40;

/// Half-vs-half maps repeats that start near the midpoint; allow ±3 s on a 60 s clip.
pub const HALF_LAG_MIDPOINT_TOLERANCE_SECS: f64 = 3.0;

#[derive(Debug, Clone, PartialEq)]
pub struct SelfMatchFinding {
    pub lag_secs: f64,
    pub confidence: f32,
    pub items_count: usize,
    pub ambiguous: bool,
    pub method: SelfMatchMethod,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelfMatchMethod {
    /// `match_fingerprints(&fp, &fp)` with non-zero lag cluster — **not viable** (library returns lag 0 only).
    FullSelfMatch,
    /// Fingerprint first/second half item split; repeat at timeline midpoint surfaces as non-zero lag.
    HalfVsHalf,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SelfMatchSpikeOutcome {
    pub item_duration_secs: f32,
    pub min_lag_secs: f64,
    pub full_self_match: Option<SelfMatchFinding>,
    pub half_vs_half: Option<SelfMatchFinding>,
    pub recommended: Option<SelfMatchFinding>,
}

pub fn item_duration_secs_for_preset(preset: ChromaprintPreset) -> f32 {
    configuration_for_preset(preset).item_duration_in_seconds()
}

pub fn min_lag_secs_for_preset(preset: ChromaprintPreset) -> f64 {
    f64::from(item_duration_secs_for_preset(preset)) * MIN_LAG_ITEMS as f64
}

/// Phase 0 spike entry point. Phase 1 `detect_clip_repetition` will replace this API.
pub fn spike_detect_internal_repeat(
    fingerprint: &Fingerprint,
    clip_duration_secs: f64,
    preset: ChromaprintPreset,
    min_confidence: f32,
) -> SelfMatchSpikeOutcome {
    let item_secs = item_duration_secs_for_preset(preset);
    let min_lag_secs = min_lag_secs_for_preset(preset);

    let full_self_match = detect_via_full_self_match(fingerprint, preset, min_confidence);
    let half_vs_half = if clip_duration_secs >= 2.0 * min_lag_secs {
        detect_via_half_vs_half(fingerprint, clip_duration_secs, preset, min_confidence)
    } else {
        None
    };

    let recommended = half_vs_half.clone();

    SelfMatchSpikeOutcome {
        item_duration_secs: item_secs,
        min_lag_secs,
        full_self_match,
        half_vs_half,
        recommended,
    }
}

fn detect_via_full_self_match(
    fingerprint: &Fingerprint,
    preset: ChromaprintPreset,
    min_confidence: f32,
) -> Option<SelfMatchFinding> {
    if fingerprint.data.is_empty() {
        return None;
    }

    let config = configuration_for_preset(preset);
    let segments = match_fingerprints(&fingerprint.data, &fingerprint.data, &config).ok()?;
    let (segment, ambiguous, confidence) =
        select_best_nonzero_lag_segment(&segments, min_confidence)?;

    let item_secs = f64::from(config.item_duration_in_seconds());
    let lag_secs = segment_offset_items(segment).unsigned_abs() as f64 * item_secs;

    Some(SelfMatchFinding {
        lag_secs,
        confidence,
        items_count: segment.items_count,
        ambiguous,
        method: SelfMatchMethod::FullSelfMatch,
    })
}

fn detect_via_half_vs_half(
    fingerprint: &Fingerprint,
    clip_duration_secs: f64,
    preset: ChromaprintPreset,
    min_confidence: f32,
) -> Option<SelfMatchFinding> {
    if fingerprint.data.is_empty() {
        return None;
    }

    let mid_secs = clip_duration_secs / 2.0;
    let config = configuration_for_preset(preset);
    let item_secs = f64::from(config.item_duration_in_seconds());
    let mid_item = (mid_secs / item_secs).round() as usize;
    if mid_item == 0 || mid_item >= fingerprint.data.len() {
        return None;
    }

    let (left, right) = fingerprint.data.split_at(mid_item);
    let segments = match_fingerprints(left, right, &config).ok()?;
    let (segment, ambiguous, confidence) =
        select_best_nonzero_lag_segment(&segments, min_confidence)?;

    let lag_items = segment_offset_items(segment).unsigned_abs() as f64;
    let lag_secs = mid_secs + lag_items * item_secs;

    if (lag_secs - mid_secs).abs() > HALF_LAG_MIDPOINT_TOLERANCE_SECS {
        return None;
    }

    Some(SelfMatchFinding {
        lag_secs,
        confidence,
        items_count: segment.items_count,
        ambiguous,
        method: SelfMatchMethod::HalfVsHalf,
    })
}

pub(crate) fn segment_offset_items(segment: &Segment) -> isize {
    segment.offset2 as isize - segment.offset1 as isize
}

pub(crate) fn select_best_nonzero_lag_segment(
    segments: &[Segment],
    min_confidence: f32,
) -> Option<(&Segment, bool, f32)> {
    let nonzero: Vec<&Segment> = segments
        .iter()
        .filter(|segment| segment_offset_items(segment).unsigned_abs() > 1)
        .collect();

    let (segment, ambiguous) = select_best_segment(&nonzero)?;
    let confidence = segment_confidence(segment.score, segment.items_count, ambiguous);
    if confidence < min_confidence {
        return None;
    }

    Some((segment, ambiguous, confidence))
}

fn select_best_segment<'a>(segments: &[&'a Segment]) -> Option<(&'a Segment, bool)> {
    if segments.is_empty() {
        return None;
    }

    let mut clusters: Vec<(isize, f64, usize, &Segment)> = Vec::new();

    for segment in segments {
        let offset_items = segment_offset_items(segment);
        if let Some(cluster) = clusters
            .iter_mut()
            .find(|(offset, _, _, _)| (*offset - offset_items).abs() <= 1)
        {
            cluster.1 += segment.items_count as f64 / (segment.score + 1.0);
            cluster.2 += segment.items_count;
            if segment.score < cluster.3.score
                || (segment.score == cluster.3.score
                    && segment.items_count > cluster.3.items_count)
            {
                cluster.3 = segment;
            }
        } else {
            clusters.push((
                offset_items,
                segment.items_count as f64 / (segment.score + 1.0),
                segment.items_count,
                segment,
            ));
        }
    }

    clusters.sort_by(|left, right| {
        right
            .1
            .partial_cmp(&left.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| right.2.cmp(&left.2))
            .then_with(|| {
                left.3
                    .score
                    .partial_cmp(&right.3.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });

    let best = clusters.first()?;
    let ambiguous = clusters.len() > 1
        && clusters[1].1 >= best.1 * 0.75
        && (clusters[1].0 - best.0).abs() > 2;

    Some((best.3, ambiguous))
}

pub(crate) fn segment_confidence(score: f64, items_count: usize, ambiguous: bool) -> f32 {
    if score >= MATCH_SCORE_THRESHOLD {
        return 0.0;
    }

    let score_conf =
        ((MATCH_SCORE_THRESHOLD - score) / MATCH_SCORE_THRESHOLD).clamp(0.0, 1.0) as f32;
    let length_conf = (items_count as f32 / MIN_RELIABLE_ITEMS as f32).clamp(0.0, 1.0);
    let mut confidence = (score_conf * length_conf).sqrt();

    if ambiguous {
        confidence *= 0.5;
    }

    confidence
}

#[cfg(test)]
mod tests {
    use std::f32::consts::TAU;

    use super::*;
    use crate::application::config::ChromaprintPreset;
    use crate::application::ports::Fingerprinter;
    use crate::domain::{prepare_clip_for_fingerprint, Fingerprint, MonoPcmClip, PcmPreparationOptions};
    use crate::infrastructure::chromaprint::ChromaprintFingerprinter;

    const SAMPLE_RATE: u32 = 44_100;
    const BLOCK_SECS: u32 = 10;
    const REPEAT_AT_SECS: u32 = 30;
    const TOTAL_SECS: u32 = 60;
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

    /// 60s clip: 10s tone @ 0s, same 10s @ 30s, silence elsewhere.
    fn repeated_segment_clip_silent_gaps() -> MonoPcmClip {
        let total_samples = SAMPLE_RATE as usize * TOTAL_SECS as usize;
        let block_samples = SAMPLE_RATE as usize * BLOCK_SECS as usize;
        let repeat_at = SAMPLE_RATE as usize * REPEAT_AT_SECS as usize;

        let mut samples = vec![0_i16; total_samples];
        let block = tone_samples(SAMPLE_RATE, 0, block_samples);
        samples[..block_samples].copy_from_slice(&block);
        samples[repeat_at..repeat_at + block_samples].copy_from_slice(&block);

        MonoPcmClip {
            sample_rate: SAMPLE_RATE,
            samples,
            decode_error_skips: 0,
            decoded_sample_count: None,
        }
    }

    fn monotonic_chirp_clip() -> MonoPcmClip {
        MonoPcmClip {
            sample_rate: SAMPLE_RATE,
            samples: chirp_samples(SAMPLE_RATE, 0, SAMPLE_RATE as usize * TOTAL_SECS as usize),
            decode_error_skips: 0,
            decoded_sample_count: None,
        }
    }

    fn fingerprint_prepared(clip: &MonoPcmClip) -> Fingerprint {
        let prepared =
            prepare_clip_for_fingerprint(clip, PcmPreparationOptions::default()).unwrap();
        ChromaprintFingerprinter::default()
            .fingerprint(&prepared)
            .unwrap()
    }

    #[test]
    fn default_preset_item_duration_and_min_lag_secs() {
        let item_secs = item_duration_secs_for_preset(ChromaprintPreset::default());
        assert!(
            (0.1..=0.2).contains(&item_secs),
            "unexpected item duration: {item_secs}s"
        );

        let min_lag = min_lag_secs_for_preset(ChromaprintPreset::default());
        assert!(
            (4.0..=8.0).contains(&min_lag),
            "min_lag_secs={min_lag} (40 items × item duration)"
        );
    }

    #[test]
    fn full_self_match_returns_only_lag_zero() {
        let fp = fingerprint_prepared(&repeated_segment_clip_silent_gaps());
        let config = configuration_for_preset(ChromaprintPreset::default());
        let segments = match_fingerprints(&fp.data, &fp.data, &config).unwrap();

        assert!(
            segments
                .iter()
                .all(|segment| segment_offset_items(segment).unsigned_abs() <= 1),
            "rusty-chromaprint self-match should collapse to lag ≈ 0"
        );
        assert!(
            spike_detect_internal_repeat(
                &fp,
                TOTAL_SECS as f64,
                ChromaprintPreset::default(),
                MIN_CONFIDENCE
            )
            .full_self_match
            .is_none()
        );
    }

    #[test]
    fn half_vs_half_finds_repeat_near_30s() {
        let clip = repeated_segment_clip_silent_gaps();
        let fp = fingerprint_prepared(&clip);
        let duration = clip.samples.len() as f64 / f64::from(SAMPLE_RATE);

        let outcome =
            spike_detect_internal_repeat(&fp, duration, ChromaprintPreset::default(), MIN_CONFIDENCE);

        let finding = outcome
            .half_vs_half
            .as_ref()
            .or(outcome.recommended.as_ref())
            .expect("expected half-vs-half to detect ~30s repeat");

        assert_eq!(finding.method, SelfMatchMethod::HalfVsHalf);
        assert!(finding.confidence >= MIN_CONFIDENCE, "confidence={}", finding.confidence);
        assert!(
            (finding.lag_secs - f64::from(REPEAT_AT_SECS)).abs() <= 3.0,
            "lag_secs={} (expected ~{REPEAT_AT_SECS}, chromaprint item quantization)",
            finding.lag_secs
        );
    }

    #[test]
    fn monotonic_chirp_has_no_half_vs_half_repeat() {
        let clip = monotonic_chirp_clip();
        let fp = fingerprint_prepared(&clip);
        let duration = TOTAL_SECS as f64;

        let outcome =
            spike_detect_internal_repeat(&fp, duration, ChromaprintPreset::default(), MIN_CONFIDENCE);

        assert!(
            outcome.recommended.is_none(),
            "monotonic chirp should not report repetition: {:?}",
            outcome.recommended
        );
    }

    #[test]
    fn select_best_nonzero_lag_ignores_zero_cluster() {
        let fp = fingerprint_prepared(&repeated_segment_clip_silent_gaps());
        let config = configuration_for_preset(ChromaprintPreset::default());
        let mid_item = ((TOTAL_SECS as f64 / 2.0) / f64::from(config.item_duration_in_seconds()))
            .round() as usize;
        let (left, right) = fp.data.split_at(mid_item);
        let segments = match_fingerprints(left, right, &config).unwrap();

        let (segment, _, confidence) =
            select_best_nonzero_lag_segment(&segments, MIN_CONFIDENCE).unwrap();
        assert!(segment_offset_items(segment).unsigned_abs() > 1);
        assert!(confidence >= MIN_CONFIDENCE);
    }
}
