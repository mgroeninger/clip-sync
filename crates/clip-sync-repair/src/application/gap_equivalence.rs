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
    classify_gap_equivalence, GapEquivalenceParams, GapEquivalenceVerdict,
};
use crate::domain::pcm::interleaved_to_mono;

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

    #[test]
    fn empty_span_yields_none_rms_and_not_evaluated() {
        let a = vec![0.0f32; 100];
        let v = measure_gap_equivalence(&a, 1, 50, 50, -50.0, Some(0.0), &on());
        assert_eq!(v.class, GapEquivalenceClass::NotEvaluated);
    }
}
