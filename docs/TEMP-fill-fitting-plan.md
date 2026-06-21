# Temporary plan: gap fill — gate → fit transition

> **Status:** Draft (2026-06-20). Phase A shipped; default `fill_mode` is **`fit`** (2026-06-20).
>
> Archive to `docs/archive/fill-fitting-plan.md` when all four phases ship (or split into shipped + backlog if Phase D defers).

**Problem:** Gap patching today is **search-then-gate**. Structure match *fits* B placement in a haystack (`search_best_*`, `fine_polish_structure_start`), but waveform Pearson is evaluated at **one** candidate and compared to fixed thresholds. `max_fill_align_adjustment_secs` mostly limits **structure** polish — verbose `structure slide` is not a waveform seam search. Failed waveform checks trigger reactive A-boundary extension; success still allows patches where only one seam correlates.

**Goal:** Treat each gap as a **local optimization** — search for `(gap on A, start on B)` that minimizes seam discontinuity — and use thresholds only as **floors** and **warn tiers**, not as the primary decision. Preserve today’s behavior behind `fill_mode = "gate"` for regression tests until fitting is default.

**Non-goals (v1 across phases):** Full GCC-PHAT alignment inside every gap (optional Phase B+ hook only); changing gap **scan** detection; resampling B fill (pitch-preserving trim/pad stays).

---

## Current codebase baseline

| Area | Path | Current state | First phase touched |
|------|------|---------------|---------------------|
| Per-gap orchestration | `application/patch_audio.rs` | `prepare_region_patch` → `evaluate_seam_gate` → splice | A |
| Seam gate | `application/patch_region.rs` | Structure match → threshold gates → single-point waveform Pearson | A, B |
| Structure search | `domain/gap_structure.rs` | Coarse + refine search; `fine_polish_structure_start` | B |
| Seam scoring | `domain/policies.rs` | `fill_seam_correlations`, `border_templates_for_gap`, `apply_seam_crossfade` | A |
| A edge tighten | `domain/policies.rs` | `refine_gap_frames` | C |
| Boundary retry | `application/patch_region.rs` | `try_extend_gap_end_for_post_seam`, `try_extend_gap_start_for_pre_seam` | C |
| Fill length | `application/patch_audio.rs` | Pre-border anchor trim; contiguous B extend when short | D |
| Outcomes | `domain/patch_result.rs` | `GapPatchStatus::Patched` / `Skipped` only | C |
| Config | `infrastructure/config.rs` | `min_fill_correlation`, `max_fill_align_adjustment_secs`, structure-trust flags | A–D |
| CLI output | `infrastructure/cli/output.rs`, `docs/cli-output.md` | `slide=`, struct pre/post; verbose B timeline (`patch_audio.rs`) | A, C |
| Integration tests | `tests/patch_audio_integration.rs` | Gate-focused fixtures (extension, structure trust) | All |

### Gating vs fitting today

```text
refine_gap_frames (fit A edges, small)
    → map gap to B (offset)
    → structure search (fit B start/end in haystack)
    → structure_passes_gate?  ──no──→ skip
    → waveform at ONE start_frame
    → structure_trusted? → skip waveform
    → seams_pass_correlation_gate? ──no──→ extend A boundary, retry
    → splice + crossfade
```

**Fitting target:**

```text
refine_gap_frames (+ optional joint A-boundary search in Phase C)
    → map gap to B
    → candidate generator (structure positions ± waveform slide grid)
    → score each candidate (unified objective)
    → pick argmax; apply floor / warn tier
    → splice at best (+ dual trim/anchor in Phase D)
```

---

## Decisions

| Topic | Decision |
|-------|----------|
| **Mode switch** | Add `fill_mode: Gate \| Fit` on `[repair]` (default **`fit`**). CLI: `--fill-mode gate\|fit` for legacy. |
| **Objective (default)** | **Phase A:** maximize `min(pre, post)` waveform Pearson over slide grid. **Phase B:** `α·structure_combined + β·min(pre,post) − γ·\|slide\| − δ·length_penalty`. |
| **Search radius** | Reuse `max_fill_align_adjustment_secs` → frames for waveform slide; structure keeps `fill_border_search_secs` coarse radius. |
| **Floors** | Keep `min_structure_match_score` and `min_fill_correlation` as **minimum acceptable best score**, not per-fixed-point checks. |
| **Structure trust** | In **`fit`** mode: never skip waveform measurement; strong structure adds weight (`α`) or lowers floor slightly — does **not** bypass scoring. `disable_structure_trust` ≡ high `β`, no soften shortcuts. |
| **Short-gap shortcuts** | In **`fit`** mode: disable `short_gap_one_strong_seam_fallback` and mean-only pass — objective already uses mean or `min`. `gate` mode unchanged. |
| **Slide reporting** | Verbose: `structure slide`, `waveform slide`, `combined score`; stdout patch line includes both slides when diagnostics on. |
| **Skip reasons** | Keep existing `CorrelationBelowThreshold` when **best** candidate is below floor. Phase C adds `PatchedMarginal` status or `patched_marginal: bool` + stderr warn. |
| **Layering** | New scoring/search in `domain/` (`gap_fill_fit.rs` or extend `gap_structure.rs`); `patch_region.rs` orchestrates; no ffmpeg changes. |

---

## Phase A — Waveform slide argmax (after structure)

**Intent:** After structure proposes `alignment.start_frame`, scan `start ± max_fine_adjustment_frames` and pick the offset that **maximizes** `min(pre_corr, post_corr)` using existing `fill_seam_correlations`. Gate only if the **best** score is below `min_fill_correlation`.

### Implementation

- [x] Add `domain/gap_fill_fit.rs` (or `policies::search_best_waveform_placement`):
  - Input: `SeamTemplates`, `SeamPlacement` nominal, `gap_frames`, `max_adjustment_frames`, `seam_gate_frames`.
  - Loop `delta` in `[-max, +max]` (step 1 frame, or coarser step for long gaps with refine pass).
  - Track best `(start, pre, post, score)` with `score = min(pre, post)`.
  - Return `FillAlignment` with updated `start_frame`, `pre_correlation`, `post_correlation`.
- [x] `evaluate_seam_gate` (`patch_region.rs`):
  - After structure match + structure floor check, call waveform search **unless** `fill_mode == Gate` and structure_trusted (legacy path).
  - In `fit` mode: always run waveform search; set `structure_trusted = false` on output (waveform always informed decision).
- [x] `RegionPatchOutcome` / verbose: expose `waveform_slide_secs` separately from structure slide (`align_adjustment_secs` today mixes structure polish — split fields in `Patched` or add `waveform_adjustment_secs`).
- [x] Config + CLI: `fill_mode` enum (default `fit`).
- [x] Tests:
  - Unit: synthetic B haystack where true offset is +3 frames from structure nominal; search finds it.
  - Unit: gate mode still skips when only structure passes (regression).
  - Integration: existing `patch_audio_integration` corpus case that required extension — expect **fit** to patch without extension or with smaller extension.

### Acceptance

- On licensed-media–like material, materially fewer `CorrelationBelowThreshold` skips without lowering `min_fill_correlation`.
- No increase in patches where `min(pre, post) < min_fill_correlation` on best candidate.
- Verbose shows `waveform slide` when non-zero.

### Docs (Phase A)

- [x] `docs/cli-output.md` — verbose gap lines: `waveform slide`.
- [x] `README.md` § Gap patching — one paragraph on `fill_mode`.

---

## Phase B — Unified scorer in structure search loop

**Intent:** Stop picking structure winner then vetoing with waveform. Score **each** structure candidate (coarse grid + fine polish positions) with a combined cost so structure and waveform agree on the same optimum.

### Implementation

- [x] Extend structure search with unified waveform scoring in `match_gap_fill_unified_in_b` (`gap_fill_fit.rs`); reuse `gap_structure` scoring helpers.
- [x] Per candidate: `combined = α·structure_combined + β·min(wave_pre, wave_post)` with late-start penalty; defaults `α = 0.35`, `β = 0.65`.
- [x] Config: `fill_fit_structure_weight`, `fill_fit_waveform_weight` on `[repair]` (no CLI yet).
- [x] Remove redundant waveform-only pass in `evaluate_seam_gate` when `fill_mode == Fit` (unified search includes waveform).
- [x] Structure floor + `min(wave_pre, wave_post) >= min_fill_correlation` on winner unchanged.
- [x] Tests: `unified_fit_score_favors_waveform_when_structure_differs_slightly`; gate regression via existing `patch_audio_integration` (`fill_mode = Gate`).

### Acceptance

- Wrong structural cycle (repeat-at-seam scenario) reduced on corpus matrix vs Phase A-only.
- Search time per gap stays bounded (profile: p95 &lt; 2× Phase A on 15 min material).

---

## Phase C — Joint A-boundary search + tiered patch / warn / skip

**Intent:** Promote gap-end/start **extension retries** from failure recovery to proactive fit. Add a **marginal** tier so gray-zone fills patch with a visible warning instead of skipping.

### Implementation

- [x] Joint search (`evaluate_seam_gate_fit_joint`): outer A start/end grid (reuse `gap_end_extend_*`); inner Phase B unified B placement; baseline fast-path when `high`.
- [x] Replace `try_extend_gap_*` retry loops in `fit` mode (gate mode unchanged).
- [x] Tiering via `classify_fill_waveform_confidence`: `high` / `marginal` / skip below `fill_absolute_floor`.
- [x] Config: `fill_marginal_margin` (0.08), `fill_absolute_floor` (0.12).
- [x] `GapPatchStatus::Patched`: `confidence`, `gap_start_adjust_frames`, `gap_end_adjust_frames`; `PatchSummary.patched_marginal_count`.
- [x] CLI: `! patched` / header `(N marginal)`; `tracing::warn` on marginal.
- [x] Tests: tier unit tests; golden JSON updated.

### Acceptance

- Fewer skips on borderline licensed media gaps; user sees explicit **marginal** flag.
- Alignment instability warning can cite marginal gap numbers.

### Docs (Phase C)

- [ ] `docs/cli-output.md` — marginal patch row, warn tier table.
- [ ] `docs/json-output.md` — new fields.

---

## Phase D — Repeat penalty + dual trim / anchor

**Intent:** Reduce **audible repeats** at gap start/end by penalizing duplicate content in the objective and choosing trim/extend strategy by score, not fixed pre-border anchor.

### Implementation

- [ ] **Duplicate penalty** in unified scorer:
  - `repeat_pre = corr(A_pre_border, B_fill_head)` and `repeat_post = corr(A_post_border, B_fill_tail)` over windows **outside** crossfade length (e.g. `normalize_window_secs`).
  - Penalize when `repeat_pre` or `repeat_post` is high **and** `min(wave_pre, wave_post)` is low — or when `repeat_pre + repeat_post` exceeds ceiling while seams disagree (tune to avoid punishing legitimate continuity).
  - Config: `fill_repeat_penalty_weight` (default TBD from corpus).
- [ ] **Dual anchor** when `source_frames != gap_frames`:
  - Today: always trim tail (`fit_fill_to_gap_frames` + pre-border anchor in `patch_audio.rs`).
  - Fit: build two candidates — **anchor start** (trim tail) and **anchor end** (trim head) — pick higher combined score after waveform re-check.
  - Contiguous B extend when short: try extend only up to score drop; zero-pad tail if extend would raise repeat_post.
- [ ] Optional (stretch): localized GCC-PHAT on border templates in B haystack when unified score ambiguous (top two within ε) — reuse `clip-sync` `pcm_search_near_offset` / `cross_correlate` behind feature flag `fill_fit_pcm_refine`.
- [ ] Tests:
  - Synthetic: wrong-cycle structure match scores lower than correct cycle when repeat penalty on.
  - Trim vs anchor-end: gap shorter than B bracket, correct content at tail.
  - Integration: no regression on `fit_fill_trims_tail_without_resampling` behavior in gate mode.

### Acceptance

- Repeat-at-seam reports down on manual licensed media listen pass (subjective checklist in corpus-validation).
- Patch rate not materially lower than Phase C.

---

## Config surface (cumulative)

| Key | Phase | Default | Notes |
|-----|-------|---------|-------|
| `fill_mode` | A | `fit` (default); `gate` = legacy | |
| `fill_marginal_margin` | C | `0.08` | Warn-patch band below `min_fill_correlation` |
| `fill_absolute_floor` | C | `0.12` | Hard skip below this |
| `fill_fit_structure_weight` | B | `0.35` | Optional expose |
| `fill_fit_waveform_weight` | B | `0.65` | Optional expose |
| `fill_repeat_penalty_weight` | D | TBD | 0 = off for A/B/C |
| `fill_fit_pcm_refine` | D | `false` | Narrow GCC-PHAT tie-break |

Existing keys retain meaning: `min_fill_correlation`, `min_structure_match_score`, `max_fill_align_adjustment_secs`, `fill_border_search_secs`, `gap_end_extend_*`, `crossfade_ms`.

---

## Testing strategy (all phases)

| Layer | What |
|-------|------|
| Unit | `gap_fill_fit` slide argmax; unified score tie-break; repeat penalty algebra; dual-anchor length fit |
| Lib | `patch_region` gate vs fit branches; `seams_pass_correlation_gate` unchanged under `gate` |
| Integration | `patch_audio_integration.rs` — duplicate each critical test under `fill_mode: Fit` |
| Corpus | `corpus-validation` matrix row for repair patch; before/after skip counts + marginal count |
| Manual | licensed media pair: `-v` compare `B fill source` / slides for patched vs marginal gaps |

**CI:** Run patch integration tests in both modes until `gate` is removed.

---

## Rollout

1. **Phase A** — shipped; default `fit` (2026-06-20).
2. **Phase B** — unified search only in `fit` mode (shipped 2026-06-20).
3. **Phase C** — joint A-boundary search + marginal tier (shipped 2026-06-20).
4. **Phase D** — repeat penalty + dual anchor.
5. Archive this doc; update `PLAN.md` repair section; add `BACKLOG.md` row.

---

## Risks and mitigations

| Risk | Mitigation |
|------|------------|
| Slower patch pass | Coarse step + cap candidates; profile `patch-gap` label |
| Over-patching bad fills | `fill_absolute_floor`; marginal warns; `--fill-mode gate` rollback |
| Repeat penalty rejects good fills | Weight starts at 0; corpus A/B; disable via config |
| JSON / CLI contract drift | Phase C schema bump documented in `json-output.md` |

---

## Related reading

- [README.md](../README.md) § Gap patching pipeline
- [docs/cli-output.md](cli-output.md) § Gap patch gate and skip reasons
- [docs/archive/clip-self-repetition-plan.md](archive/clip-self-repetition-plan.md) — alignment-time repetition (orthogonal; gap-fill repeat penalty may share helpers)
- Verbose B timeline: `application/patch_audio.rs` (`format_gap_fill_plan_lines`)

---

## Open questions

1. Default objective: `min(pre, post)` vs harmonic mean — bias toward balanced seams?
2. Should marginal patches count as `repaired` or `repaired (marginal)` in the gap table header totals?
3. Phase D GCC-PHAT: worth dependency on `clip-sync` offset_refinement from repair domain, or duplicate slim correlator in repair?
4. When to delete `gate` mode entirely — after one release with `fit` default or keep indefinitely for tests?
