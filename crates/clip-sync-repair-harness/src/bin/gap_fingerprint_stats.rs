//! `gap-fingerprint-stats` — cross-corpus gap-fingerprint analyzer (the P0 prevalence scan).
//! See `docs/TEMP-w5-timing-offset-rescue-plan.md` §5 P0 and `gap_fingerprint_corpus.rs`.
//!
//! Reads one or more `--gap-fingerprints` output dirs and tallies lag verdicts + gate outcomes across
//! every A/B pair — the numbers P0 needs (how many `timing_offset` gaps the gate skipped, constant vs
//! drift). Point it at the parent holding numbered corpora (`1/`..`N/`) or a list of specific dirs.
//!
//! Run (gated behind the `calibration` feature):
//! ```powershell
//! cargo run -p clip-sync-repair-harness --features calibration --bin gap-fingerprint-stats -- gap-files
//! #   optional env: GAP_FP_DRIFT_EPS_MS=1.0  GAP_FP_CSV=1  GAP_FP_GOLDEN=<path>
//! ```
//! Relative dirs resolve against the **repo root** (not the crate dir).

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clip_sync_repair_harness::gap_fingerprint_corpus::{
    analyze_dirs, drift_eps_from_env, tail_secs_from_env,
};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn resolve(dir: &str) -> PathBuf {
    let p = Path::new(dir.trim());
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        repo_root().join(p)
    }
}

fn main() -> ExitCode {
    // Positional args: one or more --gap-fingerprints output dirs (a parent of numbered corpora, or a
    // list of specific dirs). Also accepts a single comma-separated argument for back-compat with the
    // old GAP_FP_DIRS env form.
    let dirs: Vec<PathBuf> = std::env::args()
        .skip(1)
        .flat_map(|a| {
            a.split(',')
                .filter(|s| !s.trim().is_empty())
                .map(resolve)
                .collect::<Vec<_>>()
        })
        .collect();

    if dirs.is_empty() {
        eprintln!(
            "usage: gap-fingerprint-stats <dir>... \n\
             pass one or more --gap-fingerprints output dirs (e.g. `gap-files` auto-discovers \
             gap-files/1 .. gap-files/N).\n\
             optional env: GAP_FP_DRIFT_EPS_MS, GAP_FP_CSV=1, GAP_FP_GOLDEN=<path>"
        );
        return ExitCode::from(2);
    }

    let report = analyze_dirs(&dirs, drift_eps_from_env(), tail_secs_from_env());
    if report.total_gaps() == 0 {
        eprintln!("no gaps found under {dirs:?} — did the scans finish writing corpus.json?");
        return ExitCode::from(2);
    }

    print!("{}", report.legend_text());
    print!("{}", report.summary_text());
    print!("{}", report.gate_text());
    print!("{}", report.seam_probe_text());
    print!("{}", report.splice_text());
    print!("{}", report.occupancy_text());
    print!("{}", report.dualfit_viability_text());
    print!("{}", report.dualfit_scope_text());
    print!("{}", report.mechanism_text());
    print!("{}", report.trustworthy_text());

    if std::env::var("GAP_FP_CSV").as_deref() == Ok("1") {
        let path = repo_root().join("target").join("gap_fingerprint_corpus.csv");
        match std::fs::write(&path, report.csv()) {
            Ok(()) => eprintln!("wrote {}", path.display()),
            Err(e) => eprintln!("failed to write {}: {e}", path.display()),
        }
    }

    // Perf §4 golden baseline: `GAP_FP_GOLDEN=<path>` writes the decision-invariance snapshot.
    if let Ok(path) = std::env::var("GAP_FP_GOLDEN") {
        match std::fs::write(&path, report.golden_json()) {
            Ok(()) => eprintln!("wrote golden baseline to {path}"),
            Err(e) => eprintln!("failed to write golden baseline {path}: {e}"),
        }
    }

    ExitCode::SUCCESS
}
