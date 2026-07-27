//! Residual gate off-regression baseline — **RG04**.
//!
//! Tier: **integration**. Asserts `residual_gate = off` is a true no-op vs on for F1/F4 production
//! fixtures. Catalog: `tests/residual_gate_catalog/matrix.toml`.
//!
//! PR: **yes** — `pr-repair`.
//!
//! Run: `cargo test -p clip-sync-repair --test integration_residual_gate_smoke`

use clip_sync::testing::fakes::FakeProgressReporter;
use clip_sync::SymphoniaMediaReader;

use clip_sync_repair::application::{PatchAudio, PatchAudioRequest, PatchAudioResult};
use clip_sync_repair::domain::{
    gap_tags::GapTags, GapPatchOutcome, GapPatchSkipReason, GapPatchStatus, GapSignatureMode,
    ResidualGateMode,
};
use clip_sync_repair::infrastructure::config::RepairConfig;
use clip_sync_repair_fixtures::energy_signature_fixtures::{
    build_f1_production_at, build_f4_decoy_production, EnergySignatureFixture,
};
use clip_sync_repair_fixtures::energy_signature_production::{
    gap_report_from_energy_fixture, patch_request_from_repair, production_repair_config,
};

/// Gating-relevant [`GapPatchStatus`] — strips residual measurement scalars that may appear when
/// `measure_residual = true` but must not affect patch/skip decisions under `residual_gate = off`.
fn gap_status_gating_core(status: &GapPatchStatus) -> GapPatchStatus {
    match status {
        GapPatchStatus::Patched {
            pre_correlation,
            post_correlation,
            align_adjustment_secs,
            waveform_adjustment_secs,
            structure_trusted,
            confidence,
            gap_start_adjust_frames,
            gap_end_adjust_frames,
            anchor_seam_used,
            anchor_bracket_move_frames,
            ..
        } => GapPatchStatus::Patched {
            pre_correlation: *pre_correlation,
            post_correlation: *post_correlation,
            align_adjustment_secs: *align_adjustment_secs,
            waveform_adjustment_secs: *waveform_adjustment_secs,
            structure_trusted: *structure_trusted,
            confidence: *confidence,
            gap_start_adjust_frames: *gap_start_adjust_frames,
            gap_end_adjust_frames: *gap_end_adjust_frames,
            residual_db: None,
            floor_db: None,
            headroom_db: None,
            anchor_seam_used: *anchor_seam_used,
            anchor_bracket_move_frames: *anchor_bracket_move_frames,
            dual_fit_used: false,
        },
        other => other.clone(),
    }
}

fn gap_tags_gating_core(tags: &GapTags) -> GapTags {
    let mut t = tags.clone();
    t.residual_band = None;
    t
}

fn gap_outcome_gating_core(outcome: &GapPatchOutcome) -> GapPatchOutcome {
    GapPatchOutcome {
        a_start_secs: outcome.a_start_secs,
        a_end_secs: outcome.a_end_secs,
        status: gap_status_gating_core(&outcome.status),
        tags: gap_tags_gating_core(&outcome.tags),
        residual: None,
    }
}

/// Compare patch/skip decisions and user-visible tier tags — not diagnostic-only fields
/// (`GapPatchOutcome::residual`, `tags.residual_band`, `summary.donor_relation`).
fn assert_patch_gating_equivalent(
    baseline: &PatchAudioResult,
    actual: &PatchAudioResult,
    label: &str,
) {
    assert_eq!(
        baseline.summary.patched_count, actual.summary.patched_count,
        "{label}: patched_count"
    );
    assert_eq!(
        baseline.summary.patched_marginal_count, actual.summary.patched_marginal_count,
        "{label}: patched_marginal_count"
    );
    assert_eq!(
        baseline.summary.skipped_count, actual.summary.skipped_count,
        "{label}: skipped_count"
    );
    assert_eq!(
        baseline.summary.not_planned_count, actual.summary.not_planned_count,
        "{label}: not_planned_count"
    );
    assert_eq!(
        baseline.summary.gaps.len(),
        actual.summary.gaps.len(),
        "{label}: gap count"
    );

    for (i, (base_gap, actual_gap)) in baseline
        .summary
        .gaps
        .iter()
        .zip(actual.summary.gaps.iter())
        .enumerate()
    {
        assert_eq!(
            gap_outcome_gating_core(base_gap),
            gap_outcome_gating_core(actual_gap),
            "{label}: gap[{i}] gating outcome"
        );
    }

    match (&baseline.pcm, &actual.pcm) {
        (Some(base_pcm), Some(actual_pcm)) => {
            assert_eq!(
                base_pcm.sample_rate, actual_pcm.sample_rate,
                "{label}: pcm sample_rate"
            );
            assert_eq!(
                base_pcm.channels, actual_pcm.channels,
                "{label}: pcm channels"
            );
            assert_eq!(
                base_pcm.samples, actual_pcm.samples,
                "{label}: patched PCM must be byte-identical when gate is off"
            );
        }
        (None, None) => {}
        (base, actual) => panic!(
            "{label}: pcm presence mismatch (baseline={:?}, actual={:?})",
            base.is_some(),
            actual.is_some()
        ),
    }
}

/// Under `residual_gate = off`, skips must never be attributed to the residual veto path.
fn assert_no_residual_gate_skip_reasons(result: &PatchAudioResult, label: &str) {
    for (i, gap) in result.summary.gaps.iter().enumerate() {
        if let GapPatchStatus::Skipped { reason } = &gap.status {
            assert!(
                !matches!(reason, GapPatchSkipReason::ResidualHeadroomExceeded { .. }),
                "{label}: gap[{i}] must not be residual-vetoed when gate is off"
            );
        }
    }
}

/// When the pre-gate arm patches, `measure_residual = true` may attach diagnostics only.
fn assert_diagnostic_residual_attached_when_patched(
    pre_gate: &PatchAudioResult,
    off_with_measurement: &PatchAudioResult,
    label: &str,
) {
    if pre_gate.summary.patched_count == 0 {
        return;
    }
    let measured = off_with_measurement.summary.gaps.iter().any(|gap| {
        gap.residual.is_some()
            || matches!(
                gap.status,
                GapPatchStatus::Patched {
                    residual_db: Some(_),
                    ..
                }
            )
    });
    assert!(
        measured,
        "{label}: measure_residual should attach diagnostic scalars when gate is off and gap patches"
    );
    assert_eq!(
        pre_gate.summary.donor_relation, None,
        "{label}: pre-gate arm must not infer donor_relation without measurement"
    );
}

fn run_off_regression_arm(
    fixture: &EnergySignatureFixture,
    temp: &tempfile::TempDir,
    repair: &RepairConfig,
    label: &str,
) {
    let pre_gate = run_fixture_patch(fixture, temp, repair, false);
    let off_with_measurement = run_fixture_patch(fixture, temp, repair, true);

    assert_no_residual_gate_skip_reasons(&off_with_measurement, label);
    assert_patch_gating_equivalent(
        &pre_gate,
        &off_with_measurement,
        &format!("{label}: pre_gate vs off+measure_residual"),
    );
    assert_diagnostic_residual_attached_when_patched(&pre_gate, &off_with_measurement, label);

    let pre_gate_repeat = run_fixture_patch(fixture, temp, repair, false);
    assert_patch_gating_equivalent(
        &pre_gate,
        &pre_gate_repeat,
        &format!("{label}: pre_gate repeatability"),
    );
}

fn run_fixture_patch(
    fixture: &EnergySignatureFixture,
    temp: &tempfile::TempDir,
    repair: &RepairConfig,
    measure_residual: bool,
) -> PatchAudioResult {
    let report = gap_report_from_energy_fixture(temp.path(), fixture);
    let mut request: PatchAudioRequest = patch_request_from_repair(report, repair);
    request.measure_residual = measure_residual;
    let patch = PatchAudio::new(&SymphoniaMediaReader, &FakeProgressReporter);
    patch
        .execute(request, RepairConfig::default().crossfade_ms)
        .expect("fixture patch")
}

fn pre_gate_repair(mut repair: RepairConfig) -> RepairConfig {
    repair.residual_gate = ResidualGateMode::Off;
    repair
}

fn calibration_shaped_off() -> RepairConfig {
    pre_gate_repair(production_repair_config(GapSignatureMode::Energy, 3.0))
}

fn production_fit_off() -> RepairConfig {
    pre_gate_repair(RepairConfig {
        gap_signature_mode: GapSignatureMode::Energy,
        gap_signature_context_secs: 3.0,
        residual_gate: ResidualGateMode::Off,
        ..RepairConfig::default()
    })
}

fn with_forced_pearson_skip(mut repair: RepairConfig) -> RepairConfig {
    repair.min_fill_correlation = 1.0;
    repair
}

/// **C4 — `off` is a true no-op:** `residual_gate = off` must not change patch/skip decisions or
/// output PCM vs the pre-gate path (`measure_residual = false`, gate off).
///
/// Arms: F1 truth (patch + forced skip) and F4 decoy (energy search on a decoy fixture — exercises
/// off-regression where gate behavior differs between modes, without asserting C1 veto). Profiles:
/// calibration-shaped and `production_fit`. The diagnostic path (`measure_residual = true`, gate
/// off) may attach residual verdicts / `donor_relation` but must not alter gating.
#[test]
fn off_no_regression_baseline() {
    let temp = tempfile::tempdir().expect("tempdir");
    let f1 = build_f1_production_at(16_000, 1, 32.0, 3.0);
    let f4 = build_f4_decoy_production(16_000, 1, 32.0, 3.0);

    let f1_profiles = [
        ("f1 calibration-shaped patch", calibration_shaped_off()),
        (
            "f1 calibration-shaped forced Pearson skip",
            with_forced_pearson_skip(calibration_shaped_off()),
        ),
        ("f1 production_fit", production_fit_off()),
        (
            "f1 production_fit forced Pearson skip",
            with_forced_pearson_skip(production_fit_off()),
        ),
    ];
    for (label, repair) in f1_profiles {
        run_off_regression_arm(&f1, &temp, &repair, label);
    }

    // F4 decoy fixture: off must match pre-gate regardless of what veto would do if enabled.
    let f4_profiles = [
        (
            "f4_decoy calibration-shaped patch",
            calibration_shaped_off(),
        ),
        ("f4_decoy production_fit", production_fit_off()),
    ];
    for (label, repair) in f4_profiles {
        run_off_regression_arm(&f4, &temp, &repair, label);
    }
}

/// **C2 — `--no-dual-fit` is a no-op when the ordinary bracket search already decides:** F1/F4
/// patch outright (`dual_fit` never engages — no skip to fall through from); the forced-Pearson-skip
/// arm does reach the G6 branch but has no real step between shoulders, so `try_dual_fit` declines
/// and the outcome is unchanged either way. `dual_fit = true` vs `false` must produce identical
/// gating and PCM in all three cases. Closes perf-plan `docs/dev/archive/TEMP-pipeline-perf-redesign-plan.md`
/// §4.7 backlog item **C2**.
#[test]
fn dual_fit_flag_is_noop_when_bracket_search_already_decides() {
    let temp = tempfile::tempdir().expect("tempdir");
    let f1 = build_f1_production_at(16_000, 1, 32.0, 3.0);
    let f4 = build_f4_decoy_production(16_000, 1, 32.0, 3.0);

    let mut dual_fit_on = production_fit_off();
    dual_fit_on.dual_fit = true;
    let mut dual_fit_off = production_fit_off();
    dual_fit_off.dual_fit = false;

    for (label, fixture) in [("f1 production_fit", &f1), ("f4_decoy production_fit", &f4)] {
        let on = run_fixture_patch(fixture, &temp, &dual_fit_on, false);
        let off = run_fixture_patch(fixture, &temp, &dual_fit_off, false);
        assert_patch_gating_equivalent(&on, &off, &format!("{label}: dual_fit on vs off"));
    }

    // Forced Pearson skip (`min_fill_correlation = 1.0`) DOES qualify for the G6 branch (it's a
    // scored gate failure, not `StructureAlignmentFailed`) — but F1 has a single true bracket, no
    // real step between shoulders, so `try_dual_fit`'s step-real check declines and the gap stays
    // skipped either way. This asserts that decline, not that G6 never runs.
    let mut forced_on = with_forced_pearson_skip(production_fit_off());
    forced_on.dual_fit = true;
    let mut forced_off = with_forced_pearson_skip(production_fit_off());
    forced_off.dual_fit = false;
    let on = run_fixture_patch(&f1, &temp, &forced_on, false);
    let off = run_fixture_patch(&f1, &temp, &forced_off, false);
    assert_patch_gating_equivalent(&on, &off, "f1 forced Pearson skip: dual_fit on vs off");
}
