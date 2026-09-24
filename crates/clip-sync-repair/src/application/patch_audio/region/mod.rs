//! Region-level patching, split by responsibility.
//!
//! The folder's dependency DAG runs one way: `types` (leaf) <- `pcm` <- {`dual_fit`, `bracket`}
//! <- `execute` <- `characterize`. Siblings see each other's `pub(in ...::region)` items directly;
//! this module only re-exports the surface `patch_audio` itself consumes.

mod bracket;
mod characterize;
mod dual_fit;
mod execute;
mod pcm;
mod types;

pub(super) use characterize::{characterize_region, prepare_region_patch};
pub(super) use execute::execute_region_spec;
pub(super) use pcm::{gained_fill, gained_seam_margin, splice_into_a, SpliceFill};
/// Pre-split `pub(super)` surface whose only consumer is `anchor_retry.rs`'s unit tests.
#[cfg(test)]
pub(super) use types::skipped_patch;
pub(super) use types::{
    outcome_from_spec, outcomes_in_report_order, record_patch_gap_span, region_outcome_gap_tags,
    skip_outcome_from_fields, RegionPatch, RegionPatchContext, RegionPatchMedia, RegionPatchOpts,
    RegionPatchOutcome, ReportStamps,
};
