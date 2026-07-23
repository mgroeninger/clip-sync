# `policies.rs` module split — plan (DRAFT)

Status: **draft / not started** (refreshed **2026-07-23** from current source). Split
`crates/clip-sync-repair/src/domain/policies.rs` (**4,291 lines** — production ≈ 2,590 + tests ≈
1,701) into a `policies/` directory with a stable `crate::domain::policies::*` re-export facade.
**Opportunistic** — do alongside seam/residual/scoring work, not as a standalone mega-refactor.

**M-MOD context.** This plan is the **policies slice** of P2 finding
[M-MOD](TEMP-rust-review-findings.md#m-mod-oversized-modules--open) (oversized modules). M-MOD also
calls for later splits of `gap_fingerprint` and harness `gap_fingerprint_corpus`; those are **out of
scope** here. Do policies first when tackling M-MOD.

**Companion history (do not re-open as triggers).** The pipeline redesign
([archive/TEMP-pipeline-perf-redesign-plan.md](archive/TEMP-pipeline-perf-redesign-plan.md)) is
**closed** — it spun out focused domain modules (`seam_local.rs`, `donor.rs`, `dual_fit.rs`) and
landed characterize→execute (6b). Its successor
([archive/TEMP-production-repair-perf-plan.md](archive/TEMP-production-repair-perf-plan.md)) **refuted**
the shared mono-downmix hoist (§2.1, 2026-07-20) and therefore **removed the perf trigger** for
P2/P3 extractions. Rule (unchanged): **extract-when-you-touch**, as a **separate byte-preserving PR**
adjacent to any behavior/perf change — never bundle a split into a behavior-change PR.

Companions: [archive/residual-channel-alignment-plan.md](archive/residual-channel-alignment-plan.md)
(**shipped** — P1 trigger fired), [archive/residual-gate-findings.md](archive/residual-gate-findings.md)
(L12 prototype **already deleted**), [gap-fill-modes.md](../gap-fill-modes.md) § Multichannel seams.

---

## 1. Problem (one paragraph)

`domain/policies.rs` is still the repair crate's largest domain file (~2,590 lines of production code
+ ~1,701 lines of `#[cfg(test)]`). It bundles silence scanning, gap-border refinement, Pearson seam
scoring (including the FFT band evaluator), residual/floor cancellation, and splice crossfade into
one translation unit. Sibling concerns already live in separate modules (`gap_structure.rs`,
`gap_energy.rs`, `gap_seam_extend.rs`, `residual_gate.rs`, `seam_local.rs`, `dual_fit.rs`, …). A
monolith increases review noise, merge conflicts, and navigation cost without matching the crate's
one-concern-per-module convention.

## 2. Non-goals

- **Renaming the public path** — keep `crate::domain::policies::fill_seam_correlations` (etc.) via
  re-exports; no repo-wide import sweep unless we explicitly choose a breaking change later.
- **Changing behavior** — pure move/split; zero functional diff, tests green before/after.
- **Splitting `clip-sync` (lib) `policies.rs`** — analyzer hexagon is out of scope.
- **M-MOD siblings** — `gap_fingerprint.rs`, harness `gap_fingerprint_corpus.rs`, `patch_audio.rs`,
  `align_videos.rs` are separate M-MOD bites; not this plan.
- **Arbitrary line-count targets** — avoid three ~900-line files with tangled `pub(crate)` helpers;
  boundaries follow cohesion, not math.
- **Waiting on a perf hoist** — §2.1 downmix hoist is dead; do not block P2/P3 on it.
- **Separating unit tests from production code** — do **not** move the current `#[cfg(test)]`
  block into `tests/`, a shared `policies/tests.rs`, or `*_test.rs` siblings. Unit tests stay at
  the bottom of the same `.rs` file as the code they cover (same pattern as `residual_gate.rs`
  and today's monolith). Crate-level integration/corpus tests already outside `policies.rs` stay
  where they are.

## 3. Current layout (re-derived 2026-07-23)

| Region | Lines (approx.) | Anchor | Primary consumers |
|--------|-----------------|--------|-------------------|
| Silence + RMS + fill gain | 1–367 | `SilenceRunScanner` @37, `is_silent*` @262, `compute_fill_gain` @358 | `scan_gaps.rs`, `gap_energy.rs` |
| Gap refine + borders + channel select | 370–778 | `FillAlignment` @370, `refine_gap_frames` @442, `GapBorderSpec` @527, `selected_seam_channels` @754 | `patch_region.rs`, `patch_audio.rs`, harnesses |
| Seam Pearson scoring | 780–1630 | `SeamTemplates` @780, `fill_splice_seam_correlations*` @996/@1111, `fill_seam_correlations*` @1287+, `fill_seam_correlations_band` @1404, `seam_channel_diagnostics` @1572 | `patch_region.rs`, `gap_fill_fit.rs` |
| Seam residual / floor | 1632–2471 | `lsq_residual_ratio` @1642, `SeamFloorProbe` @1744, `seam_chosen_and_floor*` @2031/@2075, `SeamResidualVerdict` @2260 | `patch_region.rs`, corpus tests |
| Splice / crossfade | 2473–2589 | `trim_low_energy_*` @2473, `apply_seam_crossfade` @2517 | `patch_audio.rs` |
| `#[cfg(test)]` | 2591–4291 | — | — |

Production ≈ 2,590 lines, tests ≈ 1,701. Notable growth since the 2026-07-07 refresh: multichannel
residual (`seam_chosen_and_floor_multichannel`), FFT seam band evaluator
(`fill_seam_correlations_band` + colocated regression), and more residual/floor tests. Line numbers
shift with every seam/residual PR — **re-derive anchors at extraction time**.

**Cross-cutting note:** `effective_seam_crossfade_frames` (@939) lives amid scoring but is shared by
splice scoring and `apply_seam_crossfade`. Prefer `seam_splice.rs` (or a one-line re-export from
`seam_scoring`) so both sides have one owner.

**Already extracted / adjacent modules (sizes as of refresh):**

| Module | ~Lines | Role |
|--------|--------|------|
| `residual_gate.rs` | 74 | Config surface: `ResidualGateMode`, lag/headroom defaults |
| `seam_local.rs` | 383 | Local seam search (not carved from policies) |
| `donor.rs` | 121 | Donor selection helpers |
| `dual_fit.rs` | 436 | Dual-fit rescue |
| `gap_structure.rs` | 626 | Gap structure |
| `gap_energy.rs` | 309 | Energy / silence consumers of policies |
| `pcm.rs` | 29 | Shared PCM types |

**Import pattern today:** application/domain code uses `crate::domain::policies::{…}` and
`policies::fn_name` heavily (`patch_region.rs` ≈ 47 `policies::` refs; `patch_audio.rs` ≈ 35). Narrow
imports remain in `gap_structure.rs` / `gap_energy.rs` (`FillAlignment`, `is_silent_frame`, …).

## 4. Proposed structure

```text
domain/
  policies/
    mod.rs              # re-exports entire public API (stable facade); no unit tests here
    silence.rs          # SilenceRunScanner, is_silent*, rms_*, compute_fill_gain
                        # + #[cfg(test)] mod tests { … } at bottom of this file
    gap_borders.rs      # refine_gap_frames, GapBorderSpec, border_templates_*,
                        # selected_seam_channels / loudest_seam_channel
                        # + colocated #[cfg(test)]
    seam_scoring.rs     # SeamTemplates, fill_seam_*, fill_repeat_*, splice scoring,
                        # fill_seam_correlations_band, seam_channel_diagnostics
                        # + colocated #[cfg(test)]
    seam_residual.rs    # floor probe, seam_chosen_and_floor*, SeamResidualVerdict
                        # + colocated #[cfg(test)]
    seam_splice.rs      # apply_seam_crossfade, effective_seam_crossfade_frames,
                        # trim_low_energy_* + colocated #[cfg(test)] (if any)
```

**Test organization (required):** each submodule that receives production code also receives the
unit tests that cover it, as a `#[cfg(test)] mod tests { … }` block **in that same file** —
directly under the production items, same layout as today's `policies.rs` and siblings like
`residual_gate.rs`. Partition the monolith's test blob by what it covers; do not create a parallel
test tree.

Rough test homes from current names: scanner/RMS → `silence`; `refine_gap_*` / `border_*` →
`gap_borders`; `fill_seam_*` / `fill_repeat_*` / splice correlation → `seam_scoring`;
`seam_residual_*` / `seam_floor_*` / `residual_verdict_*` → `seam_residual`.

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
  a shared helper only if a third consumer appears (defer a `pcm`-style shared file until then).

### 4c. Relationship to `residual_gate.rs`

Keep `residual_gate.rs` as the **config surface** (`ResidualGateMode`, `residual_max_lag_frames`,
defaults). Measurement primitives stay in `policies/seam_residual.rs`.
`DEFAULT_RESIDUAL_FLOOR_OK_DB` (@2224 today) lives with the measurement types in `seam_residual.rs`
(or re-export from `mod.rs`); do not move gate mode enums into policies.

### 4d. Prototype path — **done**

[archive/residual-gate-findings.md](archive/residual-gate-findings.md) **L12**:
`seam_residual_diagnostics` / `SeamResidual` were deleted. No further prototype cleanup is required
when extracting `seam_residual.rs`.

## 5. Extraction order (phasing)

Triggers are **status**, not schedules. Each extraction lands as a **separate byte-preserving PR** —
never bundled into a behavior-change PR.

| Phase | Module | Trigger / driver | Status | Notes |
|-------|--------|------------------|--------|-------|
| **P0** | — | — | — | No change; monolith until a phase below is pulled. |
| **P1** | `seam_residual.rs` | Residual channel alignment **shipped**; residual still hot | **Ready** (not done) | Highest value; pairs with `residual_gate.rs`. Anchors: `lsq_residual_ratio` @1642, `seam_chosen_and_floor` @2031, `SeamResidualVerdict` @2260. |
| **P2** | `silence.rs` | Opportunistic (downmix hoist **refuted**) | Pending / no forcing function | Small, clear boundary. Extract when silence/RMS is touched or when clearing M-MOD. |
| **P3** | `gap_borders.rs` | Opportunistic (same as P2) | Pending / no forcing function | `FillAlignment`, `RefinedGapFrames`, templates, `selected_seam_channels`. |
| **P4** | `seam_scoring.rs` (+ `seam_splice.rs`) | Characterize→execute **6b landed**; scoring still grows (FFT band) | **Ready** (not done) | Largest block (~850 lines prod). Split scoring vs ~120-line splice if still large after move. |
| **P5** | Delete `policies.rs` | After P1–P4 | — | `mod.rs` only; verify `pub use` list matches pre-split API. |

**Do not** execute P1–P5 as a single “split the file” PR. Prefer P1 or P4 first when residual or
scoring is already being touched; P2/P3 can wait until convenience or a full M-MOD pass.

## 6. Verification

**Principle: a pure module move cannot change behavior** — it is the same code in different files. So the
failure modes are entirely **compile-time** (incomplete re-export facade, `super::`/`pub(crate)`
visibility slip, colocated test losing a private item) or **clippy** — all caught in
**seconds-to-minutes**. Hours-long media/behavior tiers are **not** the gate here.

**Fast gate — run per phase (P1–P5):**

- `cargo build -p clip-sync-repair --all-targets` — **the dominant check**: every re-export resolves and all
  test/bench targets still compile.
- `cargo clippy -p clip-sync-repair --all-targets` — clean.
- `.\scripts\test-tier.ps1 -Tier pr-repair` — lib unit tests + golden footguns (fast; CI-default surface).
- **Facade checklist:** `grep '^pub ' domain/policies.rs` (pre-move) → assert every symbol is re-exported from
  `policies/mod.rs` (post-move). Pre-delete guard for P5.

**Belt-and-suspenders — once, at P5 (final `mod.rs`-only delete):**

- `.\scripts\test-tier.ps1 -Tier integration` — assembled binaries still link/run. One run at the end.
- Optional `cargo doc -p clip-sync-repair --no-deps` — public rustdoc unchanged for `policies::*`.

**Not required for a module split:** `-Tier validation` (ffmpeg + fetched corpus, **hours**). A split
that would change a `validation` outcome is by definition not a pure move — re-scope that phase.

## 7. Risks

| Risk | Mitigation |
|------|------------|
| Missed re-export → compile errors in tests/corpus | `mod.rs` checklist from `grep '^pub '` on old file before delete |
| Circular imports between submodules | Extract `silence` first when doing P2/P3 together; `seam_residual` depends on border helpers only |
| Test module path churn | Keep `#[cfg(test)]` in the same file as prod; move tests with the code they cover; run residual / floor / border / band tests after each phase |
| Temptation to “clean up” into `tests/` | Non-goal §2 — reject; failure output should point at the module under edit |
| Facade hides new code location | Submodule names in file paths; optional one-line module docs at top of each file |
| Moving `effective_seam_crossfade_frames` / channel-select helpers | Decide owner at extract time; keep facade re-exports so callers do not care |

## 8. Success criteria

- No production file under `domain/policies/` exceeds ~1,000 lines of non-test code; unit tests remain
  in that same file’s `#[cfg(test)]` block (not `tests/` or a shared test module).
- `patch_region.rs` / `patch_audio.rs` imports unchanged at the `policies::` path.
- Further residual/scoring work lands in `seam_residual.rs` / `seam_scoring.rs`, not back into a monolith.
- M-MOD policies row closable after P5; fingerprint/corpus splits remain separate.

---

## Related

- [TEMP-rust-review-findings.md](TEMP-rust-review-findings.md) — **M-MOD** (this plan is step 1)
- [archive/residual-channel-alignment-plan.md](archive/residual-channel-alignment-plan.md) — shipped; P1 trigger
- [archive/residual-gate-wiring-plan.md](archive/residual-gate-wiring-plan.md) — gate wiring (orthogonal)
- [archive/residual-gate-findings.md](archive/residual-gate-findings.md) — L12 prototype deleted
- [archive/TEMP-production-repair-perf-plan.md](archive/TEMP-production-repair-perf-plan.md) — §2.1 hoist
  refuted; P2/P3 fully opportunistic (§3)
- `crates/clip-sync-repair/src/domain/policies.rs` — current monolith
