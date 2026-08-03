//! Shared matrix/oracle row runners for energy signature integration binaries.

use std::time::Instant;

use clip_sync::SymphoniaMediaReader;
use clip_sync_repair::application::PatchAudio;
use clip_sync_repair::domain::{GapPatchStatus, GapSignatureMode};
use clip_sync_repair::infrastructure::config::RepairConfig;
use clip_sync_repair_fixtures::energy_signature_fixtures::EnergySignatureFixture;
use clip_sync_repair_fixtures::energy_signature_production::{
    gap_report_from_energy_fixture, patch_request_from_repair, production_matrix_contexts,
    production_repair_config, scan_gaps_for_fixture,
};

pub fn run_oracle_control_row(
    patch: &PatchAudio<'_, SymphoniaMediaReader>,
    temp: &tempfile::TempDir,
    fixture_label: &str,
    fixture: &EnergySignatureFixture,
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
pub fn run_oracle_matrix_rows(
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

pub fn run_matrix_rows(
    patch: &PatchAudio<'_, SymphoniaMediaReader>,
    temp: &tempfile::TempDir,
    fixture_label: &str,
    source: &str,
    fixture: &EnergySignatureFixture,
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

pub fn format_slide(status: Option<&GapPatchStatus>) -> String {
    match status {
        Some(GapPatchStatus::Patched {
            align_adjustment_secs,
            ..
        }) => format!("{align_adjustment_secs:.3}"),
        _ => "n/a".into(),
    }
}

pub fn format_skip_reason(status: &GapPatchStatus) -> String {
    use clip_sync_repair::domain::GapPatchSkipReason;

    match status {
        GapPatchStatus::Skipped { reason } => match reason {
            GapPatchSkipReason::BoundaryAlignmentFailed => "boundary_alignment_failed".into(),
            GapPatchSkipReason::CorrelationBelowThreshold {
                pre_correlation,
                post_correlation,
                min_correlation,
                ..
            } => format!(
                "correlation_below_threshold(pre={pre_correlation},post={post_correlation},min={min_correlation})"
            ),
            GapPatchSkipReason::BExtractFailed => "b_extract_failed".into(),
            GapPatchSkipReason::AlignedSegmentOutOfRange => "aligned_segment_out_of_range".into(),
            GapPatchSkipReason::ZeroLengthGap => "zero_length_gap".into(),
            GapPatchSkipReason::ResidualHeadroomExceeded { headroom_db, .. } => {
                format!("residual_headroom_exceeded(headroom={headroom_db:.1})")
            }
            GapPatchSkipReason::ProgramQuiet => "program_quiet".into(),
        },
        GapPatchStatus::Patched { confidence, .. } => format!("patched({confidence:?})"),
        GapPatchStatus::NotPlanned { reason } => format!("not_planned({reason:?})"),
        GapPatchStatus::NotApplied { reason } => format!("not_applied({reason:?})"),
    }
}
