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

    /// When true, clip extraction reports per-clip labels; when false, callers may aggregate
    /// decode progress under a single stage bar.
    fn detailed_extraction_progress(&self) -> bool {
        true
    }

    fn progress(&self, label: &str, current: u64, total: u64);
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
        const NEAR_TRACK_END_TOLERANCE_SECS: f64 = 2.0;

        let duration = track
            .duration
            .filter(|value| !value.is_zero())
            .ok_or(MediaError::decode_failed(
                track.index,
                "missing track duration for sequential scan",
            ))?;
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
                Err(MediaError::DecodeFailed { .. }) | Err(MediaError::SeekFailed { .. }) => {
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
        &mut self,
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
            .ok_or(MediaError::decode_failed(
                track.index,
                "missing track duration for sequential scan",
            ))?;
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
                Err(MediaError::DecodeFailed { .. }) | Err(MediaError::SeekFailed { .. }) => {
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

/// Sample-rate conversion engine. The production adapter (rubato FFT with linear fallback)
/// lives in `infrastructure/resample/`.
///
/// Infallible by design: the current pipeline never propagates resample errors — engine
/// failure degrades to an internal fallback, not an error path.
pub trait Resampler {
    /// Resample mono PCM to `target_rate`. Returns the input unchanged when rates match.
    fn resample_mono(&self, clip: &MonoPcmClip, target_rate: u32) -> MonoPcmClip;

    /// Resample interleaved PCM to `target_rate`, preserving channel layout.
    ///
    /// No production caller dispatches through this port method — the analyzer pipeline uses
    /// [`resample_mono`](Self::resample_mono) only. Multichannel B→A resampling in
    /// `clip-sync-repair` uses the crate facade
    /// [`resample_interleaved`](crate::infrastructure::resample::resample_interleaved)
    /// (`Vec<i16>` in/out, no port injection). Adapters should delegate to the same engine as
    /// that function; keep the two shapes in sync if either changes.
    fn resample_interleaved(&self, pcm: &MultiChannelPcm, target_rate: u32) -> MultiChannelPcm;
}

/// FFT cross-correlation engine. The production adapter (`cross_correlate` crate) lives in
/// `infrastructure/correlation.rs`.
pub trait PcmCorrelator {
    /// Full cross-correlation of `a` against `b`; returns the peak's lag in samples relative
    /// to the centered (zero-lag) position, and the peak magnitude. `None` when the engine
    /// cannot produce a correlation for these inputs.
    fn cross_correlate_lag(&self, a: &[f64], b: &[f64]) -> Option<(isize, f64)>;
}

#[cfg(test)]
mod scan_default_tests {
    use std::time::Duration;

    use super::*;
    use crate::application::error::MediaError;
    use crate::domain::{AudioTrack, ClipWindow, MonoPcmClip};

    /// Minimal `MediaSession` for testing the default `scan_mono_buckets` implementation.
    /// Fails `extract_mono` when the window's start falls within `[fail_start, fail_end)`.
    /// Outside that range every extract succeeds with a dummy clip.
    struct ScanTestSession {
        track_duration_secs: f64,
        fail_window: Option<(f64, f64)>,
        fail_kind: ScanFailKind,
    }

    enum ScanFailKind {
        DecodeFailed,
        Unsupported,
    }

    impl ScanTestSession {
        fn new(
            track_duration_secs: f64,
            fail_window: Option<(f64, f64)>,
            fail_kind: ScanFailKind,
        ) -> Self {
            Self { track_duration_secs, fail_window, fail_kind }
        }
    }

    impl MediaSession for ScanTestSession {
        fn list_tracks(&self) -> Result<Vec<AudioTrack>, MediaError> {
            Ok(vec![AudioTrack {
                index: 0,
                codec: "pcm".into(),
                channels: 1,
                sample_rate: 11_025,
                duration: Some(Duration::from_secs_f64(self.track_duration_secs)),
                decodable: true,
            }])
        }

        fn extract_mono(
            &mut self,
            _track: &AudioTrack,
            window: &ClipWindow,
            _progress: &dyn ProgressReporter,
            _label: &str,
        ) -> Result<MonoPcmClip, MediaError> {
            let pos = window.start.as_secs_f64();
            let should_fail = self
                .fail_window
                .is_some_and(|(start, end)| pos >= start && pos < end);
            if should_fail {
                return match self.fail_kind {
                    ScanFailKind::DecodeFailed => {
                        Err(MediaError::decode_failed(0, "injected decode failed"))
                    }
                    ScanFailKind::Unsupported => Err(MediaError::Unsupported("injected".into())),
                };
            }
            Ok(MonoPcmClip {
                sample_rate: 11_025,
                samples: vec![0_i16; 11_025],
                decode_error_skips: 0,
                decoded_sample_count: None,
            })
        }
    }

    fn scan_bucket_starts(session: &mut ScanTestSession) -> Result<Vec<f64>, MediaError> {
        let tracks = session.list_tracks().unwrap();
        let track = &tracks[0];
        let mut starts = Vec::new();
        session.scan_mono_buckets(
            track,
            1.0,
            &crate::application::testing::fakes::FakeProgressReporter,
            "test",
            &mut |bucket| {
                starts.push(bucket.start_secs);
                Ok(())
            },
        )?;
        Ok(starts)
    }

    // Phase 0 snapshot: DecodeFailed within NEAR_TRACK_END_TOLERANCE_SECS (2 s) of end
    // terminates the scan early (break) and returns Ok.
    #[test]
    fn scan_mono_default_swallows_decode_failed_near_end() {
        // 10 s track, fail on [8, 10) — the first failure at pos=8.0 hits the tolerance
        // boundary (8.0 >= 10.0 - 2.0) and breaks the loop immediately.
        let mut session =
            ScanTestSession::new(10.0, Some((8.0, 10.0)), ScanFailKind::DecodeFailed);
        let starts = scan_bucket_starts(&mut session).expect("scan should return Ok");
        assert_eq!(starts, vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0]);
    }

    // Phase 0 snapshot: DecodeFailed more than 2 s before end is silently skipped; the
    // scan continues and the failed bucket is absent from the output.
    #[test]
    fn scan_mono_default_skips_decode_failed_mid_file() {
        // 10 s track, fail only on [5.0, 6.0) — one bucket skipped, rest succeed.
        let mut session =
            ScanTestSession::new(10.0, Some((5.0, 6.0)), ScanFailKind::DecodeFailed);
        let starts = scan_bucket_starts(&mut session).expect("scan should return Ok");
        assert_eq!(
            starts,
            vec![0.0, 1.0, 2.0, 3.0, 4.0, 6.0, 7.0, 8.0, 9.0]
        );
    }

    // Phase 0 snapshot: non-DecodeFailed / non-SeekFailed errors propagate immediately.
    #[test]
    fn scan_mono_default_propagates_non_decode_error() {
        let mut session = ScanTestSession::new(10.0, Some((5.0, 6.0)), ScanFailKind::Unsupported);
        match scan_bucket_starts(&mut session) {
            Err(MediaError::Unsupported(_)) => {}
            Ok(starts) => panic!("expected Err, got Ok with {starts:?}"),
            Err(other) => panic!("expected Unsupported, got {other:?}"),
        }
    }
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
