# `policies.rs` module split — plan

> **Archived 2026-07-24.** Planned M-MOD policies bite; shipped. Record only.

Status: **done (2026-07-23)** — P1–P5 complete. Live tree:

```text
domain/policies/
  mod.rs              # thin facade (35 lines); stable `crate::domain::policies::*`
  silence.rs          # 653 lines (390 prod + 263 colocated tests)
  gap_borders.rs      # 682 lines (380 prod)
  seam_scoring.rs     # 1121 lines (862 prod)
  seam_residual.rs    # 1562 lines (847 prod)
  seam_splice.rs      # 342 lines (139 prod)
```

Callers keep `crate::domain::policies::{…}`; no import sweep. Unit tests stay in each
submodule’s `#[cfg(test)]` block (not `tests/` / `*_test.rs`).

**M-MOD context.** This was the **policies slice** of
[M-MOD](../TEMP-rust-review-findings.md#m-mod-oversized-modules--closed). Sibling planned M-MOD
splits (production `gap_fingerprint`, harness `gap_fingerprint_corpus`, `patch_audio`) are
also done; `align_videos` remains deferred with no plan. Those bites were **out of scope**
here.

**Companion history (do not re-open).** Pipeline redesign closed; production-perf §2.1 hoist
refuted — extractions were opportunistic / M-MOD-driven, not hoist-triggered. Rule kept:
byte-preserving moves, never bundled into behavior-change PRs.

Companions: [residual-channel-alignment-plan.md](residual-channel-alignment-plan.md),
[residual-gate-findings.md](residual-gate-findings.md) (L12 deleted),
[gap-fill-modes.md](../../gap-fill-modes.md) § Multichannel seams.

---

## 1. Problem (resolved)

Pre-split, `domain/policies.rs` was a ~4.3 kloc monolith (silence + borders + Pearson scoring +
residual/floor + splice). It is now a facade + five cohesion-based submodules with colocated
tests.

## 2. Non-goals (unchanged; still apply to future edits)

- **Renaming the public path** — keep `crate::domain::policies::*` via re-exports.
- **Changing behavior** — pure move/split only.
- **Splitting `clip-sync` (lib) `policies.rs`** — analyzer hexagon out of scope.
- **M-MOD siblings** — fingerprint / harness / patch_audio / align_videos are separate.
- **Separating unit tests from production code** — do **not** move `#[cfg(test)]` into `tests/`
  or `*_test.rs`; keep tests at the bottom of the same `.rs` file.

## 3. Final layout

| Module | Owns | Notes |
|--------|------|-------|
| `silence.rs` | `SilenceRunScanner`, `is_silent*`, `is_silent_frame`, RMS, `compute_fill_gain` | `is_silent_frame` lives here (scanner + refine consumers) |
| `gap_borders.rs` | `refine_gap_frames`, `GapBorderSpec`, templates, `selected_seam_channels` | Uses `silence::is_silent_frame` + `seam_splice` trim helpers |
| `seam_scoring.rs` | `SeamTemplates`, fill/repeat/splice Pearson, FFT band, diagnostics | Uses `gap_borders` channel indices + mono helpers |
| `seam_residual.rs` | Floor probe, `seam_chosen_and_floor*`, `SeamResidualVerdict` | Uses `seam_scoring::{seam_pearson, interleaved_channel_timeline_f64}` |
| `seam_splice.rs` | `apply_seam_crossfade`, `effective_seam_crossfade_frames`, trim helpers | Trim is `pub(crate)` for `gap_borders` |
| `mod.rs` | Re-exports only | No unit tests |

### Internal visibility

- Shared helpers stay `pub(crate)` in the owning submodule (`seam_pearson`, trim, etc.).
- Facade re-exports the pre-split public API; `pub(crate)` items used outside `policies/`
  (`adaptive_seam_window_frames`, `border_active_extent_frames`, FFT band helpers,
  `seam_score_channels`) are re-exported at `pub(crate)`.

## 4. Phase ledger

| Phase | Module | Status |
|-------|--------|--------|
| **P1** | `seam_residual.rs` | **Done** 2026-07-23 |
| **P2** | `silence.rs` | **Done** 2026-07-23 |
| **P3** | `gap_borders.rs` | **Done** 2026-07-23 |
| **P4** | `seam_scoring.rs` + `seam_splice.rs` | **Done** 2026-07-23 |
| **P5** | Thin `mod.rs` only | **Done** 2026-07-23 |

## 5. Verification (as run)

Per phase / final gate:

- `cargo build -p clip-sync-repair --all-targets`
- `cargo clippy -p clip-sync-repair --all-targets`
- `.\scripts\test-tier.ps1 -Tier pr-repair`

All green after P5 (2026-07-23).

## 6. Success criteria

Verified in source 2026-07-24: `domain/policies/` facade + five submodules; no leftover
`policies.rs` monolith; public path `crate::domain::policies::*` via re-exports.

- [x] No production file under `domain/policies/` is a multi-concern monolith; unit tests colocated.
- [x] `patch_region.rs` / `patch_audio` imports unchanged at the `policies::` path.
- [x] Residual/scoring work has dedicated homes (`seam_residual.rs` / `seam_scoring.rs`).
- [x] Policies row of M-MOD closable; fingerprint/corpus splits remain separate.

---

## Related

- [TEMP-rust-review-findings.md](../TEMP-rust-review-findings.md) — **M-MOD**
- [TEMP-production-repair-perf-plan.md](TEMP-production-repair-perf-plan.md) — hoist refuted
- `crates/clip-sync-repair/src/domain/policies/` — current tree
