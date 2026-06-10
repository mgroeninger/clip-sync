use std::collections::HashMap;
use std::time::Duration;

use clip_sync::{
    select_best_track, select_track_for_reference, ClipLabel, ClipWindow, DomainError,
    MediaReader, MediaSession, MediaSource, MultiChannelPcm, ProgressReporter, resample_interleaved,
};

use crate::application::error::RepairError;
use crate::domain::{
    gap_fill::{build_gap_fill_plan, FillRegion, GapFillPlan},
    gap_structure::{self, StructureMatchParams},
    patch_result::{
        GapFillSkipReason, GapPatchOutcome, GapPatchSkipReason, GapPatchStatus, PatchSummary,
    },
    policies::{self, FillAlignment},
    Gap, GapReport,
};

pub struct PatchAudioResult {
    /// Present when A was decoded for patching; `None` when the fill plan was empty.
    pub pcm: Option<MultiChannelPcm>,
    pub summary: PatchSummary,
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
    /// In the waveform gate path, soften Pearson threshold when structure scores meet this.
    pub partial_structure_waveform_soften: f64,
    /// Peak-amplitude floor for per-frame silence checks during gap refinement (matches scan).
    pub absolute_silence_rms: f32,
}

/// How far gap edges may be adjusted against A's decoded PCM (seconds).
const GAP_EDGE_REFINE_SECS: f64 = 0.75;
/// Pearson floor used when partial structure trust softens the waveform gate.
const PARTIAL_WAVEFORM_MIN_CORRELATION: f32 = 0.12;

// Collected B segment ready to splice into A.
struct RegionPatch {
    b_samples: Vec<i16>,
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
            ));
            return Ok(PatchAudioResult { pcm: None, summary });
        }

        // Step 2: Open A, select best track, get duration.
        let source_a = MediaSource::new(request.report.video_a.clone());
        let session_a = self
            .media_reader
            .open(&source_a)
            .map_err(RepairError::Media)?;
        let tracks_a = session_a.list_tracks().map_err(RepairError::Media)?;
        let track_a = select_best_track(&tracks_a)?.clone();

        let duration_a = track_a
            .duration
            .ok_or(RepairError::Domain(DomainError::InvalidDuration))?;

        // Step 3: Extract full A timeline.
        let full_window_a = ClipWindow::new(Duration::ZERO, duration_a, ClipLabel::Interior);
        let mut a_pcm = session_a
            .extract_interleaved(&track_a, &full_window_a, self.progress, "patch-a")
            .map_err(RepairError::Media)?;

        // Step 5: Open B, select best track.
        let source_b = MediaSource::new(request.report.video_b.clone());
        let session_b = self
            .media_reader
            .open(&source_b)
            .map_err(RepairError::Media)?;
        let tracks_b = session_b.list_tracks().map_err(RepairError::Media)?;
        let track_b = select_track_for_reference(&track_a, &tracks_b)?.clone();

        // Step 6: Decode full B timeline once (sequential from t=0) to avoid per-gap MKV seeks.
        let duration_b = track_b
            .duration
            .ok_or(RepairError::Domain(DomainError::InvalidDuration))?;
        let full_window_b = ClipWindow::new(Duration::ZERO, duration_b, ClipLabel::Interior);
        let b_pcm_full = session_b
            .extract_interleaved(&track_b, &full_window_b, self.progress, "patch-b")
            .map_err(RepairError::Media)?;
        let b_samples_full = if b_pcm_full.sample_rate != a_pcm.sample_rate {
            resample_interleaved(
                &b_pcm_full.samples,
                b_pcm_full.channels,
                b_pcm_full.sample_rate,
                a_pcm.sample_rate,
            )
        } else {
            b_pcm_full.samples
        };

        // Step 7: Compute global A RMS as normalization fallback.
        let global_a_rms = policies::rms_interleaved(&a_pcm.samples);

        let channels = a_pcm.channels as usize;
        let sample_rate = a_pcm.sample_rate;
        let max_adjustment_frames =
            (request.max_fill_align_adjustment_secs * sample_rate as f64).round() as usize;
        let max_refine_frames =
            (GAP_EDGE_REFINE_SECS * sample_rate as f64).round() as usize;
        let silence_peak_fraction = request.report.silence_peak_fraction;

        // Step 8: Collect B segments (immutable borrow on a_pcm.samples),
        // then apply them in a separate pass (mutable borrow).
        let mut patches: Vec<RegionPatch> = Vec::new();
        let mut region_results: Vec<(f64, f64, RegionPatchOutcome)> = Vec::new();
        let region_count = plan.regions.len() as u64;

        self.progress.phase(&format!(
            "Aligning {region_count} fill region(s) (structure match + splice)..."
        ));

        for (index, region) in plan.regions.iter().enumerate() {
            let gap_num = index as u64 + 1;
            self.progress.progress("patch-gap", gap_num, region_count);
            self.progress.phase(&format!(
                "  gap {gap_num}/{region_count}: A [{:.1}s – {:.1}s]",
                region.a_start_secs, region.a_end_secs
            ));

            let (patch, outcome) = prepare_region_patch(
                &b_samples_full,
                &a_pcm,
                region,
                channels,
                sample_rate,
                max_adjustment_frames,
                max_refine_frames,
                request.normalize_window_secs,
                request.fill_align_margin_secs,
                request.fill_border_search_secs,
                request.min_border_discovery_secs,
                request.border_standoff_secs,
                request.short_gap_mean_correlation_secs,
                request.fill_length_slack_secs,
                request.fill_seam_search_secs,
                request.gap_signature_context_secs,
                request.gap_signature_bin_ms,
                request.min_structure_match_score,
                request.strong_structure_trust,
                request.partial_structure_waveform_soften,
                request.min_fill_correlation,
                request.normalize_fill,
                request.normalize_window_secs,
                request.max_fill_gain_db,
                global_a_rms,
                silence_peak_fraction,
                request.absolute_silence_rms,
            );
            region_results.push((region.a_start_secs, region.a_end_secs, outcome));
            if let Some(patch) = patch {
                patches.push(patch);
            }
        }

        self.progress.progress("patch-gap", region_count, region_count);

        // Step 9: Apply patches to A samples.
        let patch_count = patches.len() as u64;
        if patch_count > 0 {
            self.progress.phase(&format!("Splicing {patch_count} fill(s) into timeline..."));
        }
        for (index, patch) in patches.iter().enumerate() {
            self.progress.progress("patch-splice", index as u64 + 1, patch_count);
            let b_gained: Vec<i16> = patch
                .b_samples
                .iter()
                .map(|&s| {
                    (s as f32 * patch.gain)
                        .round()
                        .clamp(i16::MIN as f32, i16::MAX as f32) as i16
                })
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
        if patch_count > 0 {
            self.progress.progress("patch-splice", patch_count, patch_count);
        }

        let summary = PatchSummary::from_outcomes(outcomes_in_report_order(
            &request.report.gaps,
            &plan,
            &region_results,
        ));

        Ok(PatchAudioResult {
            pcm: Some(a_pcm),
            summary,
        })
    }
}

enum RegionPatchOutcome {
    Patched {
        pre_correlation: f64,
        post_correlation: f64,
        align_adjustment_secs: f64,
        structure_trusted: bool,
    },
    Skipped(GapPatchSkipReason),
}

fn gap_key(start_secs: f64, end_secs: f64) -> (u64, u64) {
    (start_secs.to_bits(), end_secs.to_bits())
}

fn outcomes_in_report_order(
    gaps: &[Gap],
    plan: &GapFillPlan,
    region_results: &[(f64, f64, RegionPatchOutcome)],
) -> Vec<GapPatchOutcome> {
    let mut status_by_gap: HashMap<(u64, u64), GapPatchStatus> = HashMap::new();

    for skip in &plan.skipped {
        status_by_gap.insert(
            gap_key(skip.a_start_secs, skip.a_end_secs),
            GapPatchStatus::NotPlanned {
                reason: skip.reason.clone(),
            },
        );
    }

    for (a_start, a_end, outcome) in region_results {
        let status = match outcome {
            RegionPatchOutcome::Patched {
                pre_correlation,
                post_correlation,
                align_adjustment_secs,
                structure_trusted,
            } => GapPatchStatus::Patched {
                pre_correlation: *pre_correlation,
                post_correlation: *post_correlation,
                align_adjustment_secs: *align_adjustment_secs,
                structure_trusted: *structure_trusted,
            },
            RegionPatchOutcome::Skipped(reason) => GapPatchStatus::Skipped {
                reason: reason.clone(),
            },
        };
        status_by_gap.insert(gap_key(*a_start, *a_end), status);
    }

    gaps
        .iter()
        .map(|gap| {
            let status = status_by_gap
                .remove(&gap_key(gap.video_a_start_secs, gap.video_a_end_secs))
                .unwrap_or(GapPatchStatus::NotPlanned {
                    reason: GapFillSkipReason::NotFillable,
                });
            GapPatchOutcome {
                a_start_secs: gap.video_a_start_secs,
                a_end_secs: gap.video_a_end_secs,
                status,
            }
        })
        .collect()
}

fn prepare_region_patch(
    b_samples_full: &[i16],
    a_pcm: &MultiChannelPcm,
    region: &FillRegion,
    channels: usize,
    sample_rate: u32,
    max_adjustment_frames: usize,
    max_refine_frames: usize,
    normalize_window_secs_for_correlate: f64,
    margin_secs: f64,
    border_search_secs: f64,
    min_border_discovery_secs: f64,
    border_standoff_secs: f64,
    short_gap_mean_correlation_secs: f64,
    fill_length_slack_secs: f64,
    fill_seam_search_secs: f64,
    gap_signature_context_secs: f64,
    gap_signature_bin_ms: u64,
    min_structure_match_score: f32,
    strong_structure_trust: f64,
    partial_structure_waveform_soften: f64,
    min_fill_correlation: f32,
    normalize_fill: bool,
    normalize_window_secs: f64,
    max_fill_gain_db: f64,
    global_a_rms: f32,
    silence_peak_fraction: f32,
    absolute_silence_rms: f32,
) -> (Option<RegionPatch>, RegionPatchOutcome) {
    debug_assert!(
        region.b_start_secs >= 0.0,
        "fill plan must not include gaps with negative B start"
    );

    let reported_start_frame = (region.a_start_secs * sample_rate as f64) as usize;
    let reported_end_frame = (region.a_end_secs * sample_rate as f64) as usize;
    let refined = policies::refine_gap_frames(
        &a_pcm.samples,
        channels,
        reported_start_frame,
        reported_end_frame,
        silence_peak_fraction,
        absolute_silence_rms,
        max_refine_frames,
    );

    let start_delta_frames =
        refined.start_frame as i64 - reported_start_frame as i64;
    let end_delta_frames = refined.end_frame as i64 - reported_end_frame as i64;
    let refined_b_start_secs =
        region.b_start_secs + start_delta_frames as f64 / sample_rate as f64;
    let refined_b_end_secs =
        region.b_end_secs + end_delta_frames as f64 / sample_rate as f64;
    let a_start_secs = refined.start_frame as f64 / sample_rate as f64;

    let gap_frames = refined.end_frame.saturating_sub(refined.start_frame);
    if gap_frames == 0 {
        return (
            None,
            RegionPatchOutcome::Skipped(GapPatchSkipReason::ZeroLengthGap),
        );
    }

    let context_frames =
        (gap_signature_context_secs * sample_rate as f64).round() as usize;
    let bin_frames =
        ((gap_signature_bin_ms as f64 / 1000.0) * sample_rate as f64).round() as usize;
    let correlate_frames = correlate_frames_for_gap(
        normalize_window_secs_for_correlate,
        min_border_discovery_secs,
        gap_frames,
        sample_rate,
    );
    let seam_gate_frames =
        seam_gate_frames_for(correlate_frames, fill_seam_search_secs, sample_rate);
    let search_radius_secs = border_search_secs.max(margin_secs);
    let b_extract_start_secs = (refined_b_start_secs
        - gap_signature_context_secs
        - search_radius_secs
        - margin_secs)
        .max(0.0);
    let length_slack_secs = fill_length_slack_secs.max(margin_secs);
    let b_extract_end_secs = refined_b_end_secs
        + gap_signature_context_secs
        + search_radius_secs
        + length_slack_secs
        + margin_secs;
    let b_samples = match slice_b_segment(
        b_samples_full,
        channels,
        sample_rate,
        b_extract_start_secs,
        b_extract_end_secs,
    ) {
        Some(samples) => samples,
        None => {
            tracing::warn!(
                a_start_secs,
                b_extract_start_secs,
                b_extract_end_secs,
                "skipping gap fill region: B slice out of range"
            );
            return (
                None,
                RegionPatchOutcome::Skipped(GapPatchSkipReason::BExtractFailed),
            );
        }
    };

    let border_frames = border_frames_from_secs(normalize_window_secs, sample_rate)
        .min(correlate_frames);
    let border_standoff_frames =
        (border_standoff_secs * sample_rate as f64).round() as usize;
    let (a_pre_border, a_post_border) = policies::border_templates_for_gap(
        &a_pcm.samples,
        channels,
        refined.start_frame,
        refined.end_frame,
        border_frames,
        border_standoff_frames,
        silence_peak_fraction,
        absolute_silence_rms,
    );
    let (a_pre_ch, a_post_ch) = policies::border_templates_per_channel_for_gap(
        &a_pcm.samples,
        channels,
        refined.start_frame,
        refined.end_frame,
        border_frames,
        border_standoff_frames,
        silence_peak_fraction,
        absolute_silence_rms,
    );
    let b_mono = policies::interleaved_to_mono(b_samples, channels);
    let b_ch = policies::interleaved_to_channels(b_samples, channels);

    let signature = gap_structure::build_gap_context_signature(
        &a_pcm.samples,
        channels,
        refined.start_frame,
        refined.end_frame,
        context_frames,
        bin_frames.max(1),
        silence_peak_fraction,
        absolute_silence_rms,
    );

    let offset_nominal_start =
        ((refined_b_start_secs - b_extract_start_secs) * sample_rate as f64).round() as usize;
    let gap_end_in_haystack =
        ((refined_b_end_secs - b_extract_start_secs) * sample_rate as f64).round() as usize;
    let search_radius_frames =
        (border_search_secs * sample_rate as f64).round() as usize;
    let fill_length_slack_frames =
        (fill_length_slack_secs * sample_rate as f64).round() as usize;

    let structure_params = StructureMatchParams {
        gap_frames,
        bin_frames: bin_frames.max(1),
        search_radius_frames,
        fill_length_slack_frames,
        max_fine_adjustment_frames: max_adjustment_frames,
        silence_peak_fraction,
        absolute_silence_rms,
    };

    let mut alignment = match gap_structure::match_gap_structure_in_b(
        &signature,
        b_samples,
        channels,
        offset_nominal_start,
        gap_end_in_haystack,
        &structure_params,
    ) {
        Some(alignment) => alignment,
        None => {
            tracing::warn!(
                a_start_secs,
                "skipping gap fill: structure alignment failed"
            );
            return (
                None,
                RegionPatchOutcome::Skipped(GapPatchSkipReason::BoundaryAlignmentFailed),
            );
        }
    };

    let structure_pre = alignment.pre_correlation;
    let structure_post = alignment.post_correlation;
    let gap_secs = gap_frames as f64 / sample_rate as f64;
    if !structure_passes_gate(
        structure_pre,
        structure_post,
        min_structure_match_score,
        gap_secs,
        short_gap_mean_correlation_secs,
    ) {
        tracing::warn!(
            structure_pre,
            structure_post,
            min_structure_match_score,
            a_start_secs,
            "skipping gap fill: structure match below threshold"
        );
        return (
            None,
            RegionPatchOutcome::Skipped(GapPatchSkipReason::CorrelationBelowThreshold {
                pre_correlation: structure_pre,
                post_correlation: structure_post,
                min_correlation: min_structure_match_score,
            }),
        );
    }

    let structure_trusted = structure_pre >= strong_structure_trust
        && structure_post >= strong_structure_trust;

    let (report_pre, report_post, patched_structure_trusted) = if structure_trusted {
        tracing::debug!(
            structure_pre,
            structure_post,
            a_start_secs,
            "trusting structure match (skipping waveform seam gate)"
        );
        (structure_pre, structure_post, true)
    } else {
        let waveform_gate_frames = seam_gate_frames.min(a_pre_border.len().max(1));
        let post_gate_frames = seam_gate_frames.min(a_post_border.len()).max(1);
        let (pre_corr, post_corr) = policies::fill_seam_correlations(
            &a_pre_border,
            &a_post_border,
            &a_pre_ch,
            &a_post_ch,
            &b_mono,
            &b_ch,
            alignment.start_frame,
            gap_frames,
            waveform_gate_frames,
            post_gate_frames,
        );

        let soften_waveform_gate = structure_pre >= partial_structure_waveform_soften
            && structure_post >= partial_structure_waveform_soften;
        let effective_min_corr = if soften_waveform_gate {
            min_fill_correlation.min(PARTIAL_WAVEFORM_MIN_CORRELATION)
        } else {
            min_fill_correlation
        };

        alignment.pre_correlation = pre_corr;
        alignment.post_correlation = post_corr;

        if !seams_pass_correlation_gate(
            &alignment,
            effective_min_corr,
            gap_secs,
            short_gap_mean_correlation_secs,
        ) {
            tracing::warn!(
                pre_correlation = pre_corr,
                post_correlation = post_corr,
                effective_min_corr,
                min_fill_correlation,
                structure_pre,
                structure_post,
                waveform_gate_frames,
                align_adjustment_secs = (alignment.start_frame as f64
                    - offset_nominal_start as f64)
                    / sample_rate as f64,
                b_fill_frames = alignment.fill_frames,
                a_gap_frames = gap_frames,
                a_start_secs,
                "skipping gap fill: waveform seam correlation below threshold"
            );
            return (
                None,
                RegionPatchOutcome::Skipped(GapPatchSkipReason::CorrelationBelowThreshold {
                    pre_correlation: pre_corr,
                    post_correlation: post_corr,
                    min_correlation: effective_min_corr,
                }),
            );
        }
        (pre_corr, post_corr, false)
    };

    let fill_start_sample = alignment.start_frame * channels;
    let b_fill_end_sample = fill_start_sample + alignment.fill_frames * channels;
    if b_fill_end_sample > b_samples.len() {
        tracing::warn!(
            a_start_secs,
            b_fill_frames = alignment.fill_frames,
            "skipping gap fill: aligned B segment out of range"
        );
        return (
            None,
            RegionPatchOutcome::Skipped(GapPatchSkipReason::AlignedSegmentOutOfRange),
        );
    }

    let mut b_fill_raw = b_samples[fill_start_sample..b_fill_end_sample].to_vec();
    let source_frames = b_fill_raw.len() / channels;
    if source_frames < gap_frames {
        let extend_from = b_fill_end_sample;
        let need_samples = (gap_frames - source_frames) * channels;
        let extend_to = (extend_from + need_samples).min(b_samples.len());
        if extend_from < extend_to {
            b_fill_raw.extend_from_slice(&b_samples[extend_from..extend_to]);
            tracing::debug!(
                a_start_secs,
                extended_frames = (extend_to - extend_from) / channels,
                "B bracket shorter than A gap; extended from contiguous B audio"
            );
        }
    } else if source_frames > gap_frames {
        tracing::debug!(
            a_start_secs,
            b_fill_frames = source_frames,
            a_gap_frames = gap_frames,
            "B fill longer than A gap; trimming tail (pre-border anchor)"
        );
    }
    let b_fill = fit_fill_to_gap_frames(&b_fill_raw, channels, gap_frames);

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

    let align_adjustment_secs =
        (alignment.start_frame as f64 - offset_nominal_start as f64) / sample_rate as f64;

    (
        Some(RegionPatch {
            b_samples: b_fill,
            gain,
            a_start_frame: refined.start_frame,
            a_end_frame: refined.end_frame,
            crossfade_secs: region.crossfade_secs,
        }),
        RegionPatchOutcome::Patched {
            pre_correlation: report_pre,
            post_correlation: report_post,
            align_adjustment_secs,
            structure_trusted: patched_structure_trusted,
        },
    )
}

/// Fit a B fill bracket to A's gap length without resampling (preserves pitch).
///
/// Longer fills are trimmed from the tail (start is pre-border anchored).
/// Shorter fills are zero-padded at the tail only when B has no more contiguous audio.
fn fit_fill_to_gap_frames(samples: &[i16], channels: usize, target_frames: usize) -> Vec<i16> {
    let channels = channels.max(1);
    let source_frames = samples.len() / channels;
    if source_frames == target_frames {
        return samples.to_vec();
    }
    if source_frames == 0 {
        return vec![0i16; target_frames * channels];
    }

    if source_frames > target_frames {
        return samples[..target_frames * channels].to_vec();
    }

    let mut out = vec![0i16; target_frames * channels];
    out[..samples.len()].copy_from_slice(samples);
    out
}

fn border_frames_from_secs(window_secs: f64, sample_rate: u32) -> usize {
    (window_secs * sample_rate as f64) as usize
}

/// Cap for fine-align slide search and seam correlation gate (frames).
fn seam_gate_frames_for(
    correlate_frames: usize,
    fill_seam_search_secs: f64,
    sample_rate: u32,
) -> usize {
    let cap = (fill_seam_search_secs * sample_rate as f64).round() as usize;
    correlate_frames.min(cap).max(1)
}

/// Seam correlation window sized to the gap (short gaps use shorter templates).
fn correlate_frames_for_gap(
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

fn slice_b_segment<'a>(
    b_samples: &'a [i16],
    channels: usize,
    sample_rate: u32,
    start_secs: f64,
    end_secs: f64,
) -> Option<&'a [i16]> {
    let channels = channels.max(1);
    let start_frame = (start_secs * sample_rate as f64).round() as usize;
    let end_frame = ((end_secs * sample_rate as f64).round() as usize)
        .min(b_samples.len() / channels);
    if start_frame >= end_frame {
        return None;
    }
    Some(&b_samples[start_frame * channels..end_frame * channels])
}

fn structure_passes_gate(
    pre_score: f64,
    post_score: f64,
    min_structure_match_score: f32,
    gap_secs: f64,
    short_gap_mean_correlation_secs: f64,
) -> bool {
    let pre = pre_score as f32;
    let post = post_score as f32;
    if gap_secs <= short_gap_mean_correlation_secs {
        (pre + post) / 2.0 >= min_structure_match_score
    } else {
        pre >= min_structure_match_score && post >= min_structure_match_score
    }
}

fn seams_pass_correlation_gate(
    alignment: &FillAlignment,
    min_fill_correlation: f32,
    gap_secs: f64,
    short_gap_mean_correlation_secs: f64,
) -> bool {
    let pre = alignment.pre_correlation as f32;
    let post = alignment.post_correlation as f32;
    if gap_secs <= short_gap_mean_correlation_secs {
        (pre + post) / 2.0 >= min_fill_correlation
    } else {
        pre >= min_fill_correlation && post >= min_fill_correlation
    }
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
            let v = f64::from(s);
            v * v
        })
        .sum();

    let rms = (sum_sq / total as f64).sqrt() as f32;
    if rms == 0.0 { fallback } else { rms }
}

/// Splice B samples into A's interleaved sample buffer at the gap location.
fn splice_into_a(
    a_samples: &mut [i16],
    b_samples: &[i16],
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
    use super::fit_fill_to_gap_frames;

    #[test]
    fn fit_fill_trims_tail_without_resampling() {
        let channels = 2usize;
        let mut samples = Vec::new();
        for frame in 0..10i16 {
            samples.push(frame * 100);
            samples.push(frame * 100);
        }
        let fitted = fit_fill_to_gap_frames(&samples, channels, 6);
        assert_eq!(fitted.len(), 12);
        assert_eq!(fitted[0], 0);
        assert_eq!(fitted[1], 0);
        assert_eq!(fitted[10], 500);
        assert_eq!(fitted[11], 500);
    }

    #[test]
    fn fit_fill_zero_pads_short_source() {
        let samples = vec![1000i16, 1000, 2000, 2000];
        let fitted = fit_fill_to_gap_frames(&samples, 2, 4);
        assert_eq!(fitted, vec![1000, 1000, 2000, 2000, 0, 0, 0, 0]);
    }
}
