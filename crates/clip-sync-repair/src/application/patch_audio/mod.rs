use clip_sync::{format_time_range_verbose, MediaReader, ProgressReporter};

use crate::application::error::RepairError;
use crate::domain::{
    fill_offset::{resolve_gap_offset_secs, AnchoredRetryPass, FillOffsetMode},
    diagnostics::{pcm_container_duration_skew, PCM_CONTAINER_WARN_SECS},
    fill_mode::FillMode,
    format_repair_profile_verbose,
    inactive_repair_flag_notes,
    gap_fill_fit::FillConfidence,
    gap_fill::{build_gap_fill_plan, format_align_fill_regions_phase, FillRegion},
    gap_tags::{FillTierThresholds, GapTags, GapTagsPatchContext},
    patch_anchor::{
        format_patch_anchor_table_summary, is_retryable_patch_skip, PatchAnchorCandidate,
        PatchAnchorPolicy, PatchAnchorTable,
    },
    patch_result::PatchSummary,
    policies,
};

mod decode;
mod geometry;
mod log;
mod region;
mod request;

pub use request::{PatchAudioRequest, PatchAudioResult, PatchRequestSettings};

pub(crate) use decode::{decode_ab, DecodedAb};
pub(crate) use geometry::{
    border_frames_from_secs, correlate_frames_for_gap, seam_gate_frames_for,
};
use geometry::repair_patch_config_view;
use log::log_gap_tags_verbose;
use region::{
    characterize_region, execute_region_spec, outcomes_in_report_order, prepare_region_patch,
    record_patch_gap_span, region_outcome_gap_tags, skip_outcome_from_spec, splice_into_a,
    RegionCharacterization, RegionPatch, RegionPatchContext, RegionPatchMedia,
    RegionPatchOpts, RegionPatchOutcome,
};

/// How far gap edges may be adjusted against A's decoded PCM (seconds).
const GAP_EDGE_REFINE_SECS: f64 = 0.75;


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
        // Step 1: Build fill plan (may be empty when tracks mismatch or no B energy). The equivalence
        // gate drops already-equivalent gaps here (when enabled) so they never reach decode/patch.
        let plan = build_gap_fill_plan(&request.report, crossfade_ms, request.skip_equivalent_gaps);

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

        // 6c: two passes — characterize ALL regions (decisions), then execute ALL patches (PCM) — instead
        // of the per-region `prepare_region_patch` shim. Same components (`characterize_region` +
        // `execute_region_spec`), byte-identical output; the per-gap tracing/progress reorders (char pass
        // then exec pass) but no tested surface (PCM/outcomes) changes. The anchored-retry (below) still
        // re-runs the `prepare_region_patch` shim on failed gaps.
        //
        // Pass 1 — characterize.
        let mut characterizations: Vec<(RegionCharacterization, GapTagsPatchContext)> =
            Vec::with_capacity(plan.regions.len());
        for (index, region) in plan.regions.iter().enumerate() {
            let gap_num = index as u64 + 1;
            self.progress.progress("patch-characterize", gap_num, region_count);
            self.progress.phase_verbose(&format!(
                "  gap {gap_num}/{region_count}: A {}",
                format_time_range_verbose(region.a_start_secs, region.a_end_secs)
            ));
            characterizations.push(characterize_region(
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
            ));
        }

        // Pass 2 — execute patches (skips carry their outcome; nothing to run).
        for ((characterization, tag_ctx), (index, region)) in
            characterizations.into_iter().zip(plan.regions.iter().enumerate())
        {
            let gap_num = index as u64 + 1;
            self.progress.progress("patch-gap", gap_num, region_count);
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

            let (patch, outcome) = match characterization {
                RegionCharacterization::Patch { spec, bracket_fill } => {
                    execute_region_spec(spec, bracket_fill, region_ctx.sample_rate)
                }
                RegionCharacterization::Skip(spec) => (None, skip_outcome_from_spec(&spec)),
            };
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



fn patch_anchor_policy(request: &PatchAudioRequest) -> PatchAnchorPolicy {
    PatchAnchorPolicy {
        min_correlation: request.fill_anchor_min_correlation,
        exclude_structure_trusted: request.fill_anchor_exclude_structure_trusted,
        max_adjustment_frac: request.fill_anchor_max_adjustment_frac,
        border_search_secs: request.fill_border_search_secs,
    }
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


#[cfg(test)]
mod tests {
    use super::region::skipped_patch;
    use super::{
        anchored_retry_gap_indices, should_apply_anchored_retry_outcome, RegionPatchOutcome,
    };
    use crate::domain::gap_fill_fit::FillConfidence;
    use crate::domain::patch_result::GapPatchSkipReason;
    use crate::domain::gap_tags::{GapTags, PatchTier, PlanKind, SeamShape};

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

}
