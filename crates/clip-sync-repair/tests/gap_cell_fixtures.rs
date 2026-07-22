//! **Phase 2** of the gap-fixture-corpus plan (`docs/TEMP-gap-fixture-corpus-plan.md`): the per-gap-**type**
//! classification contract. For every curated fixture (`clip_sync_repair_fixtures::gap_cell_fixtures`), run
//! the **live** classifiers on it and assert the result matches its declared cell — media-independent, so it
//! runs on every PR (no feature gate).
//!
//! This replaces the index-coupled `assert_footguns` semantics with per-type assertions:
//!   - seam/donor cells go through the analyzer's `GapRow` predicates (`dualfit_target()`, `program_quiet()`,
//!     `bracket_exhausted()`, `patched()`) — the same projection production analysis uses;
//!   - equivalence cells re-run the domain `classify_gap_equivalence()` on the fixture's recorded silence
//!     signals.
//!
//! The two footguns the vocabulary calls out are pinned here: a silence-splice **is** a dual-fit target, and a
//! program-quiet gap (high seams, dead donor) is **not** — see `docs/gap-vocabulary.md`.

use clip_sync_repair::domain::gap_equivalence::{
    classify_gap_equivalence, GapEquivalenceClass, GapEquivalenceParams,
};
use clip_sync_repair_fixtures::gap_cell_fixtures::{
    curated_fixtures_dir, load_gap_cell_fixtures, GapCellFixture, GapCellType,
};
use clip_sync_repair_harness::gap_fingerprint_corpus::{gap_rows_from_corpus_json, GapRow};

/// Build the analyzer `GapRow` for a fixture from its **committed** JSON bytes (not a re-serialization), so
/// the contract is checked against the exact artifact that ships.
fn row_of(fx: &GapCellFixture) -> GapRow {
    let path = curated_fixtures_dir().join(&fx.file);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let mut rows = gap_rows_from_corpus_json(&bytes, fx.cell_type.as_str(), 1.0, 30.0)
        .expect("parse fixture corpus into rows");
    assert_eq!(rows.len(), 1, "{}: expected one row", fx.file);
    rows.pop().unwrap()
}

/// Re-run the domain equivalence classifier from a fixture's recorded silence signals, asserting the class.
fn assert_equivalence_class(fx: &GapCellFixture, expected: GapEquivalenceClass) {
    let v = fx
        .gap()
        .equivalence
        .as_ref()
        .unwrap_or_else(|| panic!("{}: fixture carries no equivalence verdict", fx.file));
    let params = GapEquivalenceParams { enabled: true, ..Default::default() };
    let reclassified =
        classify_gap_equivalence(v.a_gap_rms_db, v.noise_floor_db, v.donor_silence_fraction, &params);
    assert_eq!(
        reclassified.class, expected,
        "{} ({}): classify_gap_equivalence gave {:?}, expected {expected:?} [{}]",
        fx.file,
        fx.cell_type.as_str(),
        reclassified.class,
        fx.expected,
    );
    assert_eq!(
        reclassified.drop,
        expected.drops(),
        "{}: drop disposition mismatch",
        fx.file
    );
}

/// Every fixture's live classification matches its declared cell type.
#[test]
fn each_fixture_matches_its_declared_cell() {
    let fixtures = load_gap_cell_fixtures();
    assert!(!fixtures.is_empty(), "no curated fixtures");

    for fx in &fixtures {
        let ctx = || format!("{} ({}) — {}", fx.file, fx.cell_type.as_str(), fx.expected);
        match fx.cell_type {
            GapCellType::BracketPatchClean => {
                let r = row_of(fx);
                assert!(r.patched(), "{}: expected a patched gap", ctx());
                assert!(!r.dualfit_target(), "{}: a patched gap is never a dual-fit target", ctx());
                assert_ne!(r.program_quiet(), Some(true), "{}: clean patch donor should be occupied", ctx());
            }
            GapCellType::BracketPatchDonorBroken => {
                // Footgun: a bracket that cleared the gate patches the gap even though the donor is broken
                // (dead at the nominal span). Donor state must NOT gate a bracket-cleared patch.
                let r = row_of(fx);
                assert!(r.patched(), "{}: expected a patched gap", ctx());
                assert_eq!(
                    r.program_quiet(),
                    Some(true),
                    "{}: expected a broken/dead nominal donor (the footgun premise)",
                    ctx()
                );
                assert!(!r.dualfit_target(), "{}: patched gaps are not dual-fit targets", ctx());
            }
            GapCellType::SilenceSpliceDualfitTarget => {
                // Footgun A: this IS a dual-fit target.
                let r = row_of(fx);
                assert!(r.dualfit_target(), "{}: expected dualfit_target()==true", ctx());
                assert!(!r.patched(), "{}: a dual-fit target is a bracket-exhausted skip", ctx());
                assert!(r.bracket_exhausted(), "{}: expected bracket-exhausted", ctx());
            }
            GapCellType::ProgramQuiet => {
                // Footgun B: the seams PASS the gate (high correlation — looks patchable) yet the donor is
                // dead, so it must be excluded by donor state, not by seam score. The `dualfit_pass` premise
                // is the teeth: without it the "not a target" assertion could pass on a trivially-bad gap.
                let r = row_of(fx);
                assert_eq!(r.program_quiet(), Some(true), "{}: expected program_quiet()", ctx());
                assert_eq!(
                    r.dualfit_pass,
                    Some(true),
                    "{}: footgun premise — seams must PASS the gate (high corr), so exclusion is donor-driven",
                    ctx()
                );
                assert!(!r.dualfit_target(), "{}: program-quiet must not be a dual-fit target", ctx());
                assert!(!r.patched(), "{}: program-quiet is a skip", ctx());
            }
            GapCellType::NoPlacement => {
                // Structure/anchor search found no candidate — never reached seam scoring.
                let r = row_of(fx);
                assert!(!r.patched(), "{}: no-placement is a skip", ctx());
                assert_eq!(r.brackets_total, 0, "{}: expected zero scored brackets", ctx());
                assert!(!r.dualfit_target(), "{}: no-placement is not a dual-fit target", ctx());
            }
            GapCellType::RepairableDropout => {
                assert_equivalence_class(fx, GapEquivalenceClass::RepairableDropout);
            }
            GapCellType::SharedSilence => {
                assert_equivalence_class(fx, GapEquivalenceClass::SharedSilence);
            }
            GapCellType::AmbientQuiet => {
                assert_equivalence_class(fx, GapEquivalenceClass::AmbientQuiet);
            }
            // Synthetic-only cells (Phase 5): no real fixture is committed yet, so none should appear here.
            GapCellType::Decorrelated
            | GapCellType::ResidualVeto
            | GapCellType::TailGeometryMismatch
            | GapCellType::Unfillable => {
                panic!("{}: synthetic-only cell has no committed fixture yet (Phase 5)", ctx());
            }
        }
    }
}
