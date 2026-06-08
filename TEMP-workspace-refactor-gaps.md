# Temporary: workspace refactor — implementation gaps

> **Status:** Active companion to [TEMP-workspace-refactor-plan.md](TEMP-workspace-refactor-plan.md).  
> **Purpose:** Pre-implementation gaps, ambiguities, and deferred scope identified during readiness review (2026-06-07).  
> **Archive:** Fold resolved items into the main plan or delete this file when Phases 1–3 ship.

**Parent plan:** [TEMP-workspace-refactor-plan.md](TEMP-workspace-refactor-plan.md)  
**Architecture reference:** [PLAN.md](PLAN.md)

---

## Summary

| Scope | Ready to implement? | Notes |
|-------|-------------------|-------|
| **PR A — Phases 1 + 2** (workspace, lib hexagon, CLI hexagon) | **Yes** | Minor gaps below; resolve during the PR |
| **PR B — Phase 3** (`test-utils`, CLI adapter tests, docs) | **Yes** | Depends on PR A |
| **PR C — Phase 4** (repair report-only) | **No** | Scaffold only; see § Deferred scope |
| **PR D — Phase 5** (repair write path) | **No** | Optional; depends on Phase 4 |

**Overall:** Phases **1–3** are ready to start. Phases **4–5** should wait until core extraction is merged.

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

### 2. Example config file for CLI round-trip test (Low)

**What the plan says:** Phase 2.7 — “round-trip deserialize from an existing example config file.”

**Current code:** No committed `.toml` config fixture. `README.md` contains inline TOML examples only.

**Gap:** Implementer must invent a fixture path or inline TOML in the test.

**Resolution (suggested):**

- Add `crates/clip-sync-cli/tests/fixtures/analyzer.toml` (or workspace `tests/fixtures/analyzer.toml`) with the four sections: `[clip]`, `[alignment]`, `[output]`, `[logging]`.
- Copy values from `README.md` § Analyzer config.
- `config_roundtrip.rs` deserializes via `load_app_config`, asserts key fields, optionally round-trips serialize.

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

Not ready for implementation. Track in BACKLOG or a follow-up plan after Phases 1–3 archive.

### Facade repair allow-list (placeholders)

Parent plan § Library public API and § Repair facade allow-list use incomplete exports:

```rust
pub use domain::pcm_preparation::{/* silence / peak helpers used by repair — see allow-list */};
pub use application::offset_refinement::{aligned_slice_starts, /* boundary correlation fns */};
```

**Before Phase 4:** Finalize which `pcm_preparation` and `offset_refinement` symbols repair needs (e.g. silence threshold helpers, `pcm_cross_correlate_lag`, `refine_holdout_segment_lag`) and add them to the facade allow-list table.

### `timeline_scan` helper (conditional)

Parent plan: add `application::timeline_scan` in lib Phase 4 **if** gap scan cannot be built from repeated `MediaSession::extract_mono` calls alone.

**Before Phase 4:** Spike chunked scan on a long fixture; decide whether the helper is required.

### Repair domain and use cases (sketch only)

Defined at high level only:

- `domain/gap.rs`, `domain/policies.rs` — min gap, silence threshold as pure fns
- `ScanGaps` — align → chunked extract → detect silent runs → `GapReport`
- `RepairVideos` / `gap_fill` / `FfmpegMediaMuxer` — Phase 5

**Missing detail:** Gap detection algorithm, chunk overlap policy, fillability correlation thresholds, PCM splice + crossfade behaviour.

### Repair config loader (optional in v1)

Parent plan: “v1 may accept CLI flags only; TOML loader optional in Phase 4.”

Decide flags-only vs `load_repair_app_config` before scaffolding `clip-sync-repair`.

### Repair exit codes documentation

`docs/error-mapping.md` has analyzer paths only (`src/application/error.rs`, `src/infrastructure/cli/exit_code.rs`). Phase 4.8 adds repair exit codes — file does not exist yet in repair crate.

---

## Codebase ↔ plan alignment (verified)

Facts confirmed against the repo at readiness review. No action unless the code drifts before implementation.

| Topic | Current state | Plan expectation |
|-------|---------------|------------------|
| Crate layout | Single binary at root; `main.rs` → `cli::run()` | Workspace with `crates/clip-sync`, `clip-sync-cli`, `clip-sync-repair` |
| `AlignVideosRequest.config` | `AppConfig` | `AlignConfig` after Phase 2.6 |
| Adapter wiring | Inline in `infrastructure/cli/mod.rs` | `default_pipeline::align_with_defaults` + `run_align` |
| `align_videos` config usage | `clip` + `alignment` only | Safe to split `OutputConfig` / `LoggingConfig` out |
| Corpus data | `tests/corpus/` (manifest, README, `wav/`) | Stays at workspace root |
| `corpus_root()` | `CARGO_MANIFEST_DIR/tests/corpus` | `../..` from lib crate + env override |
| `PLAN.md` | Target architecture documented | Aligned with migration plan |
| `BACKLOG.md` | “Binary-only crate” still under Defer | Mark done in Phase 2.9 per parent plan |

---

## Interaction with other active plans

Feature TEMP plans under `docs/` are **not** blocked by the workspace refactor:

- [docs/TEMP-clip-self-repetition-plan.md](docs/TEMP-clip-self-repetition-plan.md)
- [docs/TEMP-offset-verification-plan.md](docs/TEMP-offset-verification-plan.md)

Parent plan: update **path references** in those files when features land (`src/…` → `crates/clip-sync/src/…`), not during Phases 1–3 unless the same files are edited anyway.

Implement repetition/verification **after** PR A if they touch `align_videos.rs` or config — reduces merge conflict surface.

---

## Pre-flight checklist (PR A)

Run before starting Phase 1; repeat before merging Phase 2.

- [ ] `cargo test` green on current `main`
- [ ] `cargo test corpus_` green (baseline pass/fail for Tier A + B)
- [ ] `testing_paths.rs` + `corpus_root()` fix included early in Phase 1 (gap 4)
- [ ] `LoggingConfig` relocation included in config split (gap 1)
- [ ] `HighRateRefinement` added to facade `domain` re-exports (gap 7)
- [ ] `AlignVideosRequest` type change and all test helper updates in one commit (gap 3)
- [ ] `anyhow` omitted from lib Cargo.toml; `serde_json` in CLI Cargo.toml (gap 9)
- [ ] Phases 1 + 2 in one PR unless review size forces a split (gap 5)
- [ ] After merge: `cargo run -p clip-sync-cli -- A B` matches pre-refactor output

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
| 1 | `LoggingConfig` relocation | Low | PR A (Phase 1) | Open |
| 2 | Example config TOML fixture | Low | PR A (Phase 2.7) | Open |
| 3 | `OutputConfig` transition + `AlignVideosRequest` migration scope | Low | PR A (Phases 1–2) | Open |
| 4 | `corpus_root()` critical path | Low | PR A (Phase 1.7) | Open |
| 5 | Temporary root binary | Low | PR A (prefer skip) | Open |
| 6 | Publish policy | None | Before crates.io publish | Open |
| 7 | `HighRateRefinement` missing from facade | Low — correctness | PR A (Phase 1.3 facade) | Open |
| 8 | `window_slide_secs` undocumented in plan | Low — documentation | PR A (Phase 1 config docs) | Resolved |
| 9 | Cargo.toml sketch inaccuracies (`anyhow` / `serde_json`) | Low | PR A (Phase 1.1) | Open |
| 10 | Repair facade allow-list | Phase 4 | Before PR C | Deferred |
| 11 | `timeline_scan` decision | Phase 4 | Spike in PR C | Deferred |
| 12 | Repair algorithms / policies | Phase 4–5 | Follow-up spec | Deferred |
| 13 | Repair config loader choice | Phase 4 | PR C planning | Deferred |
| 14 | Repair exit codes in docs | Phase 4.8 | PR C | Deferred |

When an item is resolved, update **Status** here and optionally fold the resolution into [TEMP-workspace-refactor-plan.md](TEMP-workspace-refactor-plan.md) so the main plan stays the single source of truth.
