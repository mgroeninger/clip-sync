# Temporary plan: patch-anchor offset map

> **Status:** Draft (2026-06-20). Motivated by runs where **some** gaps patch cleanly (`slide=+0.35s`, high seam scores) while **others** fail seam search — often because the nominal B map from alignment is hundreds of ms off at that point on A, pushing the true dropout to the edge of `fill_border_search_secs`. Successful patches already measure local residual offset (`align_adjustment_secs`) but do not feed later gaps.
>
> Archive to `docs/archive/patch-anchor-offset-plan.md` when shipped.

**Problem:** Per-gap B placement starts from alignment only: `recommended_offset_secs` (single global Δ) or `interpolated` (linear drift between **two** fingerprint clip anchors). Gaps are patched **independently** in one pass; `align_adjustment_secs` is reported on success but discarded for siblings. When clock drift is nonlinear or clip anchors are sparse (e.g. 15 min windows on a 2 h file), hard gaps search the wrong neighborhood even though nearby easy gaps proved the local Δ.

**Goal:** Treat high-confidence successful patches as **empirical offset anchors** `(a_time_secs, effective_offset_secs)` and use them to improve the nominal B map for remaining gaps — primarily before structure/waveform search. Ship behind `fill_offset_mode = anchored` (or `anchored_retry` two-pass). Fall back to clip-based `interpolated` / `recommended` when no anchor applies.

**Non-goals (v1):** Re-running global alignment; changing gap scan; using skipped/marginal patches as anchors; online drift model beyond piecewise linear interpolation; persisting anchors across CLI invocations.

---

## Current codebase baseline

| Area | Path | Current state | First phase touched |
|------|------|---------------|---------------------|
| Offset map | `domain/fill_offset.rs` | `Recommended` \| `Interpolated` from `AlignmentReport` only | 1 |
| Per-gap loop | `application/patch_audio.rs` | Collect all `prepare_region_patch` results, **then** splice (pristine `a_pcm` for every gap in pass 1) | 1, 2 |
| Slide measurement | `application/patch_audio.rs` | `align_adjustment_secs = structure_slide + waveform_slide`; verbose splits both | 1 |
| Outcomes | `domain/patch_result.rs` | `Patched { align_adjustment_secs, waveform_adjustment_secs, confidence, structure_trusted, … }` | 1 |
| Fill mode | `domain/fill_mode.rs`, `docs/gap-fill-modes.md` | Default **`fit`** (unified search + marginal tier); `gate` legacy | — (orthogonal) |
| Unified fit | `domain/gap_fill_fit.rs`, `patch_region.rs` | `match_gap_fill_unified_in_b_with_timeline`; `fill_repeat_penalty_weight` default **0.4** | — |
| Fill plan | `domain/gap_fill.rs` | Regions in scan order; no patch-order policy | 2 |
| Config / CLI | `infrastructure/config.rs`, `cli/args.rs` | `fill_offset_mode`, `fill_border_search_secs` (default **10 s**), `fill_mode`, `fill_repeat_penalty_weight` | 1 |
| Cross-check | `domain/cross_check.rs` | `gap_offset_agreement` (scan-time A/B silence) — orthogonal | — |

### What a successful patch already tells us

At gap midpoint `t` on A:

```text
effective_offset(t) ≈ fill_offset_secs(alignment, mode, t) + align_adjustment_secs
```

`align_adjustment_secs` is total B slide from the mapped nominal (structure + waveform). See `patch_audio.rs` after `evaluate_seam_gate`.

A-boundary moves (`gap_start_adjust_frames`, `gap_end_adjust_frames`) adjust **A** edges, not B timeline offset — **exclude** from anchor offset in v1 (document; revisit if A-extension dominates).

### Pipeline fit

```text
align → scan → fill plan
    → [NEW] offset map: clip anchors + patch anchors
    → per gap: gap_offset_secs (improved nominal)
    → structure + waveform search (unchanged)
    → splice
```

**Perpendicular to `fill_mode` (`fit` / `gate`):** both call `fill_offset_secs` at the start of `prepare_region_patch`. This plan only improves the **offset map layer** (same axis as `--fill-offset interpolated`).

### Fit vs gate (both supported)

Anchors change **`gap_offset_secs` only** — before structure match and waveform placement. Everything after that runs unchanged for the active `fill_mode`.

| Stage | `fit` (default) | `gate` |
|-------|-----------------|--------|
| Offset map | `fill_offset_secs` → improved by anchors | same |
| Structure + waveform | Unified search; `confidence` High / Marginal / skip | Structure winner → waveform gate; structure-trust skip |
| `align_adjustment_secs` | Still structure + waveform slide vs nominal | same |
| Pass 2 retry | Re-runs full `prepare_region_patch` (fit grid or gate retries) with new offset | same |

**Anchor eligibility differs by mode:**

| Rule | `fit` | `gate` |
|------|-------|--------|
| `confidence: High` | Required (Marginal excluded) | N/A — always `High` in JSON today |
| `structure_trusted` | Always `false` — ignore | Exclude when `fill_anchor_exclude_structure_trusted` (no waveform measured) |
| `min(pre, post)` floor | Waveform Pearson at winner | Waveform Pearson, or structure scores when trusted |

Pass 1 should use the user's configured `fill_mode` for both anchor collection and failure characterization. Pass 2 only changes offset resolution; it does not switch modes.

---

## Decisions

| Topic | Decision |
|-------|----------|
| **Anchor definition** | `(a_anchor_secs, offset_secs)` where `a_anchor_secs = (a_start + a_end) / 2` of patched gap; `offset_secs = base_fill_offset + align_adjustment_secs` at patch time. |
| **Anchor eligibility** | `Patched` + `confidence: High` + `min(pre, post) ≥ min_fill_correlation` + not `structure_trusted` (gate) unless waveform scores also recorded. Exclude `Marginal`. Optional: require `\|align_adjustment\| < fill_border_search_secs * 0.9` (found inside search, not clamped at edge). |
| **Interpolation** | Piecewise linear in `a_anchor_secs` between patch anchors; merge with clip start/end anchors from alignment when `Interpolated` would apply. Extrapolate: clamp to nearest anchor pair (no wild extrapolation beyond first/last anchor). |
| **Mode switch** | Extend `FillOffsetMode`: add `Anchored` (use patch anchors when present, else same as `Interpolated` or `Recommended`). Two-pass variant: `AnchoredRetry` — pass 1 independent, pass 2 retry failures only. Default stays `Recommended` until Phase 3. |
| **Pass strategy (v1)** | **Two-pass** default for `AnchoredRetry`: avoids order dependency and wrong early anchor poisoning. Single-pass sequential deferred to Phase 4 optional. |
| **Retry scope** | Pass 2: only `Skipped` with `CorrelationBelowThreshold` (structure **or** waveform below floor in fit/gate) or `BoundaryAlignmentFailed` (`StructureAlignmentFailed`). Not `BExtractFailed`, `ZeroLengthGap`, `AlignedSegmentOutOfRange`. Optionally include marginal pass-1 successes for re-centering — defer. |
| **Search prior (optional)** | Phase 4: soft penalty in `unified_fit_score` for B candidates far from anchor-predicted offset — **fit mode only**; coordinate with `fill_repeat_penalty_weight`. Not required for v1. |
| **Decode cost** | Pass 2 reuses in-memory `a_pcm` + `b_samples_full` from pass 1 (already decoded once). Re-run `prepare_region_patch` only for retry gaps — no extra full-file decode. |
| **Reporting** | Verbose: `offset anchor: +0.35s from gap #3`; JSON: optional `patch_anchors_used` on summary (Phase 3). |

---

## Phases

### Phase 0 — Characterization

**Intent:** Quantify how often failures correlate with large `|align_adjustment|` on nearby successes.

- [ ] Manual / script checklist on licensed-media–like pair: list patched `slide=` per gap vs skipped gap times; note if skipped gaps sit between patches with consistent slide.
- [ ] Synthetic integration fixture: global offset −3 s, **local drift** +0.5 s mid-file (B timeline stretched vs A); 3 easy gaps patch with +0.5 s slide, 4th fails with `recommended` but would pass with anchored offset.
- [ ] Document baseline skip count with `recommended`, `interpolated`, and theoretical anchored (manual offset injection in test).

### Phase 1 — Anchor types + offset resolver

**Intent:** Domain types and pure functions; no behavior change in production path.

- [ ] `domain/patch_anchor.rs` (new):
  - `PatchOffsetAnchor { a_secs, offset_secs, weight, source_gap_index }`
  - `PatchAnchorTable::from_outcomes(...)` — filter eligibility rules
  - `resolve_fill_offset_secs(alignment, mode, gap_time, clip_anchors, patch_anchors) -> Option<f64>`
- [ ] Extend `FillOffsetMode` with `Anchored` (and reserve `AnchoredRetry` for Phase 2).
- [ ] `Anchored` resolution order:
  1. If ≥1 patch anchor: interpolate among patch anchors (+ clip anchors as endpoints if available)
  2. Else if clip drift: `interpolated_offset_secs`
  3. Else `recommended_offset_secs`
- [ ] Unit tests: two patch anchors + query between; extrapolation clamp; empty table → fallback; weighting ignored in v1 (equal weight).

### Phase 2 — Two-pass patch in `PatchAudio`

**Intent:** Pass 1 = current behavior; build anchor table; pass 2 retry failures with `Anchored` offset.

- [ ] `PatchAudio::execute`:
  - Pass 1: existing loop; collect `(region, outcome, anchor_candidate?)` per gap.
  - Build `PatchAnchorTable` from pass-1 outcomes.
  - If `fill_offset_mode == AnchoredRetry` && table non-empty && pass-1 had retryable skips: pass 2 loop over failed regions only with `resolve_fill_offset_secs(..., Anchored, ...)`.
  - Pass 2 success replaces pass-1 outcome in `region_results`; pass-2 failure keeps pass-1 skip.
- [ ] Wire `fill_offset_mode` through config, CLI (`--fill-offset anchored-retry`), `PatchAudioRequest`.
- [ ] `prepare_region_patch`: accept optional `PatchAnchorTable` override for offset resolution (or pass resolved `gap_offset_secs` directly).
- [ ] Integration tests:
  - Drift fixture: pass 1 skips hard gap; pass 2 patches with anchors from easy gaps.
  - Run fixture under **`fill_mode = Fit`** (default) and **`gate`** — offset layer identical; gate-only anchor eligibility path.
  - Regression: `Recommended` / `Interpolated` unchanged (single pass, no table).
  - No retry when all pass-1 succeed.

### Phase 3 — `Anchored` single-pass + docs

**Intent:** One-pass mode for users who prefer simplicity; tune eligibility; document.

- [ ] `Anchored` without retry: **easy-first** sort regions by heuristic (short gap length, high `b_has_energy`, distance from file start) then sequential update of anchor table — or document as “two-pass only” and defer single-pass.
  - *Recommendation:* ship **two-pass only** in v1; add single-pass sequential only if requested.
- [ ] Tune anchor eligibility from corpus (marginal exclusion, structure_trusted rule).
- [ ] README § `fill_offset_mode` table; `docs/gap-fill-modes.md` cross-link.
- [ ] Verbose lines for anchor source gap index.
- [ ] Default policy: keep `Recommended`; document `anchored-retry` for drift-heavy pairs.

### Phase 4 — Optional enhancements (defer)

- [ ] Soft search prior in `unified_fit_score` from anchor prediction.
- [ ] Weight anchors by `min(pre, post)` or inverse gap length.
- [ ] Combine with [TEMP-energy-signature-plan.md](TEMP-energy-signature-plan.md) — better structure + better nominal map.
- [ ] Export anchors in JSON for debugging / external tools.
- [ ] `BACKLOG` segment-wise alignment: patch anchors may reduce urgency but do not replace global refine.

---

## Config surface (cumulative)

| Key | Phase | Default | Notes |
|-----|-------|---------|-------|
| `fill_offset_mode` | 2 | `recommended` | Add `anchored`, `anchored_retry` |
| `fill_anchor_min_correlation` | 2 | same as `min_fill_correlation` | Floor for anchor eligibility |
| `fill_anchor_exclude_structure_trusted` | 2 | `true` | Gate-mode patches without waveform |
| `fill_anchor_max_adjustment_frac` | 2 | `0.9` | Fraction of `fill_border_search_secs`; reject edge-clamped slides |
| `fill_border_search_secs` | — | `10.0` | Primary B slide radius in **fit**; structure search radius in **gate**. README examples often use `30.0`. Anchors center this window. |
| `min_fill_correlation` | — | `0.35` | Used for anchor gate |

CLI: `--fill-offset anchored-retry` (clap value enum extension).

---

## Testing strategy

| Layer | What |
|-------|------|
| Unit | `PatchAnchorTable` build filter; interpolation; extrapolation clamp; fallback chain |
| Unit | `resolve_fill_offset_secs` with 0, 1, 2, N anchors |
| Integration | Drift fixture two-pass retry |
| Integration | Pass-1 success unchanged when mode `recommended` |
| Integration | No pass 2 when zero eligible anchors |
| Manual | licensed media: compare skip count `recommended` vs `anchored-retry` |

---

## Rollout

1. **Phase 0** — drift fixture + manual notes.
2. **Phase 1** — domain types + resolver (no wiring).
3. **Phase 2** — two-pass `PatchAudio` + integration tests.
4. **Phase 3** — docs + eligibility tuning.
5. **Phase 4** — optional prior / weights.
6. Archive; update `PLAN.md` repair § offset map; `BACKLOG.md` row.

---

## Risks and mitigations

| Risk | Mitigation |
|------|------------|
| Wrong easy patch → bad anchor | Strict eligibility; two-pass retries only; exclude marginal |
| Sparse anchors (1 success) | Interpolate with clip endpoints; fallback to `interpolated` |
| Pass 2 double CPU on retries | Only failed gaps; PCM already in memory |
| A-boundary adjust confounds offset | Exclude from anchor formula v1 |
| Anchored retry masks alignment bug | Log anchor table in verbose; do not change global `recommended_offset_secs` |

---

## Relationship to other plans

| Plan | Interaction |
|------|-------------|
| [archive/fill-fitting-plan.md](archive/fill-fitting-plan.md) | Shipped (fit default, repeat penalty 0.4); orthogonal — both modes benefit from better nominal map |
| [TEMP-energy-signature-plan.md](TEMP-energy-signature-plan.md) | Complementary — energy finds edges inside window; anchors center window |
| [gap-fill-modes.md](gap-fill-modes.md) | Document `anchored_retry` alongside `--fill-offset`; clarify fit vs gate anchor eligibility |
| `fill_offset interpolated` | Clip anchors remain endpoints; patch anchors add interior points |
| `BACKLOG` weighted drift warning | Unchanged; alignment-time signal vs patch-time empirical |

---

## Related reading

- [README.md](../README.md) § Per-gap B timeline (`fill_offset_mode`)
- [docs/gap-fill-modes.md](gap-fill-modes.md) — fit vs gate (orthogonal)
- `domain/fill_offset.rs` — current offset modes
- `application/patch_audio.rs` — `align_adjustment_secs`, per-gap loop
- `domain/patch_result.rs` — `GapPatchStatus::Patched`

---

## Open questions

1. **`anchored` vs `anchored_retry`:** One enum value with `patch_passes = 1 \| 2`, or two modes?
2. **Include clip anchors always** in piecewise curve when patch anchors exist, or patch-only interior + clip endpoints?
3. **Retry marginal pass-1** patches in pass 2 with anchored offset for higher seam scores?
4. **Expose anchor table in JSON** in Phase 2 or defer to Phase 4?
5. **Sort pass-1 easy-first** even in two-pass (better anchors before pass-1 failures) — worth the plan reorder?
