pub mod cross_check;
pub mod gap;
pub mod gap_fill;
pub mod gap_structure;
pub mod patch_result;
pub mod policies;
pub mod track_match;

pub use gap::{Gap, GapOffsetAgreement, GapReport};
pub use gap_fill::{build_gap_fill_plan, FillRegion, GapFillPlan, GapFillSkipped};
pub use patch_result::{
    GapFillSkipReason, GapPatchOutcome, GapPatchSkipReason, GapPatchStatus, PatchSummary,
};
pub use track_match::{
    assess_track_compatibility, CompatibilityVerdict, TrackCompatibility, TrackDescriptor,
};
