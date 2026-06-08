pub mod alignment;
pub mod audio_track;
pub mod clip_plan;
pub mod clip_window;
pub mod error;
pub mod media_source;
pub mod mono_pcm_clip;
pub mod multichannel_pcm;
pub mod pcm_preparation;
pub mod policies;
pub mod resample;

pub use alignment::{
    AlignmentMergePolicy, AlignmentResult, ClipMatch, ClipMatchEstimate, ClipPairReportInput,
    Fingerprint, HighRateRefinement, build_alignment_result, refresh_start_overlap,
};
pub use audio_track::AudioTrack;
pub use clip_plan::ClipPlan;
pub use clip_window::{ClipLabel, ClipWindow};
pub use error::DomainError;
pub use alignment::TimelineOverlap;
pub use media_source::MediaSource;
pub use mono_pcm_clip::{MonoPcmClip, MonoScanBucket};
pub use multichannel_pcm::MultiChannelPcm;
pub use pcm_preparation::{
    expand_window_for_slide, prepare_clip_for_fingerprint, select_aligned_subclip_pair,
    PcmPreparationOptions,
};
pub use policies::{
    clip_windows, decoded_timeline_extent, holdout_window_candidates, holdout_window_feasible,
    select_best_track,
};
pub use resample::{resample_interleaved, resample_mono_pcm};