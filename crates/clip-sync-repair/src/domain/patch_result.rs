use serde::Serialize;

use crate::domain::gap_fill_fit::FillConfidence;

/// Why a detected gap was not included in the fill plan.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GapFillSkipReason {
    NotFillable,
    TrackLayoutMismatch,
    TrackCompatibilityUnavailable,
    OutsideReferenceCoverage,
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
        /// Seam score used for the patch decision (structure or waveform Pearson).
        pre_correlation: f64,
        post_correlation: f64,
        /// Total B slide from mapped nominal (structure + waveform).
        align_adjustment_secs: f64,
        /// Additional slide from waveform search after structure match.
        waveform_adjustment_secs: f64,
        /// `true` when placement was accepted from structure match without a waveform gate.
        structure_trusted: bool,
        /// Fit-mode waveform tier (`high` confident, `marginal` warn-patch band).
        #[serde(default = "default_fill_confidence_high")]
        confidence: FillConfidence,
        /// Frames the winning A gap start was shifted from the pre-search refined edge.
        #[serde(default)]
        gap_start_adjust_frames: i64,
        /// Frames the winning A gap end was extended from the pre-search refined edge.
        #[serde(default)]
        gap_end_adjust_frames: i64,
    },
    Skipped {
        reason: GapPatchSkipReason,
    },
    NotPlanned {
        reason: GapFillSkipReason,
    },
}

#[allow(dead_code)]
fn default_fill_confidence_high() -> FillConfidence {
    FillConfidence::High
}

/// User-visible summary of a `PatchAudio` run (no PCM payload).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PatchSummary {
    pub patched_count: usize,
    pub patched_marginal_count: usize,
    pub skipped_count: usize,
    pub not_planned_count: usize,
    pub gaps: Vec<GapPatchOutcome>,
    /// Offset anchors built from pass-1 successes (`anchored_retry`); omitted when empty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patch_anchors_used: Option<Vec<crate::domain::patch_anchor::PatchAnchorReport>>,
}

impl PatchSummary {
    /// True when at least one gap was spliced into A's PCM.
    pub fn has_patches(&self) -> bool {
        self.patched_count > 0
    }

    pub fn from_outcomes(gaps: Vec<GapPatchOutcome>) -> Self {
        let mut patched_count = 0usize;
        let mut patched_marginal_count = 0usize;
        let mut skipped_count = 0usize;
        let mut not_planned_count = 0usize;
        for gap in &gaps {
            match gap.status {
                GapPatchStatus::Patched { confidence, .. } => {
                    patched_count += 1;
                    if confidence == FillConfidence::Marginal {
                        patched_marginal_count += 1;
                    }
                }
                GapPatchStatus::Skipped { .. } => skipped_count += 1,
                GapPatchStatus::NotPlanned { .. } => not_planned_count += 1,
            }
        }
        Self {
            patched_count,
            patched_marginal_count,
            skipped_count,
            not_planned_count,
            gaps,
            patch_anchors_used: None,
        }
    }

    pub fn with_patch_anchors(mut self, anchors: Vec<crate::domain::patch_anchor::PatchAnchorReport>) -> Self {
        if anchors.is_empty() {
            self.patch_anchors_used = None;
        } else {
            self.patch_anchors_used = Some(anchors);
        }
        self
    }
}
