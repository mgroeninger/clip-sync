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

- **Active drafts:** `TEMP-*.md` in this folder. Currently: [TEMP-scan-recipe-plan.md](TEMP-scan-recipe-plan.md) (**unparked 2026-07-30**, ready to implement), [TEMP-equivalence-instrument-convergence.md](TEMP-equivalence-instrument-convergence.md), [TEMP-gap-selection-deferred.md](TEMP-gap-selection-deferred.md), [TEMP-nway-donor-alignment-plan.md](TEMP-nway-donor-alignment-plan.md).
- **One deliverable per plan.** The gap-selection plan was split four ways on 2026-07-29 after a single 1200-line draft produced seven recorded-then-retracted "bugs" — all of them stale claims about current source, none a design error. Order settled the same day ([archive/TEMP-gap-selection-sequencing-plan.md](archive/TEMP-gap-selection-sequencing-plan.md)): ship thin v1 before provenance/`ScanRecipe` (recipe-first rejected; sequencing archived after promote). Selection v1 + v1.5 ranges are archived: [archive/TEMP-gap-selection-plan.md](archive/TEMP-gap-selection-plan.md), [archive/TEMP-gap-selection-ranges-plan.md](archive/TEMP-gap-selection-ranges-plan.md). Recipe unparked 2026-07-30 for script same-recipe equality. Each active plan carries a verification rule: a `file:line` reference belongs only in a checklist item that is about to be executed; design sections state the decision and its reason. Adjacent debt: [BACKLOG.md](../../BACKLOG.md) § Gap-selection parked debt.
- **Open review ledger:** [TEMP-equivalence-instrument-convergence.md](TEMP-equivalence-instrument-convergence.md) — the three items left open when the F14/F15 ledger closed, all one axis: the two equivalence front-ends now share corrected sensor *definitions* but sample them with different **instruments**. **I1** bin size (fine 50 ms vs scan 100 ms) — the only remaining source of *action* divergence; recommendation is to converge it **narrowly**, giving the equivalence overlay its own bin size rather than changing `gap_signature_bin_ms` (shared with fill geometry). **I2** noise-floor context window (2.0 s vs 3.0 s, median 2.13 dB) — decide after I1, leaning accept-and-document. **I3** the donor predicate's missing `b.silent ||` disjunct — **unmeasured**, and the one bias pointing the *opposite* way; measure before fixing. Note `measure_gap_equivalence` has exactly one caller (the fingerprint dump), so none of this gates repair — the risk is lost **sensitivity** in `equivalence-calibration`'s CI gate, which fires only on the dangerous direction.
- **Review ledger:** [archive/TEMP-equivalence-divergence-findings.md](archive/TEMP-equivalence-divergence-findings.md) — scan-vs-fill equivalence divergence (**F14**/**F15**), **closed and archived 2026-07-30**. F14 border alignment fixed and media-validated. F15's three fine-path fixes (silent-core floor + A RMS, interleaved reduction, block-confirmed span) implemented and media-validated: median `|fine − scan|` now A RMS **0.101 dB**, floor **0.279**, donor **0.012**, noise floor **2.129** (was one-signed to −19). Population 5/297 divergent, 0 dangerous. Kept for its rationale: the probe-then-fix method, the Cauchy–Schwarz argument that *proved* the two paths read different sample sets, the reduction/window/span decomposition, and its retracted class prediction (*3 divergences → 1*) — refuted by the combined re-dump because it reasoned from a donor's *mean* where the classifier consumes a *per-bin fraction*.
- **Review ledger:** [archive/TEMP-silence-floor-findings.md](archive/TEMP-silence-floor-findings.md) — silence-floor / fillability findings (2026-07-29), **closed and archived 2026-07-30**. F1–F12 fixed or delegated (F11 → the recipe plan), R1–R4 closed, F14/F15 split out above. Kept for its rationale: §3's refuted/withdrawn hypotheses, two of which were *re-reversed* once measured on media; §6a's three-way fingerprint differential showing the fixes are safe on previously-fingerprinted media (and retiring one fabricated `drop`); §6c's refutation of the alignment-drift theory plus the failed-B-seek mechanism that probably produced the phantom offsets.
- **Review ledger:** [archive/TEMP-rust-review-findings.md](archive/TEMP-rust-review-findings.md) — prioritized correctness / silent-failure / perf findings (2026-07-23), **closed and archived 2026-07-27**. Kept for its rationale: what was fixed, what was *withdrawn* as refuted (M-FRAMES), and what is closed will-not-fix with re-open triggers (M-RESAMPLE).
- **Shipped:** [archive/](archive/) — historical design records (do not treat as current behavior; links may be stale). Includes closed M-MOD module-split plans (repair policies / analyzer policies / fingerprint / corpus / `patch_audio`), the [`bracket_fill` elimination plan](archive/TEMP-patch-audio-bracket-fill-elimination-plan.md), the [`RegionCharacterization` collapse plan](archive/TEMP-region-characterization-collapse-plan.md), and the [FFT repeat-band plan](archive/TEMP-repeat-band-plan.md) (lever 1b(b); numbers in [repair-perf.md](repair-perf.md) §1c / §3).

> Architecture: [../../PLAN.md](../../PLAN.md). Open work: [../../BACKLOG.md](../../BACKLOG.md).
