//! A lossless A/B pair whose gap core is **bit-exact digital silence**, written to real containers.
//!
//! Two things this exists for, both of which need files on disk rather than in-memory PCM:
//!
//! 1. **Source-provenance smoke coverage.** `FileSource`'s `codec` / `bit_depth` /
//!    `native_sample_rate` / `native_channels` are filled from a container *probe*
//!    (`patch_audio::decode::SourceDescriptor`). Every unit test around them injects a
//!    `SourceDescriptor` by hand, so none of them can show that a real probe populates one. The two
//!    sides here are deliberately unlike each other — different sample rate, different bit depth — so
//!    a probe that reported one side's reading for both, or dropped a field, is visible.
//!
//! 2. **The −120 digital-silence condition.** `block_rms_db` clamps at
//!    `BLOCK_LEVEL_FLOOR_DB` (−120) instead of returning `-inf`, so a gap whose silent core is
//!    exactly zero makes `gap_floor_db` −120 as well, and `-120.0 < -120.0` is false — the I3 defect
//!    (`application/gap_equivalence.rs:287-296`), where a floor-only donor predicate reads digital
//!    silence as *occupied*. Lossy material cannot reach it: AAC/MP3/Vorbis reconstruct silence
//!    through quantization and the IMDCT and bottom out near −101 dB, which is why the whole measured
//!    corpus to date is structurally incapable of producing the condition. **Zeros survive the 44.1 →
//!    48 kHz resample**: the interior of a multi-second zero run stays exactly `0.0` because every
//!    filter tap is multiplied by zero. Only the first/last few milliseconds pull in neighbouring
//!    audio, and the core is far longer than any filter tail.
//!
//! Both files are PCM WAV. That is the deliberate choice, not a shortcut — of the codecs this tree
//! can decode (`symphonia` is configured `default, isomp4, aac, mp3, mkv, flac`), only FLAC and PCM
//! reproduce zeros exactly, and PCM is the one we can write with `hound` alone, with no encoder
//! dependency and nothing to skip when a tool is missing.
//!
//! **What this does not cover.** Both sides report the same codec family, `"pcm"` — that is correct
//! (depth lives on `bit_depth`, not the codec axis) but it means the fixture cannot exercise a
//! mixed-codec pair, and it cannot assert `codec == "flac"` or any other named arm. Those mappings
//! are pinned by `codec_name`'s own unit tests. Nor is this a substitute for a dump over real
//! licensed media: it proves the probe→`FileSource` wiring against a genuine container, not that the
//! reading is sensible across the codec variety real media actually carries.

use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavSpec, WavWriter};

/// A's rate; B is resampled to this on decode, so A reads "not resampled" and B reads "resampled".
pub const RATE_A: u32 = 48_000;
/// B's rate. Chosen to differ from [`RATE_A`] by a non-integer ratio so the resample is real.
pub const RATE_B: u32 = 44_100;
/// A's sample format → `bit_depth: "s16"`.
pub const BITS_A: u16 = 16;
/// B's sample format → `bit_depth: "s24"`. Differs from A so a per-side mix-up is visible.
pub const BITS_B: u16 = 24;

/// Total duration. Long enough for alignment to have material either side of the gap.
pub const TOTAL_SECS: u32 = 120;
/// Start of the digitally silent core, in seconds. Shared by both sides.
pub const SILENT_START_SECS: u32 = 30;
/// End of the digitally silent core, in seconds. 30 s is far longer than any resampler filter tail,
/// so the interior is exactly zero on both sides after B is resampled to [`RATE_A`].
pub const SILENT_END_SECS: u32 = 60;

/// Where the files landed and what each side was written as, so a test asserts against the values
/// the fixture actually wrote rather than restating them.
#[derive(Debug, Clone)]
pub struct LosslessSilencePair {
    pub path_a: PathBuf,
    pub path_b: PathBuf,
    pub rate_a: u32,
    pub rate_b: u32,
    pub bits_a: u16,
    pub bits_b: u16,
    pub channels: u16,
    pub silent_start_secs: f64,
    pub silent_end_secs: f64,
}

/// Rising chirp, amplitude in [-0.5, 0.5]. Time-domain identical at any rate: `t` is wall clock, so
/// the two sides carry the *same signal* sampled differently — which is what makes them alignable.
fn chirp(sample_rate: u32, index: u64) -> f64 {
    let t = index as f64 / f64::from(sample_rate);
    let freq = 300.0 + 400.0 * t;
    (std::f64::consts::TAU * freq * t).sin() * 0.5
}

/// Write one mono WAV at `bits` bits per sample, zeroing `[SILENT_START_SECS, SILENT_END_SECS)`.
///
/// The silent region is written as literal `0`, not as an attenuated signal: the whole point is a
/// sample value that decodes to exactly `0.0` so the RMS clamps to −120 rather than landing just
/// above it.
fn write_mono(path: &Path, sample_rate: u32, bits: u16) {
    let spec = WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: bits,
        sample_format: SampleFormat::Int,
    };
    // hound writes `bits`-wide samples from an i32; full-scale is 2^(bits-1) - 1.
    let full_scale = f64::from((1i32 << (bits - 1)) - 1);
    let total = u64::from(sample_rate) * u64::from(TOTAL_SECS);
    let silent = u64::from(sample_rate) * u64::from(SILENT_START_SECS)
        ..u64::from(sample_rate) * u64::from(SILENT_END_SECS);

    let mut writer = WavWriter::create(path, spec).expect("create wav");
    for index in 0..total {
        let value = if silent.contains(&index) {
            0
        } else {
            (chirp(sample_rate, index) * full_scale).round() as i32
        };
        writer.write_sample(value).expect("write sample");
    }
    writer.finalize().expect("finalize wav");
}

/// Write the pair into `dir` and describe what was written.
///
/// The two sides share a timeline (no offset) and a channel count — equal `native_channels` is the
/// precondition for pairwise measurement, since `characterize_gaps*` refuses the pair outright when
/// layouts disagree and would emit no gaps at all.
pub fn write_lossless_silence_pair(dir: &Path) -> LosslessSilencePair {
    let path_a = dir.join("lossless_silence_a.wav");
    let path_b = dir.join("lossless_silence_b.wav");
    write_mono(&path_a, RATE_A, BITS_A);
    write_mono(&path_b, RATE_B, BITS_B);
    LosslessSilencePair {
        path_a,
        path_b,
        rate_a: RATE_A,
        rate_b: RATE_B,
        bits_a: BITS_A,
        bits_b: BITS_B,
        channels: 1,
        silent_start_secs: f64::from(SILENT_START_SECS),
        silent_end_secs: f64::from(SILENT_END_SECS),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hound::WavReader;

    #[test]
    fn silent_core_is_written_as_exact_zero_at_both_depths() {
        let temp = tempfile::tempdir().expect("tempdir");
        let pair = write_lossless_silence_pair(temp.path());

        for (path, rate, bits) in [
            (&pair.path_a, pair.rate_a, pair.bits_a),
            (&pair.path_b, pair.rate_b, pair.bits_b),
        ] {
            let mut reader = WavReader::open(path).expect("open wav");
            assert_eq!(reader.spec().bits_per_sample, bits);
            assert_eq!(reader.spec().sample_rate, rate);
            let samples: Vec<i32> = reader
                .samples::<i32>()
                .map(|s| s.expect("read sample"))
                .collect();

            let start = (rate * SILENT_START_SECS) as usize;
            let end = (rate * SILENT_END_SECS) as usize;
            assert!(
                samples[start..end].iter().all(|&s| s == 0),
                "silent core must be literal zero, not merely quiet — a non-zero sample here \
                 lifts the block RMS off the −120 clamp and the fixture stops testing the \
                 condition it exists for"
            );
            assert!(
                samples[..start].iter().any(|&s| s != 0) && samples[end..].iter().any(|&s| s != 0),
                "both shoulders must carry signal, or there is no gap to detect"
            );
        }
    }

    #[test]
    fn sides_differ_in_both_rate_and_depth() {
        // If these ever coincide, the fixture stops being able to catch a probe that reports one
        // side's reading for both — which is the per-side mix-up it is here to detect.
        assert_ne!(RATE_A, RATE_B);
        assert_ne!(BITS_A, BITS_B);
    }
}
