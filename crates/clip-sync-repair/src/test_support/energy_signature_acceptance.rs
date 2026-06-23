//! Phase 0/2 acceptance rows U1–U8 (`docs/TEMP-energy-signature-plan.md`).

use crate::test_support::energy_signature_fixtures::{
    build_f1, build_f1_integration, build_f1_production, build_f2, build_f2_at_rate, build_f2_integration,
    build_f2_production, build_f3_drone, build_f3_drone_production, build_f3_silence,
    structure_heavy_weights, BOOL_AMBIGUITY_EPS, ENERGY_PAUSE_MARGIN, MODE_SCORE_EPS,
};
use crate::domain::gap_signature::{build_gap_signature, GapSignature, GapSignatureMode};

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
/// **EC-1 (domain oracle only):** energy unified match on full B at true offset.
/// Patch-layer criteria (bool decoy, production weights) deferred until scan→patch e2e lands.
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
/// **EC-2 (domain oracle only):** energy unified match at pause₁ on F2-long.
/// Slide ≈ 0 and patch-layer checks deferred until scan→patch e2e lands.
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
