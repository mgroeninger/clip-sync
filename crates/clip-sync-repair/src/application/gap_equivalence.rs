//! Application-side gap-equivalence: compute A's gap-interior RMS from decoded PCM, then classify via the
//! domain gate ([`crate::domain::gap_equivalence`]). The noise floor (A `levels.noise_floor_db`) and donor
//! silence (`donor_interior_nominal`) come from the fingerprint's **existing** measurements — no new decode,
//! no seam/residual math.
//!
//! **This front-end is diagnostic, not authoritative.** Production drops gaps on the *scan* verdict
//! ([`crate::domain::gap_equivalence::derive_gap_equivalence`]); this one only lands in the fingerprint
//! dump for calibration. The two share `classify_gap_equivalence` but differ in what they feed it — A RMS
//! window and filter, noise-floor context, donor window and predicate, and the definition of
//! `gap_floor_db`. Both known differences push this side toward `drop`, so a divergence is **not**
//! evidence the scan gate is inaccurate. `docs/dev/gap-fingerprint.md`
//! § *`equivalence` vs `scan_equivalence`*.

use crate::domain::gap_equivalence::{
    aggregate_rms_db, classify_gap_equivalence, GapEquivalenceParams, GapEquivalenceVerdict,
    SilentCoreProbe,
};
use crate::domain::pcm::interleaved_to_mono;
use crate::domain::policies::is_silent_interleaved;

/// dBFS a fully-silent span floors to — kept identical to the fingerprint's `to_db` / `LevelProfile`
/// convention so `a_gap_rms_db` is on the **same scale** as the `noise_floor_db` it's compared against.
/// (Using a lower floor here, e.g. −180 dB, would spuriously read a silent gap as far below a silent-context
/// noise floor and misclassify it as a dropout — see the domain gate's `a_below_noise_db`.)
const SILENCE_FLOOR_DB: f64 = -120.0;

/// Mono RMS of an interleaved A span, in dBFS (finite; a silent span floors at [`SILENCE_FLOOR_DB`], not −∞).
pub fn gap_interior_rms_db(
    samples: &[f32],
    channels: usize,
    start_frame: usize,
    end_frame: usize,
) -> Option<f64> {
    let ch = channels.max(1);
    let end = end_frame.min(samples.len() / ch);
    if start_frame >= end {
        return None;
    }
    let mono = interleaved_to_mono(&samples[start_frame * ch..end * ch], ch);
    if mono.is_empty() {
        return None;
    }
    let rms = (mono.iter().map(|v| v * v).sum::<f64>() / mono.len() as f64).sqrt();
    if rms <= 1e-9 {
        Some(SILENCE_FLOOR_DB)
    } else {
        Some(20.0 * rms.log10())
    }
}

/// Measure a candidate **silent-core** floor over A's gap interior at `bin_ms`, the scan path's way:
/// bin the span, keep only bins the *scanner's own predicate* ([`is_silent_interleaved`]) calls silent,
/// and report the max (candidate `gap_floor_db`) and energy mean (candidate `a_gap_rms_db`) over them.
///
/// **Provenance only** — see [`SilentCoreProbe`]. The empty-silent-set case returns `None` for both
/// statistics rather than a floored value, mirroring the scan path's `NEG_INFINITY` fold; whether that
/// fallback is load-bearing is one of the things this probe exists to find out.
///
/// A trailing partial bin is included (it is a real part of the gap); a zero-length span yields a probe
/// with `total_bins == 0`.
pub fn silent_core_probe(
    samples: &[f32],
    channels: usize,
    gap_frames: std::ops::Range<usize>,
    sample_rate: u32,
    bin_ms: u64,
    silence_peak_fraction: f32,
    absolute_silence_rms: f32,
) -> SilentCoreProbe {
    let ch = channels.max(1);
    let bin_frames = ((f64::from(sample_rate) * bin_ms as f64) / 1000.0).round() as usize;
    let bin_frames = bin_frames.max(1);
    let end = gap_frames.end.min(samples.len() / ch);
    let mut silent_levels: Vec<f64> = Vec::new();
    let mut total_bins = 0usize;
    let mut frame = gap_frames.start;
    while frame < end {
        let bin_end = (frame + bin_frames).min(end);
        total_bins += 1;
        let block = &samples[frame * ch..bin_end * ch];
        if is_silent_interleaved(block, ch, silence_peak_fraction, absolute_silence_rms) {
            if let Some(db) = gap_interior_rms_db(samples, ch, frame, bin_end) {
                silent_levels.push(db);
            }
        }
        frame = bin_end;
    }
    let floor = silent_levels
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    SilentCoreProbe {
        bin_ms,
        floor_db: floor.is_finite().then_some(floor),
        a_rms_db: aggregate_rms_db(silent_levels.iter().copied()),
        silent_bins: silent_levels.len(),
        total_bins,
    }
}

/// Compute the equivalence verdict for one gap: A's gap-interior RMS (from decoded A) + the recording's noise
/// floor + the donor silence at the nominal span → the domain classification.
pub fn measure_gap_equivalence(
    a_samples: &[f32],
    channels: usize,
    refined_start_frame: usize,
    refined_end_frame: usize,
    noise_floor_db: f64,
    donor_silence_fraction: Option<f64>,
    params: &GapEquivalenceParams,
) -> GapEquivalenceVerdict {
    let a_rms = gap_interior_rms_db(a_samples, channels, refined_start_frame, refined_end_frame);
    classify_gap_equivalence(a_rms, Some(noise_floor_db), donor_silence_fraction, params)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::gap_equivalence::GapEquivalenceClass;

    fn on() -> GapEquivalenceParams {
        GapEquivalenceParams {
            enabled: true,
            ..Default::default()
        }
    }

    /// A silent gap (dig-silence) with an occupied donor → RepairableDropout (end-to-end from PCM).
    #[test]
    fn silent_gap_occupied_donor_is_repairable() {
        let a = vec![0.0f32; 48_000]; // A gap interior = digital silence
        let v = measure_gap_equivalence(&a, 1, 0, 48_000, -50.0, Some(0.0), &on());
        assert_eq!(v.class, GapEquivalenceClass::RepairableDropout);
        // Digital silence floors at SILENCE_FLOOR_DB (−120), the same scale as noise_floor_db.
        assert_eq!(
            v.a_gap_rms_db.unwrap(),
            -120.0,
            "silent span floors at −120: {v:?}"
        );
    }

    /// Room-tone A (a few dB below the noise floor) with a silent donor → SharedSilence.
    #[test]
    fn roomtone_gap_silent_donor_is_shared_silence() {
        // amplitude ~1e-4 ⇒ ~−80 dBFS; noise floor −75 ⇒ only ~5 dB below ⇒ not a dropout; donor silent.
        let a = vec![1e-4f32; 48_000];
        let v = measure_gap_equivalence(&a, 1, 0, 48_000, -75.0, Some(0.9), &on());
        assert_eq!(v.class, GapEquivalenceClass::SharedSilence);
        assert!(v.drop);
    }

    // --- silent-core probe (F15 measurement scaffolding; provenance only) --------------------------

    /// Digital silence: every bin passes the predicate, so the candidate floor is the −120 floor and
    /// the silent-bin population is the whole span.
    #[test]
    fn silent_core_probe_counts_every_bin_of_a_silent_gap() {
        let a = vec![0.0f32; 48_000]; // 1 s @ 48 kHz
        let p = silent_core_probe(&a, 1, 0..48_000, 48_000, 50, 0.01, 0.001);
        assert_eq!((p.bin_ms, p.silent_bins, p.total_bins), (50, 20, 20));
        assert_eq!(p.floor_db, Some(-120.0));
        assert_eq!(p.a_rms_db, Some(-120.0));
    }

    /// The point of the probe: a loud bin inside the span is **excluded** from the floor, so the
    /// candidate floor tracks the quiet core rather than the span's content peak (which is exactly the
    /// difference that flips a class — F15's band mechanism).
    #[test]
    fn silent_core_probe_excludes_loud_bins_from_the_floor() {
        let mut a = vec![0.0f32; 48_000];
        // Bin 10 ([0.5 s, 0.55 s)) is loud: full-scale, so neither predicate branch calls it silent.
        for v in &mut a[24_000..26_400] {
            *v = 0.5;
        }
        let p = silent_core_probe(&a, 1, 0..48_000, 48_000, 50, 0.01, 0.001);
        assert_eq!((p.silent_bins, p.total_bins), (19, 20));
        assert_eq!(
            p.floor_db,
            Some(-120.0),
            "the −6 dBFS bin must not become the floor: {p:?}"
        );
    }

    /// No silent bin ⇒ both statistics absent, mirroring the scan path's `NEG_INFINITY` fold → `None`
    /// (which classifies `NotEvaluated` ⇒ keep). Whether this case occurs in the corpus is one of the
    /// things the probe is being dumped to find out.
    #[test]
    fn silent_core_probe_with_no_silent_bins_reports_none() {
        let a: Vec<f32> = (0..48_000).map(|i| (i as f32 * 0.3).sin() * 0.5).collect();
        let p = silent_core_probe(&a, 1, 0..48_000, 48_000, 50, 0.01, 0.001);
        assert_eq!(p.silent_bins, 0);
        assert_eq!(p.total_bins, 20);
        assert_eq!(p.floor_db, None);
        assert_eq!(p.a_rms_db, None);
    }

    /// A zero-length span bins to nothing rather than panicking or fabricating a floor.
    #[test]
    fn silent_core_probe_on_empty_span_is_empty() {
        let a = vec![0.0f32; 100];
        let p = silent_core_probe(&a, 1, 50..50, 48_000, 50, 0.01, 0.001);
        assert_eq!((p.silent_bins, p.total_bins), (0, 0));
        assert_eq!(p.floor_db, None);
    }

    /// A trailing partial bin is still a real part of the gap and is measured, not dropped.
    #[test]
    fn silent_core_probe_includes_a_trailing_partial_bin() {
        let a = vec![0.0f32; 3_600]; // 75 ms @ 48 kHz = one full 50 ms bin + a 25 ms remainder
        let p = silent_core_probe(&a, 1, 0..3_600, 48_000, 50, 0.01, 0.001);
        assert_eq!((p.silent_bins, p.total_bins), (2, 2));
    }

    /// The probe never touches the verdict's disposition — it is provenance, attached after the fact.
    #[test]
    fn attaching_probes_leaves_the_class_untouched() {
        let a = vec![0.0f32; 48_000];
        let v = measure_gap_equivalence(&a, 1, 0, 48_000, -50.0, Some(0.0), &on());
        let p = silent_core_probe(&a, 1, 0..48_000, 48_000, 50, 0.01, 0.001);
        let with = v.clone().with_silent_core_probes(vec![p]);
        assert_eq!((with.class, with.drop), (v.class, v.drop));
        assert_eq!(with.silent_core_probes.len(), 1);
    }

    #[test]
    fn empty_span_yields_none_rms_and_not_evaluated() {
        let a = vec![0.0f32; 100];
        let v = measure_gap_equivalence(&a, 1, 50, 50, -50.0, Some(0.0), &on());
        assert_eq!(v.class, GapEquivalenceClass::NotEvaluated);
    }
}
