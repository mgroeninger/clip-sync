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
use clip_sync_repair_fixtures::energy_signature_fixtures::{
    build_f1_production, build_f4_decoy_production, channel_noise, overwrite_channels,
};
use clip_sync_repair_harness::seam_residual::{
    build_broadband, disagreement_at, pearson_and_residual_selected_channels, score_placement,
    score_placement_multichannel, GateOutcomeLabel, PearsonTierLabel, Variant,
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

/// Pearson seam scoring and residual/floor measurement must pick the same energy-selected channels.
#[test]
fn seam_residual_channel_selection_matches_pearson() {
    let stereo = build_f1_production(48_000, 2, 3.0);
    let (pearson, residual) =
        pearson_and_residual_selected_channels(&stereo, stereo.true_fill_start);
    assert_eq!(
        pearson, residual,
        "stereo F1: Pearson and residual channel selection must match"
    );

    let channels = 6usize;
    let center = 2usize;
    let mut center_dominant = build_f1_production(48_000, channels, 3.0);
    let center_peak = center_dominant
        .a_samples
        .iter()
        .skip(center)
        .step_by(channels)
        .fold(0.0f32, |m, &s| m.max(s.abs()));
    let surr_amp = center_peak * 0.05;
    let surrounds: Vec<usize> = (0..channels).filter(|&c| c != center).collect();
    overwrite_channels(
        &mut center_dominant.a_samples,
        channels,
        &surrounds,
        channel_noise(0xA1, surr_amp),
    );
    overwrite_channels(
        &mut center_dominant.b_samples,
        channels,
        &surrounds,
        channel_noise(0xB2, surr_amp),
    );
    let (pearson, residual) =
        pearson_and_residual_selected_channels(&center_dominant, center_dominant.true_fill_start);
    assert_eq!(
        pearson, residual,
        "center-dominant 6ch: Pearson and residual channel selection must match"
    );
    assert_eq!(residual, vec![center]);
}

/// Center-dominant 6ch (plan §7 1a): the per-channel scoring path must follow the center channel —
/// the only one carrying signal — for residual/floor, where a mono downmix would be diluted by the
/// five quiet surrounds. This is the PR-CI guard on the now-live default-on veto for multichannel
/// (and, by the same path, stereo) gaps; the real-pipeline counterpart is the diagnostic-tier
/// `seam_residual_oracle_center_dominant_6ch`.
#[test]
fn seam_residual_center_dominant_follows_center_channel() {
    let channels = 6usize;
    let center = 2usize; // FC in FL FR FC LFE Ls Rs
    let mut fixture = build_f1_production(48_000, channels, 3.0);

    // Demote the surrounds to quiet, decorrelated noise relative to the center's own level: ~5 % of
    // the center peak (≈ −26 dB, mean-square ratio ≈ 0.0025) so they stay below the ~20 dB selection
    // gate, yet are present enough to dilute a mono downmix. Different seeds on A vs B → no cancel.
    let center_peak = fixture
        .a_samples
        .iter()
        .skip(center)
        .step_by(channels)
        .fold(0.0f32, |m, &s| m.max(s.abs()));
    let surr_amp = center_peak * 0.05;
    let surrounds: Vec<usize> = (0..channels).filter(|&c| c != center).collect();
    overwrite_channels(&mut fixture.a_samples, channels, &surrounds, channel_noise(0xA1, surr_amp));
    overwrite_channels(&mut fixture.b_samples, channels, &surrounds, channel_noise(0xB2, surr_amp));

    let truth = fixture.true_fill_start;
    let (pearson_sel, residual_sel) = pearson_and_residual_selected_channels(&fixture, truth);
    assert_eq!(pearson_sel, residual_sel, "Pearson and residual selection must match");
    let mc = score_placement_multichannel(&fixture, truth);
    let mono = score_placement(&fixture, truth);

    // Selection narrowed to the center (surrounds are below the energy gate).
    assert_eq!(
        mc.selected_channels,
        vec![center],
        "only the center channel should be selected; got {:?}",
        mc.selected_channels
    );
    // The center cancels at truth → regime established, headroom within the veto margin (gate passes).
    assert!(
        mc.verdict.informative,
        "center cancellation should be informative: floor pre={:.1} post={:.1}",
        mc.verdict.floor_pre_db,
        mc.verdict.floor_post_db,
    );
    assert!(
        mc.verdict.worst_headroom_db() <= DEFAULT_RESIDUAL_HEADROOM_MARGIN_DB,
        "center-dominant headroom {:.1} dB should be within veto margin {:.1} dB",
        mc.verdict.worst_headroom_db(),
        DEFAULT_RESIDUAL_HEADROOM_MARGIN_DB,
    );
    // Documents the fix: the mono downmix, diluted by the surrounds, cancels worse than the
    // per-channel center measurement on the identical fixture.
    assert!(
        mc.verdict.worst_floor_db() < mono.verdict().worst_floor_db() - 2.0,
        "per-channel floor {:.1} dB should be meaningfully deeper than the mono downmix {:.1} dB",
        mc.verdict.worst_floor_db(),
        mono.verdict().worst_floor_db(),
    );
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
