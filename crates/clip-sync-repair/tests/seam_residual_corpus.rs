//! Seam residual integration oracles (score-level acceptance).
//!
//! Tier: **integration**. F4 decoy headroom placement and `seam_residual_disagreement_oracles`
//! score harness (Pearson vs residual disagreement at truth/decoy placements).
//!
//! PR: **yes** — `pr-repair` (full binary).
//!
//! Run: `cargo test -p clip-sync-repair --test seam_residual_corpus`

use clip_sync_repair::domain::gap_fill_fit::{
    apply_residual_to_confidence, FillConfidence, ResidualGateError,
};
use clip_sync_repair::domain::policies::DEFAULT_RESIDUAL_FLOOR_OK_DB;
use clip_sync_repair::domain::DEFAULT_RESIDUAL_HEADROOM_MARGIN_DB;
use clip_sync_repair::test_support::energy_signature_fixtures::{
    build_f1_production, build_f4_decoy_production,
};
use clip_sync_repair_harness::seam_residual::{
    build_broadband, disagreement_at, score_placement, GateOutcomeLabel, PearsonTierLabel, Variant,
};

#[test]
fn f4_decoy_placement_informative_with_high_headroom() {
    let fixture = build_f4_decoy_production(48_000, 2, 90.0, 3.0);
    let decoy = fixture.b_decoy_fill_start();
    let placement = score_placement(&fixture, decoy);
    let s = &placement.scored;

    let pearson_min = s.pearson_pre.min(s.pearson_post);
    assert!(
        pearson_min >= 0.35,
        "F4 decoy Pearson {pearson_min:.3} should pass min_fill_correlation",
    );

    // Real verdict — `informative` is computed from the measured floor via `from_parts`.
    let verdict = placement.verdict();
    assert!(
        verdict.informative,
        "F4 decoy floor pre={:.1} post={:.1} should be informative (≤ {DEFAULT_RESIDUAL_FLOOR_OK_DB})",
        verdict.floor_pre_db,
        verdict.floor_post_db,
    );
    let headroom = verdict.worst_headroom_db();
    assert!(
        headroom > DEFAULT_RESIDUAL_HEADROOM_MARGIN_DB,
        "F4 decoy headroom {headroom:.1} dB should exceed veto margin {}",
        DEFAULT_RESIDUAL_HEADROOM_MARGIN_DB,
    );

    let err = apply_residual_to_confidence(
        Ok(FillConfidence::High),
        &verdict,
        DEFAULT_RESIDUAL_HEADROOM_MARGIN_DB,
        false,
    )
    .unwrap_err();
    assert!(matches!(err, ResidualGateError::HeadroomExceeded { .. }));
}

#[test]
fn seam_residual_disagreement_oracles() {
    let f4 = build_f4_decoy_production(48_000, 2, 90.0, 3.0);
    let f4_decoy = disagreement_at(&f4, "decoy");
    assert!(
        f4_decoy.pearson_patches,
        "F4 decoy Pearson should pass production floor"
    );
    assert!(f4_decoy.informative, "F4 decoy floor should be informative");
    assert_eq!(f4_decoy.veto_outcome, GateOutcomeLabel::Veto, "F4 decoy veto case");
    assert!(
        f4_decoy.pearson_patches && f4_decoy.veto_outcome == GateOutcomeLabel::Veto,
        "F4 decoy: Pearson pass → veto skip"
    );

    let f1 = build_f1_production(48_000, 2, 3.0);
    let f1_truth = disagreement_at(&f1, "truth");
    assert!(f1_truth.pearson_patches, "F1 truth should pass Pearson");
    assert_eq!(
        f1_truth.veto_outcome,
        GateOutcomeLabel::Pass,
        "F1 truth: veto agrees with Pearson"
    );
    assert!(
        !f1_truth.veto_flipped(),
        "F1 truth should not flip under veto"
    );

    let broadband = build_broadband(16_000, Variant::CodecNoise);
    let bb_truth = disagreement_at(&broadband, "truth");
    assert_eq!(
        bb_truth.pearson_tier,
        PearsonTierLabel::DeadZone,
        "broadband codec-noise truth: Pearson dead zone"
    );
    assert!(bb_truth.informative, "broadband truth floor informative");
    assert!(
        bb_truth.headroom_db <= DEFAULT_RESIDUAL_HEADROOM_MARGIN_DB,
        "broadband truth headroom {:.1} should be within margin",
        bb_truth.headroom_db
    );
    assert_eq!(
        bb_truth.rescue_outcome,
        GateOutcomeLabel::Rescue,
        "broadband H2-B: rescue from dead zone"
    );
    assert!(
        bb_truth.rescue_flipped(),
        "broadband rescue should flip Pearson skip → marginal"
    );
}
