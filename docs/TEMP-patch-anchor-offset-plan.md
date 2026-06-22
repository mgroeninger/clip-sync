# Temporary plan: patch-anchor offset map

> **Status:** Draft (2026-06-21). **Phases 1–4 shipped** in `clip-sync-repair`: domain types, `anchored_retry` two-pass, weighted anchors, JSON export, optional search prior. **Phase 0** drift characterization fixture still open. Single-pass `anchored` deferred (enum + resolver only; not wired in `PatchAudio`).
>
> Motivated by runs where **some** gaps patch cleanly (`slide=+0.35s`, high seam scores) while **others** fail seam search — often because the nominal B map from alignment is hundreds of ms off at that point on A, pushing the true dropout to the edge of `fill_border_search_secs`. Successful patches already measure local residual offset (`align_adjustment_secs`) but do not feed later gaps.
>
> Archive to `docs/archive/patch-anchor-offset-plan.md` when shipped.

**Problem:** Per-gap B placement starts from alignment only: `recommended_offset_secs` (single global Δ) or `interpolated` (linear drift between **two** fingerprint clip anchors). Gaps are patched **independently** in one pass; `align_adjustment_secs` is reported on success but discarded for siblings. When clock drift is nonlinear or clip anchors are sparse (e.g. 15 min windows on a 2 h file), hard gaps search the wrong neighborhood even though nearby easy gaps proved the local Δ.

**Goal:** Treat high-confidence successful patches as **empirical offset anchors** `(a_time_secs, effective_offset_secs)` and use them to improve the nominal B map for remaining gaps — primarily before structure/waveform search. Ship behind `fill_offset_mode = anchored` (or `anchored_retry` two-pass). Fall back to clip-based `interpolated` / `recommended` when no anchor applies.

**Non-goals (v1):** Re-running global alignment; changing gap scan; using skipped/marginal patches as anchors; online drift model beyond piecewise linear interpolation; persisting anchors across CLI invocations.

---

## Current codebase baseline

| Area | Path | Current state | First phase touched |
|------|------|---------------|---------------------|
| Offset map | `domain/fill_offset.rs`, `domain/patch_anchor.rs` | `Recommended` \| `Interpolated` \| `Anchored` \| `AnchoredRetry`; resolver + anchor table | 1–2 |
| Per-gap loop | `application/patch_audio.rs` | Pass 1 collect-then-splice; `anchored_retry` pass 2 retries failures on pristine `a_pcm` | 2 |
| Slide measurement | `application/patch_audio.rs` | `align_adjustment_secs = structure_slide + waveform_slide`; verbose splits both | 1 |
| Outcomes | `domain/patch_result.rs` | `Patched { align_adjustment_secs, waveform_adjustment_secs, confidence, structure_trusted, … }` | 1 |
| Fill mode | `domain/fill_mode.rs`, `docs/gap-fill-modes.md` | Default **`fit`** (unified search + marginal tier); `gate` legacy | — (orthogonal) |
| Unified fit | `domain/gap_fill_fit.rs`, `patch_region.rs` | `match_gap_fill_unified_in_b_with_timeline`; `fill_repeat_penalty_weight` default **0.4** | — |
| Fill plan | `domain/gap_fill.rs` | Regions in scan order; no patch-order policy | 2 |
| Config / CLI | `infrastructure/config.rs`, `cli/args.rs` | `fill_offset_mode`, `fill_anchor_*`, `fill_border_search_secs` (default **10 s**), `fill_mode`, `fill_repeat_penalty_weight` | 2, 4 |
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
| **Reporting** | Verbose: `anchored: N offset anchor(s) from gap #…` (pass 1); `offset anchor: +0.35s from gap #3` (pass 2). JSON: `patch.patch_anchors_used` when `anchored_retry` built a non-empty table. |

---

## Phases

### Phase 0 — Characterization

**Intent:** Quantify how often failures correlate with large `|align_adjustment|` on nearby successes.

- [ ] Manual / script checklist on a drift-heavy long-form pair: list patched `slide=` per gap vs skipped gap times; note if skipped gaps sit between patches with consistent slide.
- [ ] Synthetic integration fixture: global offset −3 s, **local drift** +0.5 s mid-file (B timeline stretched vs A); 3 easy gaps patch with +0.5 s slide, 4th fails with `recommended` but would pass with anchored offset.
- [ ] Document baseline skip count with `recommended`, `interpolated`, and theoretical anchored (manual offset injection in test).

**Code today:** `patch_audio_integration.rs` has `make_drift_alignment` and `patch_audio_interpolated_offset_maps_late_gap_with_drift` (interpolated only). No anchored-retry drift fixture yet.

### Phase 1 — Anchor types + offset resolver

**Intent:** Domain types and pure functions; resolver usable from tests before `PatchAudio` wiring.

- [x] `domain/patch_anchor.rs`:
  - `PatchOffsetAnchor { a_secs, offset_secs, weight, source_gap_index }`
  - `PatchAnchorCandidate` + `PatchAnchorTable::from_candidates(...)` — eligibility via `PatchAnchorPolicy`
  - `interpolate_anchored_offset_secs(alignment, gap_time, patch_anchors) -> Option<f64>`
- [x] `domain/fill_offset.rs`: `resolve_gap_offset_secs(alignment, mode, gap_time, patch_anchors, anchored_retry_pass) -> Option<f64>`; `fill_offset_secs` unchanged for callers without anchors.
- [x] Extend `FillOffsetMode` with `Anchored` and `AnchoredRetry` (`AnchoredRetryPass` for pass 1 vs 2).
- [x] `Anchored` / pass-2 `AnchoredRetry` resolution order (`anchored_offset_secs`):
  1. If ≥1 patch anchor: weighted piecewise interpolation among patch anchors (+ clip anchors as endpoints when present)
  2. Else clip drift: `interpolated_offset_secs` (or `recommended_offset_secs` fallback)
- [x] Unit tests (`patch_anchor.rs`, `fill_offset.rs`): two patch anchors + query between; extrapolation clamp; empty table → clip/recommended fallback; marginal + `structure_trusted` excluded; clip endpoints merged with patch anchors; verbose formatting.

### Phase 2 — Two-pass patch in `PatchAudio`

**Intent:** Pass 1 = current behavior; build anchor table; pass 2 retry failures with anchored offset.

- [x] `PatchAudio::execute` (`patch_audio.rs`):
  - Pass 1: existing loop; `region_results` per gap (`AnchoredRetryPass::First`, no anchor table).
  - Build `PatchAnchorTable` from pass-1 patched gaps via `build_patch_anchor_candidates` + `from_candidates`.
  - If `fill_offset_mode == AnchoredRetry` && table non-empty: `run_anchored_retry_pass` over retryable skips only (`is_retryable_patch_skip`: `CorrelationBelowThreshold`, `BoundaryAlignmentFailed`).
  - Pass 2 success replaces pass-1 outcome and appends splice patch; pass-2 failure keeps pass-1 skip.
- [x] Wire `fill_offset_mode` through `config.rs`, CLI (`--fill-offset anchored-retry`), `PatchAudioRequest`; `fill_anchor_*` policy keys.
- [x] `prepare_region_patch`: `anchored_retry_pass` + optional `patch_anchors` → `resolve_gap_offset_secs`.
- [x] Integration tests (partial):
  - [x] Smoke: `patch_audio_anchored_retry_passes_on_clean_single_gap` (fit, single gap, no pass-2 needed).
  - [x] Drift fixture: `patch_audio_anchored_retry_pass2_recovers_hard_gap_using_easy_anchors` — pass 1 skips hard gap under `interpolated` + tight search; `anchored_retry` pass 2 patches using interior anchors (~3 min; 60 s WAV).
  - [ ] Run drift fixture under **`fill_mode = gate`** — anchor eligibility (`structure_trusted` exclusion) distinct from fit.
  - [x] Regression: `Recommended` / `Interpolated` paths unchanged (existing integration tests; no second pass).
  - [x] Explicit: `patch_audio_anchored_retry_skips_pass2_when_no_anchors` (marginal → empty table); `patch_audio_anchored_retry_skips_pass2_when_all_gaps_patch_in_pass1` (pass-1 success exports anchors, no retries).

### Phase 3 — `Anchored` single-pass + docs

**Intent:** One-pass mode for users who prefer simplicity; tune eligibility; document.

- [x] `Anchored` without retry: **deferred** — `anchored` enum + resolver only; `PatchAudio` does not build a live anchor table in pass 1. Document two-pass (`anchored_retry`) as the supported path.
- [x] Anchor eligibility documented (`fill_anchor_*` keys in README, `gap-fill-modes.md`; marginal + `structure_trusted` rules in `anchor_eligible`).
- [x] README § `fill_offset_mode` table; `docs/gap-fill-modes.md` cross-link.
- [x] Verbose lines for anchor source gap index (`format_patch_anchor_table_summary`, `format_anchored_offset_verbose_line`).
- [x] Default policy: `Recommended`; document `anchored-retry` for drift-heavy pairs.

### Phase 4 — Optional enhancements

- [x] Soft search prior in unified fit (`AnchorSearchPrior`, `fill_anchor_search_prior_weight`): fit mode only; active on `anchored_retry` pass 2 (and would apply for single-pass `anchored` when wired).
- [x] Weight anchors by `min(pre, post)` (`PatchAnchorTable::from_candidates` + `interpolate_piecewise_weighted`).
- [x] Complementary to [TEMP-energy-signature-plan.md](TEMP-energy-signature-plan.md) — no code coupling.
- [x] Export anchors in JSON (`patch.patch_anchors_used` on `PatchSummary` via `with_patch_anchors`).
- [ ] `BACKLOG` segment-wise alignment: patch anchors may reduce urgency but do not replace global refine.

---

## Config surface (cumulative)

| Key | Phase | Default | Notes |
|-----|-------|---------|-------|
| `fill_offset_mode` | 2 | `recommended` | `recommended`, `interpolated`, `anchored` (reserved), `anchored_retry` (wired) |
| `fill_anchor_min_correlation` | 2 | same as `min_fill_correlation` (`0.35`) | Floor for anchor eligibility |
| `fill_anchor_exclude_structure_trusted` | 2 | `true` | Gate-mode patches without waveform; CLI invert: `--fill-anchor-include-structure-trusted` |
| `fill_anchor_max_adjustment_frac` | 2 | `0.9` | Fraction of `fill_border_search_secs`; reject edge-clamped slides |
| `fill_anchor_search_prior_weight` | 4 | `0.0` | Fit mode: soft penalty vs anchor-predicted B start (pass 2 of `anchored_retry`) |
| `fill_border_search_secs` | — | `10.0` | Primary B slide radius in **fit**; structure search radius in **gate**. README examples often use `30.0`. Anchors center this window. |
| `min_fill_correlation` | — | `0.35` | Patch seam floor; default for `fill_anchor_min_correlation` |

CLI: `--fill-offset anchored-retry`; `--fill-anchor-min-correlation`, `--fill-anchor-max-adjustment-frac`, `--fill-anchor-search-prior-weight`.

---

## Testing strategy

| Layer | What | Status |
|-------|------|--------|
| Unit | `PatchAnchorTable::from_candidates` filter; weighted interpolation; extrapolation clamp; fallback chain | Shipped (`patch_anchor.rs`) |
| Unit | `resolve_gap_offset_secs` / `anchored_retry` pass 1 vs 2 | Shipped (`fill_offset.rs`) |
| Integration | Drift fixture two-pass retry | Shipped (`patch_audio_anchored_retry_pass2_recovers_hard_gap_using_easy_anchors`; ~3 min) |
| Integration | Pass-1 success unchanged when mode `recommended` / `interpolated` | Covered by existing patch tests |
| Integration | `anchored_retry` smoke (clean single gap) | Shipped |
| Integration | No pass 2 when zero eligible anchors / all pass-1 succeed | Shipped (`skips_pass2_when_no_anchors`, `skips_pass2_when_all_gaps_patch_in_pass1`) |
| Integration | `fill_mode = gate` anchor eligibility | **Open** |
| Manual | Long-form drift pair: compare skip count `recommended` vs `anchored-retry` | **Open** (Phase 0) |

---

## Rollout

1. **Phase 0** — drift fixture + manual notes. **Remaining.**
2. **Phase 1** — domain types + resolver. **Done.**
3. **Phase 2** — two-pass `PatchAudio` + wiring. **Done** (integration drift/gate tests still open).
4. **Phase 3** — docs + eligibility tuning. **Done** (single-pass `anchored` deferred).
5. **Phase 4** — search prior / weights / JSON export. **Done.**
6. Archive to `docs/archive/`; update `PLAN.md` repair § offset map; trim `BACKLOG.md` row when Phase 0 closes.

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
- `domain/fill_offset.rs` — offset modes + `resolve_gap_offset_secs`
- `domain/patch_anchor.rs` — anchor table, interpolation, verbose formatting
- `application/patch_audio.rs` — two-pass `anchored_retry`, `align_adjustment_secs`
- `domain/patch_result.rs` — `GapPatchStatus::Patched`

---

## Open questions

1. **`anchored` vs `anchored_retry`:** **Resolved** — two enum values; only `anchored_retry` wired in `PatchAudio`.
2. **Include clip anchors always** in piecewise curve when patch anchors exist, or patch-only interior + clip endpoints? **Resolved** — merge clip endpoints + patch anchors (see `interpolate_anchored_offset_secs`).
3. **Retry marginal pass-1** patches in pass 2 with anchored offset for higher seam scores? Open (Phase 4).
4. **Expose anchor table in JSON** in Phase 2 or defer to Phase 4? **Resolved** — `patch.patch_anchors_used` on `PatchSummary` (Phase 4).
5. **Sort pass-1 easy-first** even in two-pass (better anchors before pass-1 failures) — worth the plan reorder? Open (low priority; collect-then-splice makes pass-1 order irrelevant today).
