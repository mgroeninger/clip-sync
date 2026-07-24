# Analyzer `policies.rs` module split — plan

Status: **planned** — not started. Target tree:

```text
crates/clip-sync/src/domain/policies/
  mod.rs              # thin facade; stable `crate::domain::policies::*`
  track_selection.rs  # ~55 prod + colocated track-selection tests
  clip_planning.rs    # EndClipAnchor / windows / paired / query-mode gate
  extract_quality.rs  # truncate / decode-sufficiency / end-clip unreliable
  holdout.rs          # hold-out placement + mapped-region resolve
```

Callers keep `crate::domain::policies::{…}` (and existing `domain/mod.rs` / `lib.rs`
re-exports); no import sweep. Unit tests stay in each submodule’s `#[cfg(test)]` block
(not `tests/` / `*_test.rs`).

**M-MOD context.** This is the **analyzer hexagon** policies bite that
[`TEMP-policies-module-split-plan.md`](archive/TEMP-policies-module-split-plan.md) explicitly left
out of scope (“Splitting `clip-sync` (lib) `policies.rs` — analyzer hexagon out of scope”).
Repair `domain/policies/` is **done**, as are the other planned repair M-MOD splits
(harness corpus, production fingerprint, `patch_audio`). `align_videos` stays deferred
and **out of scope** here.

**Ground rules (same family as repair policies / fingerprint / corpus).** Derived from
[`TEMP-policies-module-split-plan.md`](archive/TEMP-policies-module-split-plan.md),
[`TEMP-gap-fingerprint-module-split-plan.md`](archive/TEMP-gap-fingerprint-module-split-plan.md),
and [`TEMP-gap-fingerprint-corpus-module-split-plan.md`](archive/TEMP-gap-fingerprint-corpus-module-split-plan.md):

1. **Byte-preserving moves only** — never bundle into a behavior-change PR. No threshold
   retunes, no API redesign, no algorithm edits.
2. **Stable public path** — facade re-exports the pre-split `pub` surface;
   `crate::domain::policies::*` (and crate-root re-exports) unchanged.
3. **No import sweep** — callers keep their current `policies::` / `domain::` paths.
4. **Colocated unit tests** — keep `#[cfg(test)]` at the bottom of the owning `.rs`; do
   **not** move into `tests/` or `*_test.rs`. Split a test only when it clearly belongs
   with one submodule; cross-cutting tests may stay on the parent (corpus pattern) — this
   file’s tests partition cleanly, so prefer per-submodule homes.
5. **One cohesion slice per phase** — dependency-forward, then thin `mod.rs`.
6. **Shared helpers stay `pub(crate)`** in the owning submodule; facade re-exports only
   what was public pre-split.
7. **Avoid visibility churn** — if moving a private helper would force crate-wide
   `pub(crate)` on internals with a single consumer, keep the helper with that consumer
   (corpus P1 deviation precedent).
8. **Document the DAG** — no cycles between submodules.
9. **Do not reopen companions** — repair policies, fingerprint, corpus splits are closed;
   this plan is maintainability only.

Companions: [TEMP-rust-review-findings.md](TEMP-rust-review-findings.md) **M-MOD**,
[TEMP-policies-module-split-plan.md](archive/TEMP-policies-module-split-plan.md) (repair ground-rules
template), `crates/clip-sync/src/domain/policies.rs` (current monolith).

---

## 1. Problem

`crates/clip-sync/src/domain/policies.rs` is a **~1.7 kloc** monolith (~896 production +
~829 colocated tests) mixing four analyzer-domain concerns:

| Concern | Approx. locus today | Role |
|---------|---------------------|------|
| **Track selection** | ~L15–69 | `select_best_track`, `select_track_for_reference`, `order_track_pairs_for_alignment` |
| **Clip planning** | ~L71–437 | `EndClipAnchor`, `ClipPlanningOptions`, `clip_windows_*`, paired planning, interiors, `attach_symmetric_planning_report_metadata`, `should_use_query_mode` |
| **Extract quality** | ~L439–499 | `truncate_padded_tail`, `holdout_extract_sufficient`, `end_clip_extract_unreliable` |
| **Hold-out placement** | ~L501–895 | `pick_holdout_window` … `resolve_holdout_candidates`, mapped-region rebase/shift |

Natural seams are already the public function clusters above (no section-comment banners
required). Split along those; do not redesign planning / hold-out policy.

`secs_to_duration` (~L501–503) is a **shared private helper** used by clip planning
(`interior_windows_along_timeline`) and hold-out. Ownership: **`clip_planning` as
`pub(crate)`** — planning is the lower dependency; hold-out imports it. Do **not** put a
third “util” module for three lines, and do **not** invert the DAG by parking it in
`holdout`.

---

## 2. Non-goals

- **Renaming the public path** — keep `crate::domain::policies::*` via re-exports.
- **Changing behavior** — pure move/split only.
- **Touching repair `clip-sync-repair` `domain/policies/`** — already split; out of scope.
- **M-MOD siblings** — `align_videos` split / repair `lib.rs` curation are separate
  (`patch_audio` repair split is already done).
- **Separating unit tests from production code** — do **not** move `#[cfg(test)]` into
  `tests/` or `*_test.rs`.
- **Reworking `domain/mod.rs` / `lib.rs` symbol lists** — keep the same names; only the
  backing module path becomes a directory.
- **M-MOD-DEPS-style helper relocation in the same PR** — if a misplacement is discovered
  after the split, land it as a **follow-up** commit (repair precedent), not inside a
  byte-preserving phase.

---

## 3. Final layout

| Module | Owns | Notes |
|--------|------|-------|
| `track_selection.rs` | `select_best_track`, `select_track_for_reference`, `order_track_pairs_for_alignment` | Leaf. Imports `AudioTrack` / `DomainError` only. |
| `clip_planning.rs` | `EndClipAnchor`, `INTERIOR_OVERLAP_TOLERANCE`, `ClipPlanningOptions`, `effective_timeline_end`, `clip_windows_with_options`, `interior_windows_along_timeline`, `interior_overlaps_fixed_clip`, `clip_windows_paired`, `attach_symmetric_planning_report_metadata`, private paired helpers (`window_overlap_secs`, `end_window_for_file`, `filter_overlapping_interiors_paired`, `assemble_labeled_windows`), `should_use_query_mode`, **`secs_to_duration` (`pub(crate)`)** | Leaf w.r.t. siblings. Query-mode gate folds here (tiny; decisions from extents + window counts from planning). |
| `extract_quality.rs` | `truncate_padded_tail`, `holdout_extract_sufficient`, `end_clip_extract_unreliable` | Leaf. PCM decode / pad gates; not placement. |
| `holdout.rs` | `pick_holdout_window`, `holdout_window_candidates`, `parallel_holdout_window_candidates`, `holdout_window_centered_in`, `anchor_holdout_candidates`, `holdout_window_feasible`, `holdout_b_window_for_offset`, `holdout_pick_duration`, `mapped_region_holdout_candidates`, `resolve_holdout_candidates`, private `rebase_clip_window_to_region` / `shift_clip_window_on_a` | Depends on `clip_planning::secs_to_duration` only among siblings. |
| `mod.rs` | Re-exports only | No production code; no unit tests (tests partition cleanly). |

### Internal visibility

- `secs_to_duration` → `pub(crate)` in `clip_planning`.
- Private paired / rebase helpers stay private in their owning submodule.
- Facade `pub use` matches today’s public API (everything currently `pub fn` / `pub struct` /
  `pub enum` / `pub const` in the monolith). `domain/mod.rs` and `lib.rs` keep listing the
  same symbols from `policies::{…}`.

### Dependency direction (must hold)

```text
track_selection     (leaf)
clip_planning       (leaf; owns secs_to_duration)
extract_quality     (leaf)
holdout  ←  clip_planning   (secs_to_duration only)

mod.rs  re-exports all four
```

No other edges between submodules. In particular: **no** `clip_planning` → `holdout`,
**no** `extract_quality` ↔ anything.

---

## 4. Phase ledger

Extract **one cohesion slice per phase**. Do not combine with behavior PRs.

| Phase | Slice | Status | Notes |
|-------|-------|--------|-------|
| **P1** | `track_selection.rs` | Planned | Mechanical warm-up (corpus-style leaf first). Move the three track fns + their ~12 tests. `git mv` monolith → `policies/mod.rs` in this phase **or** P4 — either works; prefer **P1 `git mv` then extract** (fingerprint P1 pattern) so intermediate states are already a directory. |
| **P2** | `clip_planning.rs` | Planned | Types + single-file / paired windows + query-mode gate + `secs_to_duration` as `pub(crate)`. Move clip-window / paired / interior / attach-metadata tests. Hold-out code remaining in `mod.rs` must `use super::clip_planning::secs_to_duration` (or equivalent) until P4. |
| **P3** | `extract_quality.rs` | Planned | Three decode/pad gates + their tests (`truncate_*`, `end_clip_extract_*`, `holdout_extract_sufficient_*`). Leaf — can swap with P2 if desired; listed after planning so P2 clears the shared helper early. |
| **P4** | `holdout.rs` + thin `mod.rs` | Planned | Remainder of placement API + mapped-region helpers + hold-out tests. `mod.rs` becomes facade only: `mod track_selection; mod clip_planning; mod extract_quality; mod holdout;` + `pub use …::*`. |

**Suggested order rationale.** Leaves first (`track_selection`), then the shared-helper owner
(`clip_planning`), then the other leaf (`extract_quality`), then the dependent remainder
(`holdout`) + thin facade — same “dependency-forward, then thin mod” pattern as repair
policies P1–P5 and corpus P1–P4.

**P1 procedure sketch (fingerprint-style).**

1. `git mv crates/clip-sync/src/domain/policies.rs crates/clip-sync/src/domain/policies/mod.rs`
2. Extract track-selection production + tests into `track_selection.rs`
3. `mod track_selection; pub use track_selection::*;` on the parent
4. Verify (§5)

---

## 5. Verification (per phase / final gate)

- `cargo build -p clip-sync --all-targets`
- `cargo clippy -p clip-sync --all-targets`
- `cargo test -p clip-sync --lib` (policies tests must keep the same names / counts)
- `.\scripts\test-tier.ps1 -Tier pr-align` (analyzer bar; or `-Tier pr` if running the full gate)

After **P4**, confirm external call sites still compile **without** import edits:
`align_videos`, `offset_verification`, `high_rate_refinement`, `locate_query*`,
`application/config`, `application/report`, `domain/mod.rs`, `lib.rs`.

Byte-preserving check (same bar as prior splits): normalized line-set / union diff of moved
bodies should show **zero function-body changes**; only allowed deltas are module docs,
`mod` / `pub use` facade lines, `use super::…` imports, and `secs_to_duration` visibility
(`fn` → `pub(crate) fn`).

---

## 6. Success criteria

- [ ] No production file under `domain/policies/` is a multi-concern monolith; unit tests
      colocated per submodule.
- [ ] Public import paths unchanged (`policies::` / existing `domain` + crate-root re-exports).
- [ ] Dependency DAG in §3 holds (only `holdout` → `clip_planning` for `secs_to_duration`).
- [ ] Policies unit test names/count unchanged and green under `pr-align` / lib tests.
- [ ] Analyzer policies row of M-MOD closable; `align_videos` remains deferred separately.

---

## Related

- [TEMP-rust-review-findings.md](TEMP-rust-review-findings.md) — **M-MOD**
- [TEMP-policies-module-split-plan.md](archive/TEMP-policies-module-split-plan.md) — repair policies (done; ground-rules template; explicitly deferred this bite)
- [TEMP-gap-fingerprint-module-split-plan.md](archive/TEMP-gap-fingerprint-module-split-plan.md) — production fingerprint (done)
- [TEMP-gap-fingerprint-corpus-module-split-plan.md](archive/TEMP-gap-fingerprint-corpus-module-split-plan.md) — harness corpus (done)
- [TEMP-patch-audio-module-split-plan.md](archive/TEMP-patch-audio-module-split-plan.md) — repair `patch_audio` (done)
- `crates/clip-sync/src/domain/policies.rs` — current monolith (~1725 lines)
