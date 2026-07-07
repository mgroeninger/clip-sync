//! **C4 skeleton — `GapRepairSpec` cell identity + golden `gap_row` projection** (step 6a, §4.7 C4).
//!
//! STATUS: SKELETON. Not wired into `lib.rs` yet — `GapRepairSpec` / `to_fingerprint_summary` do not exist
//! until §3 step 6a lands. Add `pub mod gap_repair_spec_projection;` to `lib.rs` at 6a; until then this file
//! is not compiled (Cargo only builds modules reachable from the crate root), so it cannot break the build.
//!
//! ## The invariant this pins (read `gap-vocabulary.md` first)
//!
//! A gap's **identity is its cell** (`gap-vocabulary.md:4` — *"patch/skip is a function of the cell, not a
//! score"*; `:66` — *"read the cell instead"*). `outcome.tier`, `dualfit_target()`, and the legacy W5/W7 tags
//! are **projections** of the cell, not facts about the gap. In particular a **Silence-splice** is the SAME
//! cell whether dual-fit repaired it (`Patch(SilenceSplice)`, `tier=patch`, legacy W7) or the flag was off /
//! it declined (`Skip { cell: SilenceSplice }`, `tier=skip`, legacy W5). Characterize *detects* the cell
//! regardless of the `dual_fit` flag; the flag only chooses the emitted verdict.
//!
//! So C4 asserts, per §4.1a class:
//!   1. **Cell identity (primary):** `spec.cell()` equals the class's cell — derived from D/R axes, computed
//!      directly on the spec, INDEPENDENT of the verdict/tier. Plus an explicit flag-invariance check on the
//!      Silence-splice class: built as `Patch` vs `Skip { cell: SilenceSplice }`, `cell()` is unchanged while
//!      only the projections (`tier`/`dualfit_target`) differ.
//!   2. **Projection faithfulness:** `spec.to_fingerprint_summary(None)` → serialize → `gap_row` reproduces
//!      the Tier-1 axes bit-exact and the Tier-2 D/R inputs equal to the values STORED on the spec (no
//!      re-measurement — the A7 / two-oracle drift guard, §2.5.2a single-source rule).
//!
//! Going through JSON (not a direct `spec -> GapRow` method) is deliberate: it exercises the actual
//! licensing-safe export chain and reuses the frozen `gap_row` reader, so C4 and the golden corpus diff read
//! specs through identical code.

#[cfg(test)]
mod tests {
    use crate::gap_fingerprint_corpus::{analyze_dirs, GapRow};
    // TODO(6a): from the new `domain/gap_repair_spec.rs` (§2.5.2 / §2.5.2a).
    // use clip_sync_repair::domain::gap_repair_spec::{
    //     GapRepairSpec, GapRepairVerdict, GapRepairStrategy, GapRepairCell, GapRepairTags,
    //     RegistrationTags, SeamLocalTags, GateTags, LevelTags, BExtractWindow,
    // };
    // use clip_sync_repair::domain::policies::{FillAlignment, RefinedGapFrames};

    /// The six §4.1a causal classes (harness name → vocab cell). NOTE the cell is coarser than the class:
    /// classes 2/4/5 are distinct upstream reasons that all resolve to the **Program-quiet** cell (class 4 is
    /// "reported as Program-quiet/donor-dead — not a cell of its own", §4.1a / §2.5.2 cell table).
    #[derive(Clone, Copy, Debug)]
    enum Class {
        NoPlacement,          // 1 → cell NoPlacement
        ProgramQuietEarly,    // 2 → cell ProgramQuiet (brackets_total=0, nominal silent)
        SilenceSplice,        // 3 → cell SilenceSplice (the dual-fit addressable set)
        DonorAlignedDecline,  // 4 → cell ProgramQuiet (exhausted, seams good, aligned donor BROKEN)
        ProgramQuietExtreme,  // 5 → cell ProgramQuiet (exhausted, nominal 100% silent)
        BracketPatch,         // 6 → cell BracketPatch (negative control)
        // --- wider-production cells: HAND-BUILT fixtures, zero re-anchor members (§2.5.2) ---
        // 7 — §4.6 decorrelated/different-source. Bracket-exhausted, donor OCCUPIED, seams don't recover
        // (gate_pass=false) — bare `CorrelationBelowThreshold` in the source. Reconciles to a reasoned skip
        // ⇒ cell Decorrelated (all 32 re-anchor bracket-exhausted gaps have gate_pass True/None, never false).
        Decorrelated,
        // 8 — seams PASS the waveform gate but the residual gate vetoes (B ≠ A: echo/repeat). Reasoned skip
        // ⇒ cell ResidualVeto. `ResidualHeadroomExceeded` in the source; not in the re-anchor golden.
        ResidualVeto,
        // 9 — mechanical: B window empty / segment out of range / zero-length. Definite skip (no judgment)
        // ⇒ cell Unfillable. `BExtractFailed`/`AlignedSegmentOutOfRange`/`ZeroLengthGap` in the source.
        Unfillable,
    }

    const ALL_CLASSES: [Class; 9] = [
        Class::NoPlacement,
        Class::ProgramQuietEarly,
        Class::SilenceSplice,
        Class::DonorAlignedDecline,
        Class::ProgramQuietExtreme,
        Class::BracketPatch,
        Class::Decorrelated,
        Class::ResidualVeto,
        Class::Unfillable,
    ];

    /// The cell identity for each class — the invariant C4 exists to pin. `cell()` is TOTAL to a cell (no
    /// `Option`): under the "reconcile to an action" discriminator every per-region disposition names a cell.
    /// NOTE class 4 (`DonorAlignedDecline`): its cell is **ProgramQuiet** (via the aligned-donor-dead term of
    /// the two-predicate union, §2.5.2), even though the harness `program_quiet_skip()` is FALSE for it (that
    /// predicate is nominal-only and nominal is occupied). C4 asserts both — see the axes note in the test.
    /// ```ignore
    /// fn expected_cell(class: Class) -> GapRepairCell {
    ///     match class {
    ///         Class::NoPlacement           => GapRepairCell::NoPlacement,
    ///         Class::SilenceSplice         => GapRepairCell::SilenceSplice,
    ///         Class::BracketPatch          => GapRepairCell::BracketPatch,
    ///         Class::ProgramQuietEarly
    ///         | Class::DonorAlignedDecline // aligned-donor-dead term ⇒ ProgramQuiet cell
    ///         | Class::ProgramQuietExtreme => GapRepairCell::ProgramQuiet,
    ///         Class::Decorrelated          => GapRepairCell::Decorrelated,  // bare CorrelationBelowThreshold
    ///         Class::ResidualVeto          => GapRepairCell::ResidualVeto,  // ResidualHeadroomExceeded
    ///         Class::Unfillable            => GapRepairCell::Unfillable,    // BExtract/OutOfRange/ZeroLength
    ///     }
    /// }
    /// ```
    #[allow(dead_code)]
    fn expected_cell(_class: Class) -> ! {
        todo!("6a: return GapRepairCell — see sketch")
    }

    /// Tier-1 D/R axes that must round-trip through the fingerprint projection, per class. These are
    /// tier-INDEPENDENT (they hold whether or not dual-fit repaired the gap) — the raw material `cell()`
    /// derives from, and what `gap_row` re-exposes.
    struct ExpectAxes {
        bracket_exhausted: bool,
        program_quiet_skip: bool,      // NOTE: `program_quiet_skip()` requires tier==skip; see note in test
        donor_continuous: Option<bool>,
        dualfit_pass: Option<bool>,
    }

    fn expect_axes(class: Class) -> ExpectAxes {
        match class {
            // brackets_total=0 ⇒ bracket_exhausted (total>0 && passing==0) is FALSE for classes 1–2.
            Class::NoPlacement => ExpectAxes {
                bracket_exhausted: false,
                program_quiet_skip: false,
                donor_continuous: None, // no dual-fit detect ran ⇒ donor unmeasured
                dualfit_pass: None,
            },
            Class::ProgramQuietEarly => ExpectAxes {
                bracket_exhausted: false,
                program_quiet_skip: true,
                donor_continuous: None,
                dualfit_pass: None,
            },
            Class::SilenceSplice => ExpectAxes {
                bracket_exhausted: true,
                program_quiet_skip: false,
                donor_continuous: Some(true),
                dualfit_pass: Some(true),
            },
            Class::DonorAlignedDecline => ExpectAxes {
                bracket_exhausted: true,
                program_quiet_skip: false,
                donor_continuous: Some(false), // internal silent run ≥150 ms ⇒ declined
                dualfit_pass: Some(true),      // seams score as well as class 3; decline is donor, not seam
            },
            Class::ProgramQuietExtreme => ExpectAxes {
                bracket_exhausted: true,
                program_quiet_skip: true, // nominal donor 100% silent
                donor_continuous: Some(false),
                dualfit_pass: Some(true),
            },
            Class::BracketPatch => ExpectAxes {
                bracket_exhausted: false, // ≥1 bracket passes
                program_quiet_skip: false,
                donor_continuous: None,
                dualfit_pass: None,
            },
            // --- wider-production cells (HAND-BUILT; axes speculative until real media produces one) ---
            Class::Decorrelated => ExpectAxes {
                bracket_exhausted: true,
                program_quiet_skip: false,     // donor OCCUPIED at nominal ⇒ not program-quiet
                donor_continuous: Some(true),  // aligned donor bridges — the discriminator from class 4
                dualfit_pass: Some(false),     // ...but seams don't recover ⇒ CorrelationBelowThreshold skip
                                               //    ⇒ cell Decorrelated (§4.6, no re-anchor member)
            },
            Class::ResidualVeto => ExpectAxes {
                bracket_exhausted: true,       // a bracket scored but failed at the residual stage
                program_quiet_skip: false,
                donor_continuous: Some(true),  // donor is fine — the veto is "correlates but B ≠ A"
                dualfit_pass: None,            // residual veto is on the ordinary path, not a dual-fit axis
                // Distinguisher from Decorrelated is `closest_failure_stage == "residual"` + residual headroom
                // > margin (not a GapRow bool here) — assert via the residual fields in assert_no_recompute.
            },
            Class::Unfillable => ExpectAxes {
                bracket_exhausted: false,      // mechanical skip BEFORE the seam gate
                program_quiet_skip: false,
                donor_continuous: None,
                dualfit_pass: None,
            },
        }
    }

    /// TODO(6a): build a synthetic `GapRepairSpec` whose stored D/R fields realize `class`'s signature, with
    /// the production default `dual_fit = on` (so a Silence-splice is emitted as `Patch(SilenceSplice)`).
    /// Keep every seam/lag/donor value EXPLICIT so `assert_no_recompute` has known ground truth. One arm per
    /// class; set `verdict` + `tags_ctx` per §2.5.2 / §2.5.2a. See the §2.5.2a doc-comment sketch for the
    /// SilenceSplice arm (single-source: `strategy.pre_seam_r == tags.seam_local.pre_seam_r`).
    #[allow(dead_code)]
    fn spec_for_class(_class: Class) -> ! {
        todo!("6a: construct GapRepairSpec per §2.5.2a")
    }

    /// TODO(6a): same axes as `spec_for_class(SilenceSplice)` but emitted with `dual_fit` OFF, i.e.
    /// `verdict = Skip { cell: SilenceSplice, .. }`. Used only by the flag-invariance assertion.
    #[allow(dead_code)]
    fn spec_silence_splice_declined() -> ! {
        todo!("6a: SilenceSplice axes, verdict = Skip { cell: SilenceSplice }")
    }

    /// TODO(6a): the production export projection under test.
    /// ```ignore
    /// fn project_to_row(spec: &GapRepairSpec) -> GapRow {
    ///     let fp = spec.to_fingerprint_summary(None); // X-set omitted — decisions only
    ///     let json = serde_json::to_string(&[fp]).unwrap();
    ///     let root = tempfile::tempdir().unwrap();
    ///     write_corpus(&root.path().join("1"), "aaaa", "bbbb", &json); // shared test helper (make pub(crate))
    ///     analyze_dirs(&[root.path().to_path_buf()], 1.0, 30.0).rows.remove(0)
    /// }
    /// ```
    #[allow(dead_code)]
    fn project_to_row(_spec: &()) -> ! {
        let _ = analyze_dirs; // keep the import live in the skeleton
        todo!("6a: to_fingerprint_summary → serialize → gap_row")
    }

    /// Tier-2: every continuous D/R input on the row equals the value the spec STORED (within ε), proving no
    /// re-measurement in the export. TODO(6a): compare `row.*` to `spec.tags_ctx.*` per the §2.5.2a mapping
    /// (peak_r, frac_lag_ms, step_ms, dualfit_*_r, donor silence/rms, residual headroom, structure_min,
    /// seam_min, a_*_floor_db). For SilenceSplice also assert `row.dualfit_pre_r == strategy.pre_seam_r`
    /// (single-source: strategy and tags carry the SAME DualFitResult value).
    #[allow(dead_code)]
    fn assert_no_recompute(_row: &GapRow, _spec: &()) {
        todo!("6a: field-by-field Tier-2 equality vs stored spec values")
    }

    /// Primary C4 test: cell identity + axis-projection faithfulness, all six classes.
    #[test]
    #[ignore = "C4 skeleton — enable at step 6a once GapRepairSpec + to_fingerprint_summary exist"]
    fn gap_repair_spec_cell_identity_and_projection_per_class() {
        for class in ALL_CLASSES {
            let axes = expect_axes(class);
            let _spec = spec_for_class(class); // TODO(6a)

            // --- (1) Cell identity — the invariant, computed directly on the spec, tier-independent. ---
            // assert_eq!(_spec.cell(), expected_cell(class), "{class:?} cell identity");

            // --- (2) Projection faithfulness — through the frozen gap_row reader. ---
            let row: GapRow = unreachable!("wire project_to_row(&_spec) at 6a");
            #[allow(unreachable_code)]
            {
                assert_eq!(row.bracket_exhausted(), axes.bracket_exhausted, "{class:?} bracket_exhausted");
                assert_eq!(row.donor_continuous, axes.donor_continuous, "{class:?} donor_continuous");
                assert_eq!(row.dualfit_pass, axes.dualfit_pass, "{class:?} dualfit_pass");
                // `program_quiet_skip()` is a PARTIAL projection of the ProgramQuiet cell:
                //   * it gates on tier==skip — specs here are dual_fit ON, so a Silence-splice is a Patch and
                //     this is correctly false for it (identity asserted via `cell()` above, tier-independent);
                //   * it is nominal-only — class 4 (DonorAlignedDecline) has `cell()==ProgramQuiet` via the
                //     aligned-donor-dead term, yet `program_quiet_skip()==false` because nominal is occupied.
                // Both are correct; the cell is the identity, this predicate is one of its two terms (§2.5.2).
                assert_eq!(row.program_quiet_skip(), axes.program_quiet_skip, "{class:?} program_quiet_skip");

                assert_no_recompute(&row, &()); // TODO(6a): pass &_spec
            }
        }
    }

    /// The correction that motivated this file: a Silence-splice keeps its cell across the `dual_fit` flag;
    /// only the projections move. Guards against ever re-deriving the cell from the verdict/tier.
    #[test]
    #[ignore = "C4 skeleton — enable at step 6a"]
    fn silence_splice_cell_is_invariant_across_dual_fit_flag() {
        let _patched = spec_for_class(Class::SilenceSplice); // dual_fit on  → Patch(SilenceSplice)
        let _declined = spec_silence_splice_declined();      // dual_fit off → Skip { cell: SilenceSplice }

        // Same identity regardless of whether it was repaired:
        // assert_eq!(_patched.cell(), GapRepairCell::SilenceSplice);
        // assert_eq!(_declined.cell(), GapRepairCell::SilenceSplice);

        // ...but the projections differ, and follow from (cell, dual_fit), never the reverse:
        // let (rp, rd) = (project_to_row(&_patched), project_to_row(&_declined));
        // assert!(rp.patched() && !rp.dualfit_target(), "repaired: patch tier, no longer a pending target");
        // assert!(!rd.patched() && rd.dualfit_target(), "declined: skip tier, still a pending dual-fit target");
    }

    /// **C4b — cell derivation is exhaustive over `GapPatchSkipReason`.** Grounds the vocabulary in the source:
    /// every skip reason the code can emit maps to a defined cell, so a NEW reason variant added to
    /// `patch_result.rs` cannot silently escape the vocabulary.
    ///
    /// PRIMARY enforcement is compile-time: `cell_for_skip_reason` (the `GapPatchSkipReason → GapRepairCell`
    /// arm of `cell()`, added at 6a) must match **with no `_ =>` wildcard**, so a new variant is a build error
    /// until it is classified. This runtime test is the backstop — it constructs one of EVERY variant and
    /// asserts the mapping, catching a lazy `_ =>` if one is ever added.
    ///
    /// ```ignore
    /// #[test]
    /// fn cell_derivation_is_exhaustive_over_skip_reasons() {
    ///     use clip_sync_repair::domain::patch_result::GapPatchSkipReason as R;
    ///     use clip_sync_repair::domain::gap_repair_spec::{cell_for_skip_reason, GapRepairCell as C};
    ///     let cases = [
    ///         (R::BoundaryAlignmentFailed,                         C::NoPlacement),
    ///         (R::ProgramQuiet,                                    C::ProgramQuiet),
    ///         (R::CorrelationBelowThreshold { .. },                C::Decorrelated),   // bare = decorrelated
    ///         (R::ResidualHeadroomExceeded { .. },                 C::ResidualVeto),
    ///         (R::BExtractFailed,                                  C::Unfillable),
    ///         (R::AlignedSegmentOutOfRange,                        C::Unfillable),
    ///         (R::ZeroLengthGap,                                   C::Unfillable),
    ///         // NOTE: CorrelationBelowThreshold is `Decorrelated` ONLY as a bare skip; when characterize
    ///         // sub-classifies it (dual-fit accepted → SilenceSplice, donor-dead → ProgramQuiet) the cell is
    ///         // decided upstream of the reason. `cell_for_skip_reason` is the fallback for the un-sub-
    ///         // classified reason; `cell()` on the full spec layers the sub-classification on top.
    ///     ];
    ///     for (reason, want) in cases { assert_eq!(cell_for_skip_reason(&reason), want); }
    ///     // Pair-level `GapFillSkipReason` (TrackLayoutMismatch / TrackCompatibilityUnavailable) is NOT
    ///     // covered here — it never reaches per-region cell(); it lives on GapFillPlan.skipped.
    /// }
    /// ```
    #[test]
    #[ignore = "C4b skeleton — enable at step 6a with cell_for_skip_reason"]
    fn cell_derivation_is_exhaustive_over_skip_reasons() {
        // TODO(6a): replace with the `cases` table above; the wildcard-free match in `cell_for_skip_reason`
        // is the real guarantee, this test is the backstop against a future `_ =>`.
    }
}
