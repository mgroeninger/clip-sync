//! Energy signature patch integration — **SP01–SP03** (`i1_`–`i3_`).
//!
//! Tier: **integration**. 8 s F1/F2 fixtures: domain + haystack + full `PatchAudio` via
//! `assert_energy_integration_patch` (`clip-sync-repair-harness`).
//!
//! PR: **no** — full `integration` tier only (PR uses `corpus_scan_patch_smoke` + SD domain rows).
//!
//! Run: `cargo test -p clip-sync-repair --test integration_energy_patch`

use clip_sync_repair::domain::GapSignatureMode;
use clip_sync_repair_fixtures::energy_signature_fixtures::{
    build_f1_integration, build_f2_integration, structure_heavy_weights, structure_slide_secs,
};
use clip_sync_repair_fixtures::energy_signature_production::gap_report_from_energy_fixture;
use clip_sync_repair_harness::patch_audio::{
    assert_domain_energy_finds_truth, assert_energy_integration_patch, energy_sig_patch_options,
};

const ENERGY_SIG_RATE: u32 = 48_000;
const CHANNELS: usize = 2;

#[test]
fn i1_f1_energy_finds_true_offset_domain_and_patch_when_aligned() {
    let temp = tempfile::tempdir().expect("tempdir");
    let fixture = build_f1_integration(ENERGY_SIG_RATE, CHANNELS);
    let options = energy_sig_patch_options(GapSignatureMode::Energy);
    let report = gap_report_from_energy_fixture(temp.path(), &fixture);
    assert_energy_integration_patch(&fixture, report, options, "I1", None);
}

#[test]
fn i2_f1_bool_domain_closer_to_decoy_than_energy() {
    let fixture = build_f1_integration(ENERGY_SIG_RATE, CHANNELS);
    assert_domain_energy_finds_truth(&fixture);

    let energy = fixture
        .unified_match(GapSignatureMode::Energy, structure_heavy_weights())
        .expect("I2 energy domain");
    let bool_match = fixture
        .unified_match(GapSignatureMode::Bool, structure_heavy_weights())
        .expect("I2 bool domain");
    let truth = structure_slide_secs(&fixture, fixture.true_fill_start);
    let energy_dist = (structure_slide_secs(&fixture, energy.alignment.start_frame) - truth).abs();
    let bool_dist = (structure_slide_secs(&fixture, bool_match.alignment.start_frame) - truth).abs();
    assert!(
        bool_match.alignment.start_frame == fixture.b_decoy_fill_start()
            || bool_match.alignment.start_frame == fixture.nominal_fill_start
            || bool_dist >= energy_dist,
        "I2: bool start {} energy start {} decoy {} (bool_dist={bool_dist}, energy_dist={energy_dist})",
        bool_match.alignment.start_frame,
        energy.alignment.start_frame,
        fixture.b_decoy_fill_start(),
    );
}

#[test]
fn i3_f2_energy_finds_pause_one_domain_and_patch_when_aligned() {
    let temp = tempfile::tempdir().expect("tempdir");
    let fixture = build_f2_integration(ENERGY_SIG_RATE, CHANNELS);
    let options = energy_sig_patch_options(GapSignatureMode::Energy);
    let report = gap_report_from_energy_fixture(temp.path(), &fixture);
    assert_energy_integration_patch(&fixture, report, options, "I3", Some(0.0));
}
