//! Perf §4 decision-invariance harness — compare live corpus analysis against the frozen golden baseline.
//!
//! Tier: **validation** (needs `gap-files/re-anchor-dual-fit-on-nominal` on disk).
//!
//! ```powershell
//! .\scripts\test-tier.ps1 -Tier validation
//! # or ad hoc:
//! cargo test -p clip-sync-repair --features validation-tests --test golden_baseline_invariance
//! ```

use std::path::{Path, PathBuf};

use clip_sync_repair_harness::gap_fingerprint_corpus::{
    analyze_dirs, drift_eps_from_env, tail_secs_from_env,
};
use clip_sync_repair_harness::golden_baseline::{
    assert_footguns, diff_baselines, parse_golden_baseline, TIER2_ABS_EPS,
};

const GOLDEN_JSON: &str = include_str!(
    "../../clip-sync-repair-harness/golden/re-anchor-dual-fit-on-nominal.golden.json"
);

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn default_corpus_dir() -> PathBuf {
    repo_root().join("gap-files").join("re-anchor-dual-fit-on-nominal")
}

fn corpus_dirs() -> Option<Vec<PathBuf>> {
    if let Ok(dirs_env) = std::env::var("GAP_FP_DIRS") {
        let dirs: Vec<PathBuf> = dirs_env
            .split(',')
            .filter(|s| !s.trim().is_empty())
            .map(|s| {
                let p = Path::new(s.trim());
                if p.is_absolute() {
                    p.to_path_buf()
                } else {
                    repo_root().join(p)
                }
            })
            .collect();
        if !dirs.is_empty() {
            return Some(dirs);
        }
    }
    let default = default_corpus_dir();
    if default.is_dir() {
        return Some(vec![default]);
    }
    None
}

/// §4.4 footguns on the checked-in golden — always runs (no corpus required).
#[test]
fn golden_baseline_footguns() {
    let baseline = parse_golden_baseline(GOLDEN_JSON).expect("parse frozen golden JSON");
    assert_footguns(&baseline).expect("§4.4 footgun assertions on frozen baseline");
    assert_eq!(baseline.gap_count, 62, "frozen baseline gap_count");
    assert_eq!(baseline.dualfit_targets.len(), 9, "frozen dual-fit target count");
}

/// Round-trip: re-analyze corpus.json dirs and diff against the frozen golden snapshot.
#[test]
#[ignore = "tier:validation — needs gap-files/re-anchor-dual-fit-on-nominal; test-tier.ps1 -Tier validation"]
fn golden_baseline_corpus_invariance() {
    let Some(dirs) = corpus_dirs() else {
        eprintln!(
            "skip: corpus not found — set GAP_FP_DIRS or place fingerprints at {}",
            default_corpus_dir().display()
        );
        return;
    };

    let expected = parse_golden_baseline(GOLDEN_JSON).expect("parse frozen golden JSON");
    let report = analyze_dirs(&dirs, drift_eps_from_env(), tail_secs_from_env());
    assert!(
        report.total_gaps() > 0,
        "no gaps under {dirs:?} — did the fingerprint scan finish?"
    );

    let actual = report.golden_baseline();
    let diffs = diff_baselines(&expected, &actual, TIER2_ABS_EPS);
    if !diffs.is_empty() {
        let preview: Vec<_> = diffs.iter().take(20).cloned().collect();
        let more = diffs.len().saturating_sub(20);
        panic!(
            "golden baseline drift ({} mismatch{}):\n{}\n{}",
            diffs.len(),
            if diffs.len() == 1 { "" } else { "es" },
            preview.join("\n"),
            if more > 0 {
                format!("… and {more} more")
            } else {
                String::new()
            }
        );
    }
}
