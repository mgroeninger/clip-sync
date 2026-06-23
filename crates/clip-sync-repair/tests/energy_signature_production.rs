//! Production-scale energy signature corpus: mode matrix (integration crate).

use std::time::Instant;

use clip_sync::SymphoniaMediaReader;
use clip_sync::testing::fakes::FakeProgressReporter;

use clip_sync_repair::application::PatchAudio;
use clip_sync_repair::domain::{GapPatchSkipReason, GapPatchStatus, GapSignatureMode};
use clip_sync_repair::infrastructure::config::RepairConfig;
use clip_sync_repair::test_support::energy_signature_fixtures::{
    build_f1_production, build_f1_production_at,
};
use clip_sync_repair::test_support::energy_signature_production::{
    gap_report_from_energy_fixture, normalize_scan_gap_b_mapping, patch_request_from_repair,
    production_matrix_contexts, production_repair_config, scan_gaps_for_fixture,
};

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
#[ignore = "control: cargo test -p clip-sync-repair f1_production_scan_patch_control -- --ignored --nocapture"]
fn f1_production_scan_patch_control() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repair_defaults = RepairConfig::default();
    let patch = PatchAudio::new(&SymphoniaMediaReader, &FakeProgressReporter);
    let fixture = build_f1_production(48_000, 2, 3.0);
    let repair = production_repair_config(GapSignatureMode::Energy, 3.0);

    let raw_report = scan_gaps_for_fixture(&fixture, temp.path());
    let raw_result = patch
        .execute(
            patch_request_from_repair(raw_report.clone(), &repair),
            repair_defaults.crossfade_ms,
        )
        .expect("raw scan patch");
    eprintln!(
        "scan raw: patched={} skipped={} status={:?}",
        raw_result.summary.patched_count,
        raw_result.summary.skipped_count,
        raw_result.summary.gaps.first().map(|g| &g.status),
    );

    let mut normalized = raw_report;
    normalize_scan_gap_b_mapping(&mut normalized, &fixture);
    let norm_result = patch
        .execute(
            patch_request_from_repair(normalized, &repair),
            repair_defaults.crossfade_ms,
        )
        .expect("normalized scan patch");
    eprintln!(
        "scan normalized: patched={} skipped={} status={:?}",
        norm_result.summary.patched_count,
        norm_result.summary.skipped_count,
        norm_result.summary.gaps.first().map(|g| &g.status),
    );
}

#[test]
#[ignore = "matrix: cargo test -p clip-sync-repair energy_signature_mode_matrix -- --ignored --nocapture"]
fn energy_signature_mode_matrix() {
    let temp = tempfile::tempdir().expect("tempdir");
    let repair_defaults = RepairConfig::default();
    let patch = PatchAudio::new(&SymphoniaMediaReader, &FakeProgressReporter);

    eprintln!("fixture,mode,context_secs,patched,skipped,marginal,wall_ms,skip_reason");

    run_matrix_rows(
        &patch,
        &temp,
        "F1-long",
        &build_f1_production(48_000, 2, 3.0),
        60.0,
        &repair_defaults,
    );

    // Context 30 requires ≥ ~83 s tail room; 120 s fixture only.
    let fixture_120 = build_f1_production_at(48_000, 2, 120.0, 30.0);
    run_matrix_rows(
        &patch,
        &temp,
        "F1-long-120s",
        &fixture_120,
        120.0,
        &repair_defaults,
    );
}

fn run_matrix_rows(
    patch: &PatchAudio<'_, SymphoniaMediaReader>,
    temp: &tempfile::TempDir,
    fixture_label: &str,
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
        for context in &contexts {
            let repair = production_repair_config(mode, *context);
            let request = patch_request_from_repair(report.clone(), &repair);
            let started = Instant::now();
            let result = patch
                .execute(request, repair_defaults.crossfade_ms)
                .expect("matrix patch");
            let skip_reason = result
                .summary
                .gaps
                .first()
                .map(|g| format_skip_reason(&g.status))
                .unwrap_or_else(|| "no_gap".into());
            eprintln!(
                "{fixture_label},{mode:?},{context},{},{},{},{},{}",
                result.summary.patched_count,
                result.summary.skipped_count,
                result.summary.patched_marginal_count,
                started.elapsed().as_millis(),
                skip_reason,
            );
        }
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
