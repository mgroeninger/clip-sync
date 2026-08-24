# `patch_audio/region.rs` folder split — plan

Status: **ready to implement / not started**. Maintainability-only peel of the remaining
`region.rs` monolith into `patch_audio/region/` (folder + submodules). Byte-preserving
moves; never bundled into a behavior-change PR.

Refreshed 2026-08-24 against `region.rs` (3633 lines, split not started).

**Code-verified 2026-08-24.** Locus table, the §3 `pub(super)` surface list (exactly the
16 top-level `pub(super)` items in the file), and every DAG edge were checked against
`region.rs`. Confirmed: `characterize_region` never calls `execute_region_spec` (only
`prepare_region_patch` does, ~L2362); `skip_or_dual_fit` / `finalize_dual_fit` reach
bracket and characterize **only in doc comments**, so the cycle guards are satisfiable
as written; `characterize_all_regions` is genuinely `#[cfg(test)]` (~L2324);
`assemble_bracket_fill` really is the production caller of `fit_fill_to_gap_frames`
(~L990). The review added the field-visibility rule and the import/doc-link rules below —
those were the gaps, not the layout.

This is the **deferred dual-fit / bracket peel** the 2026-07-24
[`TEMP-patch-audio-module-split-plan.md`](archive/TEMP-patch-audio-module-split-plan.md)
explicitly left out of its success gate (`region.rs` was allowed to land near ~2 kloc).
The file is now **3633 lines** (~2510 production + ~1120 colocated tests).

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
| **Types / outcomes** | `RegionPatch`…`seam_failure_outcome` (~L49–423); `skip_region_spec` / `skip_outcome_from_fields` / `outcome_from_spec` (~L1248–1361) | Patch/outcome/media/opts/context + report-order join + spec→outcome. `anchor_search_prior_for_gap` (~L153) sits in this block but is characterize-owned |
| **PCM helpers** | `gained_fill` (~L69); `a_gap_floor_db` / `b_mapped_start_frame` (~L426–461); `slice_b_extract`…`splice_into_a` (~L2389–2536) | Slice / gain / splice / border RMS |
| **Dual-fit** | `DualFitRepairInput`…`skip_or_dual_fit` (~L463–900), including `registration_shoulder_residual_ms` / `select_registration_residual_ms` (~L652–676); `DualFitDecision` / `finalize_dual_fit` (~L1180–1241); `dual_fit_skipped` (~L1363) | Gate-fail rescue. `dual_fit_skipped` sits *after* the spec→outcome helpers, not with `DualFitDecision` |
| **Bracket** | `BracketFillAssembly`…`execute_bracket_output` (~L902–1173) | Assemble fill PCM + wrap as `RegionPatch`. `ExecuteBracketOutputCtx` carries `registration_residual_ms` (field on the existing ctx, not a new type) |
| **Execute** | `execute_region_spec` (~L1382–1523) | Spec → PCM. Bracket arm re-derives fill; SilenceSplice copies dual-fit PCM |
| **Characterize** | `characterize_region` (~L1534–2314, ~780 lines); test-only `characterize_all_regions` (~L2325–2349); `prepare_region_patch` shim (~L2354–2378) | Geometry → gate → dual-fit/reconciliation → `GapRepairSpec`. Bracket-path `registration_residual_ms` is computed here via the dual-fit helpers |

Locus table is orientation only. Extract by the §3 ownership table + DAG, not by
contiguous line ranges. Interleaved symbols (`gained_fill` next to `RegionPatch`;
`anchor_search_prior_for_gap` in the types block; `skip_region_spec` between dual-fit
finalize and execute; `dual_fit_skipped` after spec→outcome) are called out in
§3 contested.

Natural seams already exist as free functions and type clusters. Split along those.
Do **not** redesign characterize / dual-fit / bracket / execute flow, and do **not**
decompose `characterize_region` itself — it stays one function in `characterize.rs`
even if that file lands near ~1.1 kloc.

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
  types.rs            # RegionPatch / Outcome / Media / Opts / Context + skip/tag/report-order + skip_region_spec
  pcm.rs              # gained_fill, slice_b_extract, splice_into_a, border RMS, floor helpers
  dual_fit.rs         # DualFit* + skip_or_dual_fit / finalize_dual_fit + registration-residual helpers
  bracket.rs          # assemble_bracket_fill + execute_bracket_fill / execute_bracket_output
  execute.rs          # execute_region_spec (characterize→execute dispatcher)
  characterize.rs     # characterize_region + prepare_region_patch
```

| Module | Owns | Notes |
|--------|------|-------|
| `types.rs` | `RegionPatch`, `RegionPatchOutcome`, `skipped_patch` / `skipped_patch_with_residual`, `record_patch_gap_span`, `RegionPatchMedia` / `Opts` / `Context`, `outcomes_in_report_order`, `region_outcome_gap_tags`, `seam_failure_outcome`, `skip_region_spec`, `skip_outcome_from_fields`, `outcome_from_spec` | Leaf types + spec/outcome mapping. **Not** `gained_fill`. `RegionPatchOutcome::Patched` carries `registration_residual_ms` (field, not a helper) |
| `pcm.rs` | `gained_fill`, `a_gap_floor_db`, `b_mapped_start_frame`, `slice_b_extract`, `b_extract_frames`, `compute_a_border_rms`, `splice_into_a` | PCM primitives. `gained_fill` is RegionPatch math but is the splice/measure source of truth — keep next to `splice_into_a` |
| `dual_fit.rs` | `DualFitRepairInput` / `DualFitInputSpec`, `build_dual_fit_input`, `dual_fit_eligible`, `measure_dual_fit_residual_verdict`, `registration_shoulder_residual_ms`, `select_registration_residual_ms`, `skip_or_dual_fit`, `DualFitDecision` / `DualFitRescue`, `finalize_dual_fit`, `dual_fit_skipped` | Depends on `types` + `pcm` (`a_gap_floor_db`, `b_mapped_start_frame`). Calls `seam_failure_outcome` and `skip_region_spec` |
| `bracket.rs` | `BracketFillAssembly` / `BracketFill`, `assemble_bracket_fill`, `ExecuteBracketFillCtx` / `execute_bracket_fill`, `ExecuteBracketOutputCtx` / `execute_bracket_output` | Depends on `types` + `pcm` (`slice_b_extract` is called by *callers* of `execute_bracket_fill`, not owned here) |
| `execute.rs` | `execute_region_spec` | Thin match on `spec.verdict`. Bracket arm calls `execute_bracket_fill` + `execute_bracket_output`; SilenceSplice copies PCM. **Must not import `characterize`** |
| `characterize.rs` | `anchor_search_prior_for_gap`, `characterize_region`, `prepare_region_patch`, test-only `characterize_all_regions` | The long pipeline. `prepare_region_patch` is the shim that calls `execute_region_spec` — this is the only `characterize → execute` edge |
| `mod.rs` | `mod` + `pub(super) use` of the pre-split surface | No production code; no unit tests unless a cross-cutting test has nowhere else to live |

### Contested / easy-to-misplace helpers

| Symbol | Owner | Why |
|--------|-------|-----|
| `gained_fill` | **`pcm`** | Single source of truth for "what gets written"; used by `patch_audio/mod.rs` splice + fill-level. Not a type. |
| `skip_region_spec` | **`types`** | Spec constructor from (reason, residual, region geometry); no PCM. `finalize_dual_fit` *and* `characterize_region` call it. Putting it in characterize would be `dual_fit → characterize` (cycle). Take it in P1 with the other spec/outcome helpers that already sit in this cluster |
| `skip_outcome_from_fields` / `outcome_from_spec` | **`types`** | Spec/fields → `RegionPatchOutcome`; no PCM. Used by `patch_audio/mod.rs` preview + execute loop |
| `seam_failure_outcome` | **`types`** | Maps `SeamGateFailure` → skip outcome. Dual-fit calls it; it must not live in `dual_fit` (types must stay a leaf) |
| `registration_shoulder_residual_ms` / `select_registration_residual_ms` | **`dual_fit`** | Pure residual math added with [archive/TEMP-registration-residual-plan.md](archive/TEMP-registration-residual-plan.md). Sit in the dual-fit cluster; `skip_or_dual_fit` is a caller. Characterize's bracket path also calls them (DAG-legal: characterize already depends on dual_fit). Helper unit tests go with dual_fit; the fixture that drives `characterize_all_regions` stays with characterize |
| `anchor_search_prior_for_gap` | **`characterize`** | Only `characterize_region` calls it. Physically sits in the types block today (~L153) |
| `prepare_region_patch` | **`characterize`** | Shim over characterize + execute. Anchored retry is the only production caller (`anchor_retry.rs`) |
| `b_extract_frames` | **`pcm`** | Must stay next to `slice_b_extract` (S1: secs→frames rounding cannot drift from the slice clamp) |
| `fill_geometry_echo_*` test | **`geometry.rs`** (or `patch_audio/mod.rs` tests) | Asserts `fill_geometry_for` (`mod.rs`) + `FillWindowFrames`, not region logic. Relocate when `dual_fit_test_request` is still reachable, or duplicate a tiny request builder |
| M-CFG `deref_reads_*` / `into_request_defaults_*` | **stay with `dual_fit_test_request` in `dual_fit.rs` tests** | Original P1 deviation; moving them to `request.rs` now still duplicates the ScanAlignment scaffolding. Follow-up, not this plan |

### Internal visibility

Rust restricted visibility extends to **descendants** of the named module, so
`pub(super)` written in `region/types.rs` means `pub(in …::region)` — visible to
`region/mod.rs` *and* to every sibling submodule, but **not** to `patch_audio`. The
two forms are equivalent inside the folder; prefer the explicit `pub(in …)` spelling so
the intended ceiling is readable at the definition. Use:

- **Intra-folder items that were `fn` / `struct` private** in the monolith:
  `pub(in crate::application::patch_audio::region)` so siblings can `use super::pcm::slice_b_extract`
  without widening to the crate or to `patch_audio`.
- **Pre-split `pub(super)` surface** (consumed by `patch_audio/mod.rs` + `anchor_retry.rs`):
  keep that visibility on the **facade re-export** (`pub(super) use types::RegionPatch;` etc.).
  The leaf may use `pub(in crate::application::patch_audio::region)` and the facade lifts it.
- Do **not** `pub(crate)` a helper that was private. Do **not** `pub(super)`-re-export a
  formerly-private helper from `region/mod.rs` (that would leak it into `patch_audio`).

**Struct fields are the hazard — a facade re-export does not lift field visibility.**
`pub(super) use types::RegionPatch;` re-exports the *type*; each field keeps whatever
ceiling it was written with. Four exported structs have `pub(super)` **fields** that
`patch_audio` constructs or reads by name, so on the move into `region/types.rs` those
fields must be widened to `pub(in crate::application::patch_audio)` — plain `pub(super)`
in the leaf would cap them at `region` and break the callers:

| Struct | Fields | External use |
|--------|--------|--------------|
| `RegionPatch` | `b_samples`, `gain`, `a_start_frame`, `a_end_frame`, `crossfade_secs` | read at `patch_audio/mod.rs` ~L500–501, ~L523–525 |
| `RegionPatchMedia<'a>` | `b_samples_full`, `a_pcm` | struct literals at `mod.rs` ~L331, ~L399, ~L457; `&RegionPatchMedia<'_>` param in `anchor_retry.rs` ~L155 |
| `RegionPatchOpts<'a>` | `anchored_retry_pass`, `patch_anchors` | struct literals at `mod.rs` ~L338, `anchor_retry.rs` ~L205 |
| `RegionPatchContext` | `channels`, `sample_rate`, `max_refine_frames`, `global_a_rms`, `silence_peak_fraction` | struct literal at `mod.rs` ~L294 |

This is the one case where widening past `region` is correct and required; it restores
the pre-split ceiling rather than raising it. It does **not** license `pub(crate)`.

`RegionPatchOutcome`'s variant fields need no action — enum variant fields inherit the
enum's visibility.

### Import rewrites inside the folder

The monolith's head carries `use super::{border_frames_from_secs, FillWindowFrames,
PatchAudioRequest};` (~L44) plus `use super::geometry::…` / `use super::log::…`. Inside
`region/`, `super` now resolves to `region`, not `patch_audio`. Every submodule that
inherits one of those imports must rewrite it to `super::super::…` (or the absolute
`crate::application::patch_audio::…`). Mechanical and compiler-caught, but expect it in
**every** phase, not just P6's `FillWindowFrames`.

### Rustdoc intra-doc links

`region.rs` carries ~20 rustdoc intra-doc links (``[`assemble_bracket_fill`]``,
``[`slice_b_extract`]``, ``[`execute_region_spec`]``, ``[`characterize_region`]``,
``[`finalize_dual_fit`]``, …). Many cross the new module boundaries and will need path
qualification (``[`super::pcm::slice_b_extract`]``) or reduction to plain backticks.
Neither workspace nor crate `Cargo.toml` has a `[lints]` table, so these degrade to
`cargo doc` warnings, not build or clippy failures — which is exactly why they will be
missed unless `cargo doc` is run (see §5).

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
- `dual_fit` must not import `characterize` (`finalize_dual_fit` reaches skip construction via `types::skip_region_spec`, not via characterize).
- `types` / `pcm` must not import `dual_fit` / `bracket` / `execute` / `characterize`.
- `characterize` *may* import `execute` — that is the `prepare_region_patch` shim.
  Characterize's debug_assert reconstruction calls `execute_bracket_fill` directly
  (already does today, ~L2163), not `execute_region_spec`.
  Characterize *may* import `dual_fit` for the registration-residual helpers on the bracket path.

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

**P1 notes.** Leave `gained_fill` behind. Take `skip_region_spec` /
`skip_outcome_from_fields` / `outcome_from_spec` / `seam_failure_outcome` even though
they currently sit below dual-fit finalize — `finalize_dual_fit` (P3) calls
`skip_region_spec`, so it cannot wait for characterize. Tests that move:
`outcomes_in_report_order_*`, `not_applied_overrides_the_patched_region_outcome`.

This is the phase the **struct-field rule** (§3) lands in: widen the fields of
`RegionPatch` / `RegionPatchMedia` / `RegionPatchOpts` / `RegionPatchContext` to
`pub(in crate::application::patch_audio)` as they move. Getting this wrong is not a
subtle failure — `patch_audio/mod.rs` ~L294/331/338/399/457/500–525 and
`anchor_retry.rs` ~L155/205 stop compiling immediately. `skipped_patch_with_residual`
stays private to `types.rs` (only `types` callers).

**P2 notes.** `gained_fill` becomes `pub(in …region)` in `pcm.rs` and
`pub(super) use pcm::gained_fill` on the facade (`patch_audio/mod.rs` calls
`region::gained_fill`). Tests: `b_extract_slice_clamps_*`, `splice_bails_*`,
`splice_shorter_fill_*`.

**P3 notes.** `skip_or_dual_fit` needs `seam_failure_outcome` from types;
`finalize_dual_fit` needs `skip_region_spec` from types. Tests:
`dual_fit_eligible_*`, `measure_dual_fit_residual_verdict_*`,
`registration_shoulder_residual_ms_*`, `select_registration_residual_ms_*`,
`dual_fit_registration_residual_*`, plus the M-CFG pair and `dual_fit_test_request`
helper. `fit_fill_trims_tail_*` / `fit_fill_zero_pads_*` test
`domain::fit_fill_to_gap_frames` — they currently live here only as neighbors;
move them with **bracket** (P4), which is the production caller. Do **not** move
`bracket_registration_residual_*` (it drives `characterize_all_regions`).

**P4 notes.** Do not move `execute_region_spec`. Characterize's `debug_assert_eq!(execute_bracket_fill(…), b_fill)`
keeps compiling via `super::bracket::execute_bracket_fill` (intra-folder vis).

**P5 notes.** `execute_region_spec` only. Verify it imports `bracket` + `pcm::slice_b_extract`
and does **not** import `characterize`.

**P6 notes.** Everything left in `mod.rs` moves to `characterize.rs` except `mod` /
`pub(super) use`. Tests that move with it: `characterize_all_regions_yields_*`,
`bracket_registration_residual_*`. `mod.rs` should have no `#[cfg(test)]` unless
`fill_geometry_echo_*` is still homeless — prefer relocating that test to
`geometry.rs` or `patch_audio/mod.rs` tests in this phase (it asserts
`fill_geometry_for` in `mod.rs` plus `super::super::geometry::FillWindowFrames`).

---

## 5. Verification (per phase / final gate)

- `cargo build -p clip-sync-repair --all-targets`
- `cargo clippy -p clip-sync-repair --all-targets`
- `.\scripts\test-tier.ps1 -Tier pr-repair`
- `cargo doc -p clip-sync-repair --no-deps` — **new for this plan**, and the only gate
  that catches the intra-doc links broken by the module boundaries (§3). There is no
  `[lints]` table in either Cargo.toml, so broken links are warnings that build and
  clippy will not surface. Compare the warning count against the pre-P0 baseline;
  capture that baseline before starting P0.

After **P1**, confirm the widened struct fields did their job: `patch_audio/mod.rs`
still constructs `RegionPatchContext` / `RegionPatchMedia` / `RegionPatchOpts` by
literal and reads `patch.a_start_frame` / `.a_end_frame` / `.crossfade_secs` with no
edits, and `anchor_retry.rs` still takes `&RegionPatchMedia<'_>`.

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
- [ ] Struct **fields** on the four exported types are `pub(in crate::application::patch_audio)`
      — the pre-split ceiling, restored, not raised. No `pub` / `pub(crate)` fields.
- [ ] `cargo doc -p clip-sync-repair --no-deps` emits no more warnings than the
      pre-P0 baseline (no intra-doc links left dangling across the new boundaries).
- [ ] Unit tests colocated per submodule.
- [ ] Byte-preserving: no intentional behavior / string / threshold changes.

---

## Related

- [archive/TEMP-patch-audio-module-split-plan.md](archive/TEMP-patch-audio-module-split-plan.md) — parent split (done); this is the deferred peel
- [archive/TEMP-region-characterization-collapse-plan.md](archive/TEMP-region-characterization-collapse-plan.md) — characterize→execute boundary (done; do not reopen)
- [archive/TEMP-patch-audio-bracket-fill-elimination-plan.md](archive/TEMP-patch-audio-bracket-fill-elimination-plan.md) — why `execute_bracket_fill` is a reconstruction primitive
- [archive/TEMP-registration-residual-plan.md](archive/TEMP-registration-residual-plan.md) — source of `registration_shoulder_residual_ms` / `select_registration_residual_ms` (done; do not reopen)
- `crates/clip-sync-repair/src/application/patch_audio/region.rs` — current monolith (3633 lines, 2026-08-24)
- Sibling [TEMP-gap-fingerprint-measure-module-split-plan.md](archive/TEMP-gap-fingerprint-measure-module-split-plan.md) (done / archived)
