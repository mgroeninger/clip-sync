//! W5 **timing-offset** recoverability diagnostic (Phase C).
//! See `docs/archive/TEMP-w5-timing-offset-diag-plan.md` §5 Phase C.
//!
//! Tier: **diagnostic** (`diagnostic-tests` feature). Emits data for human review; no PR gate.
//!
//! `diag_w5_timing_offset_recoverability_grid` — sweeps the g003 fixture over
//! `(seam_offset_ms, drift_ppm)` and reports where the seam stops being recoverable by a bounded shift
//! (offset beyond the lag search, or drift smearing the peak below the 0.5 floor). Fast (no gate).
//!
//! `diag_w5_timing_offset_gate_probe` — runs a few cells through the production unified gate
//! (`score_w5_fixture`) to confirm the gate genuinely *skips* a recoverable seam (`waveform_floor`),
//! i.e. the timing offset is invisible to the lag-0 seam score. Slower (full haystack search).
//!
//! Run (release recommended):
//! `cargo test -p clip-sync-repair --release --features diagnostic-tests --test diag_w5_timing_offset -- --nocapture`

use clip_sync_repair::domain::gap_anchor_seam::AnchorSeamMode;
use clip_sync_repair_fixtures::energy_signature_fixtures::build_w5_timing_offset_seam;
use clip_sync_repair_fixtures::energy_signature_production::w5_anchor_rescue_repair;
use clip_sync_repair_fixtures::w5_anchor_rescue_diag::{score_w5_fixture, W5JointWinner};
use clip_sync_repair_fixtures::w5_timing_offset_diag::{
    w5_timing_offset_csv, w5_timing_offset_grid_default, W5TimingOffsetCell, COLLAR_SECS,
    PEAK_OFFSET_SECS, SR,
};

#[test]
fn diag_w5_timing_offset_recoverability_grid() {
    let cells = w5_timing_offset_grid_default();
    println!("{}", w5_timing_offset_csv(&cells));
    report_grid("recoverability grid", &cells);
    maybe_write_csv(&cells, "w5_timing_offset_sweep.csv");
}

/// How does the production gate treat a recoverable timing-offset seam? Asserts the fixture is
/// **skip-faithful** to g003: a **drifting** offset is *skipped* (every bracket `waveform_floor`, no
/// recovery), while a **constant** offset is *not* skipped (the baseline haystack slide recovers it —
/// a constant offset is a clip-level shift, not g003).
///
/// This is the payoff of the Phase A refinement (plan §6): a **continuous non-stationary broadband
/// bed** (not isolated tone bursts). Drift makes no single B placement align both seams (the pre↔post
/// lag split exceeds the bed's autocorrelation width), and the bed's energy-peak anchors are embedded
/// in continuous drifting content so no bracket move recovers — exactly g003's skip. Earlier stationary
/// / isolated-burst fixtures let the gate *escape* the offset (baseline slide for a constant offset, a
/// moving bracket onto an isolated burst for drift); see the plan for that progression.
///
/// **Slow** (full unified gate per cell; ~60 s release / cell) — `#[ignore]`d from the default
/// diagnostic lane; run via `test-tier.ps1` with `--ignored`.
#[test]
#[ignore = "slow: full unified gate per cell (~60s release) — run via test-tier --ignored"]
fn diag_w5_timing_offset_gate_probe() {
    // (offset_ms, drift_ppm, expect_skip). Constant offset → recoverable; any real drift → g003 skip.
    for &(off_ms, drift_ppm, expect_skip) in &[
        (16.0_f64, 0.0_f64, false),
        (16.0, -4_500.0, true),
        (8.0, -4_500.0, true),
        (32.0, -9_000.0, true),
    ] {
        let fixture = build_w5_timing_offset_seam(SR, 1, PEAK_OFFSET_SECS, COLLAR_SECS, off_ms, drift_ppm);
        let repair = w5_anchor_rescue_repair(AnchorSeamMode::Auto, 1.0);
        let scores = score_w5_fixture(&fixture, &repair);

        eprintln!("=== gate probe: offset={off_ms} ms drift={drift_ppm} ppm (expect_skip={expect_skip}) ===");
        match scores.baseline {
            Some((pre, post)) => eprintln!(
                "  baseline throat: pre={pre:.4} post={post:.4} min={:.4} (gate's lag-0 view)",
                pre.min(post)
            ),
            None => eprintln!("  baseline throat: DEGENERATE (no unified match)"),
        }
        eprintln!("  joint_winner: {:?}", scores.joint_winner);
        let passed = scores.brackets.iter().filter(|b| b.passed_gate).count();
        eprintln!("  brackets: {} ({} passed gate)", scores.brackets.len(), passed);
        let mut stages = std::collections::BTreeMap::<&str, usize>::new();
        for b in scores.brackets.iter().filter(|b| !b.passed_gate) {
            *stages.entry(b.failure_stage.unwrap_or("?")).or_default() += 1;
        }
        eprintln!("  failure stages (rejected brackets): {stages:?}");

        let skipped = matches!(scores.joint_winner, W5JointWinner::Skip);
        if expect_skip {
            assert!(
                skipped && passed == 0,
                "drift case (offset={off_ms} ms, drift={drift_ppm} ppm) must be SKIPPED (g003-faithful): \
                 winner={:?}, {passed} brackets passed",
                scores.joint_winner
            );
        } else {
            assert!(
                !skipped,
                "constant offset (offset={off_ms} ms) must be recoverable (not g003): winner={:?}",
                scores.joint_winner
            );
        }
    }
}

fn report_grid(label: &str, cells: &[W5TimingOffsetCell]) {
    let total = cells.len();
    let recoverable = cells.iter().filter(|c| c.recoverable()).count();
    let degenerate = cells.iter().filter(|c| c.lag.is_none()).count();
    eprintln!("=== W5 timing-offset {label}: {total} cells ===");
    eprintln!("  recoverable (both seams timing_offset): {recoverable}");
    eprintln!("  not recoverable: {}", total - recoverable - degenerate);
    if degenerate > 0 {
        eprintln!("  degenerate (window out of range): {degenerate}");
    }

    // Recoverability boundary per drift level: the largest offset that still recovers.
    use std::collections::BTreeMap;
    let mut by_drift: BTreeMap<i64, (f64, f64)> = BTreeMap::new(); // drift -> (max recoverable off, min broken off)
    for c in cells {
        let e = by_drift.entry(c.drift_ppm as i64).or_insert((f64::NAN, f64::NAN));
        if c.recoverable() {
            e.0 = if e.0.is_nan() { c.seam_offset_ms } else { e.0.max(c.seam_offset_ms) };
        } else if c.lag.is_some() {
            e.1 = if e.1.is_nan() { c.seam_offset_ms } else { e.1.min(c.seam_offset_ms) };
        }
    }
    eprintln!("  boundary (max recoverable offset → first broken offset), per drift:");
    for (drift, (max_ok, min_broken)) in &by_drift {
        eprintln!("    drift={drift:>7} ppm: recoverable ≤ {max_ok:.0} ms, breaks at {min_broken:.0} ms");
    }
}

fn maybe_write_csv(cells: &[W5TimingOffsetCell], name: &str) {
    if std::env::var("W5_SWEEP_CSV").as_deref() != Ok("1") {
        return;
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("target")
        .join(name);
    match std::fs::write(&path, w5_timing_offset_csv(cells)) {
        Ok(()) => eprintln!("wrote {}", path.display()),
        Err(e) => eprintln!("failed to write {}: {e}", path.display()),
    }
}
