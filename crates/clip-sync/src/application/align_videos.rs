use std::path::PathBuf;
use std::time::Duration;

use crate::application::config::AlignConfig;
use crate::application::error::{AppError, FingerprintError};
use crate::application::high_rate_refinement::{apply_high_rate_refinement, HighRateRefinementInput};
use crate::application::offset_verification::{apply_offset_verification, OffsetVerificationInput};
use crate::application::offset_refinement::{refine_offset_around_prior, refine_offset_estimate};
use crate::application::ports::MediaSession;
use crate::application::ports::{Aligner, Fingerprinter, MediaReader, ProgressReporter};
use crate::domain::{
    build_alignment_result, clip_windows_with_options, compute_clip_timeline_overlap,
    decoded_timeline_extent, end_clip_extract_unreliable, expand_window_for_slide,
    prepare_clip_for_fingerprint, resample_mono_pcm, select_aligned_subclip_pair,
    select_best_track, should_downgrade_repetition_confidence, truncate_padded_tail,
    AlignmentMergePolicy, AlignmentResult, AudioTrack,
    ClipLabel, ClipMatchEstimate, ClipPairReportInput, ClipPlanningOptions, ClipRepetitionReport,
    ClipWindow, DomainError, MediaSource, MonoPcmClip, OFFSET_AGREEMENT_TOLERANCE_SECS,
    PcmPreparationOptions, RepetitionFinding, TimelineOverlap,
};
use crate::infrastructure::chromaprint::repetition::detect_clip_repetition;
use crate::infrastructure::logging::ExtractionProgressScope;
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

        self.progress.phase_verbose("Opening media");
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

        apply_offset_verification(
            &OffsetVerificationInput {
                session_a: &session_a,
                session_b: &session_b,
                track_a: &outcome.track_a,
                track_b: &outcome.track_b,
                discovery_windows: &outcome.discovery_windows,
                duration_a: outcome.duration_a,
                duration_b: outcome.duration_b,
                decoded_extent_a: outcome.decoded_extent_a,
                decoded_extent_b: outcome.decoded_extent_b,
                min_holdout_decode_fraction: request.config.alignment.min_end_clip_decode_fraction,
                max_holdout_decode_skips: request.config.alignment.max_end_clip_decode_skips,
            },
            &request.config.clip,
            &request.config.validation,
            &mut result,
            self.fingerprinter,
            self.aligner,
            self.progress,
        );

        log_alignment_summary(
            &result,
            Some(outcome.duration_a),
            Some(outcome.duration_b),
            self.progress,
        );

        Ok(AlignVideosResponse { result })
    }

    fn align_single_track_pair(
        &self,
        session_a: &MR::Session,
        session_b: &MR::Session,
        request: &AlignVideosRequest,
    ) -> Result<AlignmentOutcome, AppError> {
        let plan = request.config.clip.as_plan();
        let extraction = ExtractionProgressScope::new(self.progress);
        let extracted_a = self.extract_clips(
            session_a,
            &plan,
            &request.config.clip,
            &request.config.alignment,
            "video A",
            None,
            &extraction,
        )?;
        let extracted_b = self.extract_clips(
            session_b,
            &plan,
            &request.config.clip,
            &request.config.alignment,
            "video B",
            None,
            &extraction,
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

        // Disable repetition during the track search to avoid running it for every track pair.
        // The winning pair gets a dedicated repetition pass below.
        let search_config = if request.config.validation.check_clip_repetition {
            let mut c = request.config.clone();
            c.validation.check_clip_repetition = false;
            c
        } else {
            request.config.clone()
        };

        let mut best: Option<(AlignmentOutcome, ExtractedClips, ExtractedClips, f32)> = None;
        let extraction = ExtractionProgressScope::new(self.progress);

        for track_a in &decodable_a {
            for track_b in &decodable_b {
                self.progress.phase_verbose(&format!(
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
                    &extraction,
                )?;
                let extracted_b = self.extract_clips(
                    session_b,
                    &plan,
                    &request.config.clip,
                    &request.config.alignment,
                    "video B",
                    Some(track_b),
                    &extraction,
                )?;
                let result =
                    self.align_extracted_pair(&extracted_a, &extracted_b, &search_config)?;
                let score =
                    mean_aligned_confidence(&result, request.config.alignment.min_match_score);
                let outcome = AlignmentOutcome {
                    result,
                    track_a: extracted_a.track.clone(),
                    track_b: extracted_b.track.clone(),
                    discovery_windows: extracted_a.windows.clone(),
                    duration_a: extracted_a.duration,
                    duration_b: extracted_b.duration,
                    decoded_extent_a: extracted_a.decoded_extent,
                    decoded_extent_b: extracted_b.decoded_extent,
                };
                if best.as_ref().is_none_or(|(_, _, _, best_score)| score > *best_score) {
                    best = Some((outcome, extracted_a, extracted_b, score));
                }
            }
        }

        let (mut outcome, winning_a, winning_b, _) = best.ok_or_else(|| {
            AppError::Alignment(crate::application::error::AlignmentError::EngineFailed(
                "no track pair produced an alignment".into(),
            ))
        })?;

        // Run repetition check on the winning pair only.
        if request.config.validation.check_clip_repetition {
            let rep_result =
                self.align_extracted_pair(&winning_a, &winning_b, &request.config)?;
            for (clip, new_clip) in outcome.result.clips.iter_mut().zip(rep_result.clips) {
                clip.repetition = new_clip.repetition;
            }
        }

        Ok(outcome)
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
        let mut repetition_diagnostics: Vec<(Option<RepetitionFinding>, Option<RepetitionFinding>)> =
            Vec::with_capacity(extracted_a.raw_clips.len());
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

            let source_duration_a = clip_a.duration_secs();
            let source_duration_b = clip_b.duration_secs();
            let prepared_a = prepare_clip_for_fingerprint(&clip_a, prep_options);
            let prepared_b = prepare_clip_for_fingerprint(&clip_b, prep_options);

            if is_skippable_prepare_error(&prepared_a) || is_skippable_prepare_error(&prepared_b)
            {
                self.progress.phase_verbose(&format!(
                    "{} clip [{}–{}]: skipped (insufficient audio)",
                    clip_label_name(window.label),
                    format_duration(window.start),
                    format_duration(window.end),
                ));
                estimates.push(ClipMatchEstimate {
                    offset_secs: 0.0,
                    confidence: 0.0,
                });
                repetition_diagnostics.push((None, None));
                continue;
            }

            if end_clip_unreliable && config.alignment.skip_unreliable_end_clip {
                self.progress.phase_verbose(&format!(
                    "end clip [{}–{}]: skipped (unreliable tail extract)",
                    format_duration(window.start),
                    format_duration(window.end),
                ));
                estimates.push(ClipMatchEstimate {
                    offset_secs: 0.0,
                    confidence: 0.0,
                });
                repetition_diagnostics.push((None, None));
                continue;
            }

            let clip_a = prepared_a.map_err(map_prepare_error)?;
            let clip_b = prepared_b.map_err(map_prepare_error)?;

            let end_refine_candidate = window.label == ClipLabel::End
                && config.alignment.refine_end_clip_around_start_offset
                && start_prior.is_some_and(|prior| {
                    prior.confidence >= config.alignment.min_match_score * 0.5
                });

            // End-prior refinement must compare against an independent end estimate so
            // disagreeing windows (e.g. trailing silence on one file) stay visible.
            let fp_a = self.fingerprinter.fingerprint(&clip_a)?;
            let fp_b = self.fingerprinter.fingerprint(&clip_b)?;
            let fingerprints = (fp_a, fp_b);

            if config.validation.check_clip_repetition {
                let (fp_a, fp_b) = &fingerprints;
                let preset = config.clip.chromaprint_preset;
                let min_conf = config.validation.min_repetition_confidence;
                let rep_a = detect_clip_repetition(
                    fp_a,
                    clip_a.duration_secs(),
                    preset,
                    min_conf,
                    source_duration_a,
                );
                let rep_b = detect_clip_repetition(
                    fp_b,
                    clip_b.duration_secs(),
                    preset,
                    min_conf,
                    source_duration_b,
                );
                tracing::debug!(?rep_a, ?rep_b, "clip self-repetition check");
                repetition_diagnostics.push((rep_a, rep_b));
            } else {
                repetition_diagnostics.push((None, None));
            }

            let (refined_around_start, estimate) = if end_refine_candidate {
                let prior = start_prior.expect("checked above");
                let (fp_a, fp_b) = &fingerprints;
                let mut independent = self.aligner.find_offset(fp_a, fp_b)?;
                if config.alignment.refine_offset_with_pcm
                    && independent.confidence >= config.alignment.min_match_score * 0.5
                {
                    independent = refine_offset_estimate(&clip_a, &clip_b, independent);
                }

                let agree = (prior.offset_secs - independent.offset_secs).abs()
                    <= OFFSET_AGREEMENT_TOLERANCE_SECS;
                if agree {
                    let refined = if config.alignment.refine_offset_with_pcm {
                        refine_offset_around_prior(
                            &clip_a,
                            &clip_b,
                            prior,
                            config.alignment.end_clip_refine_radius_secs,
                        )
                    } else {
                        prior
                    };
                    (true, refined)
                } else {
                    (false, independent)
                }
            } else {
                let (fp_a, fp_b) = &fingerprints;
                let mut chromaprint_estimate = self.aligner.find_offset(fp_a, fp_b)?;
                if config.alignment.refine_offset_with_pcm
                    && chromaprint_estimate.confidence >= config.alignment.min_match_score * 0.5
                {
                    chromaprint_estimate =
                        refine_offset_estimate(&clip_a, &clip_b, chromaprint_estimate);
                }
                (false, chromaprint_estimate)
            };

            if window.label == ClipLabel::Start && estimate.confidence > 0.0 {
                start_prior = Some(estimate);
            }

            self.progress.phase_verbose(&format!(
                "{} clip [{}–{}]{}: {} (confidence: {:.2})",
                clip_label_name(window.label),
                format_duration(window.start),
                format_duration(window.end),
                if refined_around_start {
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

        let mut result = build_alignment_result(
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
        );

        if config.validation.check_clip_repetition {
            debug_assert_eq!(
                result.clips.len(),
                repetition_diagnostics.len(),
                "repetition_diagnostics must be parallel to result.clips"
            );
            for (i, clip) in result.clips.iter_mut().enumerate() {
                let (rep_a, rep_b) = repetition_diagnostics[i]; // Copy
                if should_downgrade_repetition_confidence(&rep_a, &rep_b, estimates[i].offset_secs) {
                    clip.confidence *= 0.5;
                }
                clip.repetition = Some(ClipRepetitionReport { a: rep_a, b: rep_b });
            }
        }

        Ok(result)
    }

    fn extract_clips(
        &self,
        session: &MR::Session,
        plan: &crate::domain::ClipPlan,
        clip_config: &crate::application::config::ClipConfig,
        alignment_config: &crate::application::config::AlignmentConfig,
        label: &str,
        track: Option<&AudioTrack>,
        extraction: &ExtractionProgressScope<'_>,
    ) -> Result<ExtractedClips, AppError> {
        let tracks = session.list_tracks()?;
        let track = match track {
            Some(track) => track,
            None => select_best_track(&tracks)?,
        };
        self.progress.phase_verbose(&format!(
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
                    self.progress.phase_verbose(&format!(
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
                self.progress.phase_verbose(&format!(
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

        self.progress.phase_verbose(&format_clip_plan(label, &windows));

        extraction.register_batch(windows.len() as u64);

        let mut extract_order: Vec<usize> = (0..windows.len()).collect();
        if windows.len() > 1 {
            extract_order.sort_by_key(|&index| windows[index].start);
            let chronological: Vec<usize> = (0..windows.len()).collect();
            if extract_order != chronological {
                self.progress.phase_verbose(&format!(
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
            let clip_progress = extraction.for_clip(step as u64);
            let mut clip = session.extract_mono(
                track,
                &extract_window,
                &clip_progress,
                &progress_label,
            )?;
            if let Some(target_rate) = clip_config.target_sample_rate {
                clip = resample_mono_pcm(&clip, target_rate);
            }
            raw_clips[index] = Some(clip);
        }

        extraction.finish_batch(windows.len() as u64);

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

#[derive(Clone)]
struct ExtractedClips {
    raw_clips: Vec<crate::domain::MonoPcmClip>,
    decode_skips: Vec<u32>,
    windows: Vec<ClipWindow>,
    duration: std::time::Duration,
    decoded_extent: std::time::Duration,
    track: AudioTrack,
    end_clip_unreliable: bool,
}

fn log_alignment_summary(
    result: &AlignmentResult,
    duration_a: Option<Duration>,
    duration_b: Option<Duration>,
    progress: &dyn ProgressReporter,
) {
    progress.phase_verbose(&format!(
        "Start clip aligned: {}",
        yes_no(result.start_aligned)
    ));

    if let Some(end_aligned) = result.end_aligned {
        progress.phase_verbose(&format!("End clip aligned: {}", yes_no(end_aligned)));
    }

    if let Some(drift) = result.offset_drift_secs {
        if !result.offsets_consistent {
            progress.phase_verbose(&format!("Offset drift (end − start): {:+.3}s", drift));
        }
    }

    match result.recommended_offset_secs {
        Some(offset) => progress.phase_verbose(&format!(
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
        None => progress.phase_verbose("Recommended offset: none (no confident clip matches)"),
    }

    if let Some(refine) = &result.high_rate_refinement {
        if refine.applied {
            progress.phase_verbose(&format!(
                "High-rate refinement: {:+.3}s adjustment",
                refine.adjustment_secs
            ));
        } else if refine.skipped {
            progress.phase_verbose("High-rate refinement skipped");
        }
    }

    if let Some(verify) = &result.offset_verification {
        if verify.skipped {
            if let Some(reason) = &verify.skip_reason {
                progress.phase_verbose(&format!("Offset verification skipped: {reason}"));
            } else {
                progress.phase_verbose("Offset verification skipped");
            }
        } else if verify.verified {
            progress.phase_verbose(&format!(
                "Offset verification: confirmed (confidence {:.2})",
                verify.confidence
            ));
        } else {
            progress.phase_verbose(&format!(
                "Offset verification: not confirmed (confidence {:.2})",
                verify.confidence
            ));
        }
    }

    let clip_overlaps: Vec<TimelineOverlap> = result
        .clips
        .iter()
        .filter_map(|clip| compute_clip_timeline_overlap(clip, duration_a, duration_b))
        .collect();

    if !clip_overlaps.is_empty() {
        progress.phase_verbose(&format!(
            "Overlap on video A: {}",
            clip_overlaps
                .iter()
                .map(|overlap| {
                    format_overlap_window(overlap.video_a_start_secs, overlap.video_a_end_secs)
                })
                .collect::<Vec<_>>()
                .join(", ")
        ));
        progress.phase_verbose(&format!(
            "Overlap on video B: {}",
            clip_overlaps
                .iter()
                .map(|overlap| {
                    format_overlap_window(overlap.video_b_start_secs, overlap.video_b_end_secs)
                })
                .collect::<Vec<_>>()
                .join(", ")
        ));
        progress.phase_verbose(&format!(
            "Shared length: {}",
            clip_overlaps
                .iter()
                .map(|overlap| crate::domain::format_timestamp(overlap.shared_length_secs))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
}

fn format_overlap_window(start_secs: f64, end_secs: f64) -> String {
    format!(
        "[{}–{}]",
        crate::domain::format_timestamp(start_secs),
        crate::domain::format_timestamp(end_secs)
    )
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
    use crate::domain::{
        ClipLabel, ClipMatch, ClipMatchEstimate, ClipRepetitionReport, DomainError,
    };
    use crate::infrastructure::chromaprint::repetition::{
        test_reset_repetition_detect_calls, test_repetition_detect_calls,
    };

    /// Pure-tone repeat fixtures target ~30s; integration/corpus use this band for Chromaprint lag
    /// quantization (unit tests use tighter ±1–2s where prep is controlled).
    const PURE_TONE_REPEAT_LAG_SECS: f64 = 30.0;
    const PURE_TONE_REPEAT_LAG_TOLERANCE_SECS: f64 = 2.0;
    const FAKE_REPETITION_MATCH_CONFIDENCE: f32 = 0.9;

    fn assert_repetition_wrapper_without_findings(clip: &ClipMatch) {
        let report = clip
            .repetition
            .as_ref()
            .expect("repetition wrapper must be present when flag is on");
        assert!(
            report.a.is_none() && report.b.is_none(),
            "fake single-item fingerprints must not produce repetition findings"
        );
    }

    fn assert_repetition_lag_near_secs(report: &ClipRepetitionReport, expected_secs: f64) {
        let finding = report
            .a
            .as_ref()
            .or(report.b.as_ref())
            .expect("at least one repetition finding");
        assert!(
            (finding.lag_secs - expected_secs).abs() <= PURE_TONE_REPEAT_LAG_TOLERANCE_SECS,
            "lag_secs={} expected within ±{PURE_TONE_REPEAT_LAG_TOLERANCE_SECS}s of {expected_secs}",
            finding.lag_secs
        );
    }

    /// Downgrade tests: assert the side whose lag is within ±1 s of |offset|, not whichever
    /// finding `a.or(b)` returns first (offset-shifted clips can spuriously match on one side).
    fn assert_downgrade_trigger_lag_near_secs(
        report: &ClipRepetitionReport,
        offset_secs: f64,
        expected_secs: f64,
    ) {
        let close = |rep: &RepetitionFinding| {
            (rep.lag_secs - offset_secs.abs()).abs() <= 1.0
        };
        let matching = report
            .a
            .as_ref()
            .filter(|f| close(f))
            .or_else(|| report.b.as_ref().filter(|f| close(f)))
            .expect("a finding within ±1 s of |offset| must exist");
        assert!(
            (matching.lag_secs - expected_secs).abs() <= PURE_TONE_REPEAT_LAG_TOLERANCE_SECS,
            "downgrade-trigger lag_secs={} expected within ±{PURE_TONE_REPEAT_LAG_TOLERANCE_SECS}s of {expected_secs}",
            matching.lag_secs
        );
    }

    fn assert_clips_keep_aligner_confidence(clips: &[ClipMatch], expected: f32) {
        for clip in clips {
            assert!(
                (clip.confidence - expected).abs() < 0.001,
                "repetition pass must not change aligner confidence: got {} expected {expected}",
                clip.confidence
            );
        }
    }

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
            ..Default::default()
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

    fn execute_fake_repetition_case(
        config: AlignConfig,
        offset_secs: f64,
    ) -> AlignVideosResponse {
        let reader = matched_reader();
        let fingerprinter = FakeFingerprinter::new();
        let aligner = FakeAligner::with_estimate(ClipMatchEstimate {
            offset_secs,
            confidence: FAKE_REPETITION_MATCH_CONFIDENCE,
        });
        let progress = FakeProgressReporter;
        let use_case = AlignVideos::new(&reader, &fingerprinter, &aligner, &progress);
        use_case
            .execute(AlignVideosRequest {
                video_a: PathBuf::from("a.wav"),
                video_b: PathBuf::from("b.wav"),
                config,
            })
            .expect("fake repetition execute")
    }

    fn pure_tone_downgrade_config(clip_secs: u64) -> AlignConfig {
        AlignConfig {
            clip: ClipConfig {
                clip_length: Duration::from_secs(clip_secs),
                num_clips: 1,
                target_sample_rate: Some(44_100),
                normalize_loudness: true,
                trim_silence: false,
                window_slide_secs: 0,
                ..ClipConfig::default()
            },
            alignment: AlignmentConfig {
                refine_offset_with_pcm: false,
                refine_offset_high_rate: false,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn pure_tone_downgrade_wav_pair(temp: &tempfile::TempDir) -> (PathBuf, PathBuf) {
        use crate::application::testing::audio_fixtures::write_pure_tone_repeat_wav_pair;
        write_pure_tone_repeat_wav_pair(temp.path(), 44_100, 130, 30)
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
        assert_eq!(
            seen.len(),
            4,
            "start and end clips each fingerprint A and B at the target rate"
        );
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
    fn execute_end_clip_uses_start_offset_when_independent_end_agrees() {
        let reader = matched_reader();
        let fingerprinter = FakeFingerprinter::new();
        let aligner = FakeAligner::with_estimates(vec![
            ClipMatchEstimate {
                offset_secs: 10.0,
                confidence: 0.9,
            },
            ClipMatchEstimate {
                offset_secs: 10.1,
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
    fn execute_end_clip_keeps_independent_offset_when_it_disagrees_with_start() {
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

        assert!(!response.result.offsets_consistent);
        assert_eq!(response.result.clips[0].offset_secs, Some(10.0));
        assert_eq!(response.result.clips[1].offset_secs, Some(20.0));
        assert_eq!(response.result.recommended_offset_secs, None);
    }

    #[test]
    fn execute_runs_offset_verification_when_flag_on() {
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

        let mut config = AlignConfig {
            clip: ClipConfig {
                clip_length: Duration::from_secs(60),
                num_clips: 1,
                target_sample_rate: Some(SAMPLE_RATE),
                normalize_loudness: false,
                trim_silence: false,
                ..ClipConfig::default()
            },
            ..Default::default()
        };
        config.validation.verify_offset = true;

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
        assert!((offset - f64::from(OFFSET_SECS)).abs() < 1.0, "offset={offset}");

        let verify = response
            .result
            .offset_verification
            .expect("verification must run when flag on");
        assert!(!verify.skipped, "skip_reason={:?}", verify.skip_reason);
        assert!(verify.verified, "confidence={}", verify.confidence);
        assert!(verify.confidence >= 0.5);
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
            ..Default::default()
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

    #[test]
    fn repetition_detect_skipped_when_flag_off() {
        let reader = matched_reader();
        let fingerprinter = FakeFingerprinter::new();
        let aligner = FakeAligner::with_estimate(ClipMatchEstimate {
            offset_secs: 3.0,
            confidence: FAKE_REPETITION_MATCH_CONFIDENCE,
        });
        let progress = FakeProgressReporter;
        let use_case = AlignVideos::new(&reader, &fingerprinter, &aligner, &progress);

        test_reset_repetition_detect_calls();
        use_case
            .execute(request(two_clip_config()))
            .expect("execute should succeed");
        assert_eq!(
            test_repetition_detect_calls(),
            0,
            "detect_clip_repetition must not run when check_clip_repetition is off"
        );
    }

    #[test]
    fn fake_repetition_wiring_when_flag_on() {
        let mut config = two_clip_config();
        config.validation.check_clip_repetition = true;

        let response = execute_fake_repetition_case(config, 3.0);

        assert!(response.result.start_aligned);
        assert_clips_keep_aligner_confidence(
            &response.result.clips,
            FAKE_REPETITION_MATCH_CONFIDENCE,
        );
        for clip in &response.result.clips {
            assert_repetition_wrapper_without_findings(clip);
        }

        let json = serde_json::to_string(&response.result).expect("serialize");
        let value: serde_json::Value = serde_json::from_str(&json).expect("parse");
        for clip in value["clips"].as_array().unwrap() {
            let repetition = &clip["repetition"];
            assert!(repetition.is_object(), "repetition must be an object when flag on");
            assert!(
                repetition["a"].is_null() && repetition["b"].is_null(),
                "fake fingerprints must serialize null findings"
            );
        }
    }

    #[test]
    fn align_json_no_repetition_key_when_flag_off() {
        let reader = matched_reader();
        let fingerprinter = FakeFingerprinter::new();
        let aligner = FakeAligner::with_estimate(ClipMatchEstimate {
            offset_secs: 3.0,
            confidence: 0.9,
        });
        let progress = FakeProgressReporter;
        let use_case = AlignVideos::new(&reader, &fingerprinter, &aligner, &progress);

        let response = use_case
            .execute(request(two_clip_config()))
            .expect("execute should succeed");

        let json = serde_json::to_string(&response.result).expect("serialize");
        let value: serde_json::Value = serde_json::from_str(&json).expect("parse");

        for clip in value["clips"].as_array().unwrap() {
            assert!(
                clip.get("repetition").is_none(),
                "repetition key must be absent when flag off"
            );
        }
    }

    #[test]
    fn try_all_tracks_repetition_on_winner_only() {
        // Two decodable tracks on each video → 4 pairs in the search loop.
        // Repetition should run only for the winning pair.
        let two_tracks = |duration: std::time::Duration| {
            FakeMediaSession::with_tracks(vec![
                crate::domain::AudioTrack {
                    index: 0,
                    codec: "pcm".into(),
                    channels: 1,
                    sample_rate: 44_100,
                    duration: Some(duration),
                    decodable: true,
                },
                crate::domain::AudioTrack {
                    index: 1,
                    codec: "pcm".into(),
                    channels: 1,
                    sample_rate: 44_100,
                    duration: Some(duration),
                    decodable: true,
                },
            ])
        };
        let reader = FakeMediaReader::new()
            .with_session("a.wav", two_tracks(mins(3)))
            .with_session("b.wav", two_tracks(mins(3)));
        let fingerprinter = FakeFingerprinter::new();
        let aligner = FakeAligner::with_estimate(ClipMatchEstimate {
            offset_secs: 5.0,
            confidence: FAKE_REPETITION_MATCH_CONFIDENCE,
        });
        let progress = FakeProgressReporter;
        let use_case = AlignVideos::new(&reader, &fingerprinter, &aligner, &progress);

        let mut config = two_clip_config();
        config.alignment.try_all_tracks = true;
        config.validation.check_clip_repetition = true;

        test_reset_repetition_detect_calls();
        let response = use_case
            .execute(request(config))
            .expect("try_all_tracks with repetition should succeed");

        assert_clips_keep_aligner_confidence(
            &response.result.clips,
            FAKE_REPETITION_MATCH_CONFIDENCE,
        );
        for clip in &response.result.clips {
            assert_repetition_wrapper_without_findings(clip);
        }
        assert_eq!(
            test_repetition_detect_calls(),
            4,
            "repetition detect must run once on the winning pair only (2 clips × a/b)"
        );
    }

    #[test]
    fn should_downgrade_cases() {
        use crate::domain::should_downgrade_repetition_confidence;

        let finding = |lag_secs: f64| RepetitionFinding {
            lag_secs,
            confidence: 0.9,
            items_count: 100,
        };

        let cases = [
            (Some(finding(30.0)), None, 30.0, true, "exact lag match on a"),
            (None, Some(finding(30.0)), -30.0, true, "exact lag match on b with negative offset"),
            (Some(finding(45.0)), None, 30.0, false, "lag differs from offset"),
            (None, None, 30.0, false, "no findings"),
            (Some(finding(29.0)), None, 30.0, true, "lower 1 s boundary"),
            (Some(finding(28.9)), None, 30.0, false, "beyond lower 1 s boundary"),
            (Some(finding(31.0)), None, 30.0, true, "upper 1 s boundary"),
            (
                Some(finding(30.5)),
                Some(RepetitionFinding {
                    lag_secs: 29.5,
                    confidence: 0.85,
                    items_count: 90,
                }),
                30.0,
                true,
                "either side within ±1 s of |offset|",
            ),
        ];

        for (rep_a, rep_b, offset, expect, label) in cases {
            assert_eq!(
                should_downgrade_repetition_confidence(&rep_a, &rep_b, offset),
                expect,
                "{label}"
            );
        }
    }

    #[test]
    fn repetition_on_skipped_clip_has_null_findings() {
        let reader = FakeMediaReader::new()
            .with_session("a.wav", FakeMediaSession::with_duration(mins(3)))
            .with_session(
                "b.wav",
                FakeMediaSession::with_duration(mins(3)).with_silent_extract(),
            );
        let fingerprinter = FakeFingerprinter::new();
        let aligner = FakeAligner::with_estimate(ClipMatchEstimate {
            offset_secs: 3.0,
            confidence: FAKE_REPETITION_MATCH_CONFIDENCE,
        });
        let progress = FakeProgressReporter;
        let use_case = AlignVideos::new(&reader, &fingerprinter, &aligner, &progress);

        let mut config = two_clip_config();
        config.validation.check_clip_repetition = true;

        test_reset_repetition_detect_calls();
        let response = use_case
            .execute(request(config))
            .expect("execute should succeed with skipped clips");

        for clip in &response.result.clips {
            assert!(!clip.aligned, "insufficient-audio skip must not align");
            assert_repetition_wrapper_without_findings(clip);
        }
        assert_eq!(
            test_repetition_detect_calls(),
            0,
            "detect_clip_repetition must not run when fingerprint prep is skipped"
        );
    }

    #[test]
    fn repetition_reported_on_pure_tone_fixture() {
        use crate::application::config::ChromaprintPreset;
        use crate::application::testing::audio_fixtures::write_pure_tone_repeat_wav_pair;
        use crate::infrastructure::chromaprint::{ChromaprintAligner, ChromaprintFingerprinter};
        use crate::infrastructure::symphonia::SymphoniaMediaReader;

        const SAMPLE_RATE: u32 = 44_100;

        let temp = tempfile::tempdir().expect("tempdir");
        let (path_a, path_b) = write_pure_tone_repeat_wav_pair(temp.path(), SAMPLE_RATE, 65, 0);

        let base_config = AlignConfig {
            clip: ClipConfig {
                clip_length: Duration::from_secs(60),
                num_clips: 1,
                target_sample_rate: Some(SAMPLE_RATE),
                normalize_loudness: true,
                trim_silence: false,
                window_slide_secs: 0,
                ..ClipConfig::default()
            },
            alignment: AlignmentConfig {
                refine_offset_with_pcm: false,
                refine_offset_high_rate: false,
                ..Default::default()
            },
            ..Default::default()
        };

        let media_reader = SymphoniaMediaReader;
        let preset = ChromaprintPreset::default();
        let fingerprinter = ChromaprintFingerprinter::new(preset);
        let aligner = ChromaprintAligner::new(preset);
        let progress = FakeProgressReporter;
        let use_case = AlignVideos::new(&media_reader, &fingerprinter, &aligner, &progress);

        let request = |config: AlignConfig| AlignVideosRequest {
            video_a: path_a.clone(),
            video_b: path_b.clone(),
            config,
        };

        let response_base = use_case
            .execute(request(base_config.clone()))
            .expect("baseline execute");

        let mut config_rep = base_config;
        config_rep.validation.check_clip_repetition = true;

        let response_rep = use_case
            .execute(request(config_rep))
            .expect("repetition execute");

        assert!(response_rep.result.start_aligned);
        let offset = response_rep
            .result
            .recommended_offset_secs
            .unwrap_or(0.0);
        assert!(offset.abs() < 0.5, "offset={offset}");

        let clip = &response_rep.result.clips[0];
        let report = clip
            .repetition
            .as_ref()
            .expect("repetition report");
        assert_repetition_lag_near_secs(report, PURE_TONE_REPEAT_LAG_SECS);
        let finding = report
            .a
            .as_ref()
            .or(report.b.as_ref())
            .expect("repetition finding on a or b");
        assert!(finding.confidence >= 0.5);
        // Repeat lag (~30 s) differs from offset (~0 s) — downgrade must not apply in the pipeline.
        assert!(
            !should_downgrade_repetition_confidence(&report.a, &report.b, clip.offset_secs.unwrap_or(0.0)),
            "offset-aligned pair should not trigger confidence downgrade"
        );
        assert!(
            (clip.confidence - response_base.result.clips[0].confidence).abs() < 0.01,
            "confidence must be unchanged when downgrade does not apply: base={}, with_rep={}",
            response_base.result.clips[0].confidence,
            clip.confidence
        );
    }

    #[test]
    fn min_repetition_confidence_rejects_findings_in_pipeline() {
        use crate::application::config::ChromaprintPreset;
        use crate::application::testing::audio_fixtures::write_pure_tone_repeat_wav_pair;
        use crate::infrastructure::chromaprint::{ChromaprintAligner, ChromaprintFingerprinter};
        use crate::infrastructure::symphonia::SymphoniaMediaReader;

        const SAMPLE_RATE: u32 = 44_100;

        let temp = tempfile::tempdir().expect("tempdir");
        let (path_a, path_b) = write_pure_tone_repeat_wav_pair(temp.path(), SAMPLE_RATE, 65, 0);

        let mut base_config = AlignConfig {
            clip: ClipConfig {
                clip_length: Duration::from_secs(60),
                num_clips: 1,
                target_sample_rate: Some(SAMPLE_RATE),
                normalize_loudness: true,
                trim_silence: false,
                window_slide_secs: 0,
                ..ClipConfig::default()
            },
            alignment: AlignmentConfig {
                refine_offset_with_pcm: false,
                refine_offset_high_rate: false,
                ..Default::default()
            },
            ..Default::default()
        };
        base_config.validation.check_clip_repetition = true;
        base_config.validation.min_repetition_confidence = 0.0;

        let media_reader = SymphoniaMediaReader;
        let preset = ChromaprintPreset::default();
        let fingerprinter = ChromaprintFingerprinter::new(preset);
        let aligner = ChromaprintAligner::new(preset);
        let progress = FakeProgressReporter;
        let use_case = AlignVideos::new(&media_reader, &fingerprinter, &aligner, &progress);

        let request = |config: AlignConfig| AlignVideosRequest {
            video_a: path_a.clone(),
            video_b: path_b.clone(),
            config,
        };

        let response_low = use_case
            .execute(request(base_config))
            .expect("low-threshold execute");

        let report_low = response_low.result.clips[0]
            .repetition
            .as_ref()
            .expect("repetition wrapper");
        let finding = report_low
            .a
            .as_ref()
            .or(report_low.b.as_ref())
            .expect("fixture must produce a repetition finding at min_confidence=0");
        let reject_threshold = finding.confidence.next_up();

        let config_high = AlignConfig {
            clip: ClipConfig {
                clip_length: Duration::from_secs(60),
                num_clips: 1,
                target_sample_rate: Some(SAMPLE_RATE),
                normalize_loudness: true,
                trim_silence: false,
                window_slide_secs: 0,
                ..ClipConfig::default()
            },
            alignment: AlignmentConfig {
                refine_offset_with_pcm: false,
                refine_offset_high_rate: false,
                ..Default::default()
            },
            validation: crate::application::config::ValidationConfig {
                check_clip_repetition: true,
                min_repetition_confidence: reject_threshold,
                ..Default::default()
            },
            ..Default::default()
        };

        let response_high = use_case
            .execute(request(config_high))
            .expect("high-threshold execute");

        let clip = &response_high.result.clips[0];
        assert!(
            clip.repetition.is_some(),
            "repetition wrapper must be present even when findings are gated out"
        );
        assert_repetition_wrapper_without_findings(clip);
    }

    #[test]
    fn downgrade_halves_confidence_when_lag_matches_offset() {
        use crate::application::config::ChromaprintPreset;
        use crate::infrastructure::chromaprint::ChromaprintFingerprinter;
        use crate::infrastructure::symphonia::SymphoniaMediaReader;

        // Pure tone repeat at ~30 s with inter-file offset +30 s. Cross-file offset is injected
        // via FakeAligner (pure-tone pairs do not yield a reliable +30 s Chromaprint match);
        // repetition detection and downgrade merge use the real fingerprinter and pipeline.
        const CLIP_SECS: u64 = 75;
        const ALIGN_OFFSET: f64 = 30.0;

        let temp = tempfile::tempdir().expect("tempdir");
        let (path_a, path_b) = pure_tone_downgrade_wav_pair(&temp);
        let base_config = pure_tone_downgrade_config(CLIP_SECS);

        let media_reader = SymphoniaMediaReader;
        let preset = ChromaprintPreset::default();
        let fingerprinter = ChromaprintFingerprinter::new(preset);
        let aligner = FakeAligner::with_estimate(ClipMatchEstimate {
            offset_secs: ALIGN_OFFSET,
            confidence: 0.95,
        });
        let progress = FakeProgressReporter;
        let use_case = AlignVideos::new(&media_reader, &fingerprinter, &aligner, &progress);

        let response_base = use_case
            .execute(AlignVideosRequest {
                video_a: path_a.clone(),
                video_b: path_b.clone(),
                config: base_config.clone(),
            })
            .expect("baseline execute");

        assert!(response_base.result.start_aligned, "expected aligned at +30s");
        let base_offset = response_base.result.clips[0]
            .offset_secs
            .expect("offset");
        assert!(
            (base_offset - ALIGN_OFFSET).abs() < 0.01,
            "offset={base_offset}"
        );
        let base_confidence = response_base.result.clips[0].confidence;

        let mut config_rep = base_config;
        config_rep.validation.check_clip_repetition = true;

        let response_rep = use_case
            .execute(AlignVideosRequest {
                video_a: path_a,
                video_b: path_b,
                config: config_rep,
            })
            .expect("repetition execute");

        let clip = &response_rep.result.clips[0];
        let report = clip.repetition.as_ref().expect("repetition report");
        let offset = clip.offset_secs.expect("offset");
        assert!(
            should_downgrade_repetition_confidence(&report.a, &report.b, offset),
            "repeat lag must be within ±1 s of offset for downgrade: {:?} offset={offset}",
            report
        );
        assert_downgrade_trigger_lag_near_secs(report, offset, PURE_TONE_REPEAT_LAG_SECS);
        assert!(
            response_rep.result.start_aligned,
            "start_aligned must not flip after confidence downgrade"
        );
        assert!(clip.aligned, "clip.aligned must not flip after confidence downgrade");
        assert!(
            clip.confidence < base_confidence,
            "confidence must be downgraded: base={base_confidence}, after={}",
            clip.confidence
        );
        assert!(
            (clip.confidence - base_confidence * 0.5).abs() < 0.01,
            "confidence must be approximately halved: base={base_confidence}, after={}",
            clip.confidence
        );
    }

    #[test]
    fn downgrade_preserves_aligned_when_halved_below_min_match_score() {
        use crate::application::config::ChromaprintPreset;
        use crate::infrastructure::chromaprint::ChromaprintFingerprinter;
        use crate::infrastructure::symphonia::SymphoniaMediaReader;

        const CLIP_SECS: u64 = 75;
        const ALIGN_OFFSET: f64 = 30.0;
        const BASE_CONFIDENCE: f32 = 0.55;

        let temp = tempfile::tempdir().expect("tempdir");
        let (path_a, path_b) = pure_tone_downgrade_wav_pair(&temp);

        let mut config = pure_tone_downgrade_config(CLIP_SECS);
        config.validation.check_clip_repetition = true;
        let min_match_score = config.alignment.min_match_score;

        let media_reader = SymphoniaMediaReader;
        let preset = ChromaprintPreset::default();
        let fingerprinter = ChromaprintFingerprinter::new(preset);
        let aligner = FakeAligner::with_estimate(ClipMatchEstimate {
            offset_secs: ALIGN_OFFSET,
            confidence: BASE_CONFIDENCE,
        });
        let progress = FakeProgressReporter;
        let use_case = AlignVideos::new(&media_reader, &fingerprinter, &aligner, &progress);

        let response = use_case
            .execute(AlignVideosRequest {
                video_a: path_a,
                video_b: path_b,
                config,
            })
            .expect("execute");

        let clip = &response.result.clips[0];
        let report = clip.repetition.as_ref().expect("repetition report");
        let offset = clip.offset_secs.expect("offset");
        assert!(
            should_downgrade_repetition_confidence(&report.a, &report.b, offset),
            "repeat lag must trigger downgrade"
        );
        assert_downgrade_trigger_lag_near_secs(report, offset, PURE_TONE_REPEAT_LAG_SECS);
        assert!(response.result.start_aligned);
        assert!(clip.aligned, "aligned is computed before downgrade and must stay true");
        assert!(
            (clip.confidence - BASE_CONFIDENCE * 0.5).abs() < 0.01,
            "confidence after downgrade: {}",
            clip.confidence
        );
        assert!(
            clip.confidence < min_match_score,
            "halved confidence must fall below min_match_score without clearing aligned"
        );
    }
}
