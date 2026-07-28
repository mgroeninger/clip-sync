# `gap_fingerprint_corpus.rs` module split — plan

> **Archived 2026-07-24.** Planned M-MOD harness corpus bite; shipped. Record only.

Status: **COMPLETE (P1–P4 done, 2026-07-23)** — `schema.rs`, `analysis.rs`, and `report.rs` extracted; the
parent is now `gap_fingerprint_corpus/mod.rs`, a thin facade (`mod schema; mod analysis; mod report;` +
re-exports) carrying only the cross-cutting integration tests (310 lines). Public path
`clip_sync_repair_harness::gap_fingerprint_corpus::*` unchanged; `lib.rs` keeps
`pub mod gap_fingerprint_corpus;` (now a directory). Corpus slice of M-MOD is closed. Final tree:

```text
gap_fingerprint_corpus/
  mod.rs              # thin facade; stable `clip_sync_repair_harness::gap_fingerprint_corpus::*`
  schema.rs           # ~414 lines — taxonomy + thresholds + GapRow (DONE)
  analysis.rs         # ~666 lines — JSON projection + dir walk, gap_row, analyze_dirs, curated/env (DONE)
  report.rs           # ~900 lines — CorpusReport + summary/CSV/golden text
```

> **P1 deviation from the original grouping (deliberate, byte-preserving rule wins).** The minimal
> `Deserialize` projection (`CorpusFile`…`Outcome`) was *not* moved into `schema.rs`. Its only consumers
> are analysis's `gap_row` / `seam_diag` / `worst_seam_side` / `pair_min`, which read the structs' **private
> fields**; hoisting them into a child module would have forced `pub(crate)` on ~20 structs / ~80 fields (an
> ~80-line visibility churn that leaks parse internals crate-wide) — violating ground rule #1. So the
> projection **stays with its consumer** and travels to `analysis.rs` in P2, fully private. `schema.rs` is
> therefore the pure taxonomy + `GapRow` + thresholds leaf (~414 lines, no serde import), and `analysis.rs`
> absorbs the projection (~640). `LOW_UNIQUENESS_MARGIN` moved into `schema.rs` (used by `GapRow`); the six
> `SEAM_*` / `SPLICE_MIN_*` thresholds are `pub(crate)` there (shared with analysis + report).

Callers keep `clip_sync_repair_harness::gap_fingerprint_corpus::{…}`; no import sweep.
Unit tests stay in each submodule’s `#[cfg(test)]` block (not `tests/` / `*_test.rs`).

**M-MOD context.** This was the **harness corpus slice** of
[M-MOD](TEMP-rust-review-findings.md#m-mod-oversized-modules--closed) (findings L442–443).
Sibling planned M-MOD bites (production `gap_fingerprint`, `patch_audio`) are also done;
`align_videos` remains deferred. Policies slice is done —
see [`TEMP-policies-module-split-plan.md`](TEMP-policies-module-split-plan.md).
Those siblings were **out of scope** here.

**Ground rules (same as policies).** Byte-preserving moves only; never bundle into a
behavior-change PR. Pure move/split + `pub(crate)` for cross-submodule helpers. Facade
re-exports the pre-split public API. Do **not** unify this harness schema with production
`GapCorpus` — the minimal deserialize projection is intentional drift tolerance.

Companions: [TEMP-w5-timing-offset-rescue-plan.md](TEMP-w5-timing-offset-rescue-plan.md)
§5 P0, `gap-fingerprint-stats` bin, [`golden_baseline.rs`](../../../crates/clip-sync-repair-harness/src/golden_baseline.rs).

---

## 1. Problem (resolved)

Pre-split, `crates/clip-sync-repair-harness/src/gap_fingerprint_corpus.rs` was a ~2.3 kloc
monolith mixing three concerns:

| Concern | Approx. lines today | What it is |
|---------|---------------------|------------|
| Schema | ~650 | Minimal `Deserialize` projection of `corpus.json` + public taxonomy (`SkewClass`, `GapKind`, `SeamDiag`, `SpliceDiag`) + analyzed `GapRow` (+ row classifiers) |
| Analysis | ~480 | Pair-dir walk / JSON read, `gap_row` projection, `analyze_dirs`, curated fixture loaders, env knobs |
| Report | ~900 | `CorpusReport` + `summary_text` / mechanism / gate / splice / dual-fit / CSV / golden |

Natural seams already exist as section comments (`// ── minimal schema projection ──`,
`CorpusReport` shell, `impl CorpusReport { … }`, curated entrypoints). Split along those;
do not redesign classification or report text.

## 2. Non-goals

- **Renaming the public path** — keep `clip_sync_repair_harness::gap_fingerprint_corpus::*`
  via re-exports from `mod.rs`.
- **Changing behavior** — pure move/split only (same parse fallbacks, same thresholds,
  same summary/CSV strings).
- **Splitting production `gap_fingerprint.rs`** — that is the sibling M-MOD bite
  (schema / measure / project); separate plan.
- **M-MOD siblings** — `patch_audio` / `align_videos` / repair `lib.rs` curation are separate.
- **M-HARNESS CSV quoting** — do **not** introduce the `csv` crate or change RFC 4180
  quoting here (findings M-HARNESS item 4); move `csv()` as-is.
- **Separating unit tests from production code** — do **not** move `#[cfg(test)]` into
  `tests/` or `*_test.rs`; keep tests at the bottom of the same `.rs` file that owns the
  code under test (split only when a test clearly belongs with one submodule).
- **Changing the JSON projection surface** — still a *minimal* schema; no new fields,
  no tightening serde.

## 3. Final layout

| Module | Owns | Notes |
|--------|------|-------|
| `schema.rs` **(done)** | `SkewClass`, `GapKind`, `SeamDiag`, `SpliceDiag`; seam/splice + uniqueness threshold consts (`pub(crate)`); `GapRow` + row methods (`splice_diag`, `dualfit_target`, …) | Facade re-exports public taxonomy + `GapRow`; imports only the two repair-domain margins, no serde |
| `analysis.rs` | `CorpusFile`…`Outcome` (private `Deserialize`); `analyze_dirs`, `pair_dirs` / `read_pair` / `read_corpus_json`, `gap_row`, seam-side helpers (`worst_seam_side`, `seam_diag`, `pair_min`); `gap_rows_from_corpus_json`, curated_* loaders, `drift_eps_from_env` / `tail_secs_from_env` | JSON projection stays private *here* (its only consumer); builds `CorpusReport { rows, pairs, … }`; uses `schema::*` |
| `report.rs` | `CorpusReport` + all `*_text` / `csv` / `golden_*` methods; report-only helpers (`pct`, `stats`, `linfit`, `quantization_residual`) | Consumes `&[GapRow]` / `GapRow` methods; no JSON I/O |
| `mod.rs` | Re-exports only | No unit tests |

### Internal visibility

- Shared helpers stay `pub(crate)` in the owning submodule (`gap_row` helpers, report
  stats, etc.).
- Facade re-exports the pre-split **public** API (`GapRow`, `CorpusReport`, taxonomy
  enums, `analyze_dirs`, curated/env entrypoints). Private `Deserialize` types are **not**
  re-exported (they were never public).

### Dependency direction

```text
schema  ←  analysis  ←  (constructs)  report::CorpusReport
schema  ←  report    (formats GapRow)
mod.rs  re-exports all three
```

No cycles: `golden_baseline` already depends on this module’s public types; keep
`CorpusReport::golden_baseline` / `golden_json` in `report.rs` (same crate-level pattern
as today).

## 4. Phase ledger

| Phase | Module | Status | Notes |
|-------|--------|--------|-------|
| **P1** | `schema.rs` | **Done (2026-07-23)** | Moved taxonomy enums + thresholds + `GapRow` (+ its `impl`). Deserialize projection deliberately left with analysis (see deviation note). No GapRow-only tests needed relocating — the two GapRow-focused tests (`splice_diag_uses_peak_z_when_present`, `dualfit_target_scopes_…`) still pass via the facade and can move to `schema.rs`'s `#[cfg(test)]` in a later tidy. Build/clippy/lib tests green; `clip-sync-repair --all-targets` green (facade intact). |
| **P2** | `analysis.rs` | **Done (2026-07-23)** | Moved the Deserialize projection (`CorpusFile`…`Outcome`, fully private) + `DEFAULT_*` consts + I/O (`pair_dirs`/`read_pair`/`read_corpus_json`) + `gap_row` + seam helpers + `analyze_dirs` + curated/env. Imports `super::schema::{taxonomy, SEAM_* consts}` + `super::CorpusReport`. The integration tests (`analyze_dirs` → `summary_text`/`csv`) touch only the public surface, so they **stayed in the parent** (they span analysis + report — the plan's "split only when a test clearly belongs with one submodule"). Parent dropped `serde` + `std::path` at top level (`Path` now imported inside `mod tests`); `use schema::{…}` trimmed to the five report-side consts. Build/clippy/lib tests green; `clip-sync-repair --all-targets` green. |
| **P3** | `report.rs` | **Done (2026-07-23)** | Moved `CorpusReport` + all `*_text`/`csv`/`golden_*` methods + report helpers (`pct`/`stats`/`linfit`/`quantization_residual`) + `CLEAN_STEP_MS` (978 lines). Imports `super::schema::{GapKind, GapRow, SkewClass, SpliceDiag, LOW_UNIQUENESS_MARGIN, SEAM_ROBUST_R, SPLICE_MIN_*}` + `BTreeMap` + `PROGRAM_QUIET_SILENCE_FRAC`; `golden_baseline` stays via `crate::` paths. Parent dropped those imports, added `mod report;` + `pub use report::CorpusReport;` (keeps analysis's `use super::CorpusReport;` and the tests' `super::*` resolving); the cross-cutting integration tests stayed in the parent. Build/clippy/lib tests green (13 passed); `clip-sync-repair --all-targets` green. |
| **P4** | Thin `mod.rs` only | **Done (2026-07-23)** | `git mv gap_fingerprint_corpus.rs gap_fingerprint_corpus/mod.rs` (pure move, no content change). `lib.rs` unchanged — `pub mod gap_fingerprint_corpus;` now resolves to the directory. Build/clippy/lib tests green (13 passed); `clip-sync-repair --all-targets` green. |

**Suggested order rationale.** Schema first (no sibling deps), then analysis (needs schema +
constructs report shell), then report formatting, then facade — same “dependency-forward,
then thin mod” pattern as policies P1–P5.

## 5. Verification (per phase / final gate)

- `cargo build -p clip-sync-repair-harness --all-targets`
- `cargo clippy -p clip-sync-repair-harness --all-targets`
- `cargo test -p clip-sync-repair-harness --lib`
- `.\scripts\test-tier.ps1 -Tier pr` (or at least harness lib + `pr-repair` — same bar as
  M-MOD: green after each split)

Smoke (optional, after P4): `gap-fingerprint-stats` still resolves
`gap_fingerprint_corpus::{analyze_dirs, drift_eps_from_env, tail_secs_from_env}`;
`golden_baseline` / `corpus_projection` / `gap_repair_spec_projection` compile unchanged.

## 6. Success criteria

Verified in source 2026-07-24: `gap_fingerprint_corpus/{mod,schema,analysis,report}.rs`
(no leftover monolith `.rs`); bins / `golden_baseline` / projections still import
`gap_fingerprint_corpus::*`.

- [x] No production file under `gap_fingerprint_corpus/` is a multi-concern monolith; unit
      tests colocated.
- [x] Bin / `golden_baseline` / `corpus_projection` / `gap_repair_spec_projection` imports
      unchanged at the `gap_fingerprint_corpus::` path (`clip-sync-repair --all-targets` green).
- [x] Schema / analysis / report each have a dedicated home matching M-MOD’s wording.
- [x] Corpus row of M-MOD closable for this bite; production `gap_fingerprint` split remains
      a separate plan.

---

## Related

- [TEMP-rust-review-findings.md](TEMP-rust-review-findings.md) — **M-MOD**
- [TEMP-policies-module-split-plan.md](TEMP-policies-module-split-plan.md) — prior M-MOD slice (ground-rules template)
- [TEMP-gap-fingerprint-module-split-plan.md](TEMP-gap-fingerprint-module-split-plan.md) — sibling production fingerprint split
- `crates/clip-sync-repair-harness/src/gap_fingerprint_corpus/` — live split tree
