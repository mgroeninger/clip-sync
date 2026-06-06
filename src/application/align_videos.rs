use std::path::PathBuf;

use crate::application::ports::MediaSession;
use crate::application::config::AppConfig;
use crate::application::error::AppError;
use crate::application::ports::{Aligner, Fingerprinter, MediaReader, ProgressReporter};
use crate::domain::{
    build_alignment_result, clip_windows, resample_mono_pcm, select_best_track, AlignmentResult,
    ClipWindow, MediaSource,
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

        let source_a = MediaSource::new(request.video_a);
        let source_b = MediaSource::new(request.video_b);
        let plan = request.config.clip.as_plan();

        self.progress.phase("Opening media");
        let session_a = self.media_reader.open(&source_a)?;
        let session_b = self.media_reader.open(&source_b)?;

        let extracted_a = self.extract_clips(&session_a, &plan, &request.config.clip, "video A")?;
        let extracted_b = self.extract_clips(&session_b, &plan, &request.config.clip, "video B")?;

        if extracted_a.windows.len() != extracted_b.windows.len() {
            return Err(AppError::Alignment(
                crate::application::error::AlignmentError::EngineFailed(
                    "clip count mismatch between inputs".into(),
                ),
            ));
        }

        self.progress.phase(&format!(
            "Fingerprinting {} clips...",
            extracted_a.clips.len() * 2
        ));
        let fingerprints_a: Vec<_> = extracted_a
            .clips
            .iter()
            .map(|clip| self.fingerprinter.fingerprint(clip))
            .collect::<Result<_, _>>()?;
        let fingerprints_b: Vec<_> = extracted_b
            .clips
            .iter()
            .map(|clip| self.fingerprinter.fingerprint(clip))
            .collect::<Result<_, _>>()?;

        self.progress.phase("Searching for match...");
        let mut estimates = Vec::with_capacity(fingerprints_a.len());

        for (index, (left, right)) in fingerprints_a.iter().zip(fingerprints_b.iter()).enumerate()
        {
            let estimate = self.aligner.find_offset(left, right)?;
            let window = &extracted_a.windows[index];

            self.progress.phase(&format!(
                "{} clip [{}–{}]: {} (confidence: {:.2})",
                clip_label_name(window.label),
                format_duration(window.start),
                format_duration(window.end),
                if estimate.confidence >= request.config.alignment.min_match_score {
                    format!("offset {:+.3}s", estimate.offset_secs)
                } else {
                    "no match".into()
                },
                estimate.confidence
            ));

            estimates.push(estimate);
        }

        let result = build_alignment_result(
            &extracted_a.windows,
            &estimates,
            request.config.alignment.min_match_score,
            request.config.alignment.prefer_start_clip,
        );

        log_alignment_summary(&result, self.progress);

        Ok(AlignVideosResponse { result })
    }

    fn extract_clips(
        &self,
        session: &MR::Session,
        plan: &crate::domain::ClipPlan,
        clip_config: &crate::application::config::ClipConfig,
        label: &str,
    ) -> Result<ExtractedClips, AppError> {
        let tracks = session.list_tracks()?;
        let track = select_best_track(&tracks)?;
        self.progress.phase(&format!(
            "Selected track {} ({} Hz, {} channel{})",
            track.index,
            track.sample_rate,
            track.channels,
            if track.channels == 1 { "" } else { "s" }
        ));

        let duration = track.duration.filter(|value| !value.is_zero()).ok_or(
            AppError::Domain(crate::domain::DomainError::InvalidDuration),
        )?;
        let windows = clip_windows(duration, plan)?;

        self.progress.phase(&format_clip_plan(label, &windows));

        let mut clips = Vec::with_capacity(windows.len());
        for (index, window) in windows.iter().enumerate() {
            let progress_label = format!("Extracting clip {}/{} ({label})", index + 1, windows.len());
            let clip = session.extract_mono(track, window, self.progress, &progress_label)?;
            let clip = if let Some(target_rate) = clip_config.target_sample_rate {
                resample_mono_pcm(&clip, target_rate)
            } else {
                clip
            };
            clips.push(clip);
        }

        Ok(ExtractedClips { clips, windows })
    }
}

struct ExtractedClips {
    clips: Vec<crate::domain::MonoPcmClip>,
    windows: Vec<ClipWindow>,
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
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn format_clip_plan(label: &str, windows: &[ClipWindow]) -> String {
    let parts: Vec<String> = windows
        .iter()
        .map(|window| {
            format!(
                "[{}–{}] {}",
                format_duration(window.start),
                format_duration(window.end),
                clip_label_name(window.label)
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
                FakeMediaSession::with_tracks(
                    vec![crate::domain::AudioTrack {
                        index: 0,
                        codec: "test".into(),
                        channels: 1,
                        sample_rate: 44_100,
                        bitrate: None,
                        duration: None,
                    }],
                    mins(3),
                ),
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

        let response = use_case
            .execute(request(config))
            .expect("execute should succeed");

        assert!(!response.result.offsets_consistent);
        assert_eq!(response.result.recommended_offset_secs, Some(10.0));
        assert_eq!(response.result.clips[0].label, ClipLabel::Start);
        assert_eq!(response.result.clips[1].label, ClipLabel::End);
    }
}
