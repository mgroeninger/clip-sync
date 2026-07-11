//! **C4 — `GapRepairSpec` cell identity + golden `gap_row` projection** (Fingerprint-unification 8e, §4.7 C4).
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
//!   1. **Cell identity (primary):** `spec.cell()` equals the class's cell — derived from the verdict/D-R axes,
//!      computed directly on the spec, INDEPENDENT of the tier. Plus an explicit flag-invariance check on the
//!      Silence-splice class: built as `Patch` vs `Skip { cell: SilenceSplice }`, `cell()` is unchanged while
//!      only the projections (`tier`/`dualfit_target`) differ.
//!   2. **Projection faithfulness:** `spec_to_fingerprint_summary(spec, .., None)` → serialize → `gap_row`
//!      reproduces the Tier-1 axes the vocabulary derives from (`bracket_exhausted` / `donor_continuous` /
//!      `dualfit_pass` / `program_quiet_skip`), read through the *actual* licensing-safe export chain (serde
//!      JSON + the frozen corpus reader), so C4 and the golden corpus diff read specs through identical code.
//!
//! Tier-2 continuous round-trip (every stored scalar equals its read-back, no re-measurement — the A7 guard)
//! is unit-tested directly against the projected `GapFingerprint` in
//! `clip_sync_repair::application::gap_fingerprint` (`spec_to_fingerprint_projects_silence_splice_skip_axes`).

#[cfg(test)]
mod tests {
    use crate::gap_fingerprint_corpus::{analyze_dirs, GapRow};
    use clip_sync_repair::domain::donor::DonorInterior;
    use clip_sync_repair::domain::gap_fill_fit::FillConfidence;
    use clip_sync_repair::domain::gap_repair_spec::{
        BExtractWindow, GapRepairCell, GapRepairSpec, GapRepairStrategy, GapRepairTags,
        GapRepairVerdict, GateTags, SeamLocalTags,
    };
    use clip_sync_repair::domain::patch_result::GapPatchSkipReason;
    use clip_sync_repair::domain::policies::{FillAlignment, RefinedGapFrames};

    /// The nine §4.1a causal classes (harness name → vocab cell). Classes 2/4/5 are distinct upstream reasons
    /// that all resolve to the **Program-quiet** cell (class 4 is "reported as Program-quiet/donor-dead — not a
    /// cell of its own", §4.1a). Classes 7-9 are wider-production cells (hand-built; zero re-anchor members).
    #[derive(Clone, Copy, Debug)]
    enum Class {
        NoPlacement,
        ProgramQuietEarly,
        SilenceSplice,
        DonorAlignedDecline,
        ProgramQuietExtreme,
        BracketPatch,
        Decorrelated,
        ResidualVeto,
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

    /// The cell identity for each class — the invariant C4 exists to pin. Class 4 (`DonorAlignedDecline`) is
    /// **ProgramQuiet** via the aligned-donor-dead term (§2.5.2), even though `program_quiet_skip()` is FALSE
    /// for it (that predicate is nominal-only and nominal is occupied). C4 asserts both.
    fn expected_cell(class: Class) -> GapRepairCell {
        match class {
            Class::NoPlacement => GapRepairCell::NoPlacement,
            Class::SilenceSplice => GapRepairCell::SilenceSplice,
            Class::BracketPatch => GapRepairCell::BracketPatch,
            Class::ProgramQuietEarly
            | Class::DonorAlignedDecline
            | Class::ProgramQuietExtreme => GapRepairCell::ProgramQuiet,
            Class::Decorrelated => GapRepairCell::Decorrelated,
            Class::ResidualVeto => GapRepairCell::ResidualVeto,
            Class::Unfillable => GapRepairCell::Unfillable,
        }
    }

    /// Tier-1 D/R axes that must round-trip through the projection, per class. Tier-INDEPENDENT (they hold
    /// whether or not dual-fit repaired the gap) — the raw material `cell()` derives from, re-exposed by
    /// `gap_row`.
    struct ExpectAxes {
        bracket_exhausted: bool,
        program_quiet_skip: bool,
        donor_continuous: Option<bool>,
        dualfit_pass: Option<bool>,
    }

    fn expect_axes(class: Class) -> ExpectAxes {
        match class {
            Class::NoPlacement => ExpectAxes { bracket_exhausted: false, program_quiet_skip: false, donor_continuous: None, dualfit_pass: None },
            Class::ProgramQuietEarly => ExpectAxes { bracket_exhausted: false, program_quiet_skip: true, donor_continuous: None, dualfit_pass: None },
            Class::SilenceSplice => ExpectAxes { bracket_exhausted: true, program_quiet_skip: false, donor_continuous: Some(true), dualfit_pass: Some(true) },
            Class::DonorAlignedDecline => ExpectAxes { bracket_exhausted: true, program_quiet_skip: false, donor_continuous: Some(false), dualfit_pass: Some(true) },
            Class::ProgramQuietExtreme => ExpectAxes { bracket_exhausted: true, program_quiet_skip: true, donor_continuous: Some(false), dualfit_pass: Some(true) },
            Class::BracketPatch => ExpectAxes { bracket_exhausted: false, program_quiet_skip: false, donor_continuous: None, dualfit_pass: None },
            Class::Decorrelated => ExpectAxes { bracket_exhausted: true, program_quiet_skip: false, donor_continuous: Some(true), dualfit_pass: Some(false) },
            Class::ResidualVeto => ExpectAxes { bracket_exhausted: true, program_quiet_skip: false, donor_continuous: Some(true), dualfit_pass: None },
            Class::Unfillable => ExpectAxes { bracket_exhausted: false, program_quiet_skip: false, donor_continuous: None, dualfit_pass: None },
        }
    }

    // --- fixture builders -------------------------------------------------------------------------------

    fn donor(silence_fraction: f64, continuous: bool) -> DonorInterior {
        DonorInterior { rms_db: -20.0, silence_fraction, longest_silence_ms: 0.0, continuous }
    }

    fn seam(gate_pass: bool, post_seam_r: f64, post_seam_global_r: f64) -> SeamLocalTags {
        SeamLocalTags {
            pre_seam_r: 0.96, post_seam_r, post_seam_global_r,
            trim_frames: 240, gate_pass, pre_lag: 4, post_lag: -4,
            pre_seam_prom: None, post_seam_prom: None, pre_seam_z: None, post_seam_z: None,
        }
    }

    fn gate(total: usize, passing: usize, closest: Option<&str>) -> GateTags {
        GateTags {
            brackets_total: total,
            brackets_passing: passing,
            closest_failure_stage: closest.map(str::to_string),
            structure_min: Some(0.7),
            seam_min: Some(0.5),
            best_bracket_seam: (total > 0).then_some(0.6),
            residual: None,
        }
    }

    fn tags(
        gate: GateTags,
        seam_local: Option<SeamLocalTags>,
        donor_aligned: Option<DonorInterior>,
        donor_nominal: Option<DonorInterior>,
    ) -> GapRepairTags {
        GapRepairTags { gate, seam_local, donor_aligned, donor_nominal, ..GapRepairTags::default() }
    }

    fn spec(verdict: GapRepairVerdict, tags_ctx: GapRepairTags) -> GapRepairSpec {
        GapRepairSpec {
            gap_index: 0,
            a_start_secs: 5.0,
            a_end_secs: 5.4,
            gap_offset_secs: 0.1,
            refined: RefinedGapFrames { start_frame: 240_000, end_frame: 259_200 },
            b_extract: BExtractWindow { start_frame: 0, end_frame: 0, b_mapped_start_frame: 0 },
            crossfade_secs: 0.01,
            verdict,
            tags_ctx,
        }
    }

    fn corr_reason() -> GapPatchSkipReason {
        GapPatchSkipReason::CorrelationBelowThreshold {
            pre_correlation: 0.96, post_correlation: 0.95, min_correlation: 0.5, best_attempt: None,
        }
    }

    fn silence_splice_strategy() -> GapRepairStrategy {
        GapRepairStrategy::SilenceSplice {
            fill: Vec::new(),
            pre_seam_r: 0.96, post_seam_r: 0.95,
            pre_lag: 4, post_lag: -4, trim_frames: 240,
            residual: None, confidence: FillConfidence::High,
        }
    }

    fn bracket_strategy() -> GapRepairStrategy {
        GapRepairStrategy::Bracket {
            alignment: FillAlignment { start_frame: 100, fill_frames: 19_200, pre_correlation: 0.9, post_correlation: 0.9 },
            structure_start_frame: 100,
            structure_trusted: true,
            anchor_seam_used: false,
            anchor_bracket_move_frames: 0,
            anchor_trusted: false,
            seam_pre: 0.9, seam_post: 0.9, used_splice: false,
            confidence: FillConfidence::High,
            gap_start_adjust_frames: 0, gap_end_adjust_frames: 0,
            fit_used_boundary_grid: false, fit_boundary_grid_cells: None,
            residual: None, normalize_gain: 1.0,
        }
    }

    /// Build a synthetic spec whose stored tags realize `class`'s signature (production default `dual_fit` on,
    /// so a Silence-splice is emitted as `Patch(SilenceSplice)`).
    fn spec_for_class(class: Class) -> GapRepairSpec {
        let good_seam = || seam(true, 0.95, 0.40); // gate_pass + step-real (0.95 − 0.40 ≥ 0.15)
        match class {
            Class::NoPlacement => spec(
                GapRepairVerdict::skip_with_cell(GapRepairCell::NoPlacement, GapPatchSkipReason::BoundaryAlignmentFailed),
                tags(gate(0, 0, None), None, None, None),
            ),
            Class::ProgramQuietEarly => spec(
                GapRepairVerdict::skip_with_cell(GapRepairCell::ProgramQuiet, GapPatchSkipReason::ProgramQuiet),
                tags(gate(0, 0, None), None, None, Some(donor(0.9, false))),
            ),
            Class::SilenceSplice => spec(
                GapRepairVerdict::Patch(silence_splice_strategy()),
                tags(gate(4, 0, Some("waveform_floor")), Some(good_seam()), Some(donor(0.05, true)), Some(donor(0.10, true))),
            ),
            Class::DonorAlignedDecline => spec(
                GapRepairVerdict::skip_with_cell(GapRepairCell::ProgramQuiet, GapPatchSkipReason::ProgramQuiet),
                tags(gate(4, 0, Some("waveform_floor")), Some(good_seam()), Some(donor(0.6, false)), Some(donor(0.10, true))),
            ),
            Class::ProgramQuietExtreme => spec(
                GapRepairVerdict::skip_with_cell(GapRepairCell::ProgramQuiet, GapPatchSkipReason::ProgramQuiet),
                tags(gate(4, 0, Some("waveform_floor")), Some(good_seam()), Some(donor(0.6, false)), Some(donor(0.95, false))),
            ),
            Class::BracketPatch => spec(
                GapRepairVerdict::Patch(bracket_strategy()),
                tags(gate(2, 1, Some("waveform_floor")), None, None, None),
            ),
            Class::Decorrelated => spec(
                GapRepairVerdict::skip_with_cell(GapRepairCell::Decorrelated, corr_reason()),
                tags(gate(4, 0, Some("waveform_floor")), Some(seam(false, 0.5, 0.45)), Some(donor(0.05, true)), Some(donor(0.05, true))),
            ),
            Class::ResidualVeto => spec(
                GapRepairVerdict::skip_with_cell(
                    GapRepairCell::ResidualVeto,
                    GapPatchSkipReason::ResidualHeadroomExceeded {
                        pre_correlation: 0.9, post_correlation: 0.9, headroom_db: 3.0,
                        floor_pre_db: -40.0, floor_post_db: -40.0, margin_db: 1.0,
                    },
                ),
                tags(gate(4, 0, Some("residual")), None, Some(donor(0.05, true)), Some(donor(0.05, true))),
            ),
            Class::Unfillable => spec(
                GapRepairVerdict::skip_with_cell(GapRepairCell::Unfillable, GapPatchSkipReason::BExtractFailed),
                tags(gate(0, 0, None), None, None, None),
            ),
        }
    }

    /// The production export projection under test: spec → `GapFingerprint` → serialize → frozen `gap_row`.
    fn project_to_row(spec: &GapRepairSpec) -> GapRow {
        let fp = clip_sync_repair::application::gap_fingerprint::spec_to_fingerprint_summary(spec, 48_000, 2, None, None);
        let gaps_json = serde_json::to_string(&[fp]).unwrap();
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("1");
        std::fs::create_dir_all(&dir).unwrap();
        let corpus = format!(
            r#"{{"source":{{"a_source":{{"id":"aaaa","sample_rate":48000,"channels":2,"duration_secs":60.0}},"b_source":{{"id":"bbbb","sample_rate":48000,"channels":2,"duration_secs":60.0}},"scan_recipe":{{"silence_peak_fraction":0.01}},"gap_count":1}},"gaps":{gaps_json}}}"#
        );
        std::fs::write(dir.join("corpus.json"), corpus).unwrap();
        let mut report = analyze_dirs(&[root.path().to_path_buf()], 1.0, 30.0);
        report.rows.remove(0)
    }

    /// Primary C4: cell identity (tier-independent) + Tier-1 axis projection faithfulness, all nine classes.
    #[test]
    fn gap_repair_spec_cell_identity_and_projection_per_class() {
        for class in ALL_CLASSES {
            let spec = spec_for_class(class);
            // (1) Cell identity — computed directly on the spec, independent of the projection/tier.
            assert_eq!(spec.cell(), expected_cell(class), "{class:?} cell identity");

            // (2) Projection faithfulness — through the frozen gap_row reader.
            let axes = expect_axes(class);
            let row = project_to_row(&spec);
            assert_eq!(row.bracket_exhausted(), axes.bracket_exhausted, "{class:?} bracket_exhausted");
            assert_eq!(row.donor_continuous, axes.donor_continuous, "{class:?} donor_continuous");
            assert_eq!(row.dualfit_pass, axes.dualfit_pass, "{class:?} dualfit_pass");
            assert_eq!(row.program_quiet_skip(), axes.program_quiet_skip, "{class:?} program_quiet_skip");
        }
    }

    /// The **gate-scalar** round-trip the golden baseline does NOT cover: `structure_min` / `seam_min` /
    /// `best_bracket_seam` / `closest_failure_stage` are read back through the corpus reader from the
    /// projection's synthesized brackets + structure/seams blocks. Pins the common case (a bracket reached seam
    /// scoring, so `best_bracket_seam` is `Some`); the all-pre-seam-failure case (`best = None`) is a known
    /// limitation — `closest_failure_stage` is then an arbitrary tie-break in the reader itself (see
    /// `synth_brackets`), so it is not asserted.
    #[test]
    fn projection_reproduces_gate_scalars() {
        // Decorrelated: gate(total=4, passing=0, closest="waveform_floor"); structure_min 0.7, seam_min 0.5,
        // best_bracket_seam 0.6 (from the `gate()` fixture helper).
        let row = project_to_row(&spec_for_class(Class::Decorrelated));
        assert_eq!(row.brackets_total, 4);
        assert_eq!(row.brackets_passing, 0);
        assert_eq!(row.structure_min, Some(0.7), "structure_min");
        assert_eq!(row.seam_min, Some(0.5), "seam_min");
        assert_eq!(row.best_bracket_seam, Some(0.6), "best_bracket_seam");
        assert_eq!(row.closest_failure_stage.as_deref(), Some("waveform_floor"), "closest_failure_stage");
    }

    /// A Silence-splice keeps its cell across the `dual_fit` flag; only the projections move. Guards against
    /// ever re-deriving the cell from the verdict/tier.
    #[test]
    fn silence_splice_cell_is_invariant_across_dual_fit_flag() {
        // dual_fit on → Patch(SilenceSplice).
        let patched = spec_for_class(Class::SilenceSplice);
        // dual_fit off → same axes, but declined: Skip { cell: SilenceSplice }.
        let declined = spec(
            GapRepairVerdict::skip_with_cell(GapRepairCell::SilenceSplice, corr_reason()),
            tags(gate(4, 0, Some("waveform_floor")), Some(seam(true, 0.95, 0.40)), Some(donor(0.05, true)), Some(donor(0.10, true))),
        );

        assert_eq!(patched.cell(), GapRepairCell::SilenceSplice);
        assert_eq!(declined.cell(), GapRepairCell::SilenceSplice);

        let rp = project_to_row(&patched);
        let rd = project_to_row(&declined);
        assert!(rp.patched() && !rp.dualfit_target(), "repaired: patch tier, no longer a pending target");
        assert!(!rd.patched() && rd.dualfit_target(), "declined: skip tier, still a pending dual-fit target");
    }
}
