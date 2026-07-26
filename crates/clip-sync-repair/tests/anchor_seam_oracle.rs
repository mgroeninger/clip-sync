//! Anchor seam oracle: speech peaks offset from silent throat (plan §7 rows A1–A6, A2–A3).
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
use clip_sync_repair::domain::gap_tags::{FitPathTag, PatchTier, SeamShape};
use clip_sync_repair::domain::{
    FillConfidence, FillMode, FitBoundarySearch, GapPatchOutcome, GapPatchStatus, GapSignatureMode,
    ResidualGateMode,
};
use clip_sync_repair::domain::policies::{refine_gap_frames, RefinedGapFrames};
use clip_sync_repair::infrastructure::config::RepairConfig;
use clip_sync_repair_fixtures::energy_signature_fixtures::{
    build_c3_speech_boundary_asymmetric_post, build_f4_decoy_production,
    build_speech_peaks_offset_from_throat, build_w5_noise_collar_anchor_rescue,
    build_w5_symmetric_weak_throat_anchor_rescue, EnergySignatureFixture,
};
use clip_sync_repair_fixtures::energy_signature_production::{
    gap_report_from_energy_fixture, oracle_nominal_throat_pearson, patch_request_from_repair,
    production_fit_weights_config, w5_anchor_rescue_repair,
};
use clip_sync_repair_harness::patch_audio::{energy_sig_patch_options, patch_request_with_options};

/// Default `RepairConfig` ships `anchor_seam_mode = auto` (integration tier / `pr-repair`).
#[test]
fn default_repair_config_anchor_seam_mode_is_auto() {
    assert_eq!(RepairConfig::default().anchor_seam_mode, AnchorSeamMode::Auto);

    let fixture = build_speech_peaks_offset_from_throat(48_000, 1, 1.0);
    let temp = tempfile::tempdir().expect("tempdir");
    let report = gap_report_from_energy_fixture(temp.path(), &fixture);
    let request = patch_request_from_repair(report, &RepairConfig::default());
    assert_eq!(request.anchor_seam_mode, AnchorSeamMode::Auto);
}

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

/// Routing decision facts locked by **step 1** of the fit-joint routing extraction
/// (`docs/dev/archive/TEMP-fit-routing-extraction-plan.md`). These capture *which exit the router took* and the
/// resulting vocabulary — the `route_fit_joint` refactor must preserve them byte-for-byte. They
/// deliberately encode the current reality (e.g. A2/A5 patch on the baseline throat and never engage
/// the anchor path: `anchor_seam_used = false`).
#[derive(Debug, PartialEq)]
struct RoutingFacts {
    patched: bool,
    confidence: Option<FillConfidence>,
    anchor_seam_used: bool,
    anchor_move_nonzero: bool,
    patch_tier: PatchTier,
    seam_shape: SeamShape,
    fit_path: Option<FitPathTag>,
}

fn routing_facts(gap: &GapPatchOutcome) -> RoutingFacts {
    let (patched, confidence, anchor_seam_used, anchor_move_nonzero) = match gap.status {
        GapPatchStatus::Patched {
            confidence,
            anchor_seam_used,
            anchor_bracket_move_frames,
            ..
        } => (
            true,
            Some(confidence),
            anchor_seam_used,
            anchor_bracket_move_frames > 0,
        ),
        _ => (false, None, false, false),
    };
    RoutingFacts {
        patched,
        confidence,
        anchor_seam_used,
        anchor_move_nonzero,
        patch_tier: gap.tags.patch_tier,
        seam_shape: gap.tags.seam_shape,
        fit_path: gap.tags.fit_path,
    }
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
    let mut request = request;
    request.settings.anchor_seam_mode = repair.anchor_seam_mode;
    request.settings.max_anchor_bracket_secs = repair.max_anchor_bracket_secs;
    request.settings.max_anchors_per_side = repair.max_anchors_per_side;
    request.settings.anchor_seam_min_prominence = repair.anchor_seam_min_prominence;
    request.settings.residual_gate = repair.residual_gate;
    request.measure_residual = true;
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
    repair.fill_extract_tail_slack_secs = 0.05;
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
    // CHARACTERIZATION (step 1): A5 patches on the baseline throat (~0.99) and short-circuits before
    // anchor search ever runs — `anchor_seam_used = false`. Locks current routing for the refactor.
    assert_eq!(
        routing_facts(gap),
        RoutingFacts {
            patched: true,
            confidence: Some(FillConfidence::High),
            anchor_seam_used: false,
            anchor_move_nonzero: false,
            patch_tier: PatchTier::High,
            seam_shape: SeamShape::Balanced,
            fit_path: Some(FitPathTag::BaselineOnly),
        },
        "A5 routing characterization"
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
    repair.fill_extract_tail_slack_secs = 0.05;
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
    // CHARACTERIZATION (step 1): bool-mode baseline throat patch; anchor path not engaged.
    assert_eq!(
        routing_facts(gap),
        RoutingFacts {
            patched: true,
            confidence: Some(FillConfidence::High),
            anchor_seam_used: false,
            anchor_move_nonzero: false,
            patch_tier: PatchTier::High,
            seam_shape: SeamShape::Balanced,
            fit_path: Some(FitPathTag::BaselineOnly),
        },
        "A5b routing characterization"
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
    let gap = &response.summary.gaps[0];
    // CHARACTERIZATION (step 1): the decoy is rejected as a correlation HARD-SKIP (min(pre,post) <
    // 0.12), asymmetric-post shape — the router must keep skipping it. (NB: the skip surfaces as a
    // correlation hard-skip, not a residual-veto `not_applicable` — the name predates this tier.)
    assert_eq!(
        routing_facts(gap),
        RoutingFacts {
            patched: false,
            confidence: None,
            anchor_seam_used: false,
            anchor_move_nonzero: false,
            patch_tier: PatchTier::HardSkip,
            seam_shape: SeamShape::AsymmetricPost,
            fit_path: Some(FitPathTag::BaselineOnly),
        },
        "F4 routing characterization"
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
    repair.fill_extract_tail_slack_secs = 0.05;
    repair.min_border_discovery_secs = 0.25;
    repair.border_standoff_secs = 0.0;
    repair.gap_end_extend_on_post_seam_fail = false;
    repair.gap_start_extend_on_pre_seam_fail = false;
    repair.gap_end_extend_max_ms = 0;
    let request = patch_request_with_options(report, false, 5.0, 0.35, options);
    let mut request = request;
    request.settings.anchor_seam_mode = repair.anchor_seam_mode;
    request.settings.max_anchor_bracket_secs = repair.max_anchor_bracket_secs;
    request.settings.max_anchors_per_side = repair.max_anchors_per_side;
    request.settings.anchor_seam_min_prominence = repair.anchor_seam_min_prominence;
    request.settings.anchor_seam_min_match_pearson = repair.anchor_seam_min_match_pearson;
    request.settings.anchor_seam_min_xcorr_peak = repair.anchor_seam_min_xcorr_peak;
    request.settings.anchor_seam_xcorr_ambiguous_band = repair.anchor_seam_xcorr_ambiguous_band;
    request.settings.residual_gate = repair.residual_gate;
    request.settings.normalize_fill = repair.normalize_fill;
    request.settings.fill_length_slack_secs = repair.fill_length_slack_secs;
    request.settings.fill_extract_tail_slack_secs = repair.fill_extract_tail_slack_secs;
    request.settings.min_border_discovery_secs = repair.min_border_discovery_secs;
    request.settings.border_standoff_secs = repair.border_standoff_secs;
    request.settings.gap_end_extend_on_post_seam_fail = repair.gap_end_extend_on_post_seam_fail;
    request.settings.gap_start_extend_on_pre_seam_fail = repair.gap_start_extend_on_pre_seam_fail;
    request.settings.gap_end_extend_max_ms = repair.gap_end_extend_max_ms;
    request.measure_residual = true;
    let response = PatchAudio::new(&SymphoniaMediaReader, &FakeProgressReporter)
        .execute(request, RepairConfig::default().crossfade_ms)
        .expect("patch");

    let gap = &response.summary.gaps[0];
    // CHARACTERIZATION (step 1): C3 boundary gap patches *marginally* on the baseline throat with a
    // symmetric-weak seam — the anchor path is not engaged. Lock current exit + tier.
    assert_eq!(
        routing_facts(gap),
        RoutingFacts {
            patched: true,
            confidence: Some(FillConfidence::Marginal),
            anchor_seam_used: false,
            anchor_move_nonzero: false,
            patch_tier: PatchTier::Marginal,
            seam_shape: SeamShape::SymmetricWeak,
            fit_path: Some(FitPathTag::BaselineOnly),
        },
        "A2 routing characterization"
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
    base_repair.fill_extract_tail_slack_secs = 0.05;
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

const W5_AUTO_TRIGGER_FLOOR: f64 = 0.27;

fn assert_w5_throat_symmetric_weak(pre: f64, post: f64) {
    assert!(
        pre < W5_AUTO_TRIGGER_FLOOR && post < W5_AUTO_TRIGGER_FLOOR,
        "expected throat below auto trigger ({W5_AUTO_TRIGGER_FLOOR}): pre={pre:.3} post={post:.3}"
    );
    assert!(
        (pre - post).abs() < 0.10,
        "expected symmetric_weak throat: pre={pre:.3} post={post:.3}"
    );
}

/// **A6** domain — W5 throat symmetric-weak; feasible peak anchor brackets exist.
#[test]
fn w5_fixture_throat_symmetric_weak_and_brackets_exist() {
    let fixture = build_w5_symmetric_weak_throat_anchor_rescue(48_000, 1, 1.0, 0.78);
    let repair = w5_anchor_rescue_repair(AnchorSeamMode::Auto, 0.78);
    let (pre, post) = oracle_nominal_throat_pearson(&fixture, &repair);
    assert_w5_throat_symmetric_weak(pre, post);

    let scan = refined_scan_hole(&fixture);
    let params = anchor_params(&fixture);
    let set = list_anchor_candidates_a(&fixture.a_samples, fixture.channels, scan, &params);
    assert!(
        set.pre.iter().any(|c| {
            c.frame < scan.start_frame && c.source != AnchorSource::ScanRefined
        }),
        "A6: expected pre-anchor before throat: {:?}",
        set.pre
    );
    let brackets = list_feasible_anchor_brackets(&set, scan, &params);
    assert!(
        brackets.iter().any(|b| b.move_frames > 0 && b.refined.start_frame < scan.start_frame),
        "A6: expected movable bracket with pre-anchor before throat: {:?}",
        brackets
    );
}

/// Faithful **noise-collar** A6 fixture + repair (plan §8 Q1): gap flanked by *decorrelated*
/// broadband noise (baseline seam ~0 from **content**, not a clamped radius), with short triangular
/// speech anchors a moving bracket reaches at High. `anchor_seam_min_prominence` filters the collar's
/// noise peaks so the intended near anchor is the unique passing bracket (~6 candidates, 1 winner).
fn noise_collar_a6_request(mode: AnchorSeamMode, temp: &std::path::Path) -> PatchAudioRequest {
    let fixture = build_w5_noise_collar_anchor_rescue(48_000, 1, 0.5, 0.3, 0.08);
    let report = gap_report_from_energy_fixture(temp, &fixture);
    let mut repair = w5_anchor_rescue_repair(mode, 1.0);
    repair.anchor_seam_min_prominence = 0.10;
    patch_request_from_repair(report, &repair)
}

/// **A6** pipeline — `anchor_seam_mode=auto` rescues a genuine symmetric-weak throat via a moving
/// editorial-anchor bracket, end-to-end through `PatchAudio`.
///
/// **Slow** (full PatchAudio + anchor search; ~30 s release / ~150 s debug) — `#[ignore]`d from the
/// default lane and run in release via `test-tier.ps1` with `--ignored`.
#[test]
#[ignore = "slow: full PatchAudio anchor rescue (~30s release / ~150s debug) — run via test-tier release --ignored"]
fn w5_anchor_rescue_pipeline_engages_anchor_seam_auto() {
    let temp = tempfile::tempdir().expect("tempdir");
    let request = noise_collar_a6_request(AnchorSeamMode::default(), temp.path());

    let response = PatchAudio::new(&SymphoniaMediaReader, &FakeProgressReporter)
        .execute(request, RepairConfig::default().crossfade_ms)
        .expect("patch");

    let gap = &response.summary.gaps[0];
    let facts = routing_facts(gap);
    assert!(facts.patched, "expected patched gap, got {:?}", gap.status);
    assert!(facts.anchor_seam_used, "A6 auto: anchor path must win: {facts:?}");
    assert!(facts.anchor_move_nonzero, "A6 auto: expected bracket move: {facts:?}");
    assert!(
        matches!(
            facts.patch_tier,
            PatchTier::AnchorTrusted | PatchTier::Marginal | PatchTier::High
        ),
        "A6 auto: unexpected tier: {:?}",
        facts.patch_tier
    );
}

/// **A6b** pipeline — `anchor_seam_mode=force` matches auto on the noise-collar W5 fixture.
/// Slow; see [`w5_anchor_rescue_pipeline_engages_anchor_seam_auto`].
#[test]
#[ignore = "slow: full PatchAudio anchor rescue (~30s release / ~150s debug) — run via test-tier release --ignored"]
fn w5_anchor_rescue_pipeline_engages_anchor_seam_force() {
    let temp = tempfile::tempdir().expect("tempdir");
    let request = noise_collar_a6_request(AnchorSeamMode::Force, temp.path());

    let response = PatchAudio::new(&SymphoniaMediaReader, &FakeProgressReporter)
        .execute(request, RepairConfig::default().crossfade_ms)
        .expect("patch");

    let gap = &response.summary.gaps[0];
    let facts = routing_facts(gap);
    assert!(facts.patched);
    assert!(facts.anchor_seam_used, "A6b force: anchor path must win: {facts:?}");
    assert!(facts.anchor_move_nonzero);
}

