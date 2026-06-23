//! Production-scale energy signature corpus: mode matrix (integration crate).

use std::time::Instant;

use clip_sync::SymphoniaMediaReader;
use clip_sync::testing::fakes::FakeProgressReporter;

use clip_sync_repair::application::PatchAudio;
use clip_sync_repair::domain::{GapPatchSkipReason, GapPatchStatus, GapSignatureMode};
use clip_sync_repair::infrastructure::config::RepairConfig;
use clip_sync_repair::test_support::energy_signature_fixtures::{
    build_f1_production, build_f1_production_at, build_f2_production, build_f4_decoy_production,
    structure_slide_secs, EnergySignatureFixture,
};
use clip_sync_repair::test_support::energy_signature_production::{
    gap_report_from_energy_fixture, patch_request_from_repair, production_fit_weights_config,
    production_matrix_contexts, production_mode_coupled_config, production_repair_config,
    production_weight_sweep_config, scan_gaps_for_fixture,
};

/// **Non-ignored CI tripwire** for the corpus e2e path. The 48 kHz corpus patch tests below are
/// all `#[ignore]`d for cost, leaving `ScanGaps → refine → haystack → seam gates → splice`
/// uncovered on every PR. This runs the full scan→patch pipeline on a small low-rate production
/// F1 (16 kHz / 32 s, but production-shaped geometry: border 10 s, 50 ms bins) and asserts one gap
/// is detected and patched. Lives in the integration binary (not the lib suite) so it does not
/// contend with wall-clock-budget timing tests there; ~5 s.
#[test]
fn corpus_scan_patch_smoke() {
    let temp = tempfile::tempdir().expect("tempdir");
    let fixture = build_f1_production_at(16_000, 1, 32.0, 3.0);
    let repair = production_repair_config(GapSignatureMode::Energy, 3.0);

    let report = scan_gaps_for_fixture(&fixture, temp.path());
    assert_eq!(
        report.gaps.iter().filter(|g| g.is_fillable()).count(),
        1,
        "smoke: scan should find one fillable gap: {:#?}",
        report.gaps,
    );

    let patch = PatchAudio::new(&SymphoniaMediaReader, &FakeProgressReporter);
    let result = patch
        .execute(
            patch_request_from_repair(report, &repair),
            RepairConfig::default().crossfade_ms,
        )
        .expect("smoke scan→patch");
    assert_eq!(
        result.summary.patched_count, 1,
        "corpus smoke should patch one gap: {:?}",
        result.summary.gaps,
    );
}

#[test]
#[ignore = "matrix: cargo test -p clip-sync-repair energy_signature_mode_matrix -- --ignored --nocapture"]
fn energy_signature_mode_matrix() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repair_defaults = RepairConfig::default();
    let patch = PatchAudio::new(&SymphoniaMediaReader, &FakeProgressReporter);

    eprintln!("fixture,source,mode,context_secs,patched,skipped,marginal,wall_ms,slide_secs,skip_reason");

    let fixture = build_f1_production(48_000, 2, 3.0);
    run_oracle_control_row(&patch, &temp, "F1-long", &fixture, 3.0, &repair_defaults);

    run_matrix_rows(
        &patch,
        &temp,
        "F1-long",
        "scan_derived",
        &fixture,
        60.0,
        &repair_defaults,
    );

    let fixture_120 = build_f1_production_at(48_000, 2, 120.0, 30.0);
    run_oracle_control_row(
        &patch,
        &temp,
        "F1-long-120s",
        &fixture_120,
        30.0,
        &repair_defaults,
    );
    run_matrix_rows(
        &patch,
        &temp,
        "F1-long-120s",
        "scan_derived",
        &fixture_120,
        120.0,
        &repair_defaults,
    );

    // F2-long: two pauses, nominal B map at decoy pause₂. Real scan can't detect a
    // fillable gap (B is silent at A's pause₁), so the oracle-injected report path
    // supplies the pause₂ nominal map. Energy should slide back to pause₁; bool is
    // ambiguous between the two pauses — this is the EC-6 discriminator. Context is
    // pinned to 3 s: the pause spacing scales with context, and the pause₂→pause₁
    // slide must stay inside `fill_border_search_secs = 10`.
    let fixture_f2 = build_f2_production(48_000, 2, 90.0, 3.0);
    // Structure-isolated config: structure tier + `snap_fill_to_gap` drive placement.
    run_oracle_matrix_rows(
        &patch,
        &temp,
        "F2-long",
        &fixture_f2,
        &[3.0],
        production_repair_config,
        &repair_defaults,
    );
    // EC-6 follow-up: production fit weights (0.35/0.65) + nominal bias, so the waveform
    // tier (not `snap_fill_to_gap`) decides placement. Records whether modes diverge.
    run_oracle_matrix_rows(
        &patch,
        &temp,
        "F2-long-prodw",
        &fixture_f2,
        &[3.0],
        production_fit_weights_config,
        &repair_defaults,
    );

    // F4-decoy: the EC-6 discriminator. Under structure isolation, energy/auto slide to the
    // true pause (≈ +7 s) while bool stays at the decoy nominal (≈ 0). (Production-weights
    // rows are in `f4_decoy_patch_diagnostic`; they mask the split and cost ~270 s each.)
    let fixture_f4 = build_f4_decoy_production(48_000, 2, 90.0, 3.0);
    run_oracle_matrix_rows(
        &patch,
        &temp,
        "F4-decoy",
        &fixture_f4,
        &[3.0],
        production_repair_config,
        &repair_defaults,
    );
}

#[test]
#[ignore = "smoke: cargo test -p clip-sync-repair f1_production_scan_patch_smoke -- --ignored --nocapture"]
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

#[test]
#[ignore = "control: cargo test -p clip-sync-repair f1_production_oracle_patch_control -- --ignored --nocapture"]
fn f1_production_oracle_patch_control() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repair_defaults = RepairConfig::default();
    let patch = PatchAudio::new(&SymphoniaMediaReader, &FakeProgressReporter);
    let fixture = build_f1_production(48_000, 2, 3.0);
    let repair = production_repair_config(GapSignatureMode::Energy, 3.0);
    let report = gap_report_from_energy_fixture(temp.path(), &fixture);
    let result = patch
        .execute(
            patch_request_from_repair(report, &repair),
            repair_defaults.crossfade_ms,
        )
        .expect("oracle patch");
    eprintln!(
        "oracle control: patched={} skipped={} status={:?}",
        result.summary.patched_count,
        result.summary.skipped_count,
        result.summary.gaps.first().map(|g| &g.status),
    );
    assert_eq!(
        result.summary.patched_count, 1,
        "oracle control should patch F1-long with production config"
    );
}

#[test]
#[ignore = "smoke: cargo test -p clip-sync-repair f2_production_oracle_patch_smoke -- --ignored --nocapture"]
fn f2_production_oracle_patch_smoke() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repair_defaults = RepairConfig::default();
    let patch = PatchAudio::new(&SymphoniaMediaReader, &FakeProgressReporter);
    let fixture = build_f2_production(48_000, 2, 90.0, 3.0);
    let repair = production_repair_config(GapSignatureMode::Energy, 3.0);
    let report = gap_report_from_energy_fixture(temp.path(), &fixture);
    let result = patch
        .execute(
            patch_request_from_repair(report, &repair),
            repair_defaults.crossfade_ms,
        )
        .expect("F2-long oracle patch");
    assert_eq!(
        result.summary.patched_count, 1,
        "F2-long energy should patch pause₁: {:?}",
        result.summary.gaps,
    );
    // `align_adjustment_secs` is measured from the A-aligned nominal (pause₁ at zero
    // offset), so a correct pause₁ placement reads ≈ 0; landing on the decoy pause₂
    // would read ≈ +(gap + bridge). Assert pause₁ (slide ≈ 0), mirroring I3.
    let actual_slide = match &result.summary.gaps[0].status {
        GapPatchStatus::Patched {
            align_adjustment_secs,
            ..
        } => *align_adjustment_secs,
        other => panic!("expected patched, got {other:?}"),
    };
    let pause2_offset = structure_slide_secs(&fixture, fixture.true_fill_start).abs();
    assert!(
        actual_slide.abs() < pause2_offset / 2.0,
        "F2-long energy slide {actual_slide:.3}s should sit near pause₁ (≈0), \
         not the decoy pause₂ (≈{pause2_offset:.3}s)",
    );
    eprintln!("F2-long energy slide: {actual_slide:.3}s (≈0 = pause₁)");
}

#[test]
#[ignore = "diagnostic: cargo test -p clip-sync-repair f2_production_weights_diagnostic -- --ignored --nocapture"]
fn f2_production_weights_diagnostic() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repair_defaults = RepairConfig::default();
    let patch = PatchAudio::new(&SymphoniaMediaReader, &FakeProgressReporter);
    let fixture = build_f2_production(48_000, 2, 90.0, 3.0);

    eprintln!("fixture,source,mode,context_secs,patched,skipped,marginal,wall_ms,slide_secs,skip_reason");
    run_oracle_matrix_rows(
        &patch,
        &temp,
        "F2-long",
        &fixture,
        &[3.0],
        production_repair_config,
        &repair_defaults,
    );
    run_oracle_matrix_rows(
        &patch,
        &temp,
        "F2-long-prodw",
        &fixture,
        &[3.0],
        production_fit_weights_config,
        &repair_defaults,
    );
}

/// **EC-6 (patch layer):** on the F4 decoy fixture under the structure-isolated corpus config,
/// `energy`/`auto` slide to the true pause while `bool` stays at the decoy nominal — the
/// bool-vs-energy split survives the full patch path. Ignored: ~36 s per patch in debug.
#[test]
#[ignore = "ec6: cargo test -p clip-sync-repair f4_decoy_patch_discrimination -- --ignored --nocapture"]
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
#[ignore = "ec6: cargo test -p clip-sync-repair f4_decoy_mode_coupled_bias -- --ignored --nocapture"]
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
#[ignore = "ec6: cargo test -p clip-sync-repair f4_decoy_energy_recovers_at_low_bias -- --ignored --nocapture"]
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

/// EC-6 weight sweep: where between structure isolation and production fit weights does the
/// energy/bool split survive? Varies `structure/waveform` weights and `nominal_bias` on F4.
/// Energy target slide ≈ +7 s (true pause); bool/masked ≈ 0 (decoy).
#[test]
#[ignore = "sweep: cargo test -p clip-sync-repair f4_decoy_weight_sweep -- --ignored --nocapture"]
fn f4_decoy_weight_sweep() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repair_defaults = RepairConfig::default();
    let patch = PatchAudio::new(&SymphoniaMediaReader, &FakeProgressReporter);
    let fixture = build_f4_decoy_production(48_000, 2, 90.0, 3.0);
    let truth_slide = structure_slide_secs(&fixture, fixture.true_fill_start);
    eprintln!("F4 weight sweep: truth slide ≈ {truth_slide:.3}s (energy target), decoy ≈ 0");
    eprintln!("mode,structure_w,waveform_w,nominal_bias,patched,slide_secs,wall_ms");

    // (mode, structure_w, waveform_w, nominal_bias)
    let grid = [
        (GapSignatureMode::Energy, 1.00, 0.00, 0.0), // control: structure isolation
        (GapSignatureMode::Energy, 0.35, 0.65, 0.0), // production weights, no bias
        (GapSignatureMode::Energy, 0.35, 0.65, 0.5), // production weights, half bias
        (GapSignatureMode::Energy, 0.35, 0.65, 1.0), // full production: masked baseline
        (GapSignatureMode::Energy, 0.65, 0.35, 1.0), // structure-leaning + full bias
        (GapSignatureMode::Energy, 0.50, 0.50, 1.0), // balanced + full bias
        (GapSignatureMode::Bool, 1.00, 0.00, 0.0),   // control: bool should stay at decoy
        (GapSignatureMode::Bool, 0.35, 0.65, 0.0),   // bool with no bias
    ];

    for (mode, sw, ww, bias) in grid {
        let repair = production_weight_sweep_config(mode, 3.0, sw, ww, bias);
        let report = gap_report_from_energy_fixture(temp.path(), &fixture);
        let started = Instant::now();
        let result = patch
            .execute(
                patch_request_from_repair(report, &repair),
                repair_defaults.crossfade_ms,
            )
            .expect("F4 weight sweep patch");
        let first = result.summary.gaps.first();
        eprintln!(
            "{mode:?},{sw},{ww},{bias},{},{},{}",
            result.summary.patched_count,
            format_slide(first.map(|g| &g.status)),
            started.elapsed().as_millis(),
        );
    }
}

/// EC-6 bias boundary: the weight sweep showed `nominal_bias` (not waveform weight) masks the
/// energy/bool split. Pin the threshold between 0 and 0.5 at production weights (0.35/0.65).
#[test]
#[ignore = "sweep: cargo test -p clip-sync-repair f4_decoy_bias_boundary -- --ignored --nocapture"]
fn f4_decoy_bias_boundary() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repair_defaults = RepairConfig::default();
    let patch = PatchAudio::new(&SymphoniaMediaReader, &FakeProgressReporter);
    let fixture = build_f4_decoy_production(48_000, 2, 90.0, 3.0);
    let truth_slide = structure_slide_secs(&fixture, fixture.true_fill_start);
    eprintln!("F4 bias boundary (Energy, 0.35/0.65): truth ≈ {truth_slide:.3}s, decoy ≈ 0");
    eprintln!("nominal_bias,patched,slide_secs,wall_ms");

    for bias in [0.1, 0.2, 0.25, 0.35] {
        let repair = production_weight_sweep_config(GapSignatureMode::Energy, 3.0, 0.35, 0.65, bias);
        let report = gap_report_from_energy_fixture(temp.path(), &fixture);
        let started = Instant::now();
        let result = patch
            .execute(
                patch_request_from_repair(report, &repair),
                repair_defaults.crossfade_ms,
            )
            .expect("F4 bias boundary patch");
        let first = result.summary.gaps.first();
        eprintln!(
            "{bias},{},{},{}",
            result.summary.patched_count,
            format_slide(first.map(|g| &g.status)),
            started.elapsed().as_millis(),
        );
    }
}

#[test]
#[ignore = "diagnostic: cargo test -p clip-sync-repair f4_decoy_patch_diagnostic -- --ignored --nocapture"]
fn f4_decoy_patch_diagnostic() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repair_defaults = RepairConfig::default();
    let patch = PatchAudio::new(&SymphoniaMediaReader, &FakeProgressReporter);
    let fixture = build_f4_decoy_production(48_000, 2, 90.0, 3.0);
    let truth_slide = structure_slide_secs(&fixture, fixture.true_fill_start);
    eprintln!("F4 decoy: truth slide ≈ {truth_slide:.3}s (energy target), decoy slide ≈ 0 (bool target)");

    eprintln!("fixture,source,mode,context_secs,patched,skipped,marginal,wall_ms,slide_secs,skip_reason");
    run_oracle_matrix_rows(
        &patch,
        &temp,
        "F4-decoy",
        &fixture,
        &[3.0],
        production_repair_config,
        &repair_defaults,
    );
    run_oracle_matrix_rows(
        &patch,
        &temp,
        "F4-decoy-prodw",
        &fixture,
        &[3.0],
        production_fit_weights_config,
        &repair_defaults,
    );
}

fn run_oracle_control_row(
    patch: &PatchAudio<'_, SymphoniaMediaReader>,
    temp: &tempfile::TempDir,
    fixture_label: &str,
    fixture: &clip_sync_repair::test_support::energy_signature_fixtures::EnergySignatureFixture,
    context_secs: f64,
    repair_defaults: &RepairConfig,
) {
    let repair = production_repair_config(GapSignatureMode::Energy, context_secs);
    let report = gap_report_from_energy_fixture(temp.path(), fixture);
    let started = Instant::now();
    let result = patch
        .execute(
            patch_request_from_repair(report, &repair),
            repair_defaults.crossfade_ms,
        )
        .expect("oracle control patch");
    let first = result.summary.gaps.first();
    let skip_reason = first
        .map(|g| format_skip_reason(&g.status))
        .unwrap_or_else(|| "no_gap".into());
    eprintln!(
        "{fixture_label},oracle_injected,Energy,{context_secs},{},{},{},{},{},{}",
        result.summary.patched_count,
        result.summary.skipped_count,
        result.summary.patched_marginal_count,
        started.elapsed().as_millis(),
        format_slide(first.map(|g| &g.status)),
        skip_reason,
    );
}

/// Oracle-injected matrix: modes × contexts via [`gap_report_from_energy_fixture`].
///
/// Used for fixtures whose true fill cannot be reached by real scan (e.g. F2-long,
/// where B is silent at A's gap so the nominal map must be injected at the decoy pause₂).
fn run_oracle_matrix_rows(
    patch: &PatchAudio<'_, SymphoniaMediaReader>,
    temp: &tempfile::TempDir,
    fixture_label: &str,
    fixture: &EnergySignatureFixture,
    contexts: &[f64],
    config: fn(GapSignatureMode, f64) -> RepairConfig,
    repair_defaults: &RepairConfig,
) {
    for mode in [
        GapSignatureMode::Bool,
        GapSignatureMode::Energy,
        GapSignatureMode::Auto,
    ] {
        for &context in contexts {
            let repair = config(mode, context);
            let report = gap_report_from_energy_fixture(temp.path(), fixture);
            let request = patch_request_from_repair(report, &repair);
            let started = Instant::now();
            let result = patch
                .execute(request, repair_defaults.crossfade_ms)
                .expect("oracle matrix patch");
            let first = result.summary.gaps.first();
            let skip_reason = first
                .map(|g| format_skip_reason(&g.status))
                .unwrap_or_else(|| "no_gap".into());
            eprintln!(
                "{fixture_label},oracle_injected,{mode:?},{context},{},{},{},{},{},{}",
                result.summary.patched_count,
                result.summary.skipped_count,
                result.summary.patched_marginal_count,
                started.elapsed().as_millis(),
                format_slide(first.map(|g| &g.status)),
                skip_reason,
            );
        }
    }
}

fn run_matrix_rows(
    patch: &PatchAudio<'_, SymphoniaMediaReader>,
    temp: &tempfile::TempDir,
    fixture_label: &str,
    source: &str,
    fixture: &clip_sync_repair::test_support::energy_signature_fixtures::EnergySignatureFixture,
    total_secs: f64,
    repair_defaults: &RepairConfig,
) {
    let report = scan_gaps_for_fixture(fixture, temp.path());
    let contexts = production_matrix_contexts(total_secs);
    assert!(
        !contexts.is_empty(),
        "{fixture_label}: no valid matrix contexts for {total_secs}s fixture"
    );

    for mode in [
        GapSignatureMode::Bool,
        GapSignatureMode::Energy,
        GapSignatureMode::Auto,
    ] {
        for &context in &contexts {
            let repair = production_repair_config(mode, context);
            let request = patch_request_from_repair(report.clone(), &repair);
            let started = Instant::now();
            let result = patch
                .execute(request, repair_defaults.crossfade_ms)
                .expect("matrix patch");
            let first = result.summary.gaps.first();
            let skip_reason = first
                .map(|g| format_skip_reason(&g.status))
                .unwrap_or_else(|| "no_gap".into());
            eprintln!(
                "{fixture_label},{source},{mode:?},{context},{},{},{},{},{},{}",
                result.summary.patched_count,
                result.summary.skipped_count,
                result.summary.patched_marginal_count,
                started.elapsed().as_millis(),
                format_slide(first.map(|g| &g.status)),
                skip_reason,
            );
        }
    }
}

/// Total B slide from the mapped nominal for a patched gap (`n/a` when not patched).
fn format_slide(status: Option<&GapPatchStatus>) -> String {
    match status {
        Some(GapPatchStatus::Patched {
            align_adjustment_secs,
            ..
        }) => format!("{align_adjustment_secs:.3}"),
        _ => "n/a".into(),
    }
}

fn format_skip_reason(status: &GapPatchStatus) -> String {
    match status {
        GapPatchStatus::Skipped { reason } => match reason {
            GapPatchSkipReason::BoundaryAlignmentFailed => "boundary_alignment_failed".into(),
            GapPatchSkipReason::CorrelationBelowThreshold {
                pre_correlation,
                post_correlation,
                min_correlation,
            } => format!(
                "correlation_below_threshold(pre={pre_correlation},post={post_correlation},min={min_correlation})"
            ),
            GapPatchSkipReason::BExtractFailed => "b_extract_failed".into(),
            GapPatchSkipReason::AlignedSegmentOutOfRange => "aligned_segment_out_of_range".into(),
            GapPatchSkipReason::ZeroLengthGap => "zero_length_gap".into(),
        },
        GapPatchStatus::Patched { confidence, .. } => format!("patched({confidence:?})"),
        GapPatchStatus::NotPlanned { reason } => format!("not_planned({reason:?})"),
    }
}
