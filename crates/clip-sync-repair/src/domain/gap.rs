use std::path::PathBuf;

use serde::Serialize;

use clip_sync::AlignmentResult;

/// A silent window detected in video A's timeline.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Gap {
    pub video_a_start_secs: f64,
    pub video_a_end_secs: f64,
    /// Corresponding position in video B (via recommended_offset_secs).
    pub video_b_start_secs: f64,
    pub video_b_end_secs: f64,
    /// Whether video B has audio energy at this position (potential fill source).
    pub b_has_energy: bool,
}

impl Gap {
    pub fn duration_secs(&self) -> f64 {
        self.video_a_end_secs - self.video_a_start_secs
    }

    pub fn is_fillable(&self) -> bool {
        self.b_has_energy
    }
}

/// Full gap scan report produced by `ScanGaps`.
#[derive(Debug, Clone, Serialize)]
pub struct GapReport {
    pub video_a: PathBuf,
    pub video_b: PathBuf,
    pub alignment: AlignmentResult,
    pub gaps: Vec<Gap>,
    pub scan_window_secs: u64,
    pub silence_peak_fraction: f32,
}

impl GapReport {
    pub fn fillable_count(&self) -> usize {
        self.gaps.iter().filter(|g| g.is_fillable()).count()
    }
}
