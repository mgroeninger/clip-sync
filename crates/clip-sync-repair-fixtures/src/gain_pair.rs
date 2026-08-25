//! A lossless A/B pair differing **only** by a known gain — the known-answer fixture for the
//! whole-program level offset (`domain::program_levels`).
//!
//! Why a separate writer rather than a knob on [`crate::lossless_silence_pair`]: that fixture
//! deliberately differs in sample rate (48k/44.1k) *and* bit depth (s16/s24) between the sides,
//! which is precisely the confound this one must not carry. A pair that differs in gain *and*
//! resampling cannot tell a gain-estimator bug from a resampler artifact. Here rate, depth, channel
//! count and the generating signal are all held equal, so the applied gain is the only difference
//! and `offset_db` has exactly one thing it can be reporting.
//!
//! What this fixture is for: the wiring, end to end — decode → `SilenceRunScanner` block levels →
//! `paired_level_offset`. The estimator's own unit tests feed `BlockLevel` values in directly, so a
//! flipped sign, a threshold compared in the wrong domain or a mapping off by a bucket passes every
//! one of them. It is **not** a test of estimator behaviour on real material: the chirp is constant
//! amplitude, so the steadiness filter has nothing to reject, and the timeline is far shorter than
//! one chunk, so chunking never engages. Both belong to the media tiers of the plan.
//!
//! Lossless throughout. A lossy codec would put quantisation noise into `spread_db` with no way to
//! separate it from estimator error.

use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavSpec, WavWriter};

/// Shared by both sides — see the module note on why nothing but the gain may differ.
pub const RATE: u32 = 48_000;
/// Shared by both sides.
pub const BITS: u16 = 16;

/// Total program length per side, before any head pad.
pub const TOTAL_SECS: u32 = 120;
/// Start of the digitally silent core, shared by both sides.
pub const SILENT_START_SECS: u32 = 30;
/// End of the digitally silent core. The core is written as literal `0`, and zero times any gain is
/// zero, so it stays exactly silent on both sides at any gain and falls below the active threshold
/// on its own.
pub const SILENT_END_SECS: u32 = 60;

/// Peak amplitude of the generated chirp, which fixes the headroom: a gain above
/// `20*log10(1/CHIRP_AMPLITUDE)`, about +6.02 dB, clips. Clipping *lowers* measured RMS gain and
/// *raises* spread, so it reads exactly like estimator bias while the fixture is what is wrong —
/// [`write_gain_pair`] refuses rather than let that happen silently.
pub const CHIRP_AMPLITUDE: f64 = 0.5;

/// What was written, so a test asserts against the fixture's own values rather than restating them.
#[derive(Debug, Clone)]
pub struct GainPair {
    pub path_a: PathBuf,
    pub path_b: PathBuf,
    /// Gain applied to B, in dB. The estimator should recover this as `offset_db`, since the
    /// reported offset is B minus A.
    pub gain_db: f64,
    /// Silence prepended to B, in seconds. B's content is therefore later than A's by this much, so
    /// a successful alignment reports `+pad_secs`.
    pub pad_secs: f64,
    pub rate: u32,
    pub bits: u16,
    pub channels: u16,
    pub silent_start_secs: f64,
    pub silent_end_secs: f64,
}

/// Rising chirp at [`CHIRP_AMPLITUDE`]. Sharply autocorrelating, which is what makes the pair
/// alignable — and alignment is a precondition, since B's level timeline is scanned only when an
/// alignment offset exists.
fn chirp(index: u64) -> f64 {
    let t = index as f64 / f64::from(RATE);
    let freq = 300.0 + 400.0 * t;
    (std::f64::consts::TAU * freq * t).sin() * CHIRP_AMPLITUDE
}

/// Write one mono WAV: `pad_samples` of leading digital silence, then the chirp scaled by `gain`,
/// with `[SILENT_START_SECS, SILENT_END_SECS)` of the *content* zeroed.
fn write_side(path: &Path, gain: f64, pad_samples: u64) {
    let spec = WavSpec {
        channels: 1,
        sample_rate: RATE,
        bits_per_sample: BITS,
        sample_format: SampleFormat::Int,
    };
    // hound writes `BITS`-wide samples from an i32; full-scale is 2^(BITS-1) - 1.
    let full_scale = f64::from((1i32 << (BITS - 1)) - 1);
    let total = u64::from(RATE) * u64::from(TOTAL_SECS);
    let silent = u64::from(RATE) * u64::from(SILENT_START_SECS)
        ..u64::from(RATE) * u64::from(SILENT_END_SECS);

    let mut writer = WavWriter::create(path, spec).expect("create wav");
    for _ in 0..pad_samples {
        writer.write_sample(0i32).expect("write pad sample");
    }
    for index in 0..total {
        let value = if silent.contains(&index) {
            0
        } else {
            (chirp(index) * gain * full_scale).round() as i32
        };
        writer.write_sample(value).expect("write sample");
    }
    writer.finalize().expect("finalize wav");
}

/// Write A and a `gain_db`-scaled B into `dir`, with `pad_samples` of silence prepended to B.
///
/// `pad_samples` is in **samples**, not seconds, so a caller can place B's content at a deliberately
/// sub-block offset. That matters: a pad that is a whole multiple of the scan block leaves the two
/// timelines block-aligned, and the largest-overlap mapping is never asked to choose between two
/// candidate B blocks. A fractional pad is the only version of this fixture that exercises it.
///
/// # Panics
/// If `gain_db` would clip — see [`CHIRP_AMPLITUDE`].
pub fn write_gain_pair(dir: &Path, gain_db: f64, pad_samples: u64) -> GainPair {
    let gain = 10f64.powf(gain_db / 20.0);
    assert!(
        gain * CHIRP_AMPLITUDE <= 1.0,
        "gain_db {gain_db} clips a chirp of amplitude {CHIRP_AMPLITUDE}; \
         clipping reads as estimator bias, so the fixture refuses it"
    );

    let path_a = dir.join("gain_pair_a.wav");
    let path_b = dir.join("gain_pair_b.wav");
    write_side(&path_a, 1.0, 0);
    write_side(&path_b, gain, pad_samples);
    GainPair {
        path_a,
        path_b,
        gain_db,
        pad_secs: pad_samples as f64 / f64::from(RATE),
        rate: RATE,
        bits: BITS,
        channels: 1,
        silent_start_secs: f64::from(SILENT_START_SECS),
        silent_end_secs: f64::from(SILENT_END_SECS),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hound::WavReader;

    fn samples(path: &Path) -> Vec<i32> {
        WavReader::open(path)
            .expect("open wav")
            .into_samples::<i32>()
            .map(|s| s.expect("sample"))
            .collect()
    }

    #[test]
    fn b_is_a_scaled_by_the_requested_gain() {
        let temp = tempfile::tempdir().expect("tempdir");
        let pair = write_gain_pair(temp.path(), -6.0, 0);

        let (a, b) = (samples(&pair.path_a), samples(&pair.path_b));
        assert_eq!(
            a.len(),
            b.len(),
            "no pad requested, so the sides are equal length"
        );

        // Compare in energy, not sample by sample: rounding to s16 makes a per-sample ratio
        // meaningless near the zero crossings.
        let energy = |s: &[i32]| s.iter().map(|&v| f64::from(v) * f64::from(v)).sum::<f64>();
        let measured_db = 10.0 * (energy(&b) / energy(&a)).log10();
        assert!(
            (measured_db - pair.gain_db).abs() < 0.01,
            "wrote {measured_db} dB, asked for {}",
            pair.gain_db
        );
    }

    #[test]
    fn the_silent_core_stays_exactly_zero_under_gain() {
        let temp = tempfile::tempdir().expect("tempdir");
        let pair = write_gain_pair(temp.path(), -6.0, 0);

        let b = samples(&pair.path_b);
        let core = (RATE as usize) * (SILENT_START_SECS as usize)
            ..(RATE as usize) * (SILENT_END_SECS as usize);
        assert!(
            b[core].iter().all(|&v| v == 0),
            "the core must decode to exact zero so it reads as digital silence, not a quiet floor"
        );
    }

    #[test]
    fn the_pad_lands_ahead_of_b_content() {
        let temp = tempfile::tempdir().expect("tempdir");
        let pad_samples = 1_440;
        let pair = write_gain_pair(temp.path(), -6.0, pad_samples);

        let b = samples(&pair.path_b);
        assert_eq!(pair.pad_secs, 0.03);
        assert!(b[..pad_samples as usize].iter().all(|&v| v == 0));
        assert_eq!(
            b.len(),
            samples(&pair.path_a).len() + pad_samples as usize,
            "the pad lengthens B rather than displacing its content"
        );
    }

    #[test]
    #[should_panic(expected = "clips a chirp")]
    fn a_clipping_gain_is_refused() {
        let temp = tempfile::tempdir().expect("tempdir");
        write_gain_pair(temp.path(), 12.0, 0);
    }
}
