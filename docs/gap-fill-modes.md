# Gap fill modes (`fit` vs `gate`)

Reference for `clip-sync-repair` gap patching: how `fill_mode` interacts with CLI flags, config keys, performance, and report output.

**Related:** [cli-output.md](cli-output.md) (human/JSON patch lines), [json-output.md](json-output.md) (`GapPatchStatus`, `confidence`), [README.md](../README.md) § Gap patching (overview).

---

## Quick answers

| Question | Answer |
|----------|--------|
| Default mode? | **`fit`** (`fill_mode = "fit"`) |
| Does `--no-gap-end-extend` restore **gate**? | **No.** It only disables A-boundary extension. Use **`--fill-mode gate`** for legacy gating. |
| What does extension do in **fit**? | **Proactive joint grid** over gap start/end (when flags are on), each cell runs unified B placement. |
| What does extension do in **gate**? | **Reactive retries** after waveform failure: extend end, then extend start, re-score. |
| Why is repair slow? | Often the **fit slow path**: baseline not **High** → ~13×13 boundary grid × unified search with large `fill_border_search_secs`. |

---

## Pipelines

### `fill_mode = fit` (default)

```text
Per gap:
  1. Map gap on A → B (fill_offset_mode)
  2. Refine gap edges on A; structure-match on B (always)
  3. Evaluate baseline bracket:
       unified structure+waveform search on B
       tier: High | Marginal | skip (fill_absolute_floor)
     If High → done (fast path)
  4. If extension flags on and not High:
       joint grid: shift A start earlier × extend A end later
       (gap_end_extend_max_ms / step_ms, capped ~12 steps/axis)
       pick best combined candidate
  5. Splice at winner
```

- **No** structure-trust waveform skip, **no** one-strong-seam / mean-only waveform shortcuts.
- `structure_trusted` is always `false` in JSON.
- Marginal patches: `min(pre, post)` in `[min_fill_correlation - fill_marginal_margin, min_fill_correlation)` → patched with `confidence: marginal`, `!` in human output.

### `fill_mode = gate` (legacy)

```text
Per gap:
  1–2. Same mapping + structure match
  3. Waveform Pearson at structure winner (may be skipped if structure-trusted)
  4. On waveform failure only: sequential A-boundary extension retries
  5. Splice
```

- Structure trust, partial soften, short-gap mean, one-strong-seam apply here.
- Set `--fill-mode gate` or `fill_mode = "gate"` in config.

---

## Flag × mode matrix

CLI flags are accepted in both modes unless noted. **Effect** differs by mode.

| Flag / config | `fit` | `gate` |
|---------------|-------|--------|
| `--fill-mode` | Default **`fit`** | Legacy pipeline |
| `--min-fill-correlation` | Floor on `min(pre, post)` at winning candidate; drives High vs Marginal | Waveform gate threshold (with trust/shortcuts) |
| `--no-structure-trust` | **No extra effect** (fit never skips waveform) | Always run waveform; disable soften + short-gap shortcuts |
| `--no-short-gap-one-strong-seam` | **No effect** | Disable one-strong-seam fallback |
| `strong_structure_trust`, `partial_structure_waveform_soften` | **No effect on waveform** | Structure-trust skip / soften |
| `short_gap_one_strong_seam_fallback` | **No effect** | Short-gap shortcut |
| `--fill-offset` | **Active** | **Active** |
| `--border-standoff-secs` | **Active** | **Active** |
| `--no-gap-end-extend` | Disables **joint grid** end axis (baseline only on that axis) | Disables post-seam **retry** loop |
| `--no-gap-start-extend` | Disables **joint grid** start axis | Disables pre-seam **retry** loop |
| `--gap-end-extend-max-ms`, `--gap-end-extend-step-ms` | Grid span / step on A (fit) | Retry span / step (gate) |
| `--crossfade-ms`, `--no-normalize` | **Active** | **Active** |
| `--max-fill-align-adjust-secs` | Config key kept; **not** the main B search radius in fit (see below) | Structure polish window (legacy) |
| `fill_border_search_secs` | **Primary** B haystack slide radius for unified search (config-only) | Structure match search radius |
| `fill_fit_structure_weight`, `fill_fit_waveform_weight` | Unified scorer weights (config; CLI optional) | Ignored |
| `fill_marginal_margin`, `fill_absolute_floor` | Warn tier / hard skip (config-only) | Ignored |

**Align / scan flags** (`--clip-length`, `--num-clips`, query-reference, high-rate, gap scan knobs) are orthogonal to `fill_mode`.

---

## A-boundary extension (often confused with “mode”)

Extension flags control **whether A’s gap edges may move** during patch planning. They do **not** select `fit` vs `gate`.

### Fit: joint boundary search

When `gap_end_extend_on_post_seam_fail` and/or `gap_start_extend_on_pre_seam_fail` is **true** (defaults):

- After baseline evaluation, if the result is not **High**, search a grid of `(start, end)` brackets within `gap_end_extend_max_ms` (default **500 ms**) and `gap_end_extend_step_ms` (default **20 ms**), with ~**12 steps per axis** cap.
- Each grid cell runs **full unified B placement** (structure + waveform).
- Winning cell sets `gap_start_adjust_frames` / `gap_end_adjust_frames` in JSON.

With **`--no-gap-end-extend --no-gap-start-extend`**: only the **baseline** bracket is evaluated (no grid). Still **fit** placement and tiering.

### Gate: sequential retries

When waveform check **fails**:

1. Try extending **gap end** in steps (if post-seam extension enabled and candidate rules pass).
2. Else try shifting **gap start** earlier (pre-seam extension).

Gate retries use the same `gap_end_extend_*` ms limits but **different** eligibility rules (see [cli-output.md](cli-output.md) § Boundary extension retries).

---

## Waveform placement details

### Fit unified search

- Scores B candidates with  
  `fill_fit_structure_weight · structure_combined + fill_fit_waveform_weight · min(pre, post)`  
  (defaults **0.35 / 0.65**).
- B slide radius: **`fill_border_search_secs`** (default **10 s**), not `--max-fill-align-adjust-secs`.
- Haystack extract also uses context, margin, length slack, and extension slack — see config example in README.

### Gate waveform gate

- Pearson at the structure winner’s seams.
- May be **skipped** when both structure scores ≥ `strong_structure_trust` (default 0.90).
- Short gaps may pass on **mean** or **one strong seam** when enabled.

---

## Performance

| Path | Typical trigger | Cost driver |
|------|-----------------|-------------|
| **Fast** | Baseline **High** (`min(pre, post) ≥ min_fill_correlation`) | One unified search per gap |
| **Slow** | Borderline seams + extension on + large `fill_border_search_secs` | ~13×13 grid × unified search × long B haystack |

**Per-gap time scales with gap count** — ten slow-path gaps can mean hours.

### Recipes

**Interactive / faster fit** (still default mode):

```toml
[repair]
fill_mode = "fit"
fill_border_search_secs = 5.0      # default 10 — largest lever
gap_end_extend_max_ms = 200        # default 500
gap_end_extend_step_ms = 40
# optional: disable grid entirely
# gap_end_extend_on_post_seam_fail = false
# gap_start_extend_on_pre_seam_fail = false
```

```powershell
clip-sync-repair a.mkv b.mkv --mux out.mp4 `
  --fill-offset interpolated `
  --min-fill-correlation 0.35 `
  --no-gap-end-extend --no-gap-start-extend `
  -v
```

**Legacy strict gate** (pre-fit behavior):

```powershell
clip-sync-repair a.mkv b.mkv --mux out.mp4 `
  --fill-mode gate `
  --no-structure-trust `
  --min-fill-correlation 0.5 `
  -v
```

**Drift + fit** (common long-form example):

```powershell
clip-sync-repair "source.mp4" "recording.mkv" `
  --mux repaired.mp4 `
  --fill-offset interpolated `
  --min-fill-correlation 0.35 `
  -v
```

(`--fill-mode fit` is default; add tighter `fill_border_search_secs` in config if patch phase is slow.)

---

## Config keys (fit-specific)

| Key | Default | Notes |
|-----|---------|--------|
| `fill_mode` | `"fit"` | `"gate"` for legacy |
| `fill_border_search_secs` | `10.0` | B slide radius (unified search) |
| `fill_repeat_penalty_weight` | `0.4` | Phase D: penalize repeat-at-seam when seams weak (0 = off). Repeat window = `border_frames` (`normalize_window_secs`), not crossfade length — keep crossfade ≤ border window. |
| `fill_fit_structure_weight` | `0.35` | Unified scorer |
| `fill_fit_waveform_weight` | `0.65` | Unified scorer |
| `fill_marginal_margin` | `0.08` | Warn band below `min_fill_correlation` |
| `fill_absolute_floor` | `0.12` | Hard skip; follows lowered `min_fill_correlation` when gate disabled |
| `gap_end_extend_max_ms` | `500` | A-boundary grid / gate retries |
| `gap_end_extend_step_ms` | `20` | Grid/retry step |
| `max_fill_align_adjustment_secs` | `0.5` | Legacy; see matrix above |

**Fit-mode short B bracket:** when structure match returns fewer frames than the A gap, fit mode greedily extends into contiguous B audio frame-by-frame while padded `min(pre, post)` does not fall and `fill_repeat_correlations` post-repeat stays bounded; remaining frames are zero-padded. Gate mode still blind-extends then pads.

CLI: `--fill-fit-structure-weight`, `--fill-fit-waveform-weight` override fit weights when exposed in your build.

---

## Report / JSON

| `fill_mode` | Human patched row | JSON notes |
|-------------|-------------------|------------|
| `fit` | `patched (pre→post)` or `! patched` if marginal | `confidence`, `gap_*_adjust_frames`, `structure_trusted: false` |
| `gate` | `patched (struct …)` if structure-trusted | `structure_trusted: true` when waveform skipped |

Full field list: [json-output.md](json-output.md) § `GapPatchStatus`.
