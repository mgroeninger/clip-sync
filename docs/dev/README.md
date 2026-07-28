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
| [gap-vocabulary.md](gap-vocabulary.md) | Gap cells, silence-character pre-gate, fixture mapping |
| [gap-fingerprint.md](gap-fingerprint.md) | Gap fingerprint dump schema and corpus format; bulk dump via `measure-gap-fingerprints.ps1` |

## Plans

- **Active drafts:** `TEMP-*.md` in this folder (e.g. [TEMP-gap-selection-plan.md](TEMP-gap-selection-plan.md)).
- **Review ledger:** [archive/TEMP-rust-review-findings.md](archive/TEMP-rust-review-findings.md) — prioritized correctness / silent-failure / perf findings (2026-07-23), **closed and archived 2026-07-27**. Kept for its rationale: what was fixed, what was *withdrawn* as refuted (M-FRAMES), and what is closed will-not-fix with re-open triggers (M-RESAMPLE).
- **Shipped:** [archive/](archive/) — historical design records (do not treat as current behavior; links may be stale). Includes closed M-MOD module-split plans (repair policies / analyzer policies / fingerprint / corpus / `patch_audio`), the [`bracket_fill` elimination plan](archive/TEMP-patch-audio-bracket-fill-elimination-plan.md), the [`RegionCharacterization` collapse plan](archive/TEMP-region-characterization-collapse-plan.md), and the [FFT repeat-band plan](archive/TEMP-repeat-band-plan.md) (lever 1b(b); numbers in [repair-perf.md](repair-perf.md) §1c / §3).

> Architecture: [../../PLAN.md](../../PLAN.md). Open work: [../../BACKLOG.md](../../BACKLOG.md).
