# clip-sync developer docs

Build, test, corpus, and design plans. Operator guides and output contracts stay in [../](../) (parent `docs/`).

## Testing & development

| Doc | Covers |
|-----|--------|
| [test-tiers.md](test-tiers.md) | **How to run test tiers** — `test-tier.ps1`, composite profiles, prerequisites |
| [development.md](development.md) | Build, features, integration binary matrix, `#[ignore]` scheduling |
| [test-tier-remainder.md](test-tier-remainder.md) | Deferred tiers (2b, ignore follow-ups, nextest, validate crate) |
| [corpus-validation.md](corpus-validation.md) | Test corpus tiers, acceptance, energy-signature corpus |
| [corpus-matrix.md](corpus-matrix.md) | Alignment corpus matrix |
| [test-acceptance-glossary.md](test-acceptance-glossary.md) | SD/SP/EC/RG acceptance IDs |

## Performance

| Doc | Covers |
|-----|--------|
| [repair-perf.md](repair-perf.md) | **Where repair time goes** — current measured baseline, how to measure (`measure-repair-perf.ps1`), settled/refuted candidates, open candidates, media-hygiene rule |

## Domain / corpus tooling

| Doc | Covers |
|-----|--------|
| [gap-vocabulary.md](gap-vocabulary.md) | **Gap numbering** (0-based data / 1-based display), gap cells, silence-character pre-gate, fixture mapping |
| [gap-fingerprint.md](gap-fingerprint.md) | Gap fingerprint dump schema and corpus format; bulk dump via `measure-gap-fingerprints.ps1` |
| [shoulder-sweep.md](shoulder-sweep.md) | **`shoulder-sweep` bin contract** — the independent wide-window (±2 s) waveform lag sweep of both gap shoulders on the `--gap-listen` WAVs. Shares no correlation code with the `lag_decision` it checks; measures and records, does not classify |

## Plans

- **Active drafts:** `TEMP-*.md` in this folder. Currently: [TEMP-envelope-adaptive-widen-plan.md](TEMP-envelope-adaptive-widen-plan.md) (envelope window sizing + rescue-branch guard refinement; re-scoped to the rescue branch after nominal-first), [TEMP-patch-audio-region-module-split-plan.md](TEMP-patch-audio-region-module-split-plan.md) (folder split of `region.rs`; draft / not started), [TEMP-nway-donor-alignment-plan.md](TEMP-nway-donor-alignment-plan.md), [TEMP-flac-output-plan.md](TEMP-flac-output-plan.md), [TEMP-collapsed-bridge-census-2026-08-20.md](TEMP-collapsed-bridge-census-2026-08-20.md) (findings note, not a Change — full-corpus `bridge_ms` re-scan; **it refuted the predicate it set out to extend**: `collapsed_bridge` is an arithmetic coincidence over the real invariant, the step boundary, whose dump-visible base rate is zero. Re-aimed BACKLOG's skip-verdict item 2026-08-21. The step-boundary **rule** it applies is not in this note — it lives in [archive/TEMP-step-boundary-census-plan.md](archive/TEMP-step-boundary-census-plan.md) §4–§5, which is canonical; do not re-derive it, and see Question 12 for what happens when its sequence test is dropped), [TEMP-pair50-anchor-seam-outcomes-2026-08-21.md](TEMP-pair50-anchor-seam-outcomes-2026-08-21.md) (findings note, not a Change — split out of the census: pair 50's ~2.8 s map step is corrected in production by the anchor-seam bracket search, but one gap patches audibly wrong and three skip between correctly-resolved flanks).
- **One deliverable per plan.** Each active plan carries a verification rule: a `file:line` reference belongs only in a checklist item that is about to be executed; design sections state the decision and its reason. Adjacent debt: [BACKLOG.md](../../BACKLOG.md) § Gap-selection parked debt.
- **Archived plans & closed review ledgers:** [archive/](archive/) — historical design records only. Index: [archive/README.md](archive/README.md). Do not treat archive docs as current behavior; links inside them may be stale. Current equivalence-path behaviour is in [gap-fingerprint.md](gap-fingerprint.md) § *`equivalence_diagnostic` vs `equivalence_production`*.

> Architecture: [../../PLAN.md](../../PLAN.md). Open work: [../../BACKLOG.md](../../BACKLOG.md).
