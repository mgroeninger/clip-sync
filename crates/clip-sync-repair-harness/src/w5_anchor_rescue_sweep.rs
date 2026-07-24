//! W5 anchor-rescue grid sweep (Phase 2). See docs/dev/archive/TEMP-w5-anchor-rescue-diag-plan.md §5.2.
//!
//! Maps the `(peak_offset_secs, fill_border_search_secs)` plane into behavioral regimes so we can
//! locate the E3 (anchor-rescue) pocket — where an anchor bracket reaches Pearson High and wins the
//! production joint pool. Coarse grid first, then refine only where neighbours change regime.
//!
//! Per-cell scoring is the diagnostic `evaluate_w5_cell` (`clip-sync-repair-fixtures`); this module owns only
//! grid generation, boundary refinement, and CSV I/O.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;

use clip_sync_repair_fixtures::w5_anchor_rescue_diag::{
    classify_w5_cell, evaluate_w5_cell, W5AnchorRescueCell, W5AnchorRescueRegime, W5CellEvaluation,
    W5JointWinner,
};

/// Default coarse grid (plan §5.2): `peak_offset ∈ [0.6, 1.1]` step 0.05 s,
/// `search ∈ [0.65, 0.85]` step 0.02 s.
pub const DEFAULT_OFFSET_RANGE: (f64, f64) = (0.6, 1.1);
pub const DEFAULT_OFFSET_STEP: f64 = 0.05;
pub const DEFAULT_SEARCH_RANGE: (f64, f64) = (0.65, 0.85);
pub const DEFAULT_SEARCH_STEP: f64 = 0.02;

/// One evaluated grid cell: the diagnostic evaluation plus its regime label.
#[derive(Debug, Clone)]
pub struct W5SweepCell {
    pub eval: W5CellEvaluation,
    pub regime: W5AnchorRescueRegime,
}

impl W5SweepCell {
    fn evaluate(cell: W5AnchorRescueCell) -> Self {
        let eval = evaluate_w5_cell(cell);
        let regime = classify_w5_cell(&eval);
        W5SweepCell { eval, regime }
    }

    pub fn peak_offset_secs(&self) -> f64 {
        self.eval.scores.cell.peak_offset_secs
    }

    pub fn fill_border_search_secs(&self) -> f64 {
        self.eval.scores.cell.fill_border_search_secs
    }
}

/// Quantize a float axis value to a stable integer key (micro-seconds) for dedupe / neighbour maps.
fn key(value: f64) -> i64 {
    (value * 1_000_000.0).round() as i64
}

fn frange(range: (f64, f64), step: f64) -> Vec<f64> {
    let (lo, hi) = range;
    let mut out = Vec::new();
    let n = ((hi - lo) / step).round() as i64;
    for i in 0..=n {
        // Re-round to the step grid to avoid f64 drift accumulating across the range.
        out.push(((lo + i as f64 * step) * 1_000_000.0).round() / 1_000_000.0);
    }
    out
}

/// Build + evaluate the coarse grid, skipping the invalid half-plane (`search >= offset`).
pub fn coarse_w5_grid(
    offset_range: (f64, f64),
    offset_step: f64,
    search_range: (f64, f64),
    search_step: f64,
) -> Vec<W5SweepCell> {
    let offsets = frange(offset_range, offset_step);
    let searches = frange(search_range, search_step);
    let mut cells = Vec::new();
    for &peak_offset_secs in &offsets {
        for &fill_border_search_secs in &searches {
            if fill_border_search_secs >= peak_offset_secs {
                continue; // invalid half-plane — never evaluated
            }
            cells.push(W5SweepCell::evaluate(W5AnchorRescueCell::coupled(
                peak_offset_secs,
                fill_border_search_secs,
            )));
        }
    }
    cells
}

/// Decoupled (§8 Q1) sweep: fix `peak_offset_secs`, vary `(search, b_shift)`. Looks for the E3
/// pocket the coupled grid cannot reach — a moving anchor bracket at High while the throat baseline
/// stays weak. Skips `search >= offset` (invalid) and `b_shift <= search` (baseline would reach the
/// fill, so it cannot stay weak).
pub fn decoupled_w5_grid(
    peak_offset_secs: f64,
    search_range: (f64, f64),
    search_step: f64,
    b_shift_range: (f64, f64),
    b_shift_step: f64,
) -> Vec<W5SweepCell> {
    let searches = frange(search_range, search_step);
    let b_shifts = frange(b_shift_range, b_shift_step);
    let mut cells = Vec::new();
    for &fill_border_search_secs in &searches {
        if fill_border_search_secs >= peak_offset_secs {
            continue;
        }
        for &b_shift_secs in &b_shifts {
            if b_shift_secs <= fill_border_search_secs {
                continue; // baseline could reach the fill — not a weak-baseline regime
            }
            cells.push(W5SweepCell::evaluate(W5AnchorRescueCell {
                peak_offset_secs,
                fill_border_search_secs,
                b_shift_secs: Some(b_shift_secs),
            }));
        }
    }
    cells
}

/// Convenience: the default coarse grid.
pub fn coarse_w5_grid_default() -> Vec<W5SweepCell> {
    coarse_w5_grid(
        DEFAULT_OFFSET_RANGE,
        DEFAULT_OFFSET_STEP,
        DEFAULT_SEARCH_RANGE,
        DEFAULT_SEARCH_STEP,
    )
}

/// Insert midpoint cells on edges where 4-neighbour regimes differ, then re-score only the new
/// cells (plan §5.2.4, one bisection pass). Returns the merged set (existing + new), deduped.
pub fn refine_w5_boundaries(cells: &[W5SweepCell]) -> Vec<W5SweepCell> {
    let offset_step = DEFAULT_OFFSET_STEP;
    let search_step = DEFAULT_SEARCH_STEP;

    // Regime lookup by quantized (offset, search).
    let mut regime_at = std::collections::HashMap::new();
    for c in cells {
        regime_at.insert((key(c.peak_offset_secs()), key(c.fill_border_search_secs())), c.regime);
    }
    let existing: BTreeSet<(i64, i64)> = regime_at.keys().copied().collect();

    // Collect candidate midpoints on edges where neighbour regime differs.
    let mut midpoints: BTreeSet<(i64, i64)> = BTreeSet::new();
    for c in cells {
        let o = c.peak_offset_secs();
        let s = c.fill_border_search_secs();
        // East neighbour (offset + step): midpoint in offset.
        if let Some(&n) = regime_at.get(&(key(o + offset_step), key(s))) {
            if n != c.regime {
                let mid = ((o + offset_step / 2.0) * 1_000_000.0).round() / 1_000_000.0;
                if mid > s {
                    midpoints.insert((key(mid), key(s)));
                }
            }
        }
        // North neighbour (search + step): midpoint in search.
        if let Some(&n) = regime_at.get(&(key(o), key(s + search_step))) {
            if n != c.regime {
                let mid = ((s + search_step / 2.0) * 1_000_000.0).round() / 1_000_000.0;
                if mid < o {
                    midpoints.insert((key(o), key(mid)));
                }
            }
        }
    }

    let mut merged = cells.to_vec();
    for (ok, sk) in midpoints {
        if existing.contains(&(ok, sk)) {
            continue;
        }
        let peak_offset_secs = ok as f64 / 1_000_000.0;
        let fill_border_search_secs = sk as f64 / 1_000_000.0;
        merged.push(W5SweepCell::evaluate(W5AnchorRescueCell::coupled(
            peak_offset_secs,
            fill_border_search_secs,
        )));
    }
    merged
}

fn joint_winner_label(w: W5JointWinner) -> String {
    match w {
        W5JointWinner::Skip => "Skip".to_string(),
        W5JointWinner::Baseline => "Baseline".to_string(),
        W5JointWinner::Anchor { move_frames } => format!("Anchor({move_frames})"),
    }
}

/// CSV header for the sweep (plan §5.2.5; `b_shift_secs` added for §8 Q1 decoupled runs).
pub const SWEEP_CSV_HEADER: &str = "peak_offset_secs,fill_border_search_secs,b_shift_secs,regime,\
joint_winner,nominal_min,baseline_min,max_bracket_min,anchor_seam_would_run,bracket_count,\
passing_bracket_count,wall_ms";

const SWEEP_CSV_FIELDS: &[&str] = &[
    "peak_offset_secs",
    "fill_border_search_secs",
    "b_shift_secs",
    "regime",
    "joint_winner",
    "nominal_min",
    "baseline_min",
    "max_bracket_min",
    "anchor_seam_would_run",
    "bracket_count",
    "passing_bracket_count",
    "wall_ms",
];

fn opt(value: Option<f64>) -> String {
    value.map(|v| format!("{v:.4}")).unwrap_or_default()
}

/// One CSV row for a cell (RFC 4180 via the `csv` crate).
pub fn sweep_csv_row(cell: &W5SweepCell) -> String {
    let mut wtr = csv::WriterBuilder::new()
        .has_headers(false)
        .from_writer(Vec::new());
    write_sweep_row(&mut wtr, cell);
    let bytes = wtr.into_inner().expect("csv flush");
    let mut s = String::from_utf8(bytes).expect("csv utf8");
    if s.ends_with('\n') {
        s.pop();
        if s.ends_with('\r') {
            s.pop();
        }
    }
    s
}

fn write_sweep_row<W: std::io::Write>(wtr: &mut csv::Writer<W>, cell: &W5SweepCell) {
    let s = &cell.eval.scores;
    wtr.write_record([
        format!("{:.3}", s.cell.peak_offset_secs),
        format!("{:.3}", s.cell.fill_border_search_secs),
        format!("{:.3}", s.cell.effective_b_shift_secs()),
        cell.regime.as_str().to_string(),
        joint_winner_label(cell.eval.joint_winner),
        format!("{:.4}", s.nominal_pre.min(s.nominal_post)),
        format!("{:.4}", s.baseline_min()),
        opt(s.max_bracket_min()),
        cell.eval.anchor_seam_would_run.to_string(),
        s.brackets.len().to_string(),
        s.passing_bracket_count().to_string(),
        s.wall_ms.to_string(),
    ])
    .expect("csv row");
}

/// Render the full sweep CSV (header + one row per cell, sorted by `(offset, search)`).
pub fn sweep_csv(cells: &[W5SweepCell]) -> String {
    let mut sorted: Vec<&W5SweepCell> = cells.iter().collect();
    sorted.sort_by(|a, b| {
        (key(a.peak_offset_secs()), key(a.fill_border_search_secs()))
            .cmp(&(key(b.peak_offset_secs()), key(b.fill_border_search_secs())))
    });
    let mut wtr = csv::Writer::from_writer(Vec::new());
    wtr.write_record(SWEEP_CSV_FIELDS).expect("csv header");
    for c in sorted {
        write_sweep_row(&mut wtr, c);
    }
    let bytes = wtr.into_inner().expect("csv flush");
    String::from_utf8(bytes).expect("csv utf8")
}

/// Write the sweep CSV to `path`.
pub fn write_w5_sweep_csv(cells: &[W5SweepCell], path: &Path) -> std::io::Result<()> {
    let mut file = std::fs::File::create(path)?;
    file.write_all(sweep_csv(cells).as_bytes())
}

/// Cells in the E3 pocket (`AnchorRescuePossible`), best `max_bracket_min` first — Phase 3 input.
pub fn anchor_rescue_pocket(cells: &[W5SweepCell]) -> Vec<&W5SweepCell> {
    let mut pocket: Vec<&W5SweepCell> = cells
        .iter()
        .filter(|c| c.regime == W5AnchorRescueRegime::AnchorRescuePossible)
        .collect();
    pocket.sort_by(|a, b| {
        b.eval
            .scores
            .max_bracket_min()
            .partial_cmp(&a.eval.scores.max_bracket_min())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    pocket
}
