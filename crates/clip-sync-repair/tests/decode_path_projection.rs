//! **8g.1 — decode-path projection differential (always-on, no media).**
//!
//! Tier: **default** (synthetic A/B fixture; runs on every `cargo test`). The media-free counterpart of
//! `gap_repair_spec_diff` (which reads on-disk corpora): run the production oracle
//! (`characterize_gaps_with_gate`) on a synthetic A/B pair, then assert the projection
//! (`GapFingerprint → GapRepairTags → GapFingerprint`) preserves every `golden_baseline` decision axis.
//!
//! This is the harness the from-decode path (8g.3) extends: it will run the NEW characterize on the same
//! synthetic input and diff it against this OLD oracle. Shadow-first — the gate exists before the flip.

/// Diagnostics on ⇒ the corpus also carries the X-set, exercising the `b_levels` carry through the projection.
#[test]
fn decode_path_projection_preserves_golden_baseline() {
    let corpus = clip_sync_repair_fixtures::fingerprint_corpus_fixtures::synth_ab_corpus(true);
    assert!(!corpus.gaps.is_empty(), "synthetic fixture produced no gaps");
    let diffs = clip_sync_repair_harness::corpus_projection::projection_diff(&corpus);
    assert!(
        diffs.is_empty(),
        "projection changed {} decision-axis field(s) on the synthetic decode path:\n{}",
        diffs.len(),
        diffs.join("\n"),
    );
}
