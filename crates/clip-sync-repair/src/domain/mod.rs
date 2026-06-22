pub mod cross_check;
pub mod diagnostics;
pub mod fill_mode;
pub mod fill_offset;
pub mod gap_fill_fit;
pub mod gap;
pub mod gap_seam_extend;
pub mod gap_fill;
pub mod gap_energy;
pub mod gap_signature;
pub mod gap_structure;
pub mod patch_anchor;
pub mod patch_result;
pub mod policies;
pub mod repair_profile;
pub mod track_match;

pub use fill_mode::FillMode;
pub use fill_offset::{AnchoredRetryPass, FillOffsetMode, fill_offset_secs, resolve_gap_offset_secs};
pub use patch_anchor::{
    is_retryable_patch_skip, AnchorSearchPrior, PatchAnchorCandidate, PatchAnchorPolicy,
    PatchAnchorReport, PatchAnchorTable, PatchOffsetAnchor,
};
pub use gap_signature::{GapSignature, GapSignatureMode};
pub use gap_fill_fit::{FillConfidence, classify_fill_waveform_confidence};
pub use repair_profile::{
    boundary_grid_may_run, format_repair_profile_verbose, gap_extension_slack_secs,
    inactive_repair_flag_notes, FitBoundarySearch, RepairPatchConfigView, RepairProfile,
    RepairProfileBundle, RepairProfileFieldMask,
};

pub use gap::{Gap, GapOffsetAgreement, GapReport};
pub use gap_fill::{build_gap_fill_plan, FillRegion, GapFillPlan, GapFillSkipped};
pub use patch_result::{
    GapFillSkipReason, GapPatchOutcome, GapPatchSkipReason, GapPatchStatus, PatchSummary,
};
pub use track_match::{
    assess_track_compatibility, CompatibilityVerdict, TrackCompatibility, TrackDescriptor,
};
