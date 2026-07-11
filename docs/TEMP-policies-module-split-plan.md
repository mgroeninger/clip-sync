# `policies.rs` module split — plan (DRAFT)

Status: **draft / not started** (refreshed 2026-07-07). Split
`crates/clip-sync-repair/src/domain/policies.rs` (**now 3,827 lines** — grew from ~2,800 as dual-fit /
residual work piled in: A3b `single_lag_alignment`, A6 residual, seam-scoring additions) into a `policies/`
directory with a stable `crate::domain::policies::*` re-export facade. **Opportunistic** — do alongside
seam/residual work, not as a standalone refactor.

**Companion to the pipeline redesign.** This is the module-organization axis; the pipeline redesign was the
assembly axis — orthogonal, kept as separate plans. **Status update (2026-07-11):** that redesign
([archive/TEMP-pipeline-perf-redesign-plan.md](archive/TEMP-pipeline-perf-redesign-plan.md)) is **closed** —
its dump/fingerprint work landed, and it already spun out focused domain modules (`seam_local.rs`, `donor.rs`,
`dual_fit.rs`). Its remaining P2/P3 **trigger** — the shared mono-downmix **Hoist** — moved to the successor
[TEMP-production-repair-perf-plan.md](TEMP-production-repair-perf-plan.md) (§2.1), which is **measure-first**:
P2 (`silence.rs`) / P3 (`gap_borders.rs`) fire **only if** that production measurement shows the hoist is worth
doing. P4 (`seam_scoring.rs`, trigger 6b) and P1 (`seam_residual.rs`, trigger A6) already fired independently
and stay ready. **Rule (unchanged): extract-when-you-touch, as a separate byte-preserving PR adjacent to the
perf step — never bundled into a behavior-change PR** (that would wreck the §4/C2 "diff proves no behavior
change" guarantee). The production perf doc's §3 restates this ordering (extract owner, then hoist).

Companions: [archive/residual-channel-alignment-plan.md](archive/residual-channel-alignment-plan.md) (**shipped**
— P1's trigger has fired; P1 is *ready* but not done), [archive/residual-gate-findings.md](archive/residual-gate-findings.md)
(L12 prototype retirement), [gap-fill-modes.md](gap-fill-modes.md) § Multichannel seams.

---

## 1. Problem (one paragraph)

`domain/policies.rs` is the repair crate's largest domain file (~2,800 lines; ~1,900 lines of
production code + ~920 lines of `#[cfg(test)]`). It bundles silence scanning, gap-border refinement,
Pearson seam scoring, residual/floor cancellation, and splice crossfade into one translation unit.
Sibling concerns already live in separate modules (`gap_structure.rs`, `gap_energy.rs`,
`gap_seam_extend.rs`, `residual_gate.rs`). Residual and seam work is active; a monolith increases
review noise, merge conflicts, and navigation cost without matching the crate's one-concern-per-module
convention.

## 2. Non-goals

- **Renaming the public path** — keep `crate::domain::policies::fill_seam_correlations` (etc.) via
  re-exports; no repo-wide import sweep unless we explicitly choose a breaking change later.
- **Changing behavior** — pure move/split; zero functional diff, tests green before/after.
- **Splitting `clip-sync` (lib) `policies.rs`** — analyzer hexagon is out of scope.
- **Arbitrary line-count targets** — avoid three ~900-line files with tangled `pub(crate)` helpers;
  boundaries follow cohesion, not math.
- **Blocking residual channel alignment** — channel work can land first; this plan sequences
  extraction so alignment can pull `seam_residual` out when touched.

## 3. Current layout (re-derived 2026-07-07)

| Region | Lines (approx.) | Anchor | Primary consumers |
|--------|-----------------|--------|-------------------|
| Silence + RMS | 1–305 | `SilenceRunScanner` @12 | `scan_gaps.rs`, `gap_energy.rs` |
| Gap refine + borders | 306–643 | `refine_gap_frames` @306 | `patch_region.rs`, `patch_audio.rs`, harnesses |
| Seam Pearson scoring | 644–1,710 | `SeamTemplates` @644, `fill_splice_seam_correlations_interleaved` @975 | `patch_region.rs`, `gap_fill_fit.rs` |
| Seam residual / floor | 1,711–2,196 | `seam_chosen_and_floor` @1711, `SeamResidualVerdict` @1940 | `patch_region.rs`, `gap_fill_fit.rs`, corpus tests |
| Splice / crossfade | 2,197–2,270 | `apply_seam_crossfade` @2197 | `patch_audio.rs` |
| `#[cfg(test)]` | 2,271–3,827 | — | — |

Production ≈ 2,270 lines, tests ≈ 1,557. The **seam-scoring region roughly doubled** (was 538–1,248 ≈ 710
lines; now 644–1,710 ≈ 1,066) — the dual-fit seam work (A3b `single_lag_alignment`, per-channel splice
scoring). Line numbers shift with every seam/residual PR; re-derive the anchors at extraction time rather than
trusting these.

**Already extracted / adjacent modules:** `residual_gate.rs` (86 lines) — `ResidualGateMode`, lag/frame
helpers, thresholds (config/mode only; measurement primitives remain in `policies.rs`). The pipeline redesign
also created **new** focused primitives — `seam_local.rs` (355), `donor.rs` (133), `dual_fit.rs` (448) — not
carved *from* policies but demonstrating the one-concern-per-module target this plan finishes.

**Import pattern today:** application code uses `crate::domain::policies::{self, …}` and
`policies::fn_name` (~25 references in `patch_region.rs` alone). `gap_structure.rs` and
`gap_energy.rs` import narrow slices (`FillAlignment`, `is_silent_frame`).

## 4. Proposed structure

```text
domain/
  policies/
    mod.rs              # re-exports entire public API (stable facade)
    silence.rs          # SilenceRunScanner, is_silent*, rms_*
    gap_borders.rs      # refine_gap_frames, GapBorderSpec, border_templates_*, interleaved_to_*
    seam_scoring.rs     # SeamTemplates, fill_seam_*, repeat/splice Pearson, channel diagnostics
    seam_residual.rs    # floor probe, seam_chosen_and_floor, SeamResidualVerdict, informative
    seam_splice.rs      # apply_seam_crossfade, effective_seam_crossfade, trim_low_energy_*
```

Each submodule owns its `#[cfg(test)] mod tests { … }` block (move tests with the code they cover).

### 4a. `mod.rs` facade

Re-export every type and function currently public on `policies`. Callers keep:

```rust
use crate::domain::policies::{FillAlignment, fill_seam_correlations, …};
```

Optional: add `pub mod silence;` etc. for explicit submodule paths in new code only — not required for
phase 1.

### 4b. Internal visibility

- Shared helpers (`seam_pearson`, `lsq_residual_ratio`, `mono_window`) stay `pub(crate)` in the
  submodule that owns them; sibling submodules import via `super::` or `crate::domain::policies::…`
  as needed.
- Prefer **minimal cross-submodule coupling**: `seam_residual` may call `gap_borders::mono_window` or
  duplicate-free shared `pcm.rs` only if a third shared helper file is justified (defer until a second
  consumer appears).

### 4c. Relationship to `residual_gate.rs`

Keep `residual_gate.rs` as the **config surface** (`ResidualGateMode`, `residual_max_lag_frames`).
Move **measurement primitives** (`SeamFloorProbe`, `seam_chosen_and_floor`, `SeamResidualVerdict`,
`floor_probe_informative`) into `policies/seam_residual.rs`. `DEFAULT_RESIDUAL_FLOOR_OK_DB` can
live in `seam_residual.rs` or `residual_gate.rs` — pick one home; re-export from `mod.rs`.

### 4d. Retire prototype path (optional, same PR as `seam_residual` extract)

[archive/residual-gate-findings.md](archive/residual-gate-findings.md) **L12**: `seam_residual_diagnostics` /
`SeamResidual` are test-only. When extracting `seam_residual.rs`, either delete the prototype or
move it to `seam_residual.rs` behind `#[cfg(test)]` with a one-line doc comment. Do not leave dead
`floor_db` / `frac_lag` fields on the hot path (L9, L10).

## 5. Extraction order (phasing)

Triggers are now **anchored to pipeline-redesign steps** (the de facto driver). Each extraction lands as a
**separate byte-preserving PR adjacent to** its trigger step — see the pipeline plan §2.6 for the live status
table that keeps these visible during the refactor.

| Phase | Module | Pipeline trigger step | Status | Notes |
|-------|--------|----------------------|--------|-------|
| **P0** | — | — | — | No change; monolith until a step below touches its region. |
| **P1** | `seam_residual.rs` | A6 residual (**landed**) / residual cleanup | **Ready** (trigger fired, not done) | Highest value; pairs with `residual_gate.rs`; retire L12 prototype. `seam_chosen_and_floor` @1711, `SeamResidualVerdict` @1940. |
| **P2** | `silence.rs` | **Step 8** hoist (binned-RMS single owner) | Pending step 8 | Independent of seam; small, clear boundary. The hoist *needs* this owner — decomposition = the perf motion. |
| **P3** | `gap_borders.rs` | **Step 8** hoist (border-extract single owner) | Pending step 8 | `FillAlignment`, `RefinedGapFrames`, templates. Same motion as P2. |
| **P4** | `seam_scoring.rs` (+ `seam_splice.rs`) | **6b** (seam scoring consolidates into characterize; #4 reconciliation) | Pending 6b | Largest block (~1,066 lines); split scoring vs the ~73-line splice only if still large. |
| **P5** | Delete `policies.rs` | After P1–P4 | — | `mod.rs` only; verify `pub use` list matches pre-split API. |

**Do not** execute P1–P5 as a single “split the file” PR, and **do not** bundle any Pn into its trigger step's
PR — phased, standalone, byte-preserving landings keep both the split *and* the pipeline diff reviewable.

## 6. Verification

**Principle: a pure module move cannot change behavior** — it is the same code in different files. So the
failure modes are entirely **compile-time** (an incomplete re-export facade, a `super::`/`pub(crate)`
visibility slip, a colocated test that loses access to a private item) or **clippy** — all caught in
**seconds-to-minutes**. The hours-long media/behavior tiers test outcomes a move literally cannot alter, so
they are **not** the gate here. (The old "full lib + integration green" line assumed a fast suite; the
integration/`validation` tiers now take hours and are unnecessary for a byte-preserving split.)

**Fast gate — run per phase (P1–P5):**

- `cargo build -p clip-sync-repair --all-targets` — **the dominant check**: every re-export resolves and all
  test/bench targets still compile. A missing `pub use` or a moved-test visibility break fails here.
- `cargo clippy -p clip-sync-repair --all-targets` — clean (catches new module-level lint triggers).
- `.\scripts\test-tier.ps1 -Tier pr-repair` — lib unit tests + golden footguns (fast; the CI-default surface).
  This runs the colocated `#[cfg(test)]` blocks that moved with each region.
- **Facade checklist:** `grep '^pub ' domain/policies.rs` (pre-move) → assert every symbol is re-exported from
  `policies/mod.rs` (post-move). This is the pre-delete guard (§7 risk 1).

**Belt-and-suspenders — once, at P5 (final `mod.rs`-only delete):**

- `.\scripts\test-tier.ps1 -Tier integration` — confirms the assembled binaries still link/run. One run at the
  end, not per phase.
- Optional `cargo doc -p clip-sync-repair --no-deps` — public rustdoc unchanged for `policies::*`.

**Not required for a module split:** `-Tier validation` (ffmpeg + fetched corpus, **hours**, media-behavior).
A split that would change a `validation` outcome is by definition not a pure move — if one is ever needed, the
phase is doing more than moving code and must be re-scoped. Reserve `validation` for the *pipeline* steps that
touch behavior, per that plan's §4 harness — not for these extractions.

## 7. Risks

| Risk | Mitigation |
|------|------------|
| Missed re-export → compile errors in tests/corpus | `mod.rs` checklist generated from `grep '^pub '` on old file before delete |
| Circular imports between submodules | Extract `silence` first (no seam deps); `seam_residual` depends on borders helpers only |
| Test module path churn | Move tests with code; run `seam_residual_*` / `seam_floor_*` / border tests after each phase |
| Facade hides new code location | Submodule names in file paths; optional one-line module docs at top of each file |

## 8. Success criteria

- No production file in `domain/` exceeds ~1,000 lines of non-test code (tests colocated per module).
- `patch_region.rs` imports unchanged at the `policies::` path.
- Residual channel alignment lands in `seam_residual.rs`, not back into a monolith.

---

## Related

- [archive/residual-channel-alignment-plan.md](archive/residual-channel-alignment-plan.md) — shipped; P1 trigger for `seam_residual.rs` split
- [archive/residual-gate-wiring-plan.md](archive/residual-gate-wiring-plan.md) — gate wiring (orthogonal)
- [archive/residual-gate-findings.md](archive/residual-gate-findings.md) — L9–L13 smells to clean during P1
- `crates/clip-sync-repair/src/domain/policies.rs` — current monolith
