//! PCM primitives for the region patch pipeline: slicing B, applying gain, measuring
//! border RMS / gap floor, and splicing the finished fill into A.
//!
//! Depends only on `types`. `gained_fill` lives here rather than with `RegionPatch` because it
//! is the splice / measurement source of truth for "what gets written", next to `splice_into_a`.

use clip_sync::MultiChannelPcm;

use crate::domain::{
    patch_result::GapPatchNotAppliedReason,
    policies::{self},
};

use super::super::border_frames_from_secs;

use super::types::RegionPatch;

/// The PCM this patch actually puts into A: gain applied, clamped, and cut to the destination span.
///
/// Single source of truth for "what gets written", used by both the splice and the record-only
/// fill-level measurement (`super::measure_fill_level`). They were two copies of the same
/// expression, which is how a diagnostic quietly starts describing something other than the audio.
///
/// The truncation is the reason this cuts rather than just gains: `splice_into_a` needs
/// `len >= gap_frames * channels` and writes exactly that many, so any surplus tail is never heard
/// and must not reach a measurement. Every fill goes through `fit_fill_to_gap_frames` and arrives
/// exactly the right length today; this keeps that from being load-bearing. A *short* fill is left
/// short, so the splice can still reject it as `FillShorterThanGap` rather than being handed a
/// silently padded buffer.
pub(in crate::application::patch_audio) fn gained_fill(
    patch: &RegionPatch,
    channels: usize,
) -> Vec<f32> {
    let destination = patch.a_end_frame.saturating_sub(patch.a_start_frame) * channels.max(1);
    patch
        .b_samples
        .iter()
        .take(destination)
        .map(|&s| (s * patch.gain).clamp(-1.0, 1.0))
        .collect()
}

/// A-side gap-interior RMS floor (dB) — approximation of the fingerprint `levels.gap_floor_db`.
pub(in crate::application::patch_audio::region) fn a_gap_floor_db(
    a_samples: &[f32],
    channels: usize,
    gap_start: usize,
    gap_end: usize,
) -> f64 {
    let ch = channels.max(1);
    let lo = gap_start.min(gap_end);
    let hi = gap_start.max(gap_end);
    if lo >= hi {
        return -120.0;
    }
    let sum_sq: f64 = (lo..hi)
        .map(|f| {
            let base = f * ch;
            let m = a_samples[base..base + ch]
                .iter()
                .map(|&x| x as f64)
                .sum::<f64>()
                / ch as f64;
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

pub(in crate::application::patch_audio::region) fn b_mapped_start_frame(
    refined_b_start_secs: f64,
    b_extract_start_secs: f64,
    sample_rate: u32,
) -> usize {
    (((refined_b_start_secs - b_extract_start_secs) * sample_rate as f64).round() as i64).max(0)
        as usize
}

/// Slice the B extract window out of the **full** B buffer — the one place that turns a
/// [`BExtractWindow`](crate::domain::gap_repair_spec::BExtractWindow)'s frame indices into the actual `&[f32]` characterize works on (S1).
///
/// **The clamp is the load-bearing part.** `end_frame` as stored on the spec is the *nominal* window end
/// and is deliberately left unclamped (it is derived geometry, not a buffer fact); near the end of B it can
/// point past the decode buffer. Clamping here — rather than at either call site — is what lets the executor
/// re-slice from `spec.b_extract` and get back the byte-identical buffer characterize used. Without it the
/// executor's slice would be longer, which lengthens `b_extension` and silently changes both Gate-mode
/// extension and Fit-mode length fitting on end-of-B gaps.
pub(in crate::application::patch_audio::region) fn slice_b_extract(
    b_samples: &[f32],
    channels: usize,
    start_frame: usize,
    end_frame: usize,
) -> Option<&[f32]> {
    let channels = channels.max(1);
    let end_frame = end_frame.min(b_samples.len() / channels);
    if start_frame >= end_frame {
        return None;
    }
    Some(&b_samples[start_frame * channels..end_frame * channels])
}

/// Frame indices of the B extract window for a gap. Kept next to [`slice_b_extract`] so the secs→frames
/// rounding that characterize stores on the spec and the rounding the slice uses cannot drift apart.
pub(in crate::application::patch_audio::region) fn b_extract_frames(
    sample_rate: u32,
    start_secs: f64,
    end_secs: f64,
) -> (usize, usize) {
    (
        (start_secs * sample_rate as f64).round() as usize,
        (end_secs * sample_rate as f64).round() as usize,
    )
}

/// Compute RMS of the A samples bordering the gap region.
///
/// Looks at `window_secs` before the gap start and `window_secs` after the gap end.
/// Returns `fallback` when the computed RMS is zero.
pub(in crate::application::patch_audio::region) fn compute_a_border_rms(
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
    if rms == 0.0 {
        fallback
    } else {
        rms
    }
}

/// Splice B samples into A's interleaved sample buffer at the gap location.
///
/// `Err` means A was left byte-identical across the gap: the caller must report the gap as
/// [`GapPatchStatus::NotApplied`](crate::domain::patch_result::GapPatchStatus::NotApplied) rather
/// than `Patched`. Every error arm is a geometry defect upstream of here, not a routine outcome.
pub(in crate::application::patch_audio) fn splice_into_a(
    a_samples: &mut [f32],
    b_samples: &[f32],
    channels: usize,
    gap_start_frame: usize,
    gap_end_frame: usize,
    crossfade_secs: f64,
    sample_rate: u32,
) -> Result<(), GapPatchNotAppliedReason> {
    let channels = channels.max(1);

    if gap_start_frame * channels >= a_samples.len() || gap_end_frame * channels > a_samples.len() {
        tracing::warn!(
            gap_start_frame,
            gap_end_frame,
            a_len = a_samples.len() / channels,
            "splice_into_a: region out of range, skipping"
        );
        return Err(GapPatchNotAppliedReason::DestinationOutOfRange {
            gap_start_frame,
            gap_end_frame,
            a_frames: a_samples.len() / channels,
        });
    }

    if gap_start_frame >= gap_end_frame {
        tracing::warn!(
            gap_start_frame,
            gap_end_frame,
            "splice_into_a: destination empty or inverted, skipping"
        );
        return Err(GapPatchNotAppliedReason::DestinationEmptyOrInverted {
            gap_start_frame,
            gap_end_frame,
        });
    }

    let gap_frames = gap_end_frame.saturating_sub(gap_start_frame);
    let needed_samples = gap_frames * channels;
    // Every fill path terminates in `fit_fill_to_gap_frames`, which pads or truncates to exactly
    // `target_frames * channels` — so a short fill here means the fill was fitted to a different
    // frame count than the destination it is being spliced into. Loud in debug, degraded to
    // `NotApplied` in release rather than silently reported as `Patched`.
    debug_assert!(
        b_samples.len() >= needed_samples,
        "fill ({} samples) shorter than splice destination ({needed_samples} samples): fill was \
         fitted to a different frame count than [{gap_start_frame}, {gap_end_frame})",
        b_samples.len()
    );
    if b_samples.len() < needed_samples {
        tracing::warn!(
            gap_start_frame,
            gap_end_frame,
            b_len = b_samples.len(),
            needed_samples,
            "splice_into_a: B fill shorter than gap, skipping"
        );
        return Err(GapPatchNotAppliedReason::FillShorterThanGap {
            fill_frames: b_samples.len() / channels,
            gap_frames,
        });
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **S1 — the B-extract re-slice clamps, so the executor gets characterize's exact buffer.** The window
    /// stored on a spec is nominal geometry and is deliberately unclamped, so near the end of B it points
    /// past the decode buffer. Both passes must nonetheless slice the same samples: an unclamped executor
    /// slice would be longer, lengthening `b_extension` and changing Gate-mode extension / Fit-mode length
    /// fitting on exactly those gaps. Fixtures don't reliably place a gap at the end of B, so pin it here.
    #[test]
    fn b_extract_slice_clamps_a_window_that_overruns_the_buffer() {
        let channels = 2usize;
        let frames = 100usize;
        let b: Vec<f32> = (0..frames * channels).map(|i| i as f32).collect();

        // A window whose nominal end runs 50 frames past the buffer yields the same slice as one that
        // stops exactly at the end — this is the equality the executor's reconstruction depends on.
        let overrun =
            slice_b_extract(&b, channels, 10, frames + 50).expect("overrunning window slices");
        let exact = slice_b_extract(&b, channels, 10, frames).expect("exact window slices");
        assert_eq!(overrun, exact);
        assert_eq!(overrun.len(), (frames - 10) * channels);

        // Degenerate windows stay `None` rather than panicking on the reversed range.
        assert!(slice_b_extract(&b, channels, 10, 10).is_none());
        assert!(slice_b_extract(&b, channels, frames + 10, frames + 50).is_none());
    }

    /// **Every `splice_into_a` bail is reportable, and none of them touch A.**
    ///
    /// Each arm names which invariant broke, because the first real occurrence is expected on
    /// media that cannot be re-run. The fill-shorter arm is excluded here: it trips a
    /// `debug_assert!` (see `splice_shorter_fill_trips_the_debug_assert`) and is only reachable in
    /// release.
    #[test]
    fn splice_bails_report_a_reason_and_leave_a_untouched() {
        use super::splice_into_a;
        use crate::domain::patch_result::GapPatchNotAppliedReason;

        let fill = vec![0.5f32; 64];

        let mut a = vec![0.25f32; 16];
        let before = a.clone();
        assert_eq!(
            splice_into_a(&mut a, &fill, 2, 100, 200, 0.0, 48_000),
            Err(GapPatchNotAppliedReason::DestinationOutOfRange {
                gap_start_frame: 100,
                gap_end_frame: 200,
                a_frames: 8,
            })
        );
        assert_eq!(a, before, "an out-of-range destination must not modify A");

        let mut a = vec![0.25f32; 16];
        let before = a.clone();
        assert_eq!(
            splice_into_a(&mut a, &fill, 2, 4, 4, 0.0, 48_000),
            Err(GapPatchNotAppliedReason::DestinationEmptyOrInverted {
                gap_start_frame: 4,
                gap_end_frame: 4,
            })
        );
        assert_eq!(a, before, "an empty destination must not modify A");

        let mut a = vec![0.25f32; 16];
        assert_eq!(
            splice_into_a(&mut a, &fill, 2, 0, 8, 0.0, 48_000),
            Ok(()),
            "a well-formed splice still succeeds"
        );
        assert_ne!(a, before, "a successful splice must modify A");
    }

    /// The fill-length invariant is *enforced*, not sampled. Every fill path terminates in
    /// `fit_fill_to_gap_frames`, which returns exactly `target_frames * channels` — so a short fill
    /// is a construction bug, and no corpus can prove it will never happen. Debug builds panic on
    /// it; release degrades to `NotApplied` rather than a false `Patched`.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "shorter than splice destination")]
    fn splice_shorter_fill_trips_the_debug_assert() {
        let mut a = vec![0.25f32; 16];
        let _ = super::splice_into_a(&mut a, &[0.5f32; 4], 2, 0, 8, 0.0, 48_000);
    }
}
