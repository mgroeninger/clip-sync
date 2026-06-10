#![allow(dead_code)]

#[cfg(test)]
use std::time::Duration;

#[cfg(test)]
use crate::domain::{AlignmentResult, ClipLabel, ClipMatch};

/// Minimal [`AlignmentResult`] for unit tests. Extend fields here when the struct grows.
#[cfg(test)]
pub fn minimal_alignment_result(recommended_offset_secs: Option<f64>) -> AlignmentResult {
    let aligned = recommended_offset_secs.is_some();
    AlignmentResult {
        clips: vec![ClipMatch {
            label: ClipLabel::Start,
            window_start_secs: 0.0,
            window_end_secs: 60.0,
            aligned,
            offset_secs: recommended_offset_secs,
            confidence: if aligned { 0.9 } else { 0.0 },
            video_a_decode_skips: 0,
            video_b_decode_skips: 0,
            repetition: None,
        }],
        start_aligned: aligned,
        end_aligned: None,
        recommended_offset_secs,
        offsets_consistent: true,
        offset_drift_secs: None,
        start_overlap: None,
        high_rate_refinement: None,
        offset_verification: None,
    }
}

#[cfg(test)]
pub fn mins(m: u64) -> Duration {
    Duration::from_secs(m * 60)
}
