# `policies.rs` module split — plan (DRAFT)

Status: **draft / not started**. Split `crates/clip-sync-repair/src/domain/policies.rs` (~2,800
lines) into a `policies/` directory with a stable `crate::domain::policies::*` re-export facade.
**Opportunistic** — do alongside seam/residual work, not as a standalone refactor.

Companions: [TEMP-residual-channel-alignment-plan.md](TEMP-residual-channel-alignment-plan.md)
(first extraction target), [residual-gate-findings.md](residual-gate-findings.md) (L12 prototype
retirement), [gap-fill-modes.md](gap-fill-modes.md) § Multichannel seams.

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

## 3. Current layout (exact)

| Region | Lines (approx.) | Primary consumers |
|--------|-----------------|-------------------|
| Silence + RMS | 1–215 | `scan_gaps.rs`, `gap_energy.rs` |
| Gap refine + borders | 216–537 | `patch_region.rs`, `patch_audio.rs`, harnesses |
| Seam Pearson scoring | 538–1,248 | `patch_region.rs`, `gap_fill_fit.rs` |
| Seam residual / floor | 1,249–1,763 | `patch_region.rs`, `gap_fill_fit.rs`, corpus tests |
| Splice / crossfade | 1,764–1,883 | `patch_audio.rs` |
| `#[cfg(test)]` | 1,885–2,804 | — |

**Already extracted:** `residual_gate.rs` (55 lines) — `ResidualGateMode`, lag/frame helpers, default
thresholds. Config/mode only; measurement primitives remain in `policies.rs`.

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

[residual-gate-findings.md](residual-gate-findings.md) **L12**: `seam_residual_diagnostics` /
`SeamResidual` are test-only. When extracting `seam_residual.rs`, either delete the prototype or
move it to `seam_residual.rs` behind `#[cfg(test)]` with a one-line doc comment. Do not leave dead
`floor_db` / `frac_lag` fields on the hot path (L9, L10).

## 5. Extraction order (phasing)

| Phase | Module | Trigger | Notes |
|-------|--------|---------|-------|
| **P0** | — | — | No change; file stays monolithic until a seam/residual PR needs it. |
| **P1** | `seam_residual.rs` | [TEMP-residual-channel-alignment-plan.md](TEMP-residual-channel-alignment-plan.md) or residual cleanup | Highest value; pairs with `residual_gate.rs`; retire L12 prototype. |
| **P2** | `silence.rs` | Next `scan_gaps` / silence-threshold touch | Independent of seam; small, clear boundary. |
| **P3** | `gap_borders.rs` | Border/standoff/refine work | `FillAlignment`, `RefinedGapFrames`, templates. |
| **P4** | `seam_scoring.rs` + `seam_splice.rs` | Seam Pearson or crossfade change | Largest block; split scoring vs splice only if `seam_scoring` still feels large (~650 + ~120 lines). |
| **P5** | Delete `policies.rs` | After P1–P4 | `mod.rs` only; verify `pub use` list matches pre-split API. |

**Do not** execute P1–P5 as a single “split the file” PR unless review bandwidth allows — phased
landings keep diffs reviewable.

## 6. Verification

- `cargo test -p clip-sync-repair` — full lib + integration green.
- `cargo clippy -p clip-sync-repair --all-targets` — clean.
- Grep: no remaining `domain/policies.rs` file; all former `policies::` paths resolve via `mod.rs`.
- Optional: `cargo doc -p clip-sync-repair --no-deps` — public rustdoc unchanged for `policies::*`.

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

- [TEMP-residual-channel-alignment-plan.md](TEMP-residual-channel-alignment-plan.md) — P1 trigger
- [residual-gate-wiring-plan.md](residual-gate-wiring-plan.md) — gate wiring (orthogonal)
- [residual-gate-findings.md](residual-gate-findings.md) — L9–L13 smells to clean during P1
- `crates/clip-sync-repair/src/domain/policies.rs` — current monolith
