//! Patch-pipeline validation — **SP05** production-default fit smoke.
//!
//! Tier: **validation** (`validation-tests` feature). `patch_audio_fit_production_defaults_smoke`
//! — sine fixtures with `fill_border_search_secs = 10` and full extension grid.
//!
//! PR: **no** — run before release via validation tier.
//!
//! Run: `cargo test -p clip-sync-repair --features validation-tests --test validate_patch_audio`

use clip_sync_repair_harness::patch_audio::run_fit_production_defaults_smoke;

#[test]
fn patch_audio_fit_production_defaults_smoke() {
    run_fit_production_defaults_smoke();
}
