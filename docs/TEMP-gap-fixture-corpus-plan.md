# Gap-type fixture corpus — plan (DRAFT)

Status: **draft / Phases 0–2 done** (2026-07-21).

Replace the fragile dependency on the ephemeral, licensed-media-derived `gap-files/` corpus with a curated,
committed set of **per-gap-type fingerprint fixtures** — one self-contained JSON per gap *cell*, so the
validation tests assert against named gap types and run on **every PR with no media**. Tracked in
[BACKLOG.md](../BACKLOG.md). Background: [gap-vocabulary.md](gap-vocabulary.md);
[[gap-files-ephemeral-goldens-durable]] (memory).

## Problem

The golden/validation tests are wired to `gap-files/re-anchor-dual-fit-on-nominal`, which is:

- **Ephemeral** — `gap-files/` is gitignored (derived from licensed media, treated as a licensing concern),
  so it can vanish at any time. The two corpus-dependent validation tests then silently **skip**.
- **Unrecoverable** — re-anchor's source media is gone (confirmed: zero source-ID overlap with the newer
  `equiv-coarse-vs-fine`; the two corpora are different media, not a re-scan). We cannot regenerate it.
- **Coarse** — `assert_footguns` couples assertions to gap *indices* in one big corpus (`1·g19`, a frozen
  9-target set). It tests aggregate snapshots, not named gap **types**.

The durable artifact today is the committed golden JSON (non-identifying fingerprint projection — safe to
commit). The corpus that produced it is not. This plan makes the **per-gap fixtures** themselves the durable,
committed artifact, keyed to gap type.

## Taxonomy (vocabulary cells × classifier enums)

The set is grounded in [gap-vocabulary.md](gap-vocabulary.md) cross-referenced with the enums the analyzer
actually emits — `GapEquivalenceClass` (`domain/gap_equivalence.rs`), `GapPatchSkipReason` /
`GapFillSkipReason` (`domain/patch_result.rs`), and the fingerprint readouts `LagVerdict` +
`dualfit_target()` (`application/gap_fingerprint.rs`). Discriminators that separate the cells:
`dNom` = donor silence fraction at nominal program time; `dAlign` = donor bridges once registered;
`df` = `splice_dualfit` present (reached seam scoring).

| # | Fixture type | Signal | Source | Guards |
|---|--------------|--------|--------|--------|
| 1 | `bracket_patch_clean` | patch; dNom=0, dAlign=T | re-anchor 1·g6 | Normal path: both seams pass at lag 0 |
| 2 | `bracket_patch_donor_broken` | patch; dNom=1, dAlign=F | re-anchor 1·g1 | **Footgun:** donor-broken must NOT block a bracket-cleared patch |
| 3 | `silence_splice_dualfit_target` | skip; dNom=0, dAlign=T, timing_offset | re-anchor 7·g3 | Dual-fit addressable: own-lag shoulders + step + B occupied |
| 4 | `program_quiet` | skip; seams ~0.998, dNom=0.97, dAlign=F | re-anchor 1·g19 | **Footgun:** high seam score + dead donor ⇒ NOT a dual-fit target |
| 5 | `no_placement` | skip; df=F | re-anchor 4·g0 | Structure/anchor search found nothing; never seam-scored |
| 6 | `repairable_dropout` | class=repairable_dropout, drop=F | equiv 1·g1 | A died ∧ B occupied ⇒ keep → proceeds into seam cells |
| 7 | `shared_silence` | class=shared_silence, drop=T | equiv 1·g0 | B silent at nominal ⇒ plan-time drop (program-quiet disposition) |
| 8 | `ambient_quiet` | class=ambient_quiet, drop=T | equiv 2·g19 | New cell: A is room tone, decided on A's character not donor |
| 9 | `decorrelated` | lag=decorrelated, bare correlation-skip, donor occupied | **synthetic** | B has *different* content; skips at any lag, no rescue |
| 10 | `residual_veto` | skip=ResidualHeadroomExceeded | **synthetic** (or `residual_gate_catalog`) | Seams pass but B≠A cancellation ⇒ anti-echo veto |
| 11 | `tail_geometry_mismatch` | filtered pre-scoring | **synthetic** | Length-mismatch tail, excluded from matched denominator |
| 12 | `unfillable` (1–2) | BExtractFailed / ZeroLengthGap | **synthetic** | Structural non-fill, no judgment |

**Real vs synthetic (evidence-backed):** #1–8 have clean real members and are extracted in Phase 0.
#9 `decorrelated` is genuinely **n=0** in both corpora — the one decorrelated-*lag* candidate (equiv 5·g2)
resolves to `tier=patch`, not a decorrelated-skip, confirming the vocab doc. #10–12 are also n=0. All four
become hand-built synthetics (Phase 5).

**Excluded (decided):** `not_evaluated` (degenerate — gate off); pair-level aborts `TrackLayoutMismatch` /
`TrackCompatibilityUnavailable` (abort the whole pair, no per-gap cell); `5·g0` bracket-exhausted-gate-
unmeasured (the vocab doc notes it disappears in the characterize→execute pipeline).

**Scope note:** one representative per cell **to start**; high-value cells (silence-splice, program-quiet,
bracket-patch) will grow to 2–3 members each once the harness is in place.

## Constraint

- Fixtures are the non-identifying single-gap `GapCorpus` projection (hashed source IDs, no titles/paths) —
  same licensing-safe class as the committed golden. Leak-scanned in Phase 0 (clean).
- re-anchor and equiv were dumped at **different diagnostic tiers**: re-anchor carries
  `splice_dualfit`/`residual`/`splice` (seam/dual-fit cells) but no equivalence; equiv carries
  `equivalence`/`scan_equivalence` but no dual-fit fields. Each fixture asserts only on the fields its type
  needs — the loader pairs each file with type-appropriate assertions, not a uniform schema.

## Phases

- **Phase 0 — Extract & stage. ✅ DONE (2026-07-21).** 8 real fixtures copied from both corpora into
  `crates/clip-sync-repair/tests/gap_corpus/fingerprints/curated/` with `manifest.json` (type, provenance:
  corpus/pair/gap-index/original-filename, expected assertion). All parse as single-gap `GapCorpus`; tracked
  (not gitignored); leak-scanned clean. This preserves the raw material against `gap-files/` deletion.
- **Phase 1 — Loader. ✅ DONE (2026-07-21).** `clip_sync_repair_fixtures::gap_cell_fixtures` reads
  `manifest.json` + the JSONs and exposes typed records: `GapCellType` (complete taxonomy enum incl. the four
  synthetic-only cells), `GapCellFixture { cell_type, file, expected, provenance, corpus }` with a `.gap()`
  accessor, and `load_gap_cell_fixtures()`. Path resolved from the crate manifest dir (CWD-independent).
  3 loader unit tests: every fixture loads single-gap with matching index; all Phase-0 cells present;
  manifest ↔ on-disk `.json` files agree. Extends the existing committed fixture precedent
  (`tests/gap_corpus/fingerprints/g003_timing_offset.json`).
- **Phase 2 — Per-type assertion test. ✅ DONE (2026-07-21).** `tests/gap_cell_fixtures.rs` (PR-tier, no
  feature gate) runs **live** classifiers on each committed fixture and asserts the declared cell: seam/donor
  cells via the analyzer `GapRow` predicates (`patched()`, `bracket_exhausted()`, `dualfit_target()`,
  `program_quiet()`, `brackets_total`) — reached through a new public `gap_rows_from_corpus_json` in the
  harness; equivalence cells re-run the domain `classify_gap_equivalence()` on the fixture's recorded silence
  signals. Both vocabulary footguns are pinned (silence-splice **is** a target; program-quiet is **not**).
  Reads the committed bytes directly (no re-serialization round-trip). Media-independent replacement for
  `assert_footguns`' index-coupled semantics.
- **Phase 3 — Re-home the golden.** Regenerate the aggregate golden *from the committed set* (self-hosting —
  reproducible with zero external media). Repoint `golden_baseline_invariance` + `gap_repair_spec_diff` at
  the committed set.
- **Phase 4 — Retire re-anchor.** Recast `assert_footguns` onto the Phase-2 expectations; drop the re-anchor
  golden + all `gap-files/` default-path references from tests and docs (`golden_baseline_smoke`,
  `test-tier.ps1`, `test-tiers.md`, `golden/README.md`, the `dual_fit.rs` comment). Fragility gone.
- **Phase 5 — Synthetic cells.** Hand-build `decorrelated`, `residual_veto`, `tail_geometry_mismatch`,
  `unfillable` (reuse `residual_gate_catalog` / the synthetic `synth_ab_from_decode_corpus` generator where
  possible). Completes coverage of the n=0 production cells.

Each phase lands independently and leaves the tree green; re-anchor stays wired until Phase 4 flips over.

## Out of scope

- Growing beyond one-per-cell (deferred; do after Phase 4 proves the harness).
- Changing the `GapCorpus` schema or the analyzer classification logic — fixtures capture current output.
- The `equiv-coarse-vs-fine` / `re-anchor` raw dirs themselves — they remain disposable scratch once the
  Phase 0 fixtures are committed.
