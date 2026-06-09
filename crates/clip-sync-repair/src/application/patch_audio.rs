use std::collections::HashMap;
use std::time::Duration;

use clip_sync::{
    select_best_track, ClipLabel, ClipWindow, DomainError, MediaReader, MediaSession, MediaSource,
    MultiChannelPcm, ProgressReporter, resample_interleaved,
};

use crate::application::error::RepairError;
use crate::domain::{
    gap_fill::{build_gap_fill_plan, FillRegion, GapFillPlan},
    patch_result::{
        GapFillSkipReason, GapPatchOutcome, GapPatchSkipReason, GapPatchStatus, PatchSummary,
    },
    policies::{self, FillAlignment},
    Gap, GapReport,
};

pub struct PatchAudioResult {
    pub pcm: MultiChannelPcm,
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
    /// Peak-amplitude floor for per-frame silence checks during gap refinement (matches scan).
    pub absolute_silence_rms: f32,
}

/// How far gap edges may be adjusted against A's decoded PCM (seconds).
const GAP_EDGE_REFINE_SECS: f64 = 0.75;

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
        // Step 1: Open A, select best track, get duration.
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

        // Step 2: Extract full A timeline.
        let full_window_a = ClipWindow::new(Duration::ZERO, duration_a, ClipLabel::Interior);
        let mut a_pcm = session_a
            .extract_interleaved(&track_a, &full_window_a, self.progress, "patch-a")
            .map_err(RepairError::Media)?;

        // Step 3: Build fill plan.
        let plan = build_gap_fill_plan(&request.report, crossfade_ms);

        // Step 4: If no regions, return A as-is with per-gap outcomes.
        if plan.regions.is_empty() {
            let summary = PatchSummary::from_outcomes(outcomes_in_report_order(
                &request.report.gaps,
                &plan,
                &[],
            ));
            return Ok(PatchAudioResult { pcm: a_pcm, summary });
        }

        // Step 5: Open B, select best track.
        let source_b = MediaSource::new(request.report.video_b.clone());
        let session_b = self
            .media_reader
            .open(&source_b)
            .map_err(RepairError::Media)?;
        let tracks_b = session_b.list_tracks().map_err(RepairError::Media)?;
        let track_b = select_best_track(&tracks_b)?.clone();

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

        for region in &plan.regions {
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

        // Step 9: Apply patches to A samples.
        for patch in patches {
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

        let summary = PatchSummary::from_outcomes(outcomes_in_report_order(
            &request.report.gaps,
            &plan,
            &region_results,
        ));

        Ok(PatchAudioResult { pcm: a_pcm, summary })
    }
}

enum RegionPatchOutcome {
    Patched {
        pre_correlation: f64,
        post_correlation: f64,
        align_adjustment_secs: f64,
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
            } => GapPatchStatus::Patched {
                pre_correlation: *pre_correlation,
                post_correlation: *post_correlation,
                align_adjustment_secs: *align_adjustment_secs,
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

    let correlate_frames = correlate_frames_for_gap(
        normalize_window_secs_for_correlate,
        min_border_discovery_secs,
        gap_frames,
        sample_rate,
    );
    let align_margin_secs = margin_secs.max(
        (max_adjustment_frames + correlate_frames) as f64 / sample_rate as f64 + 0.25,
    );
    let search_radius_secs = border_search_secs.max(align_margin_secs);
    let b_extract_start_secs = (refined_b_start_secs - search_radius_secs - align_margin_secs).max(0.0);
    // Extra slack so B-derived fills may run longer than A's scanned gap.
    let b_extract_end_secs =
        refined_b_end_secs + search_radius_secs * 2.0 + align_margin_secs;
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

    let border_frames = border_frames_from_secs(normalize_window_secs, sample_rate);
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

    let offset_nominal_start =
        ((refined_b_start_secs - b_extract_start_secs) * sample_rate as f64).round() as usize;
    let gap_end_in_haystack =
        ((refined_b_end_secs - b_extract_start_secs) * sample_rate as f64).round() as usize;
    let search_radius_frames =
        (border_search_secs * sample_rate as f64).round() as usize;
    let nominal_start_frame = policies::discover_fill_start_in_b(
        &a_pre_border,
        &a_post_border,
        &b_mono,
        &a_pre_ch,
        &a_post_ch,
        &b_ch,
        offset_nominal_start,
        offset_nominal_start,
        gap_end_in_haystack,
        gap_frames,
        correlate_frames,
        search_radius_frames,
    );
    let discovery_adjustment_secs =
        (nominal_start_frame as f64 - offset_nominal_start as f64) / sample_rate as f64;

    let alignment = policies::align_fill_bracket(
        &a_pre_border,
        &a_post_border,
        &b_mono,
        &a_pre_ch,
        &a_post_ch,
        &b_ch,
        gap_frames,
        nominal_start_frame,
        gap_end_in_haystack,
        correlate_frames,
        max_adjustment_frames,
        search_radius_frames,
    );

    let alignment = match alignment {
        Some(alignment) => alignment,
        None => {
            tracing::warn!(
                a_start_secs,
                "skipping gap fill: boundary alignment failed"
            );
            return (
                None,
                RegionPatchOutcome::Skipped(GapPatchSkipReason::BoundaryAlignmentFailed),
            );
        }
    };

    let gap_secs = gap_frames as f64 / sample_rate as f64;
    if !seams_pass_correlation_gate(
        &alignment,
        min_fill_correlation,
        gap_secs,
        short_gap_mean_correlation_secs,
    ) {
        tracing::warn!(
            pre_correlation = alignment.pre_correlation,
            post_correlation = alignment.post_correlation,
            min_fill_correlation,
            discovery_adjustment_secs,
            align_adjustment_secs = (alignment.start_frame as f64 - nominal_start_frame as f64)
                / sample_rate as f64,
            correlate_window_secs = correlate_frames as f64 / sample_rate as f64,
            b_fill_frames = alignment.fill_frames,
            a_gap_frames = gap_frames,
            a_start_secs,
            "skipping gap fill: boundary correlation below threshold"
        );
        return (
            None,
            RegionPatchOutcome::Skipped(GapPatchSkipReason::CorrelationBelowThreshold {
                pre_correlation: alignment.pre_correlation,
                post_correlation: alignment.post_correlation,
                min_correlation: min_fill_correlation,
            }),
        );
    }

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

    let b_fill_raw = b_samples[fill_start_sample..b_fill_end_sample].to_vec();
    let b_fill = stretch_interleaved_fill(&b_fill_raw, channels, gap_frames);

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
            pre_correlation: alignment.pre_correlation,
            post_correlation: alignment.post_correlation,
            align_adjustment_secs,
        },
    )
}

/// Time-stretch (or compress) an interleaved B fill to exactly `target_frames`.
fn stretch_interleaved_fill(samples: &[i16], channels: usize, target_frames: usize) -> Vec<i16> {
    let channels = channels.max(1);
    let source_frames = samples.len() / channels;
    if source_frames == target_frames {
        return samples.to_vec();
    }
    if source_frames == 0 {
        return vec![0i16; target_frames * channels];
    }

    let mut out = Vec::with_capacity(target_frames * channels);
    for out_frame in 0..target_frames {
        let src_pos = out_frame as f64 * source_frames as f64 / target_frames as f64;
        let idx = src_pos.floor() as usize;
        let frac = (src_pos - idx as f64) as f32;
        let idx2 = (idx + 1).min(source_frames - 1);
        for ch in 0..channels {
            let s0 = f32::from(samples[idx * channels + ch]);
            let s1 = f32::from(samples[idx2 * channels + ch]);
            out.push((s0 * (1.0 - frac) + s1 * frac).round() as i16);
        }
    }
    out
}

fn border_frames_from_secs(window_secs: f64, sample_rate: u32) -> usize {
    (window_secs * sample_rate as f64) as usize
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
