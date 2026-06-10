mod application;
mod domain;
mod infrastructure;

// --- application ---
pub use application::config::{
    AlignConfig, AlignmentConfig, ChromaprintPreset, ClipConfig,
};
pub use application::default_pipeline::align_with_defaults;
pub use application::{AlignVideos, AlignVideosRequest, AlignVideosResponse, AppError, ConfigError};
pub use application::error::{AlignmentError, FingerprintError, MediaError};
pub use application::ports::{Aligner, Fingerprinter, MediaReader, MediaSession, ProgressReporter};
// --- domain (selected types) ---
pub use domain::{
    format_high_rate_refinement_lines, format_offset_verification_lines, format_time_range,
    format_timestamp, AlignmentResult, AudioTrack, ClipMatch,
    ClipMatchEstimate, ClipRepetitionReport, ClipWindow, ClipLabel, DomainError, Fingerprint,
    HighRateRefinement, InterleavedScanBucket, MediaSource, MonoPcmClip, MonoScanBucket,
    MultiChannelPcm, OffsetVerification, RepetitionFinding, TimelineOverlap,
};
pub use domain::policies::{select_best_track, select_track_for_reference};
pub use domain::{resample_interleaved, resample_mono_pcm};
pub use application::offset_refinement::normalized_correlation;

// --- default adapter types ---
pub use infrastructure::chromaprint::{ChromaprintAligner, ChromaprintFingerprinter};
pub use infrastructure::symphonia::SymphoniaMediaReader;
pub use infrastructure::config::file::load_align_config;
pub use infrastructure::logging::{
    FINGERPRINT_ALIGN_STAGE, LoggingConfig, LogLevel, ProgressMode, StderrProgressReporter,
};
#[cfg(feature = "default-tracing")]
pub use infrastructure::logging::init_tracing;

#[cfg(feature = "test-utils")]
pub use application::testing;
