//! Anchor seam oracle: speech peaks offset from silent throat (plan §7 rows A1–A5b, A2–A3).
//!
//! Tier: **integration** (`Invoke-RepairIntegrationOnly` in `scripts/test-tier.ps1`).
//! Domain + pipeline oracles for editorial anchor search; distinct from sine-seam rows in
//! `patch_audio_integration.rs` and harness-backed SP01–SP03 in `integration_energy_patch.rs`.

use clip_sync::SymphoniaMediaReader;
use clip_sync::testing::fakes::FakeProgressReporter;

use clip_sync_repair::application::{PatchAudio, PatchAudioRequest};
use clip_sync_repair::domain::gap_anchor_seam::{
    list_anchor_candidates_a, list_feasible_anchor_brackets, AnchorSeamMode, AnchorSeamParams,
    AnchorSeamSide, AnchorSource,
};
use clip_sync_repair::domain::gap_tags::FitPathTag;
use clip_sync_repair::domain::{
    FillMode, FitBoundarySearch, GapPatchStatus, GapSignatureMode, ResidualGateMode,
};
use clip_sync_repair::domain::policies::{refine_gap_frames, RefinedGapFrames};
use clip_sync_repair::infrastructure::config::RepairConfig;
use clip_sync_repair::test_support::energy_signature_fixtures::{
    build_c3_speech_boundary_asymmetric_post, build_f4_decoy_production,
    build_speech_peaks_offset_from_throat, EnergySignatureFixture,
};
use clip_sync_repair::test_support::energy_signature_production::{
    gap_report_from_energy_fixture, patch_request_from_repair, production_fit_weights_config,
};
use clip_sync_repair_harness::patch_audio::{energy_sig_patch_options, patch_request_with_options};

fn anchor_params(fixture: &EnergySignatureFixture) -> AnchorSeamParams {
    AnchorSeamParams {
        context_frames: fixture.context_frames,
        max_anchors_per_side: 5,
        max_bracket_frames: (5.0 * fixture.sample_rate as f64).round() as usize,
        min_prominence: 0.0,
        structure: fixture.structure_params,
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
            c.frame < scan.start_frame && c.source != AnchorSource::ScanRefined
        }),
        "expected salient pre-anchor before throat (not scan edge): {:?}",
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
    let mut options = energy_sig_patch_options(GapSignatureMode::Energy);
    options.fill_mode = clip_sync_repair::domain::FillMode::Fit;
    options.gap_signature_context_secs = 3.0;
    options.fill_border_search_secs = 10.0;
    options.fill_align_margin_secs = 1.0;
    let mut repair = production_fit_weights_config(GapSignatureMode::Energy, 3.0);
    repair.anchor_seam_mode = AnchorSeamMode::Force;
    repair.residual_gate = ResidualGateMode::VetoRescue;
    repair.min_fill_correlation = 0.35;
    repair.fill_fit_structure_weight = 0.35;
    repair.fill_fit_waveform_weight = 0.65;
    let request = patch_request_with_options(report, false, 5.0, 0.35, options);
    let request = PatchAudioRequest {
        anchor_seam_mode: repair.anchor_seam_mode,
        max_anchor_bracket_secs: repair.max_anchor_bracket_secs,
        max_anchors_per_side: repair.max_anchors_per_side,
        anchor_seam_min_prominence: repair.anchor_seam_min_prominence,
        residual_gate: repair.residual_gate,
        measure_residual: true,
        ..request
    };
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

/// **A5** — `baseline_only` + `anchor_seam_mode=auto` patches without boundary grid (`--full`).
///
/// Uses [`patch_request_from_repair`] (TOML/CLI config path), not harness option overrides.
#[test]
fn a5_baseline_only_auto_patches_speech_peaks_without_boundary_grid() {
    let fixture = build_speech_peaks_offset_from_throat(48_000, 1, 1.0);
    let temp = tempfile::tempdir().expect("tempdir");
    let report = gap_report_from_energy_fixture(temp.path(), &fixture);

    let mut repair = production_fit_weights_config(GapSignatureMode::Energy, 3.0);
    repair.fill_mode = FillMode::Fit;
    repair.fit_boundary_search = FitBoundarySearch::BaselineOnly;
    repair.anchor_seam_mode = AnchorSeamMode::Auto;
    repair.residual_gate = ResidualGateMode::VetoRescue;
    repair.min_fill_correlation = 0.35;
    // Geometry aligned with passing A1 pipeline oracle (harness-tuned haystack).
    repair.normalize_fill = false;
    repair.fill_length_slack_secs = 0.05;
    repair.min_border_discovery_secs = 0.25;
    repair.border_standoff_secs = 0.0;
    repair.gap_end_extend_on_post_seam_fail = false;
    repair.gap_start_extend_on_pre_seam_fail = false;
    repair.gap_end_extend_max_ms = 0;

    let request = patch_request_from_repair(report, &repair);
    assert_eq!(request.fit_boundary_search, FitBoundarySearch::BaselineOnly);
    assert_eq!(request.anchor_seam_mode, AnchorSeamMode::Auto);
    assert_eq!(request.fill_mode, FillMode::Fit);

    let response = PatchAudio::new(&SymphoniaMediaReader, &FakeProgressReporter)
        .execute(request, RepairConfig::default().crossfade_ms)
        .expect("patch");

    let gap = &response.summary.gaps[0];
    assert!(
        matches!(gap.status, GapPatchStatus::Patched { .. }),
        "A5: expected patched gap under baseline_only+auto, got {:?}",
        gap.status
    );
    assert_eq!(
        gap.tags.fit_path,
        Some(FitPathTag::BaselineOnly),
        "A5: anchor seam must patch without boundary grid"
    );
}

/// **A5b** — bool signature path: `baseline_only` + `anchor_seam_mode=auto` without boundary grid.
#[test]
fn a5b_baseline_only_auto_patches_speech_peaks_bool_mode() {
    let fixture = build_speech_peaks_offset_from_throat(48_000, 1, 1.0);
    let temp = tempfile::tempdir().expect("tempdir");
    let report = gap_report_from_energy_fixture(temp.path(), &fixture);

    let mut repair = production_fit_weights_config(GapSignatureMode::Bool, 3.0);
    repair.fill_mode = FillMode::Fit;
    repair.fit_boundary_search = FitBoundarySearch::BaselineOnly;
    repair.anchor_seam_mode = AnchorSeamMode::Auto;
    repair.residual_gate = ResidualGateMode::VetoRescue;
    repair.min_fill_correlation = 0.35;
    repair.normalize_fill = false;
    repair.fill_length_slack_secs = 0.05;
    repair.min_border_discovery_secs = 0.25;
    repair.border_standoff_secs = 0.0;
    repair.gap_end_extend_on_post_seam_fail = false;
    repair.gap_start_extend_on_pre_seam_fail = false;
    repair.gap_end_extend_max_ms = 0;

    let request = patch_request_from_repair(report, &repair);
    assert_eq!(request.gap_signature_mode, GapSignatureMode::Bool);
    assert_eq!(request.anchor_seam_mode, AnchorSeamMode::Auto);

    let response = PatchAudio::new(&SymphoniaMediaReader, &FakeProgressReporter)
        .execute(request, RepairConfig::default().crossfade_ms)
        .expect("patch");

    let gap = &response.summary.gaps[0];
    assert!(
        matches!(gap.status, GapPatchStatus::Patched { .. }),
        "A5b: expected patched gap under bool+baseline_only+auto, got {:?}",
        gap.status
    );
    assert_eq!(
        gap.tags.fit_path,
        Some(FitPathTag::BaselineOnly),
        "A5b: anchor seam must patch without boundary grid in bool mode"
    );
}

#[test]
fn anchor_seam_f4_decoy_still_skips_under_residual_veto() {
    let fixture = build_f4_decoy_production(48_000, 1, 60.0, 3.0);
    let temp = tempfile::tempdir().expect("tempdir");
    let report = gap_report_from_energy_fixture(temp.path(), &fixture);
    let repair = RepairConfig {
        gap_signature_mode: GapSignatureMode::Energy,
        gap_signature_context_secs: 3.0,
        anchor_seam_mode: AnchorSeamMode::Force,
        fill_mode: clip_sync_repair::domain::FillMode::Fit,
        residual_gate: ResidualGateMode::Veto,
        min_fill_correlation: 0.35,
        fill_absolute_floor: 0.12,
        ..Default::default()
    };
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

/// **A2** — C3 speech boundary: post-side bool onset anchor, not scan throat only.
#[test]
fn a2_c3_post_onset_anchor_near_speech_boundary() {
    let fixture = build_c3_speech_boundary_asymmetric_post(48_000, 1, 0.05);
    let scan = refined_scan_hole(&fixture);
    let params = anchor_params(&fixture);
    let set = list_anchor_candidates_a(
        &fixture.a_samples,
        fixture.channels,
        scan,
        &params,
    );
    assert!(
        set.post.iter().any(|c| {
            c.frame > scan.end_frame && c.source != AnchorSource::ScanRefined
        }),
        "A2: expected post anchor at speech onset after throat, got {:?}",
        set.post
    );
    let brackets = list_feasible_anchor_brackets(&set, scan, &params);
    assert!(
        brackets.iter().any(|b| b.post.frame > scan.end_frame),
        "A2: expected bracket with post-anchor past scan end"
    );
}

/// **A2** pipeline — bool + `anchor_seam_mode=force` patches C3 asymmetric-post fixture.
#[test]
fn a2_c3_pipeline_patches_speech_boundary_bool_mode() {
    let fixture = build_c3_speech_boundary_asymmetric_post(48_000, 1, 0.05);
    let temp = tempfile::tempdir().expect("tempdir");
    let report = gap_report_from_energy_fixture(temp.path(), &fixture);
    let mut options = energy_sig_patch_options(GapSignatureMode::Bool);
    options.fill_mode = FillMode::Fit;
    options.gap_signature_context_secs = 3.0;
    options.fill_border_search_secs = 10.0;
    options.fill_align_margin_secs = 1.0;
    let mut repair = production_fit_weights_config(GapSignatureMode::Bool, 3.0);
    repair.anchor_seam_mode = AnchorSeamMode::Force;
    repair.residual_gate = ResidualGateMode::VetoRescue;
    repair.min_fill_correlation = 0.35;
    repair.fill_fit_structure_weight = 0.35;
    repair.fill_fit_waveform_weight = 0.65;
    repair.normalize_fill = false;
    repair.fill_length_slack_secs = 0.05;
    repair.min_border_discovery_secs = 0.25;
    repair.border_standoff_secs = 0.0;
    repair.gap_end_extend_on_post_seam_fail = false;
    repair.gap_start_extend_on_pre_seam_fail = false;
    repair.gap_end_extend_max_ms = 0;
    let request = patch_request_with_options(report, false, 5.0, 0.35, options);
    let request = PatchAudioRequest {
        anchor_seam_mode: repair.anchor_seam_mode,
        max_anchor_bracket_secs: repair.max_anchor_bracket_secs,
        max_anchors_per_side: repair.max_anchors_per_side,
        anchor_seam_min_prominence: repair.anchor_seam_min_prominence,
        anchor_seam_min_match_pearson: repair.anchor_seam_min_match_pearson,
        anchor_seam_min_xcorr_peak: repair.anchor_seam_min_xcorr_peak,
        anchor_seam_xcorr_ambiguous_band: repair.anchor_seam_xcorr_ambiguous_band,
        residual_gate: repair.residual_gate,
        measure_residual: true,
        normalize_fill: repair.normalize_fill,
        fill_length_slack_secs: repair.fill_length_slack_secs,
        min_border_discovery_secs: repair.min_border_discovery_secs,
        border_standoff_secs: repair.border_standoff_secs,
        gap_end_extend_on_post_seam_fail: repair.gap_end_extend_on_post_seam_fail,
        gap_start_extend_on_pre_seam_fail: repair.gap_start_extend_on_pre_seam_fail,
        gap_end_extend_max_ms: repair.gap_end_extend_max_ms,
        ..request
    };
    let response = PatchAudio::new(&SymphoniaMediaReader, &FakeProgressReporter)
        .execute(request, RepairConfig::default().crossfade_ms)
        .expect("patch");

    let gap = &response.summary.gaps[0];
    assert!(
        matches!(gap.status, GapPatchStatus::Patched { .. }),
        "A2: expected patched C3 gap, got {:?}",
        gap.status
    );
}

/// **A3** pipeline — flat C1: `anchor_seam_mode=off` and `force` yield the same outcome.
#[test]
fn a3_flat_c1_anchor_off_and_force_same_outcome() {
    let mut fixture = build_speech_peaks_offset_from_throat(48_000, 1, 1.0);
    fixture.a_samples.fill(0.0);
    fixture.b_samples.fill(0.0);
    let temp = tempfile::tempdir().expect("tempdir");

    let mut base_repair = production_fit_weights_config(GapSignatureMode::Energy, 3.0);
    base_repair.fill_mode = FillMode::Fit;
    base_repair.fit_boundary_search = FitBoundarySearch::BaselineOnly;
    base_repair.residual_gate = ResidualGateMode::VetoRescue;
    base_repair.min_fill_correlation = 0.35;
    base_repair.normalize_fill = false;
    base_repair.fill_length_slack_secs = 0.05;
    base_repair.min_border_discovery_secs = 0.25;
    base_repair.border_standoff_secs = 0.0;
    base_repair.gap_end_extend_on_post_seam_fail = false;
    base_repair.gap_start_extend_on_pre_seam_fail = false;
    base_repair.gap_end_extend_max_ms = 0;

    let run = |mode: AnchorSeamMode| {
        let mut repair = base_repair.clone();
        repair.anchor_seam_mode = mode;
        let report = gap_report_from_energy_fixture(temp.path(), &fixture);
        let request = patch_request_from_repair(report, &repair);
        PatchAudio::new(&SymphoniaMediaReader, &FakeProgressReporter)
            .execute(request, RepairConfig::default().crossfade_ms)
            .expect("patch")
            .summary
            .gaps[0]
            .status
            .clone()
    };

    let off = run(AnchorSeamMode::Off);
    let force = run(AnchorSeamMode::Force);
    assert_eq!(
        std::mem::discriminant(&off),
        std::mem::discriminant(&force),
        "A3: flat C1 should behave the same with anchor off vs force (only scan edges); off={off:?} force={force:?}"
    );
}
