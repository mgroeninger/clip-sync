//! **8f differential** — the fingerprint projection preserves every decision axis, checked on the curated
//! per-gap-type fixture set (gap cells: `docs/gap-vocabulary.md`).
//!
//! Tier: **pr-repair** — media-free (the committed fixtures are the input), so no `#[ignore]`, no corpus on
//! disk, no `validation-tests` feature. (Was a `gap-files/`-dependent validation test until Phase 3.)
//!
//! In-process differential: for each committed fixture we build its analyzer row twice — from the fixture as
//! committed, and after routing its corpus through the tags→fingerprint projection
//! (`corpus_projection::project_corpus`) — and assert the golden-baseline D/R axes are identical (Tier-1
//! bit-exact, Tier-2 within ε). This proves `GapRepairTags` is a complete decision carrier and the projection
//! is faithful, without any real media.

use clip_sync_repair_harness::gap_fingerprint_corpus::{
    curated_gap_cell_projected_rows, curated_gap_cell_rows,
};
use clip_sync_repair_harness::golden_baseline::{baseline_from_rows, diff_baselines, TIER2_ABS_EPS};

#[test]
fn projection_preserves_curated_golden_baseline() {
    let old = baseline_from_rows(&curated_gap_cell_rows());
    assert!(old.gap_count > 0, "no curated fixtures");
    let new = baseline_from_rows(&curated_gap_cell_projected_rows());

    let diffs = diff_baselines(&old, &new, TIER2_ABS_EPS);
    assert!(
        diffs.is_empty(),
        "projection changed {} decision-axis field(s) across {} curated gaps:\n{}",
        diffs.len(),
        old.gap_count,
        diffs.iter().take(30).cloned().collect::<Vec<_>>().join("\n"),
    );
}
