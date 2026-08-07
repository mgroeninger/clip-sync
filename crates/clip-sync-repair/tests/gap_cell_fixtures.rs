//! The per-gap-**type** classification contract (gap cells defined in `docs/dev/gap-vocabulary.md`). For every
//! curated fixture (`clip_sync_repair_fixtures::gap_cell_fixtures`), run
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
//! program-quiet gap (high seams, dead donor) is **not** — see `docs/dev/gap-vocabulary.md`.

use clip_sync_repair::domain::gap_equivalence::{
    classify_gap_equivalence, GapEquivalenceClass, GapEquivalenceParams,
};
use clip_sync_repair_fixtures::gap_cell_fixtures::{
    curated_fixtures_dir, load_gap_cell_fixtures, GapCellFixture, GapCellType,
};
use clip_sync_repair_harness::gap_fingerprint_corpus::{
    gap_rows_from_corpus_json, GapKind, GapRow,
};

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
///
/// Reads **`equivalence_production`**, not `equivalence_diagnostic`. The cells in this taxonomy are
/// *scan-time* cells (see `GapCellType`'s variant docs) and the scan verdict is what production acts on;
/// `equivalence_diagnostic` is a second opinion that diverges on a small fraction of gaps (1.7 % measured
/// 2026-07-30, **before** I1/I3 converged the binning and the donor predicate — expect lower now), in the
/// safe direction on every observed case. Until 2026-07-30 this read the diagnostic block — harmless on
/// these fixtures, where the two agree, but the wrong authority.
/// `docs/dev/gap-fingerprint.md` § *`equivalence_diagnostic` vs `equivalence_production`*.
fn assert_equivalence_class(fx: &GapCellFixture, expected: GapEquivalenceClass) {
    let v = fx.gap().equivalence_production.as_ref().unwrap_or_else(|| {
        panic!(
            "{}: fixture carries no equivalence_production verdict",
            fx.file
        )
    });
    let params = GapEquivalenceParams {
        enabled: true,
        ..Default::default()
    };
    let reclassified = classify_gap_equivalence(
        v.a_gap_rms_db,
        v.noise_floor_db,
        v.donor_silence_fraction,
        &params,
    );
    assert_eq!(
        reclassified.class,
        expected,
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
                assert!(
                    !r.dualfit_target(),
                    "{}: a patched gap is never a dual-fit target",
                    ctx()
                );
                assert_ne!(
                    r.program_quiet(),
                    Some(true),
                    "{}: clean patch donor should be occupied",
                    ctx()
                );
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
                assert!(
                    !r.dualfit_target(),
                    "{}: patched gaps are not dual-fit targets",
                    ctx()
                );
            }
            GapCellType::SilenceSpliceDualfitTarget => {
                // Footgun A: this IS a dual-fit target.
                let r = row_of(fx);
                assert!(
                    r.dualfit_target(),
                    "{}: expected dualfit_target()==true",
                    ctx()
                );
                assert!(
                    !r.patched(),
                    "{}: a dual-fit target is a bracket-exhausted skip",
                    ctx()
                );
                assert!(
                    r.bracket_exhausted(),
                    "{}: expected bracket-exhausted",
                    ctx()
                );
            }
            GapCellType::ProgramQuiet => {
                // Footgun B: the seams PASS the gate (high correlation — looks patchable) yet the donor is
                // dead, so it must be excluded by donor state, not by seam score. The `dualfit_pass` premise
                // is the teeth: without it the "not a target" assertion could pass on a trivially-bad gap.
                let r = row_of(fx);
                assert_eq!(
                    r.program_quiet(),
                    Some(true),
                    "{}: expected program_quiet()",
                    ctx()
                );
                assert_eq!(
                    r.dualfit_pass,
                    Some(true),
                    "{}: footgun premise — seams must PASS the gate (high corr), so exclusion is donor-driven",
                    ctx()
                );
                assert!(
                    !r.dualfit_target(),
                    "{}: program-quiet must not be a dual-fit target",
                    ctx()
                );
                assert!(!r.patched(), "{}: program-quiet is a skip", ctx());
            }
            GapCellType::NoPlacement => {
                // Structure/anchor search found no candidate — never reached seam scoring.
                let r = row_of(fx);
                assert!(!r.patched(), "{}: no-placement is a skip", ctx());
                assert_eq!(
                    r.brackets_total,
                    0,
                    "{}: expected zero scored brackets",
                    ctx()
                );
                assert!(
                    !r.dualfit_target(),
                    "{}: no-placement is not a dual-fit target",
                    ctx()
                );
            }
            GapCellType::RepairableDropout => {
                assert_equivalence_class(fx, GapEquivalenceClass::RepairableDropout);
                // Orthogonality (plan data note): equivalence class and seam disposition are independent —
                // this real dropout is *also* a dual-fit target. Pin it so the orthogonality can't silently
                // regress (the golden freezes it, but nothing else asserts it live).
                assert!(
                    row_of(fx).dualfit_target(),
                    "{}: this repairable_dropout is also a dual-fit target (orthogonal axes)",
                    ctx()
                );
            }
            GapCellType::SharedSilence => {
                assert_equivalence_class(fx, GapEquivalenceClass::SharedSilence);
            }
            GapCellType::AmbientQuiet => {
                assert_equivalence_class(fx, GapEquivalenceClass::AmbientQuiet);
            }
            GapCellType::Decorrelated => {
                // Donor is occupied (unlike program-quiet) but the seams recover at NO lag on any peak layer,
                // so — unlike a silence-splice — it is not a dual-fit target. The third action-distinct skip.
                let r = row_of(fx);
                assert!(!r.patched(), "{}: decorrelated is a skip", ctx());
                assert_eq!(
                    r.verdict.as_deref(),
                    Some("decorrelated"),
                    "{}: expected a decorrelated lag verdict",
                    ctx()
                );
                assert_ne!(
                    r.program_quiet(),
                    Some(true),
                    "{}: donor is occupied (distinguishes decorrelated from program-quiet)",
                    ctx()
                );
                // Teeth: the seams genuinely don't recover — pin the mechanism, not just the outcome.
                assert_eq!(
                    r.dualfit_pass,
                    Some(false),
                    "{}: seam gate must NOT pass (that's why it's not a target)",
                    ctx()
                );
                assert!(
                    !r.both_sides_recoverable(),
                    "{}: neither shoulder recovers a unique peak",
                    ctx()
                );
                for (side, pk) in [("pre", r.peak_r_pre), ("post", r.peak_r_post)] {
                    assert!(
                        pk.is_some_and(|v| v < 0.5),
                        "{}: {side} gross peak {pk:?} should be low (seams don't recover)",
                        ctx()
                    );
                }
                assert!(
                    !r.dualfit_target(),
                    "{}: seams recover at no lag ⇒ not a dual-fit target",
                    ctx()
                );
            }
            GapCellType::TailGeometryMismatch => {
                // A length-mismatch tail: filtered before per-gap scoring, excluded from the matched denom.
                let r = row_of(fx);
                assert_eq!(r.kind, GapKind::Tail, "{}: expected GapKind::Tail", ctx());
                assert!(
                    r.duration_secs.is_some_and(|d| d >= 30.0),
                    "{}: tail duration {:?} must be ≥ the tail cutoff",
                    ctx(),
                    r.duration_secs
                );
                assert_eq!(
                    r.brackets_total,
                    0,
                    "{}: a tail is filtered before seam scoring (unscored)",
                    ctx()
                );
                assert!(
                    !r.dualfit_target(),
                    "{}: a tail is not a dual-fit target",
                    ctx()
                );
            }
            // Not fingerprint-representable — the dump path sets `outcome.tier` from seam scoring (`any_ok`,
            // gap_fingerprint.rs), never from residual gating or plan/execution failures, so neither cell can
            // appear as an honest characterized gap. The residual-gate *decision* (headroom → veto verdict) is
            // tested at score/region level (`validate_residual_gate` F4 cases, `seam_residual_oracle`,
            // `measure_dual_fit_residual_verdict`); the end-to-end pipeline veto is the **optional/unproved
            // C1b** item (`tests/residual_gate_catalog/README.md`). `unfillable` is covered by
            // `GapPatchSkipReason` unit tests. No manifest entry should declare either type.
            GapCellType::ResidualVeto | GapCellType::Unfillable => {
                panic!(
                    "{}: not a fingerprint-representable cell — must not have a curated fixture",
                    ctx()
                );
            }
        }
    }
}
