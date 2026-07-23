# Patch-anchor offset map (archived)

> **Status:** **Archived** (2026-06-22). Phases 1–4 **shipped** in `clip-sync-repair`: `anchored_retry` two-pass, weighted anchors, JSON export, optional search prior, optional marginal pass-2 upgrade (`fill_anchor_retry_marginal`). Single-pass `anchored` **deferred** (enum + resolver only). **Phase 0** manual long-form drift validation **not yet recorded** — see [corpus-validation.md](../corpus-validation.md).

**Problem:** Per-gap B placement starts from alignment only: `recommended_offset_secs` (single global Δ) or `interpolated` (linear drift between **two** fingerprint clip anchors). Gaps are patched **independently** in one pass; `align_adjustment_secs` is reported on success but discarded for siblings. When clock drift is nonlinear or clip anchors are sparse (e.g. 15 min windows on a 2 h file), hard gaps search the wrong neighborhood even though nearby easy gaps proved the local Δ.

**Goal:** Treat high-confidence successful patches as **empirical offset anchors** `(a_time_secs, effective_offset_secs)` and use them to improve the nominal B map for remaining gaps — primarily before structure/waveform search. Ship behind `fill_offset_mode = anchored_retry` (two-pass). Fall back to clip-based `interpolated` / `recommended` when no anchor applies.

**Non-goals (v1):** Re-running global alignment; changing gap scan; using skipped/marginal patches as anchors; online drift model beyond piecewise linear interpolation; persisting anchors across CLI invocations.

---

## Shipped surface

| Key | Default | Notes |
|-----|---------|-------|
| `fill_offset_mode` | `recommended` | `anchored_retry` wired; `anchored` reserved |
| `fill_anchor_min_correlation` | same as `min_fill_correlation` | Anchor eligibility floor |
| `fill_anchor_exclude_structure_trusted` | `true` | Gate patches without waveform |
| `fill_anchor_max_adjustment_frac` | `0.9` | Reject edge-clamped slides |
| `fill_anchor_search_prior_weight` | `0.0` | Fit mode unified-search prior (pass 2) |
| `fill_anchor_retry_marginal` | `false` | Fit mode: pass 2 re-run marginal pass-1; keep only `High` |

CLI: `--fill-offset anchored-retry`; `--fill-anchor-*`; `--fill-anchor-retry-marginal`.

---

## Phase summary

| Phase | Shipped |
|-------|---------|
| 0 — Characterization | **Partial** — synthetic drift fixture; manual long-form notes open |
| 1 — Anchor types + resolver | Yes |
| 2 — Two-pass `PatchAudio` + wiring | Yes (fit + gate integration) |
| 3 — Docs + eligibility | Yes (`anchored` single-pass deferred) |
| 4 — Search prior / weights / JSON | Yes |
| Post-ship — Marginal pass-2 retry | Yes (`fill_anchor_retry_marginal`) |

### Phase 0 detail

- [ ] Manual long-form drift pair: compare skip count `recommended` vs `anchored-retry`
- [x] Synthetic integration fixture: `patch_audio_anchored_retry_pass2_recovers_hard_gap_using_easy_anchors` (+ gate variant)
- [ ] Document baseline skip counts on corpus

### Phase 2 detail

- [x] Drift fixture two-pass retry (fit + gate)
- [x] Smoke, no-pass-2-when-empty-table, all-pass-1-success regressions

### Deferred / backlog

- Single-pass `anchored` in `PatchAudio` (enum + resolver only today)
- Pass-1 easy-first ordering (low priority; collect-then-splice makes pass-1 order irrelevant)
- `BACKLOG` segment-wise alignment: patch anchors reduce urgency but do not replace global refine

---

## Architecture (shipped)

```text
align → scan → fill plan
    → offset map: clip anchors + patch anchors
    → per gap: gap_offset_secs (improved nominal)
    → structure + waveform search (unchanged)
    → splice
```

**Orthogonal to `fill_mode` (`fit` / `gate`):** anchors change `gap_offset_secs` only.

At gap midpoint `t` on A: `effective_offset(t) ≈ fill_offset_secs(…) + align_adjustment_secs`. A-boundary adjusts (`gap_*_adjust_frames`) are excluded from anchor offset in v1.

### Pass 2 retry scope

| Pass-1 outcome | Retried when |
|----------------|--------------|
| `Skipped` + `CorrelationBelowThreshold` or `BoundaryAlignmentFailed` | Always (if anchor table non-empty) |
| `Patched` + `confidence: Marginal` (fit only) | `fill_anchor_retry_marginal = true`; replace only if pass 2 is `High` |

Marginal patches are **not** anchor sources.

---

## Testing

| Layer | Status |
|-------|--------|
| Unit (`patch_anchor.rs`, `fill_offset.rs`, `patch_audio.rs` retry helpers) | Done |
| Integration drift two-pass (fit + gate) | Done (~3 min fit test) |
| Marginal retry flag regression | Done |
| Manual / corpus long-form drift | **Not done** |

---

## Outstanding backlog

| Item | Priority |
|------|----------|
| Manual listen / skip-count comparison on drift-heavy long-form pair | Medium |
| Wire single-pass `anchored` | Low |
| Segment-wise global alignment refine | Low (see `BACKLOG.md`) |

---

## Related reading

- [gap-fill-modes.md](../../gap-fill-modes.md) § Patch anchors
- [README.md](../../README.md) § Per-gap B timeline
- [archive/fill-fitting-plan.md](fill-fitting-plan.md) — orthogonal fit/gate placement layer
- [TEMP-energy-signature-plan.md](../TEMP-energy-signature-plan.md) — complementary structure matching

---

## Open questions (resolved)

1. **`anchored` vs `anchored_retry`:** two enum values; only `anchored_retry` wired in `PatchAudio`.
2. **Clip + patch anchor merge:** clip endpoints + patch anchors in `interpolate_anchored_offset_secs`.
3. **Retry marginal pass-1 in pass 2:** shipped behind `fill_anchor_retry_marginal` (default off).
4. **JSON anchor table:** `patch.patch_anchors_used` on `PatchSummary`.
5. **Pass-1 easy-first:** deferred (low priority).
