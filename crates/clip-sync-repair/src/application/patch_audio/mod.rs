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
    patch_result::PatchSummary,
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
                preview: kind == PatchRunKind::Preview,
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
            preview = kind == PatchRunKind::Preview,
        )
        .entered();

        if kind == PatchRunKind::Preview {
            self.progress
                .phase("Repair preview: characterizing gaps (no splice / write; pass-1 only)...");
        }

        self.progress.phase_verbose(&format_repair_profile_verbose(
            request.profile,
            request.fit_boundary_search,
            request.fill_border_search_secs,
            request.gap_end_extend_on_post_seam_fail,
            request.gap_start_extend_on_pre_seam_fail,
        ));
        let patch_config_view = repair_patch_config_view(&request);
        for note in inactive_repair_flag_notes(patch_config_view) {
            self.progress.phase_verbose(&format!("repair note: {note}"));
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
        let max_refine_frames = (GAP_EDGE_REFINE_SECS * sample_rate as f64).round() as usize;
        let region_ctx = RegionPatchContext {
            channels,
            sample_rate,
            max_refine_frames,
            global_a_rms,
            silence_peak_fraction: request.report.silence_peak_fraction,
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
        let mut patch_slot_by_gap: Vec<Option<usize>> = Vec::new();
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
            self.progress
                .phase(&format!("Splicing {patch_count} fill(s) into timeline..."));
        }
        if patch_count > 0 {
            let _splice_span = tracing::info_span!("patch_splice", patch_count).entered();
            for (index, patch) in patches.iter().enumerate() {
                self.progress
                    .progress("patch-splice", index as u64 + 1, patch_count);
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
            preview: false,
            source_audio_bitrate_a_bps,
            source_audio_bitrate_b_bps,
            pcm_container_skew,
        })
    }
}
