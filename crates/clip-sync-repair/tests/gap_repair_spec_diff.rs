//! **8f differential** — the fingerprint projection preserves every decision axis on real oracle output.
//!
//! Tier: **validation** (needs `gap-files/re-anchor-dual-fit-on-nominal` — 69 real gaps, no media).
//!
//! ```powershell
//! cargo test -p clip-sync-repair --test gap_repair_spec_diff -- --ignored
//! ```
//!
//! In-process differential (the 6b shadow pattern, adapted to media-free validation): the frozen corpus is
//! **old-oracle output**. For each gap we extract the D/R payload (`tags_from_fingerprint`), re-project it
//! (`spec_to_fingerprint_summary`), and assert the corpus reader's `golden_baseline` (Tier-1 bit-exact, Tier-2
//! within ε) is unchanged. This proves `GapRepairTags` is a complete decision carrier and the projection is
//! faithful — the behavior-preservation gate for flipping the dump path to the shared characterize (8g).
//!
//! Runs on the in-repo corpus with **zero media** by default. To broaden coverage to real gap types, produce a
//! fingerprint run from any licensed A/B pair and point the test at that **same** output directory:
//!
//! ```powershell
//! clip-sync-repair <A> <B> ... --gap-fingerprints out_dir   # writes out_dir/corpus.json
//! $env:GAP_FP_DIRS = "out_dir"                               # read the same dir back
//! cargo test -p clip-sync-repair --features validation-tests --test gap_repair_spec_diff -- --ignored
//! ```
//!
//! `--gap-fingerprints` (write) and `GAP_FP_DIRS` (read) are the **same** directory at two stages — not extra
//! dirs. `GAP_FP_DIRS` accepts a comma-separated list only because the analyzer can take several corpus roots
//! at once (that is how the in-repo default reads its 7 `re-anchor` pair dirs in one call).

use std::path::{Path, PathBuf};

use clip_sync_repair::application::gap_fingerprint::GapCorpus;
use clip_sync_repair_harness::corpus_projection::project_corpus;
use clip_sync_repair_harness::gap_fingerprint_corpus::{
    analyze_dirs, drift_eps_from_env, tail_secs_from_env,
};
use clip_sync_repair_harness::golden_baseline::{diff_baselines, TIER2_ABS_EPS};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn default_corpus_dir() -> PathBuf {
    repo_root().join("gap-files").join("re-anchor-dual-fit-on-nominal")
}

/// Parse a comma-separated env var of corpus dirs (relative entries resolve against the repo root).
fn env_dirs(var: &str) -> Option<Vec<PathBuf>> {
    let env = std::env::var(var).ok()?;
    let dirs: Vec<PathBuf> = env
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            let p = Path::new(s);
            if p.is_absolute() { p.to_path_buf() } else { repo_root().join(p) }
        })
        .collect();
    (!dirs.is_empty()).then_some(dirs)
}

/// Corpus roots to check: `GAP_FP_DIRS` (comma-separated; the same dirs a `--gap-fingerprints` run wrote), else
/// the in-repo default. `None` ⇒ nothing to check (skip).
fn corpus_roots() -> Option<Vec<PathBuf>> {
    if let Some(dirs) = env_dirs("GAP_FP_DIRS") {
        return Some(dirs);
    }
    let d = default_corpus_dir();
    d.is_dir().then(|| vec![d])
}

/// **8g.4a media gate — the from-decode-vs-oracle differential on licensed media** (the flip gate). Point
/// `VALIDATE_FP_DIRS` at the `--gap-fingerprints DIR --validate-from-decode` output dir(s) — each holds
/// `oracle/` + `from_decode/` produced from the SAME decode — and assert their `golden_baseline` matches. The
/// two corpora are **aligned + verified by source-id** (the FNV digest of the decoded audio already in each
/// `corpus.json`), so a `source-id` mismatch is a hard error ("different media"), and folder names don't
/// matter. Run once lean, once with `--fingerprint-diagnostics`.
///
/// ```powershell
/// clip-sync-repair <A> <B> ... --gap-fingerprints val\p1 --validate-from-decode
/// $env:VALIDATE_FP_DIRS = "val\p1"   # comma-separate several
/// cargo test -p clip-sync-repair --features validation-tests --test gap_repair_spec_diff from_decode_vs_oracle -- --ignored --nocapture
/// ```
#[test]
#[ignore = "tier:validation — set VALIDATE_FP_DIRS to --validate-from-decode output dir(s)"]
fn from_decode_vs_oracle() {
    use clip_sync_repair_harness::corpus_projection::golden_baseline_of_corpus;

    let Some(dirs) = env_dirs("VALIDATE_FP_DIRS") else {
        eprintln!("skip: set VALIDATE_FP_DIRS to `--validate-from-decode` output dir(s) (each holds oracle/ + from_decode/)");
        return;
    };

    let read = |dir: &Path, sub: &str| -> GapCorpus {
        let cj = dir.join(sub).join("corpus.json");
        serde_json::from_str(&std::fs::read_to_string(&cj).unwrap_or_else(|e| panic!("read {}: {e}", cj.display())))
            .unwrap_or_else(|e| panic!("parse {}: {e}", cj.display()))
    };

    let mut total = 0usize;
    let mut all_diffs: Vec<String> = Vec::new();
    for dir in &dirs {
        let oracle = read(dir, "oracle");
        let from_decode = read(dir, "from_decode");
        // Same-media check (the "manifest hash verify"): the source-id is the digest of the decoded audio.
        assert_eq!(
            (&oracle.source.a_source.id, &oracle.source.b_source.id),
            (&from_decode.source.a_source.id, &from_decode.source.b_source.id),
            "{}: oracle vs from_decode source-id mismatch — different media?",
            dir.display(),
        );
        total += oracle.gaps.len();
        // Both re-key to the same one-pair label inside `golden_baseline_of_corpus`, so they align by index.
        let d = diff_baselines(
            &golden_baseline_of_corpus(&oracle),
            &golden_baseline_of_corpus(&from_decode),
            TIER2_ABS_EPS,
        );
        all_diffs.extend(d.into_iter().map(|m| format!("{}: {m}", dir.display())));
    }
    eprintln!("from-decode vs oracle: {} dir(s), {total} gap(s) checked", dirs.len());
    assert!(
        all_diffs.is_empty(),
        "from-decode diverged from the oracle on {} decision-axis field(s):\n{}",
        all_diffs.len(),
        all_diffs.iter().take(30).cloned().collect::<Vec<_>>().join("\n"),
    );
}

/// Re-project one pair dir's `corpus.json` into `dst/corpus.json`.
fn mirror_pair(src: &Path, dst: &Path) {
    let cj = src.join("corpus.json");
    let orig: GapCorpus = serde_json::from_str(&std::fs::read_to_string(&cj).unwrap())
        .unwrap_or_else(|e| panic!("parse {}: {e}", cj.display()));
    std::fs::create_dir_all(dst).unwrap();
    std::fs::write(dst.join("corpus.json"), serde_json::to_string(&project_corpus(&orig)).unwrap()).unwrap();
}

/// Mirror `root`'s projected corpus under `out_root`, preserving the pair layout so `analyze_dirs` derives the
/// **same** `pair` labels (it strips against each root's parent, and `out_root` keeps `root`'s basename).
/// Handles both a single pair dir (`corpus.json` at `root` — a `--gap-fingerprints DIR` run) and a parent of
/// pair subdirs (the 7 in-repo `re-anchor` dirs). Returns the pair count.
fn mirror_projected(root: &Path, out_root: &Path) -> usize {
    let mut pairs = 0;
    if root.join("corpus.json").is_file() {
        mirror_pair(root, out_root);
        pairs += 1;
    }
    if let Ok(rd) = std::fs::read_dir(root) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() && p.join("corpus.json").is_file() {
                mirror_pair(&p, &out_root.join(p.file_name().unwrap()));
                pairs += 1;
            }
        }
    }
    pairs
}

#[test]
#[ignore = "tier:validation — needs gap-files/re-anchor-dual-fit-on-nominal (or GAP_FP_DIRS)"]
fn gap_repair_spec_projection_preserves_golden_baseline() {
    let Some(roots) = corpus_roots() else {
        eprintln!("skip: no corpus (set GAP_FP_DIRS or place fingerprints under gap-files/)");
        return;
    };

    // OLD baseline: analyze the real oracle corpus.
    let using_default = std::env::var("GAP_FP_DIRS").ok().filter(|s| !s.trim().is_empty()).is_none();
    let old = analyze_dirs(&roots, drift_eps_from_env(), tail_secs_from_env()).golden_baseline();
    // Echo what was actually checked (run with `-- --nocapture`) — the source of the earlier
    // "did it use my media?" confusion: on success the test was silent.
    eprintln!(
        "differential: {} gap(s) across {} root(s) [{}]: {:?}",
        old.gap_count,
        roots.len(),
        if using_default { "in-repo default — set GAP_FP_DIRS for your media" } else { "GAP_FP_DIRS" },
        roots,
    );
    assert!(old.gap_count > 0, "no gaps under {roots:?}");

    // NEW baseline: re-project every gap into a layout-matching mirror (same root basenames ⇒ same labels).
    let tmp = tempfile::tempdir().unwrap();
    let mut new_roots = Vec::new();
    for (i, root) in roots.iter().enumerate() {
        // Basename disambiguated by index so two roots sharing a basename don't collide in the mirror.
        let base = root.file_name().map(|n| n.to_owned()).unwrap_or_else(|| format!("root{i}").into());
        let out_root = tmp.path().join(&base);
        assert!(mirror_projected(root, &out_root) > 0, "no pair dirs under {}", root.display());
        new_roots.push(out_root);
    }
    let new = analyze_dirs(&new_roots, drift_eps_from_env(), tail_secs_from_env()).golden_baseline();

    let diffs = diff_baselines(&old, &new, TIER2_ABS_EPS);
    assert!(
        diffs.is_empty(),
        "projection changed {} decision-axis field(s) across {} gaps:\n{}",
        diffs.len(),
        old.gap_count,
        diffs.iter().take(30).cloned().collect::<Vec<_>>().join("\n"),
    );
}
