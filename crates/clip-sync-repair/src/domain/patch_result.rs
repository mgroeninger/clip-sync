use serde::Serialize;

use crate::domain::fill_mode::FillMode;
use crate::domain::gap_fill_fit::FillConfidence;
use crate::domain::gap_tags::{derive_gap_tags_from_status, FillTierThresholds, GapTags};
use crate::domain::policies::SeamResidualVerdict;

/// Absent residual summary scalars on [`GapPatchStatus::Patched`] (tests / lossy builders).
#[inline]
pub const fn gap_patch_residual_unmeasured() -> (Option<f64>, Option<f64>, Option<f64>) {
    (None, None, None)
}

/// Scalar residual summary fields for patched gaps (P1 JSON).
pub fn residual_summary_scalar_fields(
    residual: Option<&SeamResidualVerdict>,
) -> (Option<f64>, Option<f64>, Option<f64>) {
    let Some(v) = residual else {
        return (None, None, None);
    };
    let residual_db = v.worst_chosen_db();
    let floor_db = v.worst_floor_db();
    let headroom_db = v.worst_headroom_db();
    (
        residual_db.is_finite().then_some(residual_db),
        floor_db.is_finite().then_some(floor_db),
        headroom_db.is_finite().then_some(headroom_db),
    )
}

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
    ResidualHeadroomExceeded {
        pre_correlation: f64,
        post_correlation: f64,
        headroom_db: f64,
        floor_pre_db: f64,
        floor_post_db: f64,
        margin_db: f64,
    },
}

/// Per-gap outcome after a patch pass (scan gaps in report order).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GapPatchOutcome {
    pub a_start_secs: f64,
    pub a_end_secs: f64,
    pub status: GapPatchStatus,
    /// Vocabulary tags derived at patch time (or from plan status for not-planned gaps).
    pub tags: GapTags,
    /// Residual/floor verdict (P1 report-only). Present only when residual measurement is enabled
    /// (`measure_residual` or debug logging) and the gap reached the fit waveform tier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub residual: Option<SeamResidualVerdict>,
}

impl GapPatchOutcome {
    pub fn new(
        a_start_secs: f64,
        a_end_secs: f64,
        status: GapPatchStatus,
        tags: GapTags,
    ) -> Self {
        Self {
            a_start_secs,
            a_end_secs,
            status,
            tags,
            residual: None,
        }
    }

    /// Attach a residual/floor verdict (P1 report-only); no-op when `None`.
    pub fn with_residual(mut self, residual: Option<SeamResidualVerdict>) -> Self {
        self.residual = residual;
        self
    }

    /// Build tags from [`GapPatchStatus`] only (lossy for patched/skipped fit gaps).
    pub fn with_tags_from_status(
        a_start_secs: f64,
        a_end_secs: f64,
        status: GapPatchStatus,
        fill_mode: FillMode,
        thresholds: FillTierThresholds,
    ) -> Self {
        let tags = derive_gap_tags_from_status(&status, fill_mode, thresholds);
        Self::new(a_start_secs, a_end_secs, status, tags)
    }
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
        /// Worst-side chosen-placement residual (dB); present when residual was measured.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        residual_db: Option<f64>,
        /// Worst-side nominal floor (dB); present when residual was measured.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        floor_db: Option<f64>,
        /// Worst-side headroom at chosen placement (dB); present when residual was measured.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        headroom_db: Option<f64>,
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
    /// Run-level donor relationship inferred from informative-floor fraction (P4 diagnostic).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub donor_relation: Option<crate::domain::gap_tags::DonorRelation>,
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
        let donor_relation = crate::domain::gap_tags::derive_donor_relation(&gaps);
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
            donor_relation,
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
