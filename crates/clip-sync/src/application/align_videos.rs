use std::path::PathBuf;
use std::time::Duration;

use crate::application::config::{AlignConfig, AlignmentMode};
use crate::application::error::{AppError, FingerprintError};
use crate::application::extraction_progress::ExtractionProgressScope;
use crate::application::high_rate_refinement::{
    apply_high_rate_refinement, HighRateRefinementInput,
};
use crate::application::locate_query::{
    locate_query_in_reference, resolve_alignment_mode, LocateQueryDeps, LocateQueryFile,
};
use crate::application::offset_refinement::{refine_offset_around_prior, refine_offset_estimate};
use crate::application::offset_verification::{
    apply_offset_verification, OffsetVerificationDeps, OffsetVerificationInput,
};
use crate::application::ports::MediaSession;
use crate::application::ports::{
    Aligner, ClipRepetitionDetector, Fingerprinter, MediaReader, PcmCorrelator, ProgressReporter,
    Resampler,
};
use crate::domain::{
    alignment::clip_with_label, attach_symmetric_planning_report_metadata, build_alignment_result,
    build_query_alignment_result, clip_windows_paired, clip_windows_with_options,
    compute_clip_timeline_overlap, end_clip_extract_unreliable, expand_window_for_slide,
    order_track_pairs_for_alignment, prepare_clip_for_fingerprint, select_aligned_subclip_pair,
    select_best_track, select_track_for_reference, set_offset_ambiguous_mod_from_start_clip,
    should_downgrade_periodic_ambiguity, should_downgrade_repetition_confidence,
    truncate_padded_tail, winning_window_on_a_timeline, AlignmentMergePolicy, AlignmentModeUsed,
    AlignmentResult, AudioTrack, ClipLabel, ClipMatchEstimate, ClipPairReportInput,
    ClipRepetitionReport, ClipWindow, DomainError, EndClipAnchor, MediaExtent, MediaSource,
    MonoPcmClip, PcmPreparationOptions, QueryLocalization, RepetitionFinding, TimelineOverlap,
    OFFSET_AGREEMENT_TOLERANCE_SECS,
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
    resampler: &'a dyn Resampler,
    correlator: &'a dyn PcmCorrelator,
    repetition_detector: &'a dyn ClipRepetitionDetector,
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
        resampler: &'a dyn Resampler,
        correlator: &'a dyn PcmCorrelator,
        repetition_detector: &'a dyn ClipRepetitionDetector,
        progress: &'a dyn ProgressReporter,
    ) -> Self {
        Self {
            media_reader,
            fingerprinter,
            aligner,
            resampler,
            correlator,
            repetition_detector,
            progress,
        }
    }

    pub fn execute(&self, request: AlignVideosRequest) -> Result<AlignVideosResponse, AppError> {
        request.config.validate()?;

        self.progress.phase_verbose("Opening media");
        let mut session_a = self
            .media_reader
            .open(&MediaSource::new(request.video_a.clone()))?;
        let mut session_b = self
            .media_reader
            .open(&MediaSource::new(request.video_b.clone()))?;

        let mode_used = self.resolve_mode(&mut session_a, &mut session_b, &request)?;

        let outcome = match mode_used {
            Some((AlignmentModeUsed::QueryReference, track_a, extent_a, track_b, extent_b)) => self
                .align_query_reference(
                    &mut session_a,
                    &mut session_b,
                    ResolvedMediaSide {
                        track: track_a,
                        extent: extent_a,
                    },
                    ResolvedMediaSide {
                        track: track_b,
                        extent: extent_b,
                    },
                    &request,
                )?,
            _ => {
                if request.config.alignment.try_all_tracks {
                    self.align_best_track_pair(&mut session_a, &mut session_b, &request)?
                } else {
                    self.align_single_track_pair(&mut session_a, &mut session_b, &request)?
                }
            }
        };

        let mut result = outcome.result;
        apply_high_rate_refinement(
            &mut HighRateRefinementInput {
                session_a: &mut session_a,
                session_b: &mut session_b,
                track_a: &outcome.track_a,
                track_b: &outcome.track_b,
                discovery_windows: &outcome.discovery_windows,
                extent_a: outcome.extent_a,
                extent_b: outcome.extent_b,
                resampler: self.resampler,
                correlator: self.correlator,
            },
            &request.config.alignment,
            &mut result,
            self.progress,
        );

        apply_offset_verification(
            &mut OffsetVerificationInput {
                session_a: &mut session_a,
                session_b: &mut session_b,
                track_a: &outcome.track_a,
                track_b: &outcome.track_b,
                discovery_windows: &outcome.discovery_windows,
                extent_a: outcome.extent_a,
                extent_b: outcome.extent_b,
                min_holdout_decode_fraction: request.config.alignment.min_end_clip_decode_fraction,
                max_holdout_decode_skips: request.config.alignment.max_end_clip_decode_skips,
                resampler: self.resampler,
                correlator: self.correlator,
            },
            &request.config,
            &mut result,
            &OffsetVerificationDeps {
                fingerprinter: self.fingerprinter,
                aligner: self.aligner,
                repetition_detector: self.repetition_detector,
            },
            self.progress,
        );

        log_alignment_summary(
            &result,
            Some(outcome.extent_a.declared),
            Some(outcome.extent_b.declared),
            self.progress,
        );

        if result.query_localization.is_none() {
            attach_symmetric_planning_report_metadata(
                &mut result,
                &outcome.extent_a,
                &outcome.extent_b,
                &request.config.clip.as_plan(),
                request.config.alignment.clip_planning_options(),
                request.config.clip.num_clips,
            );
        }

        Ok(AlignVideosResponse { result })
    }

    /// Resolve which algorithm to run, plus the per-file track + extent needed by the query
    /// path. Returns `None` when symmetric is forced or per-file resolution fails (the symmetric
    /// path then runs and surfaces any error).
    #[allow(clippy::type_complexity)]
    fn resolve_mode(
        &self,
        session_a: &mut MR::Session,
        session_b: &mut MR::Session,
        request: &AlignVideosRequest,
    ) -> Result<
        Option<(
            AlignmentModeUsed,
            AudioTrack,
            MediaExtent,
            AudioTrack,
            MediaExtent,
        )>,
        AppError,
    > {
        if request.config.alignment.mode == AlignmentMode::Symmetric {
            return Ok(None);
        }
        let plan = request.config.clip.as_plan();
        let (track_a, extent_a) = match self.resolve_track_extent(
            session_a,
            &plan,
            &request.config,
            None,
            None,
        ) {
            Ok(resolved) => resolved,
            Err(error) => {
                tracing::warn!(
                    side = "a",
                    %error,
                    "align: per-file track/extent resolution failed; falling back to symmetric alignment"
                );
                return Ok(None);
            }
        };
        let (track_b, extent_b) = match self.resolve_track_extent(
            session_b,
            &plan,
            &request.config,
            None,
            Some(&track_a),
        ) {
            Ok(resolved) => resolved,
            Err(error) => {
                tracing::warn!(
                    side = "b",
                    %error,
                    "align: per-file track/extent resolution failed; falling back to symmetric alignment"
                );
                return Ok(None);
            }
        };

        let planning = request.config.alignment.clip_planning_options();
        let (windows_a, windows_b) = if planning.end_clip_anchor == EndClipAnchor::SharedTimeline {
            clip_windows_paired(&extent_a, &extent_b, &plan, planning)
                .map(|(a, b)| (a.len(), b.len()))
                .unwrap_or((0, 0))
        } else {
            let windows_a = clip_windows_with_options(&extent_a, &plan, planning)
                .map(|w| w.len())
                .unwrap_or(0);
            let windows_b = clip_windows_with_options(&extent_b, &plan, planning)
                .map(|w| w.len())
                .unwrap_or(0);
            (windows_a, windows_b)
        };

        let mode = resolve_alignment_mode(
            request.config.alignment.mode,
            &extent_a,
            &extent_b,
            windows_a,
            windows_b,
            request.config.alignment.query_min_duration_ratio,
        );
        Ok(Some((mode, track_a, extent_a, track_b, extent_b)))
    }

    /// Lightweight track + extent resolution for the mode decision (mirrors `extract_clips`).
    ///
    /// When `channel_reference` is set and `track` is `None`, picks the best decodable B track
    /// whose channel count matches the reference (falls back to first decodable in mux order).
    fn resolve_track_extent(
        &self,
        session: &mut MR::Session,
        plan: &crate::domain::ClipPlan,
        config: &AlignConfig,
        track: Option<&AudioTrack>,
        channel_reference: Option<&AudioTrack>,
    ) -> Result<(AudioTrack, MediaExtent), AppError> {
        let tracks = session.list_tracks()?;
        let track = match track {
            Some(track) => track.clone(),
            None => {
                if let Some(reference) = channel_reference {
                    select_track_for_reference(reference, &tracks)?.clone()
                } else {
                    select_best_track(&tracks)?.clone()
                }
            }
        };
        let duration = track
            .duration
            .filter(|value| !value.is_zero())
            .ok_or(AppError::Domain(
                crate::domain::DomainError::InvalidDuration,
            ))?;
        let mut extent = MediaExtent::from_declared(duration);
        if config.needs_tail_extent_scan(plan) {
            if let Ok(tail) = session.track_decodable_extent(&track) {
                extent = extent.with_decodable(tail);
            }
        }
        Ok((track, extent))
    }

    fn log_available_tracks(&self, label: &str, tracks: &[AudioTrack]) {
        if tracks.is_empty() {
            return;
        }
        self.progress
            .phase_verbose(&format!("Audio tracks on {label}:"));
        for track in tracks {
            self.progress.phase_verbose(&format!(
                "  track {}: {}",
                track.index,
                track.format_description()
            ));
        }
    }

    fn log_selected_track(&self, label: &str, track: &AudioTrack, note: Option<&str>) {
        let suffix = note.map(|value| format!(" ({value})")).unwrap_or_default();
        self.progress.phase_verbose(&format!(
            "Selected track {}: {}{} [{label}]",
            track.index,
            track.format_description(),
            suffix
        ));
    }

    fn b_track_selection_note(
        reference: &AudioTrack,
        selected: &AudioTrack,
        tracks: &[AudioTrack],
    ) -> Option<String> {
        if selected.channels != reference.channels {
            return Some(format!(
                "no {}ch track; using first decodable",
                reference.channels
            ));
        }
        let first = select_best_track(tracks).ok()?;
        if first.index != selected.index {
            return Some(format!("channel-matched to A ({}ch)", reference.channels));
        }
        None
    }

    fn log_decodable_extent(&self, label: &str, extent: &MediaExtent) {
        if extent
            .decodable
            .is_some_and(|tail| tail + Duration::from_secs(1) < extent.declared)
        {
            self.progress.phase_verbose(&format!(
                "{label}: decodable extent {:.0}s (container {:.0}s)",
                extent.decodable.unwrap().as_secs_f64(),
                extent.declared.as_secs_f64()
            ));
        }
    }

    /// Query-reference path: localize the shorter file against the longer and build a synthetic
    /// single-clip `AlignmentResult` in A/B repair roles.
    fn align_query_reference(
        &self,
        session_a: &mut MR::Session,
        session_b: &mut MR::Session,
        side_a: ResolvedMediaSide,
        side_b: ResolvedMediaSide,
        request: &AlignVideosRequest,
    ) -> Result<AlignmentOutcome, AppError> {
        self.progress.phase("Localizing clip against reference...");
        let reference_is_a = side_a.extent.effective() >= side_b.extent.effective();
        let deps = LocateQueryDeps {
            fingerprinter: self.fingerprinter,
            aligner: self.aligner,
            resampler: self.resampler,
            correlator: self.correlator,
        };
        let search_outcome = if reference_is_a {
            locate_query_in_reference(
                LocateQueryFile {
                    session: session_a,
                    track: &side_a.track,
                    extent: side_a.extent,
                },
                LocateQueryFile {
                    session: session_b,
                    track: &side_b.track,
                    extent: side_b.extent,
                },
                &request.config,
                deps,
                self.progress,
            )
        } else {
            locate_query_in_reference(
                LocateQueryFile {
                    session: session_b,
                    track: &side_b.track,
                    extent: side_b.extent,
                },
                LocateQueryFile {
                    session: session_a,
                    track: &side_a.track,
                    extent: side_a.extent,
                },
                &request.config,
                deps,
                self.progress,
            )
        }
        .map_err(AppError::Alignment)?;
        let localization = QueryLocalization::from_reference_outcome(
            search_outcome,
            reference_is_a,
            side_a.extent,
            side_b.extent,
        );

        let (win_start, win_end) = winning_window_on_a_timeline(&localization);
        let winning = ClipWindow::new(
            Duration::from_secs_f64(win_start),
            Duration::from_secs_f64(win_end),
            ClipLabel::Start,
        );
        let result = build_query_alignment_result(
            localization,
            request.config.alignment.query_min_match_score,
        );
        Ok(AlignmentOutcome {
            result,
            track_a: side_a.track,
            track_b: side_b.track,
            discovery_windows: vec![winning],
            extent_a: side_a.extent,
            extent_b: side_b.extent,
        })
    }

    fn align_single_track_pair(
        &self,
        session_a: &mut MR::Session,
        session_b: &mut MR::Session,
        request: &AlignVideosRequest,
    ) -> Result<AlignmentOutcome, AppError> {
        let plan = request.config.clip.as_plan();
        let planning = request.config.alignment.clip_planning_options();

        let tracks_a = session_a.list_tracks()?;
        self.log_available_tracks("video A", &tracks_a);
        let (track_a, extent_a) =
            self.resolve_track_extent(session_a, &plan, &request.config, None, None)?;
        self.log_selected_track("video A", &track_a, None);
        let tracks_b = session_b.list_tracks()?;
        self.log_available_tracks("video B", &tracks_b);
        let (track_b, extent_b) =
            self.resolve_track_extent(session_b, &plan, &request.config, None, Some(&track_a))?;
        self.log_selected_track(
            "video B",
            &track_b,
            Self::b_track_selection_note(&track_a, &track_b, &tracks_b).as_deref(),
        );
        self.log_decodable_extent("video A", &extent_a);
        self.log_decodable_extent("video B", &extent_b);

        let (windows_a, windows_b) = clip_windows_paired(&extent_a, &extent_b, &plan, planning)?;
        let plan_ctx = ClipPlanFormatContext {
            end_clip_anchor: planning.end_clip_anchor,
        };
        let side = ClipExtractionSideContext {
            label: "video A",
            timeline_end: extent_a.effective(),
            plan: &plan_ctx,
            progress: &ExtractionProgressScope::with_stage_label(
                self.progress,
                "Aligning audio fingerprints (video A)...".into(),
            ),
        };
        let extracted_a = self.extract_clips_at_windows(
            session_a,
            &track_a,
            &extent_a,
            &windows_a,
            &request.config,
            &side,
        )?;
        let extracted_b = self.extract_clips_at_windows(
            session_b,
            &track_b,
            &extent_b,
            &windows_b,
            &request.config,
            &ClipExtractionSideContext {
                label: "video B",
                timeline_end: extent_b.effective(),
                plan: &plan_ctx,
                progress: &ExtractionProgressScope::with_stage_label(
                    self.progress,
                    "Aligning audio fingerprints (video B)...".into(),
                ),
            },
        )?;
        let result = self.align_extracted_pair(&extracted_a, &extracted_b, &request.config)?;
        Ok(AlignmentOutcome {
            result,
            track_a: extracted_a.track,
            track_b: extracted_b.track,
            discovery_windows: extracted_a.windows,
            extent_a: extracted_a.extent,
            extent_b: extracted_b.extent,
        })
    }

    fn align_best_track_pair(
        &self,
        session_a: &mut MR::Session,
        session_b: &mut MR::Session,
        request: &AlignVideosRequest,
    ) -> Result<AlignmentOutcome, AppError> {
        let tracks_a = session_a.list_tracks()?;
        let tracks_b = session_b.list_tracks()?;
        let decodable_a: Vec<&AudioTrack> =
            tracks_a.iter().filter(|track| track.decodable).collect();
        let decodable_b: Vec<&AudioTrack> =
            tracks_b.iter().filter(|track| track.decodable).collect();

        if decodable_a.is_empty() || decodable_b.is_empty() {
            return Err(crate::domain::DomainError::NoDecodableAudioTracks.into());
        }

        self.log_available_tracks("video A", &tracks_a);
        self.log_available_tracks("video B", &tracks_b);

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

        let pairs = order_track_pairs_for_alignment(&decodable_a, &decodable_b);
        let mut pair_failures: Vec<String> = Vec::new();
        for (track_a, track_b) in pairs {
            self.progress.phase_verbose(&format!(
                "Trying track pair A:{} ({}) / B:{} ({})",
                track_a.index,
                track_a.format_description(),
                track_b.index,
                track_b.format_description(),
            ));
            let pair_outcome = (|| -> Result<
                (AlignmentOutcome, ExtractedClips, ExtractedClips, f32),
                AppError,
            > {
            let (resolved_a, extent_a) = self.resolve_track_extent(
                session_a,
                &plan,
                &request.config,
                Some(track_a),
                None,
            )?;
            let (resolved_b, extent_b) = self.resolve_track_extent(
                session_b,
                &plan,
                &request.config,
                Some(track_b),
                None,
            )?;
            let planning = request.config.alignment.clip_planning_options();
            let (windows_a, windows_b) =
                clip_windows_paired(&extent_a, &extent_b, &plan, planning)?;
            let plan_ctx = ClipPlanFormatContext {
                end_clip_anchor: planning.end_clip_anchor,
            };
            let extracted_a = self.extract_clips_at_windows(
                session_a,
                &resolved_a,
                &extent_a,
                &windows_a,
                &request.config,
                &ClipExtractionSideContext {
                    label: "video A",
                    timeline_end: extent_a.effective(),
                    plan: &plan_ctx,
                    progress: &extraction,
                },
            )?;
            let extracted_b = self.extract_clips_at_windows(
                session_b,
                &resolved_b,
                &extent_b,
                &windows_b,
                &request.config,
                &ClipExtractionSideContext {
                    label: "video B",
                    timeline_end: extent_b.effective(),
                    plan: &plan_ctx,
                    progress: &extraction,
                },
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
                extent_a: extracted_a.extent,
                extent_b: extracted_b.extent,
            };
            Ok((outcome, extracted_a, extracted_b, score))
            })();

            match pair_outcome {
                Ok(candidate) => {
                    let score = candidate.3;
                    if best
                        .as_ref()
                        .is_none_or(|(_, _, _, best_score)| score > *best_score)
                    {
                        best = Some(candidate);
                    }
                }
                Err(error) => {
                    let detail = error.to_string();
                    pair_failures.push(format!(
                        "A:{} / B:{}: {detail}",
                        track_a.index, track_b.index
                    ));
                    self.progress.phase_verbose(&format!(
                        "Track pair A:{} / B:{} skipped: {detail}",
                        track_a.index, track_b.index
                    ));
                }
            }
        }

        let (mut outcome, winning_a, winning_b, _) = best.ok_or_else(|| {
            let tried = pair_failures.join("; ");
            AppError::Alignment(crate::application::error::AlignmentError::EngineFailed(
                if tried.is_empty() {
                    "no track pair produced an alignment".into()
                } else {
                    format!("no track pair produced an alignment ({tried})")
                },
            ))
        })?;

        self.log_selected_track("video A", &outcome.track_a, None);
        self.log_selected_track(
            "video B",
            &outcome.track_b,
            Self::b_track_selection_note(&outcome.track_a, &outcome.track_b, &tracks_b).as_deref(),
        );

        // Run repetition check on the winning pair only.
        if request.config.validation.check_clip_repetition {
            let rep_result = self.align_extracted_pair(&winning_a, &winning_b, &request.config)?;
            outcome.result = rep_result;
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
        let mut repetition_diagnostics: Vec<(
            Option<RepetitionFinding>,
            Option<RepetitionFinding>,
        )> = Vec::with_capacity(extracted_a.raw_clips.len());
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

            let truncated_a = (window.label == ClipLabel::End).then(|| truncate_padded_tail(raw_a));
            let truncated_b = (window.label == ClipLabel::End).then(|| truncate_padded_tail(raw_b));
            let raw_a = truncated_a.as_deref().unwrap_or(raw_a);
            let raw_b = truncated_b.as_deref().unwrap_or(raw_b);

            // When sliding, select allocates subclips. Otherwise prepare from the borrowed
            // (or truncated) refs — one clone inside prepare, not a redundant pre-clone.
            let slid;
            let (clip_a, clip_b) = if config.clip.window_slide_secs > 0 {
                slid = select_aligned_subclip_pair(raw_a, raw_b, window.duration());
                (&slid.0, &slid.1)
            } else {
                (raw_a, raw_b)
            };

            let source_duration_a = clip_a.duration_secs();
            let source_duration_b = clip_b.duration_secs();
            let prepared_a = prepare_clip_for_fingerprint(clip_a, prep_options);
            let prepared_b = prepare_clip_for_fingerprint(clip_b, prep_options);

            if is_skippable_prepare_error(&prepared_a) || is_skippable_prepare_error(&prepared_b) {
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
                let rep_a = self.repetition_detector.detect_clip_repetition(
                    fp_a,
                    clip_a.duration_secs(),
                    preset,
                    min_conf,
                    source_duration_a,
                );
                let rep_b = self.repetition_detector.detect_clip_repetition(
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
                    independent = refine_offset_estimate(
                        &clip_a,
                        &clip_b,
                        independent,
                        self.resampler,
                        self.correlator,
                    );
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
                            self.resampler,
                            self.correlator,
                        )
                    } else {
                        prior
                    };
                    (true, refined)
                } else if config.alignment.constrain_end_clip_to_start_offset
                    && config.alignment.refine_offset_with_pcm
                    && prior.confidence >= config.alignment.min_match_score
                {
                    let refined = refine_offset_around_prior(
                        &clip_a,
                        &clip_b,
                        prior,
                        config.alignment.end_clip_refine_radius_secs,
                        self.resampler,
                        self.correlator,
                    );
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
                    chromaprint_estimate = refine_offset_estimate(
                        &clip_a,
                        &clip_b,
                        chromaprint_estimate,
                        self.resampler,
                        self.correlator,
                    );
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
                duration_a: Some(extracted_a.extent.declared),
                duration_b: Some(extracted_b.extent.declared),
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
                let report = ClipRepetitionReport { a: rep_a, b: rep_b };
                let clip_duration_secs = clip.window_end_secs - clip.window_start_secs;
                let periodic = clip.label == ClipLabel::Start
                    && should_downgrade_periodic_ambiguity(
                        &report,
                        config.validation.min_repetition_confidence,
                        Some(clip_duration_secs),
                        estimates[i].offset_secs,
                    );
                if should_downgrade_repetition_confidence(&rep_a, &rep_b, estimates[i].offset_secs)
                    || periodic
                {
                    clip.confidence *= 0.5;
                }
                clip.repetition = Some(report);
            }
            set_offset_ambiguous_mod_from_start_clip(
                &mut result,
                config.validation.min_repetition_confidence,
            );
        }

        Ok(result)
    }

    fn extract_clips_at_windows(
        &self,
        session: &mut MR::Session,
        track: &AudioTrack,
        extent: &MediaExtent,
        windows: &[ClipWindow],
        config: &AlignConfig,
        side: &ClipExtractionSideContext<'_>,
    ) -> Result<ExtractedClips, AppError> {
        self.progress
            .phase_verbose(&format_clip_plan(side, windows));

        let timeline_end = windows
            .iter()
            .find(|window| window.label == ClipLabel::End)
            .map(|window| window.end)
            .unwrap_or_else(|| extent.effective());

        side.progress.register_batch(windows.len() as u64);

        let mut extract_order: Vec<usize> = (0..windows.len()).collect();
        if windows.len() > 1 {
            extract_order.sort_by_key(|&index| windows[index].start);
            let chronological: Vec<usize> = (0..windows.len()).collect();
            if extract_order != chronological {
                self.progress.phase_verbose(&format!(
                    "Extracting {} clip(s) in chronological order ({})",
                    windows.len(),
                    side.label
                ));
            }
        }

        let mut raw_clips: Vec<Option<MonoPcmClip>> = vec![None; windows.len()];
        for (step, &index) in extract_order.iter().enumerate() {
            let window = &windows[index];
            let extract_window =
                expand_window_for_slide(window, config.clip.window_slide_secs, timeline_end);
            let progress_label = format!(
                "Extracting clip {}/{} ({}, {})",
                step + 1,
                windows.len(),
                side.label,
                format_duration(window.duration())
            );
            let clip_progress = side.progress.for_clip(step as u64);
            let mut clip =
                session.extract_mono(track, &extract_window, &clip_progress, &progress_label)?;
            if let Some(target_rate) = config.clip.target_sample_rate {
                clip = self.resampler.resample_mono(&clip, target_rate);
            }
            raw_clips[index] = Some(clip);
        }

        side.progress.finish_batch(windows.len() as u64);

        let raw_clips: Vec<MonoPcmClip> = raw_clips
            .into_iter()
            .map(|clip| clip.expect("every clip window was extracted"))
            .collect();
        let decode_skips: Vec<u32> = raw_clips
            .iter()
            .map(|clip| clip.decode_error_skips)
            .collect();

        let end_clip_unreliable = windows.iter().zip(raw_clips.iter()).any(|(window, clip)| {
            window.label == ClipLabel::End
                && end_clip_extract_unreliable(
                    clip,
                    window,
                    config.alignment.min_end_clip_decode_fraction,
                    config.alignment.max_end_clip_decode_skips,
                )
        });

        Ok(ExtractedClips {
            raw_clips,
            decode_skips,
            windows: windows.to_vec(),
            extent: *extent,
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
    extent_a: MediaExtent,
    extent_b: MediaExtent,
}

/// Resolved track + decodable extent for one input file.
struct ResolvedMediaSide {
    track: AudioTrack,
    extent: MediaExtent,
}

/// Per-file context for paired clip planning, extraction, and verbose plan lines.
struct ClipExtractionSideContext<'a> {
    label: &'a str,
    timeline_end: Duration,
    plan: &'a ClipPlanFormatContext,
    progress: &'a ExtractionProgressScope<'a>,
}

struct ClipPlanFormatContext {
    end_clip_anchor: EndClipAnchor,
}

fn is_skippable_prepare_error(result: &Result<MonoPcmClip, DomainError>) -> bool {
    matches!(
        result,
        Err(DomainError::InsufficientAudio) | Err(DomainError::EmptyClip)
    )
}

fn map_prepare_error(error: DomainError) -> AppError {
    match error {
        DomainError::InsufficientAudio | DomainError::EmptyClip => AppError::Fingerprint(
            FingerprintError::InvalidPcm("insufficient audio content for fingerprinting".into()),
        ),
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
    extent: MediaExtent,
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
            recommended_offset_source_label(result)
        )),
        None => progress.phase_verbose("Recommended offset: none (no confident clip matches)"),
    }

    if let Some(refine) = &result.high_rate_refinement {
        let refine_report = crate::application::report::HighRateRefinementReport::from(refine);
        for line in
            crate::application::report::format_high_rate_refinement_lines(&refine_report, false)
        {
            progress.phase_verbose(&line);
        }
        if refine.skipped {
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

fn recommended_offset_source_label(result: &AlignmentResult) -> &'static str {
    if result.offsets_consistent {
        return "clip offsets agree";
    }
    if !result.start_aligned {
        return "clip offsets disagree; using configured preference";
    }
    let (Some(start), Some(end)) = (
        clip_with_label(&result.clips, ClipLabel::Start).and_then(|c| c.offset_secs),
        clip_with_label(&result.clips, ClipLabel::End).and_then(|c| c.offset_secs),
    ) else {
        return "clip offsets disagree; using start clip";
    };
    if (start - end).abs() <= OFFSET_AGREEMENT_TOLERANCE_SECS {
        return "clip offsets disagree; using start clip";
    }
    let median = (start + end) / 2.0;
    if (start - median).abs() <= OFFSET_AGREEMENT_TOLERANCE_SECS
        && (end - median).abs() <= OFFSET_AGREEMENT_TOLERANCE_SECS
    {
        "clip offsets disagree; confidence-weighted fusion"
    } else {
        "clip offsets disagree; using start clip"
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
    if value {
        "yes"
    } else {
        "no"
    }
}

fn format_clip_plan(side: &ClipExtractionSideContext<'_>, windows: &[ClipWindow]) -> String {
    let anchor_note = match side.plan.end_clip_anchor {
        EndClipAnchor::SharedTimeline => "shared timeline anchor",
        EndClipAnchor::FileTail => "file tail anchor",
    };

    let parts: Vec<String> = windows
        .iter()
        .map(|window| {
            let mut part = format!(
                "[{}–{}] {} ({})",
                format_duration(window.start),
                format_duration(window.end),
                clip_label_name(window.label),
                format_duration(window.duration())
            );
            if side.plan.end_clip_anchor == EndClipAnchor::SharedTimeline
                && window.label == ClipLabel::End
                && window.end + Duration::from_secs(1) < side.timeline_end
            {
                part.push_str(&format!(
                    " (anchored at {}, not file tail {})",
                    format_duration(window.end),
                    format_duration(side.timeline_end)
                ));
            }
            part
        })
        .collect();

    format!(
        "Clip plan for {} ({anchor_note}): {} clip(s) — {}",
        side.label,
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
    use crate::application::config::{
        AlignConfig, AlignmentConfig, AlignmentMode, ClipConfig, ValidationConfig,
    };
    use crate::application::error::{
        AlignmentError, AppError, ConfigError, FingerprintError, MediaError,
    };
    use crate::application::testing::fakes::{
        FakeAligner, FakeFingerprinter, FakeMediaReader, FakeMediaSession, FakePcmCorrelator,
        FakeProgressReporter,
    };
    use crate::domain::{
        ClipLabel, ClipMatch, ClipMatchEstimate, ClipRepetitionReport, DomainError, EndClipAnchor,
    };
    use crate::infrastructure::chromaprint::repetition::{
        test_repetition_detect_calls, test_reset_repetition_detect_calls,
    };
    use crate::infrastructure::chromaprint::ChromaprintClipRepetitionDetector;
    use crate::infrastructure::correlation::FftCorrelator;
    use crate::infrastructure::resample::RubatoResampler;

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
        let close = |rep: &RepetitionFinding| (rep.lag_secs - offset_secs.abs()).abs() <= 1.0;
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

    fn execute_fake_repetition_case(config: AlignConfig, offset_secs: f64) -> AlignVideosResponse {
        let reader = matched_reader();
        let fingerprinter = FakeFingerprinter::new();
        let aligner = FakeAligner::with_estimate(ClipMatchEstimate {
            offset_secs,
            confidence: FAKE_REPETITION_MATCH_CONFIDENCE,
        });
        let progress = FakeProgressReporter;
        let correlator = FakePcmCorrelator::new();
        let use_case = AlignVideos::new(
            &reader,
            &fingerprinter,
            &aligner,
            &RubatoResampler,
            &correlator,
            &ChromaprintClipRepetitionDetector,
            &progress,
        );
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
        let correlator = FakePcmCorrelator::new();
        let use_case = AlignVideos::new(
            &reader,
            &fingerprinter,
            &aligner,
            &RubatoResampler,
            &correlator,
            &ChromaprintClipRepetitionDetector,
            &progress,
        );

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
        let correlator = FakePcmCorrelator::new();
        let use_case = AlignVideos::new(
            &reader,
            &fingerprinter,
            &aligner,
            &RubatoResampler,
            &correlator,
            &ChromaprintClipRepetitionDetector,
            &progress,
        );

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
        let correlator = FakePcmCorrelator::new();
        let use_case = AlignVideos::new(
            &reader,
            &fingerprinter,
            &aligner,
            &RubatoResampler,
            &correlator,
            &ChromaprintClipRepetitionDetector,
            &progress,
        );

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
    fn execute_symmetric_paired_planning_aligns_unequal_lengths() {
        let reader = FakeMediaReader::new()
            .with_session("a.wav", FakeMediaSession::with_duration(mins(3)))
            .with_session(
                "b.wav",
                FakeMediaSession::with_duration(Duration::from_secs(45)),
            );
        let fingerprinter = FakeFingerprinter::new();
        let aligner = FakeAligner::with_estimate(ClipMatchEstimate {
            offset_secs: 0.0,
            confidence: 1.0,
        });
        let progress = FakeProgressReporter;
        let correlator = FakePcmCorrelator::new();
        let use_case = AlignVideos::new(
            &reader,
            &fingerprinter,
            &aligner,
            &RubatoResampler,
            &correlator,
            &ChromaprintClipRepetitionDetector,
            &progress,
        );

        let mut config = two_clip_config();
        config.alignment.mode = AlignmentMode::Symmetric;
        let response = use_case
            .execute(request(config))
            .expect("paired planning should keep clip counts aligned");
        assert!(response.result.start_aligned);
        assert_eq!(response.result.clips.len(), 1);
    }

    #[test]
    fn execute_auto_routes_clip_count_mismatch_to_query() {
        // A=3min, B=45s: ratio 0.25 < 0.5 triggers query mode under Auto (Tier 1). B is shorter
        // than MIN_CLIP_LENGTH, so query mode skips gracefully and records that query mode ran.
        let reader = FakeMediaReader::new()
            .with_session("a.wav", FakeMediaSession::with_duration(mins(3)))
            .with_session(
                "b.wav",
                FakeMediaSession::with_duration(Duration::from_secs(45)),
            );
        let fingerprinter = FakeFingerprinter::new();
        let aligner = FakeAligner::with_estimate(ClipMatchEstimate {
            offset_secs: 0.0,
            confidence: 1.0,
        });
        let progress = FakeProgressReporter;
        let correlator = FakePcmCorrelator::new();
        let use_case = AlignVideos::new(
            &reader,
            &fingerprinter,
            &aligner,
            &RubatoResampler,
            &correlator,
            &ChromaprintClipRepetitionDetector,
            &progress,
        );

        let response = use_case
            .execute(request(two_clip_config()))
            .expect("query mode should not error on a short query");
        assert_eq!(
            response.result.alignment_mode_used,
            Some(AlignmentModeUsed::QueryReference)
        );
        assert!(!response.result.start_aligned);
        assert!(response.result.query_localization.is_some());
    }

    #[test]
    fn execute_query_reference_runs_when_a_is_shorter_than_b() {
        use crate::application::testing::audio_fixtures::write_query_reference_b_longer_chirp_pair;
        use crate::infrastructure::chromaprint::{ChromaprintAligner, ChromaprintFingerprinter};
        use crate::infrastructure::symphonia::SymphoniaMediaReader;

        const REFERENCE_SECS: u32 = 360;
        const QUERY_ANCHOR_SECS: u32 = 240;
        const QUERY_DURATION_SECS: u32 = 90;

        let temp = tempfile::tempdir().expect("tempdir");
        let (path_a, path_b) = write_query_reference_b_longer_chirp_pair(
            temp.path(),
            11_025,
            REFERENCE_SECS,
            QUERY_ANCHOR_SECS,
            QUERY_DURATION_SECS,
        );

        let media_reader = SymphoniaMediaReader;
        let fingerprinter = ChromaprintFingerprinter::default();
        let aligner = ChromaprintAligner::default();
        let progress = FakeProgressReporter;
        let correlator = FakePcmCorrelator::new();
        let use_case = AlignVideos::new(
            &media_reader,
            &fingerprinter,
            &aligner,
            &RubatoResampler,
            &correlator,
            &ChromaprintClipRepetitionDetector,
            &progress,
        );

        let mut config = two_clip_config();
        config.alignment.mode = AlignmentMode::QueryReference;
        config.clip.clip_length = Duration::from_secs(u64::from(QUERY_DURATION_SECS));
        config.alignment.refine_offset_high_rate = false;
        config.validation.verify_offset = false;

        let response = use_case
            .execute(AlignVideosRequest {
                video_a: path_a,
                video_b: path_b,
                config,
            })
            .expect("query mode should run when A is shorter than B");

        assert_eq!(
            response.result.alignment_mode_used,
            Some(AlignmentModeUsed::QueryReference)
        );
        let loc = response
            .result
            .query_localization
            .as_ref()
            .expect("localization");
        assert!(
            loc.skip_reason.is_none(),
            "unexpected skip: {:?}",
            loc.skip_reason
        );
        let offset = response
            .result
            .recommended_offset_secs
            .expect("recommended offset");
        assert!(
            offset > 0.0,
            "B-longer query mode should yield positive offset, got {offset}"
        );
        assert!(
            (offset - f64::from(QUERY_ANCHOR_SECS)).abs() < 2.0,
            "offset {offset} expected ~{QUERY_ANCHOR_SECS}"
        );
        assert!(loc.clip_on_a_start_secs.abs() < 2.0);
        crate::domain::query_localization::assert_recommended_offset_matches_orientation(loc, 2.0);
    }

    #[test]
    fn execute_propagates_media_open_error() {
        let reader = FakeMediaReader::new()
            .with_open_error(MediaError::FileNotFound(PathBuf::from("a.wav")));
        let fingerprinter = FakeFingerprinter::new();
        let aligner = FakeAligner::with_estimate(ClipMatchEstimate {
            offset_secs: 0.0,
            confidence: 1.0,
        });
        let progress = FakeProgressReporter;
        let correlator = FakePcmCorrelator::new();
        let use_case = AlignVideos::new(
            &reader,
            &fingerprinter,
            &aligner,
            &RubatoResampler,
            &correlator,
            &ChromaprintClipRepetitionDetector,
            &progress,
        );

        let error = use_case.execute(request(two_clip_config())).unwrap_err();
        assert!(matches!(
            error,
            AppError::Media(MediaError::FileNotFound(_))
        ));
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
        let correlator = FakePcmCorrelator::new();
        let use_case = AlignVideos::new(
            &reader,
            &fingerprinter,
            &aligner,
            &RubatoResampler,
            &correlator,
            &ChromaprintClipRepetitionDetector,
            &progress,
        );

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
        let aligner =
            FakeAligner::with_error(AlignmentError::EngineFailed("matcher exploded".into()));
        let progress = FakeProgressReporter;
        let correlator = FakePcmCorrelator::new();
        let use_case = AlignVideos::new(
            &reader,
            &fingerprinter,
            &aligner,
            &RubatoResampler,
            &correlator,
            &ChromaprintClipRepetitionDetector,
            &progress,
        );

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
                    bit_depth: None,
                }]),
            )
            .with_session("b.wav", FakeMediaSession::with_duration(mins(3)));
        let fingerprinter = FakeFingerprinter::new();
        let aligner = FakeAligner::with_estimate(ClipMatchEstimate {
            offset_secs: 0.0,
            confidence: 1.0,
        });
        let progress = FakeProgressReporter;
        let correlator = FakePcmCorrelator::new();
        let use_case = AlignVideos::new(
            &reader,
            &fingerprinter,
            &aligner,
            &RubatoResampler,
            &correlator,
            &ChromaprintClipRepetitionDetector,
            &progress,
        );

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
        let correlator = FakePcmCorrelator::new();
        let use_case = AlignVideos::new(
            &reader,
            &fingerprinter,
            &aligner,
            &RubatoResampler,
            &correlator,
            &ChromaprintClipRepetitionDetector,
            &progress,
        );

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
                FakeMediaSession::with_duration(mins(3))
                    .with_extract_error(MediaError::decode_failed(0, "boom")),
            )
            .with_session("b.wav", FakeMediaSession::with_duration(mins(3)));
        let fingerprinter = FakeFingerprinter::new();
        let aligner = FakeAligner::with_estimate(ClipMatchEstimate {
            offset_secs: 0.0,
            confidence: 1.0,
        });
        let progress = FakeProgressReporter;
        let correlator = FakePcmCorrelator::new();
        let use_case = AlignVideos::new(
            &reader,
            &fingerprinter,
            &aligner,
            &RubatoResampler,
            &correlator,
            &ChromaprintClipRepetitionDetector,
            &progress,
        );

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
        let correlator = FakePcmCorrelator::new();
        let use_case = AlignVideos::new(
            &reader,
            &fingerprinter,
            &aligner,
            &RubatoResampler,
            &correlator,
            &ChromaprintClipRepetitionDetector,
            &progress,
        );

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
        let correlator = FakePcmCorrelator::new();
        let use_case = AlignVideos::new(
            &reader,
            &fingerprinter,
            &aligner,
            &RubatoResampler,
            &correlator,
            &ChromaprintClipRepetitionDetector,
            &progress,
        );

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
        let correlator = FakePcmCorrelator::new();
        let use_case = AlignVideos::new(
            &reader,
            &fingerprinter,
            &aligner,
            &RubatoResampler,
            &correlator,
            &ChromaprintClipRepetitionDetector,
            &progress,
        );

        let mut config = two_clip_config();
        config.alignment.constrain_end_clip_to_start_offset = false;

        let response = use_case
            .execute(request(config))
            .expect("execute should succeed");

        assert!(!response.result.offsets_consistent);
        assert_eq!(response.result.clips[0].offset_secs, Some(10.0));
        assert_eq!(response.result.clips[1].offset_secs, Some(20.0));
        assert_eq!(response.result.recommended_offset_secs, None);
    }

    #[test]
    fn execute_constrains_end_to_start_prior_when_chromaprint_disagrees() {
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
        let correlator = FakePcmCorrelator::new();
        let use_case = AlignVideos::new(
            &reader,
            &fingerprinter,
            &aligner,
            &RubatoResampler,
            &correlator,
            &ChromaprintClipRepetitionDetector,
            &progress,
        );

        let mut config = two_clip_config();
        config.alignment.refine_offset_with_pcm = true;
        config.alignment.constrain_end_clip_to_start_offset = true;
        config.alignment.require_consistent_offsets = false;

        let response = use_case
            .execute(request(config))
            .expect("execute should succeed");

        assert!(response.result.offsets_consistent);
        assert_eq!(response.result.clips[0].offset_secs, Some(10.0));
        assert_eq!(response.result.clips[1].offset_secs, Some(10.0));
        assert_eq!(response.result.recommended_offset_secs, Some(10.0));
    }

    #[test]
    fn execute_detects_known_offset_through_real_wav_pipeline() {
        use crate::application::config::ChromaprintPreset;
        use crate::application::testing::audio_fixtures::write_offset_chirp_wav_pair;
        use crate::infrastructure::chromaprint::{ChromaprintAligner, ChromaprintFingerprinter};
        use crate::infrastructure::symphonia::SymphoniaMediaReader;

        const SAMPLE_RATE: u32 = 44_100;
        const TOTAL_SECS: u32 = 120;
        const OFFSET_SECS: u32 = 3;

        let temp = tempfile::tempdir().expect("tempdir");
        let (path_a, path_b) =
            write_offset_chirp_wav_pair(temp.path(), SAMPLE_RATE, TOTAL_SECS, OFFSET_SECS);

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
        let use_case = AlignVideos::new(
            &media_reader,
            &fingerprinter,
            &aligner,
            &RubatoResampler,
            &FftCorrelator,
            &ChromaprintClipRepetitionDetector,
            &progress,
        );

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
        use crate::application::config::ChromaprintPreset;
        use crate::application::testing::audio_fixtures::write_offset_chirp_wav_pair;
        use crate::infrastructure::chromaprint::{ChromaprintAligner, ChromaprintFingerprinter};
        use crate::infrastructure::symphonia::SymphoniaMediaReader;

        const SAMPLE_RATE: u32 = 44_100;
        const TOTAL_SECS: u32 = 120;
        const OFFSET_SECS: u32 = 3;

        let temp = tempfile::tempdir().expect("tempdir");
        let (path_a, path_b) =
            write_offset_chirp_wav_pair(temp.path(), SAMPLE_RATE, TOTAL_SECS, OFFSET_SECS);

        let media_reader = SymphoniaMediaReader;
        let preset = ChromaprintPreset::default();
        let fingerprinter = ChromaprintFingerprinter::new(preset);
        let aligner = ChromaprintAligner::new(preset);
        let progress = FakeProgressReporter;
        let use_case = AlignVideos::new(
            &media_reader,
            &fingerprinter,
            &aligner,
            &RubatoResampler,
            &FftCorrelator,
            &ChromaprintClipRepetitionDetector,
            &progress,
        );

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
        use crate::application::config::ChromaprintPreset;
        use crate::application::testing::audio_fixtures::write_offset_chirp_wav_pair;
        use crate::infrastructure::chromaprint::{ChromaprintAligner, ChromaprintFingerprinter};
        use crate::infrastructure::symphonia::SymphoniaMediaReader;

        const SAMPLE_RATE: u32 = 44_100;
        const TOTAL_SECS: u32 = 180;
        const CLIP_SECS: u64 = 60;
        const OFFSET_SECS: f64 = 3.0;

        let temp = tempfile::tempdir().expect("tempdir");
        let (path_a, path_b) =
            write_offset_chirp_wav_pair(temp.path(), SAMPLE_RATE, TOTAL_SECS, OFFSET_SECS as u32);

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
        let use_case = AlignVideos::new(
            &media_reader,
            &fingerprinter,
            &aligner,
            &RubatoResampler,
            &FftCorrelator,
            &ChromaprintClipRepetitionDetector,
            &progress,
        );

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

        let start_offset = response.result.clips[0].offset_secs.expect("start offset");
        let end_offset = response.result.clips[1].offset_secs.expect("end offset");
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
        let correlator = FakePcmCorrelator::new();
        let use_case = AlignVideos::new(
            &reader,
            &fingerprinter,
            &aligner,
            &RubatoResampler,
            &correlator,
            &ChromaprintClipRepetitionDetector,
            &progress,
        );

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

        let report = crate::application::report::AlignmentReport::from(&response.result);
        let json = serde_json::to_string(&report).expect("serialize");
        let value: serde_json::Value = serde_json::from_str(&json).expect("parse");
        for clip in value["clips"].as_array().unwrap() {
            let repetition = &clip["repetition"];
            assert!(
                repetition.is_object(),
                "repetition must be an object when flag on"
            );
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
        let correlator = FakePcmCorrelator::new();
        let use_case = AlignVideos::new(
            &reader,
            &fingerprinter,
            &aligner,
            &RubatoResampler,
            &correlator,
            &ChromaprintClipRepetitionDetector,
            &progress,
        );

        let response = use_case
            .execute(request(two_clip_config()))
            .expect("execute should succeed");

        let report = crate::application::report::AlignmentReport::from(&response.result);
        let json = serde_json::to_string(&report).expect("serialize");
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
                    bit_depth: None,
                },
                crate::domain::AudioTrack {
                    index: 1,
                    codec: "pcm".into(),
                    channels: 1,
                    sample_rate: 44_100,
                    duration: Some(duration),
                    decodable: true,
                    bit_depth: None,
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
        let correlator = FakePcmCorrelator::new();
        let use_case = AlignVideos::new(
            &reader,
            &fingerprinter,
            &aligner,
            &RubatoResampler,
            &correlator,
            &ChromaprintClipRepetitionDetector,
            &progress,
        );

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
        let correlator = FakePcmCorrelator::new();
        let use_case = AlignVideos::new(
            &reader,
            &fingerprinter,
            &aligner,
            &RubatoResampler,
            &correlator,
            &ChromaprintClipRepetitionDetector,
            &progress,
        );

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
        let use_case = AlignVideos::new(
            &media_reader,
            &fingerprinter,
            &aligner,
            &RubatoResampler,
            &FftCorrelator,
            &ChromaprintClipRepetitionDetector,
            &progress,
        );

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
        let offset = response_rep.result.recommended_offset_secs.unwrap_or(0.0);
        assert!(offset.abs() < 0.5, "offset={offset}");

        let clip = &response_rep.result.clips[0];
        let report = clip.repetition.as_ref().expect("repetition report");
        assert_repetition_lag_near_secs(report, PURE_TONE_REPEAT_LAG_SECS);
        let finding = report
            .a
            .as_ref()
            .or(report.b.as_ref())
            .expect("repetition finding on a or b");
        assert!(finding.confidence >= 0.5);
        // Repeat lag (~30 s) differs from offset (~0 s) — downgrade must not apply in the pipeline.
        assert!(
            !should_downgrade_repetition_confidence(
                &report.a,
                &report.b,
                clip.offset_secs.unwrap_or(0.0)
            ),
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
        let use_case = AlignVideos::new(
            &media_reader,
            &fingerprinter,
            &aligner,
            &RubatoResampler,
            &FftCorrelator,
            &ChromaprintClipRepetitionDetector,
            &progress,
        );

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
        let use_case = AlignVideos::new(
            &media_reader,
            &fingerprinter,
            &aligner,
            &RubatoResampler,
            &FftCorrelator,
            &ChromaprintClipRepetitionDetector,
            &progress,
        );

        let response_base = use_case
            .execute(AlignVideosRequest {
                video_a: path_a.clone(),
                video_b: path_b.clone(),
                config: base_config.clone(),
            })
            .expect("baseline execute");

        assert!(
            response_base.result.start_aligned,
            "expected aligned at +30s"
        );
        let base_offset = response_base.result.clips[0].offset_secs.expect("offset");
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
        assert!(
            clip.aligned,
            "clip.aligned must not flip after confidence downgrade"
        );
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
        let use_case = AlignVideos::new(
            &media_reader,
            &fingerprinter,
            &aligner,
            &RubatoResampler,
            &FftCorrelator,
            &ChromaprintClipRepetitionDetector,
            &progress,
        );

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
        assert!(
            clip.aligned,
            "aligned is computed before downgrade and must stay true"
        );
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

    fn anchored_end_chromaprint_config(
        num_clips: u32,
        anchor: EndClipAnchor,
        mode: AlignmentMode,
    ) -> AlignConfig {
        AlignConfig {
            clip: ClipConfig {
                clip_length: Duration::from_secs(60),
                num_clips,
                target_sample_rate: Some(11_025),
                normalize_loudness: false,
                trim_silence: false,
                window_slide_secs: 0,
                ..ClipConfig::default()
            },
            alignment: AlignmentConfig {
                mode,
                end_clip_anchor: anchor,
                refine_offset_high_rate: false,
                refine_offset_with_pcm: false,
                ..Default::default()
            },
            validation: ValidationConfig {
                verify_offset: false,
                check_clip_repetition: false,
                ..Default::default()
            },
        }
    }

    fn run_anchored_end_chirp_alignment(
        shared_secs: u32,
        long_secs: u32,
        offset_secs: u32,
        config: AlignConfig,
    ) -> AlignVideosResponse {
        use crate::application::config::ChromaprintPreset;
        use crate::application::testing::audio_fixtures::write_anchored_end_symmetric_pair;
        use crate::infrastructure::chromaprint::{ChromaprintAligner, ChromaprintFingerprinter};
        use crate::infrastructure::symphonia::SymphoniaMediaReader;

        let temp = tempfile::tempdir().expect("tempdir");
        let (path_a, path_b) = write_anchored_end_symmetric_pair(
            temp.path(),
            11_025,
            shared_secs,
            long_secs,
            offset_secs,
        );
        let media_reader = SymphoniaMediaReader;
        let preset = ChromaprintPreset::default();
        let fingerprinter = ChromaprintFingerprinter::new(preset);
        let aligner = ChromaprintAligner::new(preset);
        let progress = FakeProgressReporter;
        let use_case = AlignVideos::new(
            &media_reader,
            &fingerprinter,
            &aligner,
            &RubatoResampler,
            &FftCorrelator,
            &ChromaprintClipRepetitionDetector,
            &progress,
        );
        use_case
            .execute(AlignVideosRequest {
                video_a: path_a,
                video_b: path_b,
                config,
            })
            .expect("anchored end chirp execute")
    }

    #[test]
    fn symmetric_shared_timeline_end_clips_agree_on_unequal_pair() {
        const SHARED_SECS: u32 = 120;
        const LONG_SECS: u32 = 300;
        const OFFSET_SECS: u32 = 12;

        let response = run_anchored_end_chirp_alignment(
            SHARED_SECS,
            LONG_SECS,
            OFFSET_SECS,
            anchored_end_chromaprint_config(
                2,
                EndClipAnchor::SharedTimeline,
                AlignmentMode::Symmetric,
            ),
        );
        let result = &response.result;
        assert_eq!(result.clips.len(), 2);
        let start = result
            .clips
            .iter()
            .find(|c| c.label == ClipLabel::Start)
            .expect("start clip");
        let end = result
            .clips
            .iter()
            .find(|c| c.label == ClipLabel::End)
            .expect("end clip");
        assert!(start.aligned && end.aligned, "both clips should align");
        assert!(
            start.confidence >= 0.5 && end.confidence >= 0.5,
            "start={:.2} end={:.2}",
            start.confidence,
            end.confidence
        );
        let start_off = start.offset_secs.expect("start offset");
        let end_off = end.offset_secs.expect("end offset");
        assert!(
            (start_off - end_off).abs() <= OFFSET_AGREEMENT_TOLERANCE_SECS,
            "start={start_off} end={end_off}"
        );
        assert!(
            (start_off - f64::from(OFFSET_SECS)).abs() < 2.0,
            "start offset {start_off} expected ~{OFFSET_SECS}"
        );
        assert!(result.offsets_consistent);
        assert_eq!(result.end_aligned, Some(true));
    }

    /// Full 2-clip align on MKV/AAC unequal-length pair (shared-timeline end anchor).
    /// Clip length is 120 s so seek-boundary shortfall stays within the 95 % pad threshold.
    #[cfg(feature = "ffmpeg-tests")]
    #[test]
    fn symmetric_shared_timeline_end_clips_agree_on_unequal_mkv_aac_pair() {
        use crate::application::config::ChromaprintPreset;
        use crate::application::testing::audio_fixtures::write_anchored_end_symmetric_pair;
        use crate::infrastructure::chromaprint::{ChromaprintAligner, ChromaprintFingerprinter};
        use crate::infrastructure::symphonia::SymphoniaMediaReader;
        use crate::test_support::ffmpeg_util::{self, EncodeFormat};

        if !ffmpeg_util::ffmpeg_available() {
            eprintln!("skipping MKV/AAC anchored end alignment: ffmpeg unavailable");
            return;
        }

        const SHARED_SECS: u32 = 240;
        const LONG_SECS: u32 = 360;
        const OFFSET_SECS: u32 = 12;

        let temp = tempfile::tempdir().expect("tempdir");
        let (wav_a, wav_b) = write_anchored_end_symmetric_pair(
            temp.path(),
            11_025,
            SHARED_SECS,
            LONG_SECS,
            OFFSET_SECS,
        );
        let path_a = temp.path().join("a.mkv");
        let path_b = temp.path().join("b.mkv");
        assert!(
            ffmpeg_util::encode_audio(&wav_a, &path_a, EncodeFormat::MkvAac),
            "encode a.mkv failed"
        );
        assert!(
            ffmpeg_util::encode_audio(&wav_b, &path_b, EncodeFormat::MkvAac),
            "encode b.mkv failed"
        );

        let media_reader = SymphoniaMediaReader;
        let preset = ChromaprintPreset::default();
        let fingerprinter = ChromaprintFingerprinter::new(preset);
        let aligner = ChromaprintAligner::new(preset);
        let progress = FakeProgressReporter;
        let use_case = AlignVideos::new(
            &media_reader,
            &fingerprinter,
            &aligner,
            &RubatoResampler,
            &FftCorrelator,
            &ChromaprintClipRepetitionDetector,
            &progress,
        );
        let mut config = anchored_end_chromaprint_config(
            2,
            EndClipAnchor::SharedTimeline,
            AlignmentMode::Symmetric,
        );
        config.clip.clip_length = Duration::from_secs(120);
        let response = use_case
            .execute(AlignVideosRequest {
                video_a: path_a,
                video_b: path_b,
                config,
            })
            .expect("MKV/AAC anchored end chirp execute");

        let result = &response.result;
        assert_eq!(result.clips.len(), 2);
        let start = result
            .clips
            .iter()
            .find(|c| c.label == ClipLabel::Start)
            .expect("start clip");
        let end = result
            .clips
            .iter()
            .find(|c| c.label == ClipLabel::End)
            .expect("end clip");
        assert!(
            start.aligned && end.aligned,
            "both clips should align on MKV/AAC (start={} end={})",
            start.aligned,
            end.aligned
        );
        assert!(
            start.confidence >= 0.5 && end.confidence >= 0.5,
            "start={:.2} end={:.2}",
            start.confidence,
            end.confidence
        );
        let start_off = start.offset_secs.expect("start offset");
        let end_off = end.offset_secs.expect("end offset");
        assert!(
            (start_off - end_off).abs() <= OFFSET_AGREEMENT_TOLERANCE_SECS,
            "start={start_off} end={end_off}"
        );
        assert!(
            (start_off - f64::from(OFFSET_SECS)).abs() < 2.0,
            "start offset {start_off} expected ~{OFFSET_SECS}"
        );
        assert!(result.offsets_consistent);
        assert_eq!(result.end_aligned, Some(true));
    }

    #[test]
    fn symmetric_file_tail_end_clip_disagrees_on_unequal_pair() {
        const SHARED_SECS: u32 = 120;
        const LONG_SECS: u32 = 300;
        const OFFSET_SECS: u32 = 12;

        let response = run_anchored_end_chirp_alignment(
            SHARED_SECS,
            LONG_SECS,
            OFFSET_SECS,
            anchored_end_chromaprint_config(2, EndClipAnchor::FileTail, AlignmentMode::Symmetric),
        );
        let result = &response.result;
        assert_eq!(result.clips.len(), 2);
        let start_off = result
            .clips
            .iter()
            .find(|c| c.label == ClipLabel::Start)
            .and_then(|c| c.offset_secs)
            .expect("start offset");
        let end_off = result
            .clips
            .iter()
            .find(|c| c.label == ClipLabel::End)
            .and_then(|c| c.offset_secs);
        if let Some(end_off) = end_off {
            assert!(
                (start_off - end_off).abs() > OFFSET_AGREEMENT_TOLERANCE_SECS
                    || !result.offsets_consistent,
                "file-tail end should disagree with start (start={start_off} end={end_off})"
            );
        } else {
            assert_eq!(result.end_aligned, Some(false));
        }
    }

    #[test]
    fn auto_routes_unequal_anchored_pair_to_query_reference() {
        const SHARED_SECS: u32 = 120;
        const LONG_SECS: u32 = 600;
        const OFFSET_SECS: u32 = 12;

        let response = run_anchored_end_chirp_alignment(
            SHARED_SECS,
            LONG_SECS,
            OFFSET_SECS,
            anchored_end_chromaprint_config(2, EndClipAnchor::SharedTimeline, AlignmentMode::Auto),
        );
        assert_eq!(
            response.result.alignment_mode_used,
            Some(AlignmentModeUsed::QueryReference)
        );
        assert!(response.result.query_localization.is_some());
    }

    #[test]
    fn symmetric_three_clip_shared_timeline_offsets_consistent() {
        const SHARED_SECS: u32 = 180;
        const LONG_SECS: u32 = 600;
        const OFFSET_SECS: u32 = 12;

        let response = run_anchored_end_chirp_alignment(
            SHARED_SECS,
            LONG_SECS,
            OFFSET_SECS,
            anchored_end_chromaprint_config(
                3,
                EndClipAnchor::SharedTimeline,
                AlignmentMode::Symmetric,
            ),
        );
        let result = &response.result;
        assert_eq!(result.clips.len(), 3);
        assert!(result.offsets_consistent);
        let offsets: Vec<f64> = result
            .clips
            .iter()
            .filter_map(|clip| clip.offset_secs)
            .collect();
        assert_eq!(offsets.len(), 3);
        for offset in &offsets {
            assert!(
                (*offset - f64::from(OFFSET_SECS)).abs() < 2.5,
                "offset {offset} expected ~{OFFSET_SECS}"
            );
        }
    }
}
