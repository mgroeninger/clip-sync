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

- **Active drafts:** `TEMP-*.md` in this folder. Currently: [TEMP-lag-envelope-recenter-plan.md](TEMP-lag-envelope-recenter-plan.md) (`lag_decision` envelope recenter; unblocked 2026-08-18), [TEMP-nway-donor-alignment-plan.md](TEMP-nway-donor-alignment-plan.md), [TEMP-flac-output-plan.md](TEMP-flac-output-plan.md).
- **Findings notes (not a Change):** [TEMP-31g7-model-vs-findings.md](TEMP-31g7-model-vs-findings.md) — placement model vs 31/g7 (and 26/g6) measurements; gap signature vs patch disagreement (§8; rev 2 landed 2026-08-18). **Parallel (resolved 2026-08-18, archived):** [archive/TEMP-31g7-gap-shape-vocabulary.md](archive/TEMP-31g7-gap-shape-vocabulary.md) §9 — the one-sided-closeability cell was refuted by the post-shoulder wide sweep: 31/g7 is a **step gap with a zero-length donor bridge** (both shoulders live at own lags, −47/−1160 ms; no donor fill possible; honest action = reasoned skip). 26/g6's choice-axis hole stays open — the two gaps were never one class. Durable statement: [gap-vocabulary.md](gap-vocabulary.md) § *Known holes*. Leftovers in [BACKLOG.md](../../BACKLOG.md) § *Fill placement*.
- **Recently archived:** [archive/TEMP-unmatchable-shoulder-plan.md](archive/TEMP-unmatchable-shoulder-plan.md) (S2b) — **refuted 2026-08-18, archived same day**: its premise ("the post shoulder is unmatchable") died under the ±2000 ms wide sweep (post uniquely at −1160 ms; 31/g7 is a step gap with a zero-length donor bridge, so "still patch at ≈ −47 ms" was the wrong action). · [archive/TEMP-31g7-gap-shape-vocabulary.md](archive/TEMP-31g7-gap-shape-vocabulary.md) — one-sided-closeability derivation + resolution (§9), **archived 2026-08-18**; durable statement in [gap-vocabulary.md](gap-vocabulary.md) § *Known holes*, work items in [BACKLOG.md](../../BACKLOG.md) § *Fill placement*. · [archive/TEMP-fill-placement-registration-plan.md](archive/TEMP-fill-placement-registration-plan.md) — fill placement vs local registration **investigation closed 2026-08-17**: two distinct defects (26/g6 source, 31/g7 `min`-reduction). Live delivery is now [TEMP-lag-envelope-recenter-plan.md](TEMP-lag-envelope-recenter-plan.md) (its other child, S2b, was refuted — first entry above); leftovers in [BACKLOG.md](../../BACKLOG.md) § *Fill placement*. · [archive/TEMP-fill-normalize-boost-only-plan.md](archive/TEMP-fill-normalize-boost-only-plan.md) — A-border fill normalize made **boost-only** (never attenuate) + `normalize_gain` emitted on the patch record, **archived 2026-08-12**; durable behaviour in [../pipeline.md](../pipeline.md) § *Fill normalization* and [../json-output.md](../json-output.md) § *FillLevelCheck*. Its follow-ups, and the **ear-confirmed defect on the boost side found while validating it**, are in [BACKLOG.md](../../BACKLOG.md) § *Fill normalize boosts against a non-local reference*. · [archive/TEMP-fingerprint-field-clarity-plan.md](archive/TEMP-fingerprint-field-clarity-plan.md) — rename five high-confusion wire fields (R1–R3) + co-locate `_contract` on eight metric groups (C1/C2), **archived 2026-08-08**; C3 deferred to [BACKLOG.md](../../BACKLOG.md) § *Fingerprint contract follow-ups (C3)*; durable behaviour in [gap-fingerprint.md](gap-fingerprint.md) § *`_contract`* and § *Shape*. · [archive/TEMP-residual-abstention-reporting-plan.md](archive/TEMP-residual-abstention-reporting-plan.md) — name *why* a residual carries no usable headroom reading, in production output and in dumps, **archived 2026-08-05**; durable behaviour in [../json-output.md](../json-output.md) § *SeamResidualVerdict* and [gap-fingerprint.md](gap-fingerprint.md) § *Gate recipe*. · [archive/TEMP-donor-apply-edge-exclusion-plan.md](archive/TEMP-donor-apply-edge-exclusion-plan.md) — head/tail Apply → nominal classify by A-span edge, **archived 2026-08-05**; durable behaviour in [gap-scan.md](../gap-scan.md) § Donor registration. · [archive/TEMP-gap-listen-wav-plan.md](archive/TEMP-gap-listen-wav-plan.md) — `--gap-listen [DIR]` WAV side channel on `--gap-fingerprints` (one decode), **archived 2026-08-04**; durable behaviour in [gap-fingerprint.md](gap-fingerprint.md) § *`--gap-listen`*. · [archive/TEMP-fingerprint-provenance-plan.md](archive/TEMP-fingerprint-provenance-plan.md) — fingerprint-dump source + measurement provenance, Tracks A and B shipped, **archived 2026-07-31**; durable behaviour in [gap-fingerprint.md](gap-fingerprint.md). · [archive/TEMP-gap-selection-deferred.md](archive/TEMP-gap-selection-deferred.md) — refused `--scan-window` (durable note in [gap-vocabulary.md](gap-vocabulary.md)) + `--gaps-from` sketch, **archived 2026-07-31**. · [archive/TEMP-scan-recipe-plan.md](archive/TEMP-scan-recipe-plan.md) — `ScanRecipe` + JSON scan-params echo, **archived 2026-07-31**.
- **One deliverable per plan.** Each active plan carries a verification rule: a `file:line` reference belongs only in a checklist item that is about to be executed; design sections state the decision and its reason. Adjacent debt: [BACKLOG.md](../../BACKLOG.md) § Gap-selection parked debt.
- **Archived plans & closed review ledgers:** [archive/](archive/) — historical design records only. Index: [archive/README.md](archive/README.md). Do not treat archive docs as current behavior; links inside them may be stale. Current equivalence-path behaviour is in [gap-fingerprint.md](gap-fingerprint.md) § *`equivalence_diagnostic` vs `equivalence_production`*.

> Architecture: [../../PLAN.md](../../PLAN.md). Open work: [../../BACKLOG.md](../../BACKLOG.md).
