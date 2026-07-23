//! **B2** — live re-characterization smoke on real, open-licensed media.
//!
//! Tier: **validation** (`validation-tests` feature). Runs a real jump-cut oracle through ffmpeg
//! codecs and needs `ffmpeg` on PATH and `.\scripts\fetch_corpus_sources.ps1`.
//!
//! PR: **no** — `.\scripts\test-tier.ps1 -Tier validation -Package clip-sync-repair`.
//!
//! Run: `cargo test -p clip-sync-repair --features validation-tests --test validate_dual_fit_oracle -- --nocapture`
//!
//! Closes the gap flagged in `docs/dev/archive/TEMP-pipeline-perf-redesign-plan.md` §4.7 (B2): the golden
//! invariance harness (`golden_baseline_corpus_invariance`) only diffs a frozen `corpus.json` — it
//! never calls the fingerprint dump (`characterize_gaps_from_decode`) or the real production `PatchAudio` path against
//! decoded audio. This test runs the ACTUAL production entry point
//! (`residual_gate::run_built_floor_oracle_cfg` → `PatchAudio::execute`) on a real, licensed-media
//! jump-cut gap and asserts the Tier-1 §4.1 boolean the golden harness cares about most for A3:
//! `dual_fit_used`.

use clip_sync_repair::domain::ResidualGateMode;
use clip_sync_repair_harness::dual_fit_oracle::{
    build_stepped_floor_oracle_pair, load_manifest, require_case_sources, DualFitOracleCase,
    DualFitOracleManifest,
};
use clip_sync_repair_harness::floor_oracle::require_validation_env;
use clip_sync_repair_harness::residual_gate::{production_fit_repair_config, run_built_floor_oracle_cfg};

fn manifest_case<'a>(manifest: &'a DualFitOracleManifest, id: &str) -> &'a DualFitOracleCase {
    manifest
        .case
        .iter()
        .find(|c| c.id == id)
        .unwrap_or_else(|| panic!("missing dual-fit oracle case {id}"))
}

/// A genuine jump-cut on a real Wikimedia speech master, run through the production `PatchAudio`
/// entry point, must be rescued via **G6 dual-fit** — not the ordinary rigid bracket search —
/// because no single lag satisfies both seams (§4.4 "step-real"). Proves the A3 wiring
/// (`skip_or_dual_fit` → `try_dual_fit`) end-to-end on real codecs/content, not just the synthetic
/// unit fixtures in `domain::dual_fit::tests`.
#[test]
fn dual_fit_rescues_real_media_jump_cut_wav() {
    require_validation_env();
    let manifest = load_manifest(clip_sync_repair_harness::repair_tests_dir!());
    let case = manifest_case(&manifest, "cc_speech_dual_fit_jump_cut_wav");
    require_case_sources(case);

    let temp = tempfile::tempdir().expect("tempdir");
    let built = build_stepped_floor_oracle_pair(&temp.path().join(&case.id), case, &manifest.defaults);

    let mut repair = production_fit_repair_config(ResidualGateMode::Off);
    repair.dual_fit = true;
    let run = run_built_floor_oracle_cfg(&built, &repair);
    assert_eq!(
        run.status, "patched",
        "{}: jump-cut gap should be rescued (skip={})",
        case.id, run.skip_reason
    );
    assert!(
        run.dual_fit_used,
        "{}: real-media jump-cut gap must be rescued via the A3 dual-fit path (G6), not the ordinary bracket search",
        case.id
    );

    // Fixture invariant: with dual-fit OFF, the same rigid-placement gate must NOT patch this
    // gap — that's the entire premise of the jump-cut (a single lag can't satisfy both seams). If
    // this fails, the fixture's step_ms is too small or the gate is too lenient to exercise G6.
    let mut no_dual_fit = repair.clone();
    no_dual_fit.dual_fit = false;
    let baseline = run_built_floor_oracle_cfg(&built, &no_dual_fit);
    assert_ne!(
        baseline.status, "patched",
        "{}: fixture invariant broken — rigid placement patched WITHOUT dual-fit",
        case.id
    );
}

/// Same jump-cut, real AAC round-trip on both sides — confirms G6 rescue survives lossy
/// re-encoding, not just the WAV case.
#[test]
fn dual_fit_rescues_real_media_jump_cut_aac() {
    require_validation_env();
    let manifest = load_manifest(clip_sync_repair_harness::repair_tests_dir!());
    let case = manifest_case(&manifest, "cc_speech_dual_fit_jump_cut_aac");
    require_case_sources(case);

    let temp = tempfile::tempdir().expect("tempdir");
    let built = build_stepped_floor_oracle_pair(&temp.path().join(&case.id), case, &manifest.defaults);

    let mut repair = production_fit_repair_config(ResidualGateMode::Off);
    repair.dual_fit = true;
    let run = run_built_floor_oracle_cfg(&built, &repair);
    assert_eq!(
        run.status, "patched",
        "{}: jump-cut gap should be rescued (skip={})",
        case.id, run.skip_reason
    );
    assert!(
        run.dual_fit_used,
        "{}: real-media (AAC) jump-cut gap must be rescued via the A3 dual-fit path (G6)",
        case.id
    );
}
