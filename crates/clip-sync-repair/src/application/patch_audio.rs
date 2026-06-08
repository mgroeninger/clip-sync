use std::time::Duration;

use clip_sync::{
    normalized_correlation, select_best_track, ClipLabel, ClipWindow, DomainError, MediaReader,
    MediaSession, MediaSource, MultiChannelPcm, ProgressReporter, resample_interleaved,
};

use crate::application::error::RepairError;
use crate::domain::{
    gap_fill::{build_gap_fill_plan, FillRegion},
    policies, GapReport,
};

pub struct PatchAudioRequest {
    pub report: GapReport,
    pub normalize_fill: bool,
    pub normalize_window_secs: f64,
    pub max_fill_gain_db: f64,
    /// Minimum normalized Pearson correlation between A's pre-gap border audio and the B fill
    /// segment at the seam. Regions below this threshold are skipped.
    pub min_fill_correlation: f32,
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
    ) -> Result<MultiChannelPcm, RepairError> {
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

        // Step 4: If no regions, return A as-is.
        if plan.regions.is_empty() {
            return Ok(a_pcm);
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
        let border_frames = (request.normalize_window_secs * sample_rate as f64) as usize;

        // Step 7: Collect B segments (immutable borrow on a_pcm.samples),
        // then apply them in a separate pass (mutable borrow).
        let mut patches: Vec<RegionPatch> = Vec::new();

        for region in &plan.regions {
            // Step 7a: Extract B segment.
            debug_assert!(
                region.b_start_secs >= 0.0,
                "fill plan must not include gaps with negative B start"
            );
            let b_start = Duration::from_secs_f64(region.b_start_secs);
            let b_end = Duration::from_secs_f64(region.b_end_secs);
            let window_b = ClipWindow::new(b_start, b_end, ClipLabel::Interior);

            let b_pcm = match session_b.extract_interleaved(&track_b, &window_b, self.progress, "patch-b") {
                Ok(pcm) => pcm,
                Err(e) => {
                    tracing::warn!(error = %e, "skipping gap fill region: B extraction failed");
                    continue;
                }
            };

            // Step 7b: Resample B to A's sample rate if needed.
            let b_samples = if b_pcm.sample_rate != sample_rate {
                resample_interleaved(&b_pcm.samples, b_pcm.channels, b_pcm.sample_rate, sample_rate)
            } else {
                b_pcm.samples  // move — b_pcm is not used again
            };

            // Step 7c: Boundary correlation gate — skip fill if A and B don't agree at the seam.
            let gap_start_sample = (region.a_start_secs * sample_rate as f64) as usize * channels;
            let a_border_start = gap_start_sample.saturating_sub(border_frames * channels);
            let a_border = &a_pcm.samples[a_border_start..gap_start_sample];
            let b_border_end = (border_frames * channels).min(b_samples.len());
            let b_border = &b_samples[..b_border_end];
            let corr = boundary_correlation(a_border, b_border, channels);
            if (corr as f32) < request.min_fill_correlation {
                tracing::warn!(
                    corr,
                    min_fill_correlation = request.min_fill_correlation,
                    a_start_secs = region.a_start_secs,
                    "skipping gap fill: boundary correlation below threshold"
                );
                continue;
            }

            // Step 7d: Compute gain.
            let gain = if request.normalize_fill {
                let border_rms = compute_a_border_rms(
                    &a_pcm,
                    region,
                    request.normalize_window_secs,
                    global_a_rms,
                );
                let b_rms = policies::rms_interleaved(&b_samples);
                policies::compute_fill_gain(border_rms, b_rms, request.max_fill_gain_db)
            } else {
                1.0f32
            };

            patches.push(RegionPatch {
                b_samples,
                gain,
                a_start_secs: region.a_start_secs,
                a_end_secs: region.a_end_secs,
                crossfade_secs: region.crossfade_secs,
            });
        }

        // Step 8: Apply patches to A samples.
        for patch in patches {
            // Apply gain to B.
            let b_gained: Vec<i16> = patch
                .b_samples
                .iter()
                .map(|&s| {
                    (s as f32 * patch.gain)
                        .round()
                        .clamp(i16::MIN as f32, i16::MAX as f32) as i16
                })
                .collect();

            // Splice into A.
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

        Ok(a_pcm)
    }
}

/// Normalized Pearson correlation between A's pre-gap border audio and the start of B's fill.
///
/// Both interleaved i16 slices are averaged to mono before comparison. Returns a value in [-1, 1].
fn boundary_correlation(a_border: &[i16], b_border: &[i16], channels: usize) -> f64 {
    let channels = channels.max(1);
    let to_mono = |s: &[i16]| -> Vec<f64> {
        s.chunks(channels)
            .map(|frame| frame.iter().map(|&x| f64::from(x)).sum::<f64>() / channels as f64)
            .collect()
    };
    let a_mono = to_mono(a_border);
    let b_mono = to_mono(b_border);
    let len = a_mono.len().min(b_mono.len());
    if len == 0 {
        // No border samples available — treat as uncorrelated rather than blocking the fill.
        return 1.0;
    }
    normalized_correlation(&a_mono[..len], &b_mono[..len])
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
    let window_frames = (window_secs * sample_rate as f64) as usize;

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
    let a_start_sample = (a_start_secs * sample_rate as f64) as usize * channels;
    let a_end_sample = (a_end_secs * sample_rate as f64) as usize * channels;

    if a_start_sample >= a_samples.len() || a_end_sample > a_samples.len() {
        tracing::warn!(
            a_start_sample,
            a_end_sample,
            a_len = a_samples.len(),
            "splice_into_a: region out of range, skipping"
        );
        return;
    }

    let gap_len = a_end_sample - a_start_sample;
    if gap_len == 0 {
        return;
    }

    // Pad or truncate b_samples to match gap length.
    let b_padded: Vec<i16> = if b_samples.len() >= gap_len {
        b_samples[..gap_len].to_vec()
    } else {
        let mut padded = b_samples.to_vec();
        padded.resize(gap_len, 0);
        padded
    };

    let crossfade_frames = (crossfade_secs * sample_rate as f64) as usize;

    policies::apply_crossfade(
        &mut a_samples[a_start_sample..a_end_sample],
        &b_padded,
        channels,
        crossfade_frames,
    );
}
