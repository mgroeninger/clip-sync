# Golden baselines — perf §4 decision-invariance harness

`<corpus>.golden.json` is the reference snapshot future perf refactors are diffed
against: per matched gap, the D/R axis coordinates (prefixed by placement) + derived
verdicts. Emitted by `CorpusReport::golden_json()` (reuses the analyzer's own
predicate methods, so it can't drift from the decision logic). See
[docs/TEMP-pipeline-perf-redesign-plan.md](../../../docs/TEMP-pipeline-perf-redesign-plan.md) §4.

## Current reference: `re-anchor-dual-fit-on-nominal.golden.json` — **FROZEN**

Captured on the **nominal-reanchored** `splice_dualfit` (commit `2622c7a`) + the
corrected `step_is_real` predicate (`b099b83`). 62 matched gaps; **9 dual-fit
targets**: 1·g3, 1·g5, 1·g22, 2·g1, 2·g2, 5·g6, 7·g2, 7·g3, 7·g4.

All §4.0 freeze criteria met (2026-07-02):
1. ✅ Re-anchor rescan validates — `7·g3`/`7·g4` (operator-confirmed drops) flip to
   targets; no new false-negatives.
2. ✅ **P2 orthogonality gate** — cells clean/interpretable. Two axes degenerate *on
   this corpus*: `gate_pass` (31/32 pass — ±600 ms search is over-permissive) and
   `donor-aligned ≡ donor-nominal` (agree 32/32; kept as the D8 safety net). The
   discriminating axes are donor-occupancy ∧ step-real. Noted, not blocking.
3. ✅ **`b_levels`-vs-elimination cross-check** clean — every eliminated "B-loud" gap
   is donor-BROKEN (genuine multi-second interior silence) or a start-of-file `g0`.

**Caveat (D8):** `gate_pass` being degenerate means the target set rests on the donor +
step-real filters, not seam viability. Fine for same-master; a decoy/different-content
regime would need a real alias gate (`seam_z`/wide-env). Parked at D8.

## Regenerate

```
GAP_FP_DIRS=gap-files/<corpus> \
GAP_FP_GOLDEN=crates/clip-sync-repair-harness/golden/<corpus>.golden.json \
  cargo test -p clip-sync-repair --features diagnostic-tests --test diag_fingerprint_corpus -- --nocapture
```

For analyzer depth (`seam_probe`, `wide_envelope`, diagnostic `lag`, `b_levels`), pass
`--fingerprint-diagnostics` when running `clip-sync-repair --gap-fingerprints`. Default fingerprint
dumps emit decision/repair (D/R) fields only (D12 §3 step 2).

Prior `seam-local-fix.golden.json` (±100 ms gross-anchored, 7 targets) was superseded
and removed.
