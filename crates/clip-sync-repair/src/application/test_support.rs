//! Shared test stubs used by repair unit tests and `clip-sync-repair-fixtures`.
//!
//! Lives in the repair crate (not fixtures) so unit tests and fixtures share one `Aligner`
//! impl without hitting the duplicate-crate trait bound when fixtures is a repair
//! `[dev-dependency]`.

use clip_sync::{AlignmentResult, ClipLabel, ClipMatch, TimelineOverlap};

use crate::application::ports::Aligner;

/// Aligner stub for tests that call `scan_after_alignment` (or inject alignment) directly.
#[doc(hidden)]
pub struct NeverCalledAligner;

impl Aligner for NeverCalledAligner {
    fn align(
        &self,
        _: clip_sync::AlignVideosRequest,
        _: &dyn clip_sync::ProgressReporter,
    ) -> Result<AlignmentResult, clip_sync::AppError> {
        unreachable!("tests use scan_after_alignment (or inject alignment) directly")
    }
}

/// Unaligned start clip with a zero-length window — for scan-after-alignment corpus cases that
/// do not care about clip geometry.
#[doc(hidden)]
pub fn no_op_alignment() -> AlignmentResult {
    start_clip_alignment(0.0, None)
}

/// Start clip spanning `[0, window_end_secs]` with optional offset (no timeline overlap).
#[doc(hidden)]
pub fn start_clip_alignment(window_end_secs: f64, offset: Option<f64>) -> AlignmentResult {
    let aligned = offset.is_some();
    AlignmentResult {
        clips: vec![ClipMatch {
            label: ClipLabel::Start,
            window_start_secs: 0.0,
            window_end_secs,
            aligned,
            offset_secs: offset,
            confidence: if aligned { 0.9 } else { 0.0 },
            video_a_decode_skips: 0,
            video_b_decode_skips: 0,
            repetition: None,
            video_b_window_start_secs: None,
            video_b_window_end_secs: None,
        }],
        start_aligned: aligned,
        end_aligned: None,
        recommended_offset_secs: offset,
        offsets_consistent: true,
        offset_drift_secs: None,
        start_overlap: None,
        high_rate_refinement: None,
        offset_verification: None,
        offset_ambiguous_mod_secs: None,
        alignment_mode_used: None,
        query_localization: None,
        end_clip_anchor: None,
    }
}

/// Zero-offset start clip with full A/B timeline overlap.
#[doc(hidden)]
pub fn zero_offset_alignment(duration_secs: f64) -> AlignmentResult {
    AlignmentResult {
        clips: vec![ClipMatch {
            label: ClipLabel::Start,
            window_start_secs: 0.0,
            window_end_secs: duration_secs,
            aligned: true,
            offset_secs: Some(0.0),
            confidence: 0.95,
            video_a_decode_skips: 0,
            video_b_decode_skips: 0,
            repetition: None,
            video_b_window_start_secs: None,
            video_b_window_end_secs: None,
        }],
        start_aligned: true,
        end_aligned: None,
        recommended_offset_secs: Some(0.0),
        offsets_consistent: true,
        offset_drift_secs: None,
        start_overlap: Some(TimelineOverlap {
            video_a_start_secs: 0.0,
            video_a_end_secs: duration_secs,
            video_b_start_secs: 0.0,
            video_b_end_secs: duration_secs,
            shared_length_secs: duration_secs,
        }),
        high_rate_refinement: None,
        offset_verification: None,
        offset_ambiguous_mod_secs: None,
        alignment_mode_used: None,
        query_localization: None,
        end_clip_anchor: None,
    }
}

/// Start + end clips at a half split, zero drift (oracle-injected style).
#[doc(hidden)]
pub fn oracle_injected_alignment(timeline_secs: f64) -> AlignmentResult {
    let half = timeline_secs / 2.0;
    AlignmentResult {
        clips: vec![
            ClipMatch {
                label: ClipLabel::Start,
                window_start_secs: 0.0,
                window_end_secs: half,
                aligned: true,
                offset_secs: Some(0.0),
                confidence: 0.95,
                video_a_decode_skips: 0,
                video_b_decode_skips: 0,
                repetition: None,
                video_b_window_start_secs: None,
                video_b_window_end_secs: None,
            },
            ClipMatch {
                label: ClipLabel::End,
                window_start_secs: half,
                window_end_secs: timeline_secs,
                aligned: true,
                offset_secs: Some(0.0),
                confidence: 0.95,
                video_a_decode_skips: 0,
                video_b_decode_skips: 0,
                repetition: None,
                video_b_window_start_secs: None,
                video_b_window_end_secs: None,
            },
        ],
        start_aligned: true,
        end_aligned: Some(true),
        recommended_offset_secs: Some(0.0),
        offsets_consistent: false,
        offset_drift_secs: Some(0.0),
        start_overlap: None,
        high_rate_refinement: None,
        offset_verification: None,
        offset_ambiguous_mod_secs: None,
        alignment_mode_used: None,
        query_localization: None,
        end_clip_anchor: None,
    }
}
