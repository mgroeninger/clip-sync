//! W5 anchor-rescue single-cell diagnostic (Phase 1).
//! See docs/dev/archive/TEMP-w5-anchor-rescue-diag-plan.md.
//!
//! Tier: **diagnostic** (`diagnostic-tests` feature). Emits per-cell nominal/baseline throat Pearson
//! plus per-bracket unified gate scores for human review — replaces `probe_w5_anchor_rescue_scores`.
//!
//! PR: **no** — diagnostic tier with `--nocapture`.
//!
//! Run: `cargo test -p clip-sync-repair --features diagnostic-tests --test diag_w5_anchor_rescue -- --nocapture`

use clip_sync_repair::domain::gap_anchor_seam::AnchorSeamMode;
use clip_sync_repair_fixtures::energy_signature_fixtures::build_w5_noise_collar_anchor_rescue;
use clip_sync_repair_fixtures::energy_signature_production::w5_anchor_rescue_repair;
use clip_sync_repair_fixtures::w5_anchor_rescue_diag::{
    score_w5_anchor_rescue_cell, score_w5_fixture, W5AnchorRescueCell, W5AnchorRescueCellScores,
    W5FixtureScores,
};
use clip_sync_repair_harness::w5_anchor_rescue_sweep::{
    anchor_rescue_pocket, coarse_w5_grid_default, decoupled_w5_grid, refine_w5_boundaries,
    sweep_csv, write_w5_sweep_csv, W5SweepCell,
};

const CSV_HEADER: &str = "peak_offset_secs,fill_border_search_secs,nominal_pre,nominal_post,\
baseline_pre,baseline_post,bracket_pre,bracket_post,bracket_move,passed_gate,pre_pearson,\
post_pearson,min_pearson,confidence,ranking_score,wall_ms,failure_stage";

fn opt(value: Option<f64>) -> String {
    value.map(|v| format!("{v:.4}")).unwrap_or_default()
}

fn print_cell_csv(scores: &W5AnchorRescueCellScores) {
    let c = &scores.cell;
    println!("{CSV_HEADER}");
    // Summary row (bracket columns empty).
    println!(
        "{:.3},{:.3},{:.4},{:.4},{:.4},{:.4},,,,,,,,,,{},",
        c.peak_offset_secs,
        c.fill_border_search_secs,
        scores.nominal_pre,
        scores.nominal_post,
        scores.baseline_pre,
        scores.baseline_post,
        scores.wall_ms,
    );
    // One row per bracket.
    for b in &scores.brackets {
        let confidence = b.confidence.map(|c| format!("{c:?}")).unwrap_or_default();
        println!(
            "{:.3},{:.3},{:.4},{:.4},{:.4},{:.4},{},{},{},{},{},{},{},{},{},,{}",
            c.peak_offset_secs,
            c.fill_border_search_secs,
            scores.nominal_pre,
            scores.nominal_post,
            scores.baseline_pre,
            scores.baseline_post,
            b.pre_frame,
            b.post_frame,
            b.move_frames,
            b.passed_gate,
            opt(b.pre_pearson),
            opt(b.post_pearson),
            opt(b.min_pearson),
            confidence,
            opt(b.ranking_score),
            b.failure_stage.unwrap_or(""),
        );
    }
}

fn print_human_summary(scores: &W5AnchorRescueCellScores) {
    let c = &scores.cell;
    eprintln!(
        "W5 cell (peak_offset={:.3}s, fill_border_search={:.3}s) — {} ms",
        c.peak_offset_secs, c.fill_border_search_secs, scores.wall_ms
    );
    eprintln!(
        "  nominal throat pearson : pre={:.4} post={:.4} min={:.4}",
        scores.nominal_pre,
        scores.nominal_post,
        scores.nominal_pre.min(scores.nominal_post),
    );
    eprintln!(
        "  baseline unified throat: pre={:.4} post={:.4} min={:.4}",
        scores.baseline_pre,
        scores.baseline_post,
        scores.baseline_pre.min(scores.baseline_post),
    );
    eprintln!("  feasible brackets: {}", scores.brackets.len());
    for b in &scores.brackets {
        if b.passed_gate {
            eprintln!(
                "    bracket pre={} post={} move={} -> gate ok: pre={:.4} post={:.4} min={:.4} conf={:?} rank={:.4}",
                b.pre_frame,
                b.post_frame,
                b.move_frames,
                b.pre_pearson.unwrap_or(f64::NAN),
                b.post_pearson.unwrap_or(f64::NAN),
                b.min_pearson.unwrap_or(f64::NAN),
                b.confidence,
                b.ranking_score.unwrap_or(f64::NAN),
            );
        } else {
            eprintln!(
                "    bracket pre={} post={} move={} -> gate FAILED [{}]: pre={} post={}",
                b.pre_frame,
                b.post_frame,
                b.move_frames,
                b.failure_stage.unwrap_or("?"),
                b.pre_pearson
                    .map(|v| format!("{v:.4}"))
                    .unwrap_or_else(|| "—".into()),
                b.post_pearson
                    .map(|v| format!("{v:.4}"))
                    .unwrap_or_else(|| "—".into()),
            );
        }
    }
}

#[test]
fn w5_anchor_rescue_single_cell() {
    let cell = W5AnchorRescueCell::default(); // (1.0, 0.78)
    let scores = score_w5_anchor_rescue_cell(cell);
    print_cell_csv(&scores);
    print_human_summary(&scores);
}

/// Phase 2 coarse grid: regime map across `(peak_offset, fill_border_search)`. Emits CSV (stdout,
/// and under `target/w5_anchor_rescue_sweep.csv` when `W5_SWEEP_CSV=1`) plus a regime tally and the
/// E3 pocket (if any). See docs/dev/archive/TEMP-w5-anchor-rescue-diag-plan.md §5.2.
#[test]
fn diag_w5_anchor_rescue_coarse_grid() {
    let cells = coarse_w5_grid_default();
    println!("{}", sweep_csv(&cells));
    report_sweep("coarse grid", &cells);
    maybe_write_csv(&cells, "w5_anchor_rescue_sweep.csv");
}

/// Phase 2 boundary refine: one bisection pass on cells whose 4-neighbour regime differs, to bracket
/// any `AnchorRescuePossible` pocket more tightly.
#[test]
fn diag_w5_anchor_rescue_refine_boundaries() {
    let coarse = coarse_w5_grid_default();
    let refined = refine_w5_boundaries(&coarse);
    println!("{}", sweep_csv(&refined));
    report_sweep("refined boundaries", &refined);
    maybe_write_csv(&refined, "w5_anchor_rescue_sweep_refined.csv");
}

fn print_fixture_scores(label: &str, s: &W5FixtureScores) {
    eprintln!("=== {label} ===");
    eprintln!("  nominal: pre={:.4} post={:.4}", s.nominal.0, s.nominal.1);
    match s.baseline {
        Some((pre, post)) => eprintln!(
            "  baseline throat: pre={pre:.4} post={post:.4} min={:.4}",
            pre.min(post)
        ),
        None => eprintln!("  baseline throat: DEGENERATE (no unified match)"),
    }
    eprintln!("  joint_winner: {:?}", s.joint_winner);
    eprintln!("  anchor_seam_would_run: {}", s.anchor_seam_would_run);
    eprintln!("  brackets: {}", s.brackets.len());
    for b in &s.brackets {
        let prom = format!(
            "prom(pre={:.3},post={:.3})",
            b.pre_prominence, b.post_prominence
        );
        if b.passed_gate {
            eprintln!(
                "    move={} {prom} -> PASS pre={:.4} post={:.4} min={:.4} conf={:?}",
                b.move_frames,
                b.pre_pearson.unwrap_or(f64::NAN),
                b.post_pearson.unwrap_or(f64::NAN),
                b.min_pearson.unwrap_or(f64::NAN),
                b.confidence,
            );
        } else {
            eprintln!(
                "    move={} {prom} -> FAIL [{}] pre={} post={}",
                b.move_frames,
                b.failure_stage.unwrap_or("?"),
                b.pre_pearson
                    .map(|v| format!("{v:.4}"))
                    .unwrap_or_else(|| "—".into()),
                b.post_pearson
                    .map(|v| format!("{v:.4}"))
                    .unwrap_or_else(|| "—".into()),
            );
        }
    }
}

/// §8 Q1 faithful (noise-collar) fixture probe: does a genuine W5 (noise collar at the gap, anchors
/// reachable only by a moving bracket) finally let a moving bracket pass at High? See
/// docs/dev/archive/TEMP-w5-anchor-rescue-diag-plan.md §8 Q1.
#[test]
fn diag_w5_noise_collar() {
    // (peak_offset, collar, burst, search_radius, min_prominence). Short triangular bursts + a
    // prominence floor between collar (~0.10 log-RMS) and burst should give ONE clean winner.
    for &(off, collar, burst, radius, prom) in &[
        (0.5_f64, 0.30_f64, 0.08_f64, 1.0_f64, 0.10_f32),
        (0.5, 0.30, 0.08, 1.0, 0.12),
        (0.5, 0.30, 0.06, 1.0, 0.10),
        (0.6, 0.30, 0.10, 1.2, 0.12),
    ] {
        let fixture = build_w5_noise_collar_anchor_rescue(48_000, 1, off, collar, burst);
        let mut repair = w5_anchor_rescue_repair(AnchorSeamMode::Auto, radius);
        repair.anchor_seam_min_prominence = prom;
        let scores = score_w5_fixture(&fixture, &repair);
        print_fixture_scores(
            &format!(
                "noise_collar off={off} collar={collar} burst={burst} radius={radius} prom={prom}"
            ),
            &scores,
        );
    }
}

/// §8 Q1 decoupled exploration: fix `peak_offset=1.0`, sweep `(search, b_shift)` to test whether
/// decoupling the B dropout shift from the A peak offset opens an E3 pocket the coupled grid cannot
/// reach (Phase 2 finding). See docs/dev/archive/TEMP-w5-anchor-rescue-diag-plan.md §8.
#[test]
fn diag_w5_anchor_rescue_decoupled_grid() {
    let cells = decoupled_w5_grid(1.0, (0.30, 0.70), 0.10, (0.60, 1.40), 0.10);
    println!("{}", sweep_csv(&cells));
    report_sweep("decoupled grid (peak_offset=1.0)", &cells);
    maybe_write_csv(&cells, "w5_anchor_rescue_sweep_decoupled.csv");
}

fn report_sweep(label: &str, cells: &[W5SweepCell]) {
    use std::collections::BTreeMap;
    let mut tally: BTreeMap<&str, usize> = BTreeMap::new();
    for c in cells {
        *tally.entry(c.regime.as_str()).or_default() += 1;
    }
    eprintln!("=== W5 {label}: {} cells ===", cells.len());
    for (regime, count) in &tally {
        eprintln!("  {regime}: {count}");
    }
    // Moving-bracket failure analysis — why E3 never wins (instrumentation, plan §8 Q1).
    let mut stage_tally: BTreeMap<&str, usize> = BTreeMap::new();
    let mut moving_pass = 0usize;
    let mut best_aligned_moving_post = f64::NAN; // best post among structure-aligned moving brackets
    for c in cells {
        for b in &c.eval.scores.brackets {
            if b.move_frames == 0 {
                continue;
            }
            if b.passed_gate {
                moving_pass += 1;
                continue;
            }
            if let Some(stage) = b.failure_stage {
                *stage_tally.entry(stage).or_default() += 1;
            }
            // Brackets that reached the waveform floor stage are structure-aligned; their post
            // Pearson tells us how far a moving bracket can get toward the 0.35 High floor.
            if b.failure_stage == Some("waveform_floor") {
                if let Some(post) = b.post_pearson {
                    best_aligned_moving_post = if best_aligned_moving_post.is_nan() {
                        post
                    } else {
                        best_aligned_moving_post.max(post)
                    };
                }
            }
        }
    }
    eprintln!(
        "  moving brackets: {moving_pass} passed; failure stages: {stage_tally:?}; best aligned-moving post_pearson={best_aligned_moving_post:.4} (High floor 0.35)"
    );

    let pocket = anchor_rescue_pocket(cells);
    if pocket.is_empty() {
        eprintln!("  E3 pocket: EMPTY — no AnchorRescuePossible cell (plan §5.2.8)");
    } else {
        eprintln!("  E3 pocket: {} cell(s), best first:", pocket.len());
        for c in pocket.iter().take(5) {
            eprintln!(
                "    offset={:.3} search={:.3} max_bracket_min={:?}",
                c.peak_offset_secs(),
                c.fill_border_search_secs(),
                c.eval.scores.max_bracket_min(),
            );
        }
    }
}

fn maybe_write_csv(cells: &[W5SweepCell], name: &str) {
    if std::env::var("W5_SWEEP_CSV").as_deref() != Ok("1") {
        return;
    }
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("target")
        .join(name);
    match write_w5_sweep_csv(cells, &path) {
        Ok(()) => eprintln!("wrote {}", path.display()),
        Err(e) => eprintln!("failed to write {}: {e}", path.display()),
    }
}
