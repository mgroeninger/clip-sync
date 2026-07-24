# `patch_audio.rs` module split — plan

Status: **planned** (not started) — target tree:

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

**M-MOD context.** This is the **optional `patch_audio` slice** of
[M-MOD](TEMP-rust-review-findings.md#m-mod-oversized-modules--open). Policies,
harness corpus, and production `gap_fingerprint` are **done** (see their plans /
archives). `align_videos` is **out of scope** here (test-inflated orchestrator;
already decomposed via sibling application modules — decline or defer without a
sibling plan unless that changes).

**Companion history (do not re-open).** Config collapse is closed
([archive/TEMP-repair-config-bundles-plan.md](archive/TEMP-repair-config-bundles-plan.md)
— M-CFG). This plan is **M-MOD maintainability only**: byte-preserving moves,
never bundled into behavior-change PRs. Do **not** retune gates, dual-fit, or
anchor policy while splitting.

Companions: [TEMP-policies-module-split-plan.md](TEMP-policies-module-split-plan.md)
(ground-rules template),
[TEMP-gap-fingerprint-module-split-plan.md](TEMP-gap-fingerprint-module-split-plan.md),
[TEMP-rust-review-findings.md](TEMP-rust-review-findings.md) **M-MOD**,
[gap-fill-modes.md](../gap-fill-modes.md), [pipeline.md](../pipeline.md).

---

## 1. Problem

`application/patch_audio.rs` (~3.6 kloc; ~3.0 k production + ~0.6 k colocated
tests) is a single-file repair orchestrator mixing several cohesion slices.

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
| **P1** | `request.rs` (result / request / settings) | **Planned** |
| **P2** | `decode.rs` + `geometry.rs` (leaf helpers; external consumers) | **Planned** |
| **P3** | `log.rs` (formatters + verbose helpers + their unit tests) | **Planned** |
| **P4** | `region.rs` (outcomes + characterize + dual-fit + bracket + splice) | **Planned** |
| **P5** | `anchor_retry.rs` (retry pass; needs `prepare_region_patch`) | **Planned** |
| **P6** | Thin `mod.rs` — only `PatchAudio::execute` + re-exports; delete monolith `.rs` | **Planned** |

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

**P2 notes.** Keep `patch_region`’s `crate::application::patch_audio::correlate_frames_for_gap`
(etc.) working via facade `pub(crate) use`. `SeamGateDerived` *type* remains in
`patch_region`; only the `from_repair` impl moves into `geometry.rs`. Accept the
pre-existing `geometry` ↔ `patch_region` intra-crate edge (see §3).

**P3 notes.** Move `gap_key` and `fill_offset_mode_label` with the log slice
(even though they currently sit above the anchor-retry block). Format-line unit
tests move here. Leave `region_outcome_gap_tags` behind for P4.

**P4 notes.** Expect `region.rs` ≈ 1.8–2.2 kloc after the move. That is acceptable
for this plan. Optional later peel (`dual_fit.rs` / `bracket.rs`) is **not**
required to close the M-MOD `patch_audio` row. Explicitly take
`RegionPatchMedia` / `RegionPatchOpts` / `RegionPatchContext`, `skipped_patch*`,
`record_patch_gap_span`, and `region_outcome_gap_tags` here (see contested
table). `region` may `use` `log::gap_key` for `outcomes_in_report_order`.

**P5 notes.** Anchor retry imports media/opts from `region`; do not redefine or
move those structs into `anchor_retry.rs`. Take only the retry policy / candidate
/ pass helpers and `AnchoredRetryState`.

**P6 notes.** `mod.rs` should hold orchestration only (plan → decode →
`characterize_*` / execute loop → optional `run_anchored_retry_pass` →
`splice_into_a` → summary). No leftover free-function clusters.

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

- [ ] No production file under `application/patch_audio/` is a multi-concern
      monolith; unit tests colocated per submodule (facade may keep only
      cross-cutting tests).
- [ ] Public import paths unchanged (`patch_audio::` and
      `application::{PatchAudio, …}`).
- [ ] Dependency DAG in §3 holds (no `region` ↔ `anchor_retry` cycle; no
      `geometry` → `region`; no `log` → `region`).
- [ ] Contested helpers match the §3 table (`gap_key` / `fill_offset_mode_label`
      in `log`; `region_outcome_gap_tags` / media-opts / `skipped_patch*` in
      `region`; `from_repair` in `geometry`).
- [ ] `decode_ab` / frame helpers / request types remain reachable at the same
      paths; harness + composition + `patch_region` compile without import sweep.
- [ ] Byte-preserving: no intentional behavior / string / threshold changes.
- [ ] `patch_audio` row of M-MOD closable; `align_videos` remains optional /
      declined separately.

---

## Related

- [TEMP-rust-review-findings.md](TEMP-rust-review-findings.md) — **M-MOD**
- [TEMP-policies-module-split-plan.md](TEMP-policies-module-split-plan.md) — ground-rules template (done)
- [TEMP-gap-fingerprint-module-split-plan.md](TEMP-gap-fingerprint-module-split-plan.md) — sibling production split (done)
- [TEMP-gap-fingerprint-corpus-module-split-plan.md](TEMP-gap-fingerprint-corpus-module-split-plan.md) — harness sibling (done)
- [archive/TEMP-repair-config-bundles-plan.md](archive/TEMP-repair-config-bundles-plan.md) — M-CFG (do not reopen)
- `crates/clip-sync-repair/src/application/patch_audio.rs` — current monolith
- `crates/clip-sync-repair/src/application/patch_region.rs` — gate sibling (stays)
