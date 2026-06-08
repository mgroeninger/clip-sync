use std::path::PathBuf;
use std::time::Duration;

use crate::application::config::AlignConfig;
use crate::application::error::{AppError, FingerprintError};
use crate::application::high_rate_refinement::{apply_high_rate_refinement, HighRateRefinementInput};
use crate::application::offset_refinement::{refine_offset_around_prior, refine_offset_estimate};
use crate::application::ports::MediaSession;
use crate::application::ports::{Aligner, Fingerprinter, MediaReader, ProgressReporter};
use crate::domain::{
    build_alignment_result, clip_windows_with_options, decoded_timeline_extent,
    end_clip_extract_unreliable, expand_window_for_slide, prepare_clip_for_fingerprint,
    resample_mono_pcm, select_aligned_subclip_pair, select_best_track, truncate_padded_tail,
    AlignmentMergePolicy, AlignmentResult, AudioTrack, ClipLabel, ClipMatchEstimate,
    ClipPairReportInput, ClipPlanningOptions,
    ClipWindow, DomainError, MediaSource, MonoPcmClip, PcmPreparationOptions,
};
pub struct AlignVideosRequest {
    pub video_a: PathBuf,
    pub video_b: PathBuf,
    pub config: AlignConfig,
}

#[derive(Debug)]
pub struct AlignVideosResponse {
    pub result: AlignmentResult,
}

pub struct AlignVideos<'a, MR, FP, AL> {
    media_reader: &'a MR,
    fingerprinter: &'a FP,
    aligner: &'a AL,
    progress: &'a dyn ProgressReporter,
}

impl<'a, MR, FP, AL> AlignVideos<'a, MR, FP, AL>
where
    MR: MediaReader,
    FP: Fingerprinter,
    AL: Aligner,
{
    pub fn new(
        media_reader: &'a MR,
        fingerprinter: &'a FP,
        aligner: &'a AL,
        progress: &'a dyn ProgressReporter,
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

        let outcome = if request.config.alignment.try_all_tracks {
            self.align_best_track_pair(&session_a, &session_b, &request)?
        } else {
            self.align_single_track_pair(&session_a, &session_b, &request)?
        };

        let mut result = outcome.result;
        apply_high_rate_refinement(
            &HighRateRefinementInput {
                session_a: &session_a,
                session_b: &session_b,
                track_a: &outcome.track_a,
                track_b: &outcome.track_b,
                discovery_windows: &outcome.discovery_windows,
                duration_a: outcome.duration_a,
                duration_b: outcome.duration_b,
                decoded_extent_a: outcome.decoded_extent_a,
                decoded_extent_b: outcome.decoded_extent_b,
            },
            &request.config.alignment,
            &mut result,
            self.progress,
        );

        log_alignment_summary(&result, self.progress);

        Ok(AlignVideosResponse { result })
    }

    fn align_single_track_pair(
        &self,
        session_a: &MR::Session,
        session_b: &MR::Session,
        request: &AlignVideosRequest,
    ) -> Result<AlignmentOutcome, AppError> {
        let plan = request.config.clip.as_plan();
        let extracted_a = self.extract_clips(
            session_a,
            &plan,
            &request.config.clip,
            &request.config.alignment,
            "video A",
            None,
        )?;
        let extracted_b = self.extract_clips(
            session_b,
            &plan,
            &request.config.clip,
            &request.config.alignment,
            "video B",
            None,
        )?;
        let result = self.align_extracted_pair(&extracted_a, &extracted_b, &request.config)?;
        Ok(AlignmentOutcome {
            result,
            track_a: extracted_a.track,
            track_b: extracted_b.track,
            discovery_windows: extracted_a.windows,
            duration_a: extracted_a.duration,
            duration_b: extracted_b.duration,
            decoded_extent_a: extracted_a.decoded_extent,
            decoded_extent_b: extracted_b.decoded_extent,
        })
    }

    fn align_best_track_pair(
        &self,
        session_a: &MR::Session,
        session_b: &MR::Session,
        request: &AlignVideosRequest,
    ) -> Result<AlignmentOutcome, AppError> {
        let tracks_a = session_a.list_tracks()?;
        let tracks_b = session_b.list_tracks()?;
        let decodable_a: Vec<&AudioTrack> = tracks_a.iter().filter(|track| track.decodable).collect();
        let decodable_b: Vec<&AudioTrack> = tracks_b.iter().filter(|track| track.decodable).collect();

        if decodable_a.is_empty() || decodable_b.is_empty() {
            return Err(crate::domain::DomainError::NoDecodableAudioTracks.into());
        }

        let plan = request.config.clip.as_plan();
        let mut best: Option<(AlignmentOutcome, f32)> = None;

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
                    &request.config.alignment,
                    "video A",
                    Some(track_a),
                )?;
                let extracted_b = self.extract_clips(
                    session_b,
                    &plan,
                    &request.config.clip,
                    &request.config.alignment,
                    "video B",
                    Some(track_b),
                )?;
                let result = self.align_extracted_pair(&extracted_a, &extracted_b, &request.config)?;
                let score = mean_aligned_confidence(&result, request.config.alignment.min_match_score);
                let outcome = AlignmentOutcome {
                    result,
                    track_a: extracted_a.track,
                    track_b: extracted_b.track,
                    discovery_windows: extracted_a.windows,
                    duration_a: extracted_a.duration,
                    duration_b: extracted_b.duration,
                    decoded_extent_a: extracted_a.decoded_extent,
                    decoded_extent_b: extracted_b.decoded_extent,
                };
                if best.as_ref().is_none_or(|(_, best_score)| score > *best_score) {
                    best = Some((outcome, score));
                }
            }
        }

        best.map(|(outcome, _)| outcome).ok_or_else(|| {
            AppError::Alignment(crate::application::error::AlignmentError::EngineFailed(
                "no track pair produced an alignment".into(),
            ))
        })
    }

    fn align_extracted_pair(
        &self,
        extracted_a: &ExtractedClips,
        extracted_b: &ExtractedClips,
        config: &AlignConfig,
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

        self.progress.phase("Searching for match...");
        let mut estimates = Vec::with_capacity(extracted_a.raw_clips.len());
        let mut start_prior: Option<ClipMatchEstimate> = None;

        for (index, (raw_a, raw_b)) in extracted_a
            .raw_clips
            .iter()
            .zip(extracted_b.raw_clips.iter())
            .enumerate()
        {
            let window = &extracted_a.windows[index];
            let end_clip_unreliable = window.label == ClipLabel::End
                && (extracted_a.end_clip_unreliable || extracted_b.end_clip_unreliable);

            let (raw_a, raw_b) = if window.label == ClipLabel::End {
                (
                    truncate_padded_tail(raw_a.clone()),
                    truncate_padded_tail(raw_b.clone()),
                )
            } else {
                (raw_a.clone(), raw_b.clone())
            };

            let (clip_a, clip_b) = if config.clip.window_slide_secs > 0 {
                select_aligned_subclip_pair(&raw_a, &raw_b, window.duration())
            } else {
                (raw_a, raw_b)
            };

            let prepared_a = prepare_clip_for_fingerprint(&clip_a, prep_options);
            let prepared_b = prepare_clip_for_fingerprint(&clip_b, prep_options);

            if is_skippable_prepare_error(&prepared_a) || is_skippable_prepare_error(&prepared_b)
            {
                self.progress.phase(&format!(
                    "{} clip [{}–{}]: skipped (insufficient audio)",
                    clip_label_name(window.label),
                    format_duration(window.start),
                    format_duration(window.end),
                ));
                estimates.push(ClipMatchEstimate {
                    offset_secs: 0.0,
                    confidence: 0.0,
                });
                continue;
            }

            if end_clip_unreliable && config.alignment.skip_unreliable_end_clip {
                self.progress.phase(&format!(
                    "end clip [{}–{}]: skipped (unreliable tail extract)",
                    format_duration(window.start),
                    format_duration(window.end),
                ));
                estimates.push(ClipMatchEstimate {
                    offset_secs: 0.0,
                    confidence: 0.0,
                });
                continue;
            }

            let clip_a = prepared_a.map_err(map_prepare_error)?;
            let clip_b = prepared_b.map_err(map_prepare_error)?;

            let use_end_prior = window.label == ClipLabel::End
                && config.alignment.refine_end_clip_around_start_offset
                && start_prior.is_some_and(|prior| {
                    prior.confidence >= config.alignment.min_match_score * 0.5
                });

            let estimate = if use_end_prior {
                let prior = start_prior.expect("checked above");
                if config.alignment.refine_offset_with_pcm {
                    refine_offset_around_prior(
                        &clip_a,
                        &clip_b,
                        prior,
                        config.alignment.end_clip_refine_radius_secs,
                    )
                } else {
                    prior
                }
            } else {
                let fingerprint_a = self.fingerprinter.fingerprint(&clip_a)?;
                let fingerprint_b = self.fingerprinter.fingerprint(&clip_b)?;

                let mut chromaprint_estimate = self
                    .aligner
                    .find_offset(&fingerprint_a, &fingerprint_b)?;
                if config.alignment.refine_offset_with_pcm
                    && chromaprint_estimate.confidence >= config.alignment.min_match_score * 0.5
                {
                    chromaprint_estimate =
                        refine_offset_estimate(&clip_a, &clip_b, chromaprint_estimate);
                }
                chromaprint_estimate
            };

            if window.label == ClipLabel::Start && estimate.confidence > 0.0 {
                start_prior = Some(estimate);
            }

            self.progress.phase(&format!(
                "{} clip [{}–{}]{}: {} (confidence: {:.2})",
                clip_label_name(window.label),
                format_duration(window.start),
                format_duration(window.end),
                if use_end_prior {
                    format!(
                        " (refined ±{:.0}s around start)",
                        config.alignment.end_clip_refine_radius_secs
                    )
                } else {
                    String::new()
                },
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
            ClipPairReportInput {
                windows: &extracted_a.windows,
                estimates: &estimates,
                decode_skips_a: &extracted_a.decode_skips,
                decode_skips_b: &extracted_b.decode_skips,
                duration_a: Some(extracted_a.duration),
                duration_b: Some(extracted_b.duration),
            },
            AlignmentMergePolicy {
                min_match_score: config.alignment.min_match_score,
                prefer_start_clip: config.alignment.prefer_start_clip,
                require_consistent_offsets: config.alignment.require_consistent_offsets,
            },
        ))
    }

    fn extract_clips(
        &self,
        session: &MR::Session,
        plan: &crate::domain::ClipPlan,
        clip_config: &crate::application::config::ClipConfig,
        alignment_config: &crate::application::config::AlignmentConfig,
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

        let decodable_extent = if plan.num_clips >= 2
            && alignment_config.clamp_end_clip_to_decodable_extent
        {
            match session.track_decodable_extent(track) {
                Ok(extent) => extent,
                Err(_) => {
                    self.progress.phase(&format!(
                        "{label}: tail extent scan failed; using container duration for end clip"
                    ));
                    None
                }
            }
        } else {
            None
        };

        if let Some(extent) = decodable_extent {
            if extent + Duration::from_secs(1) < duration {
                self.progress.phase(&format!(
                    "{label}: decodable extent {:.0}s (container {:.0}s)",
                    extent.as_secs_f64(),
                    duration.as_secs_f64()
                ));
            }
        }

        let planning = ClipPlanningOptions {
            decodable_extent,
            end_tail_inset: Duration::from_secs_f64(
                alignment_config.end_clip_tail_inset_secs.max(0.0),
            ),
        };
        let windows = clip_windows_with_options(duration, plan, planning)?;
        let timeline_end = windows
            .iter()
            .find(|window| window.label == ClipLabel::End)
            .map(|window| window.end)
            .unwrap_or(duration);

        self.progress.phase(&format_clip_plan(label, &windows));

        let mut extract_order: Vec<usize> = (0..windows.len()).collect();
        if windows.len() > 1 {
            extract_order.sort_by_key(|&index| windows[index].start);
            let chronological: Vec<usize> = (0..windows.len()).collect();
            if extract_order != chronological {
                self.progress.phase(&format!(
                    "Extracting {} clip(s) in chronological order ({label})",
                    windows.len()
                ));
            }
        }

        let mut raw_clips: Vec<Option<MonoPcmClip>> = vec![None; windows.len()];
        for (step, &index) in extract_order.iter().enumerate() {
            let window = &windows[index];
            let extract_window = expand_window_for_slide(
                window,
                clip_config.window_slide_secs,
                timeline_end,
            );
            let progress_label = format!(
                "Extracting clip {}/{} ({label}, {})",
                step + 1,
                windows.len(),
                format_duration(window.duration())
            );
            let mut clip =
                session.extract_mono(track, &extract_window, self.progress, &progress_label)?;
            if let Some(target_rate) = clip_config.target_sample_rate {
                clip = resample_mono_pcm(&clip, target_rate);
            }
            raw_clips[index] = Some(clip);
        }

        let raw_clips: Vec<MonoPcmClip> = raw_clips
            .into_iter()
            .map(|clip| clip.expect("every clip window was extracted"))
            .collect();
        let decode_skips: Vec<u32> = raw_clips
            .iter()
            .map(|clip| clip.decode_error_skips)
            .collect();

        let decoded_extent = decoded_timeline_extent(&windows, &raw_clips);
        let end_clip_unreliable = windows.iter().zip(raw_clips.iter()).any(|(window, clip)| {
            window.label == ClipLabel::End
                && end_clip_extract_unreliable(
                    clip,
                    window,
                    alignment_config.min_end_clip_decode_fraction,
                    alignment_config.max_end_clip_decode_skips,
                )
        });

        Ok(ExtractedClips {
            raw_clips,
            decode_skips,
            windows,
            duration,
            decoded_extent,
            track: track.clone(),
            end_clip_unreliable,
        })
    }
}

struct AlignmentOutcome {
    result: AlignmentResult,
    track_a: AudioTrack,
    track_b: AudioTrack,
    discovery_windows: Vec<ClipWindow>,
    duration_a: std::time::Duration,
    duration_b: std::time::Duration,
    decoded_extent_a: std::time::Duration,
    decoded_extent_b: std::time::Duration,
}

fn is_skippable_prepare_error(result: &Result<MonoPcmClip, DomainError>) -> bool {
    matches!(
        result,
        Err(DomainError::InsufficientAudio) | Err(DomainError::EmptyClip)
    )
}

fn map_prepare_error(error: DomainError) -> AppError {
    match error {
        DomainError::InsufficientAudio | DomainError::EmptyClip => {
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
    decode_skips: Vec<u32>,
    windows: Vec<ClipWindow>,
    duration: std::time::Duration,
    decoded_extent: std::time::Duration,
    track: AudioTrack,
    end_clip_unreliable: bool,
}

fn log_alignment_summary(result: &AlignmentResult, progress: &dyn ProgressReporter) {
    progress.phase(&format!(
        "Start clip aligned: {}",
        yes_no(result.start_aligned)
    ));

    if let Some(end_aligned) = result.end_aligned {
        progress.phase(&format!("End clip aligned: {}", yes_no(end_aligned)));
    }

    if let Some(drift) = result.offset_drift_secs {
        if !result.offsets_consistent {
            progress.phase(&format!("Offset drift (end − start): {:+.3}s", drift));
        }
    }

    match result.recommended_offset_secs {
        Some(offset) => progress.phase(&format!(
            "Recommended offset: {:+.3}s ({})",
            offset,
            if result.offsets_consistent {
                "clip offsets agree"
            } else if result.start_aligned {
                "clip offsets disagree; using start clip"
            } else {
                "clip offsets disagree; using configured preference"
            }
        )),
        None => progress.phase("Recommended offset: none (no confident clip matches)"),
    }

    if let Some(refine) = &result.high_rate_refinement {
        if refine.applied {
            progress.phase(&format!(
                "High-rate refinement: {:+.3}s adjustment (peak {:.2})",
                refine.adjustment_secs, refine.correlation_peak
            ));
        } else if refine.skipped {
            progress.phase("High-rate refinement skipped");
        }
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
    use crate::application::config::{AlignConfig, AlignmentConfig, ClipConfig};
    use crate::application::error::{AlignmentError, AppError, ConfigError, FingerprintError, MediaError};
    use crate::application::testing::fakes::{
        FakeAligner, FakeFingerprinter, FakeMediaReader, FakeMediaSession, FakeProgressReporter,
    };
    use crate::domain::{ClipLabel, ClipMatchEstimate, DomainError};

    fn mins(m: u64) -> Duration {
        Duration::from_secs(m * 60)
    }

    fn two_clip_config() -> AlignConfig {
        AlignConfig {
            clip: ClipConfig {
                clip_length: mins(1),
                num_clips: 2,
                target_sample_rate: None,
                normalize_loudness: false,
                trim_silence: false,
                window_slide_secs: 0,
                ..ClipConfig::default()
            },
            alignment: AlignmentConfig {
                refine_offset_with_pcm: false,
                refine_offset_high_rate: false,
                ..Default::default()
            },
        }
    }

    fn request(config: AlignConfig) -> AlignVideosRequest {
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
        assert_eq!(seen.len(), 2, "end clip skips Chromaprint when refining around start");
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
        config.alignment.refine_end_clip_around_start_offset = false;

        let response = use_case
            .execute(request(config))
            .expect("execute should succeed");

        assert!(!response.result.offsets_consistent);
        assert_eq!(response.result.recommended_offset_secs, Some(10.0));
        assert_eq!(response.result.clips[0].label, ClipLabel::Start);
        assert_eq!(response.result.clips[1].label, ClipLabel::End);
    }

    #[test]
    fn execute_end_clip_uses_start_offset_when_constrained_refine_enabled() {
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

        let response = use_case
            .execute(request(two_clip_config()))
            .expect("execute should succeed");

        assert!(response.result.offsets_consistent);
        assert_eq!(response.result.clips[0].offset_secs, Some(10.0));
        assert_eq!(response.result.clips[1].offset_secs, Some(10.0));
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

        let config = AlignConfig {
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

    fn cross_layer_chirp_config(refine_pcm: bool, high_rate: bool) -> AlignConfig {
        AlignConfig {
            clip: ClipConfig {
                clip_length: Duration::from_secs(60),
                num_clips: 1,
                target_sample_rate: Some(11_025),
                normalize_loudness: false,
                trim_silence: false,
                window_slide_secs: 0,
                ..ClipConfig::default()
            },
            alignment: AlignmentConfig {
                refine_offset_with_pcm: refine_pcm,
                refine_offset_high_rate: high_rate,
                ..Default::default()
            },
        }
    }

    fn run_cross_layer_chirp_alignment_with(
        refine_pcm: bool,
        high_rate: bool,
    ) -> (f64, Option<crate::domain::HighRateRefinement>) {
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
                config: cross_layer_chirp_config(refine_pcm, high_rate),
            })
            .expect("execute should succeed");

        let offset = response
            .result
            .recommended_offset_secs
            .expect("expected aligned offset");
        (offset, response.result.high_rate_refinement)
    }

    #[test]
    fn cross_layer_high_rate_refine_tightens_wav_leader_3s() {
        let (offset, refine) = run_cross_layer_chirp_alignment_with(true, true);
        const OFFSET_SECS: f64 = 3.0;

        assert!((offset - OFFSET_SECS).abs() <= 0.050, "offset={offset}");
        let refine = refine.expect("high-rate refinement report");
        assert!(refine.applied, "refine={refine:?}");
    }

    /// With 11 kHz PCM refine already applied, high-rate may be a no-op on the synthetic chirp oracle.
    #[test]
    fn high_rate_refine_is_noop_when_pcm_refine_already_tight() {
        const EXPECTED_OFFSET: f64 = 3.0;
        const TIGHT_TOLERANCE: f64 = 0.050;

        let (offset_without, _) = run_cross_layer_chirp_alignment_with(true, false);
        let (offset_with, with_report) = run_cross_layer_chirp_alignment_with(true, true);

        let error_without = (offset_without - EXPECTED_OFFSET).abs();
        let error_with = (offset_with - EXPECTED_OFFSET).abs();

        assert!(error_without <= TIGHT_TOLERANCE);
        assert!(error_with <= TIGHT_TOLERANCE);
        assert!(
            (error_with - error_without).abs() < 0.005,
            "both paths should agree when PCM refine already tight: without={offset_without}, with={offset_with}"
        );
        let refine = with_report.expect("high-rate report");
        assert!(
            !refine.applied || refine.adjustment_secs.abs() < 0.005,
            "expected no meaningful adjustment, refine={refine:?}"
        );
    }

    /// Chromaprint-only leaves a measurable residual on the 44.1 kHz oracle; high-rate tightens it.
    #[test]
    fn high_rate_refine_tightens_when_pcm_refine_disabled() {
        const EXPECTED_OFFSET: f64 = 3.0;
        const TIGHT_TOLERANCE: f64 = 0.050;

        let (offset_without, without_report) = run_cross_layer_chirp_alignment_with(false, false);
        let (offset_with, with_report) = run_cross_layer_chirp_alignment_with(false, true);

        let error_without = (offset_without - EXPECTED_OFFSET).abs();
        let error_with = (offset_with - EXPECTED_OFFSET).abs();

        assert!(
            error_without > 0.010,
            "chromaprint-only should leave residual, offset={offset_without}, error={error_without}"
        );
        assert!(
            error_with <= TIGHT_TOLERANCE,
            "high-rate should tighten, offset={offset_with}, error={error_with}"
        );
        assert!(
            error_with < error_without,
            "without={offset_without} (err={error_without}), with={offset_with} (err={error_with})"
        );

        let refine = with_report.expect("high-rate report");
        assert!(refine.applied, "refine={refine:?}");
        assert!(without_report.is_none());
    }

    /// Documents the chromaprint-only residual band on the +3 s 44.1 kHz oracle (~29 ms).
    #[test]
    fn chromaprint_only_44k_chirp_leaves_known_residual_band() {
        const EXPECTED_OFFSET: f64 = 3.0;

        let (offset, _) = run_cross_layer_chirp_alignment_with(false, false);
        let error = (offset - EXPECTED_OFFSET).abs();
        assert!(
            (0.015..0.060).contains(&error),
            "expected chromaprint residual band, offset={offset}, error={error}"
        );
    }

    #[test]
    fn two_clip_end_refines_around_start_on_constant_offset_wav() {
        use crate::application::testing::audio_fixtures::write_offset_chirp_wav_pair;
        use crate::application::config::ChromaprintPreset;
        use crate::infrastructure::chromaprint::{ChromaprintAligner, ChromaprintFingerprinter};
        use crate::infrastructure::symphonia::SymphoniaMediaReader;

        const SAMPLE_RATE: u32 = 44_100;
        const TOTAL_SECS: u32 = 180;
        const CLIP_SECS: u64 = 60;
        const OFFSET_SECS: f64 = 3.0;

        let temp = tempfile::tempdir().expect("tempdir");
        let (path_a, path_b) = write_offset_chirp_wav_pair(
            temp.path(),
            SAMPLE_RATE,
            TOTAL_SECS,
            OFFSET_SECS as u32,
        );

        let config = AlignConfig {
            clip: ClipConfig {
                clip_length: Duration::from_secs(CLIP_SECS),
                num_clips: 2,
                target_sample_rate: Some(SAMPLE_RATE),
                normalize_loudness: false,
                trim_silence: false,
                window_slide_secs: 0,
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

        assert_eq!(response.result.clips.len(), 2);
        assert!(response.result.start_aligned);
        assert_eq!(response.result.end_aligned, Some(true));
        assert!(response.result.offsets_consistent);

        let start_offset = response.result.clips[0]
            .offset_secs
            .expect("start offset");
        let end_offset = response.result.clips[1]
            .offset_secs
            .expect("end offset");
        assert!(
            (start_offset - OFFSET_SECS).abs() < 1.0,
            "start_offset={start_offset}"
        );
        assert!(
            (end_offset - OFFSET_SECS).abs() < 1.0,
            "end_offset={end_offset}"
        );
        assert!(
            (end_offset - start_offset).abs() < 0.5,
            "start={start_offset}, end={end_offset}"
        );
    }
}
