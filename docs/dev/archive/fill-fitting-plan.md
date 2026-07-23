# Gap fill — gate → fit transition (archived)

> **Status:** **Archived** (2026-06-20). Phases A–D **shipped** for production use. Default `fill_mode` is **`fit`**. Repeat penalty default **`fill_repeat_penalty_weight = 0.4`** (CLI: `--fill-repeat-penalty-weight`). Corpus / manual acceptance **not yet recorded** — see [corpus-validation.md](../corpus-validation.md) § Gap fill.

**Problem:** Gap patching was **search-then-gate**. Structure match *fits* B placement in a haystack, but waveform Pearson was evaluated at **one** candidate and compared to fixed thresholds.

**Goal:** Treat each gap as a **local optimization** — search for `(gap on A, start on B)` that minimizes seam discontinuity — and use thresholds only as **floors** and **warn tiers**. Preserve legacy behavior behind `fill_mode = "gate"` for regression tests.

**Non-goals (v1):** Full GCC-PHAT inside every gap; changing gap **scan** detection; resampling B fill.

---

## Shipped surface

| Key | Default | Notes |
|-----|---------|-------|
| `fill_mode` | `fit` | `gate` = legacy |
| `fill_fit_structure_weight` | `0.35` | CLI override |
| `fill_fit_waveform_weight` | `0.65` | CLI override |
| `fill_border_search_secs` | `10.0` | B slide radius |
| `fill_repeat_penalty_weight` | **`0.4`** | `0` = off; CLI override |
| `fill_marginal_margin` | `0.08` | Warn band |
| `fill_absolute_floor` | `0.12` | Hard skip |

---

## Phase summary

| Phase | Shipped |
|-------|---------|
| A — Waveform slide argmax | Yes |
| B — Unified scorer | Yes |
| C — Joint A-boundary grid + marginal tier | Yes |
| Performance — timeline cache, early exit, border search 10 s | Yes |
| D — Repeat penalty + dual trim/anchor | **Partial** — penalty on; score-based B extend shipped; GCC-PHAT **not built** |

### Phase D detail

- [x] `fill_repeat_correlations`, `repeat_penalty_at_placement` in unified scorer
- [x] `pick_fill_length_anchor` + stereo splice scoring (`min(pre,post)` + post tie-break)
- [x] Config + CLI for `fill_repeat_penalty_weight`
- [x] Unit tests: penalty algebra, dual anchor, wrong-cycle scorer
- [x] Integration: default penalty regression on clean sine fixture
- [x] Score-based B extend when bracket short (fit mode; gate keeps blind extend)
- [ ] Optional `fill_fit_pcm_refine` (GCC-PHAT tie-break)

---

## Testing

| Layer | Status |
|-------|--------|
| Unit (slide, unified score, penalty, dual anchor) | Done |
| Integration (`patch_audio_integration` gate + fit) | Done |
| Corpus repair row / manual listen | **Not done** |

Wrong-cycle **scorer** behavior is covered in `gap_fill_fit` unit tests; full patch path usually corrects placement via waveform search before penalty matters.

---

## Outstanding backlog

| Item | Priority |
|------|----------|
| Manual listen checklist (gap corpus / long-form) | High |
| Tune `fill_repeat_penalty_weight` on corpus | Medium |
| Score-based B extend when short | Low |
| `fill_fit_pcm_refine` (GCC-PHAT) | Low |
| Marginal count in alignment-instability warning | Low |

---

## Risks (final)

| Risk | Mitigation | Status |
|------|------------|--------|
| Slower patch pass | Timeline cache, High early exit, border search 10 s | Mitigated (unprofiled on long media) |
| Over-patching bad fills | `fill_absolute_floor`; marginal warns; `--fill-mode gate` | Shipped |
| Repeat penalty rejects good fills | Conservative default 0.4; CLI/config override to 0 | Shipped |
| Dual-anchor picks wrong trim | `min(pre,post)` + post tie-break at splice | **Fixed** |

---

## Related reading

- [gap-fill-modes.md](../../gap-fill-modes.md)
- [cli-output.md](../../cli-output.md)
- [corpus-validation.md](../corpus-validation.md)
