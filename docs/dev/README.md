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

## Plans

- **Active drafts:** `TEMP-*.md` in this folder. Currently: [TEMP-nway-donor-alignment-plan.md](TEMP-nway-donor-alignment-plan.md), [TEMP-flac-output-plan.md](TEMP-flac-output-plan.md), [TEMP-gap-listen-wav-plan.md](TEMP-gap-listen-wav-plan.md).
- **Recently archived:** [archive/TEMP-fingerprint-provenance-plan.md](archive/TEMP-fingerprint-provenance-plan.md) — fingerprint-dump source + measurement provenance, Tracks A and B shipped, **archived 2026-07-31**; durable behaviour in [gap-fingerprint.md](gap-fingerprint.md). · [archive/TEMP-gap-selection-deferred.md](archive/TEMP-gap-selection-deferred.md) — refused `--scan-window` (durable note in [gap-vocabulary.md](gap-vocabulary.md)) + `--gaps-from` sketch, **archived 2026-07-31**. · [archive/TEMP-scan-recipe-plan.md](archive/TEMP-scan-recipe-plan.md) — `ScanRecipe` + JSON scan-params echo, **archived 2026-07-31**.
- **One deliverable per plan.** Each active plan carries a verification rule: a `file:line` reference belongs only in a checklist item that is about to be executed; design sections state the decision and its reason. Adjacent debt: [BACKLOG.md](../../BACKLOG.md) § Gap-selection parked debt.
- **Archived plans & closed review ledgers:** [archive/](archive/) — historical design records only. Index: [archive/README.md](archive/README.md). Do not treat archive docs as current behavior; links inside them may be stale. Current equivalence-path behaviour is in [gap-fingerprint.md](gap-fingerprint.md) § *`equivalence` vs `scan_equivalence`*.

> Architecture: [../../PLAN.md](../../PLAN.md). Open work: [../../BACKLOG.md](../../BACKLOG.md).
