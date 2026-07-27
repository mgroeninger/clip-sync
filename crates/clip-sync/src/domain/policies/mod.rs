//! Analyzer domain policies: track selection, clip planning, extract quality, hold-out placement.
//!
//! Public paths stay `crate::domain::policies::*` via re-exports from submodules.
//!
//! ```text
//! track_selection  (leaf)
//! clip_planning    (leaf; owns secs_to_duration)
//! extract_quality  (leaf)
//! holdout  ←  clip_planning   (secs_to_duration only)
//! ```

mod clip_planning;
mod extract_quality;
mod holdout;
mod track_selection;

// Re-export the full pre-split `pub` surface (some symbols are only used via
// `crate::domain::policies::` / submodule internals, not via `domain::` re-exports).
#[allow(unused_imports)]
pub use clip_planning::{
    attach_symmetric_planning_report_metadata, clip_windows_paired, clip_windows_with_options,
    effective_timeline_end, interior_overlaps_fixed_clip, interior_windows_along_timeline,
    should_use_query_mode, ClipPlanningOptions, EndClipAnchor, INTERIOR_OVERLAP_TOLERANCE,
};
#[allow(unused_imports)]
pub use extract_quality::{
    end_clip_extract_unreliable, holdout_extract_sufficient, truncate_padded_tail,
};
#[allow(unused_imports)]
pub use holdout::{
    anchor_holdout_candidates, holdout_b_window_for_offset, holdout_pick_duration,
    holdout_window_candidates, holdout_window_centered_in, holdout_window_feasible,
    mapped_region_holdout_candidates, parallel_holdout_window_candidates, pick_holdout_window,
    resolve_holdout_candidates,
};
#[allow(unused_imports)]
pub use track_selection::{
    order_track_pairs_for_alignment, select_best_track, select_track_for_reference,
};
