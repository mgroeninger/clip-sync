//! Seam residual CSV diagnostics.
//!
//! Tier: **diagnostic** (`diagnostic-tests` feature). Score/disagreement dumps for F1/F2/F4 and
//! broadband fixtures — human review, not pass/fail contract.
//!
//! PR: **no** — diagnostic tier with `-Nocapture`.
//!
//! Run: `cargo test -p clip-sync-repair --features diagnostic-tests --test diag_seam_residual -- --nocapture`

use clip_sync_repair::test_support::energy_signature_fixtures::{
    build_f1_production, build_f2_production, build_f4_decoy_production,
};
use clip_sync_repair_harness::seam_residual::{
    build_broadband, build_broadband_with, run_disagreement_fixture, run_fixture, score_at, Variant,
};

#[test]
fn seam_residual_broadband_csv() {
    println!(
        "fixture,variant,placement,oracle_correct,seam_pre,seam_post,\
         residual_pre_db,residual_post_db,floor_pre_db,floor_post_db,\
         headroom_pre_db,headroom_post_db,floor_source_pre,floor_source_post"
    );
    for variant in [Variant::Clean, Variant::CodecNoise, Variant::CodecNoiseShift] {
        run_fixture(&build_broadband(16_000, variant), variant.label());
    }
}

/// Placement-offset sweep (unified model): floor anchored at the true fill, chosen placement moved
/// `offset` frames off it (B itself is aligned). With seam and floor sharing one lag radius
/// (`residual_lag_secs`), a true fill offset within that radius recovers → headroom ≈ 0; beyond it
/// headroom grows (correct reject). Codec noise on → realistic floor.

#[test]
fn seam_residual_alignment_sweep_csv() {
    println!(
        "rate,offset_samples,seam_pre,seam_post,residual_pre_db,residual_post_db,\
         floor_pre_db,floor_post_db,headroom_pre_db,headroom_post_db"
    );
    let rate = 16_000u32;
    let fixture = build_broadband_with(rate, 40.0, 0.0); // B aligned; vary the *placement* instead
    let offsets = [0i64, 16, 32, 64, 100, 200, 400, 512, 600, 1000];
    for offset in offsets {
        let start = (fixture.true_fill_start as i64 + offset) as usize;
        let s = score_at(&fixture, start);
        println!(
            "{},{},{:.3},{:.3},{:.1},{:.1},{:.1},{:.1},{:.1},{:.1}",
            rate,
            offset,
            s.seam_pre,
            s.seam_post,
            s.residual_pre_db,
            s.residual_post_db,
            s.floor_pre_db,
            s.floor_post_db,
            s.headroom_pre(),
            s.headroom_post(),
        );
    }
}

/// F4 decoy at the bool nominal: Pearson accepts (~0.84) but residual headroom is huge — the EC-6
/// veto case. Fast score-level check (no full patch search).

#[test]
fn seam_residual_truth_decoy_csv() {
    println!(
        "fixture,variant,placement,oracle_correct,seam_pre,seam_post,\
         residual_pre_db,residual_post_db,floor_pre_db,floor_post_db,\
         headroom_pre_db,headroom_post_db,floor_source_pre,floor_source_post"
    );

    run_fixture(&build_f1_production(48_000, 2, 3.0), "clean");
    run_fixture(&build_f2_production(48_000, 2, 90.0, 3.0), "clean");
    run_fixture(&build_f4_decoy_production(48_000, 2, 90.0, 3.0), "clean");
}

#[test]
fn seam_residual_disagreement_csv() {
    println!(
        "fixture,variant,placement,oracle_correct,pearson_min,pearson_tier,\
         pearson_patches,informative,headroom_db,veto_outcome,rescue_outcome,\
         veto_flipped,rescue_flipped"
    );

    run_disagreement_fixture(&build_f1_production(48_000, 2, 3.0), "clean");
    run_disagreement_fixture(&build_f2_production(48_000, 2, 90.0, 3.0), "clean");
    run_disagreement_fixture(&build_f4_decoy_production(48_000, 2, 90.0, 3.0), "clean");

    for variant in [Variant::Clean, Variant::CodecNoise] {
        run_disagreement_fixture(&build_broadband(16_000, variant), variant.label());
    }
}

