pub mod alignment;
pub mod audio_track;
pub mod clip_plan;
pub mod clip_window;
pub mod error;
pub mod media_source;
pub mod mono_pcm_clip;
pub mod policies;

pub use alignment::{AlignmentResult, Fingerprint};
pub use audio_track::AudioTrack;
pub use clip_plan::ClipPlan;
pub use clip_window::{ClipLabel, ClipWindow};
pub use error::DomainError;
pub use media_source::MediaSource;
pub use mono_pcm_clip::MonoPcmClip;
pub use policies::{clip_windows, select_best_track};
