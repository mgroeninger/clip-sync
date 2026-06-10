use tracing::debug;

use rusty_chromaprint::{match_fingerprints, MatchError};

use crate::application::config::ChromaprintPreset;
use crate::application::error::AlignmentError;
use crate::application::ports::Aligner;
use crate::domain::{ClipMatchEstimate, Fingerprint};
use crate::infrastructure::chromaprint::config::configuration_for_preset;
use crate::infrastructure::chromaprint::matching::{
    segment_confidence, select_best_segment,
};

#[derive(Debug, Clone, Copy)]
pub struct ChromaprintAligner {
    preset: ChromaprintPreset,
}

impl ChromaprintAligner {
    pub fn new(preset: ChromaprintPreset) -> Self {
        Self { preset }
    }
}

impl Default for ChromaprintAligner {
    fn default() -> Self {
        Self::new(ChromaprintPreset::default())
    }
}

impl Aligner for ChromaprintAligner {
    fn find_offset(
        &self,
        left: &Fingerprint,
        right: &Fingerprint,
    ) -> Result<ClipMatchEstimate, AlignmentError> {
        find_offset(left, right, self.preset)
    }
}

fn find_offset(
    left: &Fingerprint,
    right: &Fingerprint,
    preset: ChromaprintPreset,
) -> Result<ClipMatchEstimate, AlignmentError> {
    if left.data.is_empty() || right.data.is_empty() {
        return Ok(ClipMatchEstimate {
            offset_secs: 0.0,
            confidence: 0.0,
        });
    }

    let config = configuration_for_preset(preset);
    let segments = match_fingerprints(&left.data, &right.data, &config)
        .map_err(map_match_error)?;

    let selection = select_best_segment(&segments);
    let Some((segment, ambiguous)) = selection else {
        debug!("no matching fingerprint segments found");
        return Ok(ClipMatchEstimate {
            offset_secs: 0.0,
            confidence: 0.0,
        });
    };

    let item_secs = f64::from(config.item_duration_in_seconds());
    let offset_secs = (segment.offset2 as f64 - segment.offset1 as f64) * item_secs;
    let confidence = segment_confidence(segment.score, segment.items_count, ambiguous);

    debug!(
        offset_secs,
        confidence,
        segment_score = segment.score,
        segment_items = segment.items_count,
        ambiguous,
        "fingerprint match segment selected"
    );

    Ok(ClipMatchEstimate {
        offset_secs,
        confidence,
    })
}

fn map_match_error(error: MatchError) -> AlignmentError {
    match error {
        MatchError::FingerprintTooLong { index } => AlignmentError::EngineFailed(format!(
            "fingerprint {index} is too long to compare"
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::TAU;

    use super::*;
    use crate::application::ports::{Aligner, Fingerprinter as FingerprintPort};
    use crate::domain::MonoPcmClip;
    use crate::infrastructure::chromaprint::ChromaprintFingerprinter;

    fn fingerprint(clip: &MonoPcmClip) -> Fingerprint {
        ChromaprintFingerprinter::default().fingerprint(clip).unwrap()
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

    fn tone_clip_from(sample_rate: u32, start_index: u64, seconds: u32) -> MonoPcmClip {
        MonoPcmClip {
            sample_rate,
            decode_error_skips: 0,
            decoded_sample_count: None,
            samples: tone_samples(
                sample_rate,
                start_index,
                sample_rate as usize * seconds as usize,
            ),
        }
    }

    fn tone_clip(sample_rate: u32, seconds: u32) -> MonoPcmClip {
        tone_clip_from(sample_rate, 0, seconds)
    }

    #[test]
    fn identical_clips_align_with_high_confidence() {
        let clip = tone_clip(44_100, 15);
        let left = fingerprint(&clip);
        let right = fingerprint(&clip);

        let estimate = ChromaprintAligner::default().find_offset(&left, &right).unwrap();
        assert!(estimate.confidence >= 0.5, "confidence={}", estimate.confidence);
        assert!(
            estimate.offset_secs.abs() < 0.25,
            "offset={}",
            estimate.offset_secs
        );
    }

    fn chirp_clip_from(sample_rate: u32, start_index: u64, seconds: u32) -> MonoPcmClip {
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

    #[test]
    fn phase_shifted_chirp_reports_negative_offset_when_b_is_ahead() {
        let sample_rate = 44_100;
        let lead_secs = 2;
        let left = fingerprint(&chirp_clip_from(sample_rate, 0, 20));
        let right = fingerprint(&chirp_clip_from(
            sample_rate,
            lead_secs as u64 * sample_rate as u64,
            20,
        ));

        let estimate = ChromaprintAligner::default().find_offset(&left, &right).unwrap();
        assert!(
            estimate.confidence >= 0.3,
            "confidence={}",
            estimate.confidence
        );
        assert!(
            (estimate.offset_secs + f64::from(lead_secs)).abs() < 1.0,
            "offset={}",
            estimate.offset_secs
        );
    }

    #[test]
    fn empty_fingerprint_returns_zero_confidence() {
        let left = Fingerprint { data: vec![1, 2, 3] };
        let right = Fingerprint { data: vec![] };

        let estimate = ChromaprintAligner::default()
            .find_offset(&left, &right)
            .unwrap();
        assert_eq!(estimate.confidence, 0.0);
        assert_eq!(estimate.offset_secs, 0.0);
    }

    #[test]
    fn short_segment_has_lower_confidence_than_long_segment_at_same_score() {
        let short = segment_confidence(2.0, 5, false);
        let long = segment_confidence(2.0, 200, false);
        assert!(long > short);
    }
}
