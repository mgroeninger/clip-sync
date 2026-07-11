//! Shared projection-differential helpers (Fingerprint-unification 8f/8g).
//!
//! Given an oracle-produced [`GapCorpus`], re-project every gap through the D/R tags and check the corpus
//! reader's decision axes (`golden_baseline`) are unchanged. Used by the corpus differential
//! (`gap_repair_spec_diff`, from disk) and by the in-crate decode-path driver (8g.1, from a freshly
//! characterized corpus) so both read specs through identical code.

use clip_sync_repair::application::gap_fingerprint::{
    fingerprint_to_spec, spec_to_fingerprint_summary, FingerprintXSet, GapCorpus,
};

use crate::gap_fingerprint_corpus::analyze_dirs;
use crate::golden_baseline::{diff_baselines, GoldenBaseline, TIER2_ABS_EPS};

/// Re-project every gap: old `GapFingerprint` → `GapRepairTags` → new `GapFingerprint`. `b_levels` is carried
/// through the X-set (the corpus reader's `b_*_floor_db` read it; it is the only diagnostic field
/// `golden_baseline` captures) so a diagnostics-on corpus round-trips it.
pub fn project_corpus(orig: &GapCorpus) -> GapCorpus {
    let gaps = orig
        .gaps
        .iter()
        .map(|fp| {
            let spec = fingerprint_to_spec(fp);
            let x = FingerprintXSet { b_levels: fp.b_levels.clone(), ..Default::default() };
            // `None` real_brackets: the spec (from stored tags) carries only bracket counts, not per-bracket
            // rows, so synthesize. `golden_baseline` reads counts, not rows, so this is faithful for the diff.
            spec_to_fingerprint_summary(&spec, fp.sample_rate, fp.channels, Some(x), None)
        })
        .collect();
    GapCorpus { source: orig.source.clone(), gaps }
}

/// `golden_baseline` for a single in-memory corpus — write it to a one-pair temp tree and analyze it through
/// the frozen reader (same path the on-disk corpus differential uses). `drift_eps`/`tail_secs` are the
/// analyzer defaults; they do not affect the decision axes this compares.
pub fn golden_baseline_of_corpus(corpus: &GapCorpus) -> GoldenBaseline {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path().join("pair");
    std::fs::create_dir_all(&dir).expect("mkdir");
    std::fs::write(dir.join("corpus.json"), serde_json::to_string(corpus).expect("serialize")).expect("write");
    analyze_dirs(&[dir], 1.0, 30.0).golden_baseline()
}

/// The 8f/8g decision-invariance check for one corpus: `golden_baseline(orig)` vs
/// `golden_baseline(project(orig))`. Empty ⇒ the projection preserved every Tier-1/2 axis. Both sides use the
/// same one-pair temp label, so records align.
pub fn projection_diff(corpus: &GapCorpus) -> Vec<String> {
    let old = golden_baseline_of_corpus(corpus);
    let new = golden_baseline_of_corpus(&project_corpus(corpus));
    diff_baselines(&old, &new, TIER2_ABS_EPS)
}
