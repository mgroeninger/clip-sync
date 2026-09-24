//! PCM primitives for the region patch pipeline: slicing B, applying gain, measuring
//! border RMS / gap floor, and splicing the finished fill into A.
//!
//! Depends only on `types`. `gained_fill` lives here rather than with `RegionPatch` because it
//! is the splice / measurement source of truth for "what gets written", next to `splice_into_a`.

use crate::domain::{
    patch_result::GapPatchNotAppliedReason,
    policies::{self},
};

use super::types::{RegionPatch, SeamMargin};

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

/// The same gain [`gained_fill`] applies, applied to the seam margins.
///
/// The margins are spliced next to the fill, so they must carry the identical gain — an ungained
/// margin would put a level step exactly at the seam the margin exists to smooth.
pub(in crate::application::patch_audio) fn gained_seam_margin(
    patch: &RegionPatch,
    _channels: usize,
) -> Option<(Vec<f32>, Vec<f32>)> {
    let margin = patch.seam_margin.as_ref()?;
    let g = |v: &[f32]| -> Vec<f32> {
        v.iter()
            .map(|&s| (s * patch.gain).clamp(-1.0, 1.0))
            .collect()
    };
    Some((g(&margin.head), g(&margin.tail)))
}

/// Cut `cf` frames of B either side of the written fill, for [`policies::apply_seam_crossfade_extended`].
///
/// `fill_start_frame` is the fill's start in **full-B** frames, already including the fit's
/// `head_trim_frames` — margins cut at the unadjusted alignment start would not be contiguous with
/// the audio actually spliced. Returns `None` at the edges of B, where the slice does not exist;
/// the caller then splices exactly as it always did.
pub(in crate::application::patch_audio::region) fn seam_margin_from_b(
    b_full: &[f32],
    channels: usize,
    fill_start_frame: usize,
    gap_frames: usize,
    cf: usize,
) -> Option<SeamMargin> {
    if cf == 0 {
        return None;
    }
    let ch = channels.max(1);
    let total_frames = b_full.len() / ch;
    let head_start = fill_start_frame.checked_sub(cf)?;
    let tail_start = fill_start_frame.checked_add(gap_frames)?;
    let tail_end = tail_start.checked_add(cf)?;
    if tail_end > total_frames {
        return None;
    }
    Some(SeamMargin {
        head: b_full[head_start * ch..fill_start_frame * ch].to_vec(),
        tail: b_full[tail_start * ch..tail_end * ch].to_vec(),
    })
}

/// As [`seam_margin_from_b`], but for a dual-fit bridge, whose two shoulders sit at their own lags.
///
/// [`seam_margin_from_b`] derives both margins from one `fill_start_frame` (`head` at
/// `fill_start − cf`, `tail` at `fill_start + gap_frames`) because a bracket fill *is* one
/// contiguous B span. A bridge is not: `tail` must be cut at the **post** shoulder's B position, not
/// at the pre shoulder's plus the gap length — the two differ by the shoulder skew
/// (`post_lag − pre_lag`, which is also `trim_frames`). Cutting the tail from the pre shoulder would
/// reintroduce a seam discontinuity of exactly that skew, which is the defect this margin exists to
/// remove.
///
/// Frames are **B-extract**-relative, the space `pre_lag`/`post_lag` are anchored in — not full-B as
/// in [`seam_margin_from_b`]. Returns `None` when either slice falls outside the extract; the caller
/// then splices exactly as it always did.
pub(in crate::application::patch_audio::region) fn seam_margin_from_b_shoulders(
    b_extract: &[f32],
    channels: usize,
    b_pre_seam_frame: i64,
    b_post_seam_frame: i64,
    cf: usize,
) -> Option<(Vec<f32>, Vec<f32>)> {
    if cf == 0 {
        return None;
    }
    let ch = channels.max(1);
    let total_frames = (b_extract.len() / ch) as i64;
    let cf_i = cf as i64;
    let head_start = b_pre_seam_frame.checked_sub(cf_i)?;
    let tail_end = b_post_seam_frame.checked_add(cf_i)?;
    if head_start < 0 || tail_end > total_frames || b_post_seam_frame < 0 {
        return None;
    }
    let cut = |from: i64, to: i64| -> Vec<f32> {
        b_extract[from as usize * ch..to as usize * ch].to_vec()
    };
    Some((
        cut(head_start, b_pre_seam_frame),
        cut(b_post_seam_frame, tail_end),
    ))
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

/// Splice B samples into A's interleaved sample buffer at the gap location.
///
/// `Err` means A was left byte-identical across the gap: the caller must report the gap as
/// [`GapPatchStatus::NotApplied`](crate::domain::patch_result::GapPatchStatus::NotApplied) rather
/// than `Patched`. Every error arm is a geometry defect upstream of here, not a routine outcome.
pub(in crate::application::patch_audio) struct SpliceFill<'a> {
    pub(in crate::application::patch_audio) b_samples: &'a [f32],
    /// Gained B either side of the fill; `None` splices via the shipped seam path.
    pub(in crate::application::patch_audio) seam_margin: Option<(&'a [f32], &'a [f32])>,
    pub(in crate::application::patch_audio) crossfade_secs: f64,
    pub(in crate::application::patch_audio) sample_rate: u32,
}

pub(in crate::application::patch_audio) fn splice_into_a(
    a_samples: &mut [f32],
    fill: SpliceFill<'_>,
    channels: usize,
    gap_start_frame: usize,
    gap_end_frame: usize,
) -> Result<(), GapPatchNotAppliedReason> {
    let SpliceFill {
        b_samples,
        seam_margin,
        crossfade_secs,
        sample_rate,
    } = fill;
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

    match seam_margin {
        Some((head, tail)) => policies::apply_seam_crossfade_extended(
            a_samples,
            policies::SeamMarginFill {
                head,
                fill: b_samples,
                tail,
            },
            channels,
            gap_start_frame,
            gap_end_frame,
            crossfade_frames,
        ),
        None => policies::apply_seam_crossfade(
            a_samples,
            b_samples,
            channels,
            gap_start_frame,
            gap_end_frame,
            crossfade_frames,
        ),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    /// Mono ramp: frame `i` holds `i as f32`, so a cut's contents name the frames it came from.
    fn ramp(frames: usize) -> Vec<f32> {
        (0..frames).map(|i| i as f32).collect()
    }

    /// The tail is cut at the POST shoulder, not at the pre shoulder plus `gap_frames`.
    ///
    /// This is the whole reason the dual-fit builder exists: with a non-zero shoulder skew the two
    /// answers differ by exactly `trim_frames`, and the single-lag arithmetic would put that step
    /// back at the seam the margin is meant to smooth.
    #[test]
    fn shoulder_margin_cuts_the_tail_at_the_post_shoulder() {
        let b = ramp(1000);
        let (pre, post, gap, cf) = (500_i64, 530_i64, 100_i64, 4);
        let (head, tail) =
            super::seam_margin_from_b_shoulders(&b, 1, pre, post + gap, cf).expect("in range");
        assert_eq!(
            head,
            vec![496.0, 497.0, 498.0, 499.0],
            "head ends at the pre shoulder"
        );
        assert_eq!(
            tail,
            vec![630.0, 631.0, 632.0, 633.0],
            "tail starts at the post shoulder"
        );
        // The single-lag reading would have started the tail at pre + gap = 600.
        assert_ne!(tail[0], (pre + gap) as f32);
    }

    /// Zero skew is the degenerate case where both models agree — a guard that the shoulder builder
    /// does not drift from [`super::seam_margin_from_b`] when there is nothing to disagree about.
    #[test]
    fn zero_skew_matches_the_single_lag_cut() {
        let b = ramp(1000);
        let (start, gap, cf) = (500_usize, 100_usize, 4);
        let single = super::seam_margin_from_b(&b, 1, start, gap, cf).expect("in range");
        let dual =
            super::seam_margin_from_b_shoulders(&b, 1, start as i64, (start + gap) as i64, cf)
                .expect("in range");
        assert_eq!((single.head, single.tail), dual);
    }

    /// Out-of-extract cuts abstain rather than panic or clamp — the caller then splices as before.
    #[test]
    fn shoulder_margin_abstains_outside_the_extract() {
        let b = ramp(1000);
        assert!(
            super::seam_margin_from_b_shoulders(&b, 1, 2, 500, 4).is_none(),
            "head would start before the extract"
        );
        assert!(
            super::seam_margin_from_b_shoulders(&b, 1, 500, 998, 4).is_none(),
            "tail would run past the extract"
        );
        assert!(
            super::seam_margin_from_b_shoulders(&b, 1, 500, 600, 0).is_none(),
            "a zero crossfade has no margin to cut"
        );
    }

    /// The pre-margin argument shape, so these cases keep testing the geometry guards only.
    fn plain_fill(b: &[f32]) -> super::SpliceFill<'_> {
        super::SpliceFill {
            b_samples: b,
            seam_margin: None,
            crossfade_secs: 0.0,
            sample_rate: 48_000,
        }
    }

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
            splice_into_a(&mut a, plain_fill(&fill), 2, 100, 200),
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
            splice_into_a(&mut a, plain_fill(&fill), 2, 4, 4),
            Err(GapPatchNotAppliedReason::DestinationEmptyOrInverted {
                gap_start_frame: 4,
                gap_end_frame: 4,
            })
        );
        assert_eq!(a, before, "an empty destination must not modify A");

        let mut a = vec![0.25f32; 16];
        assert_eq!(
            splice_into_a(&mut a, plain_fill(&fill), 2, 0, 8),
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
        let _ = super::splice_into_a(&mut a, plain_fill(&[0.5f32; 4]), 2, 0, 8);
    }
}
