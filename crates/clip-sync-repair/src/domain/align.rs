//! Repair-domain alignment context — offset mapping for gap fill without clip-sync types.

use serde::Serialize;

/// Clip role in a symmetric or query-reference alignment run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipRole {
    Start,
    Interior,
    End,
}

/// One aligned clip window used for per-gap B offset mapping.
#[derive(Debug, Clone, PartialEq)]
pub struct AlignedClip {
    pub role: ClipRole,
    pub window_start_secs: f64,
    pub window_end_secs: f64,
    pub aligned: bool,
    pub offset_secs: Option<f64>,
    pub confidence: f32,
    pub video_a_decode_skips: u32,
    pub video_b_decode_skips: u32,
    pub video_b_window_start_secs: Option<f64>,
    pub video_b_window_end_secs: Option<f64>,
}

/// Shared timeline region implied by the start clip and recommended offset.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimelineOverlap {
    pub video_a_start_secs: f64,
    pub video_a_end_secs: f64,
    pub video_b_start_secs: f64,
    pub video_b_end_secs: f64,
    pub shared_length_secs: f64,
}

/// Observed divergence between packet PTS and the sequential decoded-sample clock.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct AudioTimelineSkew {
    pub pts_secs: f64,
    pub sample_clock_secs: f64,
    pub delta_secs: f64,
}

/// Alignment summary carried on [`crate::domain::GapReport`] for fill planning.
#[derive(Debug, Clone, PartialEq)]
pub struct ScanAlignment {
    pub clips: Vec<AlignedClip>,
    pub start_aligned: bool,
    pub end_aligned: Option<bool>,
    pub recommended_offset_secs: Option<f64>,
    pub offsets_consistent: bool,
    pub offset_drift_secs: Option<f64>,
    pub start_overlap: Option<TimelineOverlap>,
    /// True when query-reference localization ran (mapped-region fill gate applies).
    pub query_reference_mode: bool,
}

impl ScanAlignment {
    pub fn clip_with_role(&self, role: ClipRole) -> Option<&AlignedClip> {
        self.clips.iter().find(|clip| clip.role == role)
    }

    pub fn start_clip(&self) -> Option<&AlignedClip> {
        self.clip_with_role(ClipRole::Start)
    }
}

#[cfg(test)]
pub(crate) fn test_aligned_clip(
    role: ClipRole,
    window_start_secs: f64,
    window_end_secs: f64,
    offset_secs: f64,
) -> AlignedClip {
    AlignedClip {
        role,
        window_start_secs,
        window_end_secs,
        aligned: true,
        offset_secs: Some(offset_secs),
        confidence: 0.95,
        video_a_decode_skips: 0,
        video_b_decode_skips: 0,
        video_b_window_start_secs: None,
        video_b_window_end_secs: None,
    }
}

#[cfg(test)]
pub(crate) fn test_two_clip_alignment(start_offset: f64, end_offset: f64) -> ScanAlignment {
    ScanAlignment {
        clips: vec![
            test_aligned_clip(ClipRole::Start, 0.0, 900.0, start_offset),
            test_aligned_clip(ClipRole::End, 6647.0, 7547.0, end_offset),
        ],
        start_aligned: true,
        end_aligned: Some(true),
        recommended_offset_secs: Some((start_offset + end_offset) / 2.0),
        offsets_consistent: false,
        offset_drift_secs: Some(end_offset - start_offset),
        start_overlap: None,
        query_reference_mode: false,
    }
}

#[cfg(test)]
pub(crate) fn test_empty_alignment(recommended_offset_secs: f64) -> ScanAlignment {
    ScanAlignment {
        clips: vec![],
        start_aligned: false,
        end_aligned: None,
        recommended_offset_secs: Some(recommended_offset_secs),
        offsets_consistent: true,
        offset_drift_secs: None,
        start_overlap: None,
        query_reference_mode: false,
    }
}
