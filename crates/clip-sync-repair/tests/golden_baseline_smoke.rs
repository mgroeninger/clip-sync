//! Perf §4 decision-invariance harness — footgun assertions on the frozen golden baseline.
//!
//! Tier: **pr-repair** (no corpus / media required — parses the checked-in golden JSON only).
//! Split out of `golden_baseline_invariance.rs` (§4.7 A1) so this cheap smoke check runs in CI
//! without pulling in the `validation-tests`-gated, corpus-dependent round-trip test.

use clip_sync_repair_harness::golden_baseline::{assert_footguns, parse_golden_baseline};

const GOLDEN_JSON: &str = include_str!(
    "../../clip-sync-repair-harness/golden/re-anchor-dual-fit-on-nominal.golden.json"
);

/// §4.4 footguns on the checked-in golden — always runs (no corpus required).
#[test]
fn golden_baseline_footguns() {
    let baseline = parse_golden_baseline(GOLDEN_JSON).expect("parse frozen golden JSON");
    assert_footguns(&baseline).expect("§4.4 footgun assertions on frozen baseline");
    assert_eq!(baseline.gap_count, 62, "frozen baseline gap_count");
    assert_eq!(baseline.dualfit_targets.len(), 9, "frozen dual-fit target count");
}
