//! Anchor seam oracle: speech peaks offset from silent throat (A1–A4 rows).

use clip_sync::SymphoniaMediaReader;
use clip_sync::testing::fakes::FakeProgressReporter;

use clip_sync_repair::application::PatchAudio;
use clip_sync_repair::domain::gap_anchor_seam::{
    list_anchor_candidates_a, list_feasible_anchor_brackets, AnchorSeamMode, AnchorSeamParams,
    AnchorSeamSide, AnchorSource,
};
use clip_sync_repair::domain::{
    GapPatchStatus, GapSignatureMode, ResidualGateMode,
};
use clip_sync_repair::domain::policies::{refine_gap_frames, RefinedGapFrames};
use clip_sync_repair::infrastructure::config::RepairConfig;
use clip_sync_repair::test_support::energy_signature_fixtures::{
    build_f4_decoy_production, build_speech_peaks_offset_from_throat, EnergySignatureFixture,
};
use clip_sync_repair::test_support::energy_signature_production::{
    gap_report_from_energy_fixture, patch_request_from_repair,
};

fn anchor_params(fixture: &EnergySignatureFixture) -> AnchorSeamParams {
    AnchorSeamParams {
        context_frames: fixture.context_frames,
        max_anchors_per_side: 5,
        max_bracket_frames: (5.0 * fixture.sample_rate as f64).round() as usize,
        min_prominence: 0.0,
        structure: fixture.structure_params.clone(),
    }
}

fn refined_scan_hole(fixture: &EnergySignatureFixture) -> RefinedGapFrames {
    let ch = fixture.channels.max(1);
    refine_gap_frames(
        &fixture.a_samples,
        ch,
        fixture.gap_start,
        fixture.gap_end,
        0.01,
        0.0,
        (0.75 * fixture.sample_rate as f64).round() as usize,
    )
}

#[test]
fn anchor_candidates_pick_speech_peak_not_throat() {
    let fixture = build_speech_peaks_offset_from_throat(48_000, 1, 1.0);
    let scan = refined_scan_hole(&fixture);
    let params = anchor_params(&fixture);
    let set = list_anchor_candidates_a(
        &fixture.a_samples,
        fixture.channels,
        scan,
        &params,
    );
    assert!(
        set.pre.iter().any(|c| {
            c.source == AnchorSource::EnergyPeak && c.frame < scan.start_frame
        }),
        "expected pre energy peak before throat: {:?}",
        set.pre
    );
    let brackets = list_feasible_anchor_brackets(&set, scan, &params);
    assert!(
        brackets.iter().any(|b| b.refined.start_frame < scan.start_frame),
        "expected bracket with pre-anchor before throat"
    );
}

#[test]
fn anchor_seam_pipeline_patches_speech_peaks_fixture() {
    let fixture = build_speech_peaks_offset_from_throat(48_000, 1, 1.0);
    let temp = tempfile::tempdir().expect("tempdir");
    let report = gap_report_from_energy_fixture(temp.path(), &fixture);
    let mut repair = RepairConfig::default();
    repair.gap_signature_mode = GapSignatureMode::Energy;
    repair.gap_signature_context_secs = 3.0;
    repair.anchor_seam_mode = AnchorSeamMode::Force;
    repair.fill_mode = clip_sync_repair::domain::FillMode::Fit;
    repair.residual_gate = ResidualGateMode::VetoRescue;
    let request = patch_request_from_repair(report, &repair);
    let reader = SymphoniaMediaReader;
    let progress = FakeProgressReporter;
    let response = PatchAudio::new(&reader, &progress)
        .execute(request, RepairConfig::default().crossfade_ms)
        .expect("patch");
    assert!(
        matches!(response.summary.gaps[0].status, GapPatchStatus::Patched { .. }),
        "expected patched gap, got {:?}",
        response.summary.gaps[0].status
    );
}

#[test]
fn anchor_seam_f4_decoy_still_skips_under_residual_veto() {
    let fixture = build_f4_decoy_production(48_000, 1, 60.0, 3.0);
    let temp = tempfile::tempdir().expect("tempdir");
    let report = gap_report_from_energy_fixture(temp.path(), &fixture);
    let mut repair = RepairConfig::default();
    repair.gap_signature_mode = GapSignatureMode::Energy;
    repair.gap_signature_context_secs = 3.0;
    repair.anchor_seam_mode = AnchorSeamMode::Force;
    repair.fill_mode = clip_sync_repair::domain::FillMode::Fit;
    repair.residual_gate = ResidualGateMode::Veto;
    repair.min_fill_correlation = 0.35;
    repair.fill_absolute_floor = 0.12;
    let request = patch_request_from_repair(report, &repair);
    let reader = SymphoniaMediaReader;
    let progress = FakeProgressReporter;
    let response = PatchAudio::new(&reader, &progress)
        .execute(request, RepairConfig::default().crossfade_ms)
        .expect("patch");
    assert!(
        matches!(response.summary.gaps[0].status, GapPatchStatus::Skipped { .. }),
        "F4 decoy should skip with residual veto, got {:?}",
        response.summary.gaps[0].status
    );
}

#[test]
fn flat_c1_fixture_falls_back_to_scan_edges() {
    let mut fixture = build_speech_peaks_offset_from_throat(48_000, 1, 1.0);
    fixture.a_samples.fill(0.0);
    fixture.b_samples.fill(0.0);
    let scan = refined_scan_hole(&fixture);
    let set = list_anchor_candidates_a(
        &fixture.a_samples,
        fixture.channels,
        scan,
        &anchor_params(&fixture),
    );
    assert_eq!(set.pre.len(), 1);
    assert_eq!(set.pre[0].source, AnchorSource::ScanRefined);
    assert_eq!(set.pre[0].side, AnchorSeamSide::Pre);
}
