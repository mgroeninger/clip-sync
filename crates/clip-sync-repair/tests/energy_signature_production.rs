//! Production-scale energy signature corpus: mode matrix (integration crate).

use std::time::Instant;

use clip_sync::SymphoniaMediaReader;
use clip_sync::testing::fakes::FakeProgressReporter;

use clip_sync_repair::application::PatchAudio;
use clip_sync_repair::domain::{GapPatchSkipReason, GapPatchStatus, GapSignatureMode};
use clip_sync_repair::infrastructure::config::RepairConfig;
use clip_sync_repair::test_support::energy_signature_fixtures::{
    build_f1_production, build_f1_production_at, build_f2_production, structure_slide_secs,
    EnergySignatureFixture,
};
use clip_sync_repair::test_support::energy_signature_production::{
    gap_report_from_energy_fixture, patch_request_from_repair, production_fit_weights_config,
    production_matrix_contexts, production_repair_config, scan_gaps_for_fixture,
};

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
