pub mod alignment;
pub mod audio_timeline;
pub mod bit_depth;
pub mod audio_track;
pub mod human_format;
pub mod clip_plan;
pub mod clip_window;
pub mod error;
pub mod media_source;
pub mod mono_pcm_clip;
pub mod multichannel_pcm;
pub mod media_extent;
pub mod pcm_preparation;
pub mod policies;
pub mod query_localization;

pub use query_localization::{
    build_query_alignment_result, compute_mapped_region, winning_window_on_a_timeline,
    AlignmentModeUsed, QueryLocalization, ReferenceLocalizationOutcome,
};

pub use alignment::{
    AlignmentMergePolicy, AlignmentResult, ClipMatch, ClipMatchEstimate, ClipPairReportInput,
    ClipRepetitionReport, Fingerprint, HighRateAnchorRefinement, HighRateRefinement,
    OffsetVerification,
    OFFSET_AGREEMENT_TOLERANCE_SECS,
    RepetitionFinding, build_alignment_result, compute_clip_timeline_overlap,
    refresh_alignment_drift_summary, refresh_recommended_offset, refresh_start_overlap,
    periodic_ambiguity_period, periodic_recheck_period_multiple,
    display_repeat_period,
    set_offset_ambiguous_mod_from_start_clip,
    should_downgrade_periodic_ambiguity, should_downgrade_repetition_confidence,
};
pub use audio_timeline::AudioTimelineSkew;
pub use audio_track::AudioTrack;
pub use bit_depth::{resolve_output_bit_depth, BitDepth, WavBitDepth};
pub use clip_plan::ClipPlan;
pub use clip_window::{ClipLabel, ClipWindow};
pub use error::DomainError;
pub use human_format::{
    format_time_range, format_time_range_verbose, format_timestamp, format_timestamp_verbose,
    VERBOSE_SUBSECOND_SPAN_SECS,
};
pub use alignment::TimelineOverlap;
pub use media_source::MediaSource;
pub use mono_pcm_clip::{MonoPcmClip, MonoScanBucket};
pub use multichannel_pcm::{InterleavedScanBucket, MultiChannelPcm};
pub use pcm_preparation::{
    expand_window_for_slide, prepare_clip_for_fingerprint, select_aligned_subclip_pair,
    PcmPreparationOptions,
};
pub use media_extent::MediaExtent;
#[allow(unused_imports)]
pub use policies::{
    attach_symmetric_planning_report_metadata, clip_windows_paired, clip_windows_with_options,
    end_clip_extract_unreliable, holdout_pick_duration,
    holdout_extract_sufficient, holdout_window_centered_in, anchor_holdout_candidates,
    holdout_b_window_for_offset, holdout_window_feasible,
    interior_overlaps_fixed_clip, interior_windows_along_timeline,
    parallel_holdout_window_candidates, resolve_holdout_candidates,
    should_use_query_mode, truncate_padded_tail,
    ClipPlanningOptions, EndClipAnchor, INTERIOR_OVERLAP_TOLERANCE, select_best_track,
    select_track_for_reference, order_track_pairs_for_alignment,
};