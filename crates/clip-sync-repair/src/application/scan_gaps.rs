use std::path::{Path, PathBuf};

use clip_sync::{
    select_best_track, select_track_for_reference, AlignConfig, AlignVideosRequest,
    AlignmentResult, AudioTrack, DomainError, InterleavedScanBucket,
    MediaError, MediaReader, MediaSession, MediaSource, ProgressReporter,
};

use crate::application::error::RepairError;
use crate::application::ports::Aligner;
use crate::domain::cross_check::{
    b_has_energy_in_range, check_gap_offset_agreement_in_overlap,
    mutual_silence_intervals_from_gaps, SilenceInterval,
};
use crate::domain::gap::{Gap, GapReport};
use crate::domain::policies;
use crate::domain::track_match::{assess_track_compatibility, TrackDescriptor};

pub struct ScanGapsRequest {
    pub video_a: PathBuf,
    pub video_b: PathBuf,
    pub align: AlignConfig,
    /// Decode chunk size (seconds) for sequential PCM scan.
    pub decode_chunk_secs: u64,
    /// Analysis block size (seconds) for silence-run detection within decoded PCM.
    pub scan_block_secs: f64,
    /// Fraction of peak amplitude below which a block is considered silent.
    pub silence_peak_fraction: f32,
    /// Absolute RMS floor (0–32767 scale) below which a block is always silent regardless of peak.
    pub absolute_silence_rms: f32,
    /// Consecutive non-silent blocks to absorb before closing a silence run.
    pub silence_hold_blocks: u32,
    /// Minimum silent-window duration (seconds) to include in the gap report.
    pub min_gap_secs: f64,
    /// When true, also scan B's native timeline for silence and compute `gap_offset_agreement`.
    pub scan_both: bool,
    /// Tolerance (seconds) for the silence-based vs alignment offset agreement check.
    pub gap_offset_tolerance_secs: f64,
    /// When query-reference alignment is used, only gaps inside the mapped clip coverage are fillable.
    pub limit_fill_to_mapped_region: bool,
}

/// One-line stderr summary after gap detection (thresholds + count).
pub(crate) fn format_scan_summary(request: &ScanGapsRequest, gap_count: usize) -> String {
    let min_gap_ms = (request.min_gap_secs * 1000.0).round() as u64;
    let block_ms = (request.scan_block_secs * 1000.0).round() as u64;
    let hold_ms = request.silence_hold_blocks as u64 * block_ms;
    let silence_pct = request.silence_peak_fraction * 100.0;
    let scan_both = if request.scan_both { "on" } else { "off" };
    let mut line = format!(
        "Gap scan: {gap_count} silent run(s) ≥{min_gap_ms}ms — block {block_ms}ms, silence {silence_pct:.1}% peak, hold {hold_ms}ms, decode {}s chunks, scan-both {scan_both}",
        request.decode_chunk_secs,
    );
    if request.absolute_silence_rms > 0.0 {
        line.push_str(&format!(", rms floor {:.0}", request.absolute_silence_rms));
    }
    line
}

pub struct ScanGaps<'r, MR: MediaReader> {
    media_reader: &'r MR,
    progress: &'r dyn ProgressReporter,
    aligner: &'r dyn Aligner,
}

impl<'r, MR: MediaReader> ScanGaps<'r, MR> {
    pub fn new(
        media_reader: &'r MR,
        progress: &'r dyn ProgressReporter,
        aligner: &'r dyn Aligner,
    ) -> Self {
        Self {
            media_reader,
            progress,
            aligner,
        }
    }

    pub fn execute(&self, request: ScanGapsRequest) -> Result<GapReport, RepairError> {
        let alignment = self.aligner.align(
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
        let mut session_a = self.media_reader.open(&source_a).map_err(RepairError::Media)?;
        let tracks_a = session_a.list_tracks().map_err(RepairError::Media)?;
        let track_a = select_best_track(&tracks_a)?.clone();

        if track_a.duration.is_none() {
            return Err(RepairError::Domain(DomainError::InvalidDuration));
        }

        // Step 3: best-effort open of video B for track compatibility + energy probing.
        // A missing or undecodable B never fails the scan — A's gaps are still reported, just
        // marked unfillable with no compatibility. Energy probing additionally requires an offset.
        let offset_secs = alignment.recommended_offset_secs;
        let mut b_session = self.open_best_track(&request.video_b, &track_a);
        let track_compatibility = b_session
            .as_ref()
            .map(|(_, track_b)| assess_track_compatibility(
                TrackDescriptor { channels: track_a.channels, sample_rate: track_a.sample_rate },
                TrackDescriptor { channels: track_b.channels, sample_rate: track_b.sample_rate },
            ));

        // Step 4: sequential decode + block-level silence-run detection on A.
        let decode_chunk_secs = request.decode_chunk_secs as f64;
        let silence_peak_fraction = request.silence_peak_fraction;
        let min_gap_secs = request.min_gap_secs;
        let progress = self.progress;

        let absolute_silence_rms = request.absolute_silence_rms;
        let silence_hold_blocks = request.silence_hold_blocks;

        let mut scanner_a = policies::SilenceRunScanner::new(
            request.scan_block_secs,
            silence_peak_fraction,
            min_gap_secs,
            silence_hold_blocks,
            absolute_silence_rms,
        );
        let mut last_fed_end_secs: Option<f64> = None;

        let mut scan_a = |bucket: InterleavedScanBucket| -> Result<(), MediaError> {
            if last_fed_end_secs
                .is_some_and(|prev_end| bucket.start_secs > prev_end + f64::EPSILON)
            {
                scanner_a.note_pcm_discontinuity();
            }
            scanner_a.feed(&bucket.pcm, bucket.start_secs);
            last_fed_end_secs = Some(bucket.end_secs);
            Ok(())
        };

        progress.phase("Scanning video A for gaps...");
        session_a
            .scan_interleaved_buckets(&track_a, decode_chunk_secs, progress, "scan-a", &mut scan_a)
            .map_err(RepairError::Media)?;

        // Step 5: scan B's native timeline sequentially to build its silence map.
        // Used for both per-gap energy lookup (replaces per-gap seeks) and the cross-check.
        // Only meaningful when we have a B session and an alignment offset.
        let b_intervals: Vec<SilenceInterval> =
            match (&mut b_session, offset_secs) {
                (Some((session_b, track_b)), Some(_)) => self.scan_silence_intervals(
                    session_b,
                    track_b,
                    decode_chunk_secs,
                    policies::SilenceRunScanner::new(
                        request.scan_block_secs,
                        request.silence_peak_fraction,
                        request.min_gap_secs,
                        silence_hold_blocks,
                        absolute_silence_rms,
                    ),
                ),
                _ => vec![],
            };

        let mut gaps = Vec::new();
        for run in scanner_a.finish() {
            let pos = run.start_secs;
            let end = run.end_secs;
            let b_positions = offset_secs.map(|delta| (pos + delta, end + delta));

            let b_has_energy = match b_positions {
                Some((b_start, b_end)) if b_start >= 0.0 => {
                    b_has_energy_in_range(&b_intervals, b_start, b_end)
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

        // Step 6: mutual-silence cross-check — only meaningful when alignment produced an offset.
        // Use co-occurring quiet on both timelines; exclude A-only dropouts (b_has_energy).
        let a_intervals = mutual_silence_intervals_from_gaps(&gaps);
        let gap_offset_agreement = if request.scan_both {
            alignment.recommended_offset_secs.and_then(|offset| {
                check_gap_offset_agreement_in_overlap(
                    &a_intervals,
                    &b_intervals,
                    alignment.start_overlap.as_ref(),
                    offset,
                    request.gap_offset_tolerance_secs,
                )
            })
        } else {
            None
        };

        progress.phase(&format_scan_summary(&request, gaps.len()));

        let overlap = alignment.start_overlap.map(Into::into);
        let alignment = clip_sync::AlignmentReport::from(&alignment);
        Ok(GapReport {
            video_a: request.video_a,
            video_b: request.video_b,
            track_compatibility,
            overlap,
            alignment,
            gaps,
            gap_offset_agreement,
            decode_chunk_secs: request.decode_chunk_secs,
            scan_block_ms: (request.scan_block_secs * 1000.0).round() as u64,
            silence_peak_fraction: request.silence_peak_fraction,
            limit_fill_to_mapped_region: request.limit_fill_to_mapped_region,
        })
    }

    /// Sequential sample-bucket silence scan on a session's native timeline.
    ///
    /// The caller supplies a configured [`policies::SilenceRunScanner`]; this method only
    /// drives it over the session's decoded buckets.
    fn scan_silence_intervals(
        &self,
        session: &mut MR::Session,
        track: &AudioTrack,
        decode_chunk_secs: f64,
        mut scanner: policies::SilenceRunScanner,
    ) -> Vec<SilenceInterval> {
        let progress = self.progress;
        let mut last_fed_end_secs: Option<f64> = None;

        let mut on_bucket = |bucket: InterleavedScanBucket| -> Result<(), MediaError> {
            if last_fed_end_secs
                .is_some_and(|prev_end| bucket.start_secs > prev_end + f64::EPSILON)
            {
                scanner.note_pcm_discontinuity();
            }
            scanner.feed(&bucket.pcm, bucket.start_secs);
            last_fed_end_secs = Some(bucket.end_secs);
            Ok(())
        };

        if session
            .scan_interleaved_buckets(track, decode_chunk_secs, progress, "scan-b", &mut on_bucket)
            .is_err()
        {
            return scanner
                .finish()
                .into_iter()
                .map(|run| SilenceInterval {
                    start_secs: run.start_secs,
                    end_secs: run.end_secs,
                })
                .collect();
        }

        scanner
            .finish()
            .into_iter()
            .map(|run| SilenceInterval {
                start_secs: run.start_secs,
                end_secs: run.end_secs,
            })
            .collect()
    }

    /// Open `path` and select its best decodable track. Returns `None` (never an error) when the
    /// file is missing, unreadable, or has no decodable audio — keeps the scan report-only safe.
    fn open_best_track(&self, path: &Path, track_a: &AudioTrack) -> Option<(MR::Session, AudioTrack)> {
        let source = MediaSource::new(path.to_path_buf());
        let session = self.media_reader.open(&source).ok()?;
        let tracks = session.list_tracks().ok()?;
        let track = select_track_for_reference(track_a, &tracks).ok()?.clone();
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
        MediaSource, MonoPcmClip, MultiChannelPcm,
    };

    use super::*;

    fn mono_clip_to_multichannel(clip: MonoPcmClip, channels: u16) -> MultiChannelPcm {
        let channels = channels.max(1);
        if channels == 1 {
            return MultiChannelPcm {
                sample_rate: clip.sample_rate,
                channels: 1,
                samples: clip.samples,
                decode_error_skips: clip.decode_error_skips,
                decoded_frame_count: clip.decoded_sample_count,
                compressed_bytes: None,
            };
        }

        let mut samples =
            Vec::with_capacity(clip.samples.len().saturating_mul(channels as usize));
        for sample in clip.samples {
            for _ in 0..channels {
                samples.push(sample);
            }
        }
        MultiChannelPcm {
            sample_rate: clip.sample_rate,
            channels,
            samples,
            decode_error_skips: clip.decode_error_skips,
            decoded_frame_count: clip
                .decoded_sample_count
                .map(|frames| frames * channels as usize),
            compressed_bytes: None,
        }
    }

    // --- minimal fakes ---

    struct LoudSession(Duration);
    struct SilentSession(Duration);

    impl MediaSession for LoudSession {
        fn list_tracks(&self) -> Result<Vec<AudioTrack>, MediaError> {
            Ok(vec![loud_track(self.0)])
        }

        fn extract_mono(
            &mut self,
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

        fn extract_interleaved(
            &mut self,
            track: &AudioTrack,
            window: &ClipWindow,
            progress: &dyn clip_sync::ProgressReporter,
            label: &str,
        ) -> Result<MultiChannelPcm, MediaError> {
            let clip = self.extract_mono(track, window, progress, label)?;
            Ok(mono_clip_to_multichannel(clip, track.channels))
        }
    }

    impl MediaSession for SilentSession {
        fn list_tracks(&self) -> Result<Vec<AudioTrack>, MediaError> {
            Ok(vec![loud_track(self.0)])
        }

        fn extract_mono(
            &mut self,
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

        fn extract_interleaved(
            &mut self,
            track: &AudioTrack,
            window: &ClipWindow,
            progress: &dyn clip_sync::ProgressReporter,
            label: &str,
        ) -> Result<MultiChannelPcm, MediaError> {
            let clip = self.extract_mono(track, window, progress, label)?;
            Ok(mono_clip_to_multichannel(clip, track.channels))
        }
    }

    fn loud_track(duration: Duration) -> AudioTrack {
        AudioTrack {
            index: 0,
            codec: "pcm".into(),
            channels: 1,
            sample_rate: 11_025,
            duration: Some(duration),
            decodable: true,
        }
    }

    enum SessionKind {
        Loud,
        Silent,
        /// Loud until `window.start >= fail_from_secs`, then `SeekFailed`.
        TailSeekFail { fail_from_secs: f64 },
        /// Silent except `DecodeFailed` when `skip_start <= window.start < skip_end`.
        SkipWindow { skip_start: f64, skip_end: f64 },
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

    struct TailSeekFailSession {
        duration: Duration,
        fail_from_secs: f64,
    }

    struct SkipWindowSession {
        duration: Duration,
        skip_start: f64,
        skip_end: f64,
    }

    impl MediaSession for SkipWindowSession {
        fn list_tracks(&self) -> Result<Vec<AudioTrack>, MediaError> {
            Ok(vec![loud_track(self.duration)])
        }

        fn extract_mono(
            &mut self,
            track: &AudioTrack,
            window: &ClipWindow,
            progress: &dyn clip_sync::ProgressReporter,
            label: &str,
        ) -> Result<MonoPcmClip, MediaError> {
            let start = window.start.as_secs_f64();
            if start >= self.skip_start && start < self.skip_end {
                return Err(MediaError::decode_failed(track.index, "skipped scan window"));
            }
            SilentSession(self.duration).extract_mono(track, window, progress, label)
        }

        fn extract_interleaved(
            &mut self,
            track: &AudioTrack,
            window: &ClipWindow,
            progress: &dyn clip_sync::ProgressReporter,
            label: &str,
        ) -> Result<MultiChannelPcm, MediaError> {
            let clip = self.extract_mono(track, window, progress, label)?;
            Ok(mono_clip_to_multichannel(clip, track.channels))
        }
    }

    impl MediaSession for TailSeekFailSession {
        fn list_tracks(&self) -> Result<Vec<AudioTrack>, MediaError> {
            Ok(vec![loud_track(self.duration)])
        }

        fn extract_mono(
            &mut self,
            _track: &AudioTrack,
            window: &ClipWindow,
            _progress: &dyn clip_sync::ProgressReporter,
            _label: &str,
        ) -> Result<MonoPcmClip, MediaError> {
            if window.start.as_secs_f64() >= self.fail_from_secs {
                return Err(MediaError::seek_failed("tail seek"));
            }
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

        fn extract_interleaved(
            &mut self,
            track: &AudioTrack,
            window: &ClipWindow,
            progress: &dyn clip_sync::ProgressReporter,
            label: &str,
        ) -> Result<MultiChannelPcm, MediaError> {
            let clip = self.extract_mono(track, window, progress, label)?;
            Ok(mono_clip_to_multichannel(clip, track.channels))
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
                SessionKind::TailSeekFail { fail_from_secs } => {
                    DispatchSession::TailSeekFail(TailSeekFailSession {
                        duration: *dur,
                        fail_from_secs: *fail_from_secs,
                    })
                }
                SessionKind::SkipWindow {
                    skip_start,
                    skip_end,
                } => DispatchSession::SkipWindow(SkipWindowSession {
                    duration: *dur,
                    skip_start: *skip_start,
                    skip_end: *skip_end,
                }),
            })
        }
    }

    enum DispatchSession {
        Loud(LoudSession),
        Silent(SilentSession),
        TailSeekFail(TailSeekFailSession),
        SkipWindow(SkipWindowSession),
    }

    impl MediaSession for DispatchSession {
        fn list_tracks(&self) -> Result<Vec<AudioTrack>, MediaError> {
            match self {
                Self::Loud(s) => s.list_tracks(),
                Self::Silent(s) => s.list_tracks(),
                Self::TailSeekFail(s) => s.list_tracks(),
                Self::SkipWindow(s) => s.list_tracks(),
            }
        }

        fn extract_mono(
            &mut self,
            track: &AudioTrack,
            window: &ClipWindow,
            progress: &dyn clip_sync::ProgressReporter,
            label: &str,
        ) -> Result<MonoPcmClip, MediaError> {
            match self {
                Self::Loud(s) => s.extract_mono(track, window, progress, label),
                Self::Silent(s) => s.extract_mono(track, window, progress, label),
                Self::TailSeekFail(s) => s.extract_mono(track, window, progress, label),
                Self::SkipWindow(s) => s.extract_mono(track, window, progress, label),
            }
        }

        fn extract_interleaved(
            &mut self,
            track: &AudioTrack,
            window: &ClipWindow,
            progress: &dyn clip_sync::ProgressReporter,
            label: &str,
        ) -> Result<MultiChannelPcm, MediaError> {
            match self {
                Self::Loud(s) => s.extract_interleaved(track, window, progress, label),
                Self::Silent(s) => s.extract_interleaved(track, window, progress, label),
                Self::TailSeekFail(s) => s.extract_interleaved(track, window, progress, label),
                Self::SkipWindow(s) => s.extract_interleaved(track, window, progress, label),
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
                repetition: None,
                video_b_window_start_secs: None,
                video_b_window_end_secs: None,
            }],
            start_aligned: offset.is_some(),
            end_aligned: None,
            recommended_offset_secs: offset,
            offsets_consistent: true,
            offset_drift_secs: None,
            start_overlap: None,
            high_rate_refinement: None,
            offset_verification: None,
            offset_ambiguous_mod_secs: None,
            alignment_mode_used: None,
            query_localization: None,
            end_clip_anchor: None,
        }
    }

    fn scan_request(a: &str, b: &str, decode_chunk_secs: u64) -> ScanGapsRequest {
        ScanGapsRequest {
            video_a: PathBuf::from(a),
            video_b: PathBuf::from(b),
            align: AlignConfig::default(),
            decode_chunk_secs,
            scan_block_secs: 0.25,
            silence_peak_fraction: 0.01,
            absolute_silence_rms: 0.0,
            silence_hold_blocks: 0,
            min_gap_secs: 1.0,
            scan_both: false,
            gap_offset_tolerance_secs: 0.5,
            limit_fill_to_mapped_region: true,
        }
    }

    struct NeverCalledAligner;

    impl crate::application::ports::Aligner for NeverCalledAligner {
        fn align(
            &self,
            _: clip_sync::AlignVideosRequest,
            _: &dyn clip_sync::ProgressReporter,
        ) -> Result<clip_sync::AlignmentResult, clip_sync::AppError> {
            unreachable!("tests use scan_after_alignment directly")
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
                duration: None,
                decodable: true,
            }])
        }

        fn extract_mono(
            &mut self,
            _track: &AudioTrack,
            _window: &ClipWindow,
            _progress: &dyn clip_sync::ProgressReporter,
            _label: &str,
        ) -> Result<MonoPcmClip, MediaError> {
            Err(MediaError::decode_failed(0, "not reached"))
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
        let loud_samples: Vec<i16> = (0..11_025)
            .map(|i| (f32::sin(i as f32 * 0.3) * 8_000.0) as i16)
            .collect();
        let loud_clip = MonoPcmClip {
            sample_rate: 11_025,
            samples: loud_samples,
            decode_error_skips: 0,
            decoded_sample_count: None,
        };
        assert!(!policies::is_silent(&loud_clip.samples, 0.01, 0.0));

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
    fn scan_after_alignment_detects_silent_gap_with_fillable_b() {
        let dur = Duration::from_secs(60);
        let reader = FixedReader::new()
            .with("a.wav", SessionKind::Silent, dur)
            .with("b.wav", SessionKind::Loud, dur);
        let progress = FakeProgressReporter;
        let scan = ScanGaps::new(&reader, &progress, &NeverCalledAligner);

        let report = scan
            .scan_after_alignment(scan_request("a.wav", "b.wav", 60), aligned_result(Some(0.0)))
            .expect("scan should succeed");

        assert_eq!(report.gaps.len(), 1);
        assert!(report.gaps[0].b_has_energy);
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
        let scan = ScanGaps::new(&reader, &progress, &NeverCalledAligner);

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
        let scan = ScanGaps::new(&reader, &progress, &NeverCalledAligner);

        let report = scan
            .scan_after_alignment(scan_request("a.wav", "b.wav", 60), aligned_result(None))
            .expect("scan should succeed");

        assert_eq!(report.gaps.len(), 1);
        assert!(!report.gaps[0].b_has_energy);
        assert!(report.gaps[0].video_b_start_secs.is_none());
        assert!(report.gaps[0].video_b_end_secs.is_none());
        assert!(!report.gaps[0].is_fillable());
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
        let scan = ScanGaps::new(&reader, &progress, &NeverCalledAligner);

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
        let scan = ScanGaps::new(&reader, &progress, &NeverCalledAligner);

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
            alignment: clip_sync::AlignmentReport::from(&aligned_result(Some(0.0))),
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
            gap_offset_agreement: None,
            decode_chunk_secs: 60,
            scan_block_ms: 250,
            silence_peak_fraction: 0.01,
            limit_fill_to_mapped_region: true,
        };

        assert_eq!(report.gaps.len(), 2);
        assert_eq!(report.fillable_count(), 1);
        assert!(report.gaps[0].is_fillable());
        assert!(!report.gaps[1].is_fillable());
        assert!((report.gaps[0].duration_secs() - 60.0).abs() < 0.001);
    }

    #[test]
    fn scan_both_skips_cross_check_when_only_a_dropouts() {
        let dur = Duration::from_secs(60);
        let reader = FixedReader::new()
            .with("a.wav", SessionKind::Silent, dur)
            .with("b.wav", SessionKind::Loud, dur);
        let progress = FakeProgressReporter;
        let scan = ScanGaps::new(&reader, &progress, &NeverCalledAligner);

        let mut request = scan_request("a.wav", "b.wav", 60);
        request.scan_both = true;

        let report = scan
            .scan_after_alignment(request, aligned_result(Some(0.0)))
            .expect("scan should succeed");

        assert_eq!(report.gaps.len(), 1);
        assert!(report.gaps[0].b_has_energy);
        assert!(
            report.gap_offset_agreement.is_none(),
            "A-only silences must not produce cross-check"
        );
    }

    #[test]
    fn scan_both_produces_agreement_when_silence_colocated() {
        let dur = Duration::from_secs(60);
        let reader = FixedReader::new()
            .with("a.wav", SessionKind::Silent, dur)
            .with("b.wav", SessionKind::Silent, dur);
        let progress = FakeProgressReporter;
        let scan = ScanGaps::new(&reader, &progress, &NeverCalledAligner);

        let mut request = scan_request("a.wav", "b.wav", 60);
        request.scan_both = true;
        request.gap_offset_tolerance_secs = 0.5;

        let report = scan
            .scan_after_alignment(request, aligned_result(Some(0.0)))
            .expect("scan should succeed");

        let agreement = report
            .gap_offset_agreement
            .expect("agreement should be present when both timelines have silence");
        assert!(agreement.agrees, "colocated silence should agree");
        assert!(agreement.delta_secs < 0.001, "delta should be ~0");
    }

    #[test]
    fn scan_both_absent_when_scan_both_disabled() {
        let dur = Duration::from_secs(60);
        let reader = FixedReader::new()
            .with("a.wav", SessionKind::Silent, dur)
            .with("b.wav", SessionKind::Silent, dur);
        let progress = FakeProgressReporter;
        let scan = ScanGaps::new(&reader, &progress, &NeverCalledAligner);

        let report = scan
            .scan_after_alignment(scan_request("a.wav", "b.wav", 60), aligned_result(Some(0.0)))
            .expect("scan should succeed");

        assert!(report.gap_offset_agreement.is_none());
    }

    #[test]
    fn scan_both_absent_when_alignment_failed() {
        let dur = Duration::from_secs(60);
        let reader = FixedReader::new()
            .with("a.wav", SessionKind::Silent, dur)
            .with("b.wav", SessionKind::Silent, dur);
        let progress = FakeProgressReporter;
        let scan = ScanGaps::new(&reader, &progress, &NeverCalledAligner);

        let mut request = scan_request("a.wav", "b.wav", 60);
        request.scan_both = true;

        let report = scan
            .scan_after_alignment(request, aligned_result(None))
            .expect("scan should succeed");

        assert!(report.gap_offset_agreement.is_none());
    }

    #[test]
    fn scan_both_stops_b_scan_at_tail_seek_failure() {
        let dur = Duration::from_secs(125);
        let reader = FixedReader::new()
            .with("a.wav", SessionKind::Loud, dur)
            .with(
                "b.wav",
                SessionKind::TailSeekFail { fail_from_secs: 118.0 },
                dur,
            );
        let progress = FakeProgressReporter;
        let scan = ScanGaps::new(&reader, &progress, &NeverCalledAligner);

        let mut request = scan_request("a.wav", "b.wav", 60);
        request.scan_both = true;

        let report = scan
            .scan_after_alignment(request, aligned_result(Some(0.0)))
            .expect("tail seek on B should not fail the scan");

        assert!(report.gaps.is_empty());
    }

    #[test]
    fn scan_continues_past_midfile_extract_failure() {
        let dur = Duration::from_secs(180);
        let reader = FixedReader::new()
            .with(
                "a.wav",
                SessionKind::SkipWindow {
                    skip_start: 60.0,
                    skip_end: 120.0,
                },
                dur,
            )
            .with("b.wav", SessionKind::Loud, dur);
        let progress = FakeProgressReporter;
        let scan = ScanGaps::new(&reader, &progress, &NeverCalledAligner);

        let report = scan
            .scan_after_alignment(scan_request("a.wav", "b.wav", 60), aligned_result(Some(0.0)))
            .expect("scan should succeed");

        assert_eq!(
            report.gaps.len(),
            2,
            "expected gaps in [0,60) and [120,180); mid-file failure must not truncate scan"
        );
    }

    #[test]
    fn format_scan_summary_includes_thresholds_and_count() {
        let request = ScanGapsRequest {
            video_a: PathBuf::from("a.wav"),
            video_b: PathBuf::from("b.wav"),
            align: AlignConfig::default(),
            decode_chunk_secs: 10,
            scan_block_secs: 0.25,
            silence_peak_fraction: 0.01,
            absolute_silence_rms: 33.0,
            silence_hold_blocks: 2,
            min_gap_secs: 1.0,
            scan_both: true,
            gap_offset_tolerance_secs: 0.5,
            limit_fill_to_mapped_region: false,
        };
        let line = format_scan_summary(&request, 30);
        assert!(line.contains("30 silent run(s)"));
        assert!(line.contains("≥1000ms"));
        assert!(line.contains("block 250ms"));
        assert!(line.contains("1.0% peak"));
        assert!(line.contains("hold 500ms"));
        assert!(line.contains("decode 10s"));
        assert!(line.contains("scan-both on"));
        assert!(line.contains("rms floor 33"));
    }
}
