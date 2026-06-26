//! Canonical gap vocabulary tags ([gap-repair-guide.md]).
//!
//! Orthogonal facts derived from plan/patch outcomes and seam scores. `content_hint`
//! remains guide-only and is not computed here.
//!
//! [gap-repair-guide.md]: ../../../docs/gap-repair-guide.md

use serde::Serialize;

use crate::domain::fill_mode::FillMode;
use crate::domain::gap_fill_fit::{effective_fill_absolute_floor, FillConfidence};
use crate::domain::patch_result::{GapFillSkipReason, GapPatchSkipReason, GapPatchStatus};
use crate::domain::policies::SeamResidualVerdict;
use crate::domain::residual_gate::DEFAULT_RESIDUAL_HEADROOM_MARGIN_DB;
use crate::domain::repair_profile::FitBoundarySearch;

/// Residual cancellation band (W-layer; guide `residual_band`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResidualBand {
    /// Informative floor and headroom within margin — content cancels at the seam.
    Cancels,
    /// Informative floor but headroom above margin — Pearson-only match (anti-echo risk).
    CorrelatesOnly,
    /// Floor uninformative, beyond lag reach, or not measured.
    NoFloor,
}

impl ResidualBand {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cancels => "cancels",
            Self::CorrelatesOnly => "correlates_only",
            Self::NoFloor => "no_floor",
        }
    }
}

/// Run-level donor relationship inferred from informative-floor fraction (guide `donor_relation`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DonorRelation {
    SameMaster,
    Mixed,
    DiffCapture,
}

impl DonorRelation {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SameMaster => "same_master",
            Self::Mixed => "mixed",
            Self::DiffCapture => "diff_capture",
        }
    }
}

/// Classify per-gap residual band from a measured verdict and headroom margin.
pub fn classify_residual_band(verdict: &SeamResidualVerdict, margin_db: f64) -> ResidualBand {
    if !verdict.informative || verdict.beyond_lag_reach() {
        ResidualBand::NoFloor
    } else if verdict.worst_headroom_db() <= margin_db {
        ResidualBand::Cancels
    } else {
        ResidualBand::CorrelatesOnly
    }
}

/// Derive run-level donor relation from per-gap residual verdicts (≥70% informative → same_master).
pub fn derive_donor_relation(gaps: &[crate::domain::patch_result::GapPatchOutcome]) -> Option<DonorRelation> {
    let measured: Vec<_> = gaps.iter().filter_map(|g| g.residual.as_ref()).collect();
    if measured.is_empty() {
        return None;
    }
    let informative_count = measured.iter().filter(|v| v.informative).count();
    let frac = informative_count as f64 / measured.len() as f64;
    if frac >= 0.70 {
        Some(DonorRelation::SameMaster)
    } else if informative_count == 0 {
        Some(DonorRelation::DiffCapture)
    } else {
        Some(DonorRelation::Mixed)
    }
}

/// Plan-time classification (P-layer in the guide).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanKind {
    BelowScanFloor,
    Unfillable,
    NotPlanned,
    Fillable,
}

impl PlanKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BelowScanFloor => "below_scan_floor",
            Self::Unfillable => "unfillable",
            Self::NotPlanned => "not_planned",
            Self::Fillable => "fillable",
        }
    }
}

/// Fit-mode patch tier (W-layer tiers; guide `patch_tier`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PatchTier {
    High,
    Marginal,
    AnchorTrusted,
    DeadZone,
    HardSkip,
    StructureFail,
    NotApplicable,
}

impl PatchTier {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::High => "high",
            Self::Marginal => "marginal",
            Self::AnchorTrusted => "anchor_trusted",
            Self::DeadZone => "dead_zone",
            Self::HardSkip => "hard_skip",
            Self::StructureFail => "structure_fail",
            Self::NotApplicable => "not_applicable",
        }
    }

    /// Short human label for the gap table status column.
    pub const fn status_label(self) -> Option<&'static str> {
        match self {
            Self::NotApplicable => None,
            Self::High => Some("high"),
            Self::Marginal => Some("marginal"),
            Self::AnchorTrusted => Some("anchor trusted"),
            Self::DeadZone => Some("dead zone"),
            Self::HardSkip => Some("hard skip"),
            Self::StructureFail => Some("structure fail"),
        }
    }
}

/// Seam score shape heuristic (W-layer patterns).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SeamShape {
    Balanced,
    AsymmetricPost,
    AsymmetricPre,
    SymmetricWeak,
    NotApplicable,
}

impl SeamShape {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Balanced => "balanced",
            Self::AsymmetricPost => "asymmetric_post",
            Self::AsymmetricPre => "asymmetric_pre",
            Self::SymmetricWeak => "symmetric_weak",
            Self::NotApplicable => "not_applicable",
        }
    }

    /// Short human label for the gap table status column.
    pub const fn status_label(self) -> Option<&'static str> {
        match self {
            Self::NotApplicable => None,
            Self::Balanced => Some("balanced"),
            Self::AsymmetricPost => Some("post-strong"),
            Self::AsymmetricPre => Some("pre-strong"),
            Self::SymmetricWeak => Some("weak both sides"),
        }
    }
}

/// Effective fit search path for a gap (guide `fit_path`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FitPathTag {
    BaselineOnly,
    BoundaryGrid,
}

impl FitPathTag {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BaselineOnly => "baseline_only",
            Self::BoundaryGrid => "boundary_grid",
        }
    }

    pub fn from_boundary_grid_used(used: bool) -> Self {
        if used {
            Self::BoundaryGrid
        } else {
            Self::BaselineOnly
        }
    }

    pub fn from_fit_boundary_search(search: FitBoundarySearch) -> Self {
        match search {
            FitBoundarySearch::BaselineOnly => Self::BaselineOnly,
            FitBoundarySearch::FullGrid => Self::BoundaryGrid,
        }
    }
}

/// Structure signature tier used for placement (guide `signature_mode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SignatureModeTag {
    Bool,
    Energy,
}

impl SignatureModeTag {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bool => "bool",
            Self::Energy => "energy",
        }
    }

    pub fn parse_label(label: &str) -> Option<Self> {
        match label {
            "bool" => Some(Self::Bool),
            "energy" => Some(Self::Energy),
            _ => None,
        }
    }
}

/// Thresholds for fit-mode waveform tiering.
#[derive(Debug, Clone, Copy)]
pub struct FillTierThresholds {
    pub min_fill_correlation: f32,
    pub fill_marginal_margin: f32,
    pub fill_absolute_floor: f32,
}

impl FillTierThresholds {
    pub const DEFAULT: Self = Self {
        min_fill_correlation: 0.35,
        fill_marginal_margin: 0.08,
        fill_absolute_floor: 0.12,
    };
}

/// Composed gap tags (orthogonal axes).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GapTags {
    pub plan_kind: PlanKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_skip_reason: Option<GapFillSkipReason>,
    pub patch_tier: PatchTier,
    pub seam_shape: SeamShape,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fit_path: Option<FitPathTag>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature_mode: Option<SignatureModeTag>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub residual_band: Option<ResidualBand>,
    /// Editorial anchor seam won (not scan-throat placement alone).
    #[serde(default, skip_serializing_if = "is_false")]
    pub anchor_seam_used: bool,
    /// Frames the winning anchor bracket moved from scan-refined baseline; omitted when 0.
    #[serde(default, skip_serializing_if = "is_zero_frames")]
    pub anchor_bracket_move_frames: usize,
}

/// Inputs for tag derivation during a fill-region patch attempt.
#[derive(Debug, Clone, Copy)]
pub struct GapTagsPatchContext {
    pub fill_mode: FillMode,
    pub thresholds: FillTierThresholds,
    pub signature_mode_label: &'static str,
    pub fit_used_boundary_grid: bool,
    pub anchor_seam_used: bool,
    pub anchor_bracket_move_frames: usize,
    pub anchor_trusted: bool,
    pub residual: Option<SeamResidualVerdict>,
    pub residual_headroom_margin_db: f64,
}

impl GapTagsPatchContext {
    pub fn fit_path_tag(self) -> Option<FitPathTag> {
        if self.fill_mode == FillMode::Fit {
            Some(FitPathTag::from_boundary_grid_used(
                self.fit_used_boundary_grid,
            ))
        } else {
            None
        }
    }

    pub fn signature_mode_tag(self) -> Option<SignatureModeTag> {
        if self.fill_mode == FillMode::Fit {
            SignatureModeTag::parse_label(self.signature_mode_label)
        } else {
            None
        }
    }

    pub fn residual_band_tag(self) -> Option<ResidualBand> {
        self.residual
            .map(|v| classify_residual_band(&v, self.residual_headroom_margin_db))
    }
}

/// Classify seam shape from pre/post scores (guide heuristics).
pub fn classify_seam_shape(pre: f64, post: f64) -> SeamShape {
    let diff = (pre - post).abs();
    if post >= 0.85 && post - pre >= 0.35 {
        return SeamShape::AsymmetricPost;
    }
    if pre >= 0.85 && pre - post >= 0.35 {
        return SeamShape::AsymmetricPre;
    }
    if pre < 0.27 && post < 0.27 && diff <= 0.10 {
        return SeamShape::SymmetricWeak;
    }
    if pre >= 0.27 && post >= 0.27 && diff <= 0.15 {
        return SeamShape::Balanced;
    }
    SeamShape::NotApplicable
}

/// Map correlation skip scores to dead zone vs hard skip.
pub fn patch_tier_from_correlation_skip(
    pre: f64,
    post: f64,
    thresholds: FillTierThresholds,
) -> PatchTier {
    let min_score = pre.min(post);
    let hard_floor = f64::from(effective_fill_absolute_floor(
        thresholds.min_fill_correlation,
        thresholds.fill_absolute_floor,
    ));
    if min_score < hard_floor {
        return PatchTier::HardSkip;
    }
    let marginal_floor =
        f64::from(thresholds.min_fill_correlation - thresholds.fill_marginal_margin);
    if min_score < marginal_floor {
        PatchTier::DeadZone
    } else {
        // Correlation skip with scores still in marginal/high band — treat as dead zone for fit.
        PatchTier::DeadZone
    }
}

fn patch_tier_from_patched(confidence: FillConfidence, fill_mode: FillMode) -> PatchTier {
    if fill_mode != FillMode::Fit {
        return PatchTier::NotApplicable;
    }
    match confidence {
        FillConfidence::High => PatchTier::High,
        FillConfidence::Marginal => PatchTier::Marginal,
    }
}

/// Derive tags from a per-gap patch attempt outcome (fillable plan region).
pub fn derive_gap_tags_from_patch_outcome(
    outcome: &GapPatchTierInput<'_>,
    ctx: GapTagsPatchContext,
) -> GapTags {
    let (patch_tier, seam_shape) = match outcome {
        GapPatchTierInput::Patched {
            pre,
            post,
            confidence,
        } => {
            let tier = if ctx.anchor_trusted && ctx.fill_mode == FillMode::Fit {
                PatchTier::AnchorTrusted
            } else {
                patch_tier_from_patched(*confidence, ctx.fill_mode)
            };
            let seam = if ctx.fill_mode == FillMode::Fit {
                classify_seam_shape(*pre, *post)
            } else {
                SeamShape::NotApplicable
            };
            (tier, seam)
        }
        GapPatchTierInput::Skipped(reason) => match reason {
            GapPatchSkipReason::BoundaryAlignmentFailed => {
                (PatchTier::StructureFail, SeamShape::NotApplicable)
            }
            GapPatchSkipReason::CorrelationBelowThreshold {
                pre_correlation,
                post_correlation,
                ..
            } => {
                let tier = if ctx.fill_mode == FillMode::Fit {
                    patch_tier_from_correlation_skip(
                        *pre_correlation,
                        *post_correlation,
                        ctx.thresholds,
                    )
                } else {
                    PatchTier::NotApplicable
                };
                let seam = if ctx.fill_mode == FillMode::Fit {
                    classify_seam_shape(*pre_correlation, *post_correlation)
                } else {
                    SeamShape::NotApplicable
                };
                (tier, seam)
            }
            _ => (PatchTier::NotApplicable, SeamShape::NotApplicable),
        },
    };

    GapTags {
        plan_kind: PlanKind::Fillable,
        plan_skip_reason: None,
        patch_tier,
        seam_shape,
        fit_path: ctx.fit_path_tag(),
        signature_mode: ctx.signature_mode_tag(),
        residual_band: ctx.residual_band_tag(),
        anchor_seam_used: ctx.anchor_seam_used,
        anchor_bracket_move_frames: if ctx.anchor_seam_used {
            ctx.anchor_bracket_move_frames
        } else {
            0
        },
    }
}

/// Derive tags from a report-order [`GapPatchStatus`].
pub fn derive_gap_tags_from_status(
    status: &GapPatchStatus,
    fill_mode: FillMode,
    thresholds: FillTierThresholds,
) -> GapTags {
    match status {
        GapPatchStatus::NotPlanned { reason } => {
            let plan_kind = match reason {
                GapFillSkipReason::NotFillable => PlanKind::Unfillable,
                GapFillSkipReason::OutsideReferenceCoverage
                | GapFillSkipReason::TrackLayoutMismatch
                | GapFillSkipReason::TrackCompatibilityUnavailable => PlanKind::NotPlanned,
            };
            GapTags {
                plan_kind,
                plan_skip_reason: Some(reason.clone()),
                patch_tier: PatchTier::NotApplicable,
                seam_shape: SeamShape::NotApplicable,
                fit_path: None,
                signature_mode: None,
                residual_band: None,
                anchor_seam_used: false,
                anchor_bracket_move_frames: 0,
            }
        }
        GapPatchStatus::Patched {
            pre_correlation,
            post_correlation,
            confidence,
            anchor_seam_used,
            anchor_bracket_move_frames,
            ..
        } => derive_gap_tags_from_patch_outcome(
            &GapPatchTierInput::Patched {
                pre: *pre_correlation,
                post: *post_correlation,
                confidence: *confidence,
            },
            GapTagsPatchContext {
                fill_mode,
                thresholds,
                signature_mode_label: "bool",
                fit_used_boundary_grid: false,
                anchor_seam_used: *anchor_seam_used,
                anchor_bracket_move_frames: *anchor_bracket_move_frames,
                anchor_trusted: false,
                residual: None,
                residual_headroom_margin_db: DEFAULT_RESIDUAL_HEADROOM_MARGIN_DB,
            },
        ),
        GapPatchStatus::Skipped { reason } => derive_gap_tags_from_patch_outcome(
            &GapPatchTierInput::Skipped(reason),
            GapTagsPatchContext {
                fill_mode,
                thresholds,
                signature_mode_label: "bool",
                fit_used_boundary_grid: false,
                anchor_seam_used: false,
                anchor_bracket_move_frames: 0,
                anchor_trusted: false,
                residual: None,
                residual_headroom_margin_db: DEFAULT_RESIDUAL_HEADROOM_MARGIN_DB,
            },
        ),
    }
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_zero_frames(frames: &usize) -> bool {
    *frames == 0
}

/// Patch outcome view for tag derivation (application layer).
pub enum GapPatchTierInput<'a> {
    Patched {
        pre: f64,
        post: f64,
        confidence: FillConfidence,
    },
    Skipped(&'a GapPatchSkipReason),
}

impl<'a> From<&'a RegionPatchOutcomeView<'a>> for GapPatchTierInput<'a> {
    fn from(view: &'a RegionPatchOutcomeView<'a>) -> Self {
        match view {
            RegionPatchOutcomeView::Patched {
                pre_correlation,
                post_correlation,
                confidence,
            } => GapPatchTierInput::Patched {
                pre: *pre_correlation,
                post: *post_correlation,
                confidence: *confidence,
            },
            RegionPatchOutcomeView::Skipped(reason) => GapPatchTierInput::Skipped(reason),
        }
    }
}

/// Minimal outcome view for tag derivation without coupling to application enums.
#[derive(Debug, Clone, Copy)]
pub enum RegionPatchOutcomeView<'a> {
    Patched {
        pre_correlation: f64,
        post_correlation: f64,
        confidence: FillConfidence,
    },
    Skipped(&'a GapPatchSkipReason),
}

fn format_plan_skip_reason(reason: &GapFillSkipReason) -> &'static str {
    match reason {
        GapFillSkipReason::NotFillable => "not_fillable",
        GapFillSkipReason::OutsideReferenceCoverage => "outside_reference_coverage",
        GapFillSkipReason::TrackLayoutMismatch => "track_layout_mismatch",
        GapFillSkipReason::TrackCompatibilityUnavailable => "track_compatibility_unavailable",
    }
}

/// Compact suffix for the human gap table status column: ` [marginal · post-strong]`.
pub fn format_gap_tags_status_suffix(tags: &GapTags) -> String {
    match (tags.patch_tier.status_label(), tags.seam_shape.status_label()) {
        (None, None) => String::new(),
        (Some(tier), None) => format!(" [{tier}]"),
        (None, Some(seam)) => format!(" [{seam}]"),
        (Some(tier), Some(seam)) => format!(" [{tier} · {seam}]"),
    }
}

/// Verbose stderr line (`-v`): `gap tags: plan=… tier=… seam=…`.
pub fn format_gap_tags_verbose_line(tags: &GapTags) -> String {
    let mut parts = vec![
        format!("plan={}", tags.plan_kind.as_str()),
        format!("tier={}", tags.patch_tier.as_str()),
        format!("seam={}", tags.seam_shape.as_str()),
    ];
    if let Some(reason) = &tags.plan_skip_reason {
        parts.push(format!("plan_skip={}", format_plan_skip_reason(reason)));
    }
    if let Some(fit_path) = tags.fit_path {
        parts.push(format!("fit_path={}", fit_path.as_str()));
    }
    if let Some(mode) = tags.signature_mode {
        parts.push(format!("signature_mode={}", mode.as_str()));
    }
    if let Some(band) = tags.residual_band {
        parts.push(format!("residual_band={}", band.as_str()));
    }
    if tags.anchor_seam_used {
        parts.push("anchor_seam=true".to_string());
        if tags.anchor_bracket_move_frames > 0 {
            parts.push(format!(
                "anchor_move_frames={}",
                tags.anchor_bracket_move_frames
            ));
        }
    }
    format!("           gap tags: {}", parts.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::patch_result::GapPatchSkipReason;

    fn patch_ctx() -> GapTagsPatchContext {
        GapTagsPatchContext {
            fill_mode: FillMode::Fit,
            thresholds: FillTierThresholds::DEFAULT,
            signature_mode_label: "bool",
            fit_used_boundary_grid: false,
            anchor_seam_used: false,
            anchor_bracket_move_frames: 0,
            anchor_trusted: false,
            residual: None,
            residual_headroom_margin_db: DEFAULT_RESIDUAL_HEADROOM_MARGIN_DB,
        }
    }

    fn tags_from_skip(pre: f64, post: f64) -> GapTags {
        let reason = GapPatchSkipReason::CorrelationBelowThreshold {
            pre_correlation: pre,
            post_correlation: post,
            min_correlation: 0.35,
        };
        derive_gap_tags_from_patch_outcome(
            &GapPatchTierInput::Skipped(&reason),
            patch_ctx(),
        )
    }

    fn tags_from_patched(pre: f64, post: f64, confidence: FillConfidence) -> GapTags {
        derive_gap_tags_from_patch_outcome(
            &GapPatchTierInput::Patched {
                pre,
                post,
                confidence,
            },
            patch_ctx(),
        )
    }

    #[test]
    fn seam_shape_guide_examples() {
        assert_eq!(classify_seam_shape(0.6, 0.5), SeamShape::Balanced);
        assert_eq!(classify_seam_shape(0.30, 0.32), SeamShape::Balanced);
        assert_eq!(classify_seam_shape(0.28, 1.0), SeamShape::AsymmetricPost);
        assert_eq!(classify_seam_shape(0.23, 1.0), SeamShape::AsymmetricPost);
        assert_eq!(classify_seam_shape(0.14, 0.14), SeamShape::SymmetricWeak);
    }

    #[test]
    fn patch_tier_guide_w4_boundary_skip() {
        let tags = tags_from_skip(0.23, 1.0);
        assert_eq!(tags.patch_tier, PatchTier::DeadZone);
        assert_eq!(tags.seam_shape, SeamShape::AsymmetricPost);
        assert_eq!(tags.plan_kind, PlanKind::Fillable);
        assert_eq!(tags.fit_path, Some(FitPathTag::BaselineOnly));
    }

    #[test]
    fn patch_tier_guide_w3_marginal_asymmetric() {
        let tags = tags_from_patched(0.28, 1.0, FillConfidence::Marginal);
        assert_eq!(tags.patch_tier, PatchTier::Marginal);
        assert_eq!(tags.seam_shape, SeamShape::AsymmetricPost);
    }

    #[test]
    fn patch_tier_guide_w5_symmetric_weak() {
        let tags = tags_from_skip(0.14, 0.14);
        assert_eq!(tags.patch_tier, PatchTier::DeadZone);
        assert_eq!(tags.seam_shape, SeamShape::SymmetricWeak);
    }

    #[test]
    fn patch_tier_hard_skip_below_floor() {
        let tags = tags_from_skip(0.05, 0.04);
        assert_eq!(tags.patch_tier, PatchTier::HardSkip);
    }

    #[test]
    fn patch_tier_structure_fail() {
        let tags = derive_gap_tags_from_patch_outcome(
            &GapPatchTierInput::Skipped(&GapPatchSkipReason::BoundaryAlignmentFailed),
            patch_ctx(),
        );
        assert_eq!(tags.patch_tier, PatchTier::StructureFail);
        assert_eq!(tags.seam_shape, SeamShape::NotApplicable);
        assert_eq!(
            format_gap_tags_status_suffix(&tags),
            " [structure fail]"
        );
    }

    #[test]
    fn status_suffix_marginal_asymmetric_post() {
        let tags = tags_from_patched(0.31, 1.0, FillConfidence::Marginal);
        assert_eq!(
            format_gap_tags_status_suffix(&tags),
            " [marginal · post-strong]"
        );
    }

    #[test]
    fn status_suffix_hard_skip_symmetric_weak() {
        let tags = tags_from_skip(0.03, 0.03);
        assert_eq!(
            format_gap_tags_status_suffix(&tags),
            " [hard skip · weak both sides]"
        );
    }

    #[test]
    fn status_suffix_dead_zone_asymmetric_post() {
        let tags = tags_from_skip(0.18, 0.87);
        assert_eq!(
            format_gap_tags_status_suffix(&tags),
            " [dead zone · post-strong]"
        );
    }

    #[test]
    fn status_suffix_anchor_trusted_symmetric_weak() {
        let mut ctx = patch_ctx();
        ctx.anchor_trusted = true;
        ctx.anchor_seam_used = true;
        let tags = derive_gap_tags_from_patch_outcome(
            &GapPatchTierInput::Patched {
                pre: 0.31,
                post: 0.29,
                confidence: FillConfidence::Marginal,
            },
            ctx,
        );
        assert_eq!(tags.patch_tier, PatchTier::AnchorTrusted);
        assert_eq!(
            format_gap_tags_status_suffix(&tags),
            " [anchor trusted · balanced]"
        );
    }

    #[test]
    fn plan_not_planned_tags() {
        let status = GapPatchStatus::NotPlanned {
            reason: GapFillSkipReason::OutsideReferenceCoverage,
        };
        let tags = derive_gap_tags_from_status(&status, FillMode::Fit, FillTierThresholds::DEFAULT);
        assert_eq!(tags.plan_kind, PlanKind::NotPlanned);
        assert_eq!(
            tags.plan_skip_reason,
            Some(GapFillSkipReason::OutsideReferenceCoverage)
        );
        assert_eq!(tags.patch_tier, PatchTier::NotApplicable);
    }

    #[test]
    fn verbose_line_includes_anchor_seam_metadata() {
        let mut ctx = patch_ctx();
        ctx.anchor_seam_used = true;
        ctx.anchor_bracket_move_frames = 1_200;
        ctx.anchor_trusted = true;
        let tags = derive_gap_tags_from_patch_outcome(
            &GapPatchTierInput::Patched {
                pre: 0.31,
                post: 0.29,
                confidence: FillConfidence::Marginal,
            },
            ctx,
        );
        assert!(tags.anchor_seam_used);
        assert_eq!(tags.anchor_bracket_move_frames, 1_200);
        let line = format_gap_tags_verbose_line(&tags);
        assert!(line.contains("anchor_seam=true"));
        assert!(line.contains("anchor_move_frames=1200"));
    }

    #[test]
    fn derive_from_status_preserves_anchor_metadata() {
        let status = GapPatchStatus::Patched {
            pre_correlation: 0.5,
            post_correlation: 0.48,
            align_adjustment_secs: 0.0,
            waveform_adjustment_secs: 0.0,
            structure_trusted: false,
            confidence: FillConfidence::High,
            gap_start_adjust_frames: 0,
            gap_end_adjust_frames: 0,
            residual_db: None,
            floor_db: None,
            headroom_db: None,
            anchor_seam_used: true,
            anchor_bracket_move_frames: 900,
        };
        let tags = derive_gap_tags_from_status(&status, FillMode::Fit, FillTierThresholds::DEFAULT);
        assert!(tags.anchor_seam_used);
        assert_eq!(tags.anchor_bracket_move_frames, 900);
    }

    #[test]
    fn verbose_line_includes_core_tags() {
        let line = format_gap_tags_verbose_line(&tags_from_skip(0.23, 1.0));
        assert!(line.contains("gap tags:"));
        assert!(line.contains("plan=fillable"));
        assert!(line.contains("tier=dead_zone"));
        assert!(line.contains("seam=asymmetric_post"));
        assert!(line.contains("fit_path=baseline_only"));
        assert!(line.contains("signature_mode=bool"));
    }

    #[test]
    fn residual_band_cancels_when_informative_low_headroom() {
        use crate::domain::policies::{
            SeamFloorProbe, SeamFloorSource, SeamResidualVerdict,
        };
        let verdict = SeamResidualVerdict::from_parts_with_placement(
            &SeamFloorProbe {
                residual_db: -40.0,
                source: SeamFloorSource::Border,
                best_lag: 0,
                gain: 1.0,
            },
            &SeamFloorProbe {
                residual_db: -40.0,
                source: SeamFloorSource::Border,
                best_lag: 0,
                gain: 1.0,
            },
            &SeamFloorProbe {
                residual_db: -44.0,
                source: SeamFloorSource::Border,
                best_lag: 0,
                gain: 1.0,
            },
            &SeamFloorProbe {
                residual_db: -44.0,
                source: SeamFloorSource::Border,
                best_lag: 0,
                gain: 1.0,
            },
            -15.0,
            0,
            480,
        );
        assert_eq!(
            classify_residual_band(&verdict, DEFAULT_RESIDUAL_HEADROOM_MARGIN_DB),
            ResidualBand::Cancels
        );
    }

    #[test]
    fn residual_band_correlates_only_when_headroom_high() {
        use crate::domain::policies::{
            SeamFloorProbe, SeamFloorSource, SeamResidualVerdict,
        };
        let verdict = SeamResidualVerdict::from_parts_with_placement(
            &SeamFloorProbe {
                residual_db: -20.0,
                source: SeamFloorSource::Border,
                best_lag: 0,
                gain: 1.0,
            },
            &SeamFloorProbe {
                residual_db: -20.0,
                source: SeamFloorSource::Border,
                best_lag: 0,
                gain: 1.0,
            },
            &SeamFloorProbe {
                residual_db: -44.0,
                source: SeamFloorSource::Border,
                best_lag: 0,
                gain: 1.0,
            },
            &SeamFloorProbe {
                residual_db: -44.0,
                source: SeamFloorSource::Border,
                best_lag: 0,
                gain: 1.0,
            },
            -15.0,
            0,
            480,
        );
        assert_eq!(
            classify_residual_band(&verdict, DEFAULT_RESIDUAL_HEADROOM_MARGIN_DB),
            ResidualBand::CorrelatesOnly
        );
    }
}
