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

/// **8g.4a — the REAL old-vs-new decode differential (CI, no media).** Run the **oracle**
/// (`characterize_gaps_with_gate`, today's inline build) and the **from-decode** pipeline
/// (`characterize_gaps_from_decode` = summary + `compute_region_measurements` + `tags_from_measurements` +
/// projection) on the **same** synthetic A/B PCM, and assert their `golden_baseline` is identical — in **both**
/// lean and diagnostics-on modes. This is the empirical proof the 8g.4b flip is behavior-preserving; it covers
/// `tier`/`levels`/`geometry`, not just tags (superseding 8g.3b's by-construction argument). The licensed-media
/// analogue (both modes) is the gate for the actual flip (8g.4b).
#[test]
fn from_decode_pipeline_matches_oracle_golden_baseline() {
    use clip_sync_repair_fixtures::fingerprint_corpus_fixtures::{synth_ab_corpus, synth_ab_from_decode_corpus};
    use clip_sync_repair_harness::corpus_projection::golden_baseline_of_corpus;
    use clip_sync_repair_harness::golden_baseline::{diff_baselines, TIER2_ABS_EPS};

    for diagnostics in [false, true] {
        let oracle = synth_ab_corpus(diagnostics);
        let from_decode = synth_ab_from_decode_corpus(diagnostics);
        assert_eq!(oracle.gaps.len(), from_decode.gaps.len(), "gap count (diagnostics={diagnostics})");
        let base_oracle = golden_baseline_of_corpus(&oracle);
        let base_from_decode = golden_baseline_of_corpus(&from_decode);
        let diffs = diff_baselines(&base_oracle, &base_from_decode, TIER2_ABS_EPS);
        assert!(
            diffs.is_empty(),
            "from-decode diverged from the oracle on {} decision axis field(s) (diagnostics={diagnostics}):\n{}",
            diffs.len(),
            diffs.join("\n"),
        );
    }
}
