mod application;
mod domain;
mod infrastructure;

// --- application ---
pub use application::config::{
    AlignConfig, AlignmentConfig, ChromaprintPreset, ClipConfig,
};
pub use application::default_pipeline::align_with_defaults;
pub use application::{AlignVideos, AlignVideosRequest, AlignVideosResponse, AppError, ConfigError};
pub use application::ports::{Aligner, Fingerprinter, MediaReader, MediaSession, ProgressReporter};
pub use application::offset_refinement::aligned_slice_starts;

// --- domain (selected types) ---
pub use domain::{
    AlignmentResult, AudioTrack, ClipMatch, ClipMatchEstimate, ClipWindow, ClipLabel,
    DomainError, Fingerprint, HighRateRefinement, MediaSource, MonoPcmClip,
};

// --- default adapter types ---
pub use infrastructure::chromaprint::{ChromaprintAligner, ChromaprintFingerprinter};
pub use infrastructure::symphonia::SymphoniaMediaReader;
pub use infrastructure::config::file::load_align_config;
pub use infrastructure::logging::{
    init_tracing, LoggingConfig, LogLevel, ProgressMode, StderrProgressReporter,
};

#[cfg(feature = "test-utils")]
pub use application::testing;
