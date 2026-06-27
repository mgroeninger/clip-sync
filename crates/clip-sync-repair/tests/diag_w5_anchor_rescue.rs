//! W5 anchor-rescue single-cell diagnostic (Phase 1).
//! See docs/TEMP-w5-anchor-rescue-diag-plan.md.
//!
//! Tier: **diagnostic** (`diagnostic-tests` feature). Emits per-cell nominal/baseline throat Pearson
//! plus per-bracket unified gate scores for human review — replaces `probe_w5_anchor_rescue_scores`.
//!
//! PR: **no** — diagnostic tier with `--nocapture`.
//!
//! Run: `cargo test -p clip-sync-repair --features diagnostic-tests --test diag_w5_anchor_rescue -- --nocapture`

use clip_sync_repair::test_support::w5_anchor_rescue_diag::{
    score_w5_anchor_rescue_cell, W5AnchorRescueCell, W5AnchorRescueCellScores,
};

const CSV_HEADER: &str = "peak_offset_secs,fill_border_search_secs,nominal_pre,nominal_post,\
baseline_pre,baseline_post,bracket_pre,bracket_post,bracket_move,passed_gate,pre_pearson,\
post_pearson,min_pearson,confidence,ranking_score,wall_ms";

fn opt(value: Option<f64>) -> String {
    value.map(|v| format!("{v:.4}")).unwrap_or_default()
}

fn print_cell_csv(scores: &W5AnchorRescueCellScores) {
    let c = &scores.cell;
    println!("{CSV_HEADER}");
    // Summary row (bracket columns empty).
    println!(
        "{:.3},{:.3},{:.4},{:.4},{:.4},{:.4},,,,,,,,,,{}",
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
        let confidence = b
            .confidence
            .map(|c| format!("{c:?}"))
            .unwrap_or_default();
        println!(
            "{:.3},{:.3},{:.4},{:.4},{:.4},{:.4},{},{},{},{},{},{},{},{},{},",
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
                "    bracket pre={} post={} move={} -> gate FAILED",
                b.pre_frame, b.post_frame, b.move_frames
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
