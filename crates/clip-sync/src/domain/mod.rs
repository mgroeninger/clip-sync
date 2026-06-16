pub mod alignment;
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
    ClipRepetitionReport, Fingerprint, HighRateRefinement, OffsetVerification,
    OFFSET_AGREEMENT_TOLERANCE_SECS,
    RepetitionFinding, build_alignment_result, compute_clip_timeline_overlap, refresh_start_overlap,
    periodic_ambiguity_period, periodic_recheck_period_multiple,
    display_repeat_period,
    set_offset_ambiguous_mod_from_start_clip,
    should_downgrade_periodic_ambiguity, should_downgrade_repetition_confidence,
};
pub use audio_track::AudioTrack;
pub use clip_plan::ClipPlan;
pub use clip_window::{ClipLabel, ClipWindow};
pub use error::DomainError;
pub use human_format::{format_time_range, format_timestamp};
pub use alignment::TimelineOverlap;
pub use media_source::MediaSource;
pub use mono_pcm_clip::{MonoPcmClip, MonoScanBucket};
pub use multichannel_pcm::{InterleavedScanBucket, MultiChannelPcm};
pub use pcm_preparation::{
    expand_window_for_slide, prepare_clip_for_fingerprint, select_aligned_subclip_pair,
    PcmPreparationOptions,
};
pub use media_extent::MediaExtent;
pub use policies::{
    clip_windows_with_options,
    end_clip_extract_unreliable, holdout_window_candidates,
    holdout_extract_sufficient, holdout_window_feasible, parallel_holdout_window_candidates,
    should_use_query_mode, truncate_padded_tail,
    ClipPlanningOptions, select_best_track,
};