//! Leaf types for the region patch pipeline: the patch / outcome payloads, the
//! media / opts / context bundles, the report-order join, and the spec -> outcome mapping.
//!
//! Imports nothing else from `region/` — every other submodule may depend on this one.

use std::collections::HashMap;

use clip_sync::{MultiChannelPcm, ProgressReporter};

use crate::application::patch_region::SeamGateFailure;
use crate::domain::{
    fill_mode::FillMode,
    fill_offset::AnchoredRetryPass,
    gap_fill::{FillRegion, GapFillPlan},
    gap_fill_fit::{FillConfidence, UnifiedFitBreakdown},
    gap_repair_spec::{
        BExtractWindow, GapRepairSpec, GapRepairStrategy, GapRepairTags, GapRepairVerdict,
    },
    gap_tags::{
        derive_gap_tags_from_patch_outcome, derive_gap_tags_from_status, FillTierThresholds,
        GapPatchTierInput, GapTags, GapTagsPatchContext,
    },
    patch_anchor::PatchAnchorTable,
    patch_result::{
        residual_summary_scalar_fields, GapFillSkipReason, GapPatchNotAppliedReason,
        GapPatchOutcome, GapPatchSkipReason, GapPatchStatus,
    },
    policies::{self, RefinedGapFrames},
    Gap,
};

use super::super::log::log_skip_gap_fill;

/// B audio contiguous with the fill, one crossfade width past each gap edge, so both seam blends
/// have their own content. `None` on every path that cannot supply it: the flag off, a dual-fit
/// bridge (synthesized, not a contiguous B slice), or a fill sitting within `cf` of either end of B.
/// A `None` splices exactly as it always did.
#[derive(Debug, PartialEq)]
pub(in crate::application::patch_audio) struct SeamMargin {
    pub(in crate::application::patch_audio) head: Vec<f32>,
    pub(in crate::application::patch_audio) tail: Vec<f32>,
}

// Collected B segment ready to splice into A.
#[derive(Debug, PartialEq)]
pub(in crate::application::patch_audio) struct RegionPatch {
    pub(in crate::application::patch_audio) b_samples: Vec<f32>,
    /// See [`SeamMargin`]. `None` keeps the shipped seam path byte-for-byte.
    pub(in crate::application::patch_audio) seam_margin: Option<SeamMargin>,
    pub(in crate::application::patch_audio) gain: f32,
    pub(in crate::application::patch_audio) a_start_frame: usize,
    pub(in crate::application::patch_audio) a_end_frame: usize,
    pub(in crate::application::patch_audio) crossfade_secs: f64,
}

#[derive(Debug, PartialEq)]
pub(in crate::application::patch_audio) enum RegionPatchOutcome {
    Patched {
        pre_correlation: f64,
        post_correlation: f64,
        align_adjustment_secs: f64,
        structure_trusted: bool,
        confidence: FillConfidence,
        gap_start_adjust_frames: i64,
        gap_end_adjust_frames: i64,
        fit_used_boundary_grid: bool,
        fit_boundary_grid_cells: Option<u32>,
        residual: Option<policies::SeamResidualVerdict>,
        anchor_seam_used: bool,
        anchor_bracket_move_frames: usize,
        /// Tail frames the fill fell short of the gap by, because contiguous B ran out. Not written:
        /// the patch destination is shortened by this much, so A keeps its own material there. `0` on
        /// a fill that reached gap length.
        zero_pad_frames: usize,
        dual_fit_used: bool,
        /// `Some(step_real)` when `dual_fit_used`: `true` = real length edit (interior-trim bridge),
        /// `false` = rigid single-lag map (trim-0 extract, docs/dev/archive/TEMP-seam-lock-rigid-fill-plan.md
        /// §3.1). `None` on every non-dual-fit path (`dual_fit_used == false`).
        dual_fit_step_real: Option<bool>,
        /// When `dual_fit_used`: the post seam's window width (ms) when a 250 ms post lock below
        /// the admit line was re-locked at a wider rung
        /// (`docs/dev/archive/TEMP-dual-fit-grow-post-plan.md` §3.2/§3.3). `None` when the post lock stayed
        /// at the 250 ms `fill_seam_search_secs` window, and on every non-dual-fit path.
        dual_fit_post_window_ms: Option<u32>,
        /// The gain actually applied to this fill — whole-program × hole-local. Carried to the
        /// patch record so `fill_level`'s `gained_fill`-based fields can be read across gain-policy
        /// changes.
        normalize_gain: f32,
        /// The hole-local half of [`Self::Patched::normalize_gain`], carried whole so the reason
        /// survives to the patch record (`domain::local_gain`).
        local_gain: crate::domain::LocalGainDecision,
        /// The unified fit objective's terms at the applied placement; `None` on paths that never ran
        /// it (dual-fit, splice). Carried through to `GapPatchStatus::Patched::fit_terms`.
        fit_terms: Option<UnifiedFitBreakdown>,
        /// S1: independent shoulder-seam read of the placement vs. the applied placement. Record-only —
        /// no gate reads this. Carried through to `GapPatchStatus::Patched::registration_residual_ms`.
        registration_residual: Option<Box<crate::domain::patch_result::RegistrationResidual>>,
    },
    Skipped {
        reason: GapPatchSkipReason,
        residual: Option<policies::SeamResidualVerdict>,
    },
}

pub(in crate::application::patch_audio) fn skipped_patch(
    reason: GapPatchSkipReason,
) -> RegionPatchOutcome {
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

pub(in crate::application::patch_audio) fn record_patch_gap_span(
    span: &tracing::Span,
    outcome: &RegionPatchOutcome,
) {
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

pub(in crate::application::patch_audio) struct RegionPatchMedia<'a> {
    pub(in crate::application::patch_audio) b_samples_full: &'a [f32],
    pub(in crate::application::patch_audio) a_pcm: &'a MultiChannelPcm,
}

pub(in crate::application::patch_audio) struct RegionPatchOpts<'a> {
    pub(in crate::application::patch_audio) anchored_retry_pass: AnchoredRetryPass,
    pub(in crate::application::patch_audio) patch_anchors: Option<&'a PatchAnchorTable>,
}

/// Join plan skips and per-region results back onto the report's gap list, in report order.
///
/// Keyed by `gap_index` (the position in [`GapReport::gaps`](crate::domain::gap::GapReport::gaps)) rather than by the gap's float
/// timestamps: the patch path refines `a_start_secs` locally, so timestamp identity is a
/// correctness trap waiting for someone to pass a refined value. The index also makes two gaps
/// with identical bounds distinguishable, which bit-pattern keys could not.
///
/// `not_applied` overrides the region outcome for gaps whose splice failed after the gate approved
/// them: those report [`GapPatchStatus::NotApplied`], never `Patched`. Callers that do not splice
/// (empty plan, preview) pass an empty slice.
///
/// `fill_level` is the record-only measurement of the PCM that was actually written, so it too is
/// empty for callers that do not splice.
/// Run-constant stamps [`outcomes_in_report_order`] writes onto every row.
///
/// Distinct from the five join sources (`gaps` / `plan` / `region_results` / `not_applied` /
/// `fill_level`): those are differently-ordered inputs this function exists to join. These three
/// are the same on every gap.
#[derive(Clone, Copy)]
pub(in crate::application::patch_audio) struct ReportStamps {
    pub fill_mode: FillMode,
    pub thresholds: FillTierThresholds,
    pub program_gain: f32,
}

/// Join per-gap sources into report order.
///
/// `gaps` / `plan` / `region_results` / `not_applied` / `fill_level` are five differently-ordered
/// sources this function exists to join. [`ReportStamps`] is the run-constant cluster stamped onto
/// the result.
pub(in crate::application::patch_audio) fn outcomes_in_report_order(
    gaps: &[Gap],
    plan: &GapFillPlan,
    region_results: &[(usize, RegionPatchOutcome, GapTags)],
    stamps: ReportStamps,
    not_applied: &[(usize, GapPatchNotAppliedReason)],
    fill_level: &[(usize, crate::domain::FillLevelCheck)],
) -> Vec<GapPatchOutcome> {
    let ReportStamps {
        fill_mode,
        thresholds,
        program_gain,
    } = stamps;
    let mut fill_level_by_gap: HashMap<usize, crate::domain::FillLevelCheck> =
        fill_level.iter().copied().collect();
    let mut status_by_gap: HashMap<usize, GapPatchStatus> = HashMap::new();
    let mut tags_by_gap: HashMap<usize, GapTags> = HashMap::new();
    let mut residual_by_gap: HashMap<usize, policies::SeamResidualVerdict> = HashMap::new();

    for skip in &plan.skipped {
        let key = skip.gap_index;
        let status = GapPatchStatus::NotPlanned {
            reason: skip.reason.clone(),
        };
        let tags = derive_gap_tags_from_status(&status, fill_mode, thresholds);
        status_by_gap.insert(key, status);
        tags_by_gap.insert(key, tags);
    }

    for (gap_index, outcome, tags) in region_results {
        let status = match outcome {
            RegionPatchOutcome::Patched {
                pre_correlation,
                post_correlation,
                align_adjustment_secs,
                structure_trusted,
                confidence,
                gap_start_adjust_frames,
                gap_end_adjust_frames,
                residual,
                anchor_seam_used,
                anchor_bracket_move_frames,
                zero_pad_frames,
                dual_fit_used,
                dual_fit_step_real,
                dual_fit_post_window_ms,
                normalize_gain,
                local_gain,
                fit_terms,
                registration_residual,
                ..
            } => {
                if let Some(verdict) = residual {
                    residual_by_gap.insert(*gap_index, *verdict);
                }
                let (residual_db, floor_db, headroom_db) =
                    residual_summary_scalar_fields(residual.as_ref());
                GapPatchStatus::Patched {
                    normalize_gain: *normalize_gain,
                    program_gain,
                    local_gain: local_gain.gain(),
                    local_gain_reason: local_gain.reason(),
                    pre_correlation: *pre_correlation,
                    post_correlation: *post_correlation,
                    align_adjustment_secs: *align_adjustment_secs,
                    structure_trusted: *structure_trusted,
                    confidence: *confidence,
                    gap_start_adjust_frames: *gap_start_adjust_frames,
                    gap_end_adjust_frames: *gap_end_adjust_frames,
                    residual_db,
                    floor_db,
                    headroom_db,
                    anchor_seam_used: *anchor_seam_used,
                    anchor_bracket_move_frames: *anchor_bracket_move_frames,
                    zero_pad_frames: *zero_pad_frames,
                    dual_fit_used: *dual_fit_used,
                    dual_fit_step_real: *dual_fit_step_real,
                    dual_fit_post_window_ms: *dual_fit_post_window_ms,
                    fit_terms: *fit_terms,
                    registration_residual: registration_residual.clone(),
                }
            }
            RegionPatchOutcome::Skipped { reason, residual } => {
                if let Some(verdict) = residual {
                    residual_by_gap.insert(*gap_index, *verdict);
                }
                GapPatchStatus::Skipped {
                    reason: reason.clone(),
                }
            }
        };
        status_by_gap.insert(*gap_index, status);
        tags_by_gap.insert(*gap_index, tags.clone());
    }

    // Applied last so it wins over the region outcome: the gate said patch, the splice did not.
    for (gap_index, reason) in not_applied {
        let status = GapPatchStatus::NotApplied {
            reason: reason.clone(),
        };
        let tags = derive_gap_tags_from_status(&status, fill_mode, thresholds);
        status_by_gap.insert(*gap_index, status);
        tags_by_gap.insert(*gap_index, tags);
    }

    gaps.iter()
        .enumerate()
        .map(|(key, gap)| {
            let status = status_by_gap
                .remove(&key)
                .unwrap_or(GapPatchStatus::NotPlanned {
                    reason: GapFillSkipReason::NotFillable,
                });
            let tags = tags_by_gap
                .remove(&key)
                .unwrap_or_else(|| derive_gap_tags_from_status(&status, fill_mode, thresholds));
            GapPatchOutcome::new(gap.video_a_start_secs, gap.video_a_end_secs, status, tags)
                .with_residual(residual_by_gap.remove(&key))
                .with_fill_level(fill_level_by_gap.remove(&key))
        })
        .collect()
}

/// Per-run values derived once in `execute` and shared by every fill region.
pub(in crate::application::patch_audio) struct RegionPatchContext {
    pub(in crate::application::patch_audio) channels: usize,
    pub(in crate::application::patch_audio) sample_rate: u32,
    pub(in crate::application::patch_audio) max_refine_frames: usize,
    /// From the scan report (matches the thresholds the gaps were detected with).
    pub(in crate::application::patch_audio) silence_peak_fraction: f32,
}

pub(in crate::application::patch_audio) fn region_outcome_gap_tags(
    outcome: &RegionPatchOutcome,
    mut tag_ctx: GapTagsPatchContext,
) -> GapTags {
    if let RegionPatchOutcome::Patched {
        anchor_seam_used,
        anchor_bracket_move_frames,
        dual_fit_used,
        dual_fit_step_real,
        ..
    } = outcome
    {
        tag_ctx.anchor_seam_used = *anchor_seam_used;
        tag_ctx.anchor_bracket_move_frames = *anchor_bracket_move_frames;
        tag_ctx.dual_fit_used = *dual_fit_used;
        tag_ctx.dual_fit_step_real = *dual_fit_step_real;
    }
    tag_ctx.residual = match outcome {
        RegionPatchOutcome::Patched { residual, .. }
        | RegionPatchOutcome::Skipped { residual, .. } => *residual,
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

pub(in crate::application::patch_audio::region) fn seam_failure_outcome(
    progress: &dyn ProgressReporter,
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
    log_skip_gap_fill(progress, region, &reason);
    let outcome = if let Some(residual) = residual {
        skipped_patch_with_residual(reason, Some(residual))
    } else {
        skipped_patch(reason)
    };
    (None, outcome, tag_ctx)
}

/// Build a `Skip`-verdict [`GapRepairSpec`] (8d). Carries the `reason` (in the verdict) and `residual` (in
/// `tags_ctx.gate.residual`) so [`skip_outcome_from_fields`] reproduces the identical [`RegionPatchOutcome`].
/// `gap_index` is `region.gap_index` — the same copy the bracket spec and dual-fit finalize make, so a skip
/// spec is a complete region identity.
/// The cell is derived from the reason + whatever tags are available (`skip_cell_from_tags`); on the decline
/// path tags are partial (no `seam_local`) until 8f, so a class-3/4 decline reports as `Decorrelated` for now
/// — cell correctness is a projection (8f) concern, not an outcome one (§8b guard).
pub(in crate::application::patch_audio::region) fn skip_region_spec(
    reason: GapPatchSkipReason,
    residual: Option<policies::SeamResidualVerdict>,
    region: &FillRegion,
    gap_offset_secs: f64,
    refined: RefinedGapFrames,
    b_extract: BExtractWindow,
) -> GapRepairSpec {
    let mut tags_ctx = GapRepairTags::default();
    tags_ctx.gate.residual = residual;
    let cell = crate::domain::gap_repair_spec::skip_cell_from_tags(&reason, &tags_ctx);
    GapRepairSpec {
        gap_index: region.gap_index,
        a_start_secs: region.a_start_secs,
        a_end_secs: region.a_end_secs,
        gap_offset_secs,
        refined,
        b_extract,
        crossfade_secs: region.crossfade_secs,
        verdict: GapRepairVerdict::skip_with_cell(cell, reason),
        tags_ctx,
    }
}

/// Field-level skip outcome — no verdict match, so no assert arm (§3.2). Callers take `reason` /
/// `residual` after dispatching on `spec.verdict` (or from `outcome_from_spec`'s Skip arm).
pub(in crate::application::patch_audio) fn skip_outcome_from_fields(
    reason: &GapPatchSkipReason,
    residual: Option<policies::SeamResidualVerdict>,
) -> RegionPatchOutcome {
    RegionPatchOutcome::Skipped {
        reason: reason.clone(),
        residual,
    }
}

/// Decision-only outcome from a spec (preview / report path — no PCM). Matches `spec.verdict` once;
/// both arms are live (no `unreachable!`).
pub(in crate::application::patch_audio) fn outcome_from_spec(
    spec: &GapRepairSpec,
    sample_rate: u32,
) -> RegionPatchOutcome {
    match &spec.verdict {
        GapRepairVerdict::Patch(GapRepairStrategy::Bracket {
            alignment,
            structure_trusted,
            anchor_seam_used,
            anchor_bracket_move_frames,
            seam_pre,
            seam_post,
            confidence,
            gap_start_adjust_frames,
            gap_end_adjust_frames,
            fit_used_boundary_grid,
            fit_boundary_grid_cells,
            residual,
            normalize_gain,
            local_gain,
            fit_terms,
            registration_residual,
            ..
        }) => {
            let offset_nominal_start = spec.b_extract.b_mapped_start_frame;
            RegionPatchOutcome::Patched {
                pre_correlation: *seam_pre,
                post_correlation: *seam_post,
                align_adjustment_secs: (alignment.start_frame as f64 - offset_nominal_start as f64)
                    / sample_rate as f64,
                structure_trusted: *structure_trusted,
                confidence: *confidence,
                gap_start_adjust_frames: *gap_start_adjust_frames,
                gap_end_adjust_frames: *gap_end_adjust_frames,
                fit_used_boundary_grid: *fit_used_boundary_grid,
                fit_boundary_grid_cells: *fit_boundary_grid_cells,
                residual: *residual,
                anchor_seam_used: *anchor_seam_used,
                anchor_bracket_move_frames: *anchor_bracket_move_frames,
                // Decision-only: no fill is assembled on this path, so the padding is not knowable
                // here. The executor's outcome carries the real count.
                zero_pad_frames: 0,
                dual_fit_used: false,
                dual_fit_step_real: None,
                dual_fit_post_window_ms: None,
                fit_terms: *fit_terms,
                normalize_gain: *normalize_gain,
                local_gain: *local_gain,
                registration_residual: registration_residual.clone(),
            }
        }
        GapRepairVerdict::Patch(GapRepairStrategy::SilenceSplice {
            pre_seam_r,
            post_seam_r,
            residual,
            confidence,
            registration_residual,
            normalize_gain,
            local_gain,
            step_real,
            dual_fit_post_window_ms,
            ..
        }) => RegionPatchOutcome::Patched {
            pre_correlation: *pre_seam_r,
            post_correlation: *post_seam_r,
            align_adjustment_secs: 0.0,
            // The dual-fit bridge is synthesized to the gap length, not cut and padded to it.
            zero_pad_frames: 0,
            structure_trusted: false,
            confidence: *confidence,
            gap_start_adjust_frames: 0,
            gap_end_adjust_frames: 0,
            fit_used_boundary_grid: false,
            fit_boundary_grid_cells: None,
            residual: *residual,
            anchor_seam_used: false,
            anchor_bracket_move_frames: 0,
            dual_fit_used: true,
            dual_fit_step_real: Some(*step_real),
            dual_fit_post_window_ms: *dual_fit_post_window_ms,
            // Dual-fit does not run the unified objective at all — there is no placement search to report.
            fit_terms: None,
            // Dual-fit takes both halves since 2026-08-31; the strategy carries them so this
            // preview read and `region/execute.rs`'s splice cannot diverge.
            normalize_gain: *normalize_gain,
            local_gain: *local_gain,
            registration_residual: registration_residual.clone(),
        },
        GapRepairVerdict::Skip { reason, .. } => {
            skip_outcome_from_fields(reason, spec.tags_ctx.gate.residual)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Production skip specs copy `FillRegion.gap_index`. A leftover `0` here is the 6b stub that
    /// made capture take the index as a second argument from the loop.
    #[test]
    fn skip_region_spec_copies_the_region_gap_index() {
        let region = FillRegion {
            gap_index: 7,
            a_start_secs: 1.0,
            a_end_secs: 2.0,
            b_start_secs: 1.0,
            b_end_secs: 2.0,
            gain: 1.0,
            crossfade_secs: 0.01,
        };
        let spec = skip_region_spec(
            GapPatchSkipReason::ZeroLengthGap,
            None,
            &region,
            0.0,
            RefinedGapFrames {
                start_frame: 0,
                end_frame: 0,
            },
            BExtractWindow {
                start_frame: 0,
                end_frame: 0,
                b_mapped_start_frame: 0,
            },
        );
        assert_eq!(spec.gap_index, 7);
    }

    /// The dual-fit / silence-splice path takes **both** halves of the gain since 2026-08-31, and
    /// takes them off the strategy rather than the request — so this preview read and the
    /// executor's splice cannot disagree about what was applied. A regression that reverted either
    /// to a literal `1.0` would leave rescued holes at a different level from bracket holes, which
    /// is exactly the split the paired estimators exist to remove.
    #[test]
    fn silence_splice_outcome_carries_both_gain_halves_from_the_strategy() {
        use crate::domain::gap_repair_spec::{GapRepairStrategy, GapRepairTags, GapRepairVerdict};
        use crate::domain::policies::RefinedGapFrames;
        use crate::domain::BExtractWindow;

        let g_program = 0.501_187f32;
        let g_local = crate::domain::LocalGainDecision::Applied { gain: 0.891_251 };
        let spec = GapRepairSpec {
            gap_index: 0,
            a_start_secs: 1.0,
            a_end_secs: 2.0,
            gap_offset_secs: 0.0,
            refined: RefinedGapFrames {
                start_frame: 48_000,
                end_frame: 96_000,
            },
            b_extract: BExtractWindow {
                start_frame: 48_000,
                end_frame: 96_000,
                b_mapped_start_frame: 48_000,
            },
            crossfade_secs: 0.01,
            verdict: GapRepairVerdict::Patch(GapRepairStrategy::SilenceSplice {
                fill: vec![0.0; 48_000],
                seam_margin: None,
                pre_seam_r: 0.98,
                post_seam_r: 0.97,
                pre_lag: 0,
                post_lag: 0,
                trim_frames: 0,
                step_real: true,
                residual: None,
                confidence: crate::domain::FillConfidence::High,
                registration_residual: None,
                normalize_gain: g_program * g_local.gain(),
                local_gain: g_local,
                dual_fit_post_window_ms: None,
            }),
            tags_ctx: GapRepairTags::default(),
        };
        match outcome_from_spec(&spec, 48_000) {
            RegionPatchOutcome::Patched {
                normalize_gain,
                local_gain,
                ..
            } => {
                assert_eq!(normalize_gain, g_program * g_local.gain());
                assert_eq!(local_gain, g_local);
                // And the reason survives to the record: without it the patched row cannot say
                // whether dual-fit was *given* a local gain or merely defaulted to unity.
                assert_eq!(local_gain.reason(), crate::domain::LocalGainReason::Applied);
            }
            other => panic!("expected a patched outcome, got {other:?}"),
        }
    }

    /// **M-GAPKEY — every gap gets its own outcome, in report order, regardless of plan order.**
    /// `outcomes_in_report_order` joins three differently-ordered sources (plan skips, region
    /// results, the report's gap list) onto one index-parallel output. This pins the join itself:
    /// report order is preserved, each of the four provenance classes lands on the *right* gap,
    /// and a gap absent from both plan and results falls back to `NotPlanned/NotFillable`.
    ///
    /// Deliberately feeds the plan and the region results in an order that does **not** match the
    /// report, so a join that accidentally relied on positional zip would fail here.
    #[test]
    fn outcomes_in_report_order_joins_every_gap_by_identity_not_position() {
        use super::{outcomes_in_report_order, skipped_patch, RegionPatchOutcome, ReportStamps};
        use crate::domain::gap::Gap;
        use crate::domain::gap_fill::{GapFillPlan, GapFillSkipped};
        use crate::domain::gap_tags::{derive_gap_tags_from_status, FillTierThresholds};
        use crate::domain::patch_result::{GapFillSkipReason, GapPatchSkipReason, GapPatchStatus};
        use crate::domain::FillMode;

        let gap = |start: f64, end: f64| Gap {
            video_a_start_secs: start,
            video_a_end_secs: end,
            video_b_start_secs: Some(start),
            video_b_end_secs: Some(end),
            b_has_energy: true,
            collar_level: None,
        };
        // Non-round values: these are the bit patterns the old float-keyed join relied on.
        let gaps = vec![
            gap(1.1, 2.2),   // g0 — skipped at plan time
            gap(10.3, 11.7), // g1 — patched
            gap(20.5, 21.9), // g2 — skipped during region patching
            gap(30.7, 31.4), // g3 — in neither plan nor results
        ];

        let fill_mode = FillMode::Fit;
        let thresholds = FillTierThresholds::DEFAULT;

        // Plan carries only g0, and the region results are pushed g2-before-g1 — both orders
        // disagree with the report.
        let plan = GapFillPlan {
            regions: vec![],
            skipped: vec![GapFillSkipped {
                gap_index: 0,
                a_start_secs: gaps[0].video_a_start_secs,
                a_end_secs: gaps[0].video_a_end_secs,
                reason: GapFillSkipReason::NotFillable,
            }],
        };

        let patched = RegionPatchOutcome::Patched {
            zero_pad_frames: 0,
            pre_correlation: 0.2,
            post_correlation: 0.9,
            align_adjustment_secs: 0.01,
            structure_trusted: true,
            confidence: FillConfidence::High,
            gap_start_adjust_frames: 0,
            gap_end_adjust_frames: 0,
            fit_used_boundary_grid: false,
            fit_boundary_grid_cells: None,
            residual: None,
            anchor_seam_used: false,
            anchor_bracket_move_frames: 0,
            dual_fit_used: false,
            dual_fit_step_real: None,
            dual_fit_post_window_ms: None,
            normalize_gain: 1.0,
            local_gain: crate::domain::LocalGainDecision::OFF,
            fit_terms: None,
            registration_residual: None,
        };
        let tags_for =
            |status: &GapPatchStatus| derive_gap_tags_from_status(status, fill_mode, thresholds);
        let placeholder = GapPatchStatus::NotPlanned {
            reason: GapFillSkipReason::NotFillable,
        };
        let region_results = vec![
            (
                2,
                skipped_patch(GapPatchSkipReason::BExtractFailed),
                tags_for(&placeholder),
            ),
            (1, patched, tags_for(&placeholder)),
        ];

        let outcomes = outcomes_in_report_order(
            &gaps,
            &plan,
            &region_results,
            ReportStamps {
                fill_mode,
                thresholds,
                program_gain: 1.0,
            },
            &[],
            &[],
        );

        assert_eq!(outcomes.len(), gaps.len(), "one outcome per reported gap");
        for (i, (outcome, g)) in outcomes.iter().zip(gaps.iter()).enumerate() {
            assert_eq!(
                (outcome.a_start_secs, outcome.a_end_secs),
                (g.video_a_start_secs, g.video_a_end_secs),
                "outcome {i} must stay aligned with the report's gap order"
            );
        }

        assert!(
            matches!(
                &outcomes[0].status,
                GapPatchStatus::NotPlanned {
                    reason: GapFillSkipReason::NotFillable
                }
            ),
            "g0 takes the plan-time skip, got {:?}",
            outcomes[0].status
        );
        assert!(
            matches!(&outcomes[1].status, GapPatchStatus::Patched { .. }),
            "g1 takes the patched result even though it was pushed second, got {:?}",
            outcomes[1].status
        );
        assert!(
            matches!(
                &outcomes[2].status,
                GapPatchStatus::Skipped {
                    reason: GapPatchSkipReason::BExtractFailed
                }
            ),
            "g2 takes the region skip even though it was pushed first, got {:?}",
            outcomes[2].status
        );
        assert!(
            matches!(
                &outcomes[3].status,
                GapPatchStatus::NotPlanned {
                    reason: GapFillSkipReason::NotFillable
                }
            ),
            "g3 appears in neither source and must default to NotPlanned, got {:?}",
            outcomes[3].status
        );
    }

    /// **A patch that never reached A must not report `Patched`.**
    ///
    /// `splice_into_a` can decline to write — the seam gate decides `Patched` upstream of it, so
    /// without this override a gap the splice skipped would be counted, tabled, and exported as a
    /// repair while A stayed byte-identical across it. The `not_applied` list wins over the region
    /// result, and `PatchSummary` counts it apart from `patched_count`.
    #[test]
    fn not_applied_overrides_the_patched_region_outcome() {
        use super::{outcomes_in_report_order, RegionPatchOutcome, ReportStamps};
        use crate::domain::gap::Gap;
        use crate::domain::gap_fill::GapFillPlan;
        use crate::domain::gap_tags::{derive_gap_tags_from_status, FillTierThresholds, PatchTier};
        use crate::domain::patch_result::{
            GapFillSkipReason, GapPatchNotAppliedReason, GapPatchStatus, PatchSummary,
        };
        use crate::domain::FillMode;

        let gaps = vec![Gap {
            video_a_start_secs: 1.5,
            video_a_end_secs: 2.5,
            video_b_start_secs: Some(1.5),
            video_b_end_secs: Some(2.5),
            b_has_energy: true,
            collar_level: None,
        }];
        let fill_mode = FillMode::Fit;
        let thresholds = FillTierThresholds::DEFAULT;
        let plan = GapFillPlan {
            regions: vec![],
            skipped: vec![],
        };
        let patched = RegionPatchOutcome::Patched {
            zero_pad_frames: 0,
            pre_correlation: 0.2,
            post_correlation: 0.95,
            align_adjustment_secs: 0.0,
            structure_trusted: true,
            confidence: FillConfidence::High,
            gap_start_adjust_frames: 0,
            gap_end_adjust_frames: 0,
            fit_used_boundary_grid: false,
            fit_boundary_grid_cells: None,
            residual: None,
            anchor_seam_used: false,
            anchor_bracket_move_frames: 0,
            dual_fit_used: false,
            dual_fit_step_real: None,
            dual_fit_post_window_ms: None,
            normalize_gain: 1.0,
            local_gain: crate::domain::LocalGainDecision::OFF,
            fit_terms: None,
            registration_residual: None,
        };
        let tags = derive_gap_tags_from_status(
            &GapPatchStatus::NotPlanned {
                reason: GapFillSkipReason::NotFillable,
            },
            fill_mode,
            thresholds,
        );
        let reason = GapPatchNotAppliedReason::FillShorterThanGap {
            fill_frames: 100,
            gap_frames: 4800,
        };

        let outcomes = outcomes_in_report_order(
            &gaps,
            &plan,
            &[(0, patched, tags)],
            ReportStamps {
                fill_mode,
                thresholds,
                program_gain: 1.0,
            },
            &[(0, reason.clone())],
            &[],
        );

        assert_eq!(
            outcomes[0].status,
            GapPatchStatus::NotApplied { reason },
            "the splice failure must win over the gate's Patched verdict"
        );
        assert_eq!(
            outcomes[0].tags.patch_tier,
            PatchTier::NotApplicable,
            "no repair happened, so no patch tier applies"
        );

        let summary = PatchSummary::from_outcomes(outcomes);
        assert_eq!(summary.patched_count, 0);
        assert_eq!(summary.not_applied_count, 1);
        assert!(
            !summary.has_patches(),
            "has_patches gates the WAV/mux write — a no-op splice must not qualify"
        );
    }
}
