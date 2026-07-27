//! Gap content-equivalence gate — "does this gap actually need patching?" (`docs/dev/gap-vocabulary.md` § Silence-character pre-gate).
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

use crate::domain::policies::{BlockLevel, BLOCK_LEVEL_FLOOR_DB};

/// Context window (seconds) each side of a gap used to estimate A's local noise floor from the scan's
/// per-block level timeline — blocks in `[a_start − ctx, a_end + ctx]` but **outside** the gap. The
/// scan-time analogue of the fingerprint's `gap_signature_context_secs`.
pub const EQUIVALENCE_CONTEXT_SECS: f64 = 2.0;

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
        Self {
            enabled: false,
            dropout_margin_db: 35.0,
            donor_silence_thresh: 0.5,
        }
    }
}

/// Vocabulary for the gate — the reason a gap does or doesn't need patching. These are the
/// **scan-time silence-character cells** in [`docs/dev/gap-vocabulary.md`] (§ *Silence-character pre-gate*),
/// a pre-filter that runs before the seam/donor cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GapEquivalenceClass {
    /// A's signal died (RMS ≥ `dropout_margin_db` below the recording's noise floor) **and** B carries content
    /// — a real dropout with a fill source. **Keep** — *not a skip cell*: the gap proceeds into the normal
    /// seam/donor cells (Bracket-patch / Silence-splice / …).
    RepairableDropout,
    /// B is silent at the nominal span (`donor_silence ≥ thresh`) — nothing to fill with, patching can't help.
    /// **Drop.** (Both "A dropped out but the donor is also dead" and "quiet in both" land here.) This is the
    /// **plan-time detection of the Program-quiet cell** — the same disposition the patch path skips as
    /// `GapPatchSkipReason::ProgramQuiet`, surfaced before decode as `GapFillSkipReason::AlreadyMatchesReference`.
    SharedSilence,
    /// A is only ambient room tone (near its own noise floor), not a signal failure, though B has content — a
    /// genuine quiet passage, not a dropout. **Drop** (don't inject content into intentional quiet). A cell with
    /// no seam/donor counterpart — decided on A's own character, not B's donor state.
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
        return GapEquivalenceVerdict::of(
            GapEquivalenceClass::NotEvaluated,
            a_gap_rms_db,
            noise_floor_db,
            donor_silence_fraction,
        );
    }
    let (Some(a), Some(nf), Some(ds)) = (a_gap_rms_db, noise_floor_db, donor_silence_fraction)
    else {
        return GapEquivalenceVerdict::of(
            GapEquivalenceClass::NotEvaluated,
            a_gap_rms_db,
            noise_floor_db,
            donor_silence_fraction,
        );
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

/// A block's timeline center (used for gap/context membership at 250 ms granularity).
fn block_center(b: &BlockLevel) -> f64 {
    (b.start_secs + b.end_secs) / 2.0
}

/// Combine per-block dB levels into one aggregate RMS in dB (energy mean of the blocks). `None` when empty.
fn aggregate_rms_db(levels: impl Iterator<Item = f64>) -> Option<f64> {
    let mut sum_sq = 0.0f64;
    let mut n = 0usize;
    for db in levels {
        let amp = 10f64.powf(db / 20.0);
        sum_sq += amp * amp;
        n += 1;
    }
    if n == 0 {
        return None;
    }
    let rms = (sum_sq / n as f64).sqrt();
    Some(if rms <= 1e-9 {
        BLOCK_LEVEL_FLOOR_DB
    } else {
        20.0 * rms.log10()
    })
}

/// Median of a set of dB values (used for A's local noise floor). `None` when empty.
fn median_db(mut vals: Vec<f64>) -> Option<f64> {
    if vals.is_empty() {
        return None;
    }
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Some(vals[vals.len() / 2])
}

/// Derive the per-gap silence-character signals from the scan's per-block level timelines and classify
/// (`docs/gap-scan.md`; vocabulary in `docs/dev/gap-vocabulary.md` § Silence-character pre-gate). All three signals are the scan-block
/// analogues of the fingerprint's finer-bin reads:
///
/// - **noise floor** — median dB of A blocks in `±`[`EQUIVALENCE_CONTEXT_SECS`] around the gap, **excluding**
///   blocks inside it (the recording's own floor; self-calibrating).
/// - **A gap RMS** — aggregate RMS of A's blocks inside the gap (the dropout-vs-room-tone A-side signal).
/// - **donor silence fraction** — fraction of B blocks over the nominal mapped span below the gap floor
///   (the loudest A gap block; the donor-occupancy B-side signal).
///
/// `b_levels`/`b_mapped` are `None` when B was not scanned (missing/unaligned) ⇒ donor signal absent ⇒
/// `NotEvaluated`. Pure — no I/O.
pub fn derive_gap_equivalence(
    a_levels: &[BlockLevel],
    a_start_secs: f64,
    a_end_secs: f64,
    b_levels: Option<&[BlockLevel]>,
    b_mapped: Option<(f64, f64)>,
    params: &GapEquivalenceParams,
) -> GapEquivalenceVerdict {
    // A gap blocks (center inside the gap) → aggregate RMS + gap floor (loudest block, donor reference).
    let gap_blocks = || {
        a_levels
            .iter()
            .filter(|b| block_center(b) >= a_start_secs && block_center(b) < a_end_secs)
    };
    let a_gap_rms_db = aggregate_rms_db(gap_blocks().map(|b| b.rms_db));
    let gap_floor_db = gap_blocks()
        .map(|b| b.rms_db)
        .fold(f64::NEG_INFINITY, f64::max);

    // A context blocks: within the context window but outside the gap → median = local noise floor.
    let ctx_lo = a_start_secs - EQUIVALENCE_CONTEXT_SECS;
    let ctx_hi = a_end_secs + EQUIVALENCE_CONTEXT_SECS;
    let noise_floor_db = median_db(
        a_levels
            .iter()
            .filter(|b| {
                let c = block_center(b);
                c >= ctx_lo && c < ctx_hi && !(c >= a_start_secs && c < a_end_secs)
            })
            .map(|b| b.rms_db)
            .collect(),
    );

    // Donor silence fraction: fraction of B blocks over the mapped span below the gap floor.
    let donor_silence_fraction = match (b_levels, b_mapped) {
        (Some(bl), Some((b_start, b_end))) if gap_floor_db.is_finite() => {
            let mut total = 0usize;
            let mut silent = 0usize;
            for b in bl.iter().filter(|b| {
                let c = block_center(b);
                c >= b_start && c < b_end
            }) {
                total += 1;
                if b.rms_db < gap_floor_db {
                    silent += 1;
                }
            }
            (total > 0).then(|| silent as f64 / total as f64)
        }
        _ => None,
    };

    classify_gap_equivalence(a_gap_rms_db, noise_floor_db, donor_silence_fraction, params)
}

#[cfg(test)]
mod tests {
    use super::*;
    use GapEquivalenceClass::*;

    fn on() -> GapEquivalenceParams {
        GapEquivalenceParams {
            enabled: true,
            ..Default::default()
        }
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
        assert_eq!(
            classify_gap_equivalence(
                Some(-106.0),
                Some(-47.0),
                Some(0.0),
                &GapEquivalenceParams::default()
            )
            .class,
            NotEvaluated
        );
        assert_eq!(
            classify_gap_equivalence(None, Some(-47.0), Some(0.0), &on()).class,
            NotEvaluated
        );
        assert_eq!(
            classify_gap_equivalence(Some(-106.0), Some(-47.0), None, &on()).class,
            NotEvaluated
        );
    }

    #[test]
    fn verdict_reports_a_below_noise() {
        let v = classify_gap_equivalence(Some(-106.0), Some(-47.0), Some(0.0), &on());
        assert_eq!(v.a_below_noise_db, Some(-59.0));
        assert!(!v.drop && v.class == RepairableDropout);
    }

    // --- derive_gap_equivalence (scan-block timelines → signals → classification) --------------------

    /// One 250 ms block at `[t, t+0.25)` with level `db`.
    fn blk(t: f64, db: f64) -> BlockLevel {
        BlockLevel {
            start_secs: t,
            end_secs: t + 0.25,
            rms_db: db,
        }
    }

    /// A dropout gap (A blocks far below a −50 floor) with an occupied B donor → RepairableDropout, and
    /// the derived signals match the block inputs (context noise floor = median of the flanking blocks).
    #[test]
    fn derive_dropout_with_occupied_donor() {
        // Context blocks at −50 dB flank a 0.5 s gap of silence-floored blocks; B is loud across the span.
        let a = vec![
            blk(9.5, -50.0),
            blk(9.75, -50.0),
            blk(10.0, -119.0),
            blk(10.25, -119.0),
            blk(10.5, -50.0),
        ];
        let b = vec![blk(10.0, -20.0), blk(10.25, -20.0)];
        let v = derive_gap_equivalence(&a, 10.0, 10.5, Some(&b), Some((10.0, 10.5)), &on());
        assert_eq!(v.class, RepairableDropout);
        assert_eq!(v.noise_floor_db, Some(-50.0));
        assert_eq!(v.donor_silence_fraction, Some(0.0));
        assert!(v.a_gap_rms_db.unwrap() < -100.0, "{v:?}");
    }

    /// A dropout on A but B is silent over the mapped span → SharedSilence (nothing to fill). A real
    /// dropout's gap floor still sits above B's true silence (−120), so B reads as silent below it.
    #[test]
    fn derive_dropout_with_silent_donor_is_shared_silence() {
        let a = vec![
            blk(9.5, -48.0),
            blk(9.75, -48.0),
            blk(10.0, -100.0),
            blk(10.25, -100.0),
            blk(10.5, -48.0),
        ];
        let b = vec![blk(10.0, -120.0), blk(10.25, -120.0)];
        let v = derive_gap_equivalence(&a, 10.0, 10.5, Some(&b), Some((10.0, 10.5)), &on());
        assert_eq!(v.class, SharedSilence);
        assert_eq!(v.donor_silence_fraction, Some(1.0));
        assert!(v.drop);
    }

    /// Room-tone gap (A only a few dB below its floor) with an occupied donor → AmbientQuiet (drop, but
    /// not a dropout) — the self-calibrating A-side at scan-block granularity.
    #[test]
    fn derive_roomtone_with_occupied_donor_is_ambient_quiet() {
        let a = vec![
            blk(9.5, -47.0),
            blk(9.75, -47.0),
            blk(10.0, -52.0),
            blk(10.25, -52.0),
            blk(10.5, -47.0),
        ];
        let b = vec![blk(10.0, -20.0), blk(10.25, -20.0)];
        let v = derive_gap_equivalence(&a, 10.0, 10.5, Some(&b), Some((10.0, 10.5)), &on());
        assert_eq!(v.class, AmbientQuiet);
        assert!(v.drop);
    }

    /// No B timeline (unscanned/unaligned) ⇒ donor signal absent ⇒ NotEvaluated (conservative keep).
    #[test]
    fn derive_without_b_levels_is_not_evaluated() {
        let a = vec![blk(9.75, -48.0), blk(10.0, -119.0), blk(10.5, -48.0)];
        let v = derive_gap_equivalence(&a, 10.0, 10.5, None, None, &on());
        assert_eq!(v.class, NotEvaluated);
    }

    /// The gate is off by default: even a clean dropout classifies NotEvaluated (advisory computes with
    /// `enabled: true` explicitly; the plan-drop flag is separate).
    #[test]
    fn derive_respects_disabled_params() {
        let a = vec![blk(9.75, -48.0), blk(10.0, -119.0), blk(10.5, -48.0)];
        let b = vec![blk(10.0, -20.0)];
        let v = derive_gap_equivalence(
            &a,
            10.0,
            10.5,
            Some(&b),
            Some((10.0, 10.5)),
            &GapEquivalenceParams::default(),
        );
        assert_eq!(v.class, NotEvaluated);
    }
}
