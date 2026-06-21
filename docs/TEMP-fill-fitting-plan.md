# Temporary plan: gap fill — gate → fit transition

> **Status:** Phases A–D **code-complete** (2026-06-20). Default `fill_mode` is **`fit`**. Repeat penalty ships **off** (`fill_repeat_penalty_weight = 0`). Corpus / manual acceptance **not yet recorded**.
>
> Archive to `docs/archive/fill-fitting-plan.md` after external/manual repair acceptance (see [corpus-validation.md](corpus-validation.md) § Gap fill) or split shipped vs backlog.

**Problem:** Gap patching today is **search-then-gate**. Structure match *fits* B placement in a haystack (`search_best_*`, `fine_polish_structure_start`), but waveform Pearson is evaluated at **one** candidate and compared to fixed thresholds. `max_fill_align_adjustment_secs` mostly limits **structure** polish — verbose `structure slide` is not a waveform seam search. Failed waveform checks trigger reactive A-boundary extension; success still allows patches where only one seam correlates.

**Goal:** Treat each gap as a **local optimization** — search for `(gap on A, start on B)` that minimizes seam discontinuity — and use thresholds only as **floors** and **warn tiers**, not as the primary decision. Preserve today’s behavior behind `fill_mode = "gate"` for regression tests until fitting is default.

**Non-goals (v1 across phases):** Full GCC-PHAT alignment inside every gap (optional Phase D hook only); changing gap **scan** detection; resampling B fill (pitch-preserving trim/pad stays).

---

## Current codebase baseline

| Area | Path | Current state | First phase touched |
|------|------|---------------|---------------------|
| Per-gap orchestration | `application/patch_audio.rs` | `prepare_region_patch` → `evaluate_seam_gate` → splice; fit dual-anchor trim | A, D |
| Seam gate | `application/patch_region.rs` | Fit: joint grid + unified search; gate: legacy thresholds | A–C |
| Structure search | `domain/gap_structure.rs` | Coarse + refine; `ActivityTimeline` reused in joint grid | B, perf |
| Unified fit | `domain/gap_fill_fit.rs` | Unified scorer, repeat penalty, dual-anchor pick | B, D |
| Seam scoring | `domain/policies.rs` | `fill_seam_correlations`, `fill_repeat_correlations`, `fill_splice_seam_correlations` | A, D |
| A edge tighten | `domain/policies.rs` | `refine_gap_frames` | C |
| Boundary retry | `application/patch_region.rs` | Fit: proactive joint grid; gate: `try_extend_gap_*` | C |
| Fill length | `application/patch_audio.rs` | Fit: `pick_fill_length_anchor`; gate: trim tail | D |
| Outcomes | `domain/patch_result.rs` | `Patched` + `confidence`; `patched_marginal_count` | C |
| Config | `infrastructure/config.rs` | Fit weights, tiers, `fill_repeat_penalty_weight`, `fill_border_search_secs` default 10 | A–D |
| CLI output | `infrastructure/cli/output.rs`, `docs/cli-output.md` | Slides, marginal `!`, JSON fields | A, C |
| Integration tests | `tests/patch_audio_integration.rs` | Gate + fit paths (extension, marginal, structure trust) | All |

### Gating vs fitting today

```text
refine_gap_frames (fit A edges, small)
    → map gap to B
    → structure search (fit B start/end in haystack)
    → structure_passes_gate?  ──no──→ skip
    → waveform at ONE start_frame
    → structure_trusted? → skip waveform
    → seams_pass_correlation_gate? ──no──→ extend A boundary, retry
    → splice + crossfade
```

**Fitting target (shipped):**

```text
refine_gap_frames (+ joint A-boundary grid in fit mode)
    → map gap to B
    → unified structure+waveform search (± repeat penalty in scorer)
    → pick argmax; apply floor / warn tier
    → splice at best (+ dual trim/anchor in fit when B bracket > gap)
```

---

## Decisions

| Topic | Decision |
|-------|----------|
| **Mode switch** | `fill_mode: Gate \| Fit` on `[repair]` (default **`fit`**). CLI: `--fill-mode gate\|fit`. |
| **Objective (default)** | Unified: `α·structure_combined + β·min(pre,post) − late_start − repeat_penalty`. |
| **Search radius** | Fit B slide: `fill_border_search_secs` (default **10 s**). Structure polish capped in `gap_structure::structure_fine_polish_frames`. |
| **Floors** | `min_structure_match_score` + `min_fill_correlation` on **winner**; `fill_absolute_floor` hard skip. |
| **Structure trust** | **`fit`:** never bypass waveform; `structure_trusted = false` on output. **`gate`:** legacy. |
| **Short-gap shortcuts** | Disabled in **`fit`**; unchanged in **`gate`**. |
| **Slide reporting** | `structure slide`, `waveform slide`, `align_adjustment_secs` sum. |
| **Skip reasons** | Below `fill_absolute_floor`; marginal patches with `confidence: marginal` + warn. |
| **Layering** | `domain/gap_fill_fit.rs`; `patch_region.rs` orchestrates. |

---

## Performance pass (2026-06-20, post Phase C)

**Intent:** Cut combinatorial cost when baseline is not High (~13×13 joint grid × unified search).

### Implementation

- [x] `FitHaystackCache`: reuse `b_mono`, `b_ch`, `ActivityTimeline` across joint-grid cells.
- [x] `match_gap_fill_unified_in_b_with_timeline` (crate-private) for pre-built timeline.
- [x] Joint grid early exit when a **High** candidate is found.
- [x] Default `fill_border_search_secs`: **30 → 10** (config override still valid).
- [x] `effective_fill_absolute_floor` follows lowered `min_fill_correlation` in tests.

### Acceptance

| Criterion | Status |
|-----------|--------|
| No infinite loops / unbounded grids | **Verified** (unit + integration bounds) |
| Typical CLI patch pass completes in minutes not hours | **Unverified** (needs production-like re-run on long media) |
| p95 per-gap search &lt; 2× pre-unified baseline | **Unverified** (no profile capture in CI) |

---

## Phase A — Waveform slide argmax (after structure)

**Intent:** After structure proposes `alignment.start_frame`, scan `start ± max_fine_adjustment_frames` and pick the offset that **maximizes** `min(pre_corr, post_corr)`.

### Implementation

- [x] `domain/gap_fill_fit.rs` — `search_best_waveform_placement`.
- [x] `evaluate_seam_gate` — fit vs gate branches.
- [x] `waveform_adjustment_secs` on `Patched`.
- [x] Config + CLI: `fill_mode` (default `fit`).
- [x] Unit + integration tests.

### Acceptance

| Criterion | Status |
|-----------|--------|
| Fewer `CorrelationBelowThreshold` skips on long-form repair material | **Unverified** (manual) |
| No increase in patches where best `min(pre, post) < min_fill_correlation` | **Partial** (floor + tier logic unit-tested; no corpus diff) |
| Verbose shows `waveform slide` when non-zero | **Verified** (cli-output + integration) |

### Docs

- [x] `docs/cli-output.md` — `waveform slide`.
- [x] `README.md` § Gap patching.

---

## Phase B — Unified scorer in structure search loop

**Intent:** Score **each** structure candidate with combined structure + waveform objective.

### Implementation

- [x] `match_gap_fill_unified_in_b` / `_with_timeline`.
- [x] Defaults `α = 0.35`, `β = 0.65`; late-start penalty.
- [x] Config: `fill_fit_structure_weight`, `fill_fit_waveform_weight`.
- [x] CLI: `--fill-fit-structure-weight`, `--fill-fit-waveform-weight`.
- [x] No redundant waveform-only pass in fit mode.
- [x] Tests: `unified_fit_score_favors_waveform_when_structure_differs_slightly`; gate regression in integration.

### Acceptance

| Criterion | Status |
|-----------|--------|
| Wrong structural cycle reduced vs Phase A-only | **Unverified** (no repeat-at-seam corpus row) |
| Search time per gap bounded (p95 &lt; 2× Phase A) | **Partial** (perf pass landed; not profiled on 15 min material) |

---

## Phase C — Joint A-boundary search + tiered patch / warn / skip

**Intent:** Proactive A-boundary grid + marginal warn tier.

### Implementation

- [x] `evaluate_seam_gate_fit_joint` + baseline High fast-path.
- [x] `classify_fill_waveform_confidence`: high / marginal / skip.
- [x] Config: `fill_marginal_margin`, `fill_absolute_floor`.
- [x] JSON + CLI: `confidence`, `gap_*_adjust_frames`, `patched_marginal_count`, `! patched`.
- [x] Fit integration tests (extension, marginal) with `fast_fit_patch_options()`.

### Acceptance

| Criterion | Status |
|-----------|--------|
| Fewer skips on borderline gaps (marginal tier) | **Unverified** (manual) |
| User sees explicit **marginal** flag | **Verified** (CLI + JSON + tests) |
| Instability warning cites marginal counts | **Unverified** (warning text not tied to marginal count yet) |

### Docs

- [x] `docs/cli-output.md` — marginal row, tier table.
- [x] `docs/json-output.md` — new fields.
- [x] `docs/gap-fill-modes.md` — fit vs gate matrix, performance recipes.

---

## Phase D — Repeat penalty + dual trim / anchor

**Intent:** Penalize duplicate A-border content in the unified scorer; pick trim head vs tail by seam score when B bracket ≠ gap length.

### Implementation

- [x] **Duplicate penalty** in unified scorer (`fill_repeat_correlations`, `repeat_penalty_at_placement`).
  - Window: `border_frames` (from `normalize_window_secs`).
  - Penalize when repeat high **and** `min(wave_pre, wave_post)` weak, or repeat sum high with weak seams.
  - Config: `fill_repeat_penalty_weight` (default **`0.0`** = off).
- [x] **Dual anchor** (fit only): `pick_fill_length_anchor` — trim tail vs trim head via `fill_splice_seam_correlations`; gate still trim tail.
- [ ] **Score-based B extend** when bracket short: extend only up to score drop; zero-pad if extend raises `repeat_post`.
- [ ] **Optional:** `fill_fit_pcm_refine` — GCC-PHAT tie-break when top two within ε.
- [x] Tests:
  - [x] Repeat penalty algebra (`repeat_penalty_downranks_duplicate_fill_when_seams_weak`).
  - [x] Dual anchor unit (`pick_fill_length_anchor_prefers_better_seam_end`).
  - [x] Gate regression: `fit_fill_trims_tail_without_resampling` (gate path unchanged).
  - [ ] Wrong-cycle structure match scores lower than correct cycle **with penalty on** (integration).

### Acceptance

| Criterion | Status |
|-----------|--------|
| Repeat-at-seam down on manual listen pass | **Unverified** (penalty off by default; see corpus-validation checklist) |
| Patch rate not materially lower than Phase C | **Unverified** (no corpus before/after) |

---

## Config surface (cumulative)

| Key | Phase | Default | Notes |
|-----|-------|---------|-------|
| `fill_mode` | A | `fit` | `gate` = legacy |
| `fill_marginal_margin` | C | `0.08` | Warn band below `min_fill_correlation` |
| `fill_absolute_floor` | C | `0.12` | Hard skip; follows lowered `min_fill_correlation` in tests |
| `fill_fit_structure_weight` | B | `0.35` | CLI override |
| `fill_fit_waveform_weight` | B | `0.65` | CLI override |
| `fill_border_search_secs` | perf | **`10.0`** | Was 30; main B slide radius |
| `fill_repeat_penalty_weight` | D | **`0.0`** | 0 = off; tune via corpus listen |
| `fill_fit_pcm_refine` | D | — | **Not implemented** |

Existing keys unchanged: `min_fill_correlation`, `min_structure_match_score`, `max_fill_align_adjustment_secs`, `gap_end_extend_*`, `crossfade_ms`.

---

## Testing strategy (all phases)

| Layer | What | Status |
|-------|------|--------|
| Unit | Slide argmax, unified score, repeat penalty, dual anchor | **Done** |
| Lib | Gate vs fit branches | **Done** |
| Integration | `patch_audio_integration` fit + gate cases | **Done** (15 tests; fast-fit options for slow paths) |
| Corpus | Repair patch row; skip/marginal counts | **Not done** |
| Manual | External pair / gap corpus: `-v` patched vs marginal | **Not done** |

**CI:** Patch integration tests run both modes where applicable.

---

## Rollout

1. **Phase A** — shipped; default `fit` (2026-06-20).
2. **Phase B** — unified search (2026-06-20).
3. **Phase C** — joint grid + marginal tier (2026-06-20).
4. **Performance pass** — cache, early exit, border search default (2026-06-20).
5. **Phase D** — repeat penalty + dual anchor (**partial**: penalty off; no score-based extend; no GCC-PHAT).
6. **Remaining:** manual repair acceptance; archive doc; `PLAN.md` / `BACKLOG.md` update.

---

## Outstanding (not completed)

| Item | Priority | Notes |
|------|----------|-------|
| Manual listen checklist (external / gap corpus) | High | Template in `corpus-validation.md`; fill in Pass? column |
| Tune `fill_repeat_penalty_weight` on corpus | Medium | Default 0 until listen evidence |
| Score-based B extend when short | Low | Phase D stretch; still blind contiguous extend |
| `fill_fit_pcm_refine` (GCC-PHAT tie-break) | Low | Optional Phase D |
| Per-gap profiling (`patch-gap` label) | Low | Phase B acceptance |
| Marginal count in alignment-instability warning | Low | Phase C nuance |
| Archive to `docs/archive/fill-fitting-plan.md` | After manual sign-off | |

**Recently fixed (2026-06-20):** dual-anchor `min(pre,post)` scoring; stereo splice channels; `fill_repeat_penalty_weight >= 0` validation; deduped `fit_fill_to_gap_frames`; removed fit-candidate cache fallback; corpus-validation repair section; `#[ignore]` production-config smoke test; wrong-cycle + balanced-seam unit tests.

---

## Risks and mitigations

| Risk | Mitigation | Status |
|------|------------|--------|
| Slower patch pass | Timeline cache, High early exit, default border search 10 s | **Mitigated** (unprofiled) |
| Over-patching bad fills | `fill_absolute_floor`; marginal warns; `--fill-mode gate` | **Shipped** |
| Repeat penalty rejects good fills | Weight starts at 0 | **Shipped** |
| JSON / CLI contract drift | Documented in `json-output.md`, `gap-fill-modes.md` | **Shipped** |
| Dual-anchor picks wrong trim | Uses `pre+post` not `min(pre,post)` at splice | **Open** — see code review |

---

## Related reading

- [README.md](../README.md) § Gap patching pipeline
- [docs/gap-fill-modes.md](gap-fill-modes.md) — fit vs gate, flags, performance
- [docs/cli-output.md](cli-output.md) § Gap patch gate and skip reasons
- [docs/json-output.md](json-output.md) § repair patch summary
- [docs/archive/clip-self-repetition-plan.md](archive/clip-self-repetition-plan.md) — alignment-time repetition (orthogonal)

---

## Open questions

1. Default objective: `min(pre, post)` vs harmonic mean — bias toward balanced seams?
2. Should marginal patches count as `repaired` or `repaired (marginal)` in gap table header totals?
3. Phase D GCC-PHAT: reuse `clip-sync` offset_refinement vs slim repair correlator?
4. When to delete `gate` mode — after one release with `fit` default or keep for tests indefinitely?
5. Dual-anchor scoring: align with `min(pre,post)` or keep `pre+post` for trim pick?
