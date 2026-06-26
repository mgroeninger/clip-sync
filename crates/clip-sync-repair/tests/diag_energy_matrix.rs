//! Energy signature mode matrix / sweep diagnostics.
//!
//! Tier: **diagnostic** (`diagnostic-tests` feature). CSV export: fixture × `gap_signature_mode` ×
//! context — emit data for tuning, not CI assertions.
//!
//! PR: **no** — `.\scripts\test-tier.ps1 -Tier diagnostic -Package clip-sync-repair -Nocapture`.
//!
//! Run: `cargo test -p clip-sync-repair --features diagnostic-tests --test diag_energy_matrix -- --nocapture`

use std::time::Instant;

use clip_sync::SymphoniaMediaReader;
use clip_sync::testing::fakes::FakeProgressReporter;

use clip_sync_repair::application::PatchAudio;
use clip_sync_repair::domain::GapSignatureMode;
use clip_sync_repair::infrastructure::config::RepairConfig;
use clip_sync_repair::test_support::energy_signature_fixtures::{
    build_f1_production, build_f1_production_at, build_f2_production, build_f4_decoy_production,
    structure_slide_secs,
};
use clip_sync_repair::test_support::energy_signature_production::{
    gap_report_from_energy_fixture, patch_request_from_repair, production_fit_weights_config,
    production_repair_config, production_weight_sweep_config,
};
use clip_sync_repair_harness::energy_matrix::{
    format_slide, run_matrix_rows, run_oracle_control_row, run_oracle_matrix_rows,
};

#[test]
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

/// EC-6 weight sweep: where between structure isolation and production fit weights does the
/// energy/bool split survive? Varies `structure/waveform` weights and `nominal_bias` on F4.
/// Energy target slide ≈ +7 s (true pause); bool/masked ≈ 0 (decoy).

#[test]
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

