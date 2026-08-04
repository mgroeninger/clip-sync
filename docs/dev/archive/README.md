# Archived developer plans & ledgers

Historical design records for `docs/dev/`. **Not current behavior** — prefer live docs in
[../](../) (`gap-fingerprint.md`, `gap-vocabulary.md`, `repair-perf.md`, `test-tiers.md`, etc.)
and the code. Links and `file:line` claims inside archive files may be stale.

Active drafts stay in [../](../) (`TEMP-*.md`); this folder is for shipped / closed / superseded
work only.

## Closed review ledgers (keep for rationale)

| Doc | Closed | What it recorded |
|-----|--------|------------------|
| [TEMP-gap-listen-wav-plan.md](TEMP-gap-listen-wav-plan.md) | 2026-08-04 | `--gap-listen [DIR]` WAV side channel on `--gap-fingerprints` (one decode → JSON + A/B surround + production-patched WAV). Kept for rationale: plan geometry, no `PendingAfterScan` arm, `--fingerprint-gap` as sole selector, §12.2 dump-oracle cost on gate-refused gaps. Current behaviour: [../gap-fingerprint.md](../gap-fingerprint.md) § *`--gap-listen`*. |
| [TEMP-fingerprint-provenance-plan.md](TEMP-fingerprint-provenance-plan.md) | 2026-07-31 | Fingerprint-dump provenance: Track A (`FileSource` codec / `bit_depth` / native rate + channels / bitrate, `was_resampled()`, codec census) and Track B (`measurement` recipe, `a_gap_total_blocks`, `silent_core_probes` hard-delete) — **both shipped**. Kept for the declines: `is_lossy()`, `container`, the `profile_db` envelope, span arg-max, probe soft-retire. Closing finding: **no lossless pair exists or is obtainable**, so §1.1's corpus null is retired as evidence and the −120 condition is covered by fixture + unit test. Current behaviour: [../gap-fingerprint.md](../gap-fingerprint.md) § *Source identity & the corpus library* and § *`measurement`*. |
| [TEMP-equivalence-instrument-convergence.md](TEMP-equivalence-instrument-convergence.md) | 2026-07-31 | I1–I3: scan vs fine equivalence **instruments** after F14/F15. Current table lives in [../gap-fingerprint.md](../gap-fingerprint.md) § *`equivalence` vs `scan_equivalence`*. |
| [TEMP-equivalence-divergence-findings.md](TEMP-equivalence-divergence-findings.md) | 2026-07-30 | F14/F15 scan-vs-fill divergence; probe-then-fix method; reduction/window/span decomposition |
| [TEMP-silence-floor-findings.md](TEMP-silence-floor-findings.md) | 2026-07-30 | F1–F12 silence-floor / fillability; F11 closed by [TEMP-scan-recipe-plan.md](TEMP-scan-recipe-plan.md) |
| [TEMP-rust-review-findings.md](TEMP-rust-review-findings.md) | 2026-07-27 | Correctness / silent-failure / perf findings; withdrawn M-FRAMES; will-not-fix M-RESAMPLE |

## Gap-selection family

Shipped thin selection v1 + v1.5 ranges first; recipe stayed parked until a same-recipe consumer
(unparked 2026-07-30, **implemented and archived 2026-07-31** — see
[TEMP-scan-recipe-plan.md](TEMP-scan-recipe-plan.md)). `--scan-window` refusal folded into
[../gap-vocabulary.md](../gap-vocabulary.md); deferred sketches archived 2026-07-31
([TEMP-gap-selection-deferred.md](TEMP-gap-selection-deferred.md)).

| Doc | Role |
|-----|------|
| [TEMP-gap-selection-sequencing-plan.md](TEMP-gap-selection-sequencing-plan.md) | Order + scope fence (archived after promote); recipe-first rejected |
| [TEMP-gap-selection-plan.md](TEMP-gap-selection-plan.md) | Selection v1 (shipped) |
| [TEMP-gap-selection-ranges-plan.md](TEMP-gap-selection-ranges-plan.md) | Selection v1.5 ranges (shipped) |
| [TEMP-gap-index-convention-plan.md](TEMP-gap-index-convention-plan.md) | Gap index convention (shipped) |
| [TEMP-scan-recipe-plan.md](TEMP-scan-recipe-plan.md) | `ScanRecipe` + JSON scan-params echo (shipped 2026-07-31) |
| [TEMP-gap-selection-deferred.md](TEMP-gap-selection-deferred.md) | Refused `--scan-window` + unbuilt `--gaps-from` sketch (archived 2026-07-31) |

## Module splits & large refactors (shipped)

| Doc | Topic |
|-----|-------|
| [TEMP-policies-module-split-plan.md](TEMP-policies-module-split-plan.md) | Repair policies module split |
| [TEMP-clip-sync-policies-module-split-plan.md](TEMP-clip-sync-policies-module-split-plan.md) | Analyzer policies module split |
| [TEMP-gap-fingerprint-module-split-plan.md](TEMP-gap-fingerprint-module-split-plan.md) | Fingerprint module split |
| [TEMP-gap-fingerprint-corpus-module-split-plan.md](TEMP-gap-fingerprint-corpus-module-split-plan.md) | Fingerprint corpus module split |
| [TEMP-patch-audio-module-split-plan.md](TEMP-patch-audio-module-split-plan.md) | `patch_audio` module split |
| [TEMP-patch-audio-bracket-fill-elimination-plan.md](TEMP-patch-audio-bracket-fill-elimination-plan.md) | `bracket_fill` elimination |
| [TEMP-region-characterization-collapse-plan.md](TEMP-region-characterization-collapse-plan.md) | `RegionCharacterization` collapse |
| [TEMP-repeat-band-plan.md](TEMP-repeat-band-plan.md) | FFT repeat-band (lever 1b(b); numbers in [../repair-perf.md](../repair-perf.md) §1c / §3) |

## Testing

| Doc | Topic |
|-----|-------|
| [test-tier-plan.md](test-tier-plan.md) | Tier migration history (archived 2026-06); live how-to: [../test-tiers.md](../test-tiers.md) |

## Everything else

Other `*.md` (and one archived script) in this folder are older design / implementation plans —
energy signature, residual gate, workspace refactor, alignment, seam repair, W5 diag, etc. Browse
the directory listing; do not treat any as an authoritative current contract.
