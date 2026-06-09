# Workspace refactor — implementation gaps (archived)

> **Status:** Archived. Phases 1–4 complete (2026-06-07 / 2026-06-08). Write path (R0–R5) shipped 2026-06-09 — [repair-write-path-plan.md](repair-write-path-plan.md).  
> **Purpose:** Pre-implementation gaps, ambiguities, and deferred scope identified during readiness review (2026-06-07).

**Parent plan:** [workspace-refactor-plan.md](workspace-refactor-plan.md)  
**Architecture reference:** [PLAN.md](../../PLAN.md)

---

## Summary

| Scope | Ready to implement? | Notes |
|-------|-------------------|-------|
| **PR A — Phases 1 + 2** (workspace, lib hexagon, CLI hexagon) | **Done (2026-06-07)** | All pre-implementation gaps resolved; 98 tests green |
| **PR B — Phase 3** (`test-utils`, CLI adapter tests, docs) | **Done (2026-06-07)** | 120 tests green |
| **PR C — Phase 4** (repair report-only) | **Done (2026-06-08)** | `clip-sync-repair` crate shipped; 120 tests green |
| **PR D — Repair write path** | **Yes** | Shipped (R0–R5); see [repair-write-path-plan.md](repair-write-path-plan.md) |

**Overall:** Phases **1–4** and the repair write path (R0–R5) are complete. See [repair-write-path-plan.md](repair-write-path-plan.md).

---

## Gaps to resolve during Phases 1–3

These are not architectural blockers. Address them in the same PRs as the parent plan steps, or add explicit sub-steps to the main plan when touching those areas.

### 1. `LoggingConfig` relocation (Low)

**What the plan says:** `LoggingConfig`, `LogLevel`, and `ProgressMode` live in lib `infrastructure::logging` (shared driving-adapter config). Facade re-exports them from there.

**Current code:** All three types are in `src/application/config.rs`. `infrastructure/logging/mod.rs` imports them from `application::config`.

**Gap:** The main plan splits `AlignConfig` / `OutputConfig` in Phase 1 step 1.4 but does not explicitly call out moving logging types to `infrastructure::logging`.

**Resolution (suggested):**

- During Phase 1 config split, move `LogLevel`, `ProgressMode`, and `LoggingConfig` to `crates/clip-sync/src/infrastructure/logging/`.
- Update `init_tracing` and `StderrProgressReporter` to use the local module.
- Re-export from facade `lib.rs` as specified in the parent plan § Library public API.

---

### 2. Example config file for CLI round-trip test (Low) — Resolved (Phase 3)

`crates/clip-sync-cli/tests/fixtures/analyzer.toml` added in Phase 3. `config_roundtrip.rs` deserializes via `load_app_config` and asserts key fields.

---

### 3. Phase 1.4 `OutputConfig` transition and `AlignVideosRequest` migration scope (Low)

**What the plan says:** Phase 1.4 — keep `AlignConfig` in lib; **move `OutputConfig` out** (stub until Phase 2; tests may still use full config temporarily). Phase 2.6 — `AlignVideosRequest` uses `AlignConfig`; update lib + all tests.

**Gap:** Two related concerns.

First, the transition mechanics are hand-wavy — when does `AlignVideosRequest` stop accepting fields that include `output`?

Second, the test update scope is larger than "update lib + all tests" signals. `align_videos.rs` contains over a dozen test helpers (`two_clip_config`, `cross_layer_chirp_config`, `request`, etc.) that all construct `AppConfig`. `corpus_fixtures.rs` does too via `build_config`. These must all be updated in the same commit as the `AlignVideosRequest` type change or the crate won't compile mid-PR.

**Resolution (suggested):**

- **Phase 1:** Introduce `AlignConfig { clip, alignment }`; keep `OutputConfig` in a temporary location or CLI-bound module if needed for compiling tests.
- **Phase 2 (same PR as 1 if practical):** `AlignVideosRequest.config` becomes `AlignConfig` only; `AppConfig` in CLI owns `output` + `logging`.
- `align_videos` already uses only `config.clip.*` and `config.alignment.*` — no use-case logic changes required, only config type substitution.
- When switching the type, update all test helpers in `align_videos.rs` and `corpus_fixtures.rs` in the same commit to keep the build green throughout the PR.

---

### 4. `corpus_root()` path fix timing (Low — critical path)

**What the plan says:** Phase 1.7 — `corpus_root()` → workspace `tests/corpus/` via `testing_paths` (`CARGO_MANIFEST_DIR/../..`, optional `CLIP_SYNC_WORKSPACE_ROOT`).

**Current code:** `corpus_fixtures.rs` uses `env!("CARGO_MANIFEST_DIR").join("tests/corpus")`, which works for the root crate but **breaks** once the lib moves to `crates/clip-sync/`.

**Gap:** None in the plan — step exists — but this is on the **critical path** for PR A. Verify `cargo test -p clip-sync corpus_` before merging Phase 2.

**Resolution:** Implement `testing_paths.rs` early in Phase 1; switch `corpus_fixtures.rs` to `corpus_root()` before or with the `git mv`.

---

### 5. Phase 1.8 — temporary root binary (Low)

**What the plan says:** “Jump to Phase 2 in same PR if practical; else temporary root binary calling `clip_sync::…`.”

**Gap:** Implementer choice, not a spec gap.

**Resolution:** Prefer **Phases 1 + 2 in one PR** (parent plan § Recommended PR sequence). Avoid an intermediate root binary unless the diff must be split for review.

---

### 6. Publish policy (None for implementation)

**What the plan says:** “Single workspace version for v1 (all crates `0.1.0`). **Publish policy TBD.**”

**Gap:** Undecided whether crates are published to crates.io.

**Impact:** No blocker for workspace extraction or local `cargo install`. Decide before any public crate publish.

---

### 7. `HighRateRefinement` type missing from lib facade (Low — correctness)

**What the plan says:** Facade re-exports `AlignmentResult` from `domain`. The plan's re-export list in § Library public API does not mention `HighRateRefinement`.

**Current code:** `AlignmentResult.high_rate_refinement: Option<HighRateRefinement>`. The `HighRateRefinement` struct is a named type in `domain/alignment.rs`. It is inspected in `align_videos.rs` (`refine.applied`, `refine.adjustment_secs`, `refine.correlation_peak`) and in test assertions.

**Gap:** After the workspace split, `clip-sync-cli` and `clip-sync-repair` will receive `AlignmentResult` values containing `Option<HighRateRefinement>`, but cannot name or match on `HighRateRefinement` if it isn't re-exported. This will cause a compile error the first time the CLI or repair crate inspects the refinement result.

**Resolution:** Add `HighRateRefinement` to the domain re-export block in `lib.rs` alongside `AlignmentResult`:

```rust
pub use domain::{
    AlignmentResult, AudioTrack, ClipMatch, ClipMatchEstimate, ClipWindow, ClipLabel,
    DomainError, Fingerprint, HighRateRefinement, MediaSource, MonoPcmClip,
};
```

Address during Phase 1.3 when writing the facade.

---

### 8. `window_slide_secs` undocumented in the plan (Low — documentation)

**What the plan says:** `ClipConfig` includes `window_slide_secs: u32` with no description.

**Current code:** Used in two places in `align_videos.rs`:
- `expand_window_for_slide(window, clip_config.window_slide_secs, duration)` — extracts a wider PCM window to allow subclip selection.
- `select_aligned_subclip_pair(raw_a, raw_b, window.duration())` — picks the best-aligned subclip pair from the wider extract when `window_slide_secs > 0`.

Setting `window_slide_secs = 0` disables sliding and uses the clip window directly (several tests set this explicitly).

**Gap:** An implementer reading only the plan cannot determine what the field does, whether it belongs in `AlignConfig` or `ClipConfig`, or what its default means. The plan's `ClipConfig` struct listing should describe it.

**Resolution:** Add a one-line description to the `ClipConfig` block in both PLAN.md and TEMP-workspace-refactor-plan.md:

```
window_slide_secs: u32,   // extra seconds extracted either side for subclip sliding (0 = disabled)
```

No code change needed; this is a documentation-only gap. Address when editing config docs during Phase 1.

---

### 9. Cargo.toml sketch inaccuracies (Low)

**What the plan says:** Cargo.toml sketches for `crates/clip-sync/Cargo.toml` and `crates/clip-sync-cli/Cargo.toml` are illustrative.

**Current Cargo.toml deviations:**

- `anyhow = "1"` is listed as a dependency but is not imported anywhere in `src/`. Safe to omit from the lib crate.
- `serde_json = "1"` is present (used for JSON output in the CLI) but absent from the plan's lib sketch. Since JSON serialization of `AlignmentResult` is part of the CLI's output layer, `serde_json` belongs in `clip-sync-cli`, not in the lib — but the plan's lib sketch should note its omission is intentional.

**Resolution:** When authoring `crates/clip-sync/Cargo.toml` in Phase 1.1, omit `anyhow`. When authoring `crates/clip-sync-cli/Cargo.toml` in Phase 2.1, include `serde_json`. No code changes needed.

---

## Intentionally deferred scope (Phases 4–5)

Items 10–14 below were deferred at gaps-review time. All are now resolved by Phase 4. Write-path items (`RepairVideos`, `gap_fill`, `MediaMuxer`, ffmpeg mux) are tracked in [repair-write-path-plan.md](repair-write-path-plan.md) (R0–R5).

### Facade repair allow-list — Resolved (Phase 4)

Only `select_best_track` from `domain::policies` was needed; re-exported on the lib facade. No `pcm_preparation` or `offset_refinement` symbols required — the repair crate implements its own `is_silent` policy in `repair/domain/policies.rs`. Verified zero `clip_sync::infrastructure::` or `clip_sync::domain::` non-facade imports in the repair crate.

### `timeline_scan` helper — Resolved (Phase 4, not needed)

Chunked scan via repeated `MediaSession::extract_mono` calls suffices. `timeline_scan` helper was not added to the lib.

### Repair domain and use cases — Resolved (Phase 4)

Implemented:

- `domain/gap.rs` — `Gap`, `GapReport`
- `domain/policies.rs` — `is_silent` (RMS vs peak-fraction threshold); 4 unit tests
- `application/scan_gaps.rs` — `ScanGaps<MR>` use case; `execute` calls `align_with_defaults` → chunked extract → `GapReport`
- `application/ports.rs` — `GapReporter` port; `MediaMuxer` stub (write path R5)
- `infrastructure/cli/` — `run()`, `Args`, `StdoutGapReporter`, `exit_code_for`

Write path (`RepairVideos`, `gap_fill`, `PatchAudio`, WAV writer, ffmpeg mux) → [repair-write-path-plan.md](repair-write-path-plan.md) R0–R5.

### Repair config loader — Resolved (Phase 4)

`load_repair_app_config` implemented in `infrastructure/config.rs`. TOML config with `[repair]` section; `AlignConfig` flattened for backward compatibility. CLI flags override loaded values.

### Repair exit codes documentation — Resolved (Phase 4)

`docs/error-mapping.md` updated with full repair exit-code table and updated implementation reference paths.

---

## Codebase ↔ plan alignment (verified)

Facts confirmed against the repo at readiness review. No action unless the code drifts before implementation.

| Topic | Current state | Plan expectation |
|-------|---------------|------------------|
| Crate layout | **Done:** workspace with `crates/clip-sync`, `clip-sync-cli`; root is workspace only | As designed |
| `AlignVideosRequest.config` | **Done:** `AlignConfig` | As designed |
| Adapter wiring | **Done:** `default_pipeline::align_with_defaults` + `run_align` | As designed |
| `align_videos` config usage | **Done:** `clip` + `alignment` only via `AlignConfig` | As designed |
| Corpus data | `tests/corpus/` (manifest, README, `wav/`) | Stays at workspace root ✓ |
| `corpus_root()` | **Done:** `../..` from lib `CARGO_MANIFEST_DIR` + `CLIP_SYNC_WORKSPACE_ROOT` override | As designed |
| `PLAN.md` | Updated: implementation status reflects workspace complete | ✓ |
| `BACKLOG.md` | Updated: “Binary-only crate” removed from Defer; workspace extraction added to Completed | ✓ |

---

## Interaction with other active plans

Feature TEMP plans under `docs/` are **not** blocked by the workspace refactor:

- [docs/TEMP-clip-self-repetition-plan.md](docs/TEMP-clip-self-repetition-plan.md)
- [docs/TEMP-offset-verification-plan.md](docs/TEMP-offset-verification-plan.md)

Parent plan: update **path references** in those files when features land (`src/…` → `crates/clip-sync/src/…`), not during Phases 1–3 unless the same files are edited anyway.

Implement repetition/verification **after** PR A if they touch `align_videos.rs` or config — reduces merge conflict surface.

---

## Pre-flight checklist (PR A) — Complete

All items verified 2026-06-07. PR A shipped.

- [x] `cargo test` green on current `main`
- [x] `cargo test corpus_` green (baseline pass/fail for Tier A + B)
- [x] `corpus_root()` fix: `../..` from `CARGO_MANIFEST_DIR` + `CLIP_SYNC_WORKSPACE_ROOT` override (gap 4)
- [x] `LoggingConfig` relocation to `infrastructure::logging` (gap 1)
- [x] `HighRateRefinement` added to facade `domain` re-exports (gap 7)
- [x] `AlignVideosRequest.config: AlignConfig`; all test helpers updated in same commit (gap 3)
- [x] `anyhow` omitted from lib Cargo.toml; `serde_json` in CLI Cargo.toml (gap 9)
- [x] Phases 1 + 2 in one PR; no intermediate binary (gap 5)
- [ ] After merge: `cargo run -p clip-sync-cli -- A B` matches pre-refactor output (manual smoke test)

---

## Risks (from parent plan — unchanged)

| Risk | Mitigation |
|------|------------|
| Large mechanical diff | Single PR per phase; no logic changes in Phases 1–2 except wiring extraction |
| `AppConfig` / TOML breakage | CLI round-trip test; `#[serde(flatten)]` on `align` |
| `corpus_root` break on move | Fix in Phase 1.7; verify before Phase 2 merge |
| Facade drift | Allow-list table; grep CI for `clip_sync::infrastructure::` in CLI/repair crates |

---

## Resolution tracking

| # | Gap | Severity | Resolve in | Status |
|---|-----|----------|------------|--------|
| 1 | `LoggingConfig` relocation | Low | PR A (Phase 1) | Resolved |
| 2 | Example config TOML fixture | Low | PR B (Phase 3) | Resolved |
| 3 | `OutputConfig` transition + `AlignVideosRequest` migration scope | Low | PR A (Phases 1–2) | Resolved |
| 4 | `corpus_root()` critical path | Low | PR A (Phase 1.7) | Resolved |
| 5 | Temporary root binary | Low | PR A (prefer skip) | Resolved — skipped |
| 6 | Publish policy | None | Before crates.io publish | Open |
| 7 | `HighRateRefinement` missing from facade | Low — correctness | PR A (Phase 1.3 facade) | Resolved |
| 8 | `window_slide_secs` undocumented in plan | Low — documentation | PR A (Phase 1 config docs) | Resolved |
| 9 | Cargo.toml sketch inaccuracies (`anyhow` / `serde_json`) | Low | PR A (Phase 1.1) | Resolved |
| 10 | Repair facade allow-list | Phase 4 | PR C | Resolved — `select_best_track` only |
| 11 | `timeline_scan` decision | Phase 4 | PR C | Resolved — not needed |
| 12 | Repair algorithms / policies | Phase 4–5 | PR C | Resolved — `is_silent` in repair domain |
| 13 | Repair config loader choice | Phase 4 | PR C | Resolved — `load_repair_app_config` with TOML |
| 14 | Repair exit codes in docs | Phase 4.8 | PR C | Resolved — `docs/error-mapping.md` updated |
