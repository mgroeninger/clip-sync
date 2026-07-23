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

## Domain / corpus tooling

| Doc | Covers |
|-----|--------|
| [gap-vocabulary.md](gap-vocabulary.md) | Gap cells, silence-character pre-gate, fixture mapping |
| [gap-fingerprint.md](gap-fingerprint.md) | Gap fingerprint dump schema and corpus format |

## Plans

- **Active drafts:** `TEMP-*.md` in this folder (e.g. [TEMP-gap-selection-plan.md](TEMP-gap-selection-plan.md), [TEMP-repair-config-bundles-plan.md](TEMP-repair-config-bundles-plan.md), [TEMP-policies-module-split-plan.md](TEMP-policies-module-split-plan.md)).
- **Review ledger:** [TEMP-rust-review-findings.md](TEMP-rust-review-findings.md) — prioritized correctness / silent-failure / perf findings (2026-07-23).
- **Shipped:** [archive/](archive/) — historical design records (do not treat as current behavior; links may be stale).

> Architecture: [../../PLAN.md](../../PLAN.md). Open work: [../../BACKLOG.md](../../BACKLOG.md).
