use crate::application::config::ChromaprintPreset;
use crate::application::error::{AlignmentError, FingerprintError, MediaError};
use std::time::Duration;

use crate::domain::{
    AudioTrack, ClipMatchEstimate, ClipWindow, Fingerprint, InterleavedScanBucket, MediaSource,
    MonoPcmClip, MonoScanBucket, MultiChannelPcm, RepetitionFinding,
};

pub trait ProgressReporter {
    /// Major stage line — shown in default (`Auto`) and `--verbose` modes.
    fn phase(&self, message: &str);

    /// Detail line — shown only with `--verbose` (`ProgressMode::Verbose`).
    fn phase_verbose(&self, message: &str) {
        let _ = message;
    }

    /// When true, clip extraction reports per-clip labels; when false, callers may aggregate
    /// decode progress under a single stage bar.
    fn detailed_extraction_progress(&self) -> bool {
        true
    }

    fn progress(&self, label: &str, current: u64, total: u64);

    /// End an in-progress TTY progress line before unrelated stderr output (e.g. `tracing` logs).
    fn flush_progress(&self) {}
}

pub trait MediaReader {
    type Session: MediaSession + Send;

    fn open(&self, source: &MediaSource) -> Result<Self::Session, MediaError>;
}

pub trait MediaSession {
    fn list_tracks(&self) -> Result<Vec<AudioTrack>, MediaError>;
    fn extract_mono(
        &mut self,
        track: &AudioTrack,
        window: &ClipWindow,
        progress: &dyn ProgressReporter,
        label: &str,
    ) -> Result<MonoPcmClip, MediaError>;

    /// Native-rate, all-channels extract for the repair fill path.
    ///
    /// Unlike [`extract_mono`](Self::extract_mono) (which downmixes to mono at the fingerprint
    /// rate), this preserves the source channel layout and sample rate. The default returns
    /// [`MediaError::Unsupported`] so fakes opt in only when a test exercises the fill path.
    fn extract_interleaved(
        &mut self,
        track: &AudioTrack,
        window: &ClipWindow,
        progress: &dyn ProgressReporter,
        label: &str,
    ) -> Result<MultiChannelPcm, MediaError> {
        let _ = (track, window, progress, label);
        Err(MediaError::Unsupported(
            "extract_interleaved not implemented for this media session".into(),
        ))
    }

    /// Last decodable packet end for a track (tail scan). Default: unknown.
    fn track_decodable_extent(
        &mut self,
        track: &AudioTrack,
    ) -> Result<Option<Duration>, MediaError> {
        let _ = track;
        Ok(None)
    }

    /// Drop cached decode state and reopen the container reader on the next extract.
    ///
    /// Symphonia sessions use this after long seeks or tail scans so the next window decode
    /// does not inherit a corrupted reader position (common on MKV after backward seeks).
    fn reset_decode_io(&mut self) -> Result<(), MediaError> {
        Ok(())
    }

    /// Scan a track by decoding sequentially from the start and invoking `on_bucket` for each
    /// fixed-duration sample bucket (avoids per-window seek on long files).
    ///
    /// Default implementation falls back to seek-based [`extract_mono`](Self::extract_mono) windows.
    fn scan_mono_buckets(
        &mut self,
        track: &AudioTrack,
        bucket_secs: f64,
        progress: &dyn ProgressReporter,
        label: &str,
        on_bucket: &mut dyn FnMut(MonoScanBucket) -> Result<(), MediaError>,
    ) -> Result<(), MediaError> {
        crate::application::media_scan::scan_mono_buckets_via_windows(
            self,
            track,
            bucket_secs,
            progress,
            label,
            on_bucket,
        )
    }

    /// Scan a track sequentially and invoke `on_bucket` with native interleaved PCM buckets.
    ///
    /// Default implementation falls back to seek-based [`extract_interleaved`](Self::extract_interleaved)
    /// windows (slower on long files; symphonia sessions override with a sequential decoder).
    ///
    /// When supported, the maximum observed |PTS − sample-clock| skew is stored in `timeline_skew`.
    fn scan_interleaved_buckets(
        &mut self,
        track: &AudioTrack,
        bucket_secs: f64,
        progress: &dyn ProgressReporter,
        label: &str,
        on_bucket: &mut dyn FnMut(InterleavedScanBucket) -> Result<(), MediaError>,
        timeline_skew: &mut Option<crate::domain::AudioTimelineSkew>,
    ) -> Result<(), MediaError> {
        let _ = timeline_skew;
        crate::application::media_scan::scan_interleaved_buckets_via_windows(
            self,
            track,
            bucket_secs,
            progress,
            label,
            on_bucket,
        )
    }
}

pub trait Fingerprinter {
    fn fingerprint(&self, clip: &MonoPcmClip) -> Result<Fingerprint, FingerprintError>;
}

/// Sample-rate conversion engine for the alignment pipeline. The production adapter (rubato FFT
/// with linear fallback) lives in `infrastructure/resample/`.
///
/// Infallible by design: the current pipeline never propagates resample errors — engine
/// failure degrades to an internal fallback, not an error path.
///
/// Multichannel B→A resampling in `clip-sync-repair` uses the crate facade
/// [`resample_interleaved`](crate::infrastructure::resample::resample_interleaved) (not this port).
pub trait Resampler {
    /// Resample mono PCM to `target_rate`. Returns the input unchanged when rates match.
    fn resample_mono(&self, clip: &MonoPcmClip, target_rate: u32) -> MonoPcmClip;
}

/// GCC-PHAT cross-correlation engine. The production adapter lives in
/// `infrastructure/correlation.rs`.
pub trait PcmCorrelator {
    /// Full GCC-PHAT cross-correlation of `a` against `b`; returns the peak's lag in samples
    /// (fractional, sub-sample when the peak is interior) relative to the centered zero-lag
    /// position, and the peak magnitude. `None` when the engine cannot correlate these inputs.
    fn cross_correlate_lag(&self, a: &[f64], b: &[f64]) -> Option<(f64, f64)>;

    /// GCC-PHAT similarity for equal-length segments aligned at lag zero.
    fn segment_similarity(&self, a: &[f64], b: &[f64]) -> f64;
}

pub trait Aligner {
    /// Compare clip fingerprints from video A (`left`) and video B (`right`).
    /// Returns seconds to add to A's timeline to align with B (see PLAN.md).
    fn find_offset(
        &self,
        left: &Fingerprint,
        right: &Fingerprint,
    ) -> Result<ClipMatchEstimate, AlignmentError>;
}

/// Detects internal repetition within a single prepared clip fingerprint.
pub trait ClipRepetitionDetector {
    fn detect_clip_repetition(
        &self,
        fingerprint: &Fingerprint,
        prepared_duration_secs: f64,
        preset: ChromaprintPreset,
        min_confidence: f32,
        source_duration_secs: f64,
    ) -> Option<RepetitionFinding>;
}
