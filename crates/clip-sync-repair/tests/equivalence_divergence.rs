//! The scan-vs-fine equivalence **divergence** contract (F15 → instrument convergence).
//!
//! The two equivalence front-ends share `classify_gap_equivalence` but historically fed it
//! differently-defined inputs (`docs/dev/gap-fingerprint.md` § *`equivalence` vs `scan_equivalence`*).
//! F15 closed the three fine-path sensor defects; I1 converged the equivalence bin size onto
//! `scan_block_ms`. This file pins the resulting contract media-free:
//!
//! - **`band_donor.json`** is a **regression** fixture for g4 of the F15 pair (re-harvested from
//!   `silence-floor/fp_i1_bin_convergence/`). The paths now **agree** on this gap. The pre-fix band
//!   arithmetic is kept as constants so the closed mechanism stays documented — never swap in a
//!   still-diverging gap to restore a diverge assertion.
//! - a change that flipped any remaining divergence into the **dangerous** direction (scan drops what
//!   fine keeps) would break the safety assertion.
//!
//! `fine_noise_floor_reads_lower_than_scan` and `divergence_is_never_in_the_dangerous_direction` are
//! **not** scheduled to change — a failure in either is a real regression. (The noise-floor residual
//! is the accepted I2 context-window term; see
//! `docs/dev/archive/TEMP-equivalence-instrument-convergence.md` § I2.)
//!
//! Fixture provenance and the measured numbers: `tests/gap_corpus/fingerprints/equivalence_divergence/`.

use clip_sync_repair::application::gap_fingerprint::{GapCorpus, GapFingerprint};
use clip_sync_repair::domain::gap_equivalence::{
    classify_gap_equivalence, GapEquivalenceClass, GapEquivalenceParams, GapEquivalenceVerdict,
};

/// Pre-fix (`fp_post_F14_fix`) numbers for the band mechanism this fixture closed. The re-harvested
/// artifact no longer carries them; they stay here so the mechanism cannot be forgotten.
mod pre_fix {
    pub const SCAN_FLOOR_DB: f64 = -79.500_993_053_487_4;
    pub const FINE_FLOOR_DB: f64 = -58.394_107_818_603_5;
    pub const DONOR_RMS_DB: f64 = -66.935_302_734_375;
    pub const SCAN_DONOR_SILENCE: f64 = 0.473_684_210_526_316;
    pub const FINE_DONOR_SILENCE: f64 = 1.0;
}

/// Load a committed single-gap fixture from its **on-disk bytes** (not a re-serialization),
/// so the contract is checked against the exact artifact that ships.
fn load(name: &str) -> GapFingerprint {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/gap_corpus/fingerprints/equivalence_divergence")
        .join(name);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let corpus: GapCorpus =
        serde_json::from_slice(&bytes).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    assert_eq!(corpus.gaps.len(), 1, "{name} must be a single-gap corpus");
    corpus.gaps.into_iter().next().unwrap()
}

fn on() -> GapEquivalenceParams {
    GapEquivalenceParams {
        enabled: true,
        ..Default::default()
    }
}

/// Re-run the **live** classifier on a verdict's recorded signals, and confirm the fixture's stored class
/// still agrees with it (a self-consistency check on the committed artifact).
fn reclassify(v: &GapEquivalenceVerdict, side: &str) -> GapEquivalenceClass {
    let live = classify_gap_equivalence(
        v.a_gap_rms_db,
        v.noise_floor_db,
        v.donor_silence_fraction,
        &on(),
    );
    assert_eq!(
        live.class, v.class,
        "{side}: fixture records {:?} but the live classifier gives {:?}",
        v.class, live.class
    );
    assert_eq!(live.drop, live.class.drops(), "{side}: drop disposition");
    live.class
}

fn both(fp: &GapFingerprint) -> (&GapEquivalenceVerdict, &GapEquivalenceVerdict) {
    (
        fp.scan_equivalence
            .as_ref()
            .expect("fixture carries no scan_equivalence"),
        fp.equivalence
            .as_ref()
            .expect("fixture carries no fine equivalence"),
    )
}

/// Regression: g4 used to diverge on the donor axis; after F15 + I1 both paths keep it as
/// `repairable_dropout`. Floors match exactly; donor fractions both sit below the occupancy threshold.
#[test]
fn band_donor_now_agrees_on_repairable_dropout() {
    let fp = load("band_donor.json");
    let (scan, fine) = both(&fp);

    assert_eq!(
        reclassify(scan, "scan"),
        GapEquivalenceClass::RepairableDropout
    );
    assert_eq!(
        reclassify(fine, "fine"),
        GapEquivalenceClass::RepairableDropout
    );
    assert!(!scan.drop && !fine.drop, "both keep");

    let (scan_floor, fine_floor) = (
        scan.gap_floor_db.expect("scan gap_floor_db"),
        fine.gap_floor_db.expect("fine gap_floor_db"),
    );
    assert_eq!(
        scan_floor, fine_floor,
        "post-I1 floors must match (scan {scan_floor}, fine {fine_floor})"
    );

    let thresh = on().donor_silence_thresh;
    let (scan_ds, fine_ds) = (
        scan.donor_silence_fraction.expect("scan donor fraction"),
        fine.donor_silence_fraction.expect("fine donor fraction"),
    );
    assert!(
        scan_ds < thresh && fine_ds < thresh,
        "both donors occupied below {thresh} (scan {scan_ds}, fine {fine_ds})"
    );
}

/// The band mechanism is closed: equal floors cannot straddle the donor mean. The pre-fix constants
/// document that they once did — that is the evidence the mechanism existed, not a live assertion on
/// the re-harvested artifact.
#[test]
fn closed_band_mechanism_no_longer_straddles_donor() {
    let fp = load("band_donor.json");
    let (scan, fine) = both(&fp);
    let donor_rms = fp
        .donor_interior_nominal
        .as_ref()
        .expect("fixture carries no nominal donor read")
        .rms_db;
    let (scan_floor, fine_floor) = (
        scan.gap_floor_db.expect("scan gap_floor_db"),
        fine.gap_floor_db.expect("fine gap_floor_db"),
    );

    assert!(
        !(scan_floor < donor_rms && donor_rms < fine_floor),
        "donor {donor_rms} must not sit in a floor band (scan {scan_floor}, fine {fine_floor})"
    );

    // Historical band (fp_post_F14_fix): donor mean between the two floors, fractions straddling 0.5.
    const {
        assert!(
            pre_fix::SCAN_FLOOR_DB < pre_fix::DONOR_RMS_DB
                && pre_fix::DONOR_RMS_DB < pre_fix::FINE_FLOOR_DB,
            "pre-fix band arithmetic"
        );
    }
    let thresh = on().donor_silence_thresh;
    assert!(
        pre_fix::SCAN_DONOR_SILENCE < thresh && pre_fix::FINE_DONOR_SILENCE >= thresh,
        "pre-fix fractions straddled occupancy"
    );
    assert_eq!(
        donor_rms,
        pre_fix::DONOR_RMS_DB,
        "donor mean is invariant across harvests — only the floors moved"
    );
}

/// The fine path still reads a **lower** noise floor than the scan path on this gap — the accepted I2
/// context-window residual (median 0.606 dB on the pair; here ~0.78 dB). A lower floor shrinks
/// `a_below_noise` and biases toward `drop`. Pinned so a sign reversal has to be deliberate.
#[test]
fn fine_noise_floor_reads_lower_than_scan() {
    let fp = load("band_donor.json");
    let (scan, fine) = both(&fp);
    let (s, f) = (
        scan.noise_floor_db.expect("scan noise floor"),
        fine.noise_floor_db.expect("fine noise floor"),
    );
    assert!(f < s, "fine noise floor {f} should read below scan's {s}");
}

/// Safety invariant: never scan-drops what fine keeps. Holds when the paths agree (both keep) and must
/// hold for any future divergence fixture added here. `equivalence-calibration` exits 1 on the
/// dangerous direction.
#[test]
fn divergence_is_never_in_the_dangerous_direction() {
    // One fixture today; extend this list as divergence / regression shapes are harvested.
    let name = "band_donor.json";
    let fp = load(name);
    let (scan, fine) = both(&fp);
    let dangerous = scan.drop && !fine.drop;
    assert!(
        !dangerous,
        "{name}: scan drops a gap the fine path keeps — the dangerous direction"
    );
}
