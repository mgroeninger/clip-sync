pub mod alignment;
pub mod audio_track;
pub mod clip_plan;
pub mod clip_window;
pub mod error;
pub mod media_source;
pub mod mono_pcm_clip;
pub mod pcm_preparation;
pub mod policies;
pub mod resample;

pub use alignment::{
    AlignmentResult, ClipMatch, ClipMatchEstimate, Fingerprint, HighRateRefinement,
    build_alignment_result, refresh_start_overlap,
};
pub use audio_track::AudioTrack;
pub use clip_plan::ClipPlan;
pub use clip_window::{ClipLabel, ClipWindow};
pub use error::DomainError;
pub use media_source::MediaSource;
pub use mono_pcm_clip::MonoPcmClip;
pub use pcm_preparation::{
    expand_window_for_slide, prepare_clip_for_fingerprint, select_aligned_subclip_pair,
    PcmPreparationOptions,
};
pub use policies::{clip_windows, holdout_window_feasible, pick_holdout_window, select_best_track};
pub use resample::resample_mono_pcm;