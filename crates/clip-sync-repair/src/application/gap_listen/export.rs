//! Windowed PCM export: slice a few seconds out of a full-timeline interleaved buffer and write it
//! as a WAV. The sample-level half of `--gap-listen`; orchestration lives in the parent module.

use std::path::Path;

use clip_sync::{BitDepth, MultiChannelPcm};

use crate::application::error::RepairError;
use crate::application::ports::PatchedAudioWriter;

/// A half-open window over an interleaved buffer, in **frames** (never samples).
///
/// Frames, because every downstream invariant is per-frame: `validate_pcm_layout` requires
/// `samples.len() % channels == 0`, and the pre/patched WAV pair is only comparable if both were
/// cut at the same frame boundaries. A sample-indexed range would let a channel-count slip produce
/// a plausible-length, wrong-sounding file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FrameRange {
    pub start: usize,
    pub end: usize,
}

impl FrameRange {
    pub fn len(self) -> usize {
        self.end.saturating_sub(self.start)
    }

    pub fn is_empty(self) -> bool {
        self.len() == 0
    }
}

/// Clamp `[start_secs, end_secs]` to a frame range inside a buffer of `total_frames`.
///
/// Both ends saturate rather than erroring: a gap within `context` seconds of the start or end of
/// the timeline yields a shorter window, which is the right behavior for a listening clip. `start`
/// floors and `end` ceils so the requested audio is always fully covered.
pub(crate) fn frame_range(
    start_secs: f64,
    end_secs: f64,
    sample_rate: u32,
    total_frames: usize,
) -> FrameRange {
    let rate = f64::from(sample_rate.max(1));
    let start = (start_secs.max(0.0) * rate).floor() as usize;
    let end = (end_secs.max(0.0) * rate).ceil() as usize;
    FrameRange {
        start: start.min(total_frames),
        end: end.min(total_frames).max(start.min(total_frames)),
    }
}

/// Copy one frame window out of an interleaved buffer.
///
/// `channels` must be **the buffer's own** channel count. For B that is `DecodedAb::b_channels`,
/// *not* A's and *not* `sources.b.native_channels`: `decode_ab` resamples B to A's rate but never
/// remixes its layout, so `b_samples_full` is interleaved at B's decoded layout and A's rate.
/// `native_channels` is container/probe metadata and can disagree with the decoder. Either wrong
/// number scrambles the interleaving into a file that is the right length and the wrong audio — an
/// audible failure, not a panic.
///
/// `compressed_bytes` / `decoded_frame_count` are deliberately dropped: both describe the *whole*
/// decode, so carrying them onto a window would make `measured_bitrate_bps()` report a bitrate for
/// a duration it was never measured over. `source_bit_depth` is kept — it drives
/// `resolve_output_bit_depth` and so the WAV's sample format.
pub(crate) fn slice_window(
    samples: &[f32],
    channels: u16,
    sample_rate: u32,
    source_bit_depth: Option<BitDepth>,
    range: FrameRange,
) -> MultiChannelPcm {
    let ch = channels.max(1) as usize;
    let total_frames = samples.len() / ch;
    let start = range.start.min(total_frames);
    let end = range.end.clamp(start, total_frames);

    MultiChannelPcm {
        sample_rate,
        channels,
        samples: samples[start * ch..end * ch].to_vec(),
        decode_error_skips: 0,
        decoded_frame_count: None,
        compressed_bytes: None,
        source_bit_depth,
    }
}

/// Order-sensitive digest of a window's samples, used to catch a patched window that came out
/// identical to the pre-patch one.
///
/// A digest rather than the samples themselves because the "before" window has to survive across
/// the splice, which consumes the decode and mutates A in place. Holding every pre-window would
/// cost megabytes per selected gap for a comparison that needs 8 bytes.
///
/// Hashes raw bit patterns (`f32::to_bits`): `f32` is not `Hash`, and bit equality is the right
/// question anyway — a window is unchanged iff its bits are. Length is folded in first so a
/// truncated window cannot collide with a longer one sharing its prefix.
///
/// `DefaultHasher` is not cryptographic and does not need to be: nothing here is adversarial, and a
/// collision costs one *missing* warning about a suspicious patch, never a wrong patch.
pub(crate) fn window_digest(samples: &[f32]) -> u64 {
    use std::hash::{Hash, Hasher};

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    samples.len().hash(&mut hasher);
    for sample in samples {
        sample.to_bits().hash(&mut hasher);
    }
    hasher.finish()
}

/// Slice a window and write it as `<dir>/<stem><suffix>`.
///
/// Returns the written path so the caller can log/report it.
pub(crate) fn write_window_wav<PW: PatchedAudioWriter>(
    writer: &PW,
    dir: &Path,
    stem: &str,
    suffix: &str,
    pcm: &MultiChannelPcm,
) -> Result<std::path::PathBuf, RepairError> {
    let path = dir.join(format!("{stem}{suffix}"));
    writer.write(pcm, &path)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::{frame_range, slice_window, window_digest, FrameRange};
    use clip_sync::BitDepth;

    /// Ramp where every frame's samples encode `frame * 10 + channel`, so a mis-strided slice is
    /// detectable by value rather than only by length.
    fn ramp(frames: usize, channels: u16) -> Vec<f32> {
        (0..frames)
            .flat_map(|f| (0..channels).map(move |c| (f * 10 + c as usize) as f32))
            .collect()
    }

    #[test]
    fn frame_range_floors_start_and_ceils_end() {
        // 1.5 s .. 2.25 s at 100 Hz = frames 150..225, both exact.
        assert_eq!(frame_range(1.5, 2.25, 100, 1000), FrameRange {
            start: 150,
            end: 225
        });
        // Fractional frames: start floors down, end ceils up, so the ask is fully covered.
        let r = frame_range(1.004, 2.001, 1000, 100_000);
        assert_eq!(r, FrameRange {
            start: 1004,
            end: 2001
        });
    }

    #[test]
    fn frame_range_clamps_both_ends_instead_of_erroring() {
        // A gap near t=0 with 3 s of context asks for a negative start.
        assert_eq!(frame_range(-3.0, 1.0, 100, 1000), FrameRange {
            start: 0,
            end: 100
        });
        // A gap near the end asks past the buffer.
        assert_eq!(frame_range(9.0, 99.0, 100, 1000), FrameRange {
            start: 900,
            end: 1000
        });
        // Entirely past the end collapses to empty at the tail, never inverted.
        let r = frame_range(50.0, 60.0, 100, 1000);
        assert!(r.is_empty());
        assert!(r.end >= r.start);
    }

    #[test]
    fn slice_window_cuts_on_frame_boundaries() {
        let samples = ramp(10, 2);
        let pcm = slice_window(
            &samples,
            2,
            48_000,
            Some(BitDepth::Int24),
            FrameRange { start: 3, end: 6 },
        );
        assert_eq!(pcm.channels, 2);
        assert_eq!(pcm.frames(), 3);
        // Frame 3 onward, both channels, in order.
        assert_eq!(pcm.samples, vec![30.0, 31.0, 40.0, 41.0, 50.0, 51.0]);
        assert_eq!(pcm.samples.len() % pcm.channels as usize, 0);
    }

    /// The §5.1 trap: B's window must be cut at B's channel count. `decode_ab` resamples B's rate
    /// but never its layout, so on a mismatched pair A's count would silently mis-stride B.
    #[test]
    fn slice_window_strides_by_the_buffers_own_channel_count() {
        // B is 2ch; A (not used here) would be 6ch on a mismatched pair.
        let b_samples = ramp(10, 2);
        let b_channels = 2u16;
        let a_channels = 6u16;

        let correct = slice_window(
            &b_samples,
            b_channels,
            48_000,
            None,
            FrameRange { start: 2, end: 5 },
        );
        assert_eq!(correct.frames(), 3);
        assert_eq!(correct.samples, vec![20.0, 21.0, 30.0, 31.0, 40.0, 41.0]);

        // Cutting the same buffer at A's count reads a different, wrong span of audio — and the
        // result is still a structurally valid WAV, which is exactly why this needs a test.
        let wrong = slice_window(
            &b_samples,
            a_channels,
            48_000,
            None,
            FrameRange { start: 2, end: 5 },
        );
        assert_ne!(wrong.samples, correct.samples);
        assert_eq!(wrong.samples.len() % wrong.channels as usize, 0);
    }

    #[test]
    fn slice_window_drops_whole_decode_metadata() {
        let samples = ramp(10, 2);
        let pcm = slice_window(
            &samples,
            2,
            48_000,
            Some(BitDepth::Int16),
            FrameRange { start: 0, end: 5 },
        );
        // Carrying these from the full decode would make the window claim a bitrate measured over
        // a completely different duration.
        assert!(pcm.compressed_bytes.is_none());
        assert!(pcm.decoded_frame_count.is_none());
        assert!(pcm.measured_bitrate_bps().is_none());
        // ...but the bit depth drives the output sample format and must survive.
        assert_eq!(pcm.source_bit_depth, Some(BitDepth::Int16));
    }

    /// The no-op-splice detector's whole basis: an unchanged window must digest equal, and a window
    /// changed anywhere — including one sample deep inside a long gap — must not.
    #[test]
    fn window_digest_detects_a_single_changed_sample() {
        let before = ramp(1000, 2);
        assert_eq!(window_digest(&before), window_digest(&before.clone()));

        let mut after = before.clone();
        after[1234] += 1.0;
        assert_ne!(window_digest(&before), window_digest(&after));
    }

    /// Length is folded in, so a prefix is not the same window as the whole.
    #[test]
    fn window_digest_separates_a_window_from_its_prefix() {
        let full = ramp(64, 2);
        assert_ne!(window_digest(&full), window_digest(&full[..full.len() - 2]));
        assert_ne!(window_digest(&[]), window_digest(&[0.0f32]));
    }

    /// `-0.0 == 0.0` numerically but they are different bit patterns. Bits are the right basis:
    /// a splice that rewrote a silent window as negative zero *did* touch the audio.
    #[test]
    fn window_digest_is_bitwise_not_numeric() {
        assert_ne!(window_digest(&[0.0f32]), window_digest(&[-0.0f32]));
    }

    #[test]
    fn slice_window_clamps_a_range_past_the_buffer() {
        let samples = ramp(4, 2);
        let pcm = slice_window(&samples, 2, 48_000, None, FrameRange { start: 2, end: 99 });
        assert_eq!(pcm.frames(), 2);
        assert_eq!(pcm.samples, vec![20.0, 21.0, 30.0, 31.0]);
    }
}
