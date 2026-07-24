# `gap_fingerprint` module split — plan

Status: **open** (2026-07-23). Target tree (after all phases):

```text
application/gap_fingerprint/
  mod.rs              # thin facade; stable `crate::application::gap_fingerprint::*`
  schema.rs           # serde corpus + per-gap types, source_id, DetailTier
  project.rs          # GapRepairSpec ↔ GapFingerprint (8e/8f/8g projection)
  measure.rs          # lag probe, build/characterize, write_corpus_dir
```

Callers keep `crate::application::gap_fingerprint::{…}`; no import sweep. Unit tests stay in
each submodule’s `#[cfg(test)]` block (not `tests/` / `*_test.rs`).

**Harness corpus is a separate plan.** `gap_fingerprint_corpus` → schema / analysis / report
lives in
[`TEMP-gap-fingerprint-corpus-module-split-plan.md`](TEMP-gap-fingerprint-corpus-module-split-plan.md)
(same ground rules; do not fold harness phases into this ledger).

**M-MOD context.** This is the **production fingerprint** slice of
[M-MOD](TEMP-rust-review-findings.md#m-mod-oversized-modules--open). Policies is done
([`TEMP-policies-module-split-plan.md`](TEMP-policies-module-split-plan.md)). Remaining M-MOD
bites (`patch_audio`, `align_videos`, optional repair `lib.rs` curation) are **out of scope**
here.

**Companion history (do not re-open).** Fingerprint feature work is closed
([archive/TEMP-gap-fingerprint-plan.md](archive/TEMP-gap-fingerprint-plan.md),
[gap-fingerprint.md](gap-fingerprint.md)). Perf 8g.5 deferred by measurement — this plan is
**M-MOD maintainability only**, not a perf lever. Rule kept: byte-preserving moves, never
bundled into behavior-change PRs.

Companions: [TEMP-policies-module-split-plan.md](TEMP-policies-module-split-plan.md) (ground-rules
template), [TEMP-gap-fingerprint-corpus-module-split-plan.md](TEMP-gap-fingerprint-corpus-module-split-plan.md),
[TEMP-rust-review-findings.md](TEMP-rust-review-findings.md) **M-MOD**.

---

## 1. Problem

`application/gap_fingerprint.rs` (~4.3 kloc; ~3.3 k production + ~1.0 k colocated tests) mixes
three concerns in one file:

| Concern | Rough locus today | Role |
|---------|-------------------|------|
| **Schema** | § Corpus envelope + § Per-gap fingerprint (~L32–696) | Licensing-safe serde types + `source_id` |
| **Project** | § GapRepairSpec ↔ GapFingerprint (~L697–1210) | Pure 8e/8f/8g projection (`spec_to_fingerprint_summary`, `tags_from_*`, `fingerprint_to_spec`) |
| **Measure** | § Lag probe + builder/characterize/write (~L1212–3293) | PCM → fingerprint (`build_gap_fingerprint`, `characterize_*`, `write_corpus_dir`) |

Helpers that *compute* values (`level_profile`, `seam_prominence`, lag sweep knobs) currently sit
inside the schema section next to the types they fill — they belong with **measure**.

Harness corpus split is **out of scope** here — see
[`TEMP-gap-fingerprint-corpus-module-split-plan.md`](TEMP-gap-fingerprint-corpus-module-split-plan.md).

---

## 2. Non-goals

- **Renaming the public path** — keep `crate::application::gap_fingerprint::*` via re-exports.
- **Changing behavior** — pure move/split only (serde field order / JSON shape / thresholds /
  classifiers unchanged).
- **Harness `gap_fingerprint_corpus` split** — separate plan (linked above).
- **Moving `lag_correlation_curve` ownership** — already shared in `domain::seam_local`; keep the
  `pub use` re-export on the fingerprint facade (or `measure`) so call sites stay stable.
- **M-MOD siblings** — `patch_audio` / `align_videos` / repair `lib.rs` curation are separate.
- **Perf / algorithm work** — no 8g.5 reopens, no threshold retunes, no new fields.
- **Separating unit tests from production code** — do **not** move `#[cfg(test)]` into `tests/`
  or `*_test.rs`; keep tests at the bottom of the same `.rs` file that owns the logic.

---

## 3. Final layout

### 3a. `clip-sync-repair` — `application/gap_fingerprint/`

| Module | Owns | Notes |
|--------|------|-------|
| `schema.rs` | `FileSource`, `ScanRecipe`, `SourceMeta`, `GapCorpus`, `DetailTier`, `GapFingerprint` + nested types (`GapGeometry`, `LevelProfile`, `SilenceProfile`, `ContourInfo`, anchors/brackets, `StructureScores`, `SeamScores`, `Lag*`, `SeamProbe*`, `ResidualInfo`, `SpliceSummary`, `SpliceDualfit`, `WideEnvelope*`, outcome/equivalence, …), serde `de_*_null_as_nan`, `source_id` / `file_source` | **Types + identity only.** No PCM measurement loops. |
| `project.rs` | `FingerprintXSet`, `spec_to_fingerprint_summary`, `projected_*` / `synth_brackets` helpers, `tags_from_fields` / `tags_from_fingerprint` / `tags_from_measurements`, `fingerprint_to_spec` | Pure Spec ↔ Fingerprint. Depends on `schema` (+ `RegionMeasurements` from `measure` **or** a thin shared measurements view — prefer keeping `tags_from_measurements` next to `RegionMeasurements` if that avoids a cycle; see §3b). |
| `measure.rs` | Lag summarize/classify, placement/lag/seam-probe/dualfit/wide-envelope builders, `build_gap_fingerprint`, `compute_region_measurements`, `characterize_gaps*`, `write_corpus_dir`, `splice_summary_from_lag`, level/RMS helpers currently interleaved with schema | Depends on `schema` + `project` (from-decode dump projects via `spec_to_fingerprint_summary`). |
| `mod.rs` | Re-exports only | No unit tests |

### 3b. Dependency direction (must hold)

```text
schema  ←  project  ←  measure
schema  ←  measure
```

**Cycle guard.** Today `tags_from_measurements` sits in the projection section but takes
`RegionMeasurements` (measure). Resolve one of:

1. **Preferred:** keep `tags_from_measurements` in `measure.rs` next to `RegionMeasurements`;
   `project.rs` owns only Spec↔Fingerprint pure maps (`tags_from_fingerprint`,
   `spec_to_fingerprint_summary`, `fingerprint_to_spec`).
2. Or extract a tiny `RegionMeasurements` DTO into `schema` / a shared private module — only if (1)
   forces awkward visibility.

Do **not** let `project` depend on `measure` and `measure` depend on `project`.

### Internal visibility

- Shared helpers stay `pub(crate)` in the owning submodule.
- Facade re-exports the pre-split **public** API (`GapCorpus`, `GapFingerprint`,
  `build_gap_fingerprint`, `characterize_gaps*`, `spec_to_fingerprint_summary`,
  `tags_from_fingerprint`, `fingerprint_to_spec`, `summarize_lag_curve`, lag re-exports,
  `splice_summary_from_lag`, `source_id`, …).
- `write_corpus_dir` stays `pub(crate)` on the facade (composition / dump path).

---

## 4. Phase ledger

Extract **one cohesion slice per phase**. Do not combine with behavior PRs.

| Phase | Slice | Status |
|-------|-------|--------|
| **P1** | `schema.rs` (types + `source_id`; strip measure helpers out of the type section) | **Done (2026-07-23)** |
| **P2** | `project.rs` (Spec↔Fingerprint; apply §3b cycle guard) | **Done (2026-07-23)** |
| **P3** | Remainder → `measure.rs` + thin `mod.rs` facade; delete monolith `.rs` | **Open** |

**P2 as-landed (2026-07-23).** Extracted the pure Spec↔Fingerprint projection into
`gap_fingerprint/project.rs` (503 lines): `FingerprintXSet`, `spec_to_fingerprint_summary`,
`fingerprint_to_spec`, `tags_from_fingerprint`, `tags_from_fields` (the shared core), and the private
helpers (`projected_lag_entry`, `synth_brackets`, `failure_stage_from_tag`, `projected_level_profile`,
`skip_reason_tag`, `mono_lag`, `failure_stage_tag`). `mod.rs` gained `mod project; pub use project::*;`.
**Cycle guard applied (§3b option 1):** `tags_from_measurements` stayed in `mod.rs` (measure — it
consumes `RegionMeasurements`) and now calls the shared `tags_from_fields`, which is `pub(crate)` in
`project`. `project.rs` imports only `super::schema::*` + domain tag structs — **verified zero
references to any measure symbol** (`RegionMeasurements`/`summarize_lag_curve`/`to_db`/… absent from
project code), so the `schema ← project ← measure` DAG holds with no `project ↔ measure` edge.
Non-move deltas (normalized union diff vs the P1 tree: 3 removed / 16 added, **zero function bodies
changed**): `SILENCE_FLOOR_DB` relocated from `mod.rs` to `schema.rs` as `pub(crate)` (it is the
`LevelProfile::floor_db` sentinel, shared by projection + measure — moving it to the common `schema`
dependency keeps the P3 DAG clean); `tags_from_fields` → `pub(crate)`; the
`use crate::domain::gap_repair_spec::{…}` import moved verbatim mod.rs→project.rs (one test's local
`use` block gained `GapRepairSpec`, which it had been taking from the now-removed top-level import);
module docs added. **Verified:** `cargo build/clippy -p clip-sync-repair --all-targets` clean;
`-Tier pr-repair` green (repair lib 382 pass; `gap_repair_spec_diff` 4/4 and `golden_baseline_invariance`
2/2 — the projection round-trip guards — plus harness 13/13).

**P1 as-landed (2026-07-23).** `git mv` monolith → `gap_fingerprint/mod.rs`; extracted the schema
types + serde `de_*_null_as_nan` helpers + `source_id`/`file_source`/`fnv_feed`/`impl ScanRecipe` +
the `domain::donor` re-export into `gap_fingerprint/schema.rs` (559 lines). `mod.rs` gained
`mod schema; pub use schema::*;` so the public path is unchanged (no import sweep). The interleaved
**measure** helpers were deliberately left in `mod.rs` for P3: `LevelProfileSpan`/`level_profile`/
`mono_slice_rms`, `LAG_EDGE_TOL_MS`/`LagSweepParams`/`LagSideSweep`, `DUALFIT_SEAM_UNIQ_LAG_MS`/
`SEAM_LOCAL_SEARCH_MS`/`seam_prominence`. Non-move deltas (normalized line-set diff: 7 removed /
14 added, **zero function bodies or serde attrs changed**): `file_source` → `pub(crate)` (called by
`characterize_gaps`); the `[`LAG_EDGE_TOL_MS`]` intra-doc link in `LagSummary::edge_pinned` softened
to inline code (target now in `mod.rs`, private — avoids a private-intra-doc warning); the two
`#[derive(Serialize)]` on `Manifest`/`ManifestEntry` fully-qualified to `serde::Serialize` (drops the
now-unused `use serde` from `mod.rs`); module docs refreshed. **Verified:** `cargo build/clippy
-p clip-sync-repair --all-targets` clean; `-Tier pr-repair` green (repair lib 382 pass, harness 13/13).

Harness corpus phases: **not here** — see
[`TEMP-gap-fingerprint-corpus-module-split-plan.md`](TEMP-gap-fingerprint-corpus-module-split-plan.md)
(P1–P4). Optional later (still M-MOD, **not** this plan’s success gate): `patch_audio` /
`align_videos` splits; repair `lib.rs` curation like `clip-sync`.

---

## 5. Verification (per phase / final gate)

- `cargo build -p clip-sync-repair --all-targets`
- `cargo clippy -p clip-sync-repair --all-targets`
- `.\scripts\test-tier.ps1 -Tier pr-repair`

After **P3**, confirm external call sites still compile without import edits:
`composition.rs`, `equivalence_calibration`, `domain/seam_local` tests, harness consumers of
`gap_fingerprint::*`.

---

## 6. Success criteria

- [ ] No production file under `application/gap_fingerprint/` is a multi-concern monolith; unit
      tests colocated per submodule.
- [ ] Public import paths unchanged (`gap_fingerprint::`).
- [ ] Dependency DAG in §3b holds (no `project` ↔ `measure` cycle).
- [ ] JSON corpus shape unchanged (round-trip / curated fixtures still green).
- [ ] Fingerprint row of M-MOD closable; harness corpus + `patch_audio` / `align_videos` remain
      separate plans.

---

## Related

- [TEMP-rust-review-findings.md](TEMP-rust-review-findings.md) — **M-MOD**
- [TEMP-policies-module-split-plan.md](TEMP-policies-module-split-plan.md) — prior M-MOD slice (done)
- [TEMP-gap-fingerprint-corpus-module-split-plan.md](TEMP-gap-fingerprint-corpus-module-split-plan.md) — harness sibling
- [gap-fingerprint.md](gap-fingerprint.md) — product docs (update path note when tree lands)
- [archive/TEMP-gap-fingerprint-plan.md](archive/TEMP-gap-fingerprint-plan.md) — original feature plan
- `crates/clip-sync-repair/src/application/gap_fingerprint.rs` — current monolith
