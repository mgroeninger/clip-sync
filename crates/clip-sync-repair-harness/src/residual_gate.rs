//! Shared floor-oracle pipeline runner for residual gate validation tests.
//!
//! One gap per `BuiltFloorOracle`, `measure_residual = true`, decode A to mono for
//! `gap_report_from_floor_oracle` — same contract as the original `floor_oracle_integration`
//! helpers.

use clip_sync::testing::fakes::FakeProgressReporter;
use clip_sync::SymphoniaMediaReader;

use clip_sync_repair::application::PatchAudio;
use clip_sync_repair::domain::gap_fill_fit::FillConfidence;
use clip_sync_repair::domain::policies::DEFAULT_RESIDUAL_FLOOR_OK_DB;
use clip_sync_repair::domain::policies::SeamResidualVerdict;
use clip_sync_repair::domain::{GapPatchSkipReason, GapPatchStatus, GapSignatureMode, ResidualGateMode};
use clip_sync_repair::infrastructure::config::RepairConfig;
use clip_sync_repair::test_support::energy_signature_production::{
    gap_report_from_floor_oracle, patch_request_from_repair, production_repair_config,
};

use crate::floor_oracle::{decode_to_mono_wav_at, read_mono_wav, BuiltFloorOracle};

pub struct FloorOracleRun {
    pub status: &'static str,
    pub skip_reason: String,
    pub structure_ok: bool,
    pub residual: Option<SeamResidualVerdict>,
    pub align_adjustment_secs: f64,
    pub confidence: Option<FillConfidence>,
    /// Seam Pearson at the decided placement (`NaN` if the gap never reached the seam tier).
    pub seam_pre: f64,
    pub seam_post: f64,
}

/// Calibration profile — structure isolation, relaxed Pearson (`min_fill 0`).
/// Used for FLOOR_OK measurement and calibration gate tests.
pub fn calibration_repair_config(residual_gate: ResidualGateMode) -> RepairConfig {
    let mut repair = production_repair_config(GapSignatureMode::Energy, 3.0);
    repair.residual_gate = residual_gate;
    repair
}

/// Production fit profile — shipped floors + waveform weighting (Run B, G5 asserts).
pub fn production_fit_repair_config(residual_gate: ResidualGateMode) -> RepairConfig {
    RepairConfig {
        gap_signature_mode: GapSignatureMode::Energy,
        gap_signature_context_secs: 3.0,
        residual_gate,
        ..RepairConfig::default()
    }
}

/// Seam Pearson carried on a patched outcome or a seam-tier skip reason (`NaN` otherwise).
pub fn seam_pre_post(status: &GapPatchStatus) -> (f64, f64) {
    match status {
        GapPatchStatus::Patched {
            pre_correlation,
            post_correlation,
            ..
        } => (*pre_correlation, *post_correlation),
        GapPatchStatus::Skipped {
            reason:
                GapPatchSkipReason::CorrelationBelowThreshold {
                    pre_correlation,
                    post_correlation,
                    ..
                }
                | GapPatchSkipReason::ResidualHeadroomExceeded {
                    pre_correlation,
                    post_correlation,
                    ..
                },
        } => (*pre_correlation, *post_correlation),
        _ => (f64::NAN, f64::NAN),
    }
}

pub fn gap_status_label(status: &GapPatchStatus) -> (&'static str, String) {
    match status {
        GapPatchStatus::Patched { .. } => ("patched", String::new()),
        GapPatchStatus::Skipped { reason } => ("skipped", skip_reason_label(reason)),
        GapPatchStatus::NotPlanned { reason } => ("not_planned", format!("{reason:?}")),
    }
}

pub fn skip_reason_label(reason: &GapPatchSkipReason) -> String {
    match reason {
        GapPatchSkipReason::CorrelationBelowThreshold {
            pre_correlation,
            post_correlation,
            min_correlation,
        } => format!(
            "correlation_below(pre={pre_correlation:.4},post={post_correlation:.4},min={min_correlation})"
        ),
        GapPatchSkipReason::ResidualHeadroomExceeded { .. } => "residual_headroom".into(),
        GapPatchSkipReason::BExtractFailed => "b_extract_failed".into(),
        GapPatchSkipReason::BoundaryAlignmentFailed => "boundary_alignment_failed".into(),
        GapPatchSkipReason::AlignedSegmentOutOfRange => "aligned_segment_out_of_range".into(),
        GapPatchSkipReason::ZeroLengthGap => "zero_length_gap".into(),
    }
}

/// Run the floor-oracle pair under the **calibration** repair profile.
pub fn run_built_floor_oracle(
    built: &BuiltFloorOracle,
    residual_gate: ResidualGateMode,
) -> FloorOracleRun {
    run_built_floor_oracle_cfg(built, &calibration_repair_config(residual_gate))
}

/// Core API: run one built floor-oracle gap through `PatchAudio` with an explicit repair config.
pub fn run_built_floor_oracle_cfg(
    built: &BuiltFloorOracle,
    repair: &RepairConfig,
) -> FloorOracleRun {
    let dir = built.path_a.parent().expect("path_a parent");
    let decoded_a = dir.join("patch_a.wav");
    assert!(decode_to_mono_wav_at(
        &built.path_a,
        &decoded_a,
        built.meta.sample_rate,
        None,
    ));
    let (_, decoded_a_mono_i16) = read_mono_wav(&decoded_a);
    let decoded_a_mono: Vec<f32> = decoded_a_mono_i16.iter().map(|&s| s as f32 / 32767.0).collect();

    let report = gap_report_from_floor_oracle(
        &built.path_a,
        &built.path_b,
        &decoded_a_mono,
        built.meta.sample_rate,
        built.meta.gap_start_frame,
        built.meta.gap_end_frame,
    );

    let mut request = patch_request_from_repair(report, repair);
    request.measure_residual = true;

    let patch = PatchAudio::new(&SymphoniaMediaReader, &FakeProgressReporter);
    let result = patch
        .execute(request, RepairConfig::default().crossfade_ms)
        .expect("floor oracle patch");

    let gap = result.summary.gaps.first().expect("one gap");
    let (status, skip_reason) = gap_status_label(&gap.status);
    let structure_ok = status == "patched" || !skip_reason.contains("structure");
    let align_adjustment_secs = match &gap.status {
        GapPatchStatus::Patched {
            align_adjustment_secs,
            ..
        } => *align_adjustment_secs,
        _ => f64::NAN,
    };
    let confidence = match &gap.status {
        GapPatchStatus::Patched { confidence, .. } => Some(*confidence),
        _ => None,
    };
    let (seam_pre, seam_post) = seam_pre_post(&gap.status);

    FloorOracleRun {
        status,
        skip_reason,
        structure_ok,
        residual: gap.residual,
        align_adjustment_secs,
        confidence,
        seam_pre,
        seam_post,
    }
}

/// Returns `Err(description)` when the measured floor disagrees with the case's expectation.
pub fn check_floor_expectations(
    case_id: &str,
    built: &BuiltFloorOracle,
    run: &FloorOracleRun,
) -> Result<(), String> {
    let Some(residual) = run.residual else {
        return if built.meta.expect_informative_floor {
            Err(format!(
                "{case_id}: no residual (status={}, skip={}) — expected patch + informative floor",
                run.status, run.skip_reason
            ))
        } else {
            Ok(())
        };
    };
    match (built.meta.expect_informative_floor, residual.informative) {
        (true, false) => Err(format!(
            "{case_id}: expected informative floor, got uninformative (pre={:.1} post={:.1}, FLOOR_OK={DEFAULT_RESIDUAL_FLOOR_OK_DB})",
            residual.floor_pre_db, residual.floor_post_db
        )),
        (false, true) => Err(format!(
            "{case_id}: expected uninformative floor, got informative (pre={:.1} post={:.1})",
            residual.floor_pre_db, residual.floor_post_db
        )),
        _ => Ok(()),
    }
}

pub fn assert_floor_expectations(case_id: &str, built: &BuiltFloorOracle, run: &FloorOracleRun) {
    if let Err(e) = check_floor_expectations(case_id, built, run) {
        panic!("{e}");
    }
}

pub fn assert_truth_patches(case_id: &str, run: &FloorOracleRun, gate: ResidualGateMode) {
    assert_eq!(
        run.status, "patched",
        "{case_id} with {:?} should patch at truth (skip={})",
        gate, run.skip_reason
    );
    let residual = run
        .residual
        .expect("truth same-master should carry residual verdict");
    assert!(
        residual.informative,
        "{case_id} {:?}: floor should be informative at truth",
        gate
    );
}

/// On real-codec truth gaps, `veto_rescue` must not widen patching vs `veto` when Pearson already
/// passes (no dead-zone rescue flip).
pub fn assert_veto_rescue_matches_veto_on_truth(case_id: &str, built: &BuiltFloorOracle) {
    let veto = run_built_floor_oracle(built, ResidualGateMode::Veto);
    let rescue = run_built_floor_oracle(built, ResidualGateMode::VetoRescue);
    assert_truth_patches(case_id, &veto, ResidualGateMode::Veto);
    assert_truth_patches(case_id, &rescue, ResidualGateMode::VetoRescue);
    assert_eq!(
        veto.status, rescue.status,
        "{case_id}: veto_rescue status should match veto on real-codec truth (veto skip={}, rescue skip={})",
        veto.skip_reason, rescue.skip_reason
    );
    assert_eq!(
        veto.confidence, rescue.confidence,
        "{case_id}: veto_rescue confidence should match veto when Pearson passes (no dead-zone rescue)"
    );
}

pub fn run_pearson_min(run: &FloorOracleRun) -> f64 {
    run.seam_pre.min(run.seam_post)
}

/// `TRUE` iff Pearson is in the dead zone, the floor is informative, and headroom is within margin
/// at the decided placement — the condition under which `veto_rescue` would upgrade a skip.
pub fn rescue_trigger(run: &FloorOracleRun, repair: &RepairConfig) -> bool {
    let pearson_min = run_pearson_min(run);
    let margin_db = repair.residual_headroom_margin_db;
    let min_fill = f64::from(repair.min_fill_correlation);
    let informative = run.residual.is_some_and(|v| v.informative);
    let headroom = run
        .residual
        .map(|v| v.worst_headroom_db())
        .unwrap_or(f64::NAN);
    pearson_min.is_finite()
        && pearson_min < min_fill
        && informative
        && headroom.is_finite()
        && headroom <= margin_db
}

/// Run the floor-oracle pair under the **production_fit** repair profile.
pub fn run_built_floor_oracle_production_fit(
    built: &BuiltFloorOracle,
    residual_gate: ResidualGateMode,
) -> FloorOracleRun {
    run_built_floor_oracle_cfg(built, &production_fit_repair_config(residual_gate))
}

/// C3 under production_fit: active gates must not worsen the `off` outcome on a truth gap.
pub fn assert_production_fit_gate_no_worse_than_off_baseline(
    case_id: &str,
    off: &FloorOracleRun,
    run: &FloorOracleRun,
    gate: ResidualGateMode,
) {
    if off.status == "patched" {
        assert_eq!(
            run.status, "patched",
            "{case_id} with {gate:?}: must not false-veto when off patches (skip={})",
            run.skip_reason
        );
        assert!(
            !run.skip_reason.contains("residual_headroom"),
            "{case_id} with {gate:?}: truth gap must not be residual-vetoed",
        );
    } else {
        assert_ne!(
            run.status, "patched",
            "{case_id} with {gate:?}: must not rescue-patch when off skips (off skip={})",
            off.skip_reason
        );
    }
}

/// When `off` patches under production_fit, `veto_rescue` must match `veto`.
pub fn assert_production_fit_veto_rescue_matches_veto_baseline(
    case_id: &str,
    veto: &FloorOracleRun,
    rescue: &FloorOracleRun,
) {
    assert_eq!(
        veto.status, rescue.status,
        "{case_id}: veto_rescue status should match veto when off patches (veto skip={}, rescue skip={})",
        veto.skip_reason, rescue.skip_reason
    );
    assert_eq!(
        veto.confidence, rescue.confidence,
        "{case_id}: veto_rescue confidence should match veto when off patches under production_fit"
    );
}

/// Production_fit Pearson dead zone: skip under all gates, uninformative floor, rescue inert.
pub fn assert_production_fit_pearson_deadzone(
    case_id: &str,
    run: &FloorOracleRun,
    gate: ResidualGateMode,
    repair: &RepairConfig,
) {
    let min_fill = f64::from(repair.min_fill_correlation);
    let pearson_min = run_pearson_min(run);

    assert_eq!(
        run.status, "skipped",
        "{case_id} with {gate:?}: should skip under production_fit (skip={})",
        run.skip_reason
    );
    assert!(
        run.skip_reason.contains("correlation_below"),
        "{case_id} with {gate:?}: expected Pearson-tier skip, got {}",
        run.skip_reason
    );
    assert!(
        !run.skip_reason.contains("residual_headroom"),
        "{case_id} with {gate:?}: Pearson dead zone must not be residual-vetoed",
    );
    assert!(
        pearson_min.is_finite() && pearson_min < min_fill,
        "{case_id} with {gate:?}: pearson_min {pearson_min:.3} should be below min_fill {min_fill}"
    );
    assert!(
        !run.residual.is_some_and(|v| v.informative),
        "{case_id} with {gate:?}: floor must be uninformative in Pearson dead zone"
    );
    assert!(
        !rescue_trigger(run, repair),
        "{case_id} with {gate:?}: rescue_trigger must be false"
    );
}

/// G5 lock: punch-after-encode finale gaps skip under shipped floors with uninformative floor.
pub fn assert_deadzone_punch_inert(
    case_id: &str,
    run: &FloorOracleRun,
    gate: ResidualGateMode,
    repair: &RepairConfig,
) {
    assert_production_fit_pearson_deadzone(case_id, run, gate, repair);
}
