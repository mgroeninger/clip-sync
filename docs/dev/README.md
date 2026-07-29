# clip-sync developer docs

Build, test, corpus, and design plans. Operator guides and output contracts stay in [../](../) (parent `docs/`).

## Testing & development

| Doc | Covers |
|-----|--------|
| [test-tiers.md](test-tiers.md) | **How to run test tiers** — `test-tier.ps1`, composite profiles, prerequisites |
| [development.md](development.md) | Build, features, integration binary matrix, `#[ignore]` scheduling |
| [archive/test-tier-plan.md](archive/test-tier-plan.md) | Tier migration history (archived 2026-06) |
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

- **Active drafts:** `TEMP-*.md` in this folder. Currently: [TEMP-gap-selection-plan.md](TEMP-gap-selection-plan.md) (v1 **implemented** — promote/archive per §11), [TEMP-gap-selection-sequencing-plan.md](TEMP-gap-selection-sequencing-plan.md) (meta — **archive**), [TEMP-scan-recipe-plan.md](TEMP-scan-recipe-plan.md) (parked until a consumer), [TEMP-gap-selection-ranges-plan.md](TEMP-gap-selection-ranges-plan.md) (v1.5), [TEMP-gap-selection-deferred.md](TEMP-gap-selection-deferred.md), [TEMP-nway-donor-alignment-plan.md](TEMP-nway-donor-alignment-plan.md).
- **One deliverable per plan.** The gap-selection plan was split four ways on 2026-07-29 after a single 1200-line draft produced seven recorded-then-retracted "bugs" — all of them stale claims about current source, none a design error. Order settled the same day ([TEMP-gap-selection-sequencing-plan.md](TEMP-gap-selection-sequencing-plan.md)): ship thin v1 before provenance/`ScanRecipe` (recipe-first rejected). Each active plan carries a verification rule: a `file:line` reference belongs only in a checklist item that is about to be executed; design sections state the decision and its reason. Parked adjacent debt: [BACKLOG.md](../../BACKLOG.md) § Gap-selection parked debt.
- **Review ledger:** [archive/TEMP-rust-review-findings.md](archive/TEMP-rust-review-findings.md) — prioritized correctness / silent-failure / perf findings (2026-07-23), **closed and archived 2026-07-27**. Kept for its rationale: what was fixed, what was *withdrawn* as refuted (M-FRAMES), and what is closed will-not-fix with re-open triggers (M-RESAMPLE).
- **Shipped:** [archive/](archive/) — historical design records (do not treat as current behavior; links may be stale). Includes closed M-MOD module-split plans (repair policies / analyzer policies / fingerprint / corpus / `patch_audio`), the [`bracket_fill` elimination plan](archive/TEMP-patch-audio-bracket-fill-elimination-plan.md), the [`RegionCharacterization` collapse plan](archive/TEMP-region-characterization-collapse-plan.md), and the [FFT repeat-band plan](archive/TEMP-repeat-band-plan.md) (lever 1b(b); numbers in [repair-perf.md](repair-perf.md) §1c / §3).

> Architecture: [../../PLAN.md](../../PLAN.md). Open work: [../../BACKLOG.md](../../BACKLOG.md).
