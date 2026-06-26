//! Patch-pipeline validation (production-default fit smokes; not energy/residual-gate rows).

use clip_sync_repair_harness::patch_audio::run_fit_production_defaults_smoke;

#[test]
fn patch_audio_fit_production_defaults_smoke() {
    run_fit_production_defaults_smoke();
}
