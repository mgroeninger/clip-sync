use clip_sync::{MediaReader, ProgressReporter};

use crate::application::error::RepairError;
use crate::domain::{
    diagnostics::{pcm_container_duration_skew, PCM_CONTAINER_WARN_SECS},
    fill_offset::{AnchoredRetryPass, FillOffsetMode},
    format_repair_profile_verbose,
    gap_fill::{build_gap_fill_plan, format_align_fill_regions_phase},
    gap_repair_spec::{GapRepairSpec, GapRepairVerdict},
    gap_tags::{FillTierThresholds, GapTags, GapTagsPatchContext},
    inactive_repair_flag_notes,
    patch_anchor::{format_patch_anchor_table_summary, PatchAnchorTable},
    patch_result::{GapPatchNotAppliedReason, PatchSummary},
    policies,
};

mod anchor_retry;
mod decode;
mod geometry;
mod log;
mod region;
mod request;

pub use geometry::FillWindowFrames;
pub use request::{PatchAudioRequest, PatchAudioResult, PatchRequestSettings};

use anchor_retry::{
    build_patch_anchor_candidates, patch_anchor_policy, run_anchored_retry_pass, AnchoredRetryState,
};
pub(crate) use decode::{decode_ab, DecodedAb};
pub use decode::{AbSources, SourceDescriptor};
pub(crate) use geometry::border_frames_from_secs;
use geometry::repair_patch_config_view;
use log::{format_patch_characterize_verbose_line, log_gap_tags_verbose, new_patch_gap_span};
use region::{
    characterize_region, execute_region_spec, outcome_from_spec, outcomes_in_report_order,
    record_patch_gap_span, region_outcome_gap_tags, skip_outcome_from_fields, splice_into_a,
    RegionPatch, RegionPatchContext, RegionPatchMedia, RegionPatchOpts, RegionPatchOutcome,
};

/// Write = full repair (characterize → execute → optional anchored retry → splice).
/// Preview = pass-1 characterize only; report would-be decisions without PCM write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PatchRunKind {
    Write,
    Preview,
}

/// How far gap edges may be adjusted against A's decoded PCM (seconds).
const GAP_EDGE_REFINE_SECS: f64 = 0.75;

/// The `patch_audio` tracing span. A free fn so the calibration listen path raises an identically
/// shaped span over its own patch step; in [`PatchAudio::run`] the caller enters it *before* the
/// decode, so `decode_ab`'s spans stay nested where the repair profile expects them.
pub(crate) fn patch_audio_span(
    plan: &crate::domain::gap_fill::GapFillPlan,
    request: &PatchAudioRequest,
    preview: bool,
) -> tracing::Span {
    tracing::info_span!(
        "patch_audio",
        region_count = plan.regions.len(),
        fill_mode = ?request.fill_mode,
        fill_offset_mode = ?request.fill_offset_mode,
        preview,
    )
}

/// Progress/verbose notes emitted once per patch run, before any region work.
pub(crate) fn log_patch_preamble(
    progress: &dyn ProgressReporter,
    request: &PatchAudioRequest,
    preview: bool,
) {
    if preview {
        progress.phase("Repair preview: characterizing gaps (no splice / write; pass-1 only)...");
    }

    progress.phase_verbose(&format_repair_profile_verbose(
        request.profile,
        request.fit_boundary_search,
        request.fill_border_search_secs,
        request.gap_end_extend_on_post_seam_fail,
        request.gap_start_extend_on_pre_seam_fail,
    ));
    let patch_config_view = repair_patch_config_view(request);
    for note in inactive_repair_flag_notes(patch_config_view) {
        progress.phase_verbose(&format!("repair note: {note}"));
    }
}

/// The result an empty plan produces: every gap `NotPlanned`, no PCM.
///
/// Shared by [`PatchAudio::run`]'s pre-decode early return and [`PatchAudio::execute_with_decoded`]
/// so the two entries cannot disagree about what "nothing to patch" looks like. Without it the
/// decoded entry would fall through to the region loop and hand back `pcm: Some(unpatched_a)` where
/// `run` returns `None` — a caller could then export an "after" clip identical to its "before".
/// [`empty_plan_result`] for callers outside this module, which run patches, never previews.
///
/// Exists so `PatchRunKind` can stay private: the distinction is internal to the patch orchestrator,
/// and an external caller that skipped the executor still ran a write-kind pass — it just had
/// nothing to write.
///
/// `--gap-listen` is its only caller, hence the feature gate.
#[cfg(feature = "calibration")]
pub(crate) fn empty_plan_write_result(
    request: &PatchAudioRequest,
    plan: &crate::domain::gap_fill::GapFillPlan,
) -> PatchAudioResult {
    empty_plan_result(request, plan, PatchRunKind::Write)
}

fn empty_plan_result(
    request: &PatchAudioRequest,
    plan: &crate::domain::gap_fill::GapFillPlan,
    kind: PatchRunKind,
) -> PatchAudioResult {
    let summary = PatchSummary::from_outcomes(outcomes_in_report_order(
        &request.report.gaps,
        plan,
        &[],
        request.fill_mode,
        FillTierThresholds {
            min_fill_correlation: request.min_fill_correlation,
            fill_marginal_margin: request.fill_marginal_margin,
            fill_absolute_floor: request.fill_absolute_floor,
        },
        &[],
        &[],
    ));
    PatchAudioResult {
        pcm: None,
        summary,
        preview: kind == PatchRunKind::Preview,
        source_audio_bitrate_a_bps: None,
        source_audio_bitrate_b_bps: None,
        pcm_container_skew: None,
    }
}

/// Join the per-slot fill-level measurements onto gap indices, dropping any gap whose splice failed.
///
/// A failed splice left A byte-identical across the gap (`splice_into_a`'s contract), so there is
/// no written fill to report a level for — reporting one would describe a buffer that never reached
/// the timeline. Slots with no gap attribution are dropped for the same reason the splice loop only
/// warns about them: nothing can be said about a gap that cannot be named.
fn fill_level_by_gap(
    fill_level_by_slot: &[Option<crate::domain::FillLevelCheck>],
    gap_index_by_slot: &[Option<usize>],
    not_applied: &[(usize, GapPatchNotAppliedReason)],
) -> Vec<(usize, crate::domain::FillLevelCheck)> {
    fill_level_by_slot
        .iter()
        .enumerate()
        .filter_map(|(slot, check)| {
            Some((gap_index_by_slot.get(slot).copied().flatten()?, (*check)?))
        })
        .filter(|(gap_index, _)| !not_applied.iter().any(|(failed, _)| failed == gap_index))
        .collect()
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
        self.run(request, crossfade_ms, PatchRunKind::Write)
    }

    /// Characterize planned gaps (pass 1 only) and return the would-be patch summary without
    /// executing fills, anchored retry, splice, or returning PCM. Same production gate as write.
    pub fn preview(
        &self,
        request: PatchAudioRequest,
        crossfade_ms: u64,
    ) -> Result<PatchAudioResult, RepairError> {
        self.run(request, crossfade_ms, PatchRunKind::Preview)
    }

    /// Full production patch (characterize → execute → anchored retry → splice) driven from a
    /// decode the caller already owns — the `--gap-listen` entry point.
    ///
    /// Deliberately offers no preview mode: [`PatchRunKind::Preview`] returns before execute and so
    /// can never produce patched PCM, which is the whole point of the listen path.
    ///
    /// An empty plan short-circuits to the same result [`Self::run`] returns, rather than falling
    /// through to a region loop with nothing in it — see [`empty_plan_result`].
    #[cfg(feature = "calibration")]
    pub(crate) fn execute_with_decoded(
        &self,
        request: PatchAudioRequest,
        plan: crate::domain::gap_fill::GapFillPlan,
        decoded: DecodedAb,
    ) -> Result<PatchAudioResult, RepairError> {
        if plan.regions.is_empty() {
            return Ok(empty_plan_result(&request, &plan, PatchRunKind::Write));
        }
        self.run_with_decoded(request, plan, PatchRunKind::Write, decoded)
    }

    fn run(
        &self,
        request: PatchAudioRequest,
        crossfade_ms: u64,
        kind: PatchRunKind,
    ) -> Result<PatchAudioResult, RepairError> {
        // Step 1: Build fill plan (may be empty when tracks mismatch or no B energy). The equivalence
        // gate drops already-equivalent gaps here (when enabled) so they never reach decode/patch.
        let plan = build_gap_fill_plan(
            &request.report,
            crossfade_ms,
            request.skip_equivalent_gaps,
            &request.gap_selection,
        );

        if plan.regions.is_empty() {
            self.progress
                .phase("No gaps planned for patch; skipping audio decode.");
            return Ok(empty_plan_result(&request, &plan, kind));
        }

        let _patch_audio_span =
            patch_audio_span(&plan, &request, kind == PatchRunKind::Preview).entered();
        log_patch_preamble(self.progress, &request, kind == PatchRunKind::Preview);

        // Steps 2–6: decode A + B (shared with the gap-fingerprint diagnostic). This is the only
        // `self.media_reader` use in the run; everything below it is `run_with_decoded`, which the
        // calibration listen path calls with a decode it already owns.
        let decoded = decode_ab(self.media_reader, &request.report, self.progress)?;
        self.run_with_decoded(request, plan, kind, decoded)
    }

    /// Everything after the decode: characterize → (preview stop) → execute → anchored retry →
    /// splice. Split out of [`Self::run`] so a caller that already holds a [`DecodedAb`] can drive
    /// the **production** gate without decoding twice (`--gap-listen`).
    ///
    /// Takes `plan` rather than rebuilding it: two `build_gap_fill_plan` calls with drifting
    /// `crossfade_ms` / `skip_equivalent_gaps` / selection would patch a different set of regions
    /// than the caller exported windows for.
    fn run_with_decoded(
        &self,
        request: PatchAudioRequest,
        plan: crate::domain::gap_fill::GapFillPlan,
        kind: PatchRunKind,
        decoded: DecodedAb,
    ) -> Result<PatchAudioResult, RepairError> {
        let DecodedAb {
            mut a_pcm,
            b_samples_full,
            source_audio_bitrate_a_bps,
            source_audio_bitrate_b_bps,
            container_duration_a_secs,
            sources: _, // fingerprint-dump provenance only; the repair path reads the bitrates above
            // The fill path strides both sides by `a_pcm.channels` below, which is sound only
            // because production fill already refuses mismatched layouts upstream. `--gap-listen`
            // exports B on its own and so needs B's real count; this path does not.
            b_channels: _,
        } = decoded;

        // Step 7: Compute global A RMS as normalization fallback.
        let global_a_rms = policies::rms_interleaved(&a_pcm.samples);

        let channels = a_pcm.channels as usize;
        let sample_rate = a_pcm.sample_rate;
        let max_refine_frames = (GAP_EDGE_REFINE_SECS * sample_rate as f64).round() as usize;
        let region_ctx = RegionPatchContext {
            channels,
            sample_rate,
            max_refine_frames,
            global_a_rms,
            silence_peak_fraction: request.report.recipe.silence_peak_fraction(),
        };

        let region_count = plan.regions.len() as u64;

        self.progress.phase(&format_align_fill_regions_phase(&plan));

        // 6c: two passes — characterize ALL regions (decisions), then execute ALL patches (PCM) — instead
        // of the per-region `prepare_region_patch` shim. Same components (`characterize_region` +
        // `execute_region_spec`), byte-identical output; the per-gap tracing/progress reorders (char pass
        // then exec pass) but no tested surface (PCM/outcomes) changes. The anchored-retry (below) still
        // re-runs the `prepare_region_patch` shim on failed gaps.
        //
        // Preview stops after characterize: outcomes from specs, no execute / retry / splice.
        //
        // Pass 1 — characterize.
        let mut characterizations: Vec<(GapRepairSpec, GapTagsPatchContext)> =
            Vec::with_capacity(plan.regions.len());
        for (index, region) in plan.regions.iter().enumerate() {
            let region_num = index as u64 + 1;
            self.progress
                .progress("patch-characterize", region_num, region_count);
            self.progress
                .phase_verbose(&format_patch_characterize_verbose_line(
                    region.gap_index,
                    region_num,
                    region_count,
                    region.a_start_secs,
                    region.a_end_secs,
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

        let thresholds = FillTierThresholds {
            min_fill_correlation: request.min_fill_correlation,
            fill_marginal_margin: request.fill_marginal_margin,
            fill_absolute_floor: request.fill_absolute_floor,
        };

        if kind == PatchRunKind::Preview {
            let mut region_results: Vec<(usize, RegionPatchOutcome, GapTags)> =
                Vec::with_capacity(plan.regions.len());
            for ((spec, tag_ctx), region) in characterizations.into_iter().zip(plan.regions.iter())
            {
                let outcome = outcome_from_spec(&spec, region_ctx.sample_rate);
                let tags = region_outcome_gap_tags(&outcome, tag_ctx);
                log_gap_tags_verbose(self.progress, &tags);
                region_results.push((region.gap_index, outcome, tags));
            }
            let summary = PatchSummary::from_outcomes(outcomes_in_report_order(
                &request.report.gaps,
                &plan,
                &region_results,
                request.fill_mode,
                thresholds,
                // Preview never splices, so no splice can fail — and nothing is written to
                // measure a fill level on.
                &[],
                &[],
            ));
            return Ok(PatchAudioResult {
                pcm: None,
                summary,
                preview: true,
                source_audio_bitrate_a_bps,
                source_audio_bitrate_b_bps,
                pcm_container_skew: None,
            });
        }

        // Pass 2 — execute patches (skips carry their outcome; nothing to run).
        let mut patches: Vec<RegionPatch> = Vec::new();
        // Parallel to `plan.regions` (one entry pushed per region below), *not* to gap indices.
        let mut patch_slot_by_region: Vec<Option<usize>> = Vec::new();
        let mut region_results: Vec<(usize, RegionPatchOutcome, GapTags)> = Vec::new();
        for ((spec, tag_ctx), (index, region)) in characterizations
            .into_iter()
            .zip(plan.regions.iter().enumerate())
        {
            let region_num = index as u64 + 1;
            self.progress
                .progress("patch-gap", region_num, region_count);
            let gap_span =
                new_patch_gap_span(region_num, region_count, region, request.fill_mode, false);
            let _gap_enter = gap_span.enter();

            let media = RegionPatchMedia {
                b_samples_full: &b_samples_full,
                a_pcm: &a_pcm,
            };
            let (patch, outcome) = if spec.verdict.is_patch() {
                execute_region_spec(spec, &request, &media, &region_ctx)
            } else {
                // Field-level skip outcome — no verdict-matching helper (§3.2).
                let residual = spec.tags_ctx.gate.residual;
                match &spec.verdict {
                    GapRepairVerdict::Skip { reason, .. } => {
                        (None, skip_outcome_from_fields(reason, residual))
                    }
                    // `is_patch()` is the dispatch predicate; this arm is exhaustiveness-only.
                    GapRepairVerdict::Patch(_) => {
                        execute_region_spec(spec, &request, &media, &region_ctx)
                    }
                }
            };
            let tags = region_outcome_gap_tags(&outcome, tag_ctx);
            log_gap_tags_verbose(self.progress, &tags);
            record_patch_gap_span(&gap_span, &outcome);
            region_results.push((region.gap_index, outcome, tags));
            if let Some(patch) = patch {
                let slot = patches.len();
                patches.push(patch);
                patch_slot_by_region.push(Some(slot));
            } else {
                patch_slot_by_region.push(None);
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
                        patch_slot_by_region: &mut patch_slot_by_region,
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
            self.progress
                .phase(&format!("Splicing {patch_count} fill(s) into timeline..."));
        }
        // Slot → gap index, built *after* the anchored-retry pass (which rewrites both `patches`
        // and `patch_slot_by_region`) so a splice failure can be attributed to the right gap.
        let mut gap_index_by_slot: Vec<Option<usize>> = vec![None; patches.len()];
        for (position, slot) in patch_slot_by_region.iter().enumerate() {
            if let (Some(slot), Some(region)) = (slot, plan.regions.get(position)) {
                if let Some(entry) = gap_index_by_slot.get_mut(*slot) {
                    *entry = Some(region.gap_index);
                }
            }
        }

        // Record-only fill-level check (`measure_fill_level`), measured HERE for two reasons: these are the
        // final patches (the anchored-retry pass has already rewritten them), and A is still
        // pristine — the splice loop below mutates `a_pcm.samples`, and a shoulder read after a
        // neighbouring splice would describe the repair rather than the program it is judged
        // against. The gain is applied because that is what gets written.
        let fill_level_by_slot: Vec<Option<crate::domain::FillLevelCheck>> =
            if request.measure_fill_level {
                patches
                    .iter()
                    .map(|patch| {
                        crate::domain::measure_fill_level(
                            &region::gained_fill(patch, channels),
                            &a_pcm.samples,
                            channels,
                            patch.a_start_frame,
                            patch.a_end_frame,
                            sample_rate,
                            crate::domain::FillLevelParams::default(),
                        )
                    })
                    .collect()
            } else {
                Vec::new()
            };

        let mut not_applied: Vec<(usize, GapPatchNotAppliedReason)> = Vec::new();
        if patch_count > 0 {
            let _splice_span = tracing::info_span!("patch_splice", patch_count).entered();
            for (index, patch) in patches.iter().enumerate() {
                self.progress
                    .progress("patch-splice", index as u64 + 1, patch_count);
                let b_gained = region::gained_fill(patch, channels);

                if let Err(reason) = splice_into_a(
                    &mut a_pcm.samples,
                    &b_gained,
                    channels,
                    patch.a_start_frame,
                    patch.a_end_frame,
                    patch.crossfade_secs,
                    sample_rate,
                ) {
                    // A is unchanged across this gap. Downgrade the gate's `Patched` verdict so no
                    // consumer — gap table, JSON, `--gap-listen` WAVs — claims a repair happened.
                    match gap_index_by_slot.get(index).copied().flatten() {
                        Some(gap_index) => not_applied.push((gap_index, reason)),
                        None => tracing::warn!(
                            slot = index,
                            ?reason,
                            "splice failed for a patch with no gap attribution; outcome will still \
                             read patched"
                        ),
                    }
                }
            }
        }

        let fill_level = fill_level_by_gap(&fill_level_by_slot, &gap_index_by_slot, &not_applied);

        let mut summary = PatchSummary::from_outcomes(outcomes_in_report_order(
            &request.report.gaps,
            &plan,
            &region_results,
            request.fill_mode,
            thresholds,
            &not_applied,
            &fill_level,
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
            preview: false,
            source_audio_bitrate_a_bps,
            source_audio_bitrate_b_bps,
            pcm_container_skew,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(peak_delta_db: f64) -> crate::domain::FillLevelCheck {
        crate::domain::FillLevelCheck {
            pre_shoulder_db: Some(-40.0),
            post_shoulder_db: Some(-40.0),
            reference_db: -40.0,
            peak_bin_db: -40.0 + peak_delta_db,
            peak_delta_db,
            reference_at_floor: false,
            peak_bin_index: 0,
            // These tests are about which gaps carry a level at all, not what the level says, so
            // the fill is flat: every bin at the peak, and a steady neighbourhood.
            head_bin_db: -40.0 + peak_delta_db,
            tail_bin_db: -40.0 + peak_delta_db,
            edge_delta_db: peak_delta_db,
            reference_spread_db: Some(0.0),
            bins: 5,
            bin_ms: 100.0,
        }
    }

    /// The measurement is taken before the splice loop, so a gap whose splice then failed has a
    /// level in hand for audio that never reached A. It must not be reported: `NotApplied` means
    /// A is byte-identical across that gap, and a `fill_level` beside it would be a measurement of
    /// nothing.
    #[test]
    fn a_failed_splice_reports_no_fill_level() {
        let levels = vec![Some(check(3.0)), Some(check(21.0))];
        let slots = vec![Some(7), Some(9)];
        let not_applied = vec![(
            9,
            GapPatchNotAppliedReason::DestinationEmptyOrInverted {
                gap_start_frame: 10,
                gap_end_frame: 10,
            },
        )];

        let joined = fill_level_by_gap(&levels, &slots, &not_applied);

        assert_eq!(joined.len(), 1, "{joined:?}");
        assert_eq!(
            joined[0].0, 7,
            "the spliced gap keeps its level: {joined:?}"
        );
    }

    #[test]
    fn every_spliced_gap_keeps_its_own_level() {
        let levels = vec![Some(check(3.0)), None, Some(check(21.0))];
        let slots = vec![Some(7), Some(8), Some(9)];

        let joined = fill_level_by_gap(&levels, &slots, &[]);

        assert_eq!(joined.len(), 2, "the unmeasured slot drops out: {joined:?}");
        assert_eq!(joined[0].0, 7);
        assert_eq!(joined[1].0, 9);
        assert_eq!(joined[1].1.peak_delta_db, 21.0, "levels are not transposed");
    }

    /// Slot → gap is built after the anchored-retry pass; a slot it cannot attribute is dropped
    /// rather than guessed at, matching how the splice loop treats the same case.
    #[test]
    fn an_unattributed_slot_is_dropped() {
        let joined = fill_level_by_gap(&[Some(check(3.0))], &[None], &[]);
        assert!(joined.is_empty(), "{joined:?}");
    }

    /// `measure_fill_level` off leaves the per-slot vector empty; the join must survive the length
    /// mismatch against a populated slot table rather than indexing past it.
    #[test]
    fn no_measurements_joins_to_nothing() {
        assert!(fill_level_by_gap(&[], &[Some(7), Some(9)], &[]).is_empty());
    }
}
