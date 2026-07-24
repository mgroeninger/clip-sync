# `gap_fingerprint_corpus.rs` module split — plan

Status: **open** — not started. Target tree:

```text
gap_fingerprint_corpus/
  mod.rs              # thin facade; stable `clip_sync_repair_harness::gap_fingerprint_corpus::*`
  schema.rs           # ~650 lines — JSON projection + taxonomy + GapRow
  analysis.rs         # ~480 lines — dir walk, gap_row, analyze_dirs, curated/env
  report.rs           # ~900 lines — CorpusReport + summary/CSV/golden text
```

Callers keep `clip_sync_repair_harness::gap_fingerprint_corpus::{…}`; no import sweep.
Unit tests stay in each submodule’s `#[cfg(test)]` block (not `tests/` / `*_test.rs`).

**M-MOD context.** This is the **harness corpus slice** of
[M-MOD](TEMP-rust-review-findings.md#m-mod-oversized-modules--open) (findings L442–443).
Sibling M-MOD bites (production `gap_fingerprint` → schema / measure / project — see
[`TEMP-gap-fingerprint-module-split-plan.md`](TEMP-gap-fingerprint-module-split-plan.md);
optionally `patch_audio` / `align_videos`) are **out of scope** here. Policies slice is done —
see [`TEMP-policies-module-split-plan.md`](TEMP-policies-module-split-plan.md).

**Ground rules (same as policies).** Byte-preserving moves only; never bundle into a
behavior-change PR. Pure move/split + `pub(crate)` for cross-submodule helpers. Facade
re-exports the pre-split public API. Do **not** unify this harness schema with production
`GapCorpus` — the minimal deserialize projection is intentional drift tolerance.

Companions: [archive/TEMP-w5-timing-offset-rescue-plan.md](archive/TEMP-w5-timing-offset-rescue-plan.md)
§5 P0, `gap-fingerprint-stats` bin, [`golden_baseline.rs`](../../crates/clip-sync-repair-harness/src/golden_baseline.rs).

---

## 1. Problem

`crates/clip-sync-repair-harness/src/gap_fingerprint_corpus.rs` is a ~2.3 kloc monolith
mixing three concerns:

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
| `schema.rs` | `CorpusFile`…`Outcome` (private `Deserialize`); `SkewClass`, `GapKind`, `SeamDiag`, `SpliceDiag`; seam/splice threshold consts; `GapRow` + row methods (`splice_diag`, `dualfit_target`, …) | JSON types stay private; facade re-exports public taxonomy + `GapRow` |
| `analysis.rs` | `analyze_dirs`, `pair_dirs` / `read_pair` / `read_corpus_json`, `gap_row`, seam-side helpers (`worst_seam_side`, `seam_diag`, `pair_min`); `gap_rows_from_corpus_json`, curated_* loaders, `drift_eps_from_env` / `tail_secs_from_env` | Builds `CorpusReport { rows, pairs, … }`; uses `schema::*` |
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
| **P1** | `schema.rs` | Pending | Move Deserialize projection + enums + thresholds + `GapRow` (+ its `impl`). Colocate GapRow-focused unit tests that do not need dir I/O. |
| **P2** | `analysis.rs` | Pending | Move I/O + `gap_row` + `analyze_dirs` + curated/env. Colocate aggregation / hygiene tests that call `analyze_dirs`. |
| **P3** | `report.rs` | Pending | Move `CorpusReport` + formatting + report helpers. Leave formatting-only tests here if any are split out; otherwise keep with analysis if they assert via `analyze_dirs` + `summary_text`. |
| **P4** | Thin `mod.rs` only | Pending | Delete the monolith file; `lib.rs` keeps `pub mod gap_fingerprint_corpus;` (now a directory). |

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

- [ ] No production file under `gap_fingerprint_corpus/` is a multi-concern monolith; unit
      tests colocated.
- [ ] Bin / `golden_baseline` / `corpus_projection` / `gap_repair_spec_projection` imports
      unchanged at the `gap_fingerprint_corpus::` path.
- [ ] Schema / analysis / report each have a dedicated home matching M-MOD’s wording.
- [ ] Corpus row of M-MOD closable for this bite; production `gap_fingerprint` split remains
      a separate plan.

---

## Related

- [TEMP-rust-review-findings.md](TEMP-rust-review-findings.md) — **M-MOD**
- [TEMP-policies-module-split-plan.md](TEMP-policies-module-split-plan.md) — prior M-MOD slice (ground-rules template)
- [TEMP-gap-fingerprint-module-split-plan.md](TEMP-gap-fingerprint-module-split-plan.md) — sibling production fingerprint split
- `crates/clip-sync-repair-harness/src/gap_fingerprint_corpus.rs` — current monolith
