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
    /// Gap-equivalence gate: A's silence is already equivalent to B's (mutual/ambient silence — nothing to
    /// repair), so patching would be a no-op. Only produced when `skip_equivalent_gaps` is enabled.
    AlreadyMatchesReference,
    /// User gap selection (`--only-gaps` / `--skip-gaps`): gap would otherwise be planned but was not
    /// selected for this run.
    GapNotSelected,
}

/// Which fit-joint placement produced a seam Pearson score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SeamScoreSource {
    Baseline,
    Anchor,
    Grid,
    Extension,
    DualFit,
}

/// One waveform seam score from a scored placement (baseline, anchor bracket, grid cell, or extension).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct SeamScoreAttempt {
    pub pre_correlation: f64,
    pub post_correlation: f64,
    pub source: SeamScoreSource,
}

impl SeamScoreAttempt {
    pub fn min_pearson(self) -> f64 {
        self.pre_correlation.min(self.post_correlation)
    }
}

pub fn format_seam_score_source(source: SeamScoreSource) -> &'static str {
    match source {
        SeamScoreSource::Baseline => "baseline",
        SeamScoreSource::Anchor => "anchor",
        SeamScoreSource::Grid => "grid",
        SeamScoreSource::Extension => "extension",
        SeamScoreSource::DualFit => "dual_fit",
    }
}

/// Pick whichever attempt has the higher `min_pearson`, keeping `a` on ties.
pub fn better_seam_score_attempt(
    a: Option<SeamScoreAttempt>,
    b: Option<SeamScoreAttempt>,
) -> Option<SeamScoreAttempt> {
    match (a, b) {
        (Some(a), Some(b)) => Some(if b.min_pearson() > a.min_pearson() {
            b
        } else {
            a
        }),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

fn format_correlation_below_threshold(
    pre_correlation: f64,
    post_correlation: f64,
    min_correlation: f32,
    best_attempt: Option<SeamScoreAttempt>,
) -> String {
    let base = format!(
        "boundary correlation below threshold (pre={pre_correlation:.2} post={post_correlation:.2} min={min_correlation:.2})"
    );
    let Some(best) = best_attempt else {
        return base;
    };
    let reported_min = pre_correlation.min(post_correlation);
    if best.min_pearson() <= reported_min + 1e-9 {
        return base;
    }
    format!(
        "{base}; best pre={:.2} post={:.2} @ {}",
        best.pre_correlation,
        best.post_correlation,
        format_seam_score_source(best.source),
    )
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
        #[serde(skip_serializing_if = "Option::is_none")]
        best_attempt: Option<SeamScoreAttempt>,
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
    /// B is silent at the nominal program-time span (D11/G5) — quiet in both masters, nothing to fill.
    ProgramQuiet,
}

/// Human-readable skip reason for stdout gap tables and verbose stderr (`-v`).
pub fn format_gap_patch_skip_reason(reason: &GapPatchSkipReason) -> String {
    match reason {
        GapPatchSkipReason::BExtractFailed => "B audio extraction failed".into(),
        GapPatchSkipReason::BoundaryAlignmentFailed => "boundary alignment failed".into(),
        GapPatchSkipReason::CorrelationBelowThreshold {
            pre_correlation,
            post_correlation,
            min_correlation,
            best_attempt,
        } => format_correlation_below_threshold(
            *pre_correlation,
            *post_correlation,
            *min_correlation,
            *best_attempt,
        ),
        GapPatchSkipReason::AlignedSegmentOutOfRange => "aligned B segment out of range".into(),
        GapPatchSkipReason::ZeroLengthGap => "zero-length gap".into(),
        GapPatchSkipReason::ProgramQuiet => {
            "program-quiet in both masters (B silent at nominal span; nothing to fill)".into()
        }
        GapPatchSkipReason::ResidualHeadroomExceeded {
            pre_correlation,
            post_correlation,
            headroom_db,
            floor_pre_db,
            floor_post_db,
            margin_db,
        } => format!(
            "residual headroom exceeded (pre={pre_correlation:.2} post={post_correlation:.2} \
             headroom={headroom_db:.1} dB floor_pre={floor_pre_db:.1} floor_post={floor_post_db:.1} \
             margin={margin_db:.1} dB)"
        ),
    }
}

/// Short skip reason for default-mode `tracing::warn` mid-run notifications.
pub fn format_gap_patch_skip_warn_reason(reason: &GapPatchSkipReason) -> String {
    match reason {
        GapPatchSkipReason::BExtractFailed => "B audio slice out of range".into(),
        GapPatchSkipReason::BoundaryAlignmentFailed => "structure alignment failed".into(),
        GapPatchSkipReason::ProgramQuiet => "program-quiet (nothing to fill)".into(),
        GapPatchSkipReason::CorrelationBelowThreshold {
            pre_correlation,
            post_correlation,
            min_correlation,
            best_attempt,
        } => format_correlation_below_threshold(
            *pre_correlation,
            *post_correlation,
            *min_correlation,
            *best_attempt,
        ),
        GapPatchSkipReason::ResidualHeadroomExceeded { .. } => {
            "residual headroom exceeded (anti-echo veto)".into()
        }
        other => format_gap_patch_skip_reason(other),
    }
}

/// Verbose stderr line (`-v`) aligned with per-gap fill plan indentation.
pub fn format_gap_fill_skip_verbose_line(reason: &GapPatchSkipReason) -> String {
    format!(
        "           skipped: {}",
        format_gap_patch_skip_reason(reason)
    )
}

/// Human-readable marginal patch detail for stdout gap tables and verbose stderr (`-v`).
pub fn format_gap_fill_marginal_detail(pre: f64, post: f64, min: f32, anchor_seam: bool) -> String {
    let kind = if anchor_seam {
        "marginal anchor seam"
    } else {
        "marginal waveform seam"
    };
    format!("{kind} (pre={pre:.2} post={post:.2} min={min:.2})")
}

/// Short marginal reason for default-mode `tracing::warn` mid-run notifications.
pub fn format_gap_fill_marginal_warn_reason(
    pre: f64,
    post: f64,
    min: f32,
    anchor_seam: bool,
) -> String {
    format_gap_fill_marginal_detail(pre, post, min, anchor_seam)
}

/// Verbose stderr line (`-v`) aligned with per-gap fill plan indentation.
pub fn format_gap_fill_marginal_verbose_line(
    pre: f64,
    post: f64,
    min: f32,
    anchor_seam: bool,
) -> String {
    format!(
        "           patched: {}",
        format_gap_fill_marginal_detail(pre, post, min, anchor_seam)
    )
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
    pub fn new(a_start_secs: f64, a_end_secs: f64, status: GapPatchStatus, tags: GapTags) -> Self {
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
        /// Run metadata: placement won at an editorial anchor seam, not the scan throat alone
        /// (anchor-seam search rescued this gap). Mirrors `structure_trusted` as a path fact —
        /// not a `GapTags` characteristic. Serialized only when `true` (absent ⇒ baseline throat).
        #[serde(default, skip_serializing_if = "is_false")]
        anchor_seam_used: bool,
        /// Frames the winning anchor bracket moved from the scan-refined baseline; 0 unless
        /// `anchor_seam_used` (the displacement of the editorial seam from the silence throat).
        #[serde(default, skip_serializing_if = "is_zero_move_frames")]
        anchor_bracket_move_frames: usize,
        /// Run metadata: this fill was rescued by the A3 dual-fit path (G6) after the bracket
        /// search exhausted, not by ordinary bracket-search fitting. Mirrors `anchor_seam_used`
        /// as a path fact — not a `GapTags` characteristic. Serialized only when `true`.
        #[serde(default, skip_serializing_if = "is_false")]
        dual_fit_used: bool,
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

/// Skip serializing `anchor_bracket_move_frames` for non-anchor patches (the common case).
fn is_zero_move_frames(frames: &usize) -> bool {
    *frames == 0
}

/// Skip serializing `anchor_seam_used` when `false` (the common baseline-throat case).
fn is_false(b: &bool) -> bool {
    !*b
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

    pub fn with_patch_anchors(
        mut self,
        anchors: Vec<crate::domain::patch_anchor::PatchAnchorReport>,
    ) -> Self {
        if anchors.is_empty() {
            self.patch_anchors_used = None;
        } else {
            self.patch_anchors_used = Some(anchors);
        }
        self
    }
}

#[cfg(test)]
mod format_tests {
    use super::*;

    #[test]
    fn correlation_skip_includes_best_attempt_when_higher() {
        let reason = GapPatchSkipReason::CorrelationBelowThreshold {
            pre_correlation: 0.06,
            post_correlation: 0.06,
            min_correlation: 0.12,
            best_attempt: Some(SeamScoreAttempt {
                pre_correlation: 0.18,
                post_correlation: 0.31,
                source: SeamScoreSource::Anchor,
            }),
        };
        let text = format_gap_patch_skip_reason(&reason);
        assert!(text.contains("pre=0.06 post=0.06"));
        assert!(text.contains("best pre=0.18 post=0.31 @ anchor"));
    }

    #[test]
    fn gap_fill_skip_verbose_line_matches_stdout_status() {
        let reason = GapPatchSkipReason::CorrelationBelowThreshold {
            pre_correlation: 0.23,
            post_correlation: 0.21,
            min_correlation: 0.35,
            best_attempt: None,
        };
        assert_eq!(
            format_gap_fill_skip_verbose_line(&reason),
            "           skipped: boundary correlation below threshold (pre=0.23 post=0.21 min=0.35)"
        );
    }

    #[test]
    fn gap_fill_marginal_verbose_line_matches_stdout_status() {
        assert_eq!(
            format_gap_fill_marginal_verbose_line(0.31, 1.0, 0.35, false),
            "           patched: marginal waveform seam (pre=0.31 post=1.00 min=0.35)"
        );
        assert_eq!(
            format_gap_fill_marginal_verbose_line(0.28, 0.9, 0.35, true),
            "           patched: marginal anchor seam (pre=0.28 post=0.90 min=0.35)"
        );
    }

    #[test]
    fn gap_patch_skip_warn_reason_keeps_legacy_short_phrases() {
        assert_eq!(
            format_gap_patch_skip_warn_reason(&GapPatchSkipReason::BoundaryAlignmentFailed),
            "structure alignment failed"
        );
        assert_eq!(
            format_gap_patch_skip_warn_reason(&GapPatchSkipReason::BExtractFailed),
            "B audio slice out of range"
        );
    }

    /// Regression for the 2026-07 production report: when the ordinary bracket search's best
    /// attempt fails the gate, but dual-fit's own (also-failed) re-validation scored higher, the
    /// final skip report must surface dual-fit's attempt instead of silently dropping it — the
    /// ordinary anchor-bracket report previously never reflected anything dual-fit tried.
    #[test]
    fn better_seam_score_attempt_prefers_the_higher_min_pearson() {
        let anchor_attempt = SeamScoreAttempt {
            pre_correlation: 0.13,
            post_correlation: 0.09,
            source: SeamScoreSource::Anchor,
        };
        let dual_fit_attempt = SeamScoreAttempt {
            pre_correlation: 0.09,
            post_correlation: 0.007,
            source: SeamScoreSource::DualFit,
        };
        // Dual-fit's own re-validation, in this example, scored WORSE than the anchor bracket's
        // best attempt — the anchor attempt must still win and be reported.
        let best = better_seam_score_attempt(Some(anchor_attempt), Some(dual_fit_attempt));
        assert_eq!(best, Some(anchor_attempt));

        let stronger_dual_fit = SeamScoreAttempt {
            pre_correlation: 0.5,
            post_correlation: 0.4,
            source: SeamScoreSource::DualFit,
        };
        let best = better_seam_score_attempt(Some(anchor_attempt), Some(stronger_dual_fit));
        assert_eq!(best, Some(stronger_dual_fit));

        // No ordinary best_attempt at all (e.g. `StructureBelowThreshold`, which hard-codes
        // `None`) — dual-fit's attempt should still surface on its own.
        let best = better_seam_score_attempt(None, Some(stronger_dual_fit));
        assert_eq!(best, Some(stronger_dual_fit));

        // dual-fit never ran / was ineligible — nothing to merge in, ordinary result unchanged.
        let best = better_seam_score_attempt(Some(anchor_attempt), None);
        assert_eq!(best, Some(anchor_attempt));
    }

    #[test]
    fn format_seam_score_source_covers_dual_fit() {
        assert_eq!(
            format_seam_score_source(SeamScoreSource::DualFit),
            "dual_fit"
        );
    }
}
