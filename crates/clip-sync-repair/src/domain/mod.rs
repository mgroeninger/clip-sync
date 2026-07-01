pub mod cross_check;
pub mod diagnostics;
pub mod fill_mode;
pub mod fill_offset;
pub mod gap_fill_fit;
pub mod gap;
pub mod gap_seam_extend;
pub mod gap_anchor_seam;
pub mod gap_fill;
pub mod gap_energy;
pub mod gap_signature;
pub mod gap_tags;
pub mod gap_structure;
pub mod patch_anchor;
pub mod patch_result;
pub mod policies;
pub mod repair_profile;
pub mod residual_gate;
pub mod seam_robust;
pub mod track_match;

pub use fill_mode::FillMode;
pub use fill_offset::{AnchoredRetryPass, FillOffsetMode, fill_offset_secs, resolve_gap_offset_secs};
pub use patch_anchor::{
    is_retryable_patch_skip, AnchorSearchPrior, PatchAnchorCandidate, PatchAnchorPolicy,
    PatchAnchorReport, PatchAnchorTable, PatchOffsetAnchor,
};
pub use gap_tags::{
    classify_residual_band, classify_seam_shape, derive_donor_relation,
    derive_gap_tags_from_patch_outcome, derive_gap_tags_from_status,
    format_gap_tags_status_suffix, format_gap_tags_verbose_line, DonorRelation, FillTierThresholds, FitPathTag,
    GapPatchTierInput, GapTags, GapTagsPatchContext, PatchTier, PlanKind,
    RegionPatchOutcomeView, ResidualBand, SeamShape, SignatureModeTag,
};
pub use gap_anchor_seam::{
    anchor_bracket_both_matchable, anchor_matchable_on_a, list_anchor_candidates_a,
    list_feasible_anchor_brackets, matchability_at_anchor, should_run_anchor_seam,
    AnchorBracket, AnchorCandidate, AnchorCandidateSet, AnchorMatchability,
    AnchorMatchabilityParams, AnchorSeamMode, AnchorSeamParams, AnchorSeamSide, AnchorSource,
    DEFAULT_ANCHOR_MATCH_MIN_PEARSON, DEFAULT_ANCHOR_MATCH_MIN_XCORR_PEAK,
    DEFAULT_ANCHOR_MATCH_XCORR_AMBIGUOUS_BAND,
};
pub use gap_signature::{GapSignature, GapSignatureMode};
pub use gap_fill_fit::{
    anchor_trust_applies, FillConfidence, ResidualGateError, apply_residual_to_confidence,
    classify_fill_waveform_confidence,
};
pub use residual_gate::{
    ResidualGateMode, DEFAULT_RESIDUAL_HEADROOM_MARGIN_DB, DEFAULT_RESIDUAL_LAG_SECS,
    residual_max_lag_frames,
};
pub use repair_profile::{
    boundary_grid_may_run, format_repair_profile_verbose, gap_extension_slack_secs,
    inactive_repair_flag_notes, FitBoundarySearch, RepairPatchConfigView, RepairProfile,
    RepairProfileBundle, RepairProfileFieldMask,
};

pub use gap::{Gap, GapOffsetAgreement, GapReport};
pub use gap_fill::{build_gap_fill_plan, FillRegion, GapFillPlan, GapFillSkipped};
pub use patch_result::{
    format_gap_fill_marginal_detail, format_gap_fill_marginal_verbose_line,
    format_gap_fill_marginal_warn_reason, format_gap_fill_skip_verbose_line,
    format_gap_patch_skip_reason, format_gap_patch_skip_warn_reason,
    residual_summary_scalar_fields, GapFillSkipReason,
    GapPatchOutcome, GapPatchSkipReason, GapPatchStatus, PatchSummary,
};
pub use track_match::{
    assess_track_compatibility, CompatibilityVerdict, TrackCompatibility, TrackDescriptor,
};
