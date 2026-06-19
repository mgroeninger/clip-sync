mod application;
mod domain;
mod infrastructure;
#[cfg(any(test, feature = "test-utils"))]
mod test_support;

// --- application ---
pub use application::config::{
    AlignConfig, AlignmentConfig, AlignmentMode, ChromaprintPreset, ClipConfig,
};
pub use application::default_pipeline::align_with_defaults;
pub use application::{AlignVideos, AlignVideosRequest, AlignVideosResponse, AppError, ConfigError};
pub use application::error::{AlignmentError, FingerprintError, MediaError};
pub use application::ports::{
    Aligner, Fingerprinter, MediaReader, MediaSession, PcmCorrelator, ProgressReporter, Resampler,
};
pub use application::report::{
    format_end_clip_anchor_line, format_high_rate_refinement_lines, format_offset_verification_lines,
    format_periodic_ambiguity_line, format_query_localization_lines,
    format_symmetric_clip_window_line, AlignmentModeUsedReport,
    AlignmentReport, ClipLabelReport, ClipMatchReport, EndClipAnchorReport,
    HighRateRefinementReport,
    OffsetVerificationReport, QueryLocalizationReport, RepetitionFindingReport, RepetitionReport,
    TimelineOverlapReport,
};
// --- domain (selected types) ---
pub use domain::{
    build_query_alignment_result, compute_mapped_region, format_time_range,
    format_timestamp, AlignmentModeUsed, AlignmentResult, AudioTrack, ClipMatch,
    ClipMatchEstimate, ClipRepetitionReport, ClipWindow, ClipLabel, DomainError, Fingerprint,
    HighRateRefinement, InterleavedScanBucket, MediaExtent, MediaSource, MonoPcmClip, MonoScanBucket,
    MultiChannelPcm, OffsetVerification, QueryLocalization, ReferenceLocalizationOutcome,
    RepetitionFinding, TimelineOverlap, AudioTimelineSkew,
};
pub use domain::policies::{
    attach_symmetric_planning_report_metadata, select_best_track, select_track_for_reference,
    EndClipAnchor, clip_windows_paired,
};
pub use application::offset_refinement::normalized_correlation;

// --- default adapter types ---
pub use infrastructure::chromaprint::{ChromaprintAligner, ChromaprintFingerprinter};
pub use infrastructure::correlation::FftCorrelator;
pub use infrastructure::resample::{resample_interleaved, RubatoResampler};
pub use infrastructure::symphonia::SymphoniaMediaReader;
pub use infrastructure::config::file::load_align_config;
pub use infrastructure::logging::{
    FINGERPRINT_ALIGN_STAGE, LoggingConfig, LogLevel, ProgressMode, StderrProgressReporter,
};
#[cfg(feature = "default-tracing")]
pub use infrastructure::logging::init_tracing;

#[cfg(feature = "test-utils")]
pub use application::testing;
