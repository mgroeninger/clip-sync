use std::path::PathBuf;

use crate::application::config::AppConfig;
use crate::application::error::{AppError, FingerprintError};
use crate::application::offset_refinement::refine_offset_estimate;
use crate::application::ports::MediaSession;
use crate::application::ports::{Aligner, Fingerprinter, MediaReader, ProgressReporter};
use crate::domain::{
    build_alignment_result, clip_windows, expand_window_for_slide, prepare_clip_for_fingerprint,
    resample_mono_pcm, select_aligned_subclip_pair, select_best_track, AlignmentResult, AudioTrack,
    ClipWindow, MediaSource, PcmPreparationOptions,
};
pub struct AlignVideosRequest {
    pub video_a: PathBuf,
    pub video_b: PathBuf,
    pub config: AppConfig,
}

#[derive(Debug)]
pub struct AlignVideosResponse {
    pub result: AlignmentResult,
}

pub struct AlignVideos<'a, MR, FP, AL, PR> {
    media_reader: &'a MR,
    fingerprinter: &'a FP,
    aligner: &'a AL,
    progress: &'a PR,
}

impl<'a, MR, FP, AL, PR> AlignVideos<'a, MR, FP, AL, PR>
where
    MR: MediaReader,
    FP: Fingerprinter,
    AL: Aligner,
    PR: ProgressReporter,
{
    pub fn new(
        media_reader: &'a MR,
        fingerprinter: &'a FP,
        aligner: &'a AL,
        progress: &'a PR,
    ) -> Self {
        Self {
            media_reader,
            fingerprinter,
            aligner,
            progress,
        }
    }

    pub fn execute(&self, request: AlignVideosRequest) -> Result<AlignVideosResponse, AppError> {
        request.config.validate()?;

        self.progress.phase(&format!(
            "clip-sync: aligning {} with {}",
            request.video_a.display(),
            request.video_b.display()
        ));

        self.progress.phase("Opening media");
        let session_a = self
            .media_reader
            .open(&MediaSource::new(request.video_a.clone()))?;
        let session_b = self
            .media_reader
            .open(&MediaSource::new(request.video_b.clone()))?;

        let result = if request.config.alignment.try_all_tracks {
            self.align_best_track_pair(&session_a, &session_b, &request)?
        } else {
            self.align_single_track_pair(&session_a, &session_b, &request)?
        };

        log_alignment_summary(&result, self.progress);

        Ok(AlignVideosResponse { result })
    }

    fn align_single_track_pair(
        &self,
        session_a: &MR::Session,
        session_b: &MR::Session,
        request: &AlignVideosRequest,
    ) -> Result<AlignmentResult, AppError> {
        let plan = request.config.clip.as_plan();
        let extracted_a =
            self.extract_clips(session_a, &plan, &request.config.clip, "video A", None)?;
        let extracted_b =
            self.extract_clips(session_b, &plan, &request.config.clip, "video B", None)?;
        self.align_extracted_pair(&extracted_a, &extracted_b, &request.config)
    }

    fn align_best_track_pair(
        &self,
        session_a: &MR::Session,
        session_b: &MR::Session,
        request: &AlignVideosRequest,
    ) -> Result<AlignmentResult, AppError> {
        let tracks_a = session_a.list_tracks()?;
        let tracks_b = session_b.list_tracks()?;
        let decodable_a: Vec<&AudioTrack> = tracks_a.iter().filter(|track| track.decodable).collect();
        let decodable_b: Vec<&AudioTrack> = tracks_b.iter().filter(|track| track.decodable).collect();

        if decodable_a.is_empty() || decodable_b.is_empty() {
            return Err(crate::domain::DomainError::NoDecodableAudioTracks.into());
        }

        let plan = request.config.clip.as_plan();
        let mut best: Option<(AlignmentResult, f32)> = None;

        for track_a in &decodable_a {
            for track_b in &decodable_b {
                self.progress.phase(&format!(
                    "Trying track pair A:{} / B:{}",
                    track_a.index, track_b.index
                ));
                let extracted_a = self.extract_clips(
                    session_a,
                    &plan,
                    &request.config.clip,
                    "video A",
                    Some(track_a),
                )?;
                let extracted_b = self.extract_clips(
                    session_b,
                    &plan,
                    &request.config.clip,
                    "video B",
                    Some(track_b),
                )?;
                let result = self.align_extracted_pair(&extracted_a, &extracted_b, &request.config)?;
                let score = mean_aligned_confidence(&result, request.config.alignment.min_match_score);
                if best.as_ref().is_none_or(|(_, best_score)| score > *best_score) {
                    best = Some((result, score));
                }
            }
        }

        best.map(|(result, _)| result).ok_or_else(|| {
            AppError::Alignment(crate::application::error::AlignmentError::EngineFailed(
                "no track pair produced an alignment".into(),
            ))
        })
    }

    fn align_extracted_pair(
        &self,
        extracted_a: &ExtractedClips,
        extracted_b: &ExtractedClips,
        config: &AppConfig,
    ) -> Result<AlignmentResult, AppError> {
        if extracted_a.windows.len() != extracted_b.windows.len() {
            return Err(AppError::Alignment(
                crate::application::error::AlignmentError::EngineFailed(
                    "clip count mismatch between inputs".into(),
                ),
            ));
        }

        let prep_options = PcmPreparationOptions {
            normalize_loudness: config.clip.normalize_loudness,
            trim_silence: config.clip.trim_silence,
            window_slide_secs: config.clip.window_slide_secs,
        };

        let mut clips_a = Vec::with_capacity(extracted_a.raw_clips.len());
        let mut clips_b = Vec::with_capacity(extracted_b.raw_clips.len());
        for (index, (raw_a, raw_b)) in extracted_a
            .raw_clips
            .iter()
            .zip(extracted_b.raw_clips.iter())
            .enumerate()
        {
            let window = &extracted_a.windows[index];
            let (mut clip_a, mut clip_b) = if config.clip.window_slide_secs > 0 {
                select_aligned_subclip_pair(raw_a, raw_b, window.duration())
            } else {
                (raw_a.clone(), raw_b.clone())
            };
            clip_a = prepare_clip_for_fingerprint(&clip_a, prep_options).map_err(map_prepare_error)?;
            clip_b = prepare_clip_for_fingerprint(&clip_b, prep_options).map_err(map_prepare_error)?;
            clips_a.push(clip_a);
            clips_b.push(clip_b);
        }

        self.progress.phase(&format!(
            "Fingerprinting {} clips...",
            clips_a.len() * 2
        ));
        let fingerprints_a: Vec<_> = clips_a
            .iter()
            .map(|clip| self.fingerprinter.fingerprint(clip))
            .collect::<Result<_, _>>()?;
        let fingerprints_b: Vec<_> = clips_b
            .iter()
            .map(|clip| self.fingerprinter.fingerprint(clip))
            .collect::<Result<_, _>>()?;

        self.progress.phase("Searching for match...");
        let mut estimates = Vec::with_capacity(fingerprints_a.len());

        for (index, (left, right)) in fingerprints_a.iter().zip(fingerprints_b.iter()).enumerate()
        {
            let mut estimate = self.aligner.find_offset(left, right)?;
            if config.alignment.refine_offset_with_pcm
                && estimate.confidence >= config.alignment.min_match_score * 0.5
            {
                estimate = refine_offset_estimate(&clips_a[index], &clips_b[index], estimate);
            }

            let window = &extracted_a.windows[index];
            self.progress.phase(&format!(
                "{} clip [{}–{}]: {} (confidence: {:.2})",
                clip_label_name(window.label),
                format_duration(window.start),
                format_duration(window.end),
                if estimate.confidence >= config.alignment.min_match_score {
                    format!("offset {:+.3}s", estimate.offset_secs)
                } else {
                    "no match".into()
                },
                estimate.confidence
            ));

            estimates.push(estimate);
        }

        Ok(build_alignment_result(
            &extracted_a.windows,
            &estimates,
            config.alignment.min_match_score,
            config.alignment.prefer_start_clip,
            config.alignment.require_consistent_offsets,
            Some(extracted_a.duration),
            Some(extracted_b.duration),
        ))
    }

    fn extract_clips(
        &self,
        session: &MR::Session,
        plan: &crate::domain::ClipPlan,
        clip_config: &crate::application::config::ClipConfig,
        label: &str,
        track: Option<&AudioTrack>,
    ) -> Result<ExtractedClips, AppError> {
        let tracks = session.list_tracks()?;
        let track = match track {
            Some(track) => track,
            None => select_best_track(&tracks)?,
        };
        self.progress.phase(&format!(
            "Selected track {} ({} Hz, {} channel{}, {}decodable)",
            track.index,
            track.sample_rate,
            track.channels,
            if track.channels == 1 { "" } else { "s" },
            if track.decodable { "" } else { "not " }
        ));

        let duration = track.duration.filter(|value| !value.is_zero()).ok_or(
            AppError::Domain(crate::domain::DomainError::InvalidDuration),
        )?;
        let windows = clip_windows(duration, plan)?;

        self.progress.phase(&format_clip_plan(label, &windows));

        let mut raw_clips = Vec::with_capacity(windows.len());
        for (index, window) in windows.iter().enumerate() {
            let extract_window = expand_window_for_slide(
                window,
                clip_config.window_slide_secs,
                duration,
            );
            let progress_label = format!(
                "Extracting clip {}/{} ({label}, {})",
                index + 1,
                windows.len(),
                format_duration(window.duration())
            );
            let mut clip =
                session.extract_mono(track, &extract_window, self.progress, &progress_label)?;
            if let Some(target_rate) = clip_config.target_sample_rate {
                clip = resample_mono_pcm(&clip, target_rate);
            }
            raw_clips.push(clip);
        }

        Ok(ExtractedClips {
            raw_clips,
            windows,
            duration,
        })
    }
}

fn map_prepare_error(error: crate::domain::DomainError) -> AppError {
    match error {
        crate::domain::DomainError::InsufficientAudio | crate::domain::DomainError::EmptyClip => {
            AppError::Fingerprint(FingerprintError::InvalidPcm(
                "insufficient audio content for fingerprinting".into(),
            ))
        }
        other => AppError::Domain(other),
    }
}

fn mean_aligned_confidence(result: &AlignmentResult, min_match_score: f32) -> f32 {
    let aligned: Vec<f32> = result
        .clips
        .iter()
        .filter(|clip| clip.confidence >= min_match_score)
        .map(|clip| clip.confidence)
        .collect();
    if aligned.is_empty() {
        0.0
    } else {
        aligned.iter().sum::<f32>() / aligned.len() as f32
    }
}

struct ExtractedClips {
    raw_clips: Vec<crate::domain::MonoPcmClip>,
    windows: Vec<ClipWindow>,
    duration: std::time::Duration,
}

fn log_alignment_summary(result: &AlignmentResult, progress: &impl ProgressReporter) {
    progress.phase(&format!(
        "Start clip aligned: {}",
        yes_no(result.start_aligned)
    ));

    if let Some(end_aligned) = result.end_aligned {
        progress.phase(&format!("End clip aligned: {}", yes_no(end_aligned)));
    }

    match result.recommended_offset_secs {
        Some(offset) => progress.phase(&format!(
            "Recommended offset: {:+.3}s ({})",
            offset,
            if result.offsets_consistent {
                "clip offsets agree"
            } else {
                "clip offsets disagree; using configured preference"
            }
        )),
        None => progress.phase("Recommended offset: none (no confident clip matches)"),
    }

    if let Some(overlap) = result.start_overlap {
        progress.phase(&format!(
            "Overlap on video A: {}",
            format_overlap_window(overlap.video_a_start_secs, overlap.video_a_end_secs)
        ));
        progress.phase(&format!(
            "Overlap on video B: {}",
            format_overlap_window(overlap.video_b_start_secs, overlap.video_b_end_secs)
        ));
        progress.phase(&format!(
            "Shared length: {}",
            format_timestamp(overlap.shared_length_secs)
        ));
    }
}

fn format_overlap_window(start_secs: f64, end_secs: f64) -> String {
    format!(
        "[{}–{}]",
        format_timestamp(start_secs),
        format_timestamp(end_secs)
    )
}

fn format_timestamp(secs: f64) -> String {
    let total = secs.round().max(0.0) as u64;
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn format_clip_plan(label: &str, windows: &[ClipWindow]) -> String {
    let parts: Vec<String> = windows
        .iter()
        .map(|window| {
            format!(
                "[{}–{}] {} ({})",
                format_duration(window.start),
                format_duration(window.end),
                clip_label_name(window.label),
                format_duration(window.duration())
            )
        })
        .collect();

    format!(
        "Clip plan for {label}: {} clip(s) — {}",
        windows.len(),
        parts.join(", ")
    )
}

fn clip_label_name(label: crate::domain::ClipLabel) -> &'static str {
    use crate::domain::ClipLabel;
    match label {
        ClipLabel::Start => "start",
        ClipLabel::Interior => "interior",
        ClipLabel::End => "end",
    }
}

fn format_duration(duration: std::time::Duration) -> String {
    let total_secs = duration.as_secs();
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;

    use super::*;
    use crate::application::config::{AppConfig, ClipConfig};
    use crate::application::error::{AlignmentError, AppError, ConfigError, FingerprintError, MediaError};
    use crate::application::testing::fakes::{
        FakeAligner, FakeFingerprinter, FakeMediaReader, FakeMediaSession, FakeProgressReporter,
    };
    use crate::domain::{ClipLabel, ClipMatchEstimate, DomainError};

    fn mins(m: u64) -> Duration {
        Duration::from_secs(m * 60)
    }

    fn two_clip_config() -> AppConfig {
        AppConfig {
            clip: ClipConfig {
                clip_length: mins(1),
                num_clips: 2,
                target_sample_rate: None,
                normalize_loudness: false,
                trim_silence: false,
                window_slide_secs: 0,
                ..ClipConfig::default()
            },
            ..Default::default()
        }
    }

    fn request(config: AppConfig) -> AlignVideosRequest {
        AlignVideosRequest {
            video_a: PathBuf::from("a.wav"),
            video_b: PathBuf::from("b.wav"),
            config,
        }
    }

    fn matched_reader() -> FakeMediaReader {
        FakeMediaReader::new()
            .with_session("a.wav", FakeMediaSession::with_duration(mins(3)))
            .with_session("b.wav", FakeMediaSession::with_duration(mins(3)))
    }

    #[test]
    fn execute_returns_alignment_when_clips_match() {
        let reader = matched_reader();
        let fingerprinter = FakeFingerprinter::new();
        let aligner = FakeAligner::with_estimate(ClipMatchEstimate {
            offset_secs: 12.0,
            confidence: 0.9,
        });
        let progress = FakeProgressReporter;
        let use_case = AlignVideos::new(&reader, &fingerprinter, &aligner, &progress);

        let response = use_case
            .execute(request(two_clip_config()))
            .expect("execute should succeed");

        assert!(response.result.start_aligned);
        assert_eq!(response.result.end_aligned, Some(true));
        assert_eq!(response.result.recommended_offset_secs, Some(12.0));
        assert!(response.result.offsets_consistent);
        assert_eq!(response.result.clips.len(), 2);

        let overlap = response
            .result
            .start_overlap
            .expect("expected start overlap");
        assert_eq!(overlap.video_a_start_secs, 0.0);
        assert_eq!(overlap.video_b_start_secs, 12.0);
        assert_eq!(overlap.shared_length_secs, 60.0);
    }

    #[test]
    fn execute_reports_no_alignment_when_below_threshold() {
        let reader = matched_reader();
        let fingerprinter = FakeFingerprinter::new();
        let aligner = FakeAligner::with_estimate(ClipMatchEstimate {
            offset_secs: 5.0,
            confidence: 0.2,
        });
        let progress = FakeProgressReporter;
        let use_case = AlignVideos::new(&reader, &fingerprinter, &aligner, &progress);

        let response = use_case
            .execute(request(two_clip_config()))
            .expect("low confidence should still succeed");

        assert!(!response.result.start_aligned);
        assert_eq!(response.result.end_aligned, Some(false));
        assert_eq!(response.result.recommended_offset_secs, None);
    }

    #[test]
    fn execute_rejects_invalid_config_before_opening_media() {
        let reader = matched_reader();
        let fingerprinter = FakeFingerprinter::new();
        let aligner = FakeAligner::with_estimate(ClipMatchEstimate {
            offset_secs: 0.0,
            confidence: 1.0,
        });
        let progress = FakeProgressReporter;
        let use_case = AlignVideos::new(&reader, &fingerprinter, &aligner, &progress);

        let mut config = two_clip_config();
        config.clip.clip_length = Duration::from_secs(30);

        let error = use_case.execute(request(config)).unwrap_err();
        assert!(matches!(
            error,
            AppError::Config(ConfigError::InvalidValue { field, .. }) if field == "clip_length"
        ));
        assert_eq!(reader.open_calls(), 0);
    }

    #[test]
    fn execute_rejects_clip_count_mismatch() {
        let reader = FakeMediaReader::new()
            .with_session("a.wav", FakeMediaSession::with_duration(mins(3)))
            .with_session("b.wav", FakeMediaSession::with_duration(Duration::from_secs(45)));
        let fingerprinter = FakeFingerprinter::new();
        let aligner = FakeAligner::with_estimate(ClipMatchEstimate {
            offset_secs: 0.0,
            confidence: 1.0,
        });
        let progress = FakeProgressReporter;
        let use_case = AlignVideos::new(&reader, &fingerprinter, &aligner, &progress);

        let error = use_case.execute(request(two_clip_config())).unwrap_err();
        assert!(matches!(
            error,
            AppError::Alignment(AlignmentError::EngineFailed(_))
        ));
    }

    #[test]
    fn execute_propagates_media_open_error() {
        let reader =
            FakeMediaReader::new().with_open_error(MediaError::FileNotFound(PathBuf::from("a.wav")));
        let fingerprinter = FakeFingerprinter::new();
        let aligner = FakeAligner::with_estimate(ClipMatchEstimate {
            offset_secs: 0.0,
            confidence: 1.0,
        });
        let progress = FakeProgressReporter;
        let use_case = AlignVideos::new(&reader, &fingerprinter, &aligner, &progress);

        let error = use_case.execute(request(two_clip_config())).unwrap_err();
        assert!(matches!(error, AppError::Media(MediaError::FileNotFound(_))));
    }

    #[test]
    fn execute_propagates_fingerprint_error() {
        let reader = matched_reader();
        let fingerprinter =
            FakeFingerprinter::with_error(FingerprintError::InvalidPcm("bad clip".into()));
        let aligner = FakeAligner::with_estimate(ClipMatchEstimate {
            offset_secs: 0.0,
            confidence: 1.0,
        });
        let progress = FakeProgressReporter;
        let use_case = AlignVideos::new(&reader, &fingerprinter, &aligner, &progress);

        let error = use_case.execute(request(two_clip_config())).unwrap_err();
        assert!(matches!(
            error,
            AppError::Fingerprint(FingerprintError::InvalidPcm(_))
        ));
    }

    #[test]
    fn execute_propagates_alignment_engine_error() {
        let reader = matched_reader();
        let fingerprinter = FakeFingerprinter::new();
        let aligner = FakeAligner::with_error(AlignmentError::EngineFailed(
            "matcher exploded".into(),
        ));
        let progress = FakeProgressReporter;
        let use_case = AlignVideos::new(&reader, &fingerprinter, &aligner, &progress);

        let error = use_case.execute(request(two_clip_config())).unwrap_err();
        assert!(matches!(
            error,
            AppError::Alignment(AlignmentError::EngineFailed(_))
        ));
    }

    #[test]
    fn execute_propagates_invalid_track_duration() {
        let reader = FakeMediaReader::new()
            .with_session(
                "a.wav",
                FakeMediaSession::with_tracks(vec![crate::domain::AudioTrack {
                    index: 0,
                    codec: "test".into(),
                    channels: 1,
                    sample_rate: 44_100,
                    bitrate: None,
                    duration: None,
                    decodable: true,
                }]),
            )
            .with_session("b.wav", FakeMediaSession::with_duration(mins(3)));
        let fingerprinter = FakeFingerprinter::new();
        let aligner = FakeAligner::with_estimate(ClipMatchEstimate {
            offset_secs: 0.0,
            confidence: 1.0,
        });
        let progress = FakeProgressReporter;
        let use_case = AlignVideos::new(&reader, &fingerprinter, &aligner, &progress);

        let error = use_case.execute(request(two_clip_config())).unwrap_err();
        assert!(matches!(
            error,
            AppError::Domain(DomainError::InvalidDuration)
        ));
    }

    #[test]
    fn execute_resamples_when_target_sample_rate_is_set() {
        let reader = matched_reader();
        let fingerprinter = FakeFingerprinter::new();
        let aligner = FakeAligner::with_estimate(ClipMatchEstimate {
            offset_secs: 0.0,
            confidence: 0.9,
        });
        let progress = FakeProgressReporter;
        let use_case = AlignVideos::new(&reader, &fingerprinter, &aligner, &progress);

        let mut config = two_clip_config();
        config.clip.target_sample_rate = Some(11_025);

        use_case
            .execute(request(config))
            .expect("execute should succeed");

        let seen = fingerprinter.seen_sample_rates();
        assert_eq!(seen.len(), 4);
        assert!(seen.iter().all(|rate| *rate == 11_025));
    }

    #[test]
    fn execute_propagates_media_extract_error() {
        let reader = FakeMediaReader::new()
            .with_session(
                "a.wav",
                FakeMediaSession::with_duration(mins(3)).with_extract_error(MediaError::DecodeFailed {
                    track: 0,
                    detail: "boom".into(),
                }),
            )
            .with_session("b.wav", FakeMediaSession::with_duration(mins(3)));
        let fingerprinter = FakeFingerprinter::new();
        let aligner = FakeAligner::with_estimate(ClipMatchEstimate {
            offset_secs: 0.0,
            confidence: 1.0,
        });
        let progress = FakeProgressReporter;
        let use_case = AlignVideos::new(&reader, &fingerprinter, &aligner, &progress);

        let error = use_case.execute(request(two_clip_config())).unwrap_err();
        assert!(matches!(
            error,
            AppError::Media(MediaError::DecodeFailed { .. })
        ));
    }

    #[test]
    fn execute_prefers_start_clip_when_offsets_disagree() {
        let reader = matched_reader();
        let fingerprinter = FakeFingerprinter::new();
        let aligner = FakeAligner::with_estimates(vec![
            ClipMatchEstimate {
                offset_secs: 10.0,
                confidence: 0.9,
            },
            ClipMatchEstimate {
                offset_secs: 20.0,
                confidence: 0.9,
            },
        ]);
        let progress = FakeProgressReporter;
        let use_case = AlignVideos::new(&reader, &fingerprinter, &aligner, &progress);

        let mut config = two_clip_config();
        config.alignment.prefer_start_clip = true;
        config.alignment.require_consistent_offsets = false;

        let response = use_case
            .execute(request(config))
            .expect("execute should succeed");

        assert!(!response.result.offsets_consistent);
        assert_eq!(response.result.recommended_offset_secs, Some(10.0));
        assert_eq!(response.result.clips[0].label, ClipLabel::Start);
        assert_eq!(response.result.clips[1].label, ClipLabel::End);
    }

    #[test]
    fn execute_detects_known_offset_through_real_wav_pipeline() {
        use crate::application::testing::audio_fixtures::write_offset_chirp_wav_pair;
        use crate::application::config::ChromaprintPreset;
        use crate::infrastructure::chromaprint::{ChromaprintAligner, ChromaprintFingerprinter};
        use crate::infrastructure::symphonia::SymphoniaMediaReader;

        const SAMPLE_RATE: u32 = 44_100;
        const TOTAL_SECS: u32 = 120;
        const OFFSET_SECS: u32 = 3;

        let temp = tempfile::tempdir().expect("tempdir");
        let (path_a, path_b) = write_offset_chirp_wav_pair(
            temp.path(),
            SAMPLE_RATE,
            TOTAL_SECS,
            OFFSET_SECS,
        );

        let config = AppConfig {
            clip: ClipConfig {
                clip_length: Duration::from_secs(60),
                num_clips: 1,
                target_sample_rate: Some(SAMPLE_RATE),
                ..ClipConfig::default()
            },
            ..Default::default()
        };

        let media_reader = SymphoniaMediaReader;
        let preset = ChromaprintPreset::default();
        let fingerprinter = ChromaprintFingerprinter::new(preset);
        let aligner = ChromaprintAligner::new(preset);
        let progress = FakeProgressReporter;
        let use_case = AlignVideos::new(&media_reader, &fingerprinter, &aligner, &progress);

        let response = use_case
            .execute(AlignVideosRequest {
                video_a: path_a,
                video_b: path_b,
                config,
            })
            .expect("execute should succeed");

        let offset = response
            .result
            .recommended_offset_secs
            .expect("expected aligned offset");
        assert!(response.result.start_aligned);
        assert!(
            (offset - f64::from(OFFSET_SECS)).abs() < 1.0,
            "offset={offset}, expected about +{OFFSET_SECS}"
        );
        assert!(
            response.result.clips[0].confidence >= 0.5,
            "confidence={}",
            response.result.clips[0].confidence
        );
    }
}
