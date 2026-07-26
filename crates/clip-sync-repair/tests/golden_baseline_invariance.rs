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

/// **Property 2 of the fill-placement re-baseline** — see docs/dev/archive/TEMP-fill-placement-axis-plan.md
/// Phase A.
///
/// `fill_start_frame` / `fill_frames` are Tier-1 axes whose purpose is to turn the golden red when a
/// fill length changes. Fixtures 01–10 were extracted before `compute_region_measurements` emitted the
/// gate's chosen placement, so they carry the axes as null — and a null column is indistinguishable
/// from a passing one. Fixtures **11 and 12** were harvested after the emit landed and are the ones
/// that actually arm the tripwire; this test asserts at least one such gap exists, so the coverage
/// cannot silently regress to all-null.
///
/// **Placement is scoped to the gate's `Ok` branch, and the golden reads only the *best* bracket.**
/// `seam_pre` / `seam_post` are populated from gate *failures* too (`stage_of`), and
/// `best_seam_bracket` ranks by min-seam over every bracket with a complete seam pair — **failures
/// included**. So the correct predicate is *the best-by-min-seam bracket passed the gate*, which is
/// strictly narrower than *some bracket passed*: a gap whose min-seam winner failed yields null
/// placement even though other brackets passed. Fixture 11 exercises exactly this shape (20 brackets,
/// 5 `waveform_floor` failures carrying seams with null placement, best bracket passing).
///
/// `fill_pre_r` / `fill_post_r` are *not* covered here: they read the pre-existing `seam_pre` /
/// `seam_post` and are populated wherever a bracket has a complete seam pair, pass or fail.
#[test]
fn curated_golden_fill_placement_is_armed() {
    let live = baseline_from_rows(&curated_gap_cell_rows());
    // Without this the assertions below would pass vacuously on an empty fixture set.
    assert!(live.gap_count > 0, "no curated fixtures analyzed");

    let armed: Vec<String> = live
        .gaps
        .iter()
        .filter(|g| g.fill_start_frame.is_some() && g.fill_frames.is_some())
        .map(|g| format!("{}·g{}", g.pair, g.index))
        .collect();
    assert!(
        !armed.is_empty(),
        "no curated gap carries fill placement — the Tier-1 fill_start_frame / fill_frames tripwire is \
         disarmed and would pass vacuously. A fixture whose BEST-by-min-seam bracket passes the gate \
         must be present (fixtures 11/12). See docs/dev/archive/TEMP-fill-placement-axis-plan.md Phase A, \
         property 2.",
    );

    // The two axes are read from one `FillAlignment`, so they are populated or absent together. A gap
    // carrying exactly one means the projection in `compute_region_measurements` has been split.
    let half: Vec<String> = live
        .gaps
        .iter()
        .filter(|g| g.fill_start_frame.is_some() != g.fill_frames.is_some())
        .map(|g| format!("{}·g{}", g.pair, g.index))
        .collect();
    assert!(
        half.is_empty(),
        "{} curated gap(s) carry only one of fill_start_frame / fill_frames: {}\n\
         Both project from the same FillAlignment on the gate's Ok branch, so they must move together.",
        half.len(),
        half.join(", "),
    );
}

