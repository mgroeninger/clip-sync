use std::path::{Path, PathBuf};
use std::time::Duration;

use clip_sync::{
    align_with_defaults, select_best_track, AlignConfig, AlignVideosRequest, AlignmentResult,
    AudioTrack, ClipLabel, ClipWindow, DomainError, MediaReader, MediaSession, MediaSource,
    ProgressReporter,
};

use crate::application::error::RepairError;
use crate::domain::gap::{Gap, GapReport};
use crate::domain::policies;
use crate::domain::track_match::assess_track_compatibility;

pub struct ScanGapsRequest {
    pub video_a: PathBuf,
    pub video_b: PathBuf,
    pub align: AlignConfig,
    /// Duration of each scan window when checking A's timeline for silence.
    pub scan_window_secs: u64,
    /// Fraction of peak amplitude below which a block is considered silent.
    pub silence_peak_fraction: f32,
    /// Minimum silent-window duration (seconds) to include in the gap report.
    pub min_gap_secs: f64,
}

pub struct ScanGaps<'r, MR: MediaReader> {
    media_reader: &'r MR,
    progress: &'r dyn ProgressReporter,
}

impl<'r, MR: MediaReader> ScanGaps<'r, MR> {
    pub fn new(media_reader: &'r MR, progress: &'r dyn ProgressReporter) -> Self {
        Self {
            media_reader,
            progress,
        }
    }

    pub fn execute(&self, request: ScanGapsRequest) -> Result<GapReport, RepairError> {
        let alignment = align_with_defaults(
            AlignVideosRequest {
                video_a: request.video_a.clone(),
                video_b: request.video_b.clone(),
                config: request.align.clone(),
            },
            self.progress,
        )?;

        self.scan_after_alignment(request, alignment)
    }

    /// Gap scan after alignment — unit-testable without the align sub-flow.
    pub(crate) fn scan_after_alignment(
        &self,
        request: ScanGapsRequest,
        alignment: AlignmentResult,
    ) -> Result<GapReport, RepairError> {
        // Step 2: open video A media session and select its best audio track.
        let source_a = MediaSource::new(request.video_a.clone());
        let session_a = self.media_reader.open(&source_a).map_err(RepairError::Media)?;
        let tracks_a = session_a.list_tracks().map_err(RepairError::Media)?;
        let track_a = select_best_track(&tracks_a)?.clone();

        let duration_a = track_a
            .duration
            .ok_or(RepairError::Domain(DomainError::InvalidDuration))?;

        // Step 3: best-effort open of video B for track compatibility + energy probing.
        // A missing or undecodable B never fails the scan — A's gaps are still reported, just
        // marked unfillable with no compatibility. Energy probing additionally requires an offset.
        let offset_secs = alignment.recommended_offset_secs;
        let b_session = self.open_best_track(&request.video_b);
        let track_compatibility = b_session
            .as_ref()
            .map(|(_, track_b)| assess_track_compatibility(&track_a, track_b));

        // Step 4: scan A in fixed-size chunks; detect silent windows.
        let chunk_secs = request.scan_window_secs as f64;
        let total_secs = duration_a.as_secs_f64();
        let mut gaps = Vec::new();
        let mut pos = 0.0f64;

        while pos < total_secs {
            let end = (pos + chunk_secs).min(total_secs);
            let window_a = ClipWindow::new(
                Duration::from_secs_f64(pos),
                Duration::from_secs_f64(end),
                ClipLabel::Interior,
            );

            let pcm_a = session_a
                .extract_mono(&track_a, &window_a, self.progress, "scan-a")
                .map_err(RepairError::Media)?;

            let window_secs = end - pos;
            if policies::is_silent(&pcm_a, request.silence_peak_fraction)
                && window_secs >= request.min_gap_secs
            {
                // B positions are only meaningful when alignment produced an offset.
                let b_positions = offset_secs.map(|delta| (pos + delta, end + delta));

                let b_has_energy = match (&b_session, b_positions) {
                    (Some((session_b, track_b)), Some((b_start, b_end))) if b_start >= 0.0 => {
                        let window_b = ClipWindow::new(
                            Duration::from_secs_f64(b_start),
                            Duration::from_secs_f64(b_end),
                            ClipLabel::Interior,
                        );
                        match session_b.extract_mono(track_b, &window_b, self.progress, "scan-b") {
                            Ok(pcm_b) => {
                                !policies::is_silent(&pcm_b, request.silence_peak_fraction)
                            }
                            Err(_) => false,
                        }
                    }
                    _ => false,
                };

                gaps.push(Gap {
                    video_a_start_secs: pos,
                    video_a_end_secs: end,
                    video_b_start_secs: b_positions.map(|(s, _)| s),
                    video_b_end_secs: b_positions.map(|(_, e)| e),
                    b_has_energy,
                });
            }

            pos = end;
        }

        Ok(GapReport {
            video_a: request.video_a,
            video_b: request.video_b,
            track_compatibility,
            overlap: alignment.start_overlap,
            alignment,
            gaps,
            scan_window_secs: request.scan_window_secs,
            silence_peak_fraction: request.silence_peak_fraction,
        })
    }

    /// Open `path` and select its best decodable track. Returns `None` (never an error) when the
    /// file is missing, unreadable, or has no decodable audio — keeps the scan report-only safe.
    fn open_best_track(&self, path: &Path) -> Option<(MR::Session, AudioTrack)> {
        let source = MediaSource::new(path.to_path_buf());
        let session = self.media_reader.open(&source).ok()?;
        let tracks = session.list_tracks().ok()?;
        let track = select_best_track(&tracks).ok()?.clone();
        Some((session, track))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::time::Duration;

    use clip_sync::testing::fakes::FakeProgressReporter;
    use clip_sync::{
        AlignmentResult, AudioTrack, ClipLabel, ClipMatch, ClipWindow, MediaError, MediaSession,
        MediaSource, MonoPcmClip,
    };

    use super::*;

    // --- minimal fakes ---

    struct LoudSession(Duration);
    struct SilentSession(Duration);

    impl MediaSession for LoudSession {
        fn list_tracks(&self) -> Result<Vec<AudioTrack>, MediaError> {
            Ok(vec![loud_track(self.0)])
        }

        fn extract_mono(
            &self,
            _track: &AudioTrack,
            window: &ClipWindow,
            _progress: &dyn clip_sync::ProgressReporter,
            _label: &str,
        ) -> Result<MonoPcmClip, MediaError> {
            let rate = 11_025u32;
            let secs = (window.end - window.start).as_secs_f64();
            let count = (rate as f64 * secs) as usize;
            let samples: Vec<i16> = (0..count)
                .map(|i| (f32::sin(i as f32 * 0.3) * 8_000.0) as i16)
                .collect();
            Ok(MonoPcmClip {
                sample_rate: rate,
                samples,
                decode_error_skips: 0,
                decoded_sample_count: None,
            })
        }
    }

    impl MediaSession for SilentSession {
        fn list_tracks(&self) -> Result<Vec<AudioTrack>, MediaError> {
            Ok(vec![loud_track(self.0)])
        }

        fn extract_mono(
            &self,
            _track: &AudioTrack,
            window: &ClipWindow,
            _progress: &dyn clip_sync::ProgressReporter,
            _label: &str,
        ) -> Result<MonoPcmClip, MediaError> {
            let rate = 11_025u32;
            let secs = (window.end - window.start).as_secs_f64();
            Ok(MonoPcmClip {
                sample_rate: rate,
                samples: vec![0i16; (rate as f64 * secs) as usize],
                decode_error_skips: 0,
                decoded_sample_count: None,
            })
        }
    }

    fn loud_track(duration: Duration) -> AudioTrack {
        AudioTrack {
            index: 0,
            codec: "pcm".into(),
            channels: 1,
            sample_rate: 11_025,
            bitrate: None,
            duration: Some(duration),
            decodable: true,
        }
    }

    enum SessionKind {
        Loud,
        Silent,
    }

    struct FixedReader(HashMap<PathBuf, (SessionKind, Duration)>);

    impl FixedReader {
        fn new() -> Self {
            Self(HashMap::new())
        }

        fn with(mut self, path: &str, kind: SessionKind, dur: Duration) -> Self {
            self.0.insert(PathBuf::from(path), (kind, dur));
            self
        }
    }

    // FakeMediaSession from clip-sync test-utils doesn't let us control silence per-window,
    // so we implement a local reader that dispatches to LoudSession / SilentSession.

    impl clip_sync::MediaReader for FixedReader {
        type Session = DispatchSession;

        fn open(&self, source: &MediaSource) -> Result<DispatchSession, MediaError> {
            let (kind, dur) = self
                .0
                .get(source.path())
                .ok_or_else(|| MediaError::FileNotFound(source.path().to_path_buf()))?;
            Ok(match kind {
                SessionKind::Loud => DispatchSession::Loud(LoudSession(*dur)),
                SessionKind::Silent => DispatchSession::Silent(SilentSession(*dur)),
            })
        }
    }

    enum DispatchSession {
        Loud(LoudSession),
        Silent(SilentSession),
    }

    impl MediaSession for DispatchSession {
        fn list_tracks(&self) -> Result<Vec<AudioTrack>, MediaError> {
            match self {
                Self::Loud(s) => s.list_tracks(),
                Self::Silent(s) => s.list_tracks(),
            }
        }

        fn extract_mono(
            &self,
            track: &AudioTrack,
            window: &ClipWindow,
            progress: &dyn clip_sync::ProgressReporter,
            label: &str,
        ) -> Result<MonoPcmClip, MediaError> {
            match self {
                Self::Loud(s) => s.extract_mono(track, window, progress, label),
                Self::Silent(s) => s.extract_mono(track, window, progress, label),
            }
        }
    }

    // --- helpers ---

    fn aligned_result(offset: Option<f64>) -> AlignmentResult {
        AlignmentResult {
            clips: vec![ClipMatch {
                label: ClipLabel::Start,
                window_start_secs: 0.0,
                window_end_secs: 60.0,
                aligned: offset.is_some(),
                offset_secs: offset,
                confidence: if offset.is_some() { 0.9 } else { 0.0 },
                video_a_decode_skips: 0,
                video_b_decode_skips: 0,
            }],
            start_aligned: offset.is_some(),
            end_aligned: None,
            recommended_offset_secs: offset,
            offsets_consistent: true,
            start_overlap: None,
            high_rate_refinement: None,
        }
    }

    fn scan_request(a: &str, b: &str, scan_window_secs: u64) -> ScanGapsRequest {
        ScanGapsRequest {
            video_a: PathBuf::from(a),
            video_b: PathBuf::from(b),
            align: AlignConfig::default(),
            scan_window_secs,
            silence_peak_fraction: 0.01,
            min_gap_secs: 30.0,
        }
    }

    struct NoDurationSession;

    impl MediaSession for NoDurationSession {
        fn list_tracks(&self) -> Result<Vec<AudioTrack>, MediaError> {
            Ok(vec![AudioTrack {
                index: 0,
                codec: "pcm".into(),
                channels: 1,
                sample_rate: 11_025,
                bitrate: None,
                duration: None,
                decodable: true,
            }])
        }

        fn extract_mono(
            &self,
            _track: &AudioTrack,
            _window: &ClipWindow,
            _progress: &dyn clip_sync::ProgressReporter,
            _label: &str,
        ) -> Result<MonoPcmClip, MediaError> {
            Err(MediaError::DecodeFailed {
                track: 0,
                detail: "not reached".into(),
            })
        }
    }

    struct NoDurationReader;

    impl clip_sync::MediaReader for NoDurationReader {
        type Session = NoDurationSession;

        fn open(&self, _source: &MediaSource) -> Result<NoDurationSession, MediaError> {
            Ok(NoDurationSession)
        }
    }

    #[test]
    fn loud_pcm_is_not_classified_as_silent() {
        // ScanGaps::execute calls align_with_defaults which requires real media;
        // this test verifies the policy layer independently using a fake reader.
        let loud_samples: Vec<i16> = (0..11_025)
            .map(|i| (f32::sin(i as f32 * 0.3) * 8_000.0) as i16)
            .collect();
        let loud_clip = MonoPcmClip {
            sample_rate: 11_025,
            samples: loud_samples,
            decode_error_skips: 0,
            decoded_sample_count: None,
        };
        assert!(!policies::is_silent(&loud_clip, 0.01));

        // Confirm the fake reader can open both sessions.
        let dur = Duration::from_secs(120);
        let reader = FixedReader::new()
            .with("a.wav", SessionKind::Loud, dur)
            .with("b.wav", SessionKind::Loud, dur);
        let source_a = MediaSource::new(PathBuf::from("a.wav"));
        let source_b = MediaSource::new(PathBuf::from("b.wav"));
        assert!(reader.open(&source_a).is_ok());
        assert!(reader.open(&source_b).is_ok());
    }

    #[test]
    fn scan_detects_gap_in_silent_a_with_loud_b() {
        let dur = Duration::from_secs(60);
        let silent_reader = FixedReader::new()
            .with("a.wav", SessionKind::Silent, dur)
            .with("b.wav", SessionKind::Loud, dur);
        let progress = FakeProgressReporter;

        // Open A and extract a window; confirm it's silent.
        let source_a = MediaSource::new(PathBuf::from("a.wav"));
        let session_a = silent_reader.open(&source_a).unwrap();
        let tracks = session_a.list_tracks().unwrap();
        let window = ClipWindow::new(Duration::ZERO, Duration::from_secs(60), ClipLabel::Interior);
        let pcm_a = session_a.extract_mono(&tracks[0], &window, &progress, "test").unwrap();
        assert!(policies::is_silent(&pcm_a, 0.01));

        // Open B and extract the same window; confirm it has energy.
        let source_b = MediaSource::new(PathBuf::from("b.wav"));
        let session_b = silent_reader.open(&source_b).unwrap();
        let tracks_b = session_b.list_tracks().unwrap();
        let pcm_b = session_b.extract_mono(&tracks_b[0], &window, &progress, "test").unwrap();
        assert!(!policies::is_silent(&pcm_b, 0.01));
    }

    #[test]
    fn scan_after_alignment_detects_silent_gap_with_fillable_b() {
        let dur = Duration::from_secs(60);
        let reader = FixedReader::new()
            .with("a.wav", SessionKind::Silent, dur)
            .with("b.wav", SessionKind::Loud, dur);
        let progress = FakeProgressReporter;
        let scan = ScanGaps::new(&reader, &progress);

        let report = scan
            .scan_after_alignment(scan_request("a.wav", "b.wav", 60), aligned_result(Some(0.0)))
            .expect("scan should succeed");

        assert_eq!(report.gaps.len(), 1);
        assert!(report.gaps[0].b_has_energy);
        // B opened successfully, so track compatibility is reported (1ch @ 11025 on both).
        let compat = report
            .track_compatibility
            .as_ref()
            .expect("compatibility should be present when B opens");
        assert_eq!(compat.verdict, crate::domain::CompatibilityVerdict::Identical);
        assert!((report.gaps[0].video_a_start_secs - 0.0).abs() < 0.001);
        assert!((report.gaps[0].video_a_end_secs - 60.0).abs() < 0.001);
        assert!((report.gaps[0].video_b_start_secs.unwrap() - 0.0).abs() < 0.001);
        assert!((report.gaps[0].video_b_end_secs.unwrap() - 60.0).abs() < 0.001);
        assert!(report.gaps[0].is_fillable());
    }

    #[test]
    fn scan_after_alignment_loud_a_finds_no_gaps() {
        let dur = Duration::from_secs(120);
        let reader = FixedReader::new()
            .with("a.wav", SessionKind::Loud, dur)
            .with("b.wav", SessionKind::Loud, dur);
        let progress = FakeProgressReporter;
        let scan = ScanGaps::new(&reader, &progress);

        let report = scan
            .scan_after_alignment(scan_request("a.wav", "b.wav", 60), aligned_result(Some(0.0)))
            .expect("scan should succeed");

        assert!(report.gaps.is_empty());
    }

    #[test]
    fn scan_after_alignment_with_failed_alignment_marks_b_unfillable() {
        let dur = Duration::from_secs(60);
        let reader = FixedReader::new().with("a.wav", SessionKind::Silent, dur);
        let progress = FakeProgressReporter;
        let scan = ScanGaps::new(&reader, &progress);

        let report = scan
            .scan_after_alignment(scan_request("a.wav", "b.wav", 60), aligned_result(None))
            .expect("scan should succeed");

        assert_eq!(report.gaps.len(), 1);
        assert!(!report.gaps[0].b_has_energy);
        assert!(report.gaps[0].video_b_start_secs.is_none());
        assert!(report.gaps[0].video_b_end_secs.is_none());
        assert!(!report.gaps[0].is_fillable());
        // B (b.wav) is absent from the reader, so no compatibility could be assessed.
        assert!(report.track_compatibility.is_none());
        assert!(report.overlap.is_none());
    }

    #[test]
    fn scan_after_alignment_applies_offset_to_b_timeline() {
        let dur = Duration::from_secs(60);
        let reader = FixedReader::new()
            .with("a.wav", SessionKind::Silent, dur)
            .with("b.wav", SessionKind::Loud, dur);
        let progress = FakeProgressReporter;
        let scan = ScanGaps::new(&reader, &progress);

        let report = scan
            .scan_after_alignment(scan_request("a.wav", "b.wav", 60), aligned_result(Some(3.0)))
            .expect("scan should succeed");

        assert_eq!(report.gaps.len(), 1);
        assert!((report.gaps[0].video_b_start_secs.unwrap() - 3.0).abs() < 0.001);
        assert!((report.gaps[0].video_b_end_secs.unwrap() - 63.0).abs() < 0.001);
    }

    #[test]
    fn scan_after_alignment_unknown_duration_returns_invalid_duration() {
        use crate::infrastructure::cli::exit_code::exit_code_for;
        use std::process::ExitCode;

        let reader = NoDurationReader;
        let progress = FakeProgressReporter;
        let scan = ScanGaps::new(&reader, &progress);

        let err = scan
            .scan_after_alignment(scan_request("a.wav", "b.wav", 60), aligned_result(Some(0.0)))
            .expect_err("missing duration should fail");

        assert!(matches!(err, RepairError::Domain(DomainError::InvalidDuration)));
        assert_eq!(exit_code_for(&err), ExitCode::from(3));
    }

    #[test]
    fn gap_report_fillable_count_counts_b_energy_gaps() {
        use crate::domain::gap::{Gap, GapReport};

        let report = GapReport {
            video_a: PathBuf::from("a.wav"),
            video_b: PathBuf::from("b.wav"),
            track_compatibility: None,
            overlap: None,
            alignment: aligned_result(Some(0.0)),
            gaps: vec![
                Gap {
                    video_a_start_secs: 0.0,
                    video_a_end_secs: 60.0,
                    video_b_start_secs: Some(0.0),
                    video_b_end_secs: Some(60.0),
                    b_has_energy: true,
                },
                Gap {
                    video_a_start_secs: 120.0,
                    video_a_end_secs: 180.0,
                    video_b_start_secs: Some(120.0),
                    video_b_end_secs: Some(180.0),
                    b_has_energy: false,
                },
            ],
            scan_window_secs: 60,
            silence_peak_fraction: 0.01,
        };

        assert_eq!(report.gaps.len(), 2);
        assert_eq!(report.fillable_count(), 1);
        assert!(report.gaps[0].is_fillable());
        assert!(!report.gaps[1].is_fillable());
        assert!((report.gaps[0].duration_secs() - 60.0).abs() < 0.001);
    }
}
