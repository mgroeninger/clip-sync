# `patch_audio/region.rs` folder split — plan

Status: **draft / not started**. Maintainability-only peel of the remaining
`region.rs` monolith into `patch_audio/region/` (folder + submodules). Byte-preserving
moves; never bundled into a behavior-change PR.

This is the **deferred dual-fit / bracket peel** the 2026-07-24
[`TEMP-patch-audio-module-split-plan.md`](archive/TEMP-patch-audio-module-split-plan.md)
explicitly left out of its success gate (`region.rs` was allowed to land near ~2 kloc).
The file has since grown to **3262 lines** (~2430 production + ~830 colocated tests).

Companions: [archive/TEMP-patch-audio-module-split-plan.md](archive/TEMP-patch-audio-module-split-plan.md)
(ground rules + DAG that this nest sits under),
[archive/TEMP-policies-module-split-plan.md](archive/TEMP-policies-module-split-plan.md)
(template), [archive/TEMP-region-characterization-collapse-plan.md](archive/TEMP-region-characterization-collapse-plan.md)
(characterize→execute boundary — do not reopen).

---

## 1. Problem

`application/patch_audio/region.rs` is still one file mixing several cohesion slices
that already have named types and free-function seams:

| Concern | Approx. locus today | Role |
|---------|---------------------|------|
| **Types / outcomes** | `RegionPatch`…`seam_failure_outcome` (~L49–419); `skip_region_spec` / `skip_outcome_from_fields` / `outcome_from_spec` (~L1187–1315) | Patch/outcome/media/opts/context + report-order join + spec→outcome |
| **PCM helpers** | `gained_fill` (~L69); `a_gap_floor_db` / `b_mapped_start_frame` (~L421–456); `slice_b_extract`…`splice_into_a` (~L2276–2432) | Slice / gain / splice / border RMS |
| **Dual-fit** | `DualFitRepairInput`…`skip_or_dual_fit` (~L458–843); `DualFitDecision` / `finalize_dual_fit` / `dual_fit_skipped` (~L1119–1315) | Gate-fail rescue |
| **Bracket** | `BracketFillAssembly`…`execute_bracket_output` (~L844–1117) | Assemble fill PCM + wrap as `RegionPatch` |
| **Execute** | `execute_region_spec` (~L1317–1454) | Spec → PCM. Bracket arm re-derives fill; SilenceSplice copies dual-fit PCM |
| **Characterize** | `characterize_region` (~L1465–2218); `prepare_region_patch` shim (~L2250–2274); test-only `characterize_all_regions` (~L2220–2244) | Geometry → gate → dual-fit/reconciliation → `GapRepairSpec` |

Locus table is orientation only. Extract by the §3 ownership table + DAG, not by
contiguous line ranges. Interleaved symbols (`gained_fill` next to `RegionPatch`;
`skip_region_spec` sitting between dual-fit finalize and execute) are called out in
§3 contested.

Natural seams already exist as free functions and type clusters. Split along those.
Do **not** redesign characterize / dual-fit / bracket / execute flow, and do **not**
decompose `characterize_region` itself — it stays one function in `characterize.rs`
even if that file lands near ~850 lines.

---

## 2. Non-goals

- **Renaming the public path** — callers keep `super::region::{…}` from
  `patch_audio/mod.rs` and `anchor_retry.rs`. No `patch_audio::region::types` in
  those files. Facade `region/mod.rs` re-exports the pre-split `pub(super)` surface.
- **Changing behavior** — pure move/split only (same characterize/execute/splice
  outcomes, same verbose strings, same debug_asserts).
- **Splitting `characterize_region` into pass helpers** — that is a later readability
  refactor with real risk of capturing the wrong locals; out of scope.
- **Moving `#[cfg(test)]` into `tests/` or `*_test.rs`** — tests stay at the bottom of
  the owning `.rs`.
- **Reopening M-CFG / request types** — `PatchRequestSettings` / `Deref` stay in
  `request.rs`. The two M-CFG guards that currently live in `region.rs`'s tests
  (`deref_reads_*`, `into_request_defaults_*`) stay with `dual_fit_test_request`
  (see contested); do **not** relocate them to `request.rs` in the same phases.
- **Perf / algorithm work** — no gate retunes, no dual-fit predicate changes, no
  new fields.
- **Touching `anchor_retry.rs` / `geometry.rs` / `log.rs` beyond import paths** —
  they already import from `super::region`; the facade keeps those names.

---

## 3. Final layout

```text
application/patch_audio/region/
  mod.rs              # thin facade; pub(super) re-exports of the pre-split surface
  types.rs            # RegionPatch / Outcome / Media / Opts / Context + skip/tag/report-order
  pcm.rs              # gained_fill, slice_b_extract, splice_into_a, border RMS, floor helpers
  dual_fit.rs         # DualFit* + skip_or_dual_fit / finalize_dual_fit
  bracket.rs          # assemble_bracket_fill + execute_bracket_fill / execute_bracket_output
  execute.rs          # execute_region_spec (characterize→execute dispatcher)
  characterize.rs     # characterize_region + skip_region_spec + prepare_region_patch
```

| Module | Owns | Notes |
|--------|------|-------|
| `types.rs` | `RegionPatch`, `RegionPatchOutcome`, `skipped_patch` / `skipped_patch_with_residual`, `record_patch_gap_span`, `RegionPatchMedia` / `Opts` / `Context`, `outcomes_in_report_order`, `region_outcome_gap_tags`, `seam_failure_outcome`, `skip_outcome_from_fields`, `outcome_from_spec` | Leaf types + outcome mapping. **Not** `gained_fill`, **not** `skip_region_spec` |
| `pcm.rs` | `gained_fill`, `a_gap_floor_db`, `b_mapped_start_frame`, `slice_b_extract`, `b_extract_frames`, `compute_a_border_rms`, `splice_into_a` | PCM primitives. `gained_fill` is RegionPatch math but is the splice/measure source of truth — keep next to `splice_into_a` |
| `dual_fit.rs` | `DualFitRepairInput` / `DualFitInputSpec`, `build_dual_fit_input`, `dual_fit_eligible`, `measure_dual_fit_residual_verdict`, `skip_or_dual_fit`, `DualFitDecision` / `DualFitRescue`, `finalize_dual_fit`, `dual_fit_skipped` | Depends on `types` + `pcm` (`a_gap_floor_db`, `b_mapped_start_frame`). Calls `seam_failure_outcome` |
| `bracket.rs` | `BracketFillAssembly` / `BracketFill`, `assemble_bracket_fill`, `ExecuteBracketFillCtx` / `execute_bracket_fill`, `ExecuteBracketOutputCtx` / `execute_bracket_output` | Depends on `types` + `pcm` (`slice_b_extract` is called by *callers* of `execute_bracket_fill`, not owned here) |
| `execute.rs` | `execute_region_spec` | Thin match on `spec.verdict`. Bracket arm calls `execute_bracket_fill` + `execute_bracket_output`; SilenceSplice copies PCM. **Must not import `characterize`** |
| `characterize.rs` | `anchor_search_prior_for_gap`, `skip_region_spec`, `characterize_region`, `prepare_region_patch`, test-only `characterize_all_regions` | The long pipeline. `prepare_region_patch` is the shim that calls `execute_region_spec` — this is the only `characterize → execute` edge |
| `mod.rs` | `mod` + `pub(super) use` of the pre-split surface | No production code; no unit tests unless a cross-cutting test has nowhere else to live |

### Contested / easy-to-misplace helpers

| Symbol | Owner | Why |
|--------|-------|-----|
| `gained_fill` | **`pcm`** | Single source of truth for "what gets written"; used by `patch_audio/mod.rs` splice + fill-level. Not a type. |
| `skip_region_spec` | **`characterize`** | Builds a `GapRepairSpec` skip from live characterize locals (`region`, `refined`, `b_extract`). Dual-fit's `finalize_dual_fit` also builds specs, but from a `DualFitDecision` — different constructor |
| `skip_outcome_from_fields` / `outcome_from_spec` | **`types`** | Spec/fields → `RegionPatchOutcome`; no PCM. Used by `patch_audio/mod.rs` preview + execute loop |
| `seam_failure_outcome` | **`types`** | Maps `SeamGateFailure` → skip outcome. Dual-fit calls it; it must not live in `dual_fit` (types must stay a leaf) |
| `anchor_search_prior_for_gap` | **`characterize`** | Only `characterize_region` calls it |
| `prepare_region_patch` | **`characterize`** | Shim over characterize + execute. Anchored retry is the only production caller (`anchor_retry.rs`) |
| `b_extract_frames` | **`pcm`** | Must stay next to `slice_b_extract` (S1: secs→frames rounding cannot drift from the slice clamp) |
| `fill_geometry_echo_*` test | **`geometry.rs`** (or `patch_audio/mod.rs` tests) | Asserts `fill_geometry_for` + `FillWindowFrames`, not region logic. Relocate when `dual_fit_test_request` is still reachable, or duplicate a tiny request builder |
| M-CFG `deref_reads_*` / `into_request_defaults_*` | **stay with `dual_fit_test_request` in `dual_fit.rs` tests** | Original P1 deviation; moving them to `request.rs` now still duplicates the ScanAlignment scaffolding. Follow-up, not this plan |

### Internal visibility

Rust `pub(super)` in a nested file is visible only to `region/mod.rs`, **not** to
sibling files and **not** to `patch_audio`. Use:

- **Intra-folder items that were `fn` / `struct` private** in the monolith:
  `pub(in crate::application::patch_audio::region)` so siblings can `use super::pcm::slice_b_extract`
  without widening to the crate or to `patch_audio`.
- **Pre-split `pub(super)` surface** (consumed by `patch_audio/mod.rs` + `anchor_retry.rs`):
  keep that visibility on the **facade re-export** (`pub(super) use types::RegionPatch;` etc.).
  The leaf may use `pub(in crate::application::patch_audio::region)` and the facade lifts it.
- Do **not** `pub(crate)` a helper that was private. Do **not** `pub(super)`-re-export a
  formerly-private helper from `region/mod.rs` (that would leak it into `patch_audio`).

Pre-split `pub(super)` surface the facade must keep (names unchanged):

`RegionPatch`, `gained_fill`, `RegionPatchOutcome`, `skipped_patch`,
`record_patch_gap_span`, `RegionPatchMedia`, `RegionPatchOpts`, `RegionPatchContext`,
`outcomes_in_report_order`, `region_outcome_gap_tags`, `skip_outcome_from_fields`,
`outcome_from_spec`, `execute_region_spec`, `characterize_region`,
`prepare_region_patch`, `splice_into_a`.

### Dependency direction (must hold)

```text
types          (leaf)
pcm       ←  types
dual_fit  ←  types, pcm
bracket   ←  types, pcm
execute   ←  types, pcm, bracket
characterize ← types, pcm, dual_fit, bracket, execute

patch_audio/{mod,anchor_retry}  ←  region (facade only)
```

**Cycle guards**

- `execute` must not import `characterize`.
- `bracket` must not import `dual_fit` (SilenceSplice PCM is already on the spec).
- `dual_fit` must not import `bracket`.
- `types` / `pcm` must not import `dual_fit` / `bracket` / `execute` / `characterize`.
- `characterize` *may* import `execute` — that is the `prepare_region_patch` shim.
  Characterize's debug_assert reconstruction calls `execute_bracket_fill` directly
  (already does today), not `execute_region_spec`.

Do **not** let `log` or `geometry` grow a `region::` dependency they do not already have.

---

## 4. Phase ledger

Extract **one cohesion slice per phase**. Do not combine with behavior PRs.

| Phase | Slice | Status |
|-------|-------|--------|
| **P0** | `git mv` `region.rs` → `region/mod.rs` (directory rename; `mod region;` in `patch_audio/mod.rs` unchanged) | not started |
| **P1** | `types.rs` (outcomes / media / opts / context / report-order / spec→outcome) | not started |
| **P2** | `pcm.rs` (gained_fill / slice / splice / RMS / floor) | not started |
| **P3** | `dual_fit.rs` | not started |
| **P4** | `bracket.rs` | not started |
| **P5** | `execute.rs` (`execute_region_spec`) | not started |
| **P6** | Remainder → `characterize.rs` + thin `mod.rs` facade | not started |

**Suggested order rationale.** Types first (no sibling deps). PCM next (leaf helpers
`splice_into_a` / `gained_fill` already consumed outside the directory). Dual-fit and
bracket are independent of each other and of execute. Execute is a thin dispatcher that
needs bracket. Characterize last among extracts (depends on everyone). Facade last —
same “dependency-forward, then thin mod” pattern as the 2026-07-24 `patch_audio` split.

**P0 notes.** After `git mv`, `mod region;` still resolves. There is no leftover
`region.rs` beside the folder (Rust forbids both). Do not extract in the same commit
as the rename if the diff would mix “path change” with “symbol moves”; two commits
in one PR is fine.

**P1 notes.** Leave `gained_fill` and `skip_region_spec` behind. Take
`skip_outcome_from_fields` / `outcome_from_spec` / `seam_failure_outcome` even though
they currently sit below dual-fit finalize. Tests that move: `outcomes_in_report_order_*`,
`not_applied_overrides_the_patched_region_outcome`.

**P2 notes.** `gained_fill` becomes `pub(in …region)` in `pcm.rs` and
`pub(super) use pcm::gained_fill` on the facade (`patch_audio/mod.rs` calls
`region::gained_fill`). Tests: `b_extract_slice_clamps_*`, `splice_bails_*`,
`splice_shorter_fill_*`.

**P3 notes.** `skip_or_dual_fit` needs `seam_failure_outcome` from types. Tests:
`dual_fit_eligible_*`, `measure_dual_fit_residual_verdict_*`, plus the M-CFG pair
and `dual_fit_test_request` helper. `fit_fill_trims_tail_*` / `fit_fill_zero_pads_*`
test `domain::fit_fill_to_gap_frames` — they currently live here only as neighbors;
move them with **bracket** (P4), which is the production caller.

**P4 notes.** Do not move `execute_region_spec`. Characterize's `debug_assert_eq!(execute_bracket_fill(…), b_fill)`
keeps compiling via `super::bracket::execute_bracket_fill` (intra-folder vis).

**P5 notes.** `execute_region_spec` only. Verify it imports `bracket` + `pcm::slice_b_extract`
and does **not** import `characterize`.

**P6 notes.** Everything left in `mod.rs` moves to `characterize.rs` except `mod` /
`pub(super) use`. `mod.rs` should have no `#[cfg(test)]` unless
`fill_geometry_echo_*` is still homeless — prefer relocating that test to
`geometry.rs` in this phase (it already calls `super::super::geometry::…`).

---

## 5. Verification (per phase / final gate)

- `cargo build -p clip-sync-repair --all-targets`
- `cargo clippy -p clip-sync-repair --all-targets`
- `.\scripts\test-tier.ps1 -Tier pr-repair`

After **P2**, confirm `patch_audio/mod.rs` still compiles against `region::gained_fill`
and `region::splice_into_a` with no import edits.

After **P5**, confirm `anchor_retry.rs` still compiles against
`region::{prepare_region_patch, RegionPatch, …}` — `prepare_region_patch` may still
be in `mod.rs` until P6.

After **P6**, confirm `-Tier pr-repair` still green including
`patch_audio_integration` / `anchor_seam_oracle` / dual-fit oracle paths that
exercise `PatchAudio::execute` and the anchored-retry `prepare_region_patch` shim.

---

## 6. Success criteria

- [ ] `application/patch_audio/region/` is a directory (`mod` + six submodules); no
      leftover sibling `region.rs`.
- [ ] `region/mod.rs` is a thin facade (re-exports only; no leftover free-function
      clusters).
- [ ] Public / `pub(super)` import paths unchanged (`region::{characterize_region,
      execute_region_spec, prepare_region_patch, splice_into_a, gained_fill, …}`).
- [ ] DAG in §3 holds (no `execute → characterize`; no `bracket ↔ dual_fit`; no
      `types`/`pcm` depending on pipeline modules).
- [ ] Contested helpers match the §3 table.
- [ ] Intra-folder vis uses `pub(in crate::application::patch_audio::region)` for
      formerly-private items; no crate-wide widening.
- [ ] Unit tests colocated per submodule.
- [ ] Byte-preserving: no intentional behavior / string / threshold changes.

---

## Related

- [archive/TEMP-patch-audio-module-split-plan.md](archive/TEMP-patch-audio-module-split-plan.md) — parent split (done); this is the deferred peel
- [archive/TEMP-region-characterization-collapse-plan.md](archive/TEMP-region-characterization-collapse-plan.md) — characterize→execute boundary (done; do not reopen)
- [archive/TEMP-patch-audio-bracket-fill-elimination-plan.md](archive/TEMP-patch-audio-bracket-fill-elimination-plan.md) — why `execute_bracket_fill` is a reconstruction primitive
- `crates/clip-sync-repair/src/application/patch_audio/region.rs` — current monolith
- Sibling [TEMP-gap-fingerprint-measure-module-split-plan.md](archive/TEMP-gap-fingerprint-measure-module-split-plan.md) (done / archived)
