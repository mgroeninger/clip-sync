# Analyzer `policies.rs` module split — plan

Status: **done (2026-07-24)** — P1–P4 complete. Live tree:

```text
crates/clip-sync/src/domain/policies/
  mod.rs              # thin facade (~40 lines); stable `crate::domain::policies::*`
  track_selection.rs  # track pick / pair ordering + colocated tests
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
template), `crates/clip-sync/src/domain/policies/` (current tree).

---

## 1. Problem (resolved)

Pre-split, `crates/clip-sync/src/domain/policies.rs` was a **~1.7 kloc** monolith (~896
production + ~829 colocated tests) mixing four analyzer-domain concerns. It is now a facade
+ four cohesion-based submodules with colocated tests.

| Concern | Role |
|---------|------|
| **Track selection** | `select_best_track`, `select_track_for_reference`, `order_track_pairs_for_alignment` |
| **Clip planning** | `EndClipAnchor`, `ClipPlanningOptions`, `clip_windows_*`, paired planning, interiors, `attach_symmetric_planning_report_metadata`, `should_use_query_mode` |
| **Extract quality** | `truncate_padded_tail`, `holdout_extract_sufficient`, `end_clip_extract_unreliable` |
| **Hold-out placement** | `pick_holdout_window` … `resolve_holdout_candidates`, mapped-region rebase/shift |

`secs_to_duration` is **`pub(crate)` in `clip_planning`**; `holdout` imports it. No util
module; DAG not inverted.

---

## 2. Non-goals (unchanged; still apply to future edits)

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
| `clip_planning.rs` | `EndClipAnchor`, `INTERIOR_OVERLAP_TOLERANCE`, `ClipPlanningOptions`, `effective_timeline_end`, `clip_windows_with_options`, `interior_windows_along_timeline`, `interior_overlaps_fixed_clip`, `clip_windows_paired`, `attach_symmetric_planning_report_metadata`, private paired helpers, `should_use_query_mode`, **`secs_to_duration` (`pub(crate)`)** | Leaf w.r.t. siblings. |
| `extract_quality.rs` | `truncate_padded_tail`, `holdout_extract_sufficient`, `end_clip_extract_unreliable` | Leaf. PCM decode / pad gates; not placement. |
| `holdout.rs` | `pick_holdout_window` … `resolve_holdout_candidates`, private rebase/shift | Depends on `clip_planning::secs_to_duration` only among siblings. |
| `mod.rs` | Explicit re-exports only | No production code; no unit tests |

### Internal visibility

- `secs_to_duration` → `pub(crate)` in `clip_planning` (not facade-re-exported).
- Private paired / rebase helpers stay private in their owning submodule.
- Facade `pub use` matches the pre-split public API.

### Dependency direction (holds)

```text
track_selection     (leaf)
clip_planning       (leaf; owns secs_to_duration)
extract_quality     (leaf)
holdout  ←  clip_planning   (secs_to_duration only)

mod.rs  re-exports all four
```

---

## 4. Phase ledger

| Phase | Slice | Status |
|-------|-------|--------|
| **P1** | `track_selection.rs` | **Done** 2026-07-24 — `git mv` monolith → `policies/mod.rs`; extracted track fns + 13 tests |
| **P2** | `clip_planning.rs` | **Done** 2026-07-24 — planning + query-mode + `secs_to_duration` `pub(crate)` + 15 tests |
| **P3** | `extract_quality.rs` | **Done** 2026-07-24 — three decode/pad gates + 5 tests |
| **P4** | `holdout.rs` + thin `mod.rs` | **Done** 2026-07-24 — placement remainder + 13 tests; facade only |

---

## 5. Verification (as run)

- `cargo build -p clip-sync --all-targets` — green
- `cargo clippy -p clip-sync --all-targets` — green (no warnings)
- `cargo test -p clip-sync --lib domain::policies` — **46 passed** (same names/count)
- `.\scripts\test-tier.ps1 -Tier pr-align` — green (`corpus_committed` ok)

---

## 6. Success criteria

- [x] No production file under `domain/policies/` is a multi-concern monolith; unit tests
      colocated per submodule.
- [x] Public import paths unchanged (`policies::` / existing `domain` + crate-root re-exports).
- [x] Dependency DAG in §3 holds (only `holdout` → `clip_planning` for `secs_to_duration`).
- [x] Policies unit test names/count unchanged and green under lib tests (46).
- [x] Analyzer policies row of M-MOD closable; `align_videos` remains deferred separately.

---

## Related

- [TEMP-rust-review-findings.md](TEMP-rust-review-findings.md) — **M-MOD**
- [TEMP-policies-module-split-plan.md](archive/TEMP-policies-module-split-plan.md) — repair policies (done; ground-rules template; explicitly deferred this bite)
- [TEMP-gap-fingerprint-module-split-plan.md](archive/TEMP-gap-fingerprint-module-split-plan.md) — production fingerprint (done)
- [TEMP-gap-fingerprint-corpus-module-split-plan.md](archive/TEMP-gap-fingerprint-corpus-module-split-plan.md) — harness corpus (done)
- [TEMP-patch-audio-module-split-plan.md](archive/TEMP-patch-audio-module-split-plan.md) — repair `patch_audio` (done)
- `crates/clip-sync/src/domain/policies/` — current tree
