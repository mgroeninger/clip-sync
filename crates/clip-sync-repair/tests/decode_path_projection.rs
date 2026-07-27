//! **C5 — live from-decode dump self-guard (always-on, no media).**
//!
//! Tier: **default** (synthetic A/B fixture; runs on every `cargo test`). Runs the live `--gap-fingerprints`
//! dump pipeline (`characterize_gaps_from_decode`, via the `synth_ab_from_decode_corpus` fixture) and asserts
//! its output is well-formed and survives a projection round-trip (`GapFingerprint → GapRepairTags →
//! GapFingerprint`) with every `golden_baseline` decision axis preserved — in both lean and diagnostics modes.
//!
//! History: this replaced the transitional old-vs-new decode differentials (`characterize_gaps_with_gate` vs
//! from-decode), which were **migration scaffolding**. The migration is complete and the oracle was deleted at
//! 8g.6, so we no longer check "new matches old" — only that the from-decode dump stays internally consistent.

/// Both modes: the live from-decode dump produces gaps, and each gap's decision axes survive the spec
/// projection round-trip unchanged (diagnostics-on additionally exercises the `b_levels` X-set carry).
#[test]
fn from_decode_dump_projection_preserves_golden_baseline() {
    for diagnostics in [false, true] {
        let corpus =
            clip_sync_repair_fixtures::fingerprint_corpus_fixtures::synth_ab_from_decode_corpus(
                diagnostics,
            );
        assert!(
            !corpus.gaps.is_empty(),
            "synthetic from-decode dump produced no gaps (diagnostics={diagnostics})"
        );
        let diffs = clip_sync_repair_harness::corpus_projection::projection_diff(&corpus);
        assert!(
            diffs.is_empty(),
            "projection changed {} decision-axis field(s) on the from-decode dump (diagnostics={diagnostics}):\n{}",
            diffs.len(),
            diffs.join("\n"),
        );
    }
}
