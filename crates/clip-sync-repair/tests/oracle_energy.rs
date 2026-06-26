//! Energy signature domain-acceptance oracles — **SD01–SD08** / **EC** domain rows.
//!
//! Tier: **integration** (oracle label). Short 8 s fixtures (`u1_`–`u8_`) + EC03/EC06 domain;
//! EC01/EC02 production geometry (`p1_`/`p2_`) and slow control rows are `#[ignore]`.
//!
//! PR: **yes** (fast SD + EC03/EC06 domain) — `pr-repair`. Ignored production/control rows:
//! `.\scripts\test-tier.ps1 -Tier oracle`.
//!
//! Run: `cargo test -p clip-sync-repair --test oracle_energy`
//! Ignored: `.\scripts\test-tier.ps1 -Tier oracle`

use clip_sync_repair::test_support::energy_signature_fixtures::{
    build_f1, build_f1_integration, build_f1_production, build_f2, build_f2_at_rate, build_f2_integration,
    build_f2_production, build_f3_drone, build_f3_drone_production, build_f3_silence,
    build_f4_decoy_production, structure_heavy_weights, BOOL_AMBIGUITY_EPS, ENERGY_PAUSE_MARGIN,
    MODE_SCORE_EPS,
};

use clip_sync_repair::test_support::energy_signature_production::{
    gap_report_from_energy_fixture, patch_request_from_repair,
    production_geometry_params, production_repair_config, scan_gaps_for_fixture,
};
use clip_sync_repair::test_support::energy_signature_fixtures::gap_report_times;
use clip_sync_repair::test_support::patch_geometry_preview::preview_patch_geometry;
use clip_sync_repair::test_support::energy_signature_fixtures::structure_slide_secs;
use clip_sync::SymphoniaMediaReader;
use clip_sync::testing::fakes::FakeProgressReporter;
use clip_sync_repair::application::PatchAudio;
use clip_sync_repair::domain::GapPatchStatus;
use clip_sync_repair::infrastructure::config::RepairConfig;
use clip_sync_repair::domain::gap_signature::{build_gap_signature, GapSignature, GapSignatureMode};

#[test]
fn f1_integration_energy_scores_are_finite() {
    let f = build_f1_integration(48_000, 2);
    assert!(
        f.energy_pre_at(f.true_fill_start).is_finite(),
        "true pre energy"
    );
    assert!(
        f.energy_pre_at(f.nominal_fill_start).is_finite(),
        "nominal pre energy"
    );
}

#[test]
fn f2_integration_energy_scores_are_finite() {
    let f = build_f2_integration(48_000, 2);
    let true_pre = f.energy_pre_at(f.true_fill_start);
    assert!(
        true_pre.is_finite(),
        "true pre energy at pause₁ should be finite (got {true_pre})"
    );
    // pause₂ pre on B is flat sustained level before the hard cut — Pearson may return −∞.
    let nominal_pre = f.energy_pre_at(f.nominal_fill_start);
    assert!(
        true_pre > nominal_pre,
        "pause₁ pre energy should exceed pause₂: {true_pre} vs {nominal_pre}"
    );
}

#[test]
fn integration_f1_unified_match_finds_offset() {
    let f = build_f1_integration(48_000, 2);
    let matched = f
        .unified_match(GapSignatureMode::Energy, structure_heavy_weights())
        .expect("integration F1 unified");
    assert!(
        f.within_bin_tolerance(matched.alignment.start_frame, f.true_fill_start),
        "integration F1 start {} true {}",
        matched.alignment.start_frame,
        f.true_fill_start
    );
}

#[test]
fn u1_f1_energy_prefers_true_pre_score() {
    let f = build_f1();
    let true_score = f.energy_pre_at(f.true_fill_start);
    let decoy_score = f.energy_pre_at(f.nominal_fill_start);
    assert!(
        true_score > decoy_score,
        "U1: energy pre true={true_score} decoy={decoy_score}"
    );
}

#[test]
fn u2_f1_bool_pre_scores_ambiguous() {
    let f = build_f1();
    let true_score = f.bool_pre_at(f.true_fill_start);
    let decoy_score = f.bool_pre_at(f.nominal_fill_start);
    assert!(
        (true_score - decoy_score).abs() <= BOOL_AMBIGUITY_EPS,
        "U2: bool pre true={true_score} decoy={decoy_score}"
    );
}

#[test]
fn u3_f1_energy_unified_finds_true_offset() {
    let f = build_f1();
    let weights = structure_heavy_weights();
    let matched = f
        .unified_match(GapSignatureMode::Energy, weights)
        .expect("U3: energy unified match");
    assert!(
        f.within_bin_tolerance(matched.alignment.start_frame, f.true_fill_start),
        "U3: start {} true {}",
        matched.alignment.start_frame,
        f.true_fill_start
    );
}

#[test]
fn u4_f1_bool_unified_closer_to_decoy_than_energy() {
    let f = build_f1();
    let weights = structure_heavy_weights();
    let bool_match = f
        .unified_match(GapSignatureMode::Bool, weights)
        .expect("U4: bool unified match");
    let energy_match = f
        .unified_match(GapSignatureMode::Energy, weights)
        .expect("U4: energy unified match");
    assert!(
        f.within_bin_tolerance(energy_match.alignment.start_frame, f.true_fill_start),
        "U4: energy start {} true {}",
        energy_match.alignment.start_frame,
        f.true_fill_start,
    );
    let energy_dist = energy_match
        .alignment
        .start_frame
        .abs_diff(f.true_fill_start);
    let bool_dist = bool_match.alignment.start_frame.abs_diff(f.true_fill_start);
    assert!(
        bool_match.alignment.start_frame == f.nominal_fill_start || bool_dist >= energy_dist,
        "U4: bool start {} energy start {} nominal {} true {} (bool_dist={bool_dist}, energy_dist={energy_dist})",
        bool_match.alignment.start_frame,
        energy_match.alignment.start_frame,
        f.nominal_fill_start,
        f.true_fill_start,
    );
}

#[test]
fn u5_f2_energy_unified_finds_pause_one() {
    let f = build_f2();
    let weights = structure_heavy_weights();
    let matched = f
        .unified_match(GapSignatureMode::Energy, weights)
        .expect("U5: energy unified match");
    assert!(
        f.within_bin_tolerance(matched.alignment.start_frame, f.true_fill_start),
        "U5: start {} pause1 {}",
        matched.alignment.start_frame,
        f.true_fill_start
    );
}

#[test]
fn u5c_f2_scaled_48k_energy_unified_finds_pause_one() {
    let f = build_f2_at_rate(48_000, 2);
    let weights = structure_heavy_weights();
    let matched = f
        .unified_match(GapSignatureMode::Energy, weights)
        .expect("U5c: energy unified match");
    assert!(
        f.within_bin_tolerance(matched.alignment.start_frame, f.true_fill_start),
        "U5c: start {} pause1 {}",
        matched.alignment.start_frame,
        f.true_fill_start,
    );
}

#[test]
fn u5b_f2_integration_energy_unified_finds_pause_one() {
    let f = build_f2_integration(48_000, 2);
    let weights = structure_heavy_weights();
    let matched = f
        .unified_match(GapSignatureMode::Energy, weights)
        .expect("U5b: energy unified match");
    assert!(
        f.within_bin_tolerance(matched.alignment.start_frame, f.true_fill_start),
        "U5b: start {} pause1 {} (nominal {})",
        matched.alignment.start_frame,
        f.true_fill_start,
        f.nominal_fill_start,
    );
}

#[test]
fn u6_f2_bool_ambiguous_or_picks_nominal_pause() {
    let f = build_f2();
    let weights = structure_heavy_weights();
    let bool_match = f
        .unified_match(GapSignatureMode::Bool, weights)
        .expect("U6: bool unified match");
    let pre_pause1 = f.bool_pre_at(f.true_fill_start);
    let pre_pause2 = f.bool_pre_at(f.nominal_fill_start);
    assert!(
        bool_match.alignment.start_frame == f.nominal_fill_start
            || bool_match.alignment.start_frame < f.nominal_fill_end
            || (pre_pause1 - pre_pause2).abs() <= BOOL_AMBIGUITY_EPS,
        "U6: bool start {} nominal {} (pre1={pre_pause1}, pre2={pre_pause2})",
        bool_match.alignment.start_frame,
        f.nominal_fill_start,
    );
    let energy_pre1 = f.energy_pre_at(f.true_fill_start);
    let energy_pre2 = f.energy_pre_at(f.nominal_fill_start);
    assert!(
        energy_pre1 >= energy_pre2 + ENERGY_PAUSE_MARGIN,
        "F2 fixture: energy should separate pauses ({energy_pre1} vs {energy_pre2})"
    );
}

#[test]
fn u7_f3_silence_auto_falls_back_to_bool() {
    let f = build_f3_silence();
    let sig = build_gap_signature(
        &f.a_samples,
        f.channels,
        f.gap_start,
        f.gap_end,
        f.context_frames,
        &f.structure_params,
        GapSignatureMode::Auto,
    );
    assert!(
        matches!(sig, GapSignature::Bool(_)),
        "U7: auto on flat silence should resolve to bool"
    );
}

#[test]
fn u8_f3_drone_energy_and_bool_scores_agree() {
    let f = build_f3_drone();
    let bool_pre = f.bool_pre_at(f.nominal_fill_start);
    let energy_pre = f.energy_pre_at(f.nominal_fill_start);
    let bool_post = f.bool_post_at(f.nominal_fill_end);
    let energy_post = f.energy_post_at(f.nominal_fill_end);
    if energy_pre.is_finite() && bool_pre.is_finite() {
        assert!((energy_pre - bool_pre).abs() <= MODE_SCORE_EPS, "U8 pre");
    }
    if energy_post.is_finite() && bool_post.is_finite() {
        assert!((energy_post - bool_post).abs() <= MODE_SCORE_EPS, "U8 post");
    }
    assert!(
        bool_pre.is_finite() || bool_post.is_finite(),
        "U8: bool scores should be finite on drone fixture"
    );
}

#[test]
#[ignore = "tier:oracle — EC-1 production geometry (60s); test-tier.ps1 -Tier oracle"]
/// **EC-1 (domain oracle only):** energy unified match on full B at true offset.
/// Patch-layer criteria (bool decoy, production weights) deferred until scan→patch e2e lands.
/// PR covers EC-1 at integration scale via `u3_f1_energy_unified_finds_true_offset`.
fn p1_f1_production_energy_unified_finds_true_offset() {
    let f = build_f1_production(48_000, 2, 3.0);
    let matched = f
        .unified_match(GapSignatureMode::Energy, structure_heavy_weights())
        .expect("P1: energy unified on F1-long");
    assert!(
        f.within_bin_tolerance(matched.alignment.start_frame, f.true_fill_start),
        "P1: start {} true {}",
        matched.alignment.start_frame,
        f.true_fill_start,
    );
}

#[test]
#[ignore = "tier:oracle — EC-2 production geometry (90s); test-tier.ps1 -Tier oracle"]
/// **EC-2 (domain oracle only):** energy unified match at pause₁ on F2-long.
/// Slide ≈ 0 and patch-layer checks deferred until scan→patch e2e lands.
/// PR covers EC-2 at integration scale via `u5_f2_energy_unified_finds_pause_one`.
fn p2_f2_production_energy_unified_finds_pause_one() {
    let f = build_f2_production(48_000, 2, 90.0, 3.0);
    let matched = f
        .unified_match(GapSignatureMode::Energy, structure_heavy_weights())
        .expect("P2: energy unified on F2-long");
    assert!(
        f.within_bin_tolerance(matched.alignment.start_frame, f.true_fill_start),
        "P2: start {} pause1 {}",
        matched.alignment.start_frame,
        f.true_fill_start,
    );
}

#[test]
fn p3_f3_production_auto_resolves_to_bool() {
    let f = build_f3_drone_production(48_000, 2, 60.0, 3.0);
    let sig = build_gap_signature(
        &f.a_samples,
        f.channels,
        f.gap_start,
        f.gap_end,
        f.context_frames,
        &f.structure_params,
        GapSignatureMode::Auto,
    );
    assert!(
        matches!(sig, GapSignature::Bool(_)),
        "P3: auto on flat drone should resolve to bool"
    );
}

/// **EC-6 (domain oracle, fast):** on the F4 decoy fixture the **energy** envelope separates the
/// true pause from the decoy, while the **bool** active/silent pattern is identical at both —
/// the discrimination F2 could not produce (score-level, no search; see the `#[ignore]`d
/// `p4_f4_decoy_unified_search_diverges` for the placement-level check).
#[test]
fn p4_f4_decoy_energy_separates_but_bool_ties() {
    let f = build_f4_decoy_production(48_000, 2, 90.0, 3.0);

    // Energy: true pause clearly outscores the decoy on both halves.
    let e_pre_true = f.energy_pre_at(f.true_fill_start);
    let e_pre_decoy = f.energy_pre_at(f.nominal_fill_start);
    assert!(
        e_pre_true > e_pre_decoy + ENERGY_PAUSE_MARGIN,
        "P4 energy pre: truth {e_pre_true} should beat decoy {e_pre_decoy} by ≥ {ENERGY_PAUSE_MARGIN}",
    );

    // Bool: identical active/silent pattern → scores tie (well within the ambiguity band).
    let b_pre_true = f.bool_pre_at(f.true_fill_start);
    let b_pre_decoy = f.bool_pre_at(f.nominal_fill_start);
    assert!(
        (b_pre_true - b_pre_decoy).abs() <= BOOL_AMBIGUITY_EPS,
        "P4 bool pre: truth {b_pre_true} and decoy {b_pre_decoy} must be ambiguous (≤ {BOOL_AMBIGUITY_EPS})",
    );
}

/// **EC-6 (domain oracle, placement):** full unified search — `energy` lands on the true pause,
/// `bool` ties and `prefer_start` keeps it at the decoy nominal. Ignored: the per-candidate
/// waveform correlation over a dense 90 s B costs ~1 min in debug.
#[test]
#[ignore = "tier:oracle — EC06 unified search (~1 min debug); test-tier.ps1 -Tier oracle"]
fn p4_f4_decoy_unified_search_diverges() {
    let f = build_f4_decoy_production(48_000, 2, 90.0, 3.0);

    let energy = f
        .unified_match(GapSignatureMode::Energy, structure_heavy_weights())
        .expect("P4: energy unified on F4-decoy");
    assert!(
        f.within_bin_tolerance(energy.alignment.start_frame, f.true_fill_start),
        "P4 energy start {} should be at truth {}",
        energy.alignment.start_frame,
        f.true_fill_start,
    );

    let bool_match = f
        .unified_match(GapSignatureMode::Bool, structure_heavy_weights())
        .expect("P4: bool unified on F4-decoy");
    assert!(
        f.within_bin_tolerance(bool_match.alignment.start_frame, f.nominal_fill_start),
        "P4 bool start {} should stay at decoy {}",
        bool_match.alignment.start_frame,
        f.nominal_fill_start,
    );

    assert!(
        energy
            .alignment
            .start_frame
            .abs_diff(bool_match.alignment.start_frame)
            > f.bin_frames(),
        "P4: energy ({}) and bool ({}) placements must diverge",
        energy.alignment.start_frame,
        bool_match.alignment.start_frame,
    );
}


#[test]
#[ignore = "tier:oracle — F1 haystack scan vs oracle; test-tier.ps1 -Tier oracle"]
fn f1_production_haystack_scan_vs_oracle() {
    let temp = tempfile::tempdir().expect("tempdir");
    let fixture = build_f1_production(48_000, 2, 3.0);
    let repair = production_repair_config(GapSignatureMode::Energy, 3.0);
    let params = production_geometry_params(&repair);
    let weights = structure_heavy_weights();

    let scan_report = scan_gaps_for_fixture(&fixture, temp.path());
    let scan_gap = scan_report
        .gaps
        .iter()
        .find(|g| g.is_fillable())
        .expect("fillable scan gap");
    let scan_preview = preview_patch_geometry(
        &fixture,
        &scan_report.alignment,
        scan_gap.video_a_start_secs,
        scan_gap.video_a_end_secs,
        scan_gap.video_b_start_secs.unwrap_or(0.0),
        scan_gap.video_b_end_secs.unwrap_or(0.0),
        &params,
    );

    let (oracle_a_start, oracle_a_end, oracle_b_start, oracle_b_end, _) =
        gap_report_times(&fixture);
    let oracle_report = gap_report_from_energy_fixture(temp.path(), &fixture);
    let oracle_preview = preview_patch_geometry(
        &fixture,
        &oracle_report.alignment,
        oracle_a_start,
        oracle_a_end,
        oracle_b_start,
        oracle_b_end,
        &params,
    );

    eprintln!("{}", scan_preview.format_diagnostic(&fixture));
    eprintln!("{}", oracle_preview.format_diagnostic(&fixture));

    let scan_haystack = scan_preview
        .unified_match_on_haystack(&fixture, GapSignatureMode::Energy, weights);
    let oracle_haystack = oracle_preview
        .unified_match_on_haystack(&fixture, GapSignatureMode::Energy, weights);

    assert!(
        oracle_preview.true_within_search_radius,
        "oracle control: true fill must be within search radius"
    );
    assert!(
        oracle_haystack.is_some(),
        "oracle haystack unified match should succeed"
    );

    if !scan_preview.true_within_search_radius {
        eprintln!("scan path: true fill outside search radius (expected blocker)");
    }
    if scan_haystack.is_none() {
        eprintln!("scan path: haystack unified match failed");
    }
}

#[test]
#[ignore = "tier:oracle — EC01 patch control; test-tier.ps1 -Tier oracle"]
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
#[ignore = "tier:oracle — EC02 patch smoke; test-tier.ps1 -Tier oracle"]
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
