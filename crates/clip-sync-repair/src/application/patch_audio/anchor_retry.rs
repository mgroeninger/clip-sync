use clip_sync::{format_time_range_verbose, ProgressReporter};

use crate::domain::{
    fill_mode::FillMode,
    fill_offset::{resolve_gap_offset_secs, AnchoredRetryPass},
    gap_fill::FillRegion,
    gap_fill_fit::FillConfidence,
    gap_tags::GapTags,
    patch_anchor::{
        is_retryable_patch_skip, PatchAnchorCandidate, PatchAnchorPolicy, PatchAnchorTable,
    },
};

use super::log::log_gap_tags_verbose;
use super::region::{
    prepare_region_patch, record_patch_gap_span, region_outcome_gap_tags, RegionPatch,
    RegionPatchContext, RegionPatchMedia, RegionPatchOpts, RegionPatchOutcome,
};
use super::PatchAudioRequest;

pub(super) fn patch_anchor_policy(request: &PatchAudioRequest) -> PatchAnchorPolicy {
    PatchAnchorPolicy {
        min_correlation: request.fill_anchor_min_correlation,
        exclude_structure_trusted: request.fill_anchor_exclude_structure_trusted,
        max_adjustment_frac: request.fill_anchor_max_adjustment_frac,
        border_search_secs: request.fill_border_search_secs,
    }
}

pub(super) fn build_patch_anchor_candidates(
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

pub(super) struct AnchoredRetryState<'a> {
    pub(super) patches: &'a mut Vec<RegionPatch>,
    pub(super) patch_slot_by_gap: &'a mut [Option<usize>],
    pub(super) region_results: &'a mut [(f64, f64, RegionPatchOutcome, GapTags)],
}

pub(super) fn run_anchored_retry_pass(
    progress: &dyn ProgressReporter,
    state: &mut AnchoredRetryState<'_>,
    regions: &[FillRegion],
    request: &PatchAudioRequest,
    ctx: &RegionPatchContext,
    media: &RegionPatchMedia<'_>,
    table: &PatchAnchorTable,
) {
    let retry_marginal = request.fill_anchor_retry_marginal && request.fill_mode == FillMode::Fit;
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
                store_anchored_retry_patch(state.patches, state.patch_slot_by_gap, index, patch);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::region::skipped_patch;
    use super::{
        anchored_retry_gap_indices, should_apply_anchored_retry_outcome, RegionPatchOutcome,
    };
    use crate::domain::gap_fill_fit::FillConfidence;
    use crate::domain::gap_tags::{GapTags, PatchTier, PlanKind, SeamShape};
    use crate::domain::patch_result::GapPatchSkipReason;

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
