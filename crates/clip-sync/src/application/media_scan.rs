//! Seek-loop fallback policy for [`MediaSession::scan_mono_buckets`](crate::application::ports::MediaSession::scan_mono_buckets)
//! and [`scan_interleaved_buckets`](crate::application::ports::MediaSession::scan_interleaved_buckets).
//!
//! **Production scans** (Symphonia sequential overrides used by repair) are **EOF-driven**:
//! bucket boundaries come from decoded sample counts and duration feeds progress estimation only.
//!
//! **These fallbacks** terminate on **declared** container duration (`while pos < total_secs`).
//! [`DecodeFailed`](crate::application::error::MediaError::DecodeFailed) /
//! [`SeekFailed`](crate::application::error::MediaError::SeekFailed) within
//! [`NEAR_TRACK_END_TOLERANCE_SECS`] of the end are swallowed (early `Ok` break); the same
//! errors earlier in the file are skipped (bucket omitted, scan continues). Other errors
//! propagate. Over-reporting duration beyond the tolerance fails loudly — acceptable for
//! fallback paths whose only callers are test fakes today.

use std::time::Duration;

use crate::application::error::MediaError;
use crate::application::ports::{MediaSession, ProgressReporter};
use crate::domain::{
    AudioTrack, ClipLabel, ClipWindow, InterleavedScanBucket, MonoScanBucket,
};

/// Within this many seconds of declared track end, seek/decode failures terminate the
/// fallback scan loop with `Ok(())` instead of propagating or skipping a bucket.
pub const NEAR_TRACK_END_TOLERANCE_SECS: f64 = 2.0;

/// Fallback mono scan: fixed windows via [`MediaSession::extract_mono`].
pub fn scan_mono_buckets_via_windows<MS: MediaSession + ?Sized>(
    session: &mut MS,
    track: &AudioTrack,
    bucket_secs: f64,
    progress: &dyn ProgressReporter,
    label: &str,
    on_bucket: &mut dyn FnMut(MonoScanBucket) -> Result<(), MediaError>,
) -> Result<(), MediaError> {
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

        match session.extract_mono(track, &window, progress, label) {
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

/// Fallback interleaved scan: fixed windows via [`MediaSession::extract_interleaved`].
pub fn scan_interleaved_buckets_via_windows<MS: MediaSession + ?Sized>(
    session: &mut MS,
    track: &AudioTrack,
    bucket_secs: f64,
    progress: &dyn ProgressReporter,
    label: &str,
    on_bucket: &mut dyn FnMut(InterleavedScanBucket) -> Result<(), MediaError>,
) -> Result<(), MediaError> {
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

        match session.extract_interleaved(track, &window, progress, label) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::testing::fakes::FakeProgressReporter;
    use crate::domain::{AudioTrack, ClipWindow, MonoPcmClip};

    /// Minimal `MediaSession` for testing fallback scan policy.
    /// Fails `extract_mono` when the window's start falls within `[fail_start, fail_end)`.
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
        let tracks = session.list_tracks()?;
        let track = &tracks[0];
        let mut starts = Vec::new();
        scan_mono_buckets_via_windows(
            session,
            track,
            1.0,
            &FakeProgressReporter,
            "test",
            &mut |bucket| {
                starts.push(bucket.start_secs);
                Ok(())
            },
        )?;
        Ok(starts)
    }

    #[test]
    fn scan_swallows_decode_failed_near_end() {
        let mut session =
            ScanTestSession::new(10.0, Some((8.0, 10.0)), ScanFailKind::DecodeFailed);
        let starts = scan_bucket_starts(&mut session).expect("scan should return Ok");
        assert_eq!(starts, vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0]);
    }

    #[test]
    fn scan_skips_decode_failed_mid_file() {
        let mut session =
            ScanTestSession::new(10.0, Some((5.0, 6.0)), ScanFailKind::DecodeFailed);
        let starts = scan_bucket_starts(&mut session).expect("scan should return Ok");
        assert_eq!(
            starts,
            vec![0.0, 1.0, 2.0, 3.0, 4.0, 6.0, 7.0, 8.0, 9.0]
        );
    }

    #[test]
    fn scan_propagates_non_decode_error() {
        let mut session = ScanTestSession::new(10.0, Some((5.0, 6.0)), ScanFailKind::Unsupported);
        match scan_bucket_starts(&mut session) {
            Err(MediaError::Unsupported(_)) => {}
            Ok(starts) => panic!("expected Err, got Ok with {starts:?}"),
            Err(other) => panic!("expected Unsupported, got {other:?}"),
        }
    }
}
