pub mod align;
pub mod cross_check;
pub mod diagnostics;
pub mod donor;
pub mod dual_fit;
pub mod fill_mode;
pub mod fill_offset;
pub mod gap;
pub mod gap_anchor_seam;
pub mod gap_energy;
pub mod gap_equivalence;
pub mod gap_fill;
pub mod gap_fill_fit;
pub mod gap_repair_spec;
pub mod gap_seam_extend;
pub mod gap_signature;
pub mod gap_structure;
pub mod gap_tags;
pub mod metrics;
pub mod patch_anchor;
pub mod patch_result;
pub mod pcm;
pub mod policies;
pub mod ports;
pub mod repair_profile;
pub mod residual_gate;
pub mod seam_local;
pub mod seam_robust;
pub mod track_match;

pub use align::{AlignedClip, AudioTimelineSkew, ClipRole, ScanAlignment, TimelineOverlap};

pub use fill_mode::FillMode;
pub use fill_offset::{
    fill_offset_secs, resolve_gap_offset_secs, AnchoredRetryPass, FillOffsetMode,
};
pub use gap_anchor_seam::{
    anchor_bracket_both_matchable, anchor_matchable_on_a, list_anchor_candidates_a,
    list_feasible_anchor_brackets, matchability_at_anchor, should_run_anchor_seam, AnchorBracket,
    AnchorCandidate, AnchorCandidateSet, AnchorMatchability, AnchorMatchabilityParams,
    AnchorSeamMode, AnchorSeamParams, AnchorSeamSide, AnchorSource,
    DEFAULT_ANCHOR_MATCH_MIN_PEARSON, DEFAULT_ANCHOR_MATCH_MIN_XCORR_PEAK,
    DEFAULT_ANCHOR_MATCH_XCORR_AMBIGUOUS_BAND,
};
pub use gap_fill_fit::{
    anchor_trust_applies, apply_residual_to_confidence, classify_fill_waveform_confidence,
    FillConfidence, ResidualGateError,
};
pub use gap_signature::{GapSignature, GapSignatureMode};
pub use gap_tags::{
    classify_residual_band, classify_seam_shape, derive_donor_relation,
    derive_gap_tags_from_patch_outcome, derive_gap_tags_from_status, format_gap_tags_status_suffix,
    format_gap_tags_verbose_line, DonorRelation, FillTierThresholds, FitPathTag, GapPatchTierInput,
    GapTags, GapTagsPatchContext, PatchTier, PlanKind, RegionPatchOutcomeView, ResidualBand,
    SeamShape, SignatureModeTag,
};
pub use patch_anchor::{
    is_retryable_patch_skip, AnchorSearchPrior, PatchAnchorCandidate, PatchAnchorPolicy,
    PatchAnchorReport, PatchAnchorTable, PatchOffsetAnchor,
};
pub use repair_profile::{
    boundary_grid_may_run, format_repair_profile_verbose, gap_extension_slack_secs,
    inactive_repair_flag_notes, FitBoundarySearch, RepairPatchConfigView, RepairProfile,
    RepairProfileBundle, RepairProfileFieldMask,
};
pub use residual_gate::{
    residual_max_lag_frames, ResidualGateMode, DEFAULT_RESIDUAL_HEADROOM_MARGIN_DB,
    DEFAULT_RESIDUAL_LAG_SECS,
};

pub use gap::{Gap, GapOffsetAgreement, GapReport};
pub use gap_fill::{build_gap_fill_plan, FillRegion, GapFillPlan, GapFillSkipped};
pub use gap_repair_spec::{
    cell_for_skip_reason, reason_admits_cell, skip_cell_from_tags, BExtractWindow, GapRepairCell,
    GapRepairPlan, GapRepairSpec, GapRepairStrategy, GapRepairTags, GapRepairVerdict, GateTags,
    LevelTags, Placement, RegistrationTags, SeamLocalTags,
};
pub use patch_result::{
    format_gap_fill_marginal_detail, format_gap_fill_marginal_verbose_line,
    format_gap_fill_marginal_warn_reason, format_gap_fill_skip_verbose_line,
    format_gap_patch_skip_reason, format_gap_patch_skip_warn_reason,
    residual_summary_scalar_fields, GapFillSkipReason, GapPatchOutcome, GapPatchSkipReason,
    GapPatchStatus, PatchSummary,
};
pub use track_match::{
    assess_track_compatibility, CompatibilityVerdict, TrackCompatibility, TrackDescriptor,
};
