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
pub use application::error::{AlignmentError, FingerprintError, MediaError};
pub use application::ports::{
    Aligner, ClipRepetitionDetector, Fingerprinter, MediaReader, MediaSession, PcmCorrelator,
    ProgressReporter, Resampler,
};
pub use application::report::{
    format_end_clip_anchor_line, format_high_rate_refinement_lines,
    format_offset_verification_lines, format_periodic_ambiguity_line,
    format_query_localization_lines, format_symmetric_clip_window_line, AlignmentModeUsedReport,
    AlignmentReport, ClipLabelReport, ClipMatchReport, EndClipAnchorReport,
    HighRateRefinementReport, OffsetVerificationReport, QueryLocalizationReport,
    RepetitionFindingReport, RepetitionReport, TimelineOverlapReport,
};
pub use application::{
    AlignVideos, AlignVideosRequest, AlignVideosResponse, AppError, ConfigError,
};
// --- domain (selected types) ---
pub use application::offset_refinement::normalized_correlation;
pub use domain::policies::{
    attach_symmetric_planning_report_metadata, clip_windows_paired,
    order_track_pairs_for_alignment, select_best_track, select_track_for_reference, EndClipAnchor,
};
pub use domain::{
    build_query_alignment_result, compute_mapped_region, format_time_range,
    format_time_range_verbose, format_timestamp, format_timestamp_verbose,
    resolve_output_bit_depth, AlignmentModeUsed, AlignmentResult, AudioTimelineSkew, AudioTrack,
    BitDepth, ClipLabel, ClipMatch, ClipMatchEstimate, ClipRepetitionReport, ClipWindow,
    DomainError, Fingerprint, HighRateRefinement, InterleavedScanBucket, MediaExtent, MediaSource,
    MonoPcmClip, MonoScanBucket, MultiChannelPcm, OffsetVerification, QueryLocalization,
    ReferenceLocalizationOutcome, RepetitionFinding, TimelineOverlap, WavBitDepth,
    VERBOSE_SUBSECOND_SPAN_SECS,
};

// --- default adapter types ---
pub use infrastructure::chromaprint::{ChromaprintAligner, ChromaprintFingerprinter};
pub use infrastructure::config::file::load_align_config;
pub use infrastructure::config::toml_keys::unknown_toml_keys;
pub use infrastructure::correlation::FftCorrelator;
#[cfg(feature = "default-tracing")]
pub use infrastructure::logging::init_tracing;
pub use infrastructure::logging::{
    LogLevel, LoggingConfig, ProgressMode, StderrProgressReporter, FINGERPRINT_ALIGN_STAGE,
};
pub use infrastructure::resample::{resample_interleaved, RubatoResampler};
pub use infrastructure::stdout::write_report_to_stdout;
pub use infrastructure::symphonia::SymphoniaMediaReader;

#[cfg(feature = "test-utils")]
pub use application::testing;
