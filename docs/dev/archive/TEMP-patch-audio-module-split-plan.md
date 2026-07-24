# `patch_audio.rs` module split — plan

> **Archived 2026-07-24.** Planned M-MOD `patch_audio` bite; shipped. Record only.

Status: **done (P1–P6, 2026-07-24)** — all phases landed; tree below is realized:

```text
application/patch_audio/
  mod.rs              # thin facade + PatchAudio::execute orchestration
  request.rs          # PatchAudioResult / Request / PatchRequestSettings
  decode.rs           # DecodedAb + decode_ab (shared with fingerprint dump)
  geometry.rs         # frame helpers + SeamGateDerived::from_repair
  log.rs              # GapFill*Log + format_* / verbose log helpers (+ gap_key leaf)
  region.rs           # RegionPatch/Outcome/media-opts + characterize + dual-fit + bracket + splice
  anchor_retry.rs     # anchored-retry pass + helpers (imports media/opts from region)
```

Callers keep `crate::application::patch_audio::{…}` (and the crate-level
`application::{PatchAudio, PatchAudioRequest, …}` re-exports); no import sweep.
Unit tests stay in each submodule’s `#[cfg(test)]` block (not `tests/` / `*_test.rs`).

**M-MOD context.** This was the **optional `patch_audio` slice** of
[M-MOD](../TEMP-rust-review-findings.md#m-mod-oversized-modules--closed). Policies,
harness corpus, and production `gap_fingerprint` were already done when this
landed; with P1–P6 complete, the planned M-MOD splits are closed. `align_videos`
remains **out of scope** (test-inflated orchestrator; already decomposed via
sibling application modules — decline or defer without a sibling plan unless
that changes).

**Companion history (do not re-open).** Config collapse is closed
([TEMP-repair-config-bundles-plan.md](TEMP-repair-config-bundles-plan.md)
— M-CFG). This plan is **M-MOD maintainability only**: byte-preserving moves,
never bundled into behavior-change PRs. Do **not** retune gates, dual-fit, or
anchor policy while splitting.

Companions: [TEMP-policies-module-split-plan.md](TEMP-policies-module-split-plan.md)
(ground-rules template),
[TEMP-gap-fingerprint-module-split-plan.md](TEMP-gap-fingerprint-module-split-plan.md),
[TEMP-rust-review-findings.md](../TEMP-rust-review-findings.md) **M-MOD**,
[gap-fill-modes.md](../../gap-fill-modes.md), [pipeline.md](../../pipeline.md).

---

## 1. Problem (resolved)

Pre-split, `application/patch_audio.rs` (~3.6 kloc; ~3.0 k production + ~0.6 k colocated
tests) was a single-file repair orchestrator mixing several cohesion slices.

**Locus table is approximate orientation only.** Symbols today are interleaved
(e.g. `RegionPatch` sits next to request types; `skipped_patch*` sits between
decode and anchor helpers; `gap_key` / `fill_offset_mode_label` / media opts sit
near the anchor block but are not anchor-owned). Extract by the §3 ownership
table + DAG, not by contiguous line ranges.

| Concern | Approx. locus today | Role |
|---------|---------------------|------|
| **Request** | ~L52–216 | `PatchAudioResult` / `PatchAudioRequest` / `PatchRequestSettings` (+ `Deref` / `into_request`) — stop before `RegionPatch` |
| **Orchestration** | `PatchAudio` + `execute` ~L226–505 | Fill plan → decode → characterize loop → optional anchored retry → splice → summary |
| **Decode** | ~L532–626 | `DecodedAb` / `decode_ab` (also used by fingerprint composition dump) |
| **Anchored retry** | ~L683–932 (policy → `run_anchored_retry_pass`) | Retry pass over failed/marginal gaps — **not** the media/opts structs or `gap_key` that happen to sit nearby |
| **Logging** | ~L934–1118 | `GapFill*Log`, `format_*`, verbose progress / skip / marginal helpers (+ leaf `gap_key` / `fill_offset_mode_label`) |
| **Region repair** | `RegionPatch` ~L218; outcome + `skipped_patch*` ~L508–668; body ~L1120–2952 + splice ~L3051–3160 | `RegionPatch` / `RegionPatchOutcome`, media/opts, characterize → dual-fit / bracket execute → PCM helpers |
| **Geometry** | ~L2954–3050 | `repair_patch_config_view`, `SeamGateDerived::from_repair`, `border_frames_from_secs`, `seam_gate_frames_for`, `correlate_frames_for_gap` (consumed by `patch_region`) |

Unlike policies / fingerprint, this is **one use-case with peelable helpers**, not
three independent products. The split still pays off because production kloc is
real (~3.0 k) and external crates already depend on a few leaf helpers
(`decode_ab`, frame helpers, request types). Keep `region.rs` as **one** cohesion
slice even if it lands near ~2 kloc (same acceptance bar as fingerprint
`measure.rs`); a further dual-fit / bracket peel is **not** this plan’s success
gate.

Natural seams already exist as free functions and type clusters. Split along
those; do not redesign characterize / dual-fit / anchor flow.

## 2. Non-goals

- **Renaming the public path** — keep `crate::application::patch_audio::*` via
  re-exports; keep `application::{PatchAudio, PatchAudioRequest,
  PatchAudioResult, PatchRequestSettings}` crate re-exports unchanged.
- **Changing behavior** — pure move/split only (same plan/decode/characterize/
  retry/splice outcomes, same verbose strings, same frame math).
- **Merging with `patch_region.rs`** — gate/structure matching stays in
  `patch_region`; only relocate the `SeamGateDerived::from_repair` *impl* that
  already lives in `patch_audio`.
- **M-MOD siblings** — `align_videos` split / repair `lib.rs` curation are
  separate (or declined).
- **Reopening M-CFG** — do not reshape `PatchRequestSettings` /
  `PatchAudioRequest` (no new bundles, no `DerefMut`).
- **Perf / algorithm work** — no gate retunes, no dual-fit predicate changes, no
  new fields.
- **Separating unit tests from production code** — do **not** move `#[cfg(test)]`
  into `tests/` or `*_test.rs`; keep tests at the bottom of the same `.rs` file
  that owns the logic (split only when a test clearly belongs with one
  submodule). Cross-cutting `execute`-level tests may stay on the facade.

## 3. Final layout

| Module | Owns | Notes |
|--------|------|-------|
| `request.rs` | `PatchAudioResult`, `PatchAudioRequest`, `PatchRequestSettings`, `Deref`, `into_request` | Leaf types only — **not** `RegionPatch` (even though it sits immediately after settings today). M-CFG guards (`deref_reads_*`, `into_request_defaults_*`) move here |
| `decode.rs` | `DecodedAb`, `decode_ab` | `pub(crate)`; `composition.rs` keeps `patch_audio::decode_ab` |
| `geometry.rs` | `repair_patch_config_view`, `impl SeamGateDerived::from_repair`, `border_frames_from_secs`, `seam_gate_frames_for`, `correlate_frames_for_gap` | `pub(crate)` helpers; `patch_region` paths stay `patch_audio::…` via facade. Intra-crate edge `geometry` → `patch_region` (for the `impl`) + `patch_region` → facade frame helpers is **pre-existing and allowed** |
| `log.rs` | `gap_key`, `fill_offset_mode_label`, `GapFillPlanLog`, `GapFillResultLog`, `format_gap_fill_*`, `format_skip_gap_fill_log`, verbose `log_gap_fill_*` / `log_skip_gap_fill` / `log_marginal_gap_fill` / `MarginalGapFillLog`, `log_gap_tags_verbose` | Formatters + leaf key/label helpers used by region + orchestrator; format-line unit tests move here. **Not** `region_outcome_gap_tags` |
| `region.rs` | `RegionPatch`, `RegionPatchOutcome`, `RegionPatchMedia`, `RegionPatchOpts`, `RegionPatchContext`, `skipped_patch` / `skipped_patch_with_residual`, `record_patch_gap_span`, `outcomes_in_report_order`, `region_outcome_gap_tags`, `seam_failure_outcome`, dual-fit (`skip_or_dual_fit`, …), bracket assemble/execute, `characterize_*`, `prepare_region_patch`, `execute_region_spec`, `slice_b_segment` / `compute_a_border_rms` / `splice_into_a` / floor helpers | Largest slice; depends on request + geometry + log. Media/opts are region API even though they currently sit next to anchor helpers |
| `anchor_retry.rs` | `patch_anchor_policy`, `anchor_search_prior_for_gap`, `build_patch_anchor_candidates`, `anchored_retry_gap_indices`, `should_apply_*`, `store_anchored_retry_patch`, `AnchoredRetryState`, `run_anchored_retry_pass` | Calls `prepare_region_patch`; depends on region + request. Imports `RegionPatchMedia` / `RegionPatchOpts` from `region` — does **not** own them |
| `mod.rs` | `PatchAudio` + `execute` orchestration only; `mod` + `pub use` / `pub(crate) use` re-exports | No unit tests unless an `execute`-only integration test has nowhere else to live |

### Contested / easy-to-misplace helpers (invariants)

Follow this table when a symbol sits in an ambiguous locus today:

| Symbol | Owner | Why |
|--------|-------|-----|
| `gap_key` | **`log`** | Used by `format_skip_gap_fill_log` **and** `outcomes_in_report_order`. DAG forbids `log → region`, so it cannot live in `region`; keep it as a leaf next to the skip formatter. `region` may call `log::gap_key`. |
| `fill_offset_mode_label` | **`log`** | Only feeds plan-format lines. |
| `region_outcome_gap_tags` | **`region`** | Matches on `RegionPatchOutcome` and builds `GapTags` — not a formatter. Do **not** put it in `log`. |
| `log_gap_tags_verbose` | **`log`** | Pure format + `progress.phase_verbose`; takes already-built `GapTags`. |
| `RegionPatchMedia` / `RegionPatchOpts` / `RegionPatchContext` | **`region`** | Characterize / prepare / execute API. Anchor retry borrows them; it does not define them. |
| `skipped_patch` / `skipped_patch_with_residual` / `record_patch_gap_span` | **`region`** | Construct / observe `RegionPatchOutcome`. |
| `SeamGateDerived::from_repair` | **`geometry`** | Next to secs→frames helpers `patch_region` already imports. Type stays in `patch_region`; only the `impl` moves. |

### Internal visibility

- Shared helpers stay `pub(crate)` in the owning submodule.
- Facade re-exports the pre-split **public** API (`PatchAudio`, `PatchAudioRequest`,
  `PatchAudioResult`, `PatchRequestSettings`) and the pre-split **`pub(crate)`**
  surface used outside the directory (`decode_ab`, `DecodedAb`,
  `border_frames_from_secs`, `seam_gate_frames_for`, `correlate_frames_for_gap`,
  `GapFillPlanLog` / `GapFillResultLog` / `format_*` if any external crate uses
  them — today mainly in-crate / tests).
- Do **not** widen visibility beyond what the monolith already exposed.
- `gap_key` / media-opts / `skipped_patch*` stay private or `pub(crate)` only as
  needed inside the `patch_audio/` directory (same as today).

### Dependency direction (must hold)

```text
request  ←  geometry
request  ←  region  ←  anchor_retry
geometry ←  region
log      ←  region          # region may use gap_key / formatters; log must not import region
log      ←  mod (orchestrator)
decode   ←  mod
region   ←  mod
anchor_retry ← mod
```

**Cycle guard.** `region` must not depend on `anchor_retry`. `geometry` must not
depend on `region` / `log`. Prefer keeping `SeamGateDerived::from_repair` in
`geometry` (next to the frame helpers `patch_region` already imports) rather than
pulling `patch_region` types into `region`.

Do **not** let `log` depend on `region` (formatters take plain log structs /
domain types / `GapTags` only). Helpers that need `RegionPatchOutcome`
(`region_outcome_gap_tags`, `skipped_patch*`, `record_patch_gap_span`,
`outcomes_in_report_order`) stay in `region`.

**Note on `geometry` ↔ `patch_region`.** `geometry` imports `SeamGateDerived` from
`patch_region`; `patch_region` imports frame helpers via the `patch_audio` facade.
That intra-crate module edge already exists in the monolith and is fine in Rust;
do not “fix” it by moving `from_repair` into `region` unless a later plan
relocates the type.

## 4. Phase ledger

Extract **one cohesion slice per phase**. Do not combine with behavior PRs.

| Phase | Slice | Status |
|-------|-------|--------|
| **P1** | `request.rs` (result / request / settings) | **Done (2026-07-24)** |
| **P2** | `decode.rs` + `geometry.rs` (leaf helpers; external consumers) | **Done (2026-07-24)** |
| **P3** | `log.rs` (formatters + verbose helpers + their unit tests) | **Done (2026-07-24)** |
| **P4** | `region.rs` (outcomes + characterize + dual-fit + bracket + splice) | **Done (2026-07-24)** |
| **P5** | `anchor_retry.rs` (retry pass; needs `prepare_region_patch`) | **Done (2026-07-24)** |
| **P6** | Thin `mod.rs` — only `PatchAudio::execute` + re-exports; delete monolith `.rs` | **Done (2026-07-24)** |

**Suggested order rationale.** Request first (no sibling deps). Decode + geometry
next (leaves with outside call sites — green `composition` / `patch_region`
compiles early). Log next (format tests move cleanly). Region is the judgment-
heavy body. Anchored retry last among extracts (depends on region’s
`prepare_region_patch`). Facade last — same “dependency-forward, then thin mod”
pattern as policies P1–P5 / fingerprint P1–P3.

**P1 notes.** `git mv` monolith → `patch_audio/mod.rs` first (or extract
`request.rs` beside a still-monolithic `mod.rs` after the directory rename).
Public path must stay stable for harness `patch_audio::{PatchAudio, …}` and
`application::{PatchAudio, …}`. Leave `RegionPatch` in the monolith/`mod.rs`
body for P4 — it is **not** part of the request slice even though it follows
`PatchRequestSettings` in the file today.

**P1 as-landed (2026-07-24).** `git mv` → `patch_audio/mod.rs`; the three request
types + `Deref` + `into_request` moved verbatim to `request.rs`; `mod request;` +
`pub use request::{PatchAudioRequest, PatchAudioResult, PatchRequestSettings};` in
`mod.rs`. No import sweep (facade + `application::` re-exports unchanged). Build /
clippy / `-Tier pr-repair` green (382 lib pass / 1 ignored).
**Deviation from §3:** the M-CFG guards (`deref_reads_*`, `into_request_defaults_*`)
did **not** move to `request.rs` this phase — they share the `dual_fit_test_request`
helper with two dual-fit residual tests bound for `region.rs` (P4). Moving them now
would duplicate ~40 lines of `ScanAlignment` / `GapReport` scaffolding that P4 would
then reconcile. They stay in `mod.rs`'s `tests` (still resolving the types via the
facade `super::` re-export) and relocate in **P4** when `dual_fit_test_request` moves.

**P2 notes.** Keep `patch_region`’s `crate::application::patch_audio::correlate_frames_for_gap`
(etc.) working via facade `pub(crate) use`. `SeamGateDerived` *type* remains in
`patch_region`; only the `from_repair` impl moves into `geometry.rs`. Accept the
pre-existing `geometry` ↔ `patch_region` intra-crate edge (see §3).

**P2 as-landed (2026-07-24).** `decode.rs` = `DecodedAb` + `decode_ab`
(`pub(crate)`, re-exported `pub(crate) use decode::{decode_ab, DecodedAb}`).
`geometry.rs` = `repair_patch_config_view` + `impl SeamGateDerived::from_repair` +
`border_frames_from_secs` / `seam_gate_frames_for` / `correlate_frames_for_gap`
(three frame helpers re-exported `pub(crate) use`; `from_repair` is an inherent
method, needs no re-export). All blocks moved verbatim. **One visibility change:**
`repair_patch_config_view` was a private `fn` in the monolith; the cross-module move
requires `pub(super)` (used only from `mod.rs` at two sites) — minimal, not widened
to the crate. Unused imports pruned from `mod.rs` (`Duration`, eight `clip_sync`
decode-only symbols, `GapReport`, `RepairPatchConfigView`). Build / clippy /
`-Tier pr-repair` green (382 lib pass / 1 ignored; composition, patch_region,
harness, fingerprint-measure external paths all compile without import edits).

**P3 notes.** Move `gap_key` and `fill_offset_mode_label` with the log slice
(even though they currently sit above the anchor-retry block). Format-line unit
tests move here. Leave `region_outcome_gap_tags` behind for P4.

**P3 as-landed (2026-07-24).** `log.rs` = the 13 log/format symbols moved
verbatim: `gap_key` (`pub(super)`), private `fill_offset_mode_label`,
`GapFillPlanLog<'a>` / `GapFillResultLog` (`pub(crate)`), `format_gap_fill_plan_lines`
/ `format_gap_fill_result_line` / `format_skip_gap_fill_log` (`pub(crate)`, used by
the colocated tests), `log_gap_fill_plan_verbose` / `log_gap_fill_result_verbose` /
`log_skip_gap_fill` / `MarginalGapFillLog<'a>` (fields `pub(super)`) /
`log_marginal_gap_fill` / `log_gap_tags_verbose` (`pub(super)`). The 4 format-line
unit tests moved into `log.rs`'s `#[cfg(test)] mod tests`. **No visibility widening**
beyond the private→`pub(super)` needed for the cross-module move of `gap_key`. Facade
wires `mod log;` + `use log::{gap_key, log_*, GapFillPlanLog, GapFillResultLog,
MarginalGapFillLog}` (internal-only, no `pub` re-export — nothing outside
`patch_audio` consumes these). Unused imports pruned from `mod.rs` (five
`crate::domain::patch_result` / `gap_tags` format-fn symbols now owned by `log.rs`;
the dead `pub(crate) use log::{format_*}` re-export dropped). `log` has zero `region`
dependencies (all helpers take plain log structs / domain types / `GapTags` /
`ProgressReporter`), so the `log ← region` DAG edge holds. Build / clippy /
`-Tier pr-repair` green.

**P4 notes.** Expect `region.rs` ≈ 1.8–2.2 kloc after the move. That is acceptable
for this plan. Optional later peel (`dual_fit.rs` / `bracket.rs`) is **not**
required to close the M-MOD `patch_audio` row. Explicitly take
`RegionPatchMedia` / `RegionPatchOpts` / `RegionPatchContext`, `skipped_patch*`,
`record_patch_gap_span`, and `region_outcome_gap_tags` here (see contested
table). `region` may `use` `log::gap_key` for `outcomes_in_report_order`.

**P4 as-landed (2026-07-24).** `region.rs` = 2511 lines (within the predicted
1.8–2.2 kloc + colocated tests). Moved verbatim: the full region-patch pipeline —
`RegionPatch` / `RegionPatchOutcome`, `skipped_patch` / `skipped_patch_with_residual`,
`record_patch_gap_span`, `anchor_search_prior_for_gap`, `RegionPatchMedia` /
`RegionPatchOpts` / `RegionPatchContext`, `outcomes_in_report_order`,
`region_outcome_gap_tags`, `seam_failure_outcome`, `a_gap_floor_db`,
`b_mapped_start_frame`, the dual-fit chain (`DualFitRepairInput` / `build_dual_fit_input`
/ `dual_fit_eligible` / `measure_dual_fit_residual_verdict` / `skip_or_dual_fit` /
`finalize_dual_fit` / `DualFitDecision` / `DualFitRescue`), the bracket chain
(`BracketFillAssembly` / `assemble_bracket_fill` / `ExecuteBracketFillCtx` /
`execute_bracket_fill` / `ExecuteBracketOutputCtx` / `execute_bracket_output`),
`RegionCharacterization`, `skip_region_spec` / `skip_outcome_from_spec` /
`dual_fit_skipped`, `execute_region_spec`, `characterize_region` /
`characterize_all_regions`, `prepare_region_patch`, `slice_b_segment`,
`compute_a_border_rms`, and `splice_into_a`. Visibility widened only where the
`execute` orchestrator + the P5 anchor code (still in `mod.rs`) consume them:
`pub(super)` on `RegionPatch`(+fields) / `RegionPatchOutcome` / `RegionPatchMedia`(+fields)
/ `RegionPatchOpts`(+fields) / `RegionPatchContext`(+fields) / `RegionCharacterization`
and on `skipped_patch` / `record_patch_gap_span` / `outcomes_in_report_order` /
`region_outcome_gap_tags` / `skip_outcome_from_spec` / `execute_region_spec` /
`characterize_region` / `prepare_region_patch` / `splice_into_a`; everything else stayed
private. `region` uses `log::gap_key` (via `super::log`), so the `log ← region` edge
holds; `region` has no `anchor_retry` dependency (that module doesn't exist yet), so the
cycle guard holds. Facade wires `mod region;` + `use region::{...}` (internal-only, plus
`pub(crate) use geometry`/`decode` unchanged). The 8 region-bound unit tests
(`characterize_all_regions_yields_one_consistent_spec_per_region`,
`dual_fit_eligible_excludes_structure_alignment_failed`, `fit_fill_trims_tail...` /
`fit_fill_zero_pads...`, the `dual_fit_test_request` helper + `measure_dual_fit_residual...`,
and the two M-CFG deref/into-request guards) moved into `region.rs`'s `#[cfg(test)] mod
tests`; the anchored-retry tests + `dummy_region_tags` stay in `mod.rs` (they move in P5).
`skipped_patch` is imported directly from `region` by the `mod.rs` tests (not re-exported
through the facade, since no non-test `mod.rs` code consumes it). Build / clippy /
`-Tier pr-repair` green.

**P5 notes.** Anchor retry imports media/opts from `region`; do not redefine or
move those structs into `anchor_retry.rs`. Take only the retry policy / candidate
/ pass helpers and `AnchoredRetryState`.

**P5 as-landed (2026-07-24).** `anchor_retry.rs` = 356 lines (206 code + 116
tests + header). Moved verbatim: `patch_anchor_policy` / `build_patch_anchor_candidates`
/ `run_anchored_retry_pass` (`pub(super)`, consumed by `execute`), `AnchoredRetryState`
(`pub(super)` struct + all three `pub(super)` fields, since `execute` constructs it),
and the internals `anchored_retry_gap_indices` / `should_apply_anchored_retry_outcome`
/ `store_anchored_retry_patch` (private). No structs redefined — `RegionPatch`,
`RegionPatchOutcome`, `RegionPatchContext`, `RegionPatchMedia`, `RegionPatchOpts`,
`prepare_region_patch`, `record_patch_gap_span`, `region_outcome_gap_tags` are imported
straight from `super::region::{…}` (not through the `mod.rs` facade, so no facade
re-export is left dangling as "unused"); `PatchAudioRequest` from `super`, log helper
from `super::log`. The `region ← anchor_retry` DAG edge holds and there is no reverse
edge (region has zero anchor_retry references). The two anchored-retry unit tests +
`dummy_region_tags` moved into `anchor_retry.rs`'s `#[cfg(test)] mod tests` (they pull
`skipped_patch` via `super::super::region`). `mod.rs` pruned four now-unmoved domain
imports (`resolve_gap_offset_secs`, `is_retryable_patch_skip`, `PatchAnchorCandidate`,
`PatchAnchorPolicy`) plus the `FillMode` / `FillConfidence` / `FillRegion` symbols that
left with the anchor code, and dropped `prepare_region_patch` from the region facade
(now only anchor_retry used it). Build / clippy / `-Tier pr-repair` green.

**P6 notes.** `mod.rs` should hold orchestration only (plan → decode →
`characterize_*` / execute loop → optional `run_anchored_retry_pass` →
`splice_into_a` → summary). No leftover free-function clusters.

**P6 as-landed (2026-07-24).** No further extraction was required: P4+P5 already
left `mod.rs` at 324 lines holding orchestration only — module declarations, the
facade re-exports, `GAP_EDGE_REFINE_SECS` (consumed by `execute`), and
`PatchAudio::{new, execute}` (plan → decode → two-pass `characterize_region` /
`execute_region_spec` → optional `run_anchored_retry_pass` → `splice_into_a` →
summary). No leftover free-function clusters, no `#[cfg(test)] mod tests` in `mod.rs`
(all unit tests are colocated in their submodules). The original monolith is `mod.rs`
itself (P1 `git mv`), so there is no separate `.rs` to delete. P6 is a ledger-only
close-out; `-Tier pr-repair` confirmed green including the `PatchAudio::execute`
integration/oracle paths.

**Module split complete.** `patch_audio/` is now `mod.rs` (orchestration) + seven
submodules: `request` (P1), `decode` / `geometry` (P2), `log` (P3), `region` (P4),
`anchor_retry` (P5). Public import paths (`patch_audio::`, `application::PatchAudio`)
unchanged throughout; every phase was byte-preserving with unit tests colocated.

## 5. Verification (per phase / final gate)

- `cargo build -p clip-sync-repair --all-targets`
- `cargo clippy -p clip-sync-repair --all-targets`
- `.\scripts\test-tier.ps1 -Tier pr-repair`

After **P2**, confirm external paths still compile without import edits:
`composition.rs` (`decode_ab`), `patch_region.rs` (frame helpers), harness
`patch_audio` request builders, `gate_oracle` / fingerprint measure
(`SeamGateDerived::from_repair`).

After **P6**, confirm `-Tier pr-repair` still green including
`patch_audio_integration` / `anchor_seam_oracle` / dual-fit oracle paths that
exercise `PatchAudio::execute`.

## 6. Success criteria

Verified in source 2026-07-24: `application/patch_audio/` has
`mod`/`request`/`decode`/`geometry`/`log`/`region`/`anchor_retry` (no leftover
monolith `.rs`); `mod.rs` holds `PatchAudio::{execute,preview}` orchestration
only (no `#[cfg(test)]`); contested helpers match §3 (`gap_key` /
`fill_offset_mode_label` in `log`; `region_outcome_gap_tags` /
`RegionPatchMedia`/`Opts`/`skipped_patch*` in `region`; `SeamGateDerived::from_repair`
in `geometry`); DAG holds (`region` has zero `anchor_retry` refs; `log` has
zero `region` refs; `geometry` does not import `region`);
`application::{PatchAudio, PatchAudioRequest, …}` and `patch_audio::decode_ab`
still resolve for composition / config / repair callers.

- [x] No production file under `application/patch_audio/` is a multi-concern
      monolith; unit tests colocated per submodule (facade may keep only
      cross-cutting tests).
- [x] Public import paths unchanged (`patch_audio::` and
      `application::{PatchAudio, …}`).
- [x] Dependency DAG in §3 holds (no `region` ↔ `anchor_retry` cycle; no
      `geometry` → `region`; no `log` → `region`).
- [x] Contested helpers match the §3 table (`gap_key` / `fill_offset_mode_label`
      in `log`; `region_outcome_gap_tags` / media-opts / `skipped_patch*` in
      `region`; `from_repair` in `geometry`).
- [x] `decode_ab` / frame helpers / request types remain reachable at the same
      paths; harness + composition + `patch_region` compile without import sweep.
- [x] Byte-preserving: no intentional behavior / string / threshold changes.
- [x] `patch_audio` row of M-MOD closable; `align_videos` remains optional /
      declined separately.

---

## Related

- [TEMP-rust-review-findings.md](../TEMP-rust-review-findings.md) — **M-MOD**
- [TEMP-policies-module-split-plan.md](TEMP-policies-module-split-plan.md) — ground-rules template (done)
- [TEMP-gap-fingerprint-module-split-plan.md](TEMP-gap-fingerprint-module-split-plan.md) — sibling production split (done)
- [TEMP-gap-fingerprint-corpus-module-split-plan.md](TEMP-gap-fingerprint-corpus-module-split-plan.md) — harness sibling (done)
- [TEMP-repair-config-bundles-plan.md](TEMP-repair-config-bundles-plan.md) — M-CFG (do not reopen)
- `crates/clip-sync-repair/src/application/patch_audio/` — live split tree
- `crates/clip-sync-repair/src/application/patch_region.rs` — gate sibling (stays)
