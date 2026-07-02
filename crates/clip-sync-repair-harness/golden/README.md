# Golden baselines — perf §4 decision-invariance harness

`<corpus>.golden.json` is the reference snapshot future perf refactors are diffed
against: per matched gap, the D/R axis coordinates (prefixed by placement) + derived
verdicts. Emitted by `CorpusReport::golden_json()` (reuses the analyzer's own
predicate methods, so it can't drift from the decision logic). See
[docs/TEMP-pipeline-perf-redesign-plan.md](../../../docs/TEMP-pipeline-perf-redesign-plan.md) §4.

## Status: `seam-local-fix.golden.json` is **PROVISIONAL — not frozen**

Captured from the **±100 ms gross-anchored** `splice_dualfit` (7 targets: 1·g3, 1·g5,
1·g22, 2·g1, 2·g2, 5·g6, 7·g2). **Superseded** by the nominal-reanchor fix (`2622c7a`),
which will change `splice_dualfit` corpus-wide and add `7·g3`/`7·g4` (operator-confirmed
real drops). **Do not treat as the reference — regenerate from the re-anchor rescan.**

## Freeze criteria (§4.0) — all must hold before this is the reference

1. Re-anchor rescan completes and validates (`7·g3`/`7·g4` flip to targets; no new
   false-negatives).
2. **P2 orthogonality gate** passes (axes independent / populated / non-redundant).
3. **`b_levels`-vs-elimination cross-check** is clean (no eliminated gap where B has
   content, beyond the explained donor-BROKEN cases).

## Regenerate

```
GAP_FP_DIRS=gap-files/<corpus> \
GAP_FP_GOLDEN=crates/clip-sync-repair-harness/golden/<corpus>.golden.json \
  cargo test -p clip-sync-repair --features diagnostic-tests --test diag_fingerprint_corpus -- --nocapture
```
