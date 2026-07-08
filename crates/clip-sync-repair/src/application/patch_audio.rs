use std::collections::HashMap;
use std::time::Duration;

use clip_sync::{
    format_time_range_verbose, select_best_track, select_track_for_reference, ClipLabel, ClipWindow,
    DomainError, MediaReader, MediaSession, MediaSource, MultiChannelPcm, ProgressReporter,
    resample_interleaved,
};

use crate::application::patch_region::{
    derive_seam_gate_geometry, evaluate_seam_gate, retry_waveform_seam_extensions,
    SeamExtensionRetry, SeamGateConfig, SeamGateFailure, SeamGateOutcome, SeamGateParams,
};
use crate::application::error::RepairError;
use crate::domain::{
    fill_offset::{resolve_gap_offset_secs, AnchoredRetryPass, FillOffsetMode},
    diagnostics::{pcm_container_duration_skew, PCM_CONTAINER_WARN_SECS},
    fill_mode::FillMode,
    format_repair_profile_verbose,
    gap_extension_slack_secs,
    inactive_repair_flag_notes,
    gap_fill_fit::FillConfidence,
    gap_fill::{build_gap_fill_plan, format_align_fill_regions_phase, FillRegion, GapFillPlan},
    gap_fill_fit::{classify_fill_waveform_confidence, fit_fill_length_for_gap, fit_fill_to_gap_frames},
    gap_signature::build_gap_signature,
    gap_structure::StructureMatchParams,
    gap_tags::{
        derive_gap_tags_from_patch_outcome, derive_gap_tags_from_status,
        format_gap_tags_verbose_line, FillTierThresholds, GapPatchTierInput, GapTags,
        GapTagsPatchContext,
    },
    patch_anchor::{
        format_anchored_offset_verbose_line, format_patch_anchor_table_summary,
        interpolate_anchored_offset_secs, is_retryable_patch_skip, AnchorSearchPrior,
        PatchAnchorCandidate, PatchAnchorPolicy, PatchAnchorTable,
    },
    patch_result::{
        format_gap_fill_marginal_verbose_line, format_gap_fill_marginal_warn_reason,
        format_gap_fill_skip_verbose_line, format_gap_patch_skip_warn_reason,
        residual_summary_scalar_fields, GapFillSkipReason, GapPatchOutcome, GapPatchSkipReason,
        GapPatchStatus, PatchSummary,
    },
    policies::{self, GapBorderSpec, RefinedGapFrames},
    RepairPatchConfigView,
    Gap, GapReport,
};

pub struct PatchAudioResult {
    /// Present when A was decoded for patching; `None` when the fill plan was empty.
    pub pcm: Option<MultiChannelPcm>,
    pub summary: PatchSummary,
    /// Measured encoded bitrate of video A's selected audio track (bits/s).
    pub source_audio_bitrate_a_bps: Option<u32>,
    /// Measured encoded bitrate of video B's selected audio track (bits/s).
    pub source_audio_bitrate_b_bps: Option<u32>,
    /// Present when patched PCM length differs materially from the container duration.
    pub pcm_container_skew: Option<crate::domain::diagnostics::PcmContainerDurationSkew>,
}

pub struct PatchAudioRequest {
    pub report: GapReport,
    pub normalize_fill: bool,
    pub normalize_window_secs: f64,
    pub max_fill_gain_db: f64,
    /// Minimum normalized Pearson correlation at each gap seam (pre and post). Regions below
    /// this threshold on either seam are skipped.
    pub min_fill_correlation: f32,
    /// Extra B audio extracted on each side of the mapped gap window for boundary alignment.
    pub fill_align_margin_secs: f64,
    /// Maximum slide (seconds) applied when searching for the best B fill position.
    pub max_fill_align_adjustment_secs: f64,
    /// How far (seconds) to search in B for A's pre-gap border before local alignment.
    pub fill_border_search_secs: f64,
    /// Minimum border template length (seconds) for discovery/correlation on short gaps.
    pub min_border_discovery_secs: f64,
    /// A-side only: skip this much audio (seconds) immediately adjacent to the dropout when
    /// building border templates (avoids corrupted seam audio on A).
    pub border_standoff_secs: f64,
    /// Gaps at or below this length (seconds) pass when mean(pre, post) correlation meets the
    /// threshold instead of requiring both seams individually.
    pub short_gap_mean_correlation_secs: f64,
    /// How far B fill length may differ from A's scanned gap when locating the post-border.
    pub fill_length_slack_secs: f64,
    /// Seam correlation window (seconds) for fine align slide search and the fill gate.
    pub fill_seam_search_secs: f64,
    /// Seconds of A audio on each side of the gap used to build the structure signature.
    pub gap_signature_context_secs: f64,
    /// Bin width (milliseconds) for active/silent structure signatures.
    pub gap_signature_bin_ms: u64,
    /// Minimum active/silent pattern match score (0–1) at each seam before waveform gate.
    pub min_structure_match_score: f32,
    /// Both structure seam scores must meet this to skip the waveform Pearson gate.
    pub strong_structure_trust: f64,
    /// When true, always run the waveform Pearson seam gate.
    pub disable_structure_trust: bool,
    /// In the waveform gate path, soften Pearson threshold when structure scores meet this.
    pub partial_structure_waveform_soften: f64,
    /// Peak-amplitude floor for per-frame silence checks during gap refinement (matches scan).
    pub absolute_silence_rms: f32,
    /// How to map each gap on A to B (`recommended` vs drift-interpolated clip offsets).
    pub fill_offset_mode: crate::domain::FillOffsetMode,
    /// When waveform post-seam correlation fails, try extending the gap end on A in small steps.
    pub gap_end_extend_on_post_seam_fail: bool,
    /// When waveform pre-seam correlation fails, try extending the gap start on A in small steps.
    pub gap_start_extend_on_pre_seam_fail: bool,
    /// Maximum gap-end extension when retrying a failed post seam (milliseconds).
    pub gap_end_extend_max_ms: u64,
    /// Step size for gap-end extension retries (milliseconds).
    pub gap_end_extend_step_ms: u64,
    /// For short gaps, allow patch when mean(pre, post) fails but either seam meets the threshold.
    pub short_gap_one_strong_seam_fallback: bool,
    /// Gap-fill placement after structure match (`gate` legacy vs `fit` waveform search).
    pub fill_mode: crate::domain::FillMode,
    /// Unified fit structure weight (fit mode only).
    pub fill_fit_structure_weight: f64,
    /// Unified fit waveform weight (fit mode only).
    pub fill_fit_waveform_weight: f64,
    /// Scales distance-from-nominal penalty in unified fit structure scoring (1.0 = default).
    pub fill_fit_nominal_bias_scale: f64,
    /// Distance-from-nominal penalty scale applied when the resolved signature is energy
    /// (mode-coupled bias; defaults lower than the base scale).
    pub fill_fit_energy_nominal_bias_scale: f64,
    /// Scales late-start penalty when structure search starts after the nominal map (1.0 = default).
    pub fill_fit_late_start_penalty_scale: f64,
    /// Fit mode marginal patch band below `min_fill_correlation` (Phase C).
    pub fill_marginal_margin: f32,
    /// Fit mode hard waveform skip floor (Phase C).
    pub fill_absolute_floor: f32,
    /// Fit mode repeat-at-seam penalty weight (Phase D; 0 = off).
    pub fill_repeat_penalty_weight: f64,
    /// Minimum seam score for a pass-1 patch to become an offset anchor.
    pub fill_anchor_min_correlation: f32,
    /// Exclude structure-trusted gate patches from the anchor table.
    pub fill_anchor_exclude_structure_trusted: bool,
    /// Max `|align_adjustment|` as a fraction of `fill_border_search_secs` for anchors.
    pub fill_anchor_max_adjustment_frac: f64,
    /// Fit mode: soft penalty in unified search for B candidates far from anchor-predicted start (0 = off).
    pub fill_anchor_search_prior_weight: f64,
    /// `anchored_retry` pass 2: re-run fit-mode marginal pass-1 patches with anchored offset; keep pass 2 only when `High`.
    pub fill_anchor_retry_marginal: bool,
    /// Structure signature representation for gap fill search.
    pub gap_signature_mode: crate::domain::GapSignatureMode,
    /// Effective repair profile for verbose logging.
    pub profile: crate::domain::RepairProfile,
    /// Fit mode boundary search policy.
    pub fit_boundary_search: crate::domain::FitBoundarySearch,
    /// Editorial seam anchor search mode (fit mode only).
    pub anchor_seam_mode: crate::domain::AnchorSeamMode,
    pub max_anchor_bracket_secs: f64,
    pub max_anchors_per_side: usize,
    pub anchor_seam_min_prominence: f32,
    pub anchor_seam_min_match_pearson: f32,
    pub anchor_seam_min_xcorr_peak: f32,
    pub anchor_seam_xcorr_ambiguous_band: f32,
    /// P1 report-only: compute the residual/floor verdict per gap and attach it to the outcome/JSON.
    /// Off by default (no cost, no field); enabled for calibration runs. Set directly on the request.
    pub measure_residual: bool,
    /// **A3 dual-fit repair** (flag-gated). When on, a gap the seam gate *skips* (bracket-exhausted) gets a
    /// fallback attempt: independent per-shoulder fit + interior-trim fill, validated by the unchanged gate
    /// (§5.2). Off by default ⇒ existing bracket-search path byte-identical (D6). Set directly on the request.
    pub dual_fit: bool,
    pub residual_gate: crate::domain::ResidualGateMode,
    pub residual_floor_ok_db: f64,
    pub residual_headroom_margin_db: f64,
    pub residual_lag_secs: f64,
}

/// Patch parameters without the scan report — filled in after gap scan.
#[derive(Clone)]
pub struct PatchRequestSettings {
    pub normalize_fill: bool,
    /// A3 dual-fit repair fallback for bracket-exhausted skips (off by default).
    pub dual_fit: bool,
    pub normalize_window_secs: f64,
    pub max_fill_gain_db: f64,
    pub min_fill_correlation: f32,
    pub fill_align_margin_secs: f64,
    pub max_fill_align_adjustment_secs: f64,
    pub fill_border_search_secs: f64,
    pub min_border_discovery_secs: f64,
    pub border_standoff_secs: f64,
    pub short_gap_mean_correlation_secs: f64,
    pub fill_length_slack_secs: f64,
    pub fill_seam_search_secs: f64,
    pub gap_signature_context_secs: f64,
    pub gap_signature_bin_ms: u64,
    pub min_structure_match_score: f32,
    pub strong_structure_trust: f64,
    pub disable_structure_trust: bool,
    pub partial_structure_waveform_soften: f64,
    pub absolute_silence_rms: f32,
    pub fill_offset_mode: crate::domain::FillOffsetMode,
    pub gap_end_extend_on_post_seam_fail: bool,
    pub gap_start_extend_on_pre_seam_fail: bool,
    pub gap_end_extend_max_ms: u64,
    pub gap_end_extend_step_ms: u64,
    pub short_gap_one_strong_seam_fallback: bool,
    pub fill_mode: crate::domain::FillMode,
    pub fill_fit_structure_weight: f64,
    pub fill_fit_waveform_weight: f64,
    pub fill_fit_nominal_bias_scale: f64,
    pub fill_fit_energy_nominal_bias_scale: f64,
    pub fill_fit_late_start_penalty_scale: f64,
    pub fill_marginal_margin: f32,
    pub fill_absolute_floor: f32,
    pub fill_repeat_penalty_weight: f64,
    pub fill_anchor_min_correlation: f32,
    pub fill_anchor_exclude_structure_trusted: bool,
    pub fill_anchor_max_adjustment_frac: f64,
    pub fill_anchor_search_prior_weight: f64,
    pub fill_anchor_retry_marginal: bool,
    pub gap_signature_mode: crate::domain::GapSignatureMode,
    pub profile: crate::domain::RepairProfile,
    pub fit_boundary_search: crate::domain::FitBoundarySearch,
    pub anchor_seam_mode: crate::domain::AnchorSeamMode,
    pub max_anchor_bracket_secs: f64,
    pub max_anchors_per_side: usize,
    pub anchor_seam_min_prominence: f32,
    pub anchor_seam_min_match_pearson: f32,
    pub anchor_seam_min_xcorr_peak: f32,
    pub anchor_seam_xcorr_ambiguous_band: f32,
    pub residual_gate: crate::domain::ResidualGateMode,
    pub residual_floor_ok_db: f64,
    pub residual_headroom_margin_db: f64,
    pub residual_lag_secs: f64,
}

impl PatchRequestSettings {
    pub fn into_request(self, report: GapReport) -> PatchAudioRequest {
        PatchAudioRequest {
            report,
            normalize_fill: self.normalize_fill,
            normalize_window_secs: self.normalize_window_secs,
            max_fill_gain_db: self.max_fill_gain_db,
            min_fill_correlation: self.min_fill_correlation,
            fill_align_margin_secs: self.fill_align_margin_secs,
            max_fill_align_adjustment_secs: self.max_fill_align_adjustment_secs,
            fill_border_search_secs: self.fill_border_search_secs,
            min_border_discovery_secs: self.min_border_discovery_secs,
            border_standoff_secs: self.border_standoff_secs,
            short_gap_mean_correlation_secs: self.short_gap_mean_correlation_secs,
            fill_length_slack_secs: self.fill_length_slack_secs,
            fill_seam_search_secs: self.fill_seam_search_secs,
            gap_signature_context_secs: self.gap_signature_context_secs,
            gap_signature_bin_ms: self.gap_signature_bin_ms,
            min_structure_match_score: self.min_structure_match_score,
            strong_structure_trust: self.strong_structure_trust,
            disable_structure_trust: self.disable_structure_trust,
            partial_structure_waveform_soften: self.partial_structure_waveform_soften,
            absolute_silence_rms: self.absolute_silence_rms,
            fill_offset_mode: self.fill_offset_mode,
            gap_end_extend_on_post_seam_fail: self.gap_end_extend_on_post_seam_fail,
            gap_start_extend_on_pre_seam_fail: self.gap_start_extend_on_pre_seam_fail,
            gap_end_extend_max_ms: self.gap_end_extend_max_ms,
            gap_end_extend_step_ms: self.gap_end_extend_step_ms,
            short_gap_one_strong_seam_fallback: self.short_gap_one_strong_seam_fallback,
            fill_mode: self.fill_mode,
            fill_fit_structure_weight: self.fill_fit_structure_weight,
            fill_fit_waveform_weight: self.fill_fit_waveform_weight,
            fill_fit_nominal_bias_scale: self.fill_fit_nominal_bias_scale,
            fill_fit_energy_nominal_bias_scale: self.fill_fit_energy_nominal_bias_scale,
            fill_fit_late_start_penalty_scale: self.fill_fit_late_start_penalty_scale,
            fill_marginal_margin: self.fill_marginal_margin,
            fill_absolute_floor: self.fill_absolute_floor,
            fill_repeat_penalty_weight: self.fill_repeat_penalty_weight,
            fill_anchor_min_correlation: self.fill_anchor_min_correlation,
            fill_anchor_exclude_structure_trusted: self.fill_anchor_exclude_structure_trusted,
            fill_anchor_max_adjustment_frac: self.fill_anchor_max_adjustment_frac,
            fill_anchor_search_prior_weight: self.fill_anchor_search_prior_weight,
            fill_anchor_retry_marginal: self.fill_anchor_retry_marginal,
            gap_signature_mode: self.gap_signature_mode,
            profile: self.profile,
            fit_boundary_search: self.fit_boundary_search,
            anchor_seam_mode: self.anchor_seam_mode,
            max_anchor_bracket_secs: self.max_anchor_bracket_secs,
            max_anchors_per_side: self.max_anchors_per_side,
            anchor_seam_min_prominence: self.anchor_seam_min_prominence,
            anchor_seam_min_match_pearson: self.anchor_seam_min_match_pearson,
            anchor_seam_min_xcorr_peak: self.anchor_seam_min_xcorr_peak,
            anchor_seam_xcorr_ambiguous_band: self.anchor_seam_xcorr_ambiguous_band,
            // Report-only residual measurement is opt-in; callers set it on the request directly.
            measure_residual: false,
            dual_fit: self.dual_fit,
            residual_gate: self.residual_gate,
            residual_floor_ok_db: self.residual_floor_ok_db,
            residual_headroom_margin_db: self.residual_headroom_margin_db,
            residual_lag_secs: self.residual_lag_secs,
        }
    }
}

/// How far gap edges may be adjusted against A's decoded PCM (seconds).
const GAP_EDGE_REFINE_SECS: f64 = 0.75;

// Collected B segment ready to splice into A.
#[derive(Debug, PartialEq)]
struct RegionPatch {
    b_samples: Vec<f32>,
    gain: f32,
    a_start_frame: usize,
    a_end_frame: usize,
    crossfade_secs: f64,
}

pub struct PatchAudio<'r, MR: MediaReader> {
    media_reader: &'r MR,
    progress: &'r dyn ProgressReporter,
}

impl<'r, MR: MediaReader> PatchAudio<'r, MR> {
    pub fn new(media_reader: &'r MR, progress: &'r dyn ProgressReporter) -> Self {
        Self {
            media_reader,
            progress,
        }
    }

    pub fn execute(
        &self,
        request: PatchAudioRequest,
        crossfade_ms: u64,
    ) -> Result<PatchAudioResult, RepairError> {
        // Step 1: Build fill plan (may be empty when tracks mismatch or no B energy).
        let plan = build_gap_fill_plan(&request.report, crossfade_ms);

        if plan.regions.is_empty() {
            self.progress
                .phase("No gaps planned for patch; skipping audio decode.");
            let summary = PatchSummary::from_outcomes(outcomes_in_report_order(
                &request.report.gaps,
                &plan,
                &[],
                request.fill_mode,
                FillTierThresholds {
                    min_fill_correlation: request.min_fill_correlation,
                    fill_marginal_margin: request.fill_marginal_margin,
                    fill_absolute_floor: request.fill_absolute_floor,
                },
            ));
            return Ok(PatchAudioResult {
                pcm: None,
                summary,
                source_audio_bitrate_a_bps: None,
                source_audio_bitrate_b_bps: None,
                pcm_container_skew: None,
            });
        }

        let _patch_audio_span = tracing::info_span!(
            "patch_audio",
            region_count = plan.regions.len(),
            fill_mode = ?request.fill_mode,
            fill_offset_mode = ?request.fill_offset_mode,
        )
        .entered();

        self.progress.phase_verbose(&format_repair_profile_verbose(
            request.profile,
            request.fit_boundary_search,
            request.fill_border_search_secs,
            request.gap_end_extend_on_post_seam_fail,
            request.gap_start_extend_on_pre_seam_fail,
        ));
        let patch_config_view = repair_patch_config_view(&request);
        for note in inactive_repair_flag_notes(patch_config_view) {
            self.progress
                .phase_verbose(&format!("repair note: {note}"));
        }

        // Steps 2–6: decode A + B (shared with the gap-fingerprint diagnostic).
        let DecodedAb {
            mut a_pcm,
            b_samples_full,
            source_audio_bitrate_a_bps,
            source_audio_bitrate_b_bps,
            container_duration_a_secs,
        } = decode_ab(self.media_reader, &request.report, self.progress)?;

        // Step 7: Compute global A RMS as normalization fallback.
        let global_a_rms = policies::rms_interleaved(&a_pcm.samples);

        let channels = a_pcm.channels as usize;
        let sample_rate = a_pcm.sample_rate;
        let max_refine_frames =
            (GAP_EDGE_REFINE_SECS * sample_rate as f64).round() as usize;
        let region_ctx = RegionPatchContext {
            channels,
            sample_rate,
            max_refine_frames,
            global_a_rms,
            silence_peak_fraction: request.report.silence_peak_fraction,
        };

        // Step 8: Collect B segments (immutable borrow on a_pcm.samples),
        // then apply them in a separate pass (mutable borrow).
        let mut patches: Vec<RegionPatch> = Vec::new();
        let mut patch_slot_by_gap: Vec<Option<usize>> = Vec::new();
        let mut region_results: Vec<(f64, f64, RegionPatchOutcome, GapTags)> = Vec::new();
        let region_count = plan.regions.len() as u64;

        self.progress
            .phase(&format_align_fill_regions_phase(&plan));

        for (index, region) in plan.regions.iter().enumerate() {
            let gap_num = index as u64 + 1;
            self.progress.progress("patch-gap", gap_num, region_count);
            self.progress.phase_verbose(&format!(
                "  gap {gap_num}/{region_count}: A {}",
                format_time_range_verbose(region.a_start_secs, region.a_end_secs)
            ));

            let gap_span = tracing::info_span!(
                "patch_gap",
                gap_index = gap_num,
                region_count,
                a_start_secs = region.a_start_secs,
                a_end_secs = region.a_end_secs,
                fill_mode = ?request.fill_mode,
                anchored_retry = false,
                outcome = tracing::field::Empty,
                confidence = tracing::field::Empty,
                skip_reason = tracing::field::Empty,
                boundary_grid = tracing::field::Empty,
                grid_cells = tracing::field::Empty,
            );
            let _gap_enter = gap_span.enter();

            let (patch, outcome, tag_ctx) = prepare_region_patch(
                self.progress,
                &RegionPatchMedia {
                    b_samples_full: &b_samples_full,
                    a_pcm: &a_pcm,
                },
                region,
                &request,
                &region_ctx,
                RegionPatchOpts {
                    anchored_retry_pass: AnchoredRetryPass::First,
                    patch_anchors: None,
                },
            );
            let tags = region_outcome_gap_tags(&outcome, tag_ctx);
            log_gap_tags_verbose(self.progress, &tags);
            record_patch_gap_span(&gap_span, &outcome);
            region_results.push((region.a_start_secs, region.a_end_secs, outcome, tags));
            if let Some(patch) = patch {
                let slot = patches.len();
                patches.push(patch);
                patch_slot_by_gap.push(Some(slot));
            } else {
                patch_slot_by_gap.push(None);
            }
        }

        let patch_anchors_used = if request.fill_offset_mode == FillOffsetMode::AnchoredRetry {
            let candidates =
                build_patch_anchor_candidates(&request, &plan.regions, &region_results);
            let table =
                PatchAnchorTable::from_candidates(&candidates, &patch_anchor_policy(&request));
            if table.is_empty() {
                None
            } else {
                self.progress
                    .phase_verbose(&format_patch_anchor_table_summary(&table));
                let _anchored_retry = tracing::info_span!(
                    "patch_anchored_retry",
                    anchor_count = table.anchors.len(),
                    fill_mode = ?request.fill_mode,
                )
                .entered();
                run_anchored_retry_pass(
                    self.progress,
                    &mut AnchoredRetryState {
                        patches: &mut patches,
                        patch_slot_by_gap: &mut patch_slot_by_gap,
                        region_results: &mut region_results,
                    },
                    &plan.regions,
                    &request,
                    &region_ctx,
                    &RegionPatchMedia {
                        b_samples_full: &b_samples_full,
                        a_pcm: &a_pcm,
                    },
                    &table,
                );
                Some(table.to_reports())
            }
        } else {
            None
        };

        // Step 9: Apply patches to A samples.
        let patch_count = patches.len() as u64;
        if patch_count > 0 {
            self.progress.phase(&format!("Splicing {patch_count} fill(s) into timeline..."));
        }
        if patch_count > 0 {
            let _splice_span = tracing::info_span!("patch_splice", patch_count).entered();
            for (index, patch) in patches.iter().enumerate() {
                self.progress.progress("patch-splice", index as u64 + 1, patch_count);
                let b_gained: Vec<f32> = patch
                    .b_samples
                    .iter()
                    .map(|&s| (s * patch.gain).clamp(-1.0, 1.0))
                    .collect();

                splice_into_a(
                    &mut a_pcm.samples,
                    &b_gained,
                    channels,
                    patch.a_start_frame,
                    patch.a_end_frame,
                    patch.crossfade_secs,
                    sample_rate,
                );
            }
        }

        let thresholds = FillTierThresholds {
            min_fill_correlation: request.min_fill_correlation,
            fill_marginal_margin: request.fill_marginal_margin,
            fill_absolute_floor: request.fill_absolute_floor,
        };
        let mut summary = PatchSummary::from_outcomes(outcomes_in_report_order(
            &request.report.gaps,
            &plan,
            &region_results,
            request.fill_mode,
            thresholds,
        ));
        if let Some(anchors) = patch_anchors_used {
            summary = summary.with_patch_anchors(anchors);
        }

        let container_secs = container_duration_a_secs;
        let pcm_secs = a_pcm.frames() as f64 / f64::from(a_pcm.sample_rate);
        let pcm_container_skew = {
            let skew = pcm_container_duration_skew(pcm_secs, container_secs);
            if skew.delta_secs > PCM_CONTAINER_WARN_SECS {
                tracing::warn!(
                    pcm_secs,
                    container_secs,
                    delta_secs = skew.delta_secs,
                    "patched PCM length differs from container duration"
                );
                Some(skew)
            } else {
                None
            }
        };

        Ok(PatchAudioResult {
            pcm: Some(a_pcm),
            summary,
            source_audio_bitrate_a_bps,
            source_audio_bitrate_b_bps,
            pcm_container_skew,
        })
    }
}

#[derive(Debug, PartialEq)]
enum RegionPatchOutcome {
    Patched {
        pre_correlation: f64,
        post_correlation: f64,
        align_adjustment_secs: f64,
        waveform_adjustment_secs: f64,
        structure_trusted: bool,
        confidence: FillConfidence,
        gap_start_adjust_frames: i64,
        gap_end_adjust_frames: i64,
        fit_used_boundary_grid: bool,
        fit_boundary_grid_cells: Option<u32>,
        residual: Option<policies::SeamResidualVerdict>,
        anchor_seam_used: bool,
        anchor_bracket_move_frames: usize,
        dual_fit_used: bool,
    },
    Skipped {
        reason: GapPatchSkipReason,
        residual: Option<policies::SeamResidualVerdict>,
    },
}

/// Full-track decoded A/B for the repair fill path (and the gap-fingerprint diagnostic).
pub(crate) struct DecodedAb {
    pub a_pcm: MultiChannelPcm,
    /// B resampled to A's rate (interleaved).
    pub b_samples_full: Vec<f32>,
    pub source_audio_bitrate_a_bps: Option<u32>,
    pub source_audio_bitrate_b_bps: Option<u32>,
    /// A track's container-reported duration (for PCM-vs-container skew).
    pub container_duration_a_secs: f64,
}

/// Open A and B, select tracks, decode both full timelines, and resample B to A's rate. Extracted
/// verbatim from `PatchAudio::run` (steps 2–6) so the fingerprint diagnostic decodes identically.
pub(crate) fn decode_ab<MR: MediaReader>(
    media_reader: &MR,
    report: &GapReport,
    progress: &dyn ProgressReporter,
) -> Result<DecodedAb, RepairError> {
    // Step 2: Open A, select best track, get duration.
    let source_a = MediaSource::new(report.video_a.clone());
    let mut session_a = media_reader.open(&source_a).map_err(RepairError::Media)?;
    let tracks_a = session_a.list_tracks().map_err(RepairError::Media)?;
    let track_a = select_best_track(&tracks_a)?.clone();
    let duration_a = track_a
        .duration
        .ok_or(RepairError::Domain(DomainError::InvalidDuration))?;

    // Step 3: Extract full A timeline.
    let full_window_a = ClipWindow::new(Duration::ZERO, duration_a, ClipLabel::Interior);
    let a_pcm = {
        let _decode_a = tracing::info_span!(
            "patch_decode_a",
            path = %report.video_a.display(),
            duration_secs = duration_a.as_secs_f64(),
            channels = track_a.channels,
            sample_rate = track_a.sample_rate,
            bit_depth = ?track_a.bit_depth,
        )
        .entered();
        session_a
            .extract_interleaved(&track_a, &full_window_a, progress, "patch-a")
            .map_err(RepairError::Media)?
    };
    let source_audio_bitrate_a_bps = a_pcm.measured_bitrate_bps();

    // Step 5: Open B, select best track.
    let source_b = MediaSource::new(report.video_b.clone());
    let mut session_b = media_reader.open(&source_b).map_err(RepairError::Media)?;
    let tracks_b = session_b.list_tracks().map_err(RepairError::Media)?;
    let track_b = select_track_for_reference(&track_a, &tracks_b)?.clone();

    // Step 6: Decode full B timeline once (sequential from t=0) to avoid per-gap MKV seeks.
    let duration_b = track_b
        .duration
        .ok_or(RepairError::Domain(DomainError::InvalidDuration))?;
    let full_window_b = ClipWindow::new(Duration::ZERO, duration_b, ClipLabel::Interior);
    let b_pcm_full = {
        let _decode_b = tracing::info_span!(
            "patch_decode_b",
            path = %report.video_b.display(),
            duration_secs = duration_b.as_secs_f64(),
            channels = track_b.channels,
            sample_rate = track_b.sample_rate,
            bit_depth = ?track_b.bit_depth,
        )
        .entered();
        session_b
            .extract_interleaved(&track_b, &full_window_b, progress, "patch-b")
            .map_err(RepairError::Media)?
    };
    let source_audio_bitrate_b_bps = b_pcm_full.measured_bitrate_bps();
    let b_samples_full = if b_pcm_full.sample_rate != a_pcm.sample_rate {
        let _resample = tracing::debug_span!(
            "patch_resample_b",
            from_rate = b_pcm_full.sample_rate,
            to_rate = a_pcm.sample_rate,
        )
        .entered();
        resample_interleaved(
            &b_pcm_full.samples,
            b_pcm_full.channels,
            b_pcm_full.sample_rate,
            a_pcm.sample_rate,
        )
    } else {
        b_pcm_full.samples
    };

    Ok(DecodedAb {
        a_pcm,
        b_samples_full,
        source_audio_bitrate_a_bps,
        source_audio_bitrate_b_bps,
        container_duration_a_secs: duration_a.as_secs_f64(),
    })
}

fn skipped_patch(reason: GapPatchSkipReason) -> RegionPatchOutcome {
    RegionPatchOutcome::Skipped {
        reason,
        residual: None,
    }
}

fn skipped_patch_with_residual(
    reason: GapPatchSkipReason,
    residual: Option<policies::SeamResidualVerdict>,
) -> RegionPatchOutcome {
    RegionPatchOutcome::Skipped { reason, residual }
}

fn record_patch_gap_span(span: &tracing::Span, outcome: &RegionPatchOutcome) {
    match outcome {
        RegionPatchOutcome::Patched {
            confidence,
            fit_used_boundary_grid,
            fit_boundary_grid_cells,
            ..
        } => {
            span.record("outcome", "patched");
            span.record(
                "confidence",
                match confidence {
                    FillConfidence::High => "high",
                    FillConfidence::Marginal => "marginal",
                },
            );
            span.record("boundary_grid", *fit_used_boundary_grid);
            if let Some(cells) = fit_boundary_grid_cells {
                span.record("grid_cells", cells);
            }
        }
        RegionPatchOutcome::Skipped { reason, .. } => {
            span.record("outcome", "skipped");
            span.record("skip_reason", format!("{reason:?}"));
        }
    }
}

fn gap_key(start_secs: f64, end_secs: f64) -> (u64, u64) {
    (start_secs.to_bits(), end_secs.to_bits())
}

fn fill_offset_mode_label(mode: FillOffsetMode) -> &'static str {
    match mode {
        FillOffsetMode::Recommended => "recommended",
        FillOffsetMode::Interpolated => "interpolated",
        FillOffsetMode::Anchored => "anchored",
        FillOffsetMode::AnchoredRetry => "anchored_retry",
    }
}

fn patch_anchor_policy(request: &PatchAudioRequest) -> PatchAnchorPolicy {
    PatchAnchorPolicy {
        min_correlation: request.fill_anchor_min_correlation,
        exclude_structure_trusted: request.fill_anchor_exclude_structure_trusted,
        max_adjustment_frac: request.fill_anchor_max_adjustment_frac,
        border_search_secs: request.fill_border_search_secs,
    }
}

fn anchor_search_prior_for_gap(
    request: &PatchAudioRequest,
    patch_anchors: Option<&PatchAnchorTable>,
    anchored_retry_pass: AnchoredRetryPass,
    region: &FillRegion,
    b_extract_start_secs: f64,
    search_radius_frames: usize,
    sample_rate: u32,
) -> Option<AnchorSearchPrior> {
    if request.fill_mode != FillMode::Fit || request.fill_anchor_search_prior_weight <= 0.0 {
        return None;
    }
    let table = patch_anchors.filter(|t| !t.is_empty())?;
    if anchored_retry_pass != AnchoredRetryPass::Second
        && request.fill_offset_mode != FillOffsetMode::Anchored
    {
        return None;
    }
    let gap_time_on_a = (region.a_start_secs + region.a_end_secs) / 2.0;
    let predicted_offset = interpolate_anchored_offset_secs(
        &request.report.alignment,
        gap_time_on_a,
        table,
    )?;
    let predicted_b_start = region.a_start_secs + predicted_offset;
    let predicted_start_frame = ((predicted_b_start - b_extract_start_secs) * sample_rate as f64)
        .round()
        .max(0.0) as usize;
    Some(AnchorSearchPrior {
        predicted_start_frame,
        weight: request.fill_anchor_search_prior_weight,
        search_radius_frames,
    })
}

fn build_patch_anchor_candidates(
    request: &PatchAudioRequest,
    regions: &[FillRegion],
    region_results: &[(f64, f64, RegionPatchOutcome, GapTags)],
) -> Vec<PatchAnchorCandidate> {
    regions
        .iter()
        .zip(region_results.iter())
        .enumerate()
        .filter_map(|(gap_index, (region, (_, _, outcome, _)))| {
            let RegionPatchOutcome::Patched {
                pre_correlation,
                post_correlation,
                align_adjustment_secs,
                structure_trusted,
                confidence,
                ..
            } = outcome
            else {
                return None;
            };
            let gap_time_on_a = (region.a_start_secs + region.a_end_secs) / 2.0;
            let base_gap_offset_secs = resolve_gap_offset_secs(
                &request.report.alignment,
                request.fill_offset_mode,
                gap_time_on_a,
                None,
                AnchoredRetryPass::First,
            )
            .unwrap_or(region.b_start_secs - region.a_start_secs);
            Some(PatchAnchorCandidate {
                gap_index,
                a_start_secs: region.a_start_secs,
                a_end_secs: region.a_end_secs,
                base_gap_offset_secs,
                align_adjustment_secs: *align_adjustment_secs,
                pre_correlation: *pre_correlation,
                post_correlation: *post_correlation,
                structure_trusted: *structure_trusted,
                confidence: *confidence,
            })
        })
        .collect()
}

fn anchored_retry_gap_indices(
    region_results: &[(f64, f64, RegionPatchOutcome, GapTags)],
    retry_marginal: bool,
) -> Vec<usize> {
    region_results
        .iter()
        .enumerate()
        .filter_map(|(index, (_, _, outcome, _))| {
            let retry = match outcome {
                RegionPatchOutcome::Skipped { reason, .. } => is_retryable_patch_skip(reason),
                RegionPatchOutcome::Patched {
                    confidence: FillConfidence::Marginal,
                    ..
                } => retry_marginal,
                _ => false,
            };
            retry.then_some(index)
        })
        .collect()
}

fn should_apply_anchored_retry_outcome(
    prior: &RegionPatchOutcome,
    new: &RegionPatchOutcome,
) -> bool {
    match (prior, new) {
        (_, RegionPatchOutcome::Skipped { .. }) => false,
        (RegionPatchOutcome::Skipped { .. }, RegionPatchOutcome::Patched { .. }) => true,
        (
            RegionPatchOutcome::Patched {
                confidence: FillConfidence::Marginal,
                ..
            },
            RegionPatchOutcome::Patched {
                confidence: FillConfidence::High,
                ..
            },
        ) => true,
        _ => false,
    }
}

fn store_anchored_retry_patch(
    patches: &mut Vec<RegionPatch>,
    patch_slot_by_gap: &mut [Option<usize>],
    gap_index: usize,
    patch: RegionPatch,
) {
    if let Some(slot) = patch_slot_by_gap
        .get_mut(gap_index)
        .and_then(|slot| slot.as_mut())
    {
        patches[*slot] = patch;
    } else {
        let slot = patches.len();
        patches.push(patch);
        patch_slot_by_gap[gap_index] = Some(slot);
    }
}

struct AnchoredRetryState<'a> {
    patches: &'a mut Vec<RegionPatch>,
    patch_slot_by_gap: &'a mut [Option<usize>],
    region_results: &'a mut [(f64, f64, RegionPatchOutcome, GapTags)],
}

struct RegionPatchMedia<'a> {
    b_samples_full: &'a [f32],
    a_pcm: &'a MultiChannelPcm,
}

struct RegionPatchOpts<'a> {
    anchored_retry_pass: AnchoredRetryPass,
    patch_anchors: Option<&'a PatchAnchorTable>,
}

fn run_anchored_retry_pass(
    progress: &dyn ProgressReporter,
    state: &mut AnchoredRetryState<'_>,
    regions: &[FillRegion],
    request: &PatchAudioRequest,
    ctx: &RegionPatchContext,
    media: &RegionPatchMedia<'_>,
    table: &PatchAnchorTable,
) {
    let retry_marginal =
        request.fill_anchor_retry_marginal && request.fill_mode == FillMode::Fit;
    let retry_indices = anchored_retry_gap_indices(state.region_results, retry_marginal);
    if retry_indices.is_empty() {
        return;
    }

    progress.phase_verbose(&format!(
        "  anchored retry: {} gap(s) using {} offset anchor(s)",
        retry_indices.len(),
        table.anchors.len()
    ));

    for index in retry_indices {
        let region = &regions[index];
        let gap_num = index + 1;
        let prior = &state.region_results[index].2;
        let retry_label = if matches!(
            prior,
            RegionPatchOutcome::Patched {
                confidence: FillConfidence::Marginal,
                ..
            }
        ) {
            "marginal upgrade"
        } else {
            "retry"
        };
        progress.phase_verbose(&format!(
            "  anchored {retry_label} gap {gap_num}: A {}",
            format_time_range_verbose(region.a_start_secs, region.a_end_secs)
        ));
        let gap_span = tracing::info_span!(
            "patch_gap",
            gap_index = gap_num,
            region_count = regions.len() as u64,
            a_start_secs = region.a_start_secs,
            a_end_secs = region.a_end_secs,
            fill_mode = ?request.fill_mode,
            anchored_retry = true,
            outcome = tracing::field::Empty,
            confidence = tracing::field::Empty,
            skip_reason = tracing::field::Empty,
            boundary_grid = tracing::field::Empty,
            grid_cells = tracing::field::Empty,
        );
        let _gap_enter = gap_span.enter();
        let (patch, outcome, tag_ctx) = prepare_region_patch(
            progress,
            media,
            region,
            request,
            ctx,
            RegionPatchOpts {
                anchored_retry_pass: AnchoredRetryPass::Second,
                patch_anchors: Some(table),
            },
        );
        let tags = region_outcome_gap_tags(&outcome, tag_ctx);
        log_gap_tags_verbose(progress, &tags);
        record_patch_gap_span(&gap_span, &outcome);
        if should_apply_anchored_retry_outcome(prior, &outcome) {
            state.region_results[index].2 = outcome;
            state.region_results[index].3 = tags;
            if let Some(patch) = patch {
                store_anchored_retry_patch(
                    state.patches,
                    state.patch_slot_by_gap,
                    index,
                    patch,
                );
            }
        }
    }
}

/// Per-gap A/B timeline fields for verbose fill planning logs.
pub(crate) struct GapFillPlanLog<'a> {
    pub scan_a_start_secs: f64,
    pub scan_a_end_secs: f64,
    pub refined_a_start_secs: f64,
    pub refined_a_end_secs: f64,
    pub gap_offset_secs: f64,
    pub fill_offset_mode: FillOffsetMode,
    pub mapped_b_start_secs: f64,
    pub mapped_b_end_secs: f64,
    pub b_search_start_secs: f64,
    pub b_search_end_secs: f64,
    pub signature_mode_label: &'a str,
}

/// B fill placement and slide metadata for verbose result logs.
pub(crate) struct GapFillResultLog {
    pub b_search_start_secs: f64,
    pub sample_rate: u32,
    pub channels: usize,
    pub fill_start_sample: usize,
    pub fill_end_sample: usize,
    pub structure_slide_secs: f64,
    pub waveform_slide_secs: f64,
    pub fit_used_boundary_grid: bool,
    pub fit_boundary_grid_cells: Option<u32>,
    pub fit_haystack_secs: f64,
    pub report_pre: f64,
    pub report_post: f64,
    pub confidence: FillConfidence,
}

/// Verbose stderr lines: per-gap A/B timeline used for structure search and fill.
pub(crate) fn format_gap_fill_plan_lines(plan: &GapFillPlanLog<'_>) -> Vec<String> {
    let mut lines = vec![format!(
        "           fill offset {:+.3}s ({})",
        plan.gap_offset_secs,
        fill_offset_mode_label(plan.fill_offset_mode),
    )];
    if (plan.refined_a_start_secs - plan.scan_a_start_secs).abs() > 0.001
        || (plan.refined_a_end_secs - plan.scan_a_end_secs).abs() > 0.001
    {
        lines.push(format!(
            "           A gap (refined): {}",
            format_time_range_verbose(plan.refined_a_start_secs, plan.refined_a_end_secs)
        ));
    }
    lines.push(format!(
        "           B gap (mapped): {}",
        format_time_range_verbose(plan.mapped_b_start_secs, plan.mapped_b_end_secs)
    ));
    lines.push(format!(
        "           B search window: {}",
        format_time_range_verbose(plan.b_search_start_secs, plan.b_search_end_secs)
    ));
    lines.push(format!(
        "           signature_mode={}",
        plan.signature_mode_label
    ));
    lines
}

pub(crate) fn format_gap_fill_result_line(result: &GapFillResultLog) -> String {
    let ch = result.channels.max(1);
    let to_secs = |sample: usize| {
        sample as f64 / ch as f64 / f64::from(result.sample_rate)
    };
    let fill_start = result.b_search_start_secs + to_secs(result.fill_start_sample);
    let fill_end = result.b_search_start_secs + to_secs(result.fill_end_sample);
    let mut slide = format!("structure slide {:+.3}s", result.structure_slide_secs);
    if result.waveform_slide_secs.abs() > 0.000_5 {
        slide.push_str(&format!(
            ", waveform slide {:+.3}s",
            result.waveform_slide_secs
        ));
    }
    let fit_path = if result.fit_used_boundary_grid {
        if let Some(cells) = result.fit_boundary_grid_cells {
            format!(
                "boundary grid ({cells} cells, haystack {:.1}s)",
                result.fit_haystack_secs
            )
        } else {
            "boundary grid".to_string()
        }
    } else if result.confidence == FillConfidence::Marginal {
        format!(
            "baseline only (marginal, pre={:.2} post={:.2})",
            result.report_pre, result.report_post
        )
    } else {
        "baseline only".to_string()
    };
    format!(
        "           B fill source: {} ({slide}; fit path: {fit_path})",
        format_time_range_verbose(fill_start, fill_end),
    )
}

fn log_gap_fill_plan_verbose(progress: &dyn ProgressReporter, plan: &GapFillPlanLog<'_>) {
    for line in format_gap_fill_plan_lines(plan) {
        progress.phase_verbose(&line);
    }
}

fn log_gap_fill_result_verbose(progress: &dyn ProgressReporter, result: &GapFillResultLog) {
    progress.phase_verbose(&format_gap_fill_result_line(result));
}

/// Human-readable skip line for stderr (`tracing::warn`) matching the stdout gap table.
pub(crate) fn format_skip_gap_fill_log(
    gaps: &[Gap],
    a_start_secs: f64,
    a_end_secs: f64,
    reason: &str,
) -> String {
    let total = gaps.len();
    let range = format_time_range_verbose(a_start_secs, a_end_secs);
    if let Some(index) = gaps.iter().position(|gap| {
        gap_key(gap.video_a_start_secs, gap.video_a_end_secs) == gap_key(a_start_secs, a_end_secs)
    }) {
        format!("gap {index}/{total} ({range}): {reason}", index = index + 1)
    } else {
        format!("gap ({range}): {reason}")
    }
}

fn log_skip_gap_fill(
    progress: &dyn ProgressReporter,
    gaps: &[Gap],
    a_start_secs: f64,
    a_end_secs: f64,
    reason: &GapPatchSkipReason,
) {
    progress.flush_progress();
    if progress.detailed_extraction_progress() {
        progress.phase_verbose(&format_gap_fill_skip_verbose_line(reason));
    } else {
        tracing::warn!(
            "{}",
            format_skip_gap_fill_log(
                gaps,
                a_start_secs,
                a_end_secs,
                &format_gap_patch_skip_warn_reason(reason),
            )
        );
    }
}

struct MarginalGapFillLog<'a> {
    gaps: &'a [Gap],
    a_start_secs: f64,
    a_end_secs: f64,
    pre: f64,
    post: f64,
    min: f32,
    anchor_seam: bool,
}

fn log_marginal_gap_fill(progress: &dyn ProgressReporter, log: &MarginalGapFillLog<'_>) {
    progress.flush_progress();
    if progress.detailed_extraction_progress() {
        progress.phase_verbose(&format_gap_fill_marginal_verbose_line(
            log.pre,
            log.post,
            log.min,
            log.anchor_seam,
        ));
    } else {
        tracing::warn!(
            "{}",
            format_skip_gap_fill_log(
                log.gaps,
                log.a_start_secs,
                log.a_end_secs,
                &format_gap_fill_marginal_warn_reason(
                    log.pre,
                    log.post,
                    log.min,
                    log.anchor_seam,
                ),
            )
        );
    }
}

fn outcomes_in_report_order(
    gaps: &[Gap],
    plan: &GapFillPlan,
    region_results: &[(f64, f64, RegionPatchOutcome, GapTags)],
    fill_mode: FillMode,
    thresholds: FillTierThresholds,
) -> Vec<GapPatchOutcome> {
    let mut status_by_gap: HashMap<(u64, u64), GapPatchStatus> = HashMap::new();
    let mut tags_by_gap: HashMap<(u64, u64), GapTags> = HashMap::new();
    let mut residual_by_gap: HashMap<(u64, u64), policies::SeamResidualVerdict> = HashMap::new();

    for skip in &plan.skipped {
        let key = gap_key(skip.a_start_secs, skip.a_end_secs);
        let status = GapPatchStatus::NotPlanned {
            reason: skip.reason.clone(),
        };
        let tags = derive_gap_tags_from_status(&status, fill_mode, thresholds);
        status_by_gap.insert(key, status);
        tags_by_gap.insert(key, tags);
    }

    for (a_start, a_end, outcome, tags) in region_results {
        let status = match outcome {
            RegionPatchOutcome::Patched {
                pre_correlation,
                post_correlation,
                align_adjustment_secs,
                waveform_adjustment_secs,
                structure_trusted,
                confidence,
                gap_start_adjust_frames,
                gap_end_adjust_frames,
                residual,
                anchor_seam_used,
                anchor_bracket_move_frames,
                dual_fit_used,
                ..
            } => {
                if let Some(verdict) = residual {
                    residual_by_gap.insert(gap_key(*a_start, *a_end), *verdict);
                }
                let (residual_db, floor_db, headroom_db) =
                    residual_summary_scalar_fields(residual.as_ref());
                GapPatchStatus::Patched {
                    pre_correlation: *pre_correlation,
                    post_correlation: *post_correlation,
                    align_adjustment_secs: *align_adjustment_secs,
                    waveform_adjustment_secs: *waveform_adjustment_secs,
                    structure_trusted: *structure_trusted,
                    confidence: *confidence,
                    gap_start_adjust_frames: *gap_start_adjust_frames,
                    gap_end_adjust_frames: *gap_end_adjust_frames,
                    residual_db,
                    floor_db,
                    headroom_db,
                    anchor_seam_used: *anchor_seam_used,
                    anchor_bracket_move_frames: *anchor_bracket_move_frames,
                    dual_fit_used: *dual_fit_used,
                }
            }
            RegionPatchOutcome::Skipped { reason, residual } => {
                if let Some(verdict) = residual {
                    residual_by_gap.insert(gap_key(*a_start, *a_end), *verdict);
                }
                GapPatchStatus::Skipped {
                    reason: reason.clone(),
                }
            }
        };
        let key = gap_key(*a_start, *a_end);
        status_by_gap.insert(key, status);
        tags_by_gap.insert(key, tags.clone());
    }

    gaps
        .iter()
        .map(|gap| {
            let key = gap_key(gap.video_a_start_secs, gap.video_a_end_secs);
            let status = status_by_gap.remove(&key).unwrap_or(GapPatchStatus::NotPlanned {
                reason: GapFillSkipReason::NotFillable,
            });
            let tags = tags_by_gap.remove(&key).unwrap_or_else(|| {
                derive_gap_tags_from_status(&status, fill_mode, thresholds)
            });
            GapPatchOutcome::new(
                gap.video_a_start_secs,
                gap.video_a_end_secs,
                status,
                tags,
            )
            .with_residual(residual_by_gap.remove(&key))
        })
        .collect()
}

/// Per-run values derived once in `execute` and shared by every fill region.
struct RegionPatchContext {
    channels: usize,
    sample_rate: u32,
    max_refine_frames: usize,
    global_a_rms: f32,
    /// From the scan report (matches the thresholds the gaps were detected with).
    silence_peak_fraction: f32,
}

fn region_outcome_gap_tags(
    outcome: &RegionPatchOutcome,
    mut tag_ctx: GapTagsPatchContext,
) -> GapTags {
    if let RegionPatchOutcome::Patched {
        anchor_seam_used,
        anchor_bracket_move_frames,
        dual_fit_used,
        ..
    } = outcome
    {
        tag_ctx.anchor_seam_used = *anchor_seam_used;
        tag_ctx.anchor_bracket_move_frames = *anchor_bracket_move_frames;
        tag_ctx.dual_fit_used = *dual_fit_used;
    }
    tag_ctx.residual = match outcome {
        RegionPatchOutcome::Patched { residual, .. } | RegionPatchOutcome::Skipped { residual, .. } => {
            *residual
        }
    };
    let input = match outcome {
        RegionPatchOutcome::Patched {
            pre_correlation,
            post_correlation,
            confidence,
            ..
        } => GapPatchTierInput::Patched {
            pre: *pre_correlation,
            post: *post_correlation,
            confidence: *confidence,
        },
        RegionPatchOutcome::Skipped { reason, .. } => GapPatchTierInput::Skipped(reason),
    };
    derive_gap_tags_from_patch_outcome(&input, tag_ctx)
}

fn log_gap_tags_verbose(progress: &dyn ProgressReporter, tags: &GapTags) {
    progress.phase_verbose(&format_gap_tags_verbose_line(tags));
}

fn seam_failure_outcome(
    progress: &dyn ProgressReporter,
    request: &PatchAudioRequest,
    region: &FillRegion,
    fail: SeamGateFailure,
    min_structure_match_score: f32,
    tag_ctx: GapTagsPatchContext,
    dual_fit_attempt: Option<crate::domain::patch_result::SeamScoreAttempt>,
) -> (Option<RegionPatch>, RegionPatchOutcome, GapTagsPatchContext) {
    let (reason, residual) = match fail {
        SeamGateFailure::StructureAlignmentFailed => {
            (GapPatchSkipReason::BoundaryAlignmentFailed, None)
        }
        SeamGateFailure::StructureBelowThreshold { pre, post } => (
            GapPatchSkipReason::CorrelationBelowThreshold {
                pre_correlation: pre,
                post_correlation: post,
                min_correlation: min_structure_match_score,
                best_attempt: crate::domain::patch_result::better_seam_score_attempt(
                    None,
                    dual_fit_attempt,
                ),
            },
            None,
        ),
        SeamGateFailure::WaveformBelowThreshold {
            pre,
            post,
            min,
            best_attempt,
        } => (
            GapPatchSkipReason::CorrelationBelowThreshold {
                pre_correlation: pre,
                post_correlation: post,
                min_correlation: min,
                best_attempt: crate::domain::patch_result::better_seam_score_attempt(
                    best_attempt,
                    dual_fit_attempt,
                ),
            },
            None,
        ),
        SeamGateFailure::ResidualHeadroomExceeded {
            pre,
            post,
            residual,
            margin_db,
        } => (
            GapPatchSkipReason::ResidualHeadroomExceeded {
                pre_correlation: pre,
                post_correlation: post,
                headroom_db: residual.worst_headroom_db(),
                floor_pre_db: residual.floor_pre_db,
                floor_post_db: residual.floor_post_db,
                margin_db,
            },
            Some(residual),
        ),
    };
    log_skip_gap_fill(
        progress,
        &request.report.gaps,
        region.a_start_secs,
        region.a_end_secs,
        &reason,
    );
    let outcome = if let Some(residual) = residual {
        skipped_patch_with_residual(reason, Some(residual))
    } else {
        skipped_patch(reason)
    };
    (None, outcome, tag_ctx)
}

/// A-side gap-interior RMS floor (dB) — approximation of the fingerprint `levels.gap_floor_db`.
fn a_gap_floor_db(a_samples: &[f32], channels: usize, gap_start: usize, gap_end: usize) -> f64 {
    let ch = channels.max(1);
    let lo = gap_start.min(gap_end);
    let hi = gap_start.max(gap_end);
    if lo >= hi {
        return -120.0;
    }
    let sum_sq: f64 = (lo..hi)
        .map(|f| {
            let base = f * ch;
            let m = a_samples[base..base + ch].iter().map(|&x| x as f64).sum::<f64>() / ch as f64;
            m * m
        })
        .sum();
    let rms = (sum_sq / (hi - lo) as f64).sqrt();
    if rms <= 1e-9 {
        -120.0
    } else {
        20.0 * rms.log10()
    }
}

fn b_mapped_start_frame(
    refined_b_start_secs: f64,
    b_extract_start_secs: f64,
    sample_rate: u32,
) -> usize {
    (((refined_b_start_secs - b_extract_start_secs) * sample_rate as f64).round() as i64).max(0) as usize
}

/// Everything the A3 dual-fit algorithm needs, built once from the decoded window — only when `--dual-fit`
/// is on (so the off path pays nothing). `None` when the gap is too near a window edge for the seam borders.
struct DualFitRepairInput<'a> {
    params: crate::domain::dual_fit::DualFitParams,
    a_pre_mono: Vec<f64>,
    a_post_mono: Vec<f64>,
    b_mono: Vec<f64>,
    b_samples: &'a [f32],
    b_mapped_start: usize,
    a_start_frame: usize,
    a_end_frame: usize,
    crossfade_secs: f64,
    /// A's full decoded PCM — for the post-assembly residual measurement (parity with the ordinary
    /// path's residual gate).
    a_samples: &'a [f32],
    /// For the post-assembly residual measurement (parity with the ordinary path's residual gate) —
    /// the reference-window walk-out standoff, same constant the ordinary path uses.
    border_standoff_frames: usize,
}

#[allow(clippy::too_many_arguments)]
fn build_dual_fit_input<'a>(
    a_samples: &'a [f32],
    b_samples: &'a [f32],
    channels: usize,
    sample_rate: u32,
    refined: RefinedGapFrames,
    refined_b_start_secs: f64,
    b_extract_start_secs: f64,
    gap_frames: usize,
    fill_seam_search_secs: f64,
    min_fill_correlation: f32,
    fill_absolute_floor: f32,
    crossfade_secs: f64,
    border_standoff_frames: usize,
) -> Option<DualFitRepairInput<'a>> {
    let ch = channels.max(1);
    let w = ((fill_seam_search_secs * sample_rate as f64).round() as usize).max(8);
    let a_frames = a_samples.len() / ch;
    if refined.start_frame < w || refined.end_frame + w > a_frames || gap_frames == 0 {
        return None;
    }
    let mono = |lo: usize, hi: usize| -> Vec<f64> {
        (lo..hi)
            .map(|f| {
                let base = f * ch;
                a_samples[base..base + ch].iter().map(|&x| x as f64).sum::<f64>() / ch as f64
            })
            .collect()
    };
    let a_pre_mono = mono(refined.start_frame - w, refined.start_frame);
    let a_post_mono = mono(refined.end_frame, refined.end_frame + w);
    let b_mono: Vec<f64> = b_samples
        .chunks(ch)
        .map(|fr| fr.iter().map(|&x| x as f64).sum::<f64>() / ch as f64)
        .collect();
    let b_mapped_start = b_mapped_start_frame(refined_b_start_secs, b_extract_start_secs, sample_rate);
    let a_gap_floor_db = a_gap_floor_db(a_samples, channels, refined.start_frame, refined.end_frame);

    Some(DualFitRepairInput {
        params: crate::domain::dual_fit::DualFitParams {
            channels: ch,
            sample_rate,
            gap_frames,
            seam_window_frames: w,
            max_lag_frames: (0.6 * sample_rate as f64) as usize,
            min_fill_correlation: min_fill_correlation as f64,
            fill_absolute_floor: fill_absolute_floor as f64,
            step_real_margin: 0.15,
            a_gap_floor_db,
        },
        a_pre_mono,
        a_post_mono,
        b_mono,
        b_samples,
        b_mapped_start,
        a_start_frame: refined.start_frame,
        a_end_frame: refined.end_frame,
        crossfade_secs,
        a_samples,
        border_standoff_frames,
    })
}

/// Flag-gated **A3 dual-fit fallback** at the seam-gate skip. With `--dual-fit` off (default) this is exactly
/// [`seam_failure_outcome`] — the existing skip, **byte-identical** (D6). With it on, a bracket-exhausted
/// skip gets a dual-fit attempt (§5.2: per-shoulder seam-local fit → interior trim → `dualfit_target`); on
/// success it returns a `Patched` fill instead of the skip.
///
/// **`StructureAlignmentFailed` is excluded** — that variant means structure search never scored a single
/// candidate bracket, so there is nothing "exhausted" (§5.2 step 1 / doc `G6` requires `bracket_exhausted`,
/// i.e. brackets were scored and all failed). Dual-fit only ever attempts a rescue on a *scored-but-failed*
/// skip.
///
/// The assembled/trimmed fill is re-validated with the **unchanged** production gate (§5.2 step 3): the
/// pre-trim seam-local scores from `try_dual_fit` only prove the *shoulders* are viable, not the spliced
/// result, so we re-score `r.fill` against the real A border templates with
/// `fill_splice_seam_correlations_interleaved` and classify it with the same
/// `classify_fill_waveform_confidence` every other fill path uses. No loosening: a fill that doesn't clear
/// the floors here falls back to the skip.
/// `StructureAlignmentFailed` never qualifies for dual-fit rescue (see doc comment on
/// [`skip_or_dual_fit`]) — pulled out as a pure predicate so the exclusion is unit-testable
/// without constructing a full [`DualFitRepairInput`]/[`PatchAudioRequest`].
fn dual_fit_eligible(request_dual_fit: bool, fail: SeamGateFailure) -> bool {
    request_dual_fit && !matches!(fail, SeamGateFailure::StructureAlignmentFailed)
}

/// Residual-gate parity for dual-fit (§ residual-gate bypass fix): the ordinary path measures one
/// `SeamResidualVerdict` per gap using a single `chosen_delta`, valid because it assumes one rigid
/// A/B placement across the whole splice. Dual-fit matches its two shoulders independently
/// (`r.pre_lag`/`r.post_lag`), so this reuses the same low-level primitives
/// (`policies::seam_chosen_and_floor`/`SeamResidualVerdict::from_parts_with_placement`) with two
/// distinct `chosen_delta` values, one per shoulder, derived from `nominal_delta + {pre,post}_lag`.
fn measure_dual_fit_residual_verdict(
    request: &PatchAudioRequest,
    df: &DualFitRepairInput<'_>,
    r: &crate::domain::dual_fit::DualFitResult,
) -> Option<policies::SeamResidualVerdict> {
    if !(request.measure_residual
        || request.residual_gate.is_active()
        || tracing::enabled!(tracing::Level::DEBUG))
    {
        return None;
    }
    let nominal_delta = df.b_mapped_start as i64 - df.a_start_frame as i64;
    let max_lag_frames =
        crate::domain::residual_max_lag_frames(df.params.sample_rate, request.residual_lag_secs);
    let floor_common = |window: usize| policies::SeamFloorParams {
        a_samples: df.a_samples,
        channels: df.params.channels,
        b_mono: &df.b_mono,
        window,
        standoff_frames: df.border_standoff_frames,
        a_to_b_delta: nominal_delta,
        step_frames: window.max(1),
        max_walk_frames: df.params.sample_rate as usize * 3,
        absolute_silence_rms: request.absolute_silence_rms,
        max_lag_frames,
    };
    let window = df.params.seam_window_frames;
    let (chosen_pre, floor_pre) = policies::seam_chosen_and_floor(
        &floor_common(window),
        policies::SeamSide::Pre,
        df.a_start_frame,
        df.a_end_frame,
        nominal_delta + r.pre_lag,
    );
    let (chosen_post, floor_post) = policies::seam_chosen_and_floor(
        &floor_common(window),
        policies::SeamSide::Post,
        df.a_start_frame,
        df.a_end_frame,
        nominal_delta + r.post_lag,
    );
    let placement_slide = r.pre_lag.abs_diff(r.post_lag);
    Some(policies::SeamResidualVerdict::from_parts_with_placement(
        &chosen_pre,
        &chosen_post,
        &floor_pre,
        &floor_post,
        request.residual_floor_ok_db,
        placement_slide,
        max_lag_frames,
    ))
}

fn skip_or_dual_fit(
    progress: &dyn ProgressReporter,
    request: &PatchAudioRequest,
    region: &FillRegion,
    fail: SeamGateFailure,
    min_structure_match_score: f32,
    tag_ctx: GapTagsPatchContext,
    dual_fit: Option<&DualFitRepairInput<'_>>,
) -> (Option<RegionPatch>, RegionPatchOutcome, GapTagsPatchContext) {
    let mut dual_fit_attempt: Option<crate::domain::patch_result::SeamScoreAttempt> = None;
    if dual_fit_eligible(request.dual_fit, fail) {
        if let Some(df) = dual_fit {
            if let Some(r) = crate::domain::dual_fit::try_dual_fit(
                &df.a_pre_mono,
                &df.a_post_mono,
                &df.b_mono,
                df.b_samples,
                df.b_mapped_start,
                &df.params,
            ) {
                // Re-validate the assembled seams. The seam scores are the ones `try_dual_fit`'s own
                // seam-local search already measured (`r.pre_seam_r`/`r.post_seam_r`): A's near-gap
                // border vs the B window ENDING at the pre shoulder / STARTING at the post shoulder —
                // the content just OUTSIDE the bridge that each shoulder matched. The interior trim
                // (`trim_at_lowest_energy_interior`) only edits the bridge interior, guarded clear of
                // `seam_window_frames` from each end, so the assembled seams equal the ones dual-fit
                // fit — nothing to re-measure.
                //
                // We deliberately do NOT route this through `fill_splice_seam_correlations_interleaved`:
                // its border branch compares A's border against the fill's own head/tail
                // (`fill[..w]`/`fill[len-w..]`), which for a dual-fit fill is the B window on the INSIDE
                // of the shoulder — i.e. the chunk of B immediately ADJACENT to (not overlapping) the
                // window each seam actually matched. For broadband audio those adjacent windows
                // correlate at ~0, so a perfect dual-fit fill scored a false ~0 and was skipped. That
                // scorer is built for the rigid single-lag splice (where the fill head overlaps A's
                // pre-gap region); it has no access to the B content outside the bridge that dual-fit's
                // per-shoulder seam-local match relies on.
                let (splice_pre, splice_post) = (r.pre_seam_r, r.post_seam_r);
                match classify_fill_waveform_confidence(
                    splice_pre,
                    splice_post,
                    request.min_fill_correlation,
                    request.fill_marginal_margin,
                    request.fill_absolute_floor,
                ) {
                    Ok(confidence) => {
                        let residual = measure_dual_fit_residual_verdict(request, df, &r);
                        let gated_confidence = match &residual {
                            Some(verdict) => match crate::domain::gap_fill_fit::apply_residual_to_confidence(
                                Ok(confidence),
                                verdict,
                                request.residual_headroom_margin_db,
                                request.residual_gate.rescue_enabled(),
                            ) {
                                Ok(tier) => Ok(tier),
                                Err(crate::domain::gap_fill_fit::ResidualGateError::HeadroomExceeded {
                                    headroom_db,
                                    margin_db,
                                }) => Err((headroom_db, margin_db, *verdict)),
                                Err(crate::domain::gap_fill_fit::ResidualGateError::PearsonBelowFloor(_)) => {
                                    Ok(confidence)
                                }
                            },
                            None => Ok(confidence),
                        };
                        let confidence = match gated_confidence {
                            Ok(tier) => tier,
                            Err((headroom_db, margin_db, verdict)) => {
                                tracing::debug!(
                                    a_start = df.a_start_frame,
                                    headroom_db,
                                    margin_db,
                                    "dual-fit candidate rejected by residual headroom gate; falling back to skip"
                                );
                                dual_fit_attempt = Some(crate::domain::patch_result::SeamScoreAttempt {
                                    pre_correlation: splice_pre,
                                    post_correlation: splice_post,
                                    source: crate::domain::patch_result::SeamScoreSource::DualFit,
                                });
                                return seam_failure_outcome(
                                    progress,
                                    request,
                                    region,
                                    SeamGateFailure::ResidualHeadroomExceeded {
                                        pre: splice_pre,
                                        post: splice_post,
                                        residual: verdict,
                                        margin_db,
                                    },
                                    min_structure_match_score,
                                    tag_ctx,
                                    dual_fit_attempt,
                                );
                            }
                        };
                        tracing::debug!(
                            a_start = df.a_start_frame,
                            pre = splice_pre,
                            post = splice_post,
                            trim = r.trim_frames,
                            ?confidence,
                            "dual-fit rescued a bracket-exhausted skip"
                        );
                        let patch = RegionPatch {
                            b_samples: r.fill,
                            gain: 1.0,
                            a_start_frame: df.a_start_frame,
                            a_end_frame: df.a_end_frame,
                            crossfade_secs: df.crossfade_secs,
                        };
                        let outcome = RegionPatchOutcome::Patched {
                            pre_correlation: splice_pre,
                            post_correlation: splice_post,
                            align_adjustment_secs: 0.0,
                            waveform_adjustment_secs: 0.0,
                            structure_trusted: false,
                            confidence,
                            gap_start_adjust_frames: 0,
                            gap_end_adjust_frames: 0,
                            fit_used_boundary_grid: false,
                            fit_boundary_grid_cells: None,
                            residual,
                            anchor_seam_used: false,
                            anchor_bracket_move_frames: 0,
                            dual_fit_used: true,
                        };
                        return (Some(patch), outcome, tag_ctx);
                    }
                    Err(min_score) => {
                        tracing::debug!(
                            a_start = df.a_start_frame,
                            pre = splice_pre,
                            post = splice_post,
                            min_score,
                            "dual-fit candidate failed re-validation at the assembled seam; falling back to skip"
                        );
                        dual_fit_attempt = Some(crate::domain::patch_result::SeamScoreAttempt {
                            pre_correlation: splice_pre,
                            post_correlation: splice_post,
                            source: crate::domain::patch_result::SeamScoreSource::DualFit,
                        });
                    }
                }
            }
        }
    }
    seam_failure_outcome(
        progress,
        request,
        region,
        fail,
        min_structure_match_score,
        tag_ctx,
        dual_fit_attempt,
    )
}

/// Inputs to [`assemble_bracket_fill`] — the sliced B fill (`b_fill_raw` + contiguous `b_extension`), the
/// A-border templates for Fit-mode length fitting, and the seam context.
struct BracketFillAssembly<'a> {
    b_fill_raw: Vec<f32>,
    b_extension: &'a [f32],
    channels: usize,
    gap_frames: usize,
    fill_mode: FillMode,
    a_pre_border: &'a [f64],
    a_post_border: &'a [f64],
    a_pre_ch: &'a [Vec<f64>],
    a_post_ch: &'a [Vec<f64>],
    pre_gate_frames: usize,
    post_gate_frames: usize,
    repeat_window_frames: usize,
    seam_ctx: policies::SpliceSeamContext<'a>,
    a_start_secs: f64,
}

/// Assemble the bracket fill PCM to exactly `gap_frames` from the sliced B: **Fit** length-fits against the A
/// borders ([`fit_fill_length_for_gap`]), **Gate** extends from contiguous B then tail-trims
/// ([`fit_fill_to_gap_frames`]). Extracted verbatim from `prepare_region_patch` (6b.3a) so the current inline
/// path and the future `execute_region_spec` share ONE PCM primitive — byte-identical by construction. The
/// executor reconstructs these inputs deterministically from the spec: `b_fill_raw`/`b_extension` re-sliced
/// from the decode buffer via `FillAlignment`, and (Fit mode only) the A-border templates + `seam_ctx` rebuilt
/// from A geometry — never re-deciding.
fn assemble_bracket_fill(inp: BracketFillAssembly<'_>) -> Vec<f32> {
    let BracketFillAssembly {
        b_fill_raw,
        b_extension,
        channels,
        gap_frames,
        fill_mode,
        a_pre_border,
        a_post_border,
        a_pre_ch,
        a_post_ch,
        pre_gate_frames,
        post_gate_frames,
        repeat_window_frames,
        seam_ctx,
        a_start_secs,
    } = inp;
    // `source_frames` is derived here (not a field) so a caller can't pass one inconsistent with `b_fill_raw`.
    let source_frames = b_fill_raw.len() / channels;
    if fill_mode == FillMode::Fit {
        let borders = policies::BorderSeamTemplates {
            a_pre: a_pre_border,
            a_post: a_post_border,
            a_pre_ch,
            a_post_ch,
            pre_window: pre_gate_frames,
            post_window: post_gate_frames,
        };
        fit_fill_length_for_gap(
            &b_fill_raw,
            b_extension,
            channels,
            gap_frames,
            &borders,
            repeat_window_frames,
            seam_ctx,
        )
    } else {
        let mut gate_fill = b_fill_raw;
        if source_frames < gap_frames {
            let need_samples = (gap_frames - source_frames) * channels;
            let extend_to = need_samples.min(b_extension.len());
            if extend_to > 0 {
                gate_fill.extend_from_slice(&b_extension[..extend_to]);
                tracing::debug!(
                    a_start_secs,
                    extended_frames = extend_to / channels,
                    "B bracket shorter than A gap; extended from contiguous B audio (gate)"
                );
            }
        }
        fit_fill_to_gap_frames(&gate_fill, channels, gap_frames)
    }
}

/// Everything the executor needs to reconstruct a bracket fill from the spec — the `FillAlignment` indices,
/// the decode buffers, and the A geometry/config to rebuild the border templates.
struct ExecuteBracketFillCtx<'a> {
    alignment: policies::FillAlignment,
    b_samples: &'a [f32],
    a_samples: &'a [f32],
    a_frames: usize,
    refined: RefinedGapFrames,
    channels: usize,
    gap_frames: usize,
    fill_mode: FillMode,
    border_frames: usize,
    border_standoff_frames: usize,
    silence_peak_fraction: f32,
    absolute_silence_rms: f32,
    seam_gate_frames: usize,
    sample_rate: u32,
    crossfade_secs: f64,
    a_start_secs: f64,
}

/// The executor's bracket-fill reconstruction (6b.3a). Re-derives everything the fill needs from the spec's
/// [`FillAlignment`] + the decode buffers + A geometry — **independently of characterize** — then assembles
/// the PCM via the shared [`assemble_bracket_fill`]. This is the "assemble twice" design: characterize
/// assembles a fill to score the report-vs-splice reconciliation and discards it; the executor re-assembles
/// from the spec for the splice (borders rebuilt from scratch — deduped later by the step-8 hoists). Pinned
/// byte-identical to the authoritative inline fill by a `debug_assert_eq!` at the characterize call site until
/// 6b.3b flips `prepare_region_patch` to the characterize→execute shim.
fn execute_bracket_fill(ctx: ExecuteBracketFillCtx<'_>) -> Vec<f32> {
    let ExecuteBracketFillCtx {
        alignment,
        b_samples,
        a_samples,
        a_frames,
        refined,
        channels,
        gap_frames,
        fill_mode,
        border_frames,
        border_standoff_frames,
        silence_peak_fraction,
        absolute_silence_rms,
        seam_gate_frames,
        sample_rate,
        crossfade_secs,
        a_start_secs,
    } = ctx;
    let fill_start_sample = alignment.start_frame * channels;
    let b_fill_end_sample = fill_start_sample + alignment.fill_frames * channels;
    let b_fill_raw = b_samples[fill_start_sample..b_fill_end_sample].to_vec();
    let b_extension = if b_fill_end_sample < b_samples.len() {
        &b_samples[b_fill_end_sample..]
    } else {
        &[][..]
    };
    let border_spec = GapBorderSpec {
        gap_start_frame: refined.start_frame,
        gap_end_frame: refined.end_frame,
        border_frames,
        border_standoff_frames,
        silence_peak_fraction,
        absolute_rms_floor: absolute_silence_rms,
    };
    let (a_pre_border, a_post_border) =
        policies::border_templates_for_gap(a_samples, channels, &border_spec);
    let (a_pre_ch, a_post_ch) =
        policies::border_templates_per_channel_for_gap(a_samples, channels, &border_spec);
    let pre_gate_frames = seam_gate_frames.min(a_pre_border.len().max(1));
    let post_gate_frames = if a_post_border.is_empty() {
        0
    } else {
        seam_gate_frames.min(a_post_border.len()).max(1)
    };
    let repeat_window_frames = border_frames.max(1);
    let seam_cf = policies::effective_seam_crossfade_frames(
        (crossfade_secs * sample_rate as f64) as usize,
        refined.start_frame,
        refined.end_frame,
        a_frames,
    );
    let seam_ctx = policies::SpliceSeamContext {
        seam_cf,
        gap_start_frame: refined.start_frame,
        gap_end_frame: refined.end_frame,
        a_samples,
        channels,
        single_lag_alignment: true,
    };
    assemble_bracket_fill(BracketFillAssembly {
        b_fill_raw,
        b_extension,
        channels,
        gap_frames,
        fill_mode,
        a_pre_border: &a_pre_border,
        a_post_border: &a_post_border,
        a_pre_ch: &a_pre_ch,
        a_post_ch: &a_post_ch,
        pre_gate_frames,
        post_gate_frames,
        repeat_window_frames,
        seam_ctx,
        a_start_secs,
    })
}

/// Resolved values the executor assembles a bracket `(RegionPatch, RegionPatchOutcome)` from. Everything
/// here is either read from the spec (gain, seam correlations, confidence, flags) or reconstructed (`fill`
/// via [`execute_bracket_fill`], slides recomputed from the geometry fields).
struct ExecuteBracketOutputCtx {
    fill: Vec<f32>,
    gain: f32,
    refined: RefinedGapFrames,
    crossfade_secs: f64,
    final_pre: f64,
    final_post: f64,
    final_confidence: FillConfidence,
    structure_trusted: bool,
    gap_start_adjust_frames: i64,
    gap_end_adjust_frames: i64,
    fit_used_boundary_grid: bool,
    fit_boundary_grid_cells: Option<u32>,
    residual: Option<policies::SeamResidualVerdict>,
    anchor_seam_used: bool,
    anchor_bracket_move_frames: usize,
    // Slide geometry — recomputed here, independent of characterize.
    structure_start_frame: usize,
    alignment_start_frame: usize,
    b_extract_start_secs: f64,
    gap_offset_secs: f64,
    sample_rate: u32,
}

/// The executor's bracket `(RegionPatch, RegionPatchOutcome)` assembly (6b.3a). Recomputes the geometry
/// slides independently and builds the structs from the resolved/spec values — `dual_fit_used` is always
/// `false` for a bracket. Pinned byte-identical to the inline return by a `debug_assert_eq!` until 6b.3b.
fn execute_bracket_output(ctx: ExecuteBracketOutputCtx) -> (RegionPatch, RegionPatchOutcome) {
    let refined_b_start_secs =
        ctx.refined.start_frame as f64 / ctx.sample_rate as f64 + ctx.gap_offset_secs;
    let offset_nominal_start =
        ((refined_b_start_secs - ctx.b_extract_start_secs) * ctx.sample_rate as f64).round() as usize;
    let structure_slide_secs =
        (ctx.structure_start_frame as f64 - offset_nominal_start as f64) / ctx.sample_rate as f64;
    let waveform_slide_secs =
        (ctx.alignment_start_frame as f64 - ctx.structure_start_frame as f64) / ctx.sample_rate as f64;
    let align_adjustment_secs = structure_slide_secs + waveform_slide_secs;
    let patch = RegionPatch {
        b_samples: ctx.fill,
        gain: ctx.gain,
        a_start_frame: ctx.refined.start_frame,
        a_end_frame: ctx.refined.end_frame,
        crossfade_secs: ctx.crossfade_secs,
    };
    let outcome = RegionPatchOutcome::Patched {
        pre_correlation: ctx.final_pre,
        post_correlation: ctx.final_post,
        align_adjustment_secs,
        waveform_adjustment_secs: waveform_slide_secs,
        structure_trusted: ctx.structure_trusted,
        confidence: ctx.final_confidence,
        gap_start_adjust_frames: ctx.gap_start_adjust_frames,
        gap_end_adjust_frames: ctx.gap_end_adjust_frames,
        fit_used_boundary_grid: ctx.fit_used_boundary_grid,
        fit_boundary_grid_cells: ctx.fit_boundary_grid_cells,
        residual: ctx.residual,
        anchor_seam_used: ctx.anchor_seam_used,
        anchor_bracket_move_frames: ctx.anchor_bracket_move_frames,
        dual_fit_used: false,
    };
    (patch, outcome)
}

fn prepare_region_patch(
    progress: &dyn ProgressReporter,
    media: &RegionPatchMedia<'_>,
    region: &FillRegion,
    request: &PatchAudioRequest,
    ctx: &RegionPatchContext,
    opts: RegionPatchOpts<'_>,
) -> (Option<RegionPatch>, RegionPatchOutcome, GapTagsPatchContext) {
    let RegionPatchMedia {
        b_samples_full,
        a_pcm,
    } = *media;
    let RegionPatchOpts {
        anchored_retry_pass,
        patch_anchors,
    } = opts;
    let &RegionPatchContext {
        channels,
        sample_rate,
        max_refine_frames,
        global_a_rms,
        silence_peak_fraction,
    } = ctx;
    let normalize_window_secs = request.normalize_window_secs;
    let margin_secs = request.fill_align_margin_secs;
    let border_search_secs = request.fill_border_search_secs;
    let min_border_discovery_secs = request.min_border_discovery_secs;
    let border_standoff_secs = request.border_standoff_secs;
    let fill_length_slack_secs = request.fill_length_slack_secs;
    let fill_seam_search_secs = request.fill_seam_search_secs;
    let gap_signature_context_secs = request.gap_signature_context_secs;
    let gap_signature_bin_ms = request.gap_signature_bin_ms;
    let min_structure_match_score = request.min_structure_match_score;
    let min_fill_correlation = request.min_fill_correlation;
    let normalize_fill = request.normalize_fill;
    let max_fill_gain_db = request.max_fill_gain_db;
    let absolute_silence_rms = request.absolute_silence_rms;
    let fill_offset_mode = request.fill_offset_mode;
    let gap_end_extend_on_post_seam_fail = request.gap_end_extend_on_post_seam_fail;
    let gap_start_extend_on_pre_seam_fail = request.gap_start_extend_on_pre_seam_fail;
    let gap_end_extend_max_ms = request.gap_end_extend_max_ms;
    let gap_end_extend_step_ms = request.gap_end_extend_step_ms;

    debug_assert!(
        region.b_start_secs >= 0.0,
        "fill plan must not include gaps with negative B start"
    );

    let gap_time_on_a = (region.a_start_secs + region.a_end_secs) / 2.0;
    let gap_offset_secs = resolve_gap_offset_secs(
        &request.report.alignment,
        fill_offset_mode,
        gap_time_on_a,
        patch_anchors,
        anchored_retry_pass,
    )
    .unwrap_or(region.b_start_secs - region.a_start_secs);

    if anchored_retry_pass == AnchoredRetryPass::Second {
        if let Some(table) = patch_anchors.filter(|t| !t.is_empty()) {
            progress.phase_verbose(&format_anchored_offset_verbose_line(
                gap_offset_secs,
                &request.report.alignment,
                gap_time_on_a,
                table,
            ));
        }
    }

    let reported_start_frame = (region.a_start_secs * sample_rate as f64) as usize;
    let reported_end_frame = (region.a_end_secs * sample_rate as f64) as usize;
    let mut refined: RefinedGapFrames = policies::refine_gap_frames(
        &a_pcm.samples,
        channels,
        reported_start_frame,
        reported_end_frame,
        silence_peak_fraction,
        absolute_silence_rms,
        max_refine_frames,
    );

    let refined_a_start_secs = refined.start_frame as f64 / sample_rate as f64;
    let refined_a_end_secs = refined.end_frame as f64 / sample_rate as f64;
    let refined_b_start_secs = refined_a_start_secs + gap_offset_secs;
    let refined_b_end_secs = refined_a_end_secs + gap_offset_secs;
    let a_start_secs = refined_a_start_secs;

    if !matches!(
        fill_offset_mode,
        FillOffsetMode::Recommended
            | FillOffsetMode::AnchoredRetry if anchored_retry_pass == AnchoredRetryPass::First
    ) {
        tracing::debug!(
            a_start_secs,
            gap_offset_secs,
            mode = ?fill_offset_mode,
            "using per-gap fill offset"
        );
    }

    let context_frames =
        (gap_signature_context_secs * sample_rate as f64).round() as usize;
    let bin_frames =
        ((gap_signature_bin_ms as f64 / 1000.0) * sample_rate as f64).round() as usize;
    let search_radius_secs = border_search_secs.max(margin_secs);
    let extend_slack_secs = gap_extension_slack_secs(repair_patch_config_view(request));
    let b_extract_start_secs = (refined_b_start_secs
        - gap_signature_context_secs
        - search_radius_secs
        - margin_secs
        - extend_slack_secs)
        .max(0.0);
    let length_slack_secs = fill_length_slack_secs.max(margin_secs);
    let b_extract_end_secs = refined_b_end_secs
        + gap_signature_context_secs
        + search_radius_secs
        + length_slack_secs
        + margin_secs
        + extend_slack_secs;

    let gap_frames_preview = refined.end_frame.saturating_sub(refined.start_frame);
    let signature_mode_label = if request.fill_mode == FillMode::Fit && gap_frames_preview > 0 {
        let structure_params = StructureMatchParams {
            gap_frames: gap_frames_preview,
            bin_frames: bin_frames.max(1),
            search_radius_frames: 0,
            fill_length_slack_frames: 0,
            max_fine_adjustment_frames: 0,
            silence_peak_fraction,
            absolute_silence_rms,
        };
        build_gap_signature(
            &a_pcm.samples,
            channels,
            refined.start_frame,
            refined.end_frame,
            context_frames,
            &structure_params,
            request.gap_signature_mode,
        )
        .mode_label()
    } else {
        "bool"
    };

    let mut tag_ctx = GapTagsPatchContext {
        fill_mode: request.fill_mode,
        thresholds: FillTierThresholds {
            min_fill_correlation: request.min_fill_correlation,
            fill_marginal_margin: request.fill_marginal_margin,
            fill_absolute_floor: request.fill_absolute_floor,
        },
        signature_mode_label,
        fit_used_boundary_grid: false,
        anchor_seam_used: false,
        anchor_bracket_move_frames: 0,
        anchor_trusted: false,
        dual_fit_used: false,
        residual: None,
        residual_headroom_margin_db: request.residual_headroom_margin_db,
    };

    log_gap_fill_plan_verbose(
        progress,
        &GapFillPlanLog {
            scan_a_start_secs: region.a_start_secs,
            scan_a_end_secs: region.a_end_secs,
            refined_a_start_secs,
            refined_a_end_secs,
            gap_offset_secs,
            fill_offset_mode,
            mapped_b_start_secs: refined_b_start_secs,
            mapped_b_end_secs: refined_b_end_secs,
            b_search_start_secs: b_extract_start_secs,
            b_search_end_secs: b_extract_end_secs,
            signature_mode_label,
        },
    );

    let gap_frames = refined.end_frame.saturating_sub(refined.start_frame);
    if gap_frames == 0 {
        return (
            None,
            skipped_patch(GapPatchSkipReason::ZeroLengthGap),
            tag_ctx,
        );
    }

    let correlate_frames = correlate_frames_for_gap(
        normalize_window_secs,
        min_border_discovery_secs,
        gap_frames,
        sample_rate,
    );
    let seam_gate_frames =
        seam_gate_frames_for(correlate_frames, fill_seam_search_secs, sample_rate);
    let b_samples = match slice_b_segment(
        b_samples_full,
        channels,
        sample_rate,
        b_extract_start_secs,
        b_extract_end_secs,
    ) {
        Some(samples) => samples,
        None => {
            let reason = GapPatchSkipReason::BExtractFailed;
            log_skip_gap_fill(
                progress,
                &request.report.gaps,
                region.a_start_secs,
                region.a_end_secs,
                &reason,
            );
            return (
                None,
                skipped_patch(reason),
                tag_ctx,
            );
        }
    };

    let border_frames = border_frames_from_secs(normalize_window_secs, sample_rate)
        .min(correlate_frames);
    let border_standoff_frames =
        (border_standoff_secs * sample_rate as f64).round() as usize;
    let search_radius_frames =
        (border_search_secs * sample_rate as f64).round() as usize;
    let max_extend_frames =
        (gap_end_extend_max_ms as f64 / 1000.0 * sample_rate as f64).round() as usize;
    let step_frames =
        (gap_end_extend_step_ms as f64 / 1000.0 * sample_rate as f64).round() as usize;

    let cfg = SeamGateConfig::from_repair(request, sample_rate, channels, silence_peak_fraction);
    let anchor_search_prior = anchor_search_prior_for_gap(
        request,
        patch_anchors,
        anchored_retry_pass,
        region,
        b_extract_start_secs,
        search_radius_frames,
        sample_rate,
    );
    let geom = derive_seam_gate_geometry(
        &cfg,
        a_pcm,
        b_samples,
        b_extract_start_secs,
        refined_b_start_secs,
        refined_b_end_secs,
        gap_frames,
        anchor_search_prior,
    );
    let seam_params = SeamGateParams { cfg: &cfg, geom };

    // A3: build the dual-fit fallback input once (only when `--dual-fit` is on), before the gate can mutate
    // `refined` via boundary-extension retry — dual-fit works on the base gap geometry.
    let dual_fit_input = request
        .dual_fit
        .then(|| {
            build_dual_fit_input(
                &a_pcm.samples,
                b_samples,
                channels,
                sample_rate,
                refined,
                refined_b_start_secs,
                b_extract_start_secs,
                gap_frames,
                fill_seam_search_secs,
                min_fill_correlation,
                request.fill_absolute_floor,
                region.crossfade_secs,
                cfg.border_standoff_frames,
            )
        })
        .flatten();

    let gate_outcome = match evaluate_seam_gate(refined, &seam_params) {
        Ok(outcome) => outcome,
        Err(fail)
            if request.fill_mode == crate::domain::FillMode::Gate
                && (gap_end_extend_on_post_seam_fail || gap_start_extend_on_pre_seam_fail) =>
        {
            match fail {
                SeamGateFailure::WaveformBelowThreshold { .. } => {
                    match retry_waveform_seam_extensions(
                        &mut refined,
                        gap_offset_secs,
                        &seam_params,
                        fail,
                        SeamExtensionRetry {
                            max_extend_frames,
                            step_frames: step_frames.max(1),
                            gap_end_extend_on_post_seam_fail,
                            gap_start_extend_on_pre_seam_fail,
                        },
                    ) {
                        Ok(outcome) => outcome,
                        Err(retry_fail) => {
                            return skip_or_dual_fit(
                                progress,
                                request,
                                region,
                                retry_fail,
                                min_structure_match_score,
                                tag_ctx,
                                dual_fit_input.as_ref(),
                            );
                        }
                    }
                }
                other => {
                    return skip_or_dual_fit(
                        progress,
                        request,
                        region,
                        other,
                        min_structure_match_score,
                        tag_ctx,
                        dual_fit_input.as_ref(),
                    );
                }
            }
        }
        Err(other) => {
            return skip_or_dual_fit(
                progress,
                request,
                region,
                other,
                min_structure_match_score,
                tag_ctx,
                dual_fit_input.as_ref(),
            );
        }
    };

    let SeamGateOutcome {
        refined,
        alignment,
        report_pre,
        report_post,
        structure_trusted: patched_structure_trusted,
        structure_start_frame,
        gap_frames,
        confidence,
        gap_start_adjust_frames,
        gap_end_adjust_frames,
        fit_used_boundary_grid,
        fit_boundary_grid_cells,
        fit_haystack_secs,
        residual: residual_verdict,
        anchor_seam_used,
        anchor_bracket_move_frames,
        anchor_trusted,
        ..
    } = gate_outcome;
    tag_ctx.fit_used_boundary_grid = fit_used_boundary_grid;
    tag_ctx.anchor_seam_used = anchor_seam_used;
    tag_ctx.anchor_bracket_move_frames = anchor_bracket_move_frames;
    tag_ctx.anchor_trusted = anchor_trusted;

    if confidence == FillConfidence::Marginal {
        log_marginal_gap_fill(
            progress,
            &MarginalGapFillLog {
                gaps: &request.report.gaps,
                a_start_secs: region.a_start_secs,
                a_end_secs: region.a_end_secs,
                pre: report_pre,
                post: report_post,
                min: request.min_fill_correlation,
                anchor_seam: anchor_seam_used,
            },
        );
    }

    let refined_b_start_secs = refined.start_frame as f64 / sample_rate as f64 + gap_offset_secs;

    let offset_nominal_start =
        ((refined_b_start_secs - b_extract_start_secs) * sample_rate as f64).round() as usize;

    let structure_slide_secs = (structure_start_frame as f64 - offset_nominal_start as f64)
        / sample_rate as f64;
    let waveform_slide_secs =
        (alignment.start_frame as f64 - structure_start_frame as f64) / sample_rate as f64;
    let align_adjustment_secs = structure_slide_secs + waveform_slide_secs;

    let fill_start_sample = alignment.start_frame * channels;
    let b_fill_end_sample = fill_start_sample + alignment.fill_frames * channels;
    if b_fill_end_sample > b_samples.len() {
        let reason = GapPatchSkipReason::AlignedSegmentOutOfRange;
        log_skip_gap_fill(
            progress,
            &request.report.gaps,
            region.a_start_secs,
            region.a_end_secs,
            &reason,
        );
        return (
            None,
            skipped_patch(reason),
            tag_ctx,
        );
    }

    let b_fill_raw = b_samples[fill_start_sample..b_fill_end_sample].to_vec();
    let source_frames = b_fill_raw.len() / channels;
    let b_extension = if b_fill_end_sample < b_samples.len() {
        &b_samples[b_fill_end_sample..]
    } else {
        &[][..]
    };
    if source_frames > gap_frames {
        tracing::debug!(
            a_start_secs,
            b_fill_frames = source_frames,
            a_gap_frames = gap_frames,
            "B fill longer than A gap; choosing trim anchor (fit) or trimming tail (gate)"
        );
    }
    let border_spec = GapBorderSpec {
        gap_start_frame: refined.start_frame,
        gap_end_frame: refined.end_frame,
        border_frames,
        border_standoff_frames,
        silence_peak_fraction,
        absolute_rms_floor: absolute_silence_rms,
    };
    let (a_pre_border, a_post_border) =
        policies::border_templates_for_gap(&a_pcm.samples, channels, &border_spec);
    let (a_pre_ch, a_post_ch) = policies::border_templates_per_channel_for_gap(
        &a_pcm.samples,
        channels,
        &border_spec,
    );
    let pre_gate_frames = seam_gate_frames.min(a_pre_border.len().max(1));
    let post_gate_frames = if a_post_border.is_empty() {
        0
    } else {
        seam_gate_frames.min(a_post_border.len()).max(1)
    };
    let repeat_window_frames = border_frames.max(1);
    let seam_cf = policies::effective_seam_crossfade_frames(
        (region.crossfade_secs * sample_rate as f64) as usize,
        refined.start_frame,
        refined.end_frame,
        a_pcm.frames(),
    );
    let seam_ctx = policies::SpliceSeamContext {
        seam_cf,
        gap_start_frame: refined.start_frame,
        gap_end_frame: refined.end_frame,
        a_samples: &a_pcm.samples,
        channels,
        single_lag_alignment: true,
    };
    let b_fill = assemble_bracket_fill(BracketFillAssembly {
        b_fill_raw,
        b_extension,
        channels,
        gap_frames,
        fill_mode: request.fill_mode,
        a_pre_border: &a_pre_border,
        a_post_border: &a_post_border,
        a_pre_ch: &a_pre_ch,
        a_post_ch: &a_post_ch,
        pre_gate_frames,
        post_gate_frames,
        repeat_window_frames,
        seam_ctx,
        a_start_secs,
    });

    // 6b.3a shadow: prove the executor can re-derive this exact fill from the spec's `FillAlignment` +
    // decode buffers alone (rebuilding borders independently). Guards the characterize→execute split before
    // 6b.3b removes the inline path. Debug-only; the release path is unchanged.
    debug_assert_eq!(
        execute_bracket_fill(ExecuteBracketFillCtx {
            alignment,
            b_samples,
            a_samples: &a_pcm.samples,
            a_frames: a_pcm.frames(),
            refined,
            channels,
            gap_frames,
            fill_mode: request.fill_mode,
            border_frames,
            border_standoff_frames,
            silence_peak_fraction,
            absolute_silence_rms,
            seam_gate_frames,
            sample_rate,
            crossfade_secs: region.crossfade_secs,
            a_start_secs,
        }),
        b_fill,
        "6b.3a: executor bracket-fill reconstruction must byte-match the inline fill"
    );

    let gain = if normalize_fill {
        let border_rms = compute_a_border_rms(
            a_pcm,
            refined.start_frame,
            refined.end_frame,
            normalize_window_secs,
            global_a_rms,
        );
        let b_rms = policies::rms_interleaved(&b_fill);
        policies::compute_fill_gain(border_rms, b_rms, max_fill_gain_db)
    } else {
        1.0f32
    };

    log_gap_fill_result_verbose(
        progress,
        &GapFillResultLog {
            b_search_start_secs: b_extract_start_secs,
            sample_rate,
            channels,
            fill_start_sample,
            fill_end_sample: b_fill_end_sample,
            structure_slide_secs,
            waveform_slide_secs,
            fit_used_boundary_grid,
            fit_boundary_grid_cells,
            fit_haystack_secs,
            report_pre,
            report_post,
            confidence,
        },
    );

    let (final_pre, final_post, final_confidence) = if patched_structure_trusted {
        (report_pre, report_post, confidence)
    } else if request.fill_mode == FillMode::Fit {
        let (splice_pre, splice_post) = policies::fill_splice_seam_correlations_interleaved(
            &b_fill,
            channels,
            &policies::BorderSeamTemplates {
                a_pre: &a_pre_border,
                a_post: &a_post_border,
                a_pre_ch: &a_pre_ch,
                a_post_ch: &a_post_ch,
                pre_window: pre_gate_frames,
                post_window: post_gate_frames,
            },
            seam_ctx,
        );
        let gate_min = report_pre.min(report_post);
        let splice_min = splice_pre.min(splice_post);
        let use_splice = splice_min >= gate_min;
        let (pre, post) = if use_splice {
            (splice_pre, splice_post)
        } else {
            (report_pre, report_post)
        };
        let splice_confidence = if use_splice {
            classify_fill_waveform_confidence(
                splice_pre,
                splice_post,
                min_fill_correlation,
                request.fill_marginal_margin,
                request.fill_absolute_floor,
            )
            .unwrap_or(confidence)
        } else {
            confidence
        };
        (pre, post, splice_confidence)
    } else {
        (report_pre, report_post, confidence)
    };

    let inline_patch = Some(RegionPatch {
        b_samples: b_fill,
        gain,
        a_start_frame: refined.start_frame,
        a_end_frame: refined.end_frame,
        crossfade_secs: region.crossfade_secs,
    });
    let inline_outcome = RegionPatchOutcome::Patched {
        pre_correlation: final_pre,
        post_correlation: final_post,
        align_adjustment_secs,
        waveform_adjustment_secs: waveform_slide_secs,
        structure_trusted: patched_structure_trusted,
        confidence: final_confidence,
        gap_start_adjust_frames,
        gap_end_adjust_frames,
        fit_used_boundary_grid,
        fit_boundary_grid_cells,
        residual: residual_verdict,
        anchor_seam_used,
        anchor_bracket_move_frames,
        dual_fit_used: false,
    };

    // 6b.3a shadow: prove the executor assembles the identical (patch, outcome) from the resolved/spec
    // values + geometry (slides recomputed independently). Debug-only; the release path is unchanged.
    #[cfg(debug_assertions)]
    if let Some(inline_p) = inline_patch.as_ref() {
        let (exec_patch, exec_outcome) = execute_bracket_output(ExecuteBracketOutputCtx {
            fill: inline_p.b_samples.clone(),
            gain: inline_p.gain,
            refined,
            crossfade_secs: region.crossfade_secs,
            final_pre,
            final_post,
            final_confidence,
            structure_trusted: patched_structure_trusted,
            gap_start_adjust_frames,
            gap_end_adjust_frames,
            fit_used_boundary_grid,
            fit_boundary_grid_cells,
            residual: residual_verdict,
            anchor_seam_used,
            anchor_bracket_move_frames,
            structure_start_frame,
            alignment_start_frame: alignment.start_frame,
            b_extract_start_secs,
            gap_offset_secs,
            sample_rate,
        });
        debug_assert_eq!(exec_patch, *inline_p, "6b.3a: executor RegionPatch must match inline");
        debug_assert_eq!(
            exec_outcome, inline_outcome,
            "6b.3a: executor RegionPatchOutcome must match inline"
        );
    }

    (inline_patch, inline_outcome, tag_ctx)
}

fn repair_patch_config_view(request: &PatchAudioRequest) -> RepairPatchConfigView {
    RepairPatchConfigView {
        fill_mode: request.fill_mode,
        fit_boundary_search: request.fit_boundary_search,
        gap_end_extend_on_post_seam_fail: request.gap_end_extend_on_post_seam_fail,
        gap_start_extend_on_pre_seam_fail: request.gap_start_extend_on_pre_seam_fail,
        gap_end_extend_max_ms: request.gap_end_extend_max_ms,
        disable_structure_trust: request.disable_structure_trust,
        short_gap_one_strong_seam_fallback: request.short_gap_one_strong_seam_fallback,
        fill_anchor_search_prior_weight: request.fill_anchor_search_prior_weight,
        fill_anchor_retry_marginal: request.fill_anchor_retry_marginal,
        fill_offset_mode: request.fill_offset_mode,
        anchor_seam_mode: request.anchor_seam_mode,
    }
}

impl SeamGateConfig {
    /// Build the run-constant seam-gate config from a repair request. Same field math production
    /// used inline in `prepare_region_patch`; reused by the W5 oracle so its scores match
    /// production by construction. `silence_peak_fraction` comes from the per-run patch context.
    pub(crate) fn from_repair(
        request: &PatchAudioRequest,
        sample_rate: u32,
        channels: usize,
        silence_peak_fraction: f32,
    ) -> Self {
        let context_frames =
            (request.gap_signature_context_secs * sample_rate as f64).round() as usize;
        let bin_frames =
            ((request.gap_signature_bin_ms as f64 / 1000.0) * sample_rate as f64).round() as usize;
        let border_standoff_frames =
            (request.border_standoff_secs * sample_rate as f64).round() as usize;
        let search_radius_frames =
            (request.fill_border_search_secs * sample_rate as f64).round() as usize;
        let fill_length_slack_frames =
            (request.fill_length_slack_secs * sample_rate as f64).round() as usize;
        let max_extend_frames =
            (request.gap_end_extend_max_ms as f64 / 1000.0 * sample_rate as f64).round() as usize;
        let step_frames =
            (request.gap_end_extend_step_ms as f64 / 1000.0 * sample_rate as f64).round() as usize;
        SeamGateConfig {
            channels,
            sample_rate,
            context_frames,
            bin_frames,
            border_standoff_frames,
            search_radius_frames,
            fill_length_slack_frames,
            max_extend_frames,
            step_frames,
            residual_max_lag_frames: crate::domain::residual_max_lag_frames(
                sample_rate,
                request.residual_lag_secs,
            ),
            normalize_window_secs: request.normalize_window_secs,
            min_border_discovery_secs: request.min_border_discovery_secs,
            fill_seam_search_secs: request.fill_seam_search_secs,
            silence_peak_fraction,
            absolute_silence_rms: request.absolute_silence_rms,
            min_structure_match_score: request.min_structure_match_score,
            strong_structure_trust: request.strong_structure_trust,
            disable_structure_trust: request.disable_structure_trust,
            partial_structure_waveform_soften: request.partial_structure_waveform_soften,
            min_fill_correlation: request.min_fill_correlation,
            short_gap_mean_correlation_secs: request.short_gap_mean_correlation_secs,
            short_gap_one_strong_seam_fallback: request.short_gap_one_strong_seam_fallback,
            fill_mode: request.fill_mode,
            fill_fit_structure_weight: request.fill_fit_structure_weight,
            fill_fit_waveform_weight: request.fill_fit_waveform_weight,
            fill_fit_nominal_bias_scale: request.fill_fit_nominal_bias_scale,
            fill_fit_energy_nominal_bias_scale: request.fill_fit_energy_nominal_bias_scale,
            fill_fit_late_start_penalty_scale: request.fill_fit_late_start_penalty_scale,
            fill_marginal_margin: request.fill_marginal_margin,
            fill_absolute_floor: request.fill_absolute_floor,
            fill_repeat_penalty_weight: request.fill_repeat_penalty_weight,
            gap_end_extend_on_post_seam_fail: request.gap_end_extend_on_post_seam_fail,
            gap_start_extend_on_pre_seam_fail: request.gap_start_extend_on_pre_seam_fail,
            gap_signature_mode: request.gap_signature_mode,
            fit_boundary_search: request.fit_boundary_search,
            anchor_seam_mode: request.anchor_seam_mode,
            max_anchor_bracket_secs: request.max_anchor_bracket_secs,
            max_anchors_per_side: request.max_anchors_per_side,
            anchor_seam_min_prominence: request.anchor_seam_min_prominence,
            anchor_matchability:
                crate::domain::gap_anchor_seam::AnchorMatchabilityParams::from_repair_fields(
                    request.anchor_seam_min_match_pearson,
                    request.anchor_seam_min_xcorr_peak,
                    request.anchor_seam_xcorr_ambiguous_band,
                ),
            measure_residual: request.measure_residual,
            residual_gate: request.residual_gate,
            residual_floor_ok_db: request.residual_floor_ok_db,
            residual_headroom_margin_db: request.residual_headroom_margin_db,
        }
    }
}

pub(crate) fn border_frames_from_secs(window_secs: f64, sample_rate: u32) -> usize {
    (window_secs * sample_rate as f64) as usize
}

/// Cap for fine-align slide search and seam correlation gate (frames).
pub(crate) fn seam_gate_frames_for(
    correlate_frames: usize,
    fill_seam_search_secs: f64,
    sample_rate: u32,
) -> usize {
    let cap = (fill_seam_search_secs * sample_rate as f64).round() as usize;
    correlate_frames.min(cap).max(1)
}

/// Seam correlation window sized to the gap (short gaps use shorter templates).
pub(crate) fn correlate_frames_for_gap(
    normalize_window_secs: f64,
    min_border_discovery_secs: f64,
    gap_frames: usize,
    sample_rate: u32,
) -> usize {
    let gap_secs = gap_frames as f64 / sample_rate as f64;
    let window_secs = normalize_window_secs
        .min(gap_secs * 0.45)
        .min(2.0)
        .max(min_border_discovery_secs)
        .max(0.25);
    ((window_secs * sample_rate as f64) as usize).max(1)
}

fn slice_b_segment(
    b_samples: &[f32],
    channels: usize,
    sample_rate: u32,
    start_secs: f64,
    end_secs: f64,
) -> Option<&[f32]> {
    let channels = channels.max(1);
    let start_frame = (start_secs * sample_rate as f64).round() as usize;
    let end_frame = ((end_secs * sample_rate as f64).round() as usize)
        .min(b_samples.len() / channels);
    if start_frame >= end_frame {
        return None;
    }
    Some(&b_samples[start_frame * channels..end_frame * channels])
}

/// Compute RMS of the A samples bordering the gap region.
///
/// Looks at `window_secs` before the gap start and `window_secs` after the gap end.
/// Returns `fallback` when the computed RMS is zero.
fn compute_a_border_rms(
    a_pcm: &MultiChannelPcm,
    gap_start_frame: usize,
    gap_end_frame: usize,
    window_secs: f64,
    fallback: f32,
) -> f32 {
    let channels = a_pcm.channels as usize;
    let sample_rate = a_pcm.sample_rate;
    let window_frames = border_frames_from_secs(window_secs, sample_rate);

    let pre_start = gap_start_frame.saturating_sub(window_frames);
    let pre_end = gap_start_frame;
    let post_start = gap_end_frame;
    let post_end = (gap_end_frame + window_frames).min(a_pcm.samples.len() / channels);

    let pre_samples = &a_pcm.samples[pre_start * channels..pre_end * channels];
    let post_samples = &a_pcm.samples[post_start * channels..post_end * channels];

    let total = pre_samples.len() + post_samples.len();
    if total == 0 {
        return fallback;
    }

    let sum_sq: f64 = pre_samples
        .iter()
        .chain(post_samples.iter())
        .map(|&s| {
            let v = s as f64;
            v * v
        })
        .sum();

    let rms = (sum_sq / total as f64).sqrt() as f32;
    if rms == 0.0 { fallback } else { rms }
}

/// Splice B samples into A's interleaved sample buffer at the gap location.
fn splice_into_a(
    a_samples: &mut [f32],
    b_samples: &[f32],
    channels: usize,
    gap_start_frame: usize,
    gap_end_frame: usize,
    crossfade_secs: f64,
    sample_rate: u32,
) {
    let channels = channels.max(1);

    if gap_start_frame * channels >= a_samples.len()
        || gap_end_frame * channels > a_samples.len()
    {
        tracing::warn!(
            gap_start_frame,
            gap_end_frame,
            a_len = a_samples.len() / channels,
            "splice_into_a: region out of range, skipping"
        );
        return;
    }

    if gap_start_frame >= gap_end_frame {
        return;
    }

    let gap_frames = gap_end_frame.saturating_sub(gap_start_frame);
    let needed_samples = gap_frames * channels;
    if b_samples.len() < needed_samples {
        tracing::warn!(
            gap_start_frame,
            gap_end_frame,
            b_len = b_samples.len(),
            needed_samples,
            "splice_into_a: B fill shorter than gap, skipping"
        );
        return;
    }

    let crossfade_frames = (crossfade_secs * sample_rate as f64) as usize;

    policies::apply_seam_crossfade(
        a_samples,
        b_samples,
        channels,
        gap_start_frame,
        gap_end_frame,
        crossfade_frames,
    );
}

#[cfg(test)]
mod tests {
    use super::{
        anchored_retry_gap_indices, dual_fit_eligible, format_gap_fill_plan_lines,
        format_gap_fill_result_line, measure_dual_fit_residual_verdict,
        should_apply_anchored_retry_outcome, skipped_patch, DualFitRepairInput,
        GapFillPlanLog, GapFillResultLog, PatchAudioRequest, RegionPatchOutcome, SeamGateFailure,
    };
    use crate::domain::gap::GapReport;
    use crate::domain::gap_fill_fit::FillConfidence;
    use crate::domain::gap_fill_fit::fit_fill_to_gap_frames;
    use crate::domain::patch_result::GapPatchSkipReason;
    use crate::domain::gap_tags::{GapTags, PatchTier, PlanKind, SeamShape};
    use crate::domain::{FillOffsetMode, Gap};

    fn dummy_region_tags() -> GapTags {
        GapTags {
            plan_kind: PlanKind::Fillable,
            plan_skip_reason: None,
            patch_tier: PatchTier::NotApplicable,
            seam_shape: SeamShape::NotApplicable,
            fit_path: None,
            signature_mode: None,
            residual_band: None,
            anchor_seam_used: false,
            anchor_bracket_move_frames: 0,
            dual_fit_used: false,
        }
    }

    #[test]
    fn dual_fit_eligible_excludes_structure_alignment_failed() {
        // A2: `StructureAlignmentFailed` means structure search never scored a bracket, so there is
        // nothing "exhausted" for dual-fit to rescue — it must fall through to the ordinary skip
        // regardless of whether `--dual-fit` is on.
        assert!(!dual_fit_eligible(true, SeamGateFailure::StructureAlignmentFailed));
        assert!(!dual_fit_eligible(false, SeamGateFailure::StructureAlignmentFailed));

        // Other seam-gate failure variants are scored-but-failed skips, so they DO qualify when
        // `--dual-fit` is on, and never qualify when it's off.
        let scored_but_failed = SeamGateFailure::StructureBelowThreshold { pre: 0.1, post: 0.1 };
        assert!(dual_fit_eligible(true, scored_but_failed));
        assert!(!dual_fit_eligible(false, scored_but_failed));
    }

    #[test]
    fn anchored_retry_gap_indices_includes_skips_and_optional_marginal() {
        let region_results = vec![
            (
                0.0,
                1.0,
                RegionPatchOutcome::Patched {
                    pre_correlation: 0.5,
                    post_correlation: 0.5,
                    align_adjustment_secs: 0.0,
                    waveform_adjustment_secs: 0.0,
                    structure_trusted: false,
                    confidence: FillConfidence::High,
                    gap_start_adjust_frames: 0,
                    gap_end_adjust_frames: 0,
                    fit_used_boundary_grid: false,
                    fit_boundary_grid_cells: None,
                    residual: None,
                    anchor_seam_used: false,
                    anchor_bracket_move_frames: 0,
                    dual_fit_used: false,
                },
                dummy_region_tags(),
            ),
            (
                2.0,
                3.0,
                RegionPatchOutcome::Patched {
                    pre_correlation: 0.3,
                    post_correlation: 0.28,
                    align_adjustment_secs: 0.1,
                    waveform_adjustment_secs: 0.1,
                    structure_trusted: false,
                    confidence: FillConfidence::Marginal,
                    gap_start_adjust_frames: 0,
                    gap_end_adjust_frames: 0,
                    fit_used_boundary_grid: false,
                    fit_boundary_grid_cells: None,
                    residual: None,
                    anchor_seam_used: false,
                    anchor_bracket_move_frames: 0,
                    dual_fit_used: false,
                },
                dummy_region_tags(),
            ),
            (
                4.0,
                5.0,
                skipped_patch(GapPatchSkipReason::CorrelationBelowThreshold {
                    pre_correlation: 0.1,
                    post_correlation: 0.1,
                    min_correlation: 0.35,
                    best_attempt: None,
                }),
                dummy_region_tags(),
            ),
        ];
        let without = anchored_retry_gap_indices(&region_results, false);
        assert_eq!(without, vec![2]);
        let with = anchored_retry_gap_indices(&region_results, true);
        assert_eq!(with, vec![1, 2]);
    }

    #[test]
    fn should_apply_anchored_retry_outcome_rules() {
        let skip = skipped_patch(GapPatchSkipReason::BoundaryAlignmentFailed);
        let marginal = RegionPatchOutcome::Patched {
            pre_correlation: 0.3,
            post_correlation: 0.28,
            align_adjustment_secs: 0.0,
            waveform_adjustment_secs: 0.0,
            structure_trusted: false,
            confidence: FillConfidence::Marginal,
            gap_start_adjust_frames: 0,
            gap_end_adjust_frames: 0,
            fit_used_boundary_grid: false,
            fit_boundary_grid_cells: None,
            residual: None,
            anchor_seam_used: false,
            anchor_bracket_move_frames: 0,
            dual_fit_used: false,
        };
        let high = RegionPatchOutcome::Patched {
            pre_correlation: 0.5,
            post_correlation: 0.5,
            align_adjustment_secs: 0.0,
            waveform_adjustment_secs: 0.0,
            structure_trusted: false,
            confidence: FillConfidence::High,
            gap_start_adjust_frames: 0,
            gap_end_adjust_frames: 0,
            fit_used_boundary_grid: false,
            fit_boundary_grid_cells: None,
            residual: None,
            anchor_seam_used: false,
            anchor_bracket_move_frames: 0,
            dual_fit_used: false,
        };
        assert!(should_apply_anchored_retry_outcome(&skip, &high));
        assert!(should_apply_anchored_retry_outcome(&marginal, &high));
        assert!(!should_apply_anchored_retry_outcome(&marginal, &marginal));
        assert!(!should_apply_anchored_retry_outcome(&high, &high));
    }

    #[test]
    fn fit_fill_trims_tail_without_resampling() {
        let channels = 2usize;
        let mut samples = Vec::new();
        for frame in 0..10i32 {
            samples.push(frame as f32 * 100.0 / 32767.0);
            samples.push(frame as f32 * 100.0 / 32767.0);
        }
        let fitted = fit_fill_to_gap_frames(&samples, channels, 6);
        assert_eq!(fitted.len(), 12);
        assert!((fitted[0] - 0.0).abs() < 1e-6);
        assert!((fitted[1] - 0.0).abs() < 1e-6);
        assert!((fitted[10] - 500.0 / 32767.0).abs() < 1e-5);
        assert!((fitted[11] - 500.0 / 32767.0).abs() < 1e-5);
    }

    #[test]
    fn fit_fill_zero_pads_short_source() {
        let v1 = 1000.0_f32 / 32767.0;
        let v2 = 2000.0_f32 / 32767.0;
        let samples = vec![v1, v1, v2, v2];
        let fitted = fit_fill_to_gap_frames(&samples, 2, 4);
        assert_eq!(fitted, vec![v1, v1, v2, v2, 0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn format_gap_fill_plan_lines_shows_mapped_and_search_windows() {
        let lines = format_gap_fill_plan_lines(&GapFillPlanLog {
            scan_a_start_secs: 0.0,
            scan_a_end_secs: 3.0,
            refined_a_start_secs: 0.1,
            refined_a_end_secs: 2.9,
            gap_offset_secs: 61.199,
            fill_offset_mode: FillOffsetMode::Interpolated,
            mapped_b_start_secs: 61.299,
            mapped_b_end_secs: 64.099,
            b_search_start_secs: 50.0,
            b_search_end_secs: 80.0,
            signature_mode_label: "energy",
        });
        assert!(lines.iter().any(|l| l.contains("fill offset +61.199s (interpolated)")));
        assert!(lines.iter().any(|l| l.contains("A gap (refined):")));
        assert!(lines.iter().any(|l| l.contains("0:00.100 – 0:02.900")));
        assert!(lines.iter().any(|l| l.contains("B gap (mapped):")));
        assert!(lines.iter().any(|l| l.contains("0:50 – 1:20")));
        assert!(lines.iter().any(|l| l.contains("signature_mode=energy")));
    }

    #[test]
    fn format_gap_fill_result_line_converts_sample_offsets_to_timeline() {
        let line = format_gap_fill_result_line(&GapFillResultLog {
            b_search_start_secs: 50.0,
            sample_rate: 48_000,
            channels: 6,
            fill_start_sample: 48_000 * 6,
            fill_end_sample: 96_000 * 6,
            structure_slide_secs: -0.02,
            waveform_slide_secs: 0.01,
            fit_used_boundary_grid: false,
            fit_boundary_grid_cells: None,
            fit_haystack_secs: 12.0,
            report_pre: 0.31,
            report_post: 1.0,
            confidence: FillConfidence::Marginal,
        });
        assert!(line.contains("B fill source:"));
        assert!(line.contains("structure slide -0.020s"));
        assert!(line.contains("waveform slide +0.010s"));
        assert!(line.contains("0:51.000 – 0:52.000"));
        assert!(line.contains("baseline only (marginal, pre=0.31 post=1.00)"));
    }

    #[test]
    fn format_gap_fill_result_line_shows_boundary_grid_cells() {
        let line = format_gap_fill_result_line(&GapFillResultLog {
            b_search_start_secs: 0.0,
            sample_rate: 48_000,
            channels: 2,
            fill_start_sample: 0,
            fill_end_sample: 96_000,
            structure_slide_secs: 0.0,
            waveform_slide_secs: 0.0,
            fit_used_boundary_grid: true,
            fit_boundary_grid_cells: Some(143),
            fit_haystack_secs: 36.0,
            report_pre: 0.5,
            report_post: 0.5,
            confidence: FillConfidence::High,
        });
        assert!(line.contains("boundary grid (143 cells, haystack 36.0s)"));
    }

    #[test]
    fn skip_gap_fill_log_matches_stdout_gap_number() {
        use crate::domain::format_gap_patch_skip_warn_reason;

        let gaps = vec![
            Gap {
                video_a_start_secs: 0.0,
                video_a_end_secs: 8.0,
                video_b_start_secs: None,
                video_b_end_secs: None,
                b_has_energy: false,
            },
            Gap {
                video_a_start_secs: 6128.25,
                video_a_end_secs: 6360.0,
                video_b_start_secs: Some(0.0),
                video_b_end_secs: Some(1.0),
                b_has_energy: true,
            },
        ];

        assert_eq!(
            super::format_skip_gap_fill_log(
                &gaps,
                6128.25,
                6360.0,
                &format_gap_patch_skip_warn_reason(&GapPatchSkipReason::BoundaryAlignmentFailed),
            ),
            "gap 2/2 (1:42:08 – 1:46:00): structure alignment failed"
        );
    }

    fn dual_fit_test_request(
        measure_residual: bool,
        residual_gate: crate::domain::ResidualGateMode,
    ) -> PatchAudioRequest {
        use crate::domain::align::{AlignedClip, ClipRole, ScanAlignment};

        let alignment = ScanAlignment {
            clips: vec![AlignedClip {
                role: ClipRole::Start,
                window_start_secs: 0.0,
                window_end_secs: 60.0,
                aligned: true,
                offset_secs: Some(0.0),
                confidence: 0.9,
                video_a_decode_skips: 0,
                video_b_decode_skips: 0,
                video_b_window_start_secs: None,
                video_b_window_end_secs: None,
            }],
            start_aligned: true,
            end_aligned: None,
            recommended_offset_secs: Some(0.0),
            offsets_consistent: true,
            offset_drift_secs: None,
            start_overlap: None,
            query_reference_mode: false,
        };
        let report = GapReport {
            video_a: std::path::PathBuf::from("a.wav"),
            video_b: std::path::PathBuf::from("b.wav"),
            track_compatibility: None,
            alignment,
            gaps: Vec::new(),
            gap_offset_agreement: None,
            decode_chunk_secs: 60,
            scan_block_ms: 250,
            silence_peak_fraction: 0.01,
            limit_fill_to_mapped_region: true,
            audio_timeline_skew: None,
        };
        let repair = crate::infrastructure::config::RepairConfig {
            residual_gate,
            absolute_silence_rms: 0.001,
            ..Default::default()
        };
        let mut request = repair.patch_settings().into_request(report);
        request.measure_residual = measure_residual;
        request
    }

    /// Regression for the residual-gate bypass fix: dual-fit's success path used to hardcode
    /// `residual: None` on every rescued patch, silently bypassing `--residual-gate` regardless of
    /// whether it was active for the rest of the run. `measure_dual_fit_residual_verdict` gives
    /// dual-fit its own residual measurement (two independent `chosen_delta`s, one per shoulder,
    /// since dual-fit — unlike the ordinary rigid splice — matches its two shoulders at
    /// independent lags). This builds a synthetic same-master gap (mirrors
    /// `dual_fit::tests::recovers_a_stepped_silence_splice`) where A's raw pre/post border windows
    /// are literal copies of the B content at the placement `try_dual_fit` actually matched, so the
    /// chosen probe should cancel almost perfectly against the floor and the gate must not reject.
    #[test]
    fn measure_dual_fit_residual_verdict_attaches_a_real_verdict() {
        let sr = 48_000u32;
        let ch = 1usize;
        let w = 1200usize;
        let gap = 4000usize;
        let step = 200i64;

        let mut seed = 0xDEAD_BEEF_1234_5678u64;
        let mut rng = move || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((seed >> 33) as f64 / (1u64 << 30) as f64) - 1.0
        };
        let bn = 40_000usize;
        let b_mono: Vec<f64> = (0..bn).map(|_| rng()).collect();
        let b_samples: Vec<f32> = b_mono.iter().map(|&x| x as f32).collect();

        let b_mapped_start = 10_000usize;
        let a_pre_mono: Vec<f64> = b_mono[b_mapped_start - w..b_mapped_start].to_vec();
        let post_src = b_mapped_start + gap + step as usize;
        let a_post_mono: Vec<f64> = b_mono[post_src..post_src + w].to_vec();

        let params = crate::domain::dual_fit::DualFitParams {
            channels: ch,
            sample_rate: sr,
            gap_frames: gap,
            seam_window_frames: w,
            max_lag_frames: (0.6 * sr as f64) as usize,
            min_fill_correlation: 0.35,
            fill_absolute_floor: 0.12,
            step_real_margin: 0.15,
            a_gap_floor_db: -60.0,
        };
        let r = crate::domain::dual_fit::try_dual_fit(
            &a_pre_mono,
            &a_post_mono,
            &b_mono,
            &b_samples,
            b_mapped_start,
            &params,
        )
        .expect("dual-fit target");
        assert_eq!(r.pre_lag, 0, "pre shoulder matches at nominal lag in this fixture");
        assert_eq!(r.post_lag, step, "post shoulder's seam-local match sits at +step in B");

        // A's raw audio around the (arbitrary, far-away) gap position: the pre/post border windows
        // are literal copies of the B content at the matched placement, so chosen == floor.
        let a_start_frame = 500_000usize;
        let a_end_frame = a_start_frame + gap;
        let mut a_samples = vec![0.0f32; a_end_frame + w];
        for (i, &v) in a_pre_mono.iter().enumerate() {
            a_samples[a_start_frame - w + i] = v as f32;
        }
        for (i, &v) in a_post_mono.iter().enumerate() {
            a_samples[a_end_frame + i] = v as f32;
        }

        let df = DualFitRepairInput {
            params,
            a_pre_mono,
            a_post_mono,
            b_mono,
            b_samples: &b_samples,
            b_mapped_start,
            a_start_frame,
            a_end_frame,
            crossfade_secs: 0.02,
            a_samples: &a_samples,
            border_standoff_frames: 0,
        };

        let request = dual_fit_test_request(true, crate::domain::ResidualGateMode::VetoRescue);
        let verdict = measure_dual_fit_residual_verdict(&request, &df, &r)
            .expect("residual measurement must run when measure_residual is set");
        assert!(
            verdict.informative,
            "both sides found energetic reference windows; verdict must be informative: {verdict:?}"
        );
        let headroom = verdict.worst_headroom_db();
        assert!(
            headroom < 1.0,
            "chosen placement is a literal copy of the matched B content, so headroom vs. the \
             nominal floor should be ~0 dB, not a rejection-worthy value: {headroom}"
        );
        let gated = crate::domain::gap_fill_fit::apply_residual_to_confidence(
            Ok(FillConfidence::High),
            &verdict,
            request.residual_headroom_margin_db,
            request.residual_gate.rescue_enabled(),
        );
        assert_eq!(
            gated,
            Ok(FillConfidence::High),
            "a well-cancelling dual-fit candidate must not be rejected by the residual gate"
        );

        // Off by default: no residual measurement, matching the ordinary path's `want_residual_measurement`.
        let off_request = dual_fit_test_request(false, crate::domain::ResidualGateMode::Off);
        assert!(measure_dual_fit_residual_verdict(&off_request, &df, &r).is_none());
    }
}
