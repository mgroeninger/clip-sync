//! Production-scale energy signature corpus: mode matrix (integration crate).

use std::time::Instant;

use clip_sync::{SymphoniaMediaReader};
use clip_sync::testing::fakes::FakeProgressReporter;

use clip_sync_repair::application::PatchAudio;
use clip_sync_repair::domain::GapSignatureMode;
use clip_sync_repair::infrastructure::config::RepairConfig;
use clip_sync_repair::test_support::energy_signature_fixtures::build_f1_production;
use clip_sync_repair::test_support::energy_signature_production::{
    patch_request_from_repair, production_repair_config, scan_gaps_for_fixture,
};

#[test]
#[ignore = "matrix: cargo test -p clip-sync-repair energy_signature_mode_matrix -- --ignored --nocapture"]
fn energy_signature_mode_matrix() {
    let temp = tempfile::tempdir().expect("tempdir");
    let fixture = build_f1_production(48_000, 2, 3.0);
    let report = scan_gaps_for_fixture(&fixture, temp.path());
    let repair_defaults = RepairConfig::default();
    let patch = PatchAudio::new(&SymphoniaMediaReader, &FakeProgressReporter);

    eprintln!("fixture,mode,context_secs,patched,skipped,marginal,wall_ms");
    for mode in [
        GapSignatureMode::Bool,
        GapSignatureMode::Energy,
        GapSignatureMode::Auto,
    ] {
        for context in [3.0, 10.0, 30.0] {
            let repair = production_repair_config(mode, context);
            let request = patch_request_from_repair(report.clone(), &repair);
            let started = Instant::now();
            let result = patch
                .execute(request, repair_defaults.crossfade_ms)
                .expect("matrix patch");
            eprintln!(
                "F1-long,{mode:?},{context},{},{},{},{}",
                result.summary.patched_count,
                result.summary.skipped_count,
                result.summary.patched_marginal_count,
                started.elapsed().as_millis(),
            );
        }
    }
}
