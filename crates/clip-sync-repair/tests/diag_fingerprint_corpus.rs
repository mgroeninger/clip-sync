//! Cross-corpus gap-fingerprint analyzer (P0 prevalence scan).
//! See `docs/TEMP-w5-timing-offset-rescue-plan.md` §5 P0 and `gap_fingerprint_corpus.rs`.
//!
//! Tier: **diagnostic** (`diagnostic-tests`). Reads `--gap-fingerprints` output dirs and tallies lag
//! verdicts + gate outcomes across every A/B pair — the numbers P0 needs (how many `timing_offset`
//! gaps the gate skipped, constant vs drift).
//!
//! Run (point at the parent holding `1/`..`7/`, or a comma-separated list of dirs):
//! ```powershell
//! $env:GAP_FP_DIRS = "gap-files"            # auto-discovers gap-files/1 .. gap-files/6
//! # optional: $env:GAP_FP_DRIFT_EPS_MS = "1.0"; $env:GAP_FP_CSV = "1"
//! cargo test -p clip-sync-repair --features diagnostic-tests --test diag_fingerprint_corpus -- --nocapture
//! ```
//! Relative dirs resolve against the **repo root** (not the crate dir).

use std::path::{Path, PathBuf};

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

#[test]
fn diag_fingerprint_corpus() {
    let Ok(dirs_env) = std::env::var("GAP_FP_DIRS") else {
        eprintln!(
            "skip: set GAP_FP_DIRS to one or more --gap-fingerprints output dirs (comma-separated).\n\
             e.g. GAP_FP_DIRS=gap-files  (auto-discovers gap-files/1 .. gap-files/N)"
        );
        return;
    };
    let dirs: Vec<PathBuf> = dirs_env
        .split(',')
        .filter(|s| !s.trim().is_empty())
        .map(resolve)
        .collect();
    assert!(!dirs.is_empty(), "GAP_FP_DIRS resolved to no dirs");

    let report = analyze_dirs(&dirs, drift_eps_from_env(), tail_secs_from_env());
    assert!(
        report.total_gaps() > 0,
        "no gaps found under {dirs:?} — did the scans finish writing corpus.json?"
    );

    print!("{}", report.legend_text());
    print!("{}", report.summary_text());
    print!("{}", report.gate_text());
    print!("{}", report.seam_probe_text());
    print!("{}", report.splice_text());
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
}
