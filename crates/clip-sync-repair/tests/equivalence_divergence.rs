//! The scan-vs-diagnostic equivalence **divergence** contract (F15 → instrument convergence).
//!
//! Tier: **pr-repair** — media-free committed fixtures; guards the production scan-time
//! equivalence gate (on by default).
//!
//! The two equivalence front-ends share `classify_gap_equivalence` but historically fed it
//! differently-defined inputs (`docs/dev/gap-fingerprint.md` § *`equivalence_diagnostic` vs `equivalence_production`*).
//! F15 closed the three diagnostic-path sensor defects; I1 converged the equivalence bin size onto
//! `scan_block_ms`. This file pins the resulting contract media-free:
//!
//! - **`band_donor.json`** is a **regression** fixture for g4 of the F15 pair (re-harvested after the
//!   I1 bin-convergence fix). The paths now **agree** on this gap. The pre-fix band
//!   arithmetic is kept as constants so the closed mechanism stays documented — never swap in a
//!   still-diverging gap to restore a diverge assertion.
//! - a change that flipped any remaining divergence into the **dangerous** direction (scan drops what
//!   the diagnostic path keeps) would break the safety assertion.
//!
//! `diag_noise_floor_reads_lower_than_scan` and `divergence_is_never_in_the_dangerous_direction` are
//! **not** scheduled to change — a failure in either is a real regression. (The noise-floor residual
//! is the accepted I2 context-window term; see
//! `docs/dev/archive/TEMP-equivalence-instrument-convergence.md` § I2.)
//!
//! Fixture provenance and the measured numbers: `tests/gap_corpus/fingerprints/equivalence_divergence/`.

use clip_sync_repair::application::gap_fingerprint::{GapCorpus, GapFingerprint};
use clip_sync_repair::domain::gap_equivalence::{
    classify_gap_equivalence, GapEquivalenceClass, GapEquivalenceParams, GapEquivalenceVerdict,
};

/// Pre-fix (post-F14, pre-F15) numbers for the band mechanism this fixture closed. The re-harvested
/// artifact no longer carries them, and the `gap-files/` dump they came from is ephemeral and now
/// deleted — **these constants are the record of that run**, which is why they are transcribed here
/// rather than cited by path.
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

/// C1 wrapped both verdicts in `Contracted<_>`, whose `#[serde(flatten)]` deserializes through serde's
/// buffered `Content` rather than the struct's own field visitor. Buffering is the one C1 change that
/// could alter *values* rather than merely add a key, so it is pinned against the committed bytes:
/// every key the live type still carries must come back **byte-equal** to the artifact.
///
/// Deliberately not a whole-object equality. `silent_core_probes` was hard-deleted from
/// `GapEquivalenceVerdict` (see `docs/dev/gap-fingerprint.md` § *`measurement`*) while committed
/// fixtures keep the dead key, so a re-serialization is *expected* to be a subset. The invariant is
/// "nothing the type still models drifted", which is what the flatten wrapper could plausibly break.
#[test]
fn wrapping_in_contracted_did_not_disturb_the_verdict_wire_shape() {
    /// Keys deleted from the type but still present in committed fixtures.
    const DEAD_KEYS: &[&str] = &["silent_core_probes"];

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/gap_corpus/fingerprints/equivalence_divergence/band_donor.json");
    let text = std::fs::read_to_string(&path).expect("read fixture");
    let before: serde_json::Value = serde_json::from_str(&text).expect("parse fixture");
    let corpus: GapCorpus = serde_json::from_str(&text).expect("typed parse");
    let after = serde_json::to_value(&corpus).expect("re-serialize");

    for key in ["equivalence_diagnostic", "equivalence_production"] {
        let orig = before["gaps"][0][key]
            .as_object()
            .unwrap_or_else(|| panic!("fixture must carry {key}"));
        let round = after["gaps"][0][key]
            .as_object()
            .unwrap_or_else(|| panic!("{key} must re-serialize as an object"));

        assert!(
            !round.contains_key("_contract"),
            "{key}: fixtures stay contract-free — contracts are stamped at measurement (plan § 4.3)"
        );
        for (k, v) in round {
            assert_eq!(
                Some(v),
                orig.get(k),
                "{key}.{k} drifted across the flatten round trip"
            );
        }
        for k in orig.keys() {
            assert!(
                round.contains_key(k) || DEAD_KEYS.contains(&k.as_str()),
                "{key}.{k} was dropped and is not a known dead key"
            );
        }
    }
}

fn both(fp: &GapFingerprint) -> (&GapEquivalenceVerdict, &GapEquivalenceVerdict) {
    (
        fp.equivalence_production_verdict()
            .expect("fixture carries no equivalence_production"),
        fp.equivalence_diagnostic_verdict()
            .expect("fixture carries no equivalence_diagnostic"),
    )
}

/// Regression: g4 used to diverge on the donor axis; after F15 + I1 both paths keep it as
/// `repairable_dropout`. Floors match exactly; donor fractions both sit below the occupancy threshold.
#[test]
fn band_donor_now_agrees_on_repairable_dropout() {
    let fp = load("band_donor.json");
    let (scan, diag) = both(&fp);

    assert_eq!(
        reclassify(scan, "scan"),
        GapEquivalenceClass::RepairableDropout
    );
    assert_eq!(
        reclassify(diag, "diag"),
        GapEquivalenceClass::RepairableDropout
    );
    assert!(!scan.drop && !diag.drop, "both keep");

    let (scan_floor, diag_floor) = (
        scan.gap_floor_db.expect("scan gap_floor_db"),
        diag.gap_floor_db.expect("diag gap_floor_db"),
    );
    assert_eq!(
        scan_floor, diag_floor,
        "post-I1 floors must match (scan {scan_floor}, diag {diag_floor})"
    );

    let thresh = on().donor_silence_thresh;
    let (scan_ds, diag_ds) = (
        scan.donor_silence_fraction.expect("scan donor fraction"),
        diag.donor_silence_fraction.expect("diag donor fraction"),
    );
    assert!(
        scan_ds < thresh && diag_ds < thresh,
        "both donors occupied below {thresh} (scan {scan_ds}, diag {diag_ds})"
    );
}

/// The band mechanism is closed: equal floors cannot straddle the donor mean. The pre-fix constants
/// document that they once did — that is the evidence the mechanism existed, not a live assertion on
/// the re-harvested artifact.
#[test]
fn closed_band_mechanism_no_longer_straddles_donor() {
    let fp = load("band_donor.json");
    let (scan, diag) = both(&fp);
    let donor_rms = fp
        .donor_interior_nominal
        .as_ref()
        .expect("fixture carries no nominal donor read")
        .rms_db;
    let (scan_floor, diag_floor) = (
        scan.gap_floor_db.expect("scan gap_floor_db"),
        diag.gap_floor_db.expect("diag gap_floor_db"),
    );

    assert!(
        !(scan_floor < donor_rms && donor_rms < diag_floor),
        "donor {donor_rms} must not sit in a floor band (scan {scan_floor}, diag {diag_floor})"
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

/// The diagnostic path still reads a **lower** noise floor than the scan path on this gap — the accepted I2
/// context-window residual (median 0.606 dB on the pair; here ~0.78 dB). A lower floor shrinks
/// `a_below_noise` and biases toward `drop`. Pinned so a sign reversal has to be deliberate.
#[test]
fn diag_noise_floor_reads_lower_than_scan() {
    let fp = load("band_donor.json");
    let (scan, diag) = both(&fp);
    let (s, f) = (
        scan.noise_floor_db.expect("scan noise floor"),
        diag.noise_floor_db.expect("diag noise floor"),
    );
    assert!(f < s, "diag noise floor {f} should read below scan's {s}");
}

/// Safety invariant: never scan-drops what the diagnostic path keeps. Holds when the paths agree (both keep) and must
/// hold for any future divergence fixture added here. `equivalence-calibration` exits 1 on the
/// dangerous direction.
#[test]
fn divergence_is_never_in_the_dangerous_direction() {
    // One fixture today; extend this list as divergence / regression shapes are harvested.
    let name = "band_donor.json";
    let fp = load(name);
    let (scan, diag) = both(&fp);
    let dangerous = scan.drop && !diag.drop;
    assert!(
        !dangerous,
        "{name}: scan drops a gap the diagnostic path keeps — the dangerous direction"
    );
}
