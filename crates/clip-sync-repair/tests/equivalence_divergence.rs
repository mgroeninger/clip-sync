//! The scan-vs-fine equivalence **divergence** contract (F15).
//!
//! The two equivalence front-ends share `classify_gap_equivalence` but feed it differently-defined inputs
//! (`docs/dev/gap-fingerprint.md` § *`equivalence` vs `scan_equivalence`*). They therefore disagree on a
//! measured ~1.7 % of gaps. That is **expected behaviour, not a defect**, and this file pins it media-free
//! so the class can't silently change shape — in either direction:
//!
//! - a refactor that accidentally *converges* the two sensors would break these assertions loudly, rather
//!   than quietly discarding the deliberate difference;
//! - a change that flipped the divergence into the **dangerous** direction (scan drops what fine keeps)
//!   would break the safety assertion.
//!
//! **If `band_donor_*` goes red after a change to fine's `gap_floor_db`, read the fixture README before
//! touching anything here.** F15's decided direction (a silent-core floor on the fine path) is *predicted*
//! to make that gap converge, which turns those two tests red **by design** — they are the fix's acceptance
//! signal. The response is to convert them into regression assertions ("used to diverge, now agrees"), never
//! to relax them or to swap in a different still-diverging gap.
//!
//! `fine_noise_floor_reads_lower_than_scan` and `divergence_is_never_in_the_dangerous_direction` are **not**
//! scheduled to change — a failure in either is a real regression.
//!
//! Fixture provenance and the measured numbers: `tests/gap_corpus/fingerprints/equivalence_divergence/`.

use clip_sync_repair::application::gap_fingerprint::{GapCorpus, GapFingerprint};
use clip_sync_repair::domain::gap_equivalence::{
    classify_gap_equivalence, GapEquivalenceClass, GapEquivalenceParams, GapEquivalenceVerdict,
};

/// Load a committed single-gap divergence fixture from its **on-disk bytes** (not a re-serialization),
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

/// A donor in the **band** — quieter than fine's `gap_floor_db`, louder than scan's — reads silent to one
/// path and occupied to the other, flipping the class. This is the F15 mechanism.
#[test]
fn band_donor_diverges_on_the_donor_axis() {
    let fp = load("band_donor.json");
    let (scan, fine) = both(&fp);

    // The two paths land on opposite dispositions, live.
    assert_eq!(
        reclassify(scan, "scan"),
        GapEquivalenceClass::RepairableDropout
    );
    assert_eq!(reclassify(fine, "fine"), GapEquivalenceClass::SharedSilence);
    assert!(!scan.drop && fine.drop, "scan keeps, fine drops");

    // The premise: the donor's level sits between the two floors. Without this the divergence would be
    // some other mechanism and the rest of this test would be asserting a coincidence.
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
        scan_floor < donor_rms && donor_rms < fine_floor,
        "donor {donor_rms} must sit in the band (scan floor {scan_floor}, fine floor {fine_floor})"
    );

    // …and the band straddles the occupancy threshold, which is what turns it into a class difference.
    let thresh = on().donor_silence_thresh;
    let (scan_ds, fine_ds) = (
        scan.donor_silence_fraction.expect("scan donor fraction"),
        fine.donor_silence_fraction.expect("fine donor fraction"),
    );
    assert!(
        scan_ds < thresh && fine_ds >= thresh,
        "donor fractions must straddle {thresh} (scan {scan_ds}, fine {fine_ds})"
    );
}

/// Teeth: the divergence is caused by the **donor** axis alone. Holding scan's A-side signals and swapping
/// in only fine's donor fraction reproduces fine's class — so the A RMS / noise-floor splits, real as they
/// are, are not what flips this gap.
#[test]
fn band_donor_divergence_is_attributable_to_the_donor_axis() {
    let fp = load("band_donor.json");
    let (scan, fine) = both(&fp);

    let swapped = classify_gap_equivalence(
        scan.a_gap_rms_db,
        scan.noise_floor_db,
        fine.donor_silence_fraction,
        &on(),
    );
    assert_eq!(
        swapped.class,
        GapEquivalenceClass::SharedSilence,
        "scan A-side + fine donor should reproduce fine's class"
    );
}

/// The fine path reads a **lower** noise floor than the scan path — the second systematic bias, measured on
/// 10/10 gaps of the characterized pair and 5/5 divergences of a 17-pair corpus. A lower floor shrinks
/// `a_below_noise`, pushing gaps *out* of `repairable_dropout`; it biases toward `drop`, same as the floor
/// split. Pinned so a change that reverses the sign has to be deliberate.
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

/// The safety invariant this whole finding rests on: the divergence is in the direction where **scan
/// keeps**. The dangerous direction — scan drops a gap fine would keep — is what
/// `equivalence-calibration` exits 1 on, and it was never observed across 307 measured gaps.
#[test]
fn divergence_is_never_in_the_dangerous_direction() {
    // One fixture today; extend this list as divergence shapes are harvested.
    let name = "band_donor.json";
    let fp = load(name);
    let (scan, fine) = both(&fp);
    let dangerous = scan.drop && !fine.drop;
    assert!(
        !dangerous,
        "{name}: scan drops a gap the fine path keeps — the dangerous direction"
    );
}
