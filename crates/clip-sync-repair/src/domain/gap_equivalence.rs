//! Gap content-equivalence gate — "does this gap actually need patching?" (`docs/TEMP-gap-equivalence-plan.md`).
//!
//! **Silence-character classification.** A scanned silent run in A is worth repairing only when A's signal
//! genuinely *died* (a dropout) **and** B carries the missing content. The two signals that decide it, both
//! already in the fingerprint:
//!
//! - **A-side (`a_rms` vs the recording's `noise_floor_db`):** a true dropout sits **far below** A's own noise
//!   floor (the signal is gone); a genuine quiet passage sits **at** the noise floor (room tone). Measuring
//!   *relative to the noise floor* makes the threshold **self-calibrating** — no hard-coded absolute dB.
//! - **B-side (`donor_silence_fraction`):** if B is silent at the nominal span there is nothing to fill with.
//!
//! Empirically (licensed media): the two silence signals separate cleanly (dropouts ≥35 dB below noise floor,
//! `donor_silence` bimodal at ~0 vs ~1) where the seam/lag approach failed — the recordings drift, so "B matches
//! A" is never a lag-0 match. This gate replaces that approach.

use serde::{Deserialize, Serialize};

/// Tunable thresholds for the equivalence gate (all overridable; gate is **off by default**).
#[derive(Debug, Clone, Copy)]
pub struct GapEquivalenceParams {
    /// Master on/off. When `false`, every gap classifies `NotEvaluated` (keep) — zero behavior change.
    pub enabled: bool,
    /// A counts as a **dropout** when `a_rms_db < noise_floor_db − dropout_margin_db` (default `35.0`).
    /// Relative to the recording's own noise floor, so it self-calibrates across noisy/clean sources.
    pub dropout_margin_db: f64,
    /// B counts as **occupied** when `donor_silence_fraction < donor_silence_thresh` (default `0.5`, the
    /// program-quiet valley); at/above ⇒ B silent ⇒ nothing to fill.
    pub donor_silence_thresh: f64,
}

impl Default for GapEquivalenceParams {
    fn default() -> Self {
        Self { enabled: false, dropout_margin_db: 35.0, donor_silence_thresh: 0.5 }
    }
}

/// Vocabulary for the gate — the reason a gap does or doesn't need patching.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GapEquivalenceClass {
    /// A's signal died (RMS ≥ `dropout_margin_db` below the recording's noise floor) **and** B carries content
    /// — a real dropout with a fill source. **Keep** (needs patching).
    RepairableDropout,
    /// B is silent at the nominal span (`donor_silence ≥ thresh`) — nothing to fill with, patching can't help.
    /// **Drop.** (Both "A dropped out but the donor is also dead" and "quiet in both" land here.)
    SharedSilence,
    /// A is only ambient room tone (near its own noise floor), not a signal failure, though B has content — a
    /// genuine quiet passage, not a dropout. **Drop** (don't inject content into intentional quiet).
    AmbientQuiet,
    /// Gate disabled or a required signal missing — **keep** (no decision made).
    NotEvaluated,
}

impl GapEquivalenceClass {
    /// Whether this gap should be dropped from the fill plan (no patching needed).
    pub fn drops(self) -> bool {
        matches!(self, Self::SharedSilence | Self::AmbientQuiet)
    }
}

/// The gate's per-gap readout: the class + the signals it was derived from (for tuning + reporting).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GapEquivalenceVerdict {
    pub class: GapEquivalenceClass,
    /// `class.drops()` — surfaced so consumers don't re-derive it.
    pub drop: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub a_gap_rms_db: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub noise_floor_db: Option<f64>,
    /// `a_gap_rms_db − noise_floor_db` — how far below the noise floor A's gap sits (the self-calibrated signal).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub a_below_noise_db: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub donor_silence_fraction: Option<f64>,
}

impl GapEquivalenceVerdict {
    fn of(class: GapEquivalenceClass, a: Option<f64>, nf: Option<f64>, ds: Option<f64>) -> Self {
        Self {
            class,
            drop: class.drops(),
            a_gap_rms_db: a,
            noise_floor_db: nf,
            a_below_noise_db: match (a, nf) {
                (Some(a), Some(nf)) => Some(a - nf),
                _ => None,
            },
            donor_silence_fraction: ds,
        }
    }
}

/// Classify one gap from its silence signals. Pure — no I/O, no measurement.
///
/// - `NotEvaluated` when the gate is off or any signal is missing.
/// - `SharedSilence` when B is silent (nothing to fill).
/// - `RepairableDropout` when A's signal died (below the noise floor by the margin) and B is occupied.
/// - `AmbientQuiet` when B is occupied but A is only room tone (not a dropout).
pub fn classify_gap_equivalence(
    a_gap_rms_db: Option<f64>,
    noise_floor_db: Option<f64>,
    donor_silence_fraction: Option<f64>,
    params: &GapEquivalenceParams,
) -> GapEquivalenceVerdict {
    if !params.enabled {
        return GapEquivalenceVerdict::of(GapEquivalenceClass::NotEvaluated, a_gap_rms_db, noise_floor_db, donor_silence_fraction);
    }
    let (Some(a), Some(nf), Some(ds)) = (a_gap_rms_db, noise_floor_db, donor_silence_fraction) else {
        return GapEquivalenceVerdict::of(GapEquivalenceClass::NotEvaluated, a_gap_rms_db, noise_floor_db, donor_silence_fraction);
    };
    let is_dropout = a < nf - params.dropout_margin_db;
    let b_occupied = ds < params.donor_silence_thresh;
    let class = match (is_dropout, b_occupied) {
        (true, true) => GapEquivalenceClass::RepairableDropout,
        (_, false) => GapEquivalenceClass::SharedSilence,
        (false, true) => GapEquivalenceClass::AmbientQuiet,
    };
    GapEquivalenceVerdict::of(class, a_gap_rms_db, noise_floor_db, donor_silence_fraction)
}

#[cfg(test)]
mod tests {
    use super::*;
    use GapEquivalenceClass::*;

    fn on() -> GapEquivalenceParams {
        GapEquivalenceParams { enabled: true, ..Default::default() }
    }

    fn class(a: f64, nf: f64, ds: f64) -> GapEquivalenceClass {
        classify_gap_equivalence(Some(a), Some(nf), Some(ds), &on()).class
    }

    /// The four measured licensed-media cases (noise floor ~−45 to −70; margin 35, donor thresh 0.5).
    #[test]
    fn measured_cases_classify_as_ground_truth() {
        // Repairable dropout: a_rms −106, noise_floor −47 ⇒ 59 dB below; donor 0.0.
        assert_eq!(class(-106.0, -47.0, 0.0), RepairableDropout);
        // Mutual silence: a_rms −81, noise_floor −71 ⇒ 10 dB below (not a dropout); donor 0.92 (B silent).
        assert_eq!(class(-81.0, -71.0, 0.92), SharedSilence);
        // Deep A but B dead (intro/tail): a_rms −108, noise_floor −46 ⇒ dropout, but donor 1.0 ⇒ nothing to fill.
        assert_eq!(class(-108.0, -46.0, 1.0), SharedSilence);
    }

    /// Ambient A with an occupied donor is a genuine quiet passage → drop (not a dropout).
    #[test]
    fn ambient_with_occupied_donor_is_quiet_passage() {
        assert_eq!(class(-80.0, -70.0, 0.0), AmbientQuiet); // only 10 dB below floor
        assert!(AmbientQuiet.drops());
    }

    /// The margin is self-calibrating: the same 40 dB drop is a dropout under both a low and a high noise floor.
    #[test]
    fn margin_is_relative_to_noise_floor() {
        assert_eq!(class(-100.0, -60.0, 0.0), RepairableDropout); // 40 dB below a −60 floor
        assert_eq!(class(-120.0, -80.0, 0.0), RepairableDropout); // 40 dB below a −80 floor
        assert_eq!(class(-90.0, -60.0, 0.0), AmbientQuiet); // only 30 dB below ⇒ not a dropout
    }

    #[test]
    fn drops_only_the_two_silence_classes() {
        assert!(!RepairableDropout.drops());
        assert!(!NotEvaluated.drops());
        assert!(SharedSilence.drops());
        assert!(AmbientQuiet.drops());
    }

    #[test]
    fn disabled_or_missing_signal_is_not_evaluated() {
        assert_eq!(classify_gap_equivalence(Some(-106.0), Some(-47.0), Some(0.0), &GapEquivalenceParams::default()).class, NotEvaluated);
        assert_eq!(classify_gap_equivalence(None, Some(-47.0), Some(0.0), &on()).class, NotEvaluated);
        assert_eq!(classify_gap_equivalence(Some(-106.0), Some(-47.0), None, &on()).class, NotEvaluated);
    }

    #[test]
    fn verdict_reports_a_below_noise() {
        let v = classify_gap_equivalence(Some(-106.0), Some(-47.0), Some(0.0), &on());
        assert_eq!(v.a_below_noise_db, Some(-59.0));
        assert!(v.drop == false && v.class == RepairableDropout);
    }
}
