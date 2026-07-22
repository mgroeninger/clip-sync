# Golden baselines — perf §4 decision-invariance harness

`curated.golden.json` is the reference snapshot future perf refactors are diffed against: per gap, the D/R
axis coordinates (prefixed by placement) + derived verdicts. Built by `baseline_from_rows` from the analyzer's
own predicate methods, so it can't drift from the decision logic. See
[docs/TEMP-pipeline-perf-redesign-plan.md](../../../docs/TEMP-pipeline-perf-redesign-plan.md) §4 and
[docs/TEMP-gap-fixture-corpus-plan.md](../../../../docs/TEMP-gap-fixture-corpus-plan.md).

## `curated.golden.json` — self-hosting

Snapshots the **committed per-gap-type fixtures**
(`crates/clip-sync-repair/tests/gap_corpus/fingerprints/curated/`) — one row per curated cell, keyed by cell
type (not `pair·gap`). Checked by `golden_baseline_invariance` (pr-repair, media-free); the per-type
classification footguns (silence-splice IS a target, program-quiet is NOT) live in `gap_cell_fixtures`.

Regenerate after an **intentional** analyzer change (it is reproduced *from* the committed fixtures — no
external media):

```powershell
$env:CURATED_GOLDEN_REGEN = "1"
cargo test -p clip-sync-repair --test golden_baseline_invariance
Remove-Item Env:\CURATED_GOLDEN_REGEN
```

## History

The prior reference, `re-anchor-dual-fit-on-nominal.golden.json` (62 matched gaps, 9 dual-fit targets), plus
its `assert_footguns` guards and the `golden_baseline_smoke` test, were retired in Phase 4 of the
gap-fixture-corpus plan: their source `gap-files/` corpus was ephemeral and unrecoverable (derived from
licensed media), so coverage moved onto the committed, media-independent curated fixtures. Earlier
`seam-local-fix.golden.json` was superseded before that.
