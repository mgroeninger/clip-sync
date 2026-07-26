//! Perf §4 decision-invariance — re-analyze the curated per-gap-type fixtures and diff against the frozen
//! golden snapshot (gap cells: `docs/dev/gap-vocabulary.md`).
//!
//! Tier: **pr-repair** — media-free (the committed fixtures + committed golden are the input), so no
//! `#[ignore]`, no `gap-files/`, no `validation-tests` feature. (Was `gap-files/re-anchor`-dependent until
//! Phase 3.)
//!
//! The golden is **self-hosting**: it is regenerated *from* the committed fixtures, so no external media is
//! ever needed to reproduce it. Regenerate after an intentional analyzer change with:
//! ```powershell
//! $env:CURATED_GOLDEN_REGEN = "1"
//! cargo test -p clip-sync-repair --test golden_baseline_invariance
//! Remove-Item Env:\CURATED_GOLDEN_REGEN
//! ```
//! The per-type classification footguns live in `gap_cell_fixtures.rs` (Phase 2).

use std::path::{Path, PathBuf};

use clip_sync_repair_harness::gap_fingerprint_corpus::curated_gap_cell_rows;
use clip_sync_repair_harness::golden_baseline::{
    baseline_from_rows, diff_baselines, parse_golden_baseline, TIER2_ABS_EPS,
};

/// Committed golden, alongside the other golden baselines in the harness crate.
fn golden_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("clip-sync-repair-harness")
        .join("golden")
        .join("curated.golden.json")
}

#[test]
fn curated_golden_baseline_invariance() {
    let live = baseline_from_rows(&curated_gap_cell_rows());
    assert!(live.gap_count > 0, "no curated fixtures analyzed");
    let path = golden_path();

    if std::env::var("CURATED_GOLDEN_REGEN").as_deref() == Ok("1") {
        let json = serde_json::to_string_pretty(&live).expect("serialize golden");
        std::fs::write(&path, json).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
        eprintln!("regenerated {}", path.display());
        return;
    }

    let committed = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "read {} ({e}) — first-time generation: run with CURATED_GOLDEN_REGEN=1",
            path.display()
        )
    });
    let expected = parse_golden_baseline(&committed).expect("parse frozen curated golden JSON");

    let diffs = diff_baselines(&expected, &live, TIER2_ABS_EPS);
    assert!(
        diffs.is_empty(),
        "curated golden baseline drift ({} mismatch{}):\n{}\nregenerate with CURATED_GOLDEN_REGEN=1 if intended",
        diffs.len(),
        if diffs.len() == 1 { "" } else { "es" },
        diffs.iter().take(20).cloned().collect::<Vec<_>>().join("\n"),
    );
}

/// **Property 2 of the fill-placement re-baseline, asserted in the negative** — see
/// docs/dev/TEMP-fill-placement-axis-plan.md Phase A.
///
/// `fill_start_frame` / `fill_frames` are Tier-1 axes whose purpose is to turn the golden red when a
/// fill length changes. They are currently **null on every curated gap**, because the committed
/// fixtures were extracted before `compute_region_measurements` emitted the gate's chosen placement.
/// A null column is indistinguishable from a passing one, so the gap is asserted rather than left to
/// be discovered.
///
/// **When the fixtures are regenerated from media, this test will fail — that is the point.** Replace
/// it with the positive form (placement is `Some` wherever the gap had a bracket with a complete seam
/// pair), which is the assertion that actually arms the tripwire. Deleting it instead re-opens the
/// blind spot the plan exists to close.
///
/// `fill_pre_r` / `fill_post_r` are *not* covered here: they read the pre-existing `seam_pre` /
/// `seam_post` and are already populated on the 6 gaps that have an eligible bracket.
#[test]
fn curated_golden_fill_placement_is_not_yet_armed() {
    let live = baseline_from_rows(&curated_gap_cell_rows());
    // Without this the emptiness assertion below would pass vacuously on an empty fixture set.
    assert!(live.gap_count > 0, "no curated fixtures analyzed");
    let armed: Vec<String> = live
        .gaps
        .iter()
        .filter(|g| g.fill_start_frame.is_some() || g.fill_frames.is_some())
        .map(|g| format!("{}·g{}", g.pair, g.index))
        .collect();
    assert!(
        armed.is_empty(),
        "fill placement is now populated on {} curated gap(s): {}\n\
         The fixtures have been regenerated — replace this test with the positive assertion \
         (placement present wherever the bracket had a complete seam pair) and regenerate the golden. \
         See docs/dev/TEMP-fill-placement-axis-plan.md Phase A.",
        armed.len(),
        armed.join(", "),
    );
}

