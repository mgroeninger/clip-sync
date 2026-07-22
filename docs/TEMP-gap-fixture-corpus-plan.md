# Gap-type fixture corpus — plan (DRAFT)

Status: **all phases done (0–5)** (2026-07-22). Ready to archive once committed.

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
| 9 | `tail_geometry_mismatch` | `GapKind::Tail` (duration ≥ cutoff) | **real** (re-anchor 6·g14, 363 s) | Filtered pre-scoring, excluded from matched denominator |
| 10 | `decorrelated` | lag=decorrelated, donor occupied, not a target | **synthetic** (from 03) | B has *different* content; skips at any lag, no rescue |
| 11 | `residual_veto` | seams pass, residual headroom > margin, informative | **synthetic** (from 01) | Seams pass but B≠A cancellation ⇒ anti-echo veto |
| — | `unfillable` | BExtractFailed / ZeroLengthGap | **not representable** | Plan/execution-time failure — never characterized; covered by unit tests, not a fixture |

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
- **Phase 3 — Re-home the golden. ✅ DONE (2026-07-21).** New harness `curated_gap_cell_rows()` /
  `curated_gap_cell_projected_rows()` build one analyzer `GapRow` per committed fixture (labelled by cell
  type, so keys are unique despite colliding source indices), and `baseline_from_rows()` (extracted from
  `baseline_from_report`) snapshots them. `golden_baseline_invariance` now diffs the live analysis against a
  **self-hosting** `curated.golden.json` (regenerated *from* the fixtures via `CURATED_GOLDEN_REGEN=1`, zero
  external media); `gap_repair_spec_diff` runs the projection-fidelity differential over the curated set. Both
  are now **media-free pr-repair** tests (dropped `#[ignore]` + `validation-tests`; wired into the pr-repair
  tier — note `gap_repair_spec_diff` never actually ran in CI before). No test depends on `gap-files/` any
  more. *(Data note: `repairable_dropout·g1` is legitimately also a dual-fit target — equivalence class and
  seam disposition are orthogonal; the golden records both.)*
- **Phase 4 — Retire re-anchor. ✅ DONE (2026-07-22).** The `assert_footguns` guards were recast as Phase-2
  per-type assertions (added the missing "seams PASS the gate yet donor-dead" premise — `dualfit_pass ==
  Some(true)` — to the program-quiet arm), and the frozen target set is now pinned by `curated.golden.json`.
  Deleted: `golden_baseline_smoke.rs` + its `[[test]]` entry, `assert_footguns` + `EXPECTED_TARGETS`
  (harness), `re-anchor-dual-fit-on-nominal.golden.json`. Updated: `test-tier.ps1`, `test-tiers.md`,
  `golden/README.md`, `development.md`, the `dual_fit.rs` comment. **Fragility gone** — no test references
  `gap-files/` or the re-anchor golden. (`analyze_dirs` stays: it still powers the `gap-fingerprint-stats`
  calibration bin, `corpus_projection` / `decode_path_projection`, and its own unit tests.)
- **Phase 5 — Remaining cells. ✅ DONE (2026-07-22).** Added `09_tail_geometry_mismatch` (**real** — the
  re-anchor corpus, still on disk, has 7 `duration ≥ 30 s` tails; extracted the 363 s one), plus
  **synthetic** `10_decorrelated` (from `03`: verdict → decorrelated, `gate_pass` → false, seams collapsed)
  and `11_residual_veto` (from `01`: skip + informative residual with 11 dB headroom). Phase-2 arms assert
  each; golden regenerated to 11 gaps. **`unfillable` was found to be not fingerprint-representable** — those
  gaps fail at plan/execution time and never get characterized (only gate/correlation skips reach a
  fingerprint), so it is documented as a taxonomy entry with no fixture (covered by `GapPatchSkipReason` unit
  tests). *Finding: the plan's original "4 synthetics" was imprecise — tail is real, unfillable is unrepresentable.*

Each phase landed independently and left the tree green; re-anchor stayed wired until Phase 4 flipped over.

## Out of scope

- Growing beyond one-per-cell (deferred; do after Phase 4 proves the harness).
- Changing the `GapCorpus` schema or the analyzer classification logic — fixtures capture current output.
- The `equiv-coarse-vs-fine` / `re-anchor` raw dirs themselves — they remain disposable scratch once the
  Phase 0 fixtures are committed.
