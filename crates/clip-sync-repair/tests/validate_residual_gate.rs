//! Residual gate validation — **RG** catalog + **EC06** patch rows.
//!
//! Tier: **validation** (`validation-tests` feature). Exhaustive gate contract from
//! `tests/residual_gate_catalog/matrix.toml` and F4-decoy patch discrimination smokes.
//!
//! PR: **no** — `.\scripts\test-tier.ps1 -Tier validation -Package clip-sync-repair`.
//!
//! Run: `cargo test -p clip-sync-repair --features validation-tests --test validate_residual_gate`

use clip_sync::testing::fakes::FakeProgressReporter;
use clip_sync::SymphoniaMediaReader;

use clip_sync_repair::application::PatchAudio;
use clip_sync_repair::domain::residual_gate::DEFAULT_RESIDUAL_HEADROOM_MARGIN_DB;
use clip_sync_repair::domain::{
    gap_tags::ResidualBand, GapPatchStatus, GapSignatureMode, ResidualGateMode,
};
use clip_sync_repair::infrastructure::config::RepairConfig;
use clip_sync_repair_fixtures::energy_signature_fixtures::{
    build_f1_production, build_f4_decoy_production, structure_slide_secs,
};
use clip_sync_repair_fixtures::energy_signature_production::{
    gap_report_from_energy_fixture, patch_request_from_repair, production_mode_coupled_config,
    production_repair_config, production_weight_sweep_config, scan_gaps_for_fixture,
};

#[test]
fn f1_production_scan_patch_smoke() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repair_defaults = RepairConfig::default();
    let patch = PatchAudio::new(&SymphoniaMediaReader, &FakeProgressReporter);
    let fixture = build_f1_production(48_000, 2, 3.0);
    let repair = production_repair_config(GapSignatureMode::Energy, 3.0);
    let report = scan_gaps_for_fixture(&fixture, temp.path());
    let result = patch
        .execute(
            patch_request_from_repair(report, &repair),
            repair_defaults.crossfade_ms,
        )
        .expect("scan patch smoke");
    assert_eq!(
        result.summary.patched_count, 1,
        "F1-long scan→patch smoke: {:?}",
        result.summary.gaps,
    );
}

/// **EC-6 + residual veto (pipeline):** bool mode lands on the F4 decoy nominal; with
/// `residual_gate = veto` the gate **abstains** (nominal floor anchor → headroom ≈ 0 at decoy) —
/// not a residual skip. Score-level F4 veto with truth-anchored floor:
/// `f4_decoy_placement_informative_with_high_headroom` / `seam_residual_disagreement_oracles`.
/// Energy mode still patches the true pause under veto. Ignored: ~72 s in debug.

#[test]
fn f4_decoy_residual_gate_vetoes_bool() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repair_defaults = RepairConfig::default();
    let patch = PatchAudio::new(&SymphoniaMediaReader, &FakeProgressReporter);
    let fixture = build_f4_decoy_production(48_000, 2, 90.0, 3.0);
    let truth_slide = structure_slide_secs(&fixture, fixture.true_fill_start);

    let mut bool_repair = production_repair_config(GapSignatureMode::Bool, 3.0);
    bool_repair.residual_gate = ResidualGateMode::Veto;
    let bool_report = gap_report_from_energy_fixture(temp.path(), &fixture);
    let bool_result = patch
        .execute(
            patch_request_from_repair(bool_report, &bool_repair),
            repair_defaults.crossfade_ms,
        )
        .expect("F4 bool+veto patch");
    assert_eq!(
        bool_result.summary.patched_count, 1,
        "bool+veto: nominal-floor headroom ≈ 0 at decoy → Pearson decides (patch): {:?}",
        bool_result.summary.gaps,
    );
    let bool_gap = &bool_result.summary.gaps[0];
    match &bool_gap.status {
        GapPatchStatus::Patched {
            align_adjustment_secs,
            headroom_db,
            ..
        } => {
            assert!(
                align_adjustment_secs.abs() <= 0.05,
                "bool should stay at decoy nominal (slide {align_adjustment_secs:.3}s)",
            );
            let headroom = headroom_db
                .or_else(|| bool_gap.residual.map(|v| v.worst_headroom_db()))
                .expect("residual measured under veto");
            assert!(
                headroom <= DEFAULT_RESIDUAL_HEADROOM_MARGIN_DB,
                "bool+veto headroom {headroom:.1} dB should be within margin (abstain, not veto)",
            );
        }
        other => panic!("bool+veto: expected patched decoy, got {other:?}"),
    }
    assert_eq!(
        bool_gap.tags.residual_band,
        Some(ResidualBand::Cancels),
        "informative floor + low headroom at nominal decoy placement",
    );

    let mut energy_repair = production_repair_config(GapSignatureMode::Energy, 3.0);
    energy_repair.residual_gate = ResidualGateMode::Veto;
    let energy_report = gap_report_from_energy_fixture(temp.path(), &fixture);
    let energy_result = patch
        .execute(
            patch_request_from_repair(energy_report, &energy_repair),
            repair_defaults.crossfade_ms,
        )
        .expect("F4 energy+veto patch");
    assert_eq!(
        energy_result.summary.patched_count, 1,
        "energy+veto should still patch truth: {:?}",
        energy_result.summary.gaps,
    );
    match &energy_result.summary.gaps[0].status {
        GapPatchStatus::Patched {
            align_adjustment_secs,
            ..
        } => {
            let tol = 0.1;
            assert!(
                (align_adjustment_secs - truth_slide).abs() <= tol,
                "energy+veto slide {align_adjustment_secs:.3}s should reach truth ≈ {truth_slide:.3}s",
            );
        }
        other => panic!("energy+veto: expected patched, got {other:?}"),
    }
}

/// **EC-6 (patch layer):** on the F4 decoy fixture under the structure-isolated corpus config,
/// `energy`/`auto` slide to the true pause while `bool` stays at the decoy nominal — the
/// bool-vs-energy split survives the full patch path. Ignored: ~36 s per patch in debug.

#[test]
fn f4_decoy_patch_discrimination() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repair_defaults = RepairConfig::default();
    let patch = PatchAudio::new(&SymphoniaMediaReader, &FakeProgressReporter);
    let fixture = build_f4_decoy_production(48_000, 2, 90.0, 3.0);
    let truth_slide = structure_slide_secs(&fixture, fixture.true_fill_start);

    let slide_for = |mode: GapSignatureMode| -> f64 {
        let repair = production_repair_config(mode, 3.0);
        let report = gap_report_from_energy_fixture(temp.path(), &fixture);
        let result = patch
            .execute(
                patch_request_from_repair(report, &repair),
                repair_defaults.crossfade_ms,
            )
            .expect("F4 decoy patch");
        assert_eq!(
            result.summary.patched_count, 1,
            "F4 {mode:?} should patch: {:?}",
            result.summary.gaps,
        );
        match &result.summary.gaps[0].status {
            GapPatchStatus::Patched {
                align_adjustment_secs,
                ..
            } => *align_adjustment_secs,
            other => panic!("F4 {mode:?}: expected patched, got {other:?}"),
        }
    };

    let tol = 0.1; // ~2 × 50 ms bins
    let energy_slide = slide_for(GapSignatureMode::Energy);
    let bool_slide = slide_for(GapSignatureMode::Bool);
    eprintln!(
        "F4 EC-6: energy slide {energy_slide:.3}s (truth ≈ {truth_slide:.3}s), bool slide {bool_slide:.3}s (decoy ≈ 0)"
    );

    assert!(
        (energy_slide - truth_slide).abs() <= tol,
        "energy slide {energy_slide:.3}s should reach the true pause ≈ {truth_slide:.3}s",
    );
    assert!(
        bool_slide.abs() <= tol,
        "bool slide {bool_slide:.3}s should stay at the decoy nominal (≈ 0)",
    );
    assert!(
        (energy_slide - bool_slide).abs() > truth_slide.abs() / 2.0,
        "energy and bool placements must diverge ({energy_slide:.3}s vs {bool_slide:.3}s)",
    );
}

/// **Mode-coupled nominal bias (parent plan Phase 4):** with production fit weights and the
/// **base** nominal bias at the default `1.0`, the lowered energy bias (`0.25`) lets `energy`
/// resolve the true pause while `bool` — still on the base `1.0` — stays at the decoy. Proves the
/// coupling un-masks the EC-6 split that full production weights otherwise hide.

#[test]
fn f4_decoy_mode_coupled_bias() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repair_defaults = RepairConfig::default();
    let patch = PatchAudio::new(&SymphoniaMediaReader, &FakeProgressReporter);
    let fixture = build_f4_decoy_production(48_000, 2, 90.0, 3.0);
    let truth_slide = structure_slide_secs(&fixture, fixture.true_fill_start);

    let slide_for = |mode: GapSignatureMode| -> f64 {
        let repair = production_mode_coupled_config(mode, 3.0);
        let report = gap_report_from_energy_fixture(temp.path(), &fixture);
        let result = patch
            .execute(
                patch_request_from_repair(report, &repair),
                repair_defaults.crossfade_ms,
            )
            .expect("F4 coupled-bias patch");
        assert_eq!(
            result.summary.patched_count, 1,
            "F4 {mode:?} should patch: {:?}",
            result.summary.gaps,
        );
        match &result.summary.gaps[0].status {
            GapPatchStatus::Patched {
                align_adjustment_secs,
                ..
            } => *align_adjustment_secs,
            other => panic!("F4 {mode:?}: expected patched, got {other:?}"),
        }
    };

    let energy_slide = slide_for(GapSignatureMode::Energy);
    let bool_slide = slide_for(GapSignatureMode::Bool);
    eprintln!(
        "F4 mode-coupled (base bias 1.0, energy 0.25): energy {energy_slide:.3}s (truth ≈ {truth_slide:.3}s), bool {bool_slide:.3}s (decoy ≈ 0)"
    );
    assert!(
        (energy_slide - truth_slide).abs() <= 0.1,
        "coupled energy should reach the true pause ({energy_slide:.3}s vs {truth_slide:.3}s)",
    );
    assert!(
        bool_slide.abs() <= 0.1,
        "bool (base bias 1.0) should stay at the decoy ({bool_slide:.3}s)",
    );
}

/// EC-6 tuning regression: energy recovers the true pause at **production fit weights**
/// (0.35/0.65) once `nominal_bias` is reduced to ≤ 0.25 — proving the signature helps under
/// realistic weights, and that the masking lever is `nominal_bias`, not the weight split.

#[test]
fn f4_decoy_energy_recovers_at_low_bias() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repair_defaults = RepairConfig::default();
    let patch = PatchAudio::new(&SymphoniaMediaReader, &FakeProgressReporter);
    let fixture = build_f4_decoy_production(48_000, 2, 90.0, 3.0);
    let truth_slide = structure_slide_secs(&fixture, fixture.true_fill_start);

    // Production fit weights (0.35 / 0.65) with nominal bias trimmed to the recovery boundary.
    let repair = production_weight_sweep_config(GapSignatureMode::Energy, 3.0, 0.35, 0.65, 0.25);
    let report = gap_report_from_energy_fixture(temp.path(), &fixture);
    let result = patch
        .execute(
            patch_request_from_repair(report, &repair),
            repair_defaults.crossfade_ms,
        )
        .expect("F4 low-bias patch");
    assert_eq!(
        result.summary.patched_count, 1,
        "F4 energy (production weights, bias 0.25) should patch: {:?}",
        result.summary.gaps,
    );
    let slide = match &result.summary.gaps[0].status {
        GapPatchStatus::Patched {
            align_adjustment_secs,
            ..
        } => *align_adjustment_secs,
        other => panic!("expected patched, got {other:?}"),
    };
    eprintln!("F4 energy @ 0.35/0.65 bias 0.25: slide {slide:.3}s (truth ≈ {truth_slide:.3}s)");
    assert!(
        (slide - truth_slide).abs() <= 0.1,
        "energy at production weights + bias 0.25 should still reach the true pause \
         ({slide:.3}s vs {truth_slide:.3}s)",
    );
}
