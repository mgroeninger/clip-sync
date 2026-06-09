use std::collections::HashMap;
use std::time::Duration;

use clip_sync::{
    select_best_track, AudioTrack, ClipLabel, ClipWindow, DomainError, MediaReader, MediaSession,
    MediaSource, MultiChannelPcm, ProgressReporter, resample_interleaved,
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
}

// Collected B segment ready to splice into A.
struct RegionPatch {
    b_samples: Vec<i16>,
    gain: f32,
    a_start_secs: f64,
    a_end_secs: f64,
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

        // Step 6: Compute global A RMS as normalization fallback.
        let global_a_rms = policies::rms_interleaved(&a_pcm.samples);

        let channels = a_pcm.channels as usize;
        let sample_rate = a_pcm.sample_rate;
        let max_adjustment_frames =
            (request.max_fill_align_adjustment_secs * sample_rate as f64).round() as usize;
        // Keep the alignment correlate window small so it fits inside the extended B extract.
        let align_correlate_secs = request.normalize_window_secs.min(1.0);
        let correlate_frames =
            ((align_correlate_secs * sample_rate as f64) as usize).max(1);

        // Step 7: Collect B segments (immutable borrow on a_pcm.samples),
        // then apply them in a separate pass (mutable borrow).
        let mut patches: Vec<RegionPatch> = Vec::new();
        let mut region_results: Vec<(f64, f64, RegionPatchOutcome)> = Vec::new();

        for region in &plan.regions {
            let (patch, outcome) = prepare_region_patch(
                &session_b,
                &track_b,
                &a_pcm,
                region,
                channels,
                sample_rate,
                max_adjustment_frames,
                correlate_frames,
                request.fill_align_margin_secs,
                request.min_fill_correlation,
                request.normalize_fill,
                request.normalize_window_secs,
                request.max_fill_gain_db,
                global_a_rms,
                self.progress,
            );
            region_results.push((region.a_start_secs, region.a_end_secs, outcome));
            if let Some(patch) = patch {
                patches.push(patch);
            }
        }

        // Step 8: Apply patches to A samples.
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
                patch.a_start_secs,
                patch.a_end_secs,
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
    session_b: &impl MediaSession,
    track_b: &AudioTrack,
    a_pcm: &MultiChannelPcm,
    region: &FillRegion,
    channels: usize,
    sample_rate: u32,
    max_adjustment_frames: usize,
    correlate_frames: usize,
    margin_secs: f64,
    min_fill_correlation: f32,
    normalize_fill: bool,
    normalize_window_secs: f64,
    max_fill_gain_db: f64,
    global_a_rms: f32,
    progress: &dyn ProgressReporter,
) -> (Option<RegionPatch>, RegionPatchOutcome) {
    debug_assert!(
        region.b_start_secs >= 0.0,
        "fill plan must not include gaps with negative B start"
    );

    let b_extract_start_secs = (region.b_start_secs - margin_secs).max(0.0);
    let b_extract_end_secs = region.b_end_secs + margin_secs;
    let window_b = ClipWindow::new(
        Duration::from_secs_f64(b_extract_start_secs),
        Duration::from_secs_f64(b_extract_end_secs),
        ClipLabel::Interior,
    );

    let b_pcm = match session_b.extract_interleaved(track_b, &window_b, progress, "patch-b") {
        Ok(pcm) => pcm,
        Err(e) => {
            tracing::warn!(error = %e, "skipping gap fill region: B extraction failed");
            return (
                None,
                RegionPatchOutcome::Skipped(GapPatchSkipReason::BExtractFailed),
            );
        }
    };

    let b_samples = if b_pcm.sample_rate != sample_rate {
        resample_interleaved(
            &b_pcm.samples,
            b_pcm.channels,
            b_pcm.sample_rate,
            sample_rate,
        )
    } else {
        b_pcm.samples
    };

    let gap_start_frame = (region.a_start_secs * sample_rate as f64) as usize;
    let gap_end_frame = (region.a_end_secs * sample_rate as f64) as usize;
    let gap_frames = gap_end_frame.saturating_sub(gap_start_frame);
    if gap_frames == 0 {
        return (
            None,
            RegionPatchOutcome::Skipped(GapPatchSkipReason::ZeroLengthGap),
        );
    }

    let pre_start_frame = gap_start_frame.saturating_sub(border_frames_from_secs(
        normalize_window_secs,
        sample_rate,
    ));
    let post_end_frame = (gap_end_frame
        + border_frames_from_secs(normalize_window_secs, sample_rate))
    .min(a_pcm.samples.len() / channels);

    let a_pre_border = policies::interleaved_to_mono(
        &a_pcm.samples[pre_start_frame * channels..gap_start_frame * channels],
        channels,
    );
    let a_post_border = policies::interleaved_to_mono(
        &a_pcm.samples[gap_end_frame * channels..post_end_frame * channels],
        channels,
    );
    let b_mono = policies::interleaved_to_mono(&b_samples, channels);

    let nominal_start_frame =
        ((region.b_start_secs - b_extract_start_secs) * sample_rate as f64).round() as usize;

    let alignment = policies::align_fill_segment(
        &a_pre_border,
        &a_post_border,
        &b_mono,
        gap_frames,
        nominal_start_frame,
        correlate_frames,
        max_adjustment_frames,
    );

    let alignment = match alignment {
        Some(alignment) => alignment,
        None => {
            tracing::warn!(
                a_start_secs = region.a_start_secs,
                "skipping gap fill: boundary alignment failed"
            );
            return (
                None,
                RegionPatchOutcome::Skipped(GapPatchSkipReason::BoundaryAlignmentFailed),
            );
        }
    };

    if !seams_pass_correlation_gate(&alignment, min_fill_correlation) {
        tracing::warn!(
            pre_correlation = alignment.pre_correlation,
            post_correlation = alignment.post_correlation,
            min_fill_correlation,
            a_start_secs = region.a_start_secs,
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
    let fill_end_sample = fill_start_sample + gap_frames * channels;
    if fill_end_sample > b_samples.len() {
        tracing::warn!(
            a_start_secs = region.a_start_secs,
            "skipping gap fill: aligned B segment out of range"
        );
        return (
            None,
            RegionPatchOutcome::Skipped(GapPatchSkipReason::AlignedSegmentOutOfRange),
        );
    }

    let b_fill = b_samples[fill_start_sample..fill_end_sample].to_vec();

    let gain = if normalize_fill {
        let border_rms = compute_a_border_rms(a_pcm, region, normalize_window_secs, global_a_rms);
        let b_rms = policies::rms_interleaved(&b_fill);
        policies::compute_fill_gain(border_rms, b_rms, max_fill_gain_db)
    } else {
        1.0f32
    };

    let align_adjustment_secs =
        (alignment.start_frame as f64 - nominal_start_frame as f64) / sample_rate as f64;

    (
        Some(RegionPatch {
            b_samples: b_fill,
            gain,
            a_start_secs: region.a_start_secs,
            a_end_secs: region.a_end_secs,
            crossfade_secs: region.crossfade_secs,
        }),
        RegionPatchOutcome::Patched {
            pre_correlation: alignment.pre_correlation,
            post_correlation: alignment.post_correlation,
            align_adjustment_secs,
        },
    )
}

fn border_frames_from_secs(window_secs: f64, sample_rate: u32) -> usize {
    (window_secs * sample_rate as f64) as usize
}

fn seams_pass_correlation_gate(alignment: &FillAlignment, min_fill_correlation: f32) -> bool {
    (alignment.pre_correlation as f32) >= min_fill_correlation
        && (alignment.post_correlation as f32) >= min_fill_correlation
}

/// Compute RMS of the A samples bordering the gap region.
///
/// Looks at `window_secs` before the gap start and `window_secs` after the gap end.
/// Returns `fallback` when the computed RMS is zero.
fn compute_a_border_rms(
    a_pcm: &MultiChannelPcm,
    region: &FillRegion,
    window_secs: f64,
    fallback: f32,
) -> f32 {
    let channels = a_pcm.channels as usize;
    let sample_rate = a_pcm.sample_rate;

    let gap_start_frame = (region.a_start_secs * sample_rate as f64) as usize;
    let gap_end_frame = (region.a_end_secs * sample_rate as f64) as usize;
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
    a_start_secs: f64,
    a_end_secs: f64,
    crossfade_secs: f64,
    sample_rate: u32,
) {
    let channels = channels.max(1);
    let gap_start_frame = (a_start_secs * sample_rate as f64) as usize;
    let gap_end_frame = (a_end_secs * sample_rate as f64) as usize;

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
