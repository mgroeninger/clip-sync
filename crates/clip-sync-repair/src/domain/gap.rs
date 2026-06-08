use std::path::PathBuf;

use serde::Serialize;

use clip_sync::{AlignmentResult, TimelineOverlap};

use crate::domain::track_match::TrackCompatibility;

/// Diagnostic comparison of the Chromaprint alignment offset vs the silence-structure-derived
/// offset produced by the R3 bidirectional scan cross-check.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GapOffsetAgreement {
    pub silence_based_offset_secs: f64,
    pub alignment_offset_secs: f64,
    /// Absolute difference between the two estimates.
    pub delta_secs: f64,
    /// `true` when `delta_secs` is within the configured tolerance.
    pub agrees: bool,
}

/// A silent region detected in video A's timeline.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Gap {
    pub video_a_start_secs: f64,
    pub video_a_end_secs: f64,
    /// Corresponding position in video B, mapped via `recommended_offset_secs`.
    /// `None` when alignment failed or produced no offset — B cannot be probed.
    pub video_b_start_secs: Option<f64>,
    pub video_b_end_secs: Option<f64>,
    /// Whether video B has audio energy at this position (potential fill source).
    pub b_has_energy: bool,
}

impl Gap {
    pub fn duration_secs(&self) -> f64 {
        self.video_a_end_secs - self.video_a_start_secs
    }

    pub fn is_fillable(&self) -> bool {
        self.video_b_start_secs.is_some() && self.b_has_energy
    }
}

/// True when `[start_secs, end_secs]` lies entirely inside a timeline window.
///
/// Used by fill planning and silence cross-check gating.
pub fn interval_fully_within_window(
    start_secs: f64,
    end_secs: f64,
    window_start_secs: f64,
    window_end_secs: f64,
) -> bool {
    start_secs >= window_start_secs && end_secs <= window_end_secs
}

/// Full gap scan report produced by `ScanGaps`.
#[derive(Debug, Clone, Serialize)]
pub struct GapReport {
    pub video_a: PathBuf,
    pub video_b: PathBuf,
    /// Audio track comparison (channels/rate). `None` when B could not be opened or has no
    /// decodable track — the scan still reports A's gaps.
    pub track_compatibility: Option<TrackCompatibility>,
    /// Shared timeline region from the alignment start clip. `None` when alignment failed.
    pub overlap: Option<TimelineOverlap>,
    pub alignment: AlignmentResult,
    pub gaps: Vec<Gap>,
    /// Present when `scan_both` was enabled and both A and B had silence intervals to compare.
    pub gap_offset_agreement: Option<GapOffsetAgreement>,
    /// Decode chunk size used during sequential scan (seconds).
    #[serde(alias = "scan_window_secs")]
    pub decode_chunk_secs: u64,
    /// Analysis block size used for silence-run detection (milliseconds).
    pub scan_block_ms: u64,
    pub silence_peak_fraction: f32,
}

impl GapReport {
    pub fn fillable_count(&self) -> usize {
        self.gaps.iter().filter(|g| g.is_fillable()).count()
    }
}
