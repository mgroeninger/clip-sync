//! Repair domain policies: silence, borders, seam scoring/residual, splice.
//!
//! Public paths stay `crate::domain::policies::*` via re-exports from submodules.

mod gap_borders;
mod seam_residual;
mod seam_scoring;
mod seam_splice;
mod silence;

pub(crate) use gap_borders::{adaptive_seam_window_frames, border_active_extent_frames};
pub use gap_borders::{
    border_templates_for_gap, border_templates_per_channel_for_gap, loudest_seam_channel,
    refine_gap_frames, selected_seam_channels, FillAlignment, GapBorderSpec, RefinedGapFrames,
};
pub use seam_residual::{
    floor_probe_informative, residual_verdict_informative, seam_chosen_and_floor,
    seam_chosen_and_floor_multichannel, seam_floor_probe, SeamChannelResidual, SeamFloorParams,
    SeamFloorProbe, SeamFloorSource, SeamResidualVerdict, SeamSide, DEFAULT_RESIDUAL_FLOOR_OK_DB,
};
pub use seam_scoring::{
    fill_repeat_correlations, fill_seam_correlations, fill_splice_seam_correlations,
    fill_splice_seam_correlations_interleaved, seam_channel_diagnostics, BorderSeamTemplates,
    SeamChannelDiagnostics, SeamPlacement, SeamTemplates, SpliceSeamContext,
};
pub(crate) use seam_scoring::{
    fill_repeat_correlations_band, fill_seam_correlations_band,
    fill_seam_correlations_with_channels, seam_score_channels,
};
pub use seam_splice::{apply_seam_crossfade, effective_seam_crossfade_frames};
pub use silence::{
    absolute_silence_floor_db, compute_fill_gain, is_silent, is_silent_frame,
    is_silent_interleaved, rms_interleaved, BlockLevel, SilenceRunScanner, SilentRun,
    BLOCK_LEVEL_FLOOR_DB,
};
