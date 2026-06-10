use crate::application::error::{AlignmentError, FingerprintError, MediaError};
use std::time::Duration;

use crate::domain::{
    AudioTrack, ClipLabel, ClipMatchEstimate, ClipWindow, Fingerprint, InterleavedScanBucket,
    MediaSource, MonoPcmClip, MonoScanBucket, MultiChannelPcm,
};

pub trait ProgressReporter {
    /// Major stage line — shown in default (`Auto`) and `--verbose` modes.
    fn phase(&self, message: &str);

    /// Detail line — shown only with `--verbose` (`ProgressMode::Verbose`).
    fn phase_verbose(&self, message: &str) {
        let _ = message;
    }

    fn progress(&self, label: &str, current: u64, total: u64);
}

pub trait MediaReader {
    type Session: MediaSession;

    fn open(&self, source: &MediaSource) -> Result<Self::Session, MediaError>;
}

pub trait MediaSession {
    fn list_tracks(&self) -> Result<Vec<AudioTrack>, MediaError>;
    fn extract_mono(
        &self,
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
        &self,
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

    /// Rewind the underlying format reader and drop cached decoders before a distant seek.
    fn reset_io(&self) -> Result<(), MediaError> {
        Ok(())
    }

    /// Last decodable packet end for a track (tail scan). Default: unknown.
    fn track_decodable_extent(&self, track: &AudioTrack) -> Result<Option<Duration>, MediaError> {
        let _ = track;
        Ok(None)
    }

    /// Scan a track by decoding sequentially from the start and invoking `on_bucket` for each
    /// fixed-duration sample bucket (avoids per-window seek on long files).
    ///
    /// Default implementation falls back to seek-based [`extract_mono`](Self::extract_mono) windows.
    fn scan_mono_buckets(
        &self,
        track: &AudioTrack,
        bucket_secs: f64,
        progress: &dyn ProgressReporter,
        label: &str,
        on_bucket: &mut dyn FnMut(MonoScanBucket) -> Result<(), MediaError>,
    ) -> Result<(), MediaError> {
        const NEAR_TRACK_END_TOLERANCE_SECS: f64 = 2.0;

        let duration = track
            .duration
            .filter(|value| !value.is_zero())
            .ok_or(MediaError::DecodeFailed {
                track: track.index,
                detail: "missing track duration for sequential scan".into(),
            })?;
        let total_secs = duration.as_secs_f64();
        let mut pos = 0.0f64;

        while pos < total_secs {
            let end = (pos + bucket_secs).min(total_secs);
            let window = ClipWindow::new(
                Duration::from_secs_f64(pos),
                Duration::from_secs_f64(end),
                ClipLabel::Interior,
            );

            match self.extract_mono(track, &window, progress, label) {
                Ok(pcm) => on_bucket(MonoScanBucket {
                    start_secs: pos,
                    end_secs: end,
                    pcm,
                })?,
                Err(MediaError::DecodeFailed { .. }) | Err(MediaError::SeekFailed(_)) => {
                    if pos >= total_secs - NEAR_TRACK_END_TOLERANCE_SECS {
                        break;
                    }
                }
                Err(error) => return Err(error),
            }
            pos = end;
        }

        Ok(())
    }

    /// Scan a track sequentially and invoke `on_bucket` with native interleaved PCM buckets.
    ///
    /// Default implementation falls back to seek-based [`extract_interleaved`](Self::extract_interleaved)
    /// windows (slower on long files; symphonia sessions override with a sequential decoder).
    fn scan_interleaved_buckets(
        &self,
        track: &AudioTrack,
        bucket_secs: f64,
        progress: &dyn ProgressReporter,
        label: &str,
        on_bucket: &mut dyn FnMut(InterleavedScanBucket) -> Result<(), MediaError>,
    ) -> Result<(), MediaError> {
        const NEAR_TRACK_END_TOLERANCE_SECS: f64 = 2.0;

        let duration = track
            .duration
            .filter(|value| !value.is_zero())
            .ok_or(MediaError::DecodeFailed {
                track: track.index,
                detail: "missing track duration for sequential scan".into(),
            })?;
        let total_secs = duration.as_secs_f64();
        let mut pos = 0.0f64;

        while pos < total_secs {
            let end = (pos + bucket_secs).min(total_secs);
            let window = ClipWindow::new(
                Duration::from_secs_f64(pos),
                Duration::from_secs_f64(end),
                ClipLabel::Interior,
            );

            match self.extract_interleaved(track, &window, progress, label) {
                Ok(pcm) => on_bucket(InterleavedScanBucket {
                    start_secs: pos,
                    end_secs: end,
                    pcm,
                })?,
                Err(MediaError::DecodeFailed { .. }) | Err(MediaError::SeekFailed(_)) => {
                    if pos >= total_secs - NEAR_TRACK_END_TOLERANCE_SECS {
                        break;
                    }
                }
                Err(error) => return Err(error),
            }
            pos = end;
        }

        Ok(())
    }
}

pub trait Fingerprinter {
    fn fingerprint(&self, clip: &MonoPcmClip) -> Result<Fingerprint, FingerprintError>;
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
