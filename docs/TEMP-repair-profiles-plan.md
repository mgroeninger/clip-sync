# Temporary plan: repair profiles (`default` / `quick` / `full`)

> **Status:** Phase 1–2 **in progress** (2026-06-22). Baseline-only marginal skip, profile enum, CLI `--quick`/`--full`/`--profile`, TOML `profile` key shipped. Phase 3 verbose diagnostics partial (profile line + per-gap fit path). Phase 4 docs open.
>
> Archive to `docs/archive/repair-profiles-plan.md` when shipped. Update [README.md](../README.md), [gap-fill-modes.md](gap-fill-modes.md), and [cli-output.md](cli-output.md) in the docs phase.

**Problem:** Fit-mode patching on long-form pairs can take **hours** when every gap lands in the marginal band (`pre` weak, `post` strong). The seam gate runs a **full joint A-boundary grid** (~13×13 cells × unified B search × long haystack) even when the **baseline** placement is already marginal-but-acceptable. Users cannot get a listenable mux without committing to a multi-hour run. There is no CLI vocabulary for “draft” vs “quality” repair.

**Observed (Red Sonja, 7 fill regions, no TOML):** ~5–6 min/gap in one run; ~30 min/gap in another with the same marginal outcomes — dominated by boundary grid cost, not offset mode. `fill_offset_mode = recommended`; `anchored_retry` off; clip drift **0.012 s** (below interpolated threshold).

**Goal:**

1. **`default` profile** — interactive repair: patch marginal gaps from **baseline** unified search; skip boundary grid when baseline tier is acceptable.
2. **`--quick` profile** — faster draft: smaller haystack, no extension flags (bundle only).
3. **`--full` profile** — today’s slow path: full grid + extension; opt-in quality pass.
4. Profiles set a **baseline bundle**; explicit CLI flags and TOML keys **override** individual fields (profiles are not a straitjacket).
5. Verbose logging: effective profile, overrides, and per-gap path (`baseline` / `boundary grid`).

**Non-goals (v1):**

- Persisting profiles across invocations or per-gap profile mixing.
- Auto-selecting `anchored_retry` in `default` / `quick` (defer gated behavior to `full` only).
- Changing align/scan phases.
- `--strict-profile` (reject conflicting overrides) — defer unless CI needs it.
- Cap/extract limits for very long gaps (e.g. 211 s) — separate follow-up; document risk in `full`.

---

## Current codebase baseline

| Area | Path | Current behavior |
|------|------|------------------|
| Fit seam gate | `application/patch_region.rs` | `evaluate_seam_gate_fit_joint`: baseline → if not **High**, run full boundary grid; early exit only on **High**; accept **Marginal** after grid exhaust |
| Patch loop | `application/patch_audio.rs` | Pass 1 all gaps; `anchored_retry` optional pass 2 |
| Config defaults | `infrastructure/config.rs` | `fill_border_search_secs = 10`, extension on, `fill_offset_mode = recommended` |
| CLI overrides | `infrastructure/cli/mod.rs` | `apply_cli_overrides` — per-flag patches after config load |
| Performance recipes | `docs/gap-fill-modes.md` | Documents manual flag combos; no `profile` key |
| Anchored retry | `domain/fill_offset.rs`, `patch_audio.rs` | Two-pass; anchors from **High** only; see [archive/patch-anchor-offset-plan.md](archive/patch-anchor-offset-plan.md) |

### Fit slow path (today)

```text
baseline unified search
  → confidence High?  done (fast)
  → else: joint grid over A start/end (extension axes)
       each cell: unified search on full B haystack
       early exit only on High
  → best Marginal or Err
```

**Cost driver:** marginal baseline ⇒ full grid ⇒ ~170× unified search per gap × haystack length.

---

## Decisions

| Topic | Decision |
|-------|----------|
| **Profile enum** | `default` \| `quick` \| `full` on `[repair]`; CLI `--quick`, `--full`, optional `--profile <name>` |
| **Precedence** | Explicit CLI flag > TOML `[repair]` key > profile bundle > `RepairConfig` struct defaults |
| **Override policy** | Profiles **do not** disable other settings; log effective config in verbose |
| **`--quick` + `--full`** | Error: `cannot use --quick and --full together` |
| **Default profile name** | `default` (implicit when no profile flag) |
| **New fit knob** | `fit_boundary_search: baseline_only \| full_grid` (internal or config); driven by profile |
| **Baseline marginal** | When `fit_boundary_search = baseline_only` and baseline `confidence == Marginal` (above `fill_absolute_floor`), **skip grid**, return baseline |
| **Baseline fail** | Below absolute floor → skip patch (unchanged); in `full`, may still try grid |
| **`quick` bundle** | `fill_border_search_secs = 5`, `gap_*_extend_on_* = false`, `fit_boundary_search = baseline_only` |
| **`full` bundle** | `fill_border_search_secs = 10`, extension on, `fit_boundary_search = full_grid`; optional future: auto `anchored_retry` |
| **`default` bundle** | `fill_border_search_secs = 10`, extension on (for `full` grid when needed), `fit_boundary_search = baseline_only` |
| **`interpolated` offset** | Not part of profile bundles; users override when clip drift ≥ 0.05 s |
| **JSON report** | Add `repair_profile: "default"\|"quick"\|"full"` and `fit_boundary_search` on patch summary (optional v1.1) |

### Profile matrix (effective defaults after apply)

| Setting | `default` | `quick` | `full` |
|---------|-------------|---------|--------|
| `fill_border_search_secs` | 10 | **5** | 10 |
| `gap_end_extend_on_post_seam_fail` | true* | **false** | true |
| `gap_start_extend_on_pre_seam_fail` | true* | **false** | true |
| `fit_boundary_search` | **baseline_only** | **baseline_only** | **full_grid** |
| `fill_offset_mode` | recommended | recommended | recommended |
| Expected per-gap time (marginal 1–3 s gaps) | minutes | sub-minute – few min | tens of minutes |

\*Extension flags only affect work when `fit_boundary_search = full_grid`; under `baseline_only` they are inert but remain true so `--full` can be expressed as a profile flip without re-toggling extension defaults.

---

## Architecture

```text
load config → resolve profile (CLI > TOML > default)
           → apply profile bundle to RepairConfig
           → apply_cli_overrides (explicit flags win)
           → verbose: log effective profile + overrides
           → patch loop (unchanged outer stages)
                → evaluate_seam_gate_fit_joint
                     → baseline candidate
                     → if baseline_only && Marginal|High → return
                     → if full_grid && !High → boundary grid …
```

### Verbose lines (new)

```text
repair profile: default (fit_boundary_search=baseline_only, fill_border_search_secs=10.0)
repair profile: quick (+ CLI override: fill_border_search_secs=8.0)
           fit path: baseline only (marginal, pre=0.31 post=1.00)
           fit path: boundary grid (143 cells, haystack 36.0s)   # full only
```

Optional: record `fit_path` on `tracing` span `patch_gap` (`boundary_grid = false|true`, `grid_cells` when true).

---

## Phases

### Phase 1 — Fit behavior: baseline-only marginal skip

**Intent:** Ship the main default behavior change without CLI profiles yet.

- [ ] Add `FitBoundarySearch` enum: `BaselineOnly` | `FullGrid` (domain or config).
- [ ] Thread through `SeamGateParams` / `PatchAudioRequest`.
- [ ] In `evaluate_seam_gate_fit_joint` (`patch_region.rs`):
  - After baseline record: if `BaselineOnly` && `confidence ∈ {High, Marginal}` → return baseline (keep marginal warn).
  - If `BaselineOnly` && below absolute floor → `Err` (no grid).
  - If `FullGrid` → existing grid logic unchanged.
- [ ] Default `RepairConfig`: `fit_boundary_search = BaselineOnly`.
- [ ] Unit tests in `patch_region.rs` or `patch_audio.rs`:
  - Marginal baseline ⇒ no grid invocation (mock or cell-count assertion).
  - `FullGrid` + marginal baseline ⇒ grid still runs.
- [ ] Integration: existing fit tests still pass; add one test with `FullGrid` mirroring pre-change behavior if needed.

**Acceptance:** Red Sonja–shaped synthetic (marginal baseline, 1 s gap) completes patch phase in **&lt;2×** baseline unified search time vs `FullGrid`.

### Phase 2 — Profile resolution + CLI

**Intent:** `--quick`, `--full`, TOML `profile = "…"`, precedence, conflict error.

- [ ] `RepairProfile` enum + `apply_repair_profile(config: &mut RepairConfig, profile)`.
- [ ] `[repair] profile = "default"|"quick"|"full"` in config (default `"default"`).
- [ ] CLI: `--quick` → `Quick`; `--full` → `Full`; `--profile <name>` (optional explicit).
- [ ] `apply_cli_overrides` order:
  1. Load TOML
  2. `apply_repair_profile` from TOML `profile` unless CLI profile set
  3. CLI profile flag if present
  4. Existing per-flag overrides
- [ ] Reject `--quick` && `--full`.
- [ ] Tests: `apply_cli_overrides` profile + override precedence; conflict error.

### Phase 3 — Verbose diagnostics

**Intent:** User can see why a run is fast/slow without reading code.

- [ ] `format_repair_profile_verbose(config, profile, overrides)` in `cli/output.rs` or `patch_audio.rs`.
- [ ] Emit once at start of patch phase (after fill plan).
- [ ] Per-gap: `fit path: baseline only` vs `boundary grid (N cells, haystack Xs)` in `evaluate_seam_gate_fit_joint` via `ProgressReporter::phase_verbose` callback or progress param.
- [ ] Update [cli-output.md](cli-output.md) verbose section.

### Phase 4 — Docs + README

- [ ] [gap-fill-modes.md](gap-fill-modes.md): replace manual “interactive / faster fit” recipe with profile table; keep flag overrides documented.
- [ ] [README.md](../README.md): `clip-sync-repair ... --mux out.mp4` (default), `--quick`, `--full` examples.
- [ ] Note: `anchored_retry` not in profiles; use `--fill-offset anchored-retry` override on `full` runs when needed.

### Phase 5 (optional) — `full` profile + anchored-retry gating

**Intent:** Quality pass may auto-enable second pass when it helps.

- [ ] After pass 1, if profile is `full` && `skipped_count > 0` && `high_anchor_count > 0` → run `anchored_retry` pass 2 (even if `fill_offset_mode` still `recommended`)? **Or** only document that users should add `--fill-offset anchored-retry` on `full` runs.
- [ ] Gate: `|start_offset - end_offset| >= 0.05` (reuse `MIN_DRIFT_FOR_INTERPOLATION_SECS`) as hint in verbose (“consider anchored-retry”).
- [ ] Defer auto-enable until corpus proves benefit; Phase 5 default deliverable = **verbose hint only**.

---

## Config / CLI surface

### New config keys

```toml
[repair]
profile = "default"   # default | quick | full

# Advanced (set by profile; overridable)
fit_boundary_search = "baseline_only"   # baseline_only | full_grid
```

### New CLI flags

| Flag | Effect |
|------|--------|
| `--quick` | `profile = quick` |
| `--full` | `profile = full` |
| `--profile <name>` | Explicit profile (for scripts/TOML parity) |

Existing flags (`--fill-border-search-secs`, `--no-gap-end-extend`, etc.) continue to override profile fields.

### Example commands

```powershell
# Interactive default (new behavior)
clip-sync-repair a.mkv b.mkv --mux out.mp4 -v

# Draft / first listen
clip-sync-repair a.mkv b.mkv --mux draft.mp4 --quick -v

# Quality pass (legacy CPU cost)
clip-sync-repair a.mkv b.mkv --mux best.mp4 --full -v

# Quick + one override
clip-sync-repair a.mkv b.mkv --mux out.mp4 --quick --fill-border-search-secs 8 -v
```

---

## Tests

| ID | Layer | Asserts |
|----|-------|---------|
| P1-U1 | unit | `BaselineOnly` + marginal baseline skips grid |
| P1-U2 | unit | `FullGrid` + marginal baseline runs grid |
| P1-U3 | unit | `BaselineOnly` + below absolute floor → skip, no grid |
| P2-U1 | cli | `--quick` sets border 5, extension off, baseline_only |
| P2-U2 | cli | `--full` sets full_grid + extension on |
| P2-U3 | cli | `--quick --fill-border-search-secs 8` → 8 |
| P2-U4 | cli | `--quick --full` → error |
| P2-U5 | cli | TOML `profile=quick` + no CLI → quick bundle |
| P3-I1 | integration | Patch with default profile on drift fixture completes; marginal gaps patched |
| P3-I2 | integration | `--full` on fixture that **needs** grid still passes (existing joint-extension test or new) |

---

## Risks and mitigations

| Risk | Mitigation |
|------|------------|
| Default skips grid; miss better A-bracket | `--full` restores today; document in README |
| `quick` border 5 misses edge-clamped fills | Verbose haystack line; user raises border or uses `--full` |
| Users expect `--quick` to block overrides | Document precedence; verbose lists overrides |
| Long gaps (211 s) still slow even baseline-only | Document; future max-gap extract cap |
| CI slowdown if tests use `FullGrid` everywhere | Tests default `BaselineOnly`; explicit `FullGrid` only where grid behavior is under test |

---

## Manual validation (post-ship)

- [ ] Red Sonja (or similar): `--quick --mux draft.mp4` completes patch phase in **&lt;15 min** for 6× short gaps.
- [ ] Same material `--full` on gap #4 only (if per-gap repair exists) or full run — compare seam/slide vs default.
- [ ] Confirm default mux is listenable; note speech-onset marginals unchanged vs full grid if applicable.

---

## References

- [gap-fill-modes.md](gap-fill-modes.md) — fit slow path, performance recipes
- [archive/patch-anchor-offset-plan.md](archive/patch-anchor-offset-plan.md) — `anchored_retry`
- `application/patch_region.rs` — `evaluate_seam_gate_fit_joint`
- `application/patch_audio.rs` — patch loop, `prepare_region_patch`
- `infrastructure/config.rs` — `RepairConfig`
- `infrastructure/cli/mod.rs` — `apply_cli_overrides`
- Conversation context: marginal `post=1.0` pattern, repeat-penalty fix, sub-second verbose timestamps
