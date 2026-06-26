//! Patch geometry CSV diagnostics (energy-signature I1 / I3).

use clip_sync_repair::test_support::energy_signature_fixtures::{
    build_f1_integration, build_f2_integration,
};
use clip_sync_repair::test_support::energy_signature_production::gap_report_from_energy_fixture;
use clip_sync_repair::domain::GapSignatureMode;
use clip_sync_repair_harness::patch_audio::{energy_sig_patch_diagnostic, energy_sig_patch_options};

const ENERGY_SIG_RATE: u32 = 48_000;
const CHANNELS: usize = 2;

#[test]
fn i1_f1_patch_diagnostic() {
    let temp = tempfile::tempdir().expect("tempdir");
    let fixture = build_f1_integration(ENERGY_SIG_RATE, CHANNELS);
    let options = energy_sig_patch_options(GapSignatureMode::Energy);
    let report = gap_report_from_energy_fixture(temp.path(), &fixture);
    energy_sig_patch_diagnostic(&fixture, &report, options, "I1 F1");
}

#[test]
fn i3_f2_patch_diagnostic() {
    let temp = tempfile::tempdir().expect("tempdir");
    let fixture = build_f2_integration(ENERGY_SIG_RATE, CHANNELS);
    let options = energy_sig_patch_options(GapSignatureMode::Energy);
    let report = gap_report_from_energy_fixture(temp.path(), &fixture);
    energy_sig_patch_diagnostic(&fixture, &report, options, "I3 F2");
}
