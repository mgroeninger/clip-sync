# Gap fill modes (`fit` vs `gate`)

Reference for `clip-sync-repair` gap patching: how `fill_mode` interacts with CLI flags, config keys, performance, and report output.

**Related:** [gap-repair-guide.md](gap-repair-guide.md) (classifying gaps and choosing profiles), [cli-output.md](cli-output.md) (human/JSON patch lines), [json-output.md](json-output.md) (`GapPatchStatus`, `confidence`), [README.md](../README.md) § Gap patching (overview). **Patch anchors:** [archive/patch-anchor-offset-plan.md](archive/patch-anchor-offset-plan.md) (`anchored_retry`).

---

## Quick answers

| Question | Answer |
|----------|--------|
| Default mode? | **`fit`** (`fill_mode = "fit"`) |
| Does `--no-gap-end-extend` restore **gate**? | **No.** It only disables A-boundary extension. Use **`--fill-mode gate`** for legacy gating. |
| What does extension do in **fit**? | **Proactive joint grid** over gap start/end (when flags are on), each cell runs unified B placement. |
| What does extension do in **gate**? | **Reactive retries** after waveform failure: extend end, then extend start, re-score. |
| Why is repair slow? | **`--full`** or `fit_boundary_search = full_grid`: baseline not **High** → boundary grid. **Default** accepts marginal baseline and skips the grid. |
| Patch anchors? | **`anchored_retry`** (config / `--fill-offset anchored-retry`): pass 1 clip offset, pass 2 retries failures using patch anchors. Works in **both** `fit` and `gate`. See [Patch anchors](#patch-anchors). |

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
| `fill_offset_mode = anchored_retry` | **Active** — two-pass offset map | **Active** |
| `--border-standoff-secs` | **Active** | **Active** |
| `--fill-border-search-secs` | **Primary** B haystack slide radius for unified search | Structure match search radius |
| `--fill-align-margin-secs` | Extra B extract padding | Extra B extract padding |
| `--gap-signature-context-secs` | Structure signature context; sizes B extract | Structure signature context |
| `--fill-length-slack-secs` | B fill-end slide slack | B fill-end slide slack |
| `--no-gap-end-extend` | Disables **joint grid** end axis (baseline only on that axis) | Disables post-seam **retry** loop |
| `--no-gap-start-extend` | Disables **joint grid** start axis | Disables pre-seam **retry** loop |
| `--gap-end-extend-max-ms`, `--gap-end-extend-step-ms` | Grid span / step on A (fit) | Retry span / step (gate) |
| `--crossfade-ms`, `--no-normalize` | **Active** | **Active** |
| `--max-fill-align-adjust-secs` | Legacy polish window only — **not** the main B search radius in fit | Structure polish window (legacy) |
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

Under **`fit_boundary_search = baseline_only`** (default profile, no `--full`): extension flags and `gap_end_extend_max_ms` do **not** run the grid or add B haystack slack; `-v` emits a `repair note:` when those settings are stored but inactive. Use **`--full`** to enable the grid and haystack slack.

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
- B slide radius: **`--fill-border-search-secs`** (default **10 s**), not `--max-fill-align-adjust-secs`.
- Haystack extract also uses context, margin, length slack, and extension slack — see config example in README.

### Gate waveform gate

- Pearson at the structure winner’s seams.
- May be **skipped** when both structure scores ≥ `strong_structure_trust` (default 0.90).
- Short gaps may pass on **mean** or **one strong seam** when enabled.

---

## Patch anchors

**Status:** `anchored_retry` shipped (2026-06-20). See [archive/patch-anchor-offset-plan.md](archive/patch-anchor-offset-plan.md).

Some runs patch several gaps cleanly (`slide=+0.35s` in verbose) while others fail seam search because the **nominal B map** from alignment is off by hundreds of ms at that point on A — the true dropout sits near the edge of `fill_border_search_secs`, not because `fit` or `gate` chose wrong.

**Patch anchors** reuse what easy gaps already measure: each successful patch records `align_adjustment_secs` (structure + waveform slide vs the mapped nominal). Flow:

```text
Pass 1: patch all gaps (clip-based offset; collect outcomes before splice)
    → build anchor table from high-confidence successes
Pass 2 (anchored_retry): retry failed gaps with improved gap_offset_secs
    → interpolate local Δ from nearby anchors (+ clip start/end when available)
    → re-run the same fill_mode (fit or gate) with a centered haystack
```

Single-pass `anchored` (easy-first sequential) is **deferred** — use `anchored_retry` today.

**Orthogonal to `fill_mode`:** anchors only change step 1 (`fill_offset_mode` / `gap_offset_secs`). Structure match, unified fit, marginal tier, gate trust, and extension behavior are unchanged.

| Topic | `fit` | `gate` |
|-------|-------|--------|
| Uses improved offset? | Yes | Yes |
| Anchor sources | `confidence: High` only (exclude Marginal) | Exclude `structure_trusted` when `fill_anchor_exclude_structure_trusted` (default true) |
| Drift without anchors | `--fill-offset interpolated` (2 clip anchors) | same |
| Drift with patch anchors | `--fill-offset anchored-retry` | same |

Try **`--fill-offset interpolated`** first on drift-heavy pairs. When hard gaps still fail near the search-window edge, add **`--fill-offset anchored-retry`** (or `fill_offset_mode = "anchored_retry"` in config).

### Anchor eligibility (config)

| Key | Default | Notes |
|-----|---------|--------|
| `fill_anchor_min_correlation` | same as `min_fill_correlation` (`0.35`) | `min(pre, post)` floor for a pass-1 patch to become an anchor |
| `fill_anchor_exclude_structure_trusted` | `true` | Gate-mode patches that skipped waveform measurement |
| `fill_anchor_max_adjustment_frac` | `0.9` | Reject anchors whose `\|align_adjustment\|` exceeds this fraction of `fill_border_search_secs` (edge-clamped slides) |
| `fill_anchor_search_prior_weight` | `0.0` | Fit mode + patch anchors: soft penalty in unified search for candidates far from anchor-predicted B start (0 = off) |
| `fill_anchor_retry_marginal` | `false` | Fit mode + `anchored_retry` pass 2: re-run pass-1 `marginal` patches with anchored offset; replace only when pass 2 is `high` |

Verbose (`-v`): after pass 1, `anchored: N offset anchor(s) from gap #…`; on pass-2 retries, `offset anchor: +Xs from gap #…` or `between gap #… and gap #…`. JSON: `patch.patch_anchors_used` when `anchored_retry` built anchors. See [cli-output.md](cli-output.md).

### Structure signatures (`gap_signature_mode`)

| Mode | Behavior |
|------|----------|
| `auto` (default) | Energy when pre/post envelope has contour; else bool |
| `bool` | Legacy active/silent bins (`gap_signature_bin_ms`) |
| `energy` | Always gated log-RMS envelope + Pearson match (fit path) |

Gate legacy path always uses bool structure. CLI: `--gap-signature-mode`.

---

## Performance

| Path | Typical trigger | Cost driver |
|------|-----------------|-------------|
| **Fast** | Baseline **High** or **Marginal** under `default` / `quick` (`fit_boundary_search = baseline_only`) | One unified search per gap |
| **Slow** | `--full` or `fit_boundary_search = full_grid` when baseline is not **High** | ~13×13 grid × unified search × long B haystack |

**Per-gap time scales with gap count** — ten slow-path gaps can mean hours.

### Repair profiles

Profiles bundle haystack size, extension flags, and boundary-grid policy. Explicit CLI flags and TOML keys **override** individual bundle fields (verbose lists overrides as `+ override: …`).

**Profile flag precedence:** `--quick` and `--full` take priority over `--profile <name>` when both are present (e.g. `--quick --profile full` resolves to **quick**). `--quick` and `--full` cannot be combined. Resolution order: load TOML → apply profile bundle from TOML `profile` unless a CLI profile flag is set → apply `--quick` / `--full` / `--profile` if present → apply per-field CLI/TOML overrides.

| Profile | CLI | Boundary grid | `fill_border_search_secs` | Typical use |
|---------|-----|---------------|---------------------------|-------------|
| **default** | *(none)* | Off (`baseline_only`) | 10 | Interactive repair; accepts marginal baseline |
| **quick** | `--quick` | Off | 5 | Draft mux; faster; smaller B window |
| **full** | `--full` | On (`full_grid`) | 10 | Quality pass; may shift A bracket on hard gaps |

```toml
[repair]
profile = "default"   # default | quick | full
# Advanced (set by profile; overridable):
# fit_boundary_search = "baseline_only"   # baseline_only | full_grid
```

```powershell
# Interactive default
clip-sync-repair a.mkv b.mkv --mux out.mp4 -v

# Draft / first listen
clip-sync-repair a.mkv b.mkv --mux draft.mp4 --quick -v

# Quality pass (legacy CPU cost)
clip-sync-repair a.mkv b.mkv --mux best.mp4 --full -v

# Quick + one override
clip-sync-repair a.mkv b.mkv --mux out.mp4 --quick --fill-border-search-secs 8 -v
```

Under **`baseline_only`**, `gap_end_extend_*` flags do **not** run the grid or add B haystack slack until `--full` (or `fit_boundary_search = full_grid`). `-v` emits `repair note:` when those settings are stored but inactive.

**`anchored_retry` is not part of profiles.** Add `--fill-offset anchored-retry` on `full` runs when drift-heavy pairs still skip gaps after the quality pass. See [gap-repair-guide.md](gap-repair-guide.md) Layer 5.

### Other recipes

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
clip-sync-repair recording_with_gaps.mp4 reference.mkv `
  --mux repaired.mp4 `
  --fill-offset interpolated `
  --min-fill-correlation 0.35 `
  -v
```

(`--fill-mode fit` is default; use `--fill-border-search-secs 5` or tighter haystack flags if patch phase is slow.)

**Drift + anchored retry** (when interpolated still skips gaps):

```powershell
clip-sync-repair recording_with_gaps.mp4 reference.mkv `
  --mux repaired.mp4 `
  --fill-offset anchored-retry `
  --min-fill-correlation 0.35 `
  -v
```

---

## Config keys (fit-specific)

| Key | Default | CLI | Notes |
|-----|---------|-----|--------|
| `fill_mode` | `"fit"` | `--fill-mode` | `"gate"` for legacy |
| `fill_border_search_secs` | `10.0` | `--fill-border-search-secs` | B slide radius (unified search) |
| `fill_align_margin_secs` | `1.0` | `--fill-align-margin-secs` | Extra B extract padding |
| `gap_signature_context_secs` | `3.0` | `--gap-signature-context-secs` | Structure signature context |
| `fill_length_slack_secs` | `5.0` | `--fill-length-slack-secs` | B fill-end slide slack |
| `fill_repeat_penalty_weight` | `0.4` | `--fill-repeat-penalty-weight` | Penalize repeat-at-seam when seams weak (0 = off) |
| `fill_fit_structure_weight` | `0.35` | `--fill-fit-structure-weight` | Unified scorer |
| `fill_fit_waveform_weight` | `0.65` | `--fill-fit-waveform-weight` | Unified scorer |
| `fill_marginal_margin` | `0.08` | — | Warn band below `min_fill_correlation` |
| `fill_absolute_floor` | `0.12` | — | Hard skip floor |
| `gap_end_extend_max_ms` | `500` | `--gap-end-extend-max-ms` | A-boundary grid / gate retries |
| `gap_end_extend_step_ms` | `20` | `--gap-end-extend-step-ms` | Grid/retry step |
| `max_fill_align_adjustment_secs` | `0.5` | `--max-fill-align-adjust-secs` | Legacy polish window |

**Fit-mode short B bracket:** when structure match returns fewer frames than the A gap, fit mode greedily extends into contiguous B audio frame-by-frame while padded `min(pre, post)` does not fall and `fill_repeat_correlations` post-repeat stays bounded; remaining frames are zero-padded. Gate mode still blind-extends then pads.

CLI: `--fill-fit-structure-weight`, `--fill-fit-waveform-weight`, and the B haystack flags above override config when passed on the command line.

---

## Report / JSON

| `fill_mode` | Human patched row | JSON notes |
|-------------|-------------------|------------|
| `fit` | `patched (pre→post)` or `! patched` if marginal | `confidence`, `gap_*_adjust_frames`, `structure_trusted: false` |
| `gate` | `patched (struct …)` if structure-trusted | `structure_trusted: true` when waveform skipped |

Full field list: [json-output.md](json-output.md) § `GapPatchStatus`.
