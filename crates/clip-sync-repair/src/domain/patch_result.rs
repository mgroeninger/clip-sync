use serde::Serialize;

/// Why a detected gap was not included in the fill plan.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GapFillSkipReason {
    NotFillable,
    TrackLayoutMismatch,
    TrackCompatibilityUnavailable,
}

/// Why a planned gap was not patched during splice.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GapPatchSkipReason {
    BExtractFailed,
    BoundaryAlignmentFailed,
    CorrelationBelowThreshold {
        pre_correlation: f64,
        post_correlation: f64,
        min_correlation: f32,
    },
    AlignedSegmentOutOfRange,
    ZeroLengthGap,
}

/// Per-gap outcome after a patch pass (scan gaps in report order).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GapPatchOutcome {
    pub a_start_secs: f64,
    pub a_end_secs: f64,
    pub status: GapPatchStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GapPatchStatus {
    Patched {
        pre_correlation: f64,
        post_correlation: f64,
        align_adjustment_secs: f64,
    },
    Skipped {
        reason: GapPatchSkipReason,
    },
    NotPlanned {
        reason: GapFillSkipReason,
    },
}

/// User-visible summary of a `PatchAudio` run (no PCM payload).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PatchSummary {
    pub patched_count: usize,
    pub skipped_count: usize,
    pub not_planned_count: usize,
    pub gaps: Vec<GapPatchOutcome>,
}

impl PatchSummary {
    pub fn from_outcomes(gaps: Vec<GapPatchOutcome>) -> Self {
        let mut patched_count = 0usize;
        let mut skipped_count = 0usize;
        let mut not_planned_count = 0usize;
        for gap in &gaps {
            match gap.status {
                GapPatchStatus::Patched { .. } => patched_count += 1,
                GapPatchStatus::Skipped { .. } => skipped_count += 1,
                GapPatchStatus::NotPlanned { .. } => not_planned_count += 1,
            }
        }
        Self {
            patched_count,
            skipped_count,
            not_planned_count,
            gaps,
        }
    }
}
