//! Gap scan corpus integration (`tests/gap_corpus/manifest.toml`).
//!
//! Tier: **integration**. Committed WAV scan cases run on PR; generated, external, patch-timing,
//! and regenerate rows are `#[ignore]`.
//!
//! PR: **yes** (committed scan: `gap_corpus_committed`, `gap_corpus_manifest_loads`) — `pr-repair`.
//!
//! Run: `cargo test -p clip-sync-repair --test integration_gap_corpus`
//! Ignored: `cargo test -p clip-sync-repair --test integration_gap_corpus gap_corpus_generated -- --ignored`

use clip_sync_repair::test_support::gap_corpus_fixtures::{
    corpus_root, load_manifest, run_gap_corpus_manifest_cases,
    run_gap_corpus_patch_timing_cases, run_gap_corpus_patch_timing_production_cases,
    write_committed_wav_fixtures, GapCorpusTier,
};

#[test]
#[ignore = "run manually to regenerate: cargo test -p clip-sync-repair --test integration_gap_corpus gap_corpus_regenerate -- --ignored --nocapture"]
fn gap_corpus_regenerate_committed_wav_fixtures() {
    write_committed_wav_fixtures();
    eprintln!("wrote fixtures under {}", corpus_root().join("wav").display());
}

#[test]
fn gap_corpus_manifest_loads() {
    let manifest = load_manifest();
    assert!(manifest.version >= 1, "manifest version should be >= 1");
    assert!(
        !manifest.case.is_empty(),
        "manifest should have at least one case"
    );
    let committed: Vec<_> = manifest
        .case
        .iter()
        .filter(|c| c.tier == GapCorpusTier::Committed)
        .collect();
    assert!(
        !committed.is_empty(),
        "manifest should have at least one committed case"
    );
}

#[test]
fn gap_corpus_committed() {
    run_gap_corpus_manifest_cases(GapCorpusTier::Committed);
}

#[test]
#[ignore = "generates WAV fixtures at test time; cargo test -p clip-sync-repair --test integration_gap_corpus gap_corpus_generated -- --ignored"]
fn gap_corpus_generated() {
    run_gap_corpus_manifest_cases(GapCorpusTier::Generated);
}

#[test]
#[ignore = "requires CLIP_SYNC_GAP_CORPUS env var pointing to real media files"]
fn gap_corpus_external() {
    if std::env::var("CLIP_SYNC_GAP_CORPUS").is_err() {
        eprintln!("skipping gap_corpus_external: CLIP_SYNC_GAP_CORPUS not set");
        return;
    }
    run_gap_corpus_manifest_cases(GapCorpusTier::External);
}

#[test]
#[ignore = "wall-clock budget; cargo test -p clip-sync-repair --test integration_gap_corpus gap_corpus_patch_timing_committed -- --ignored --nocapture"]
fn gap_corpus_patch_timing_committed() {
    run_gap_corpus_patch_timing_cases(GapCorpusTier::Committed);
}

#[test]
#[ignore = "production-default fit; cargo test -p clip-sync-repair --test integration_gap_corpus gap_corpus_patch_timing_production -- --ignored --nocapture"]
fn gap_corpus_patch_timing_production() {
    run_gap_corpus_patch_timing_production_cases(GapCorpusTier::Committed);
}

#[test]
#[ignore = "generates WAV fixtures at test time; cargo test -p clip-sync-repair --test integration_gap_corpus gap_corpus_patch_timing_generated -- --ignored"]
fn gap_corpus_patch_timing_generated() {
    run_gap_corpus_patch_timing_cases(GapCorpusTier::Generated);
}
