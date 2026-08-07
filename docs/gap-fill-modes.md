# Gap fill modes (`fit` vs `gate`)

Reference for `clip-sync-repair` gap patching: how `fill_mode` interacts with CLI flags, config keys, performance, and report output.

**Related:** [pipeline.md](pipeline.md) (phases 1–5 and `PatchAudio` run map — **read together with this doc**), [gap-repair-guide.md](gap-repair-guide.md) (classifying gaps and choosing profiles), [seam-scoring.md](seam-scoring.md) (how seams are scored), [cli-output.md](cli-output.md) (human/JSON patch lines), [json-output.md](json-output.md) (`GapPatchStatus`, `confidence`), [README.md](../README.md) § Gap patching (overview). **Patch anchors:** [archive/patch-anchor-offset-plan.md](dev/archive/patch-anchor-offset-plan.md) (`anchored_retry`).

---

## Quick answers

| Question | Answer |
|----------|--------|
| Default mode? | **`fit`** (`fill_mode = "fit"`) |
| Does `--no-gap-end-extend` restore **gate**? | **No.** It only disables A-boundary extension. Use **`--fill-mode gate`** for legacy gating. |
| What does extension do in **fit**? | **Proactive joint grid** over gap start/end (when flags are on), each cell runs unified B placement. |
| What does extension do in **gate**? | **Reactive retries** after waveform failure: extend end, then extend start, re-score. |
| Why is repair slow? | **`--full`** or `fit_boundary_search = full_grid`: every grid cell (~144 max) runs full per-bracket measurement. **Default** accepts marginal baseline and skips anchor + grid. |
| Patch anchors? | **`anchored_retry`** (config / `--fill-offset anchored-retry`): pass 1 clip offset, pass 2 retries failures using patch anchors. Works in **both** `fit` and `gate`. See [Patch anchors](#patch-anchors). |
| Editorial anchor seam? | **`anchor_seam_mode = auto|force`** (`--anchor-seam-mode`): search speech peaks / bool onsets when throat Pearson is weak. **Fit only**; orthogonal to patch anchors. See [Editorial anchor seam](#editorial-anchor-seam). |
| Residual gate? | Default **`residual_gate = veto`** (fit only): anti-echo headroom veto after Pearson tiering; `veto_rescue` opt-in for broadband dead-zone rescue. See [Residual / floor gate](#residual--floor-gate). |
| Dual-fit rescue? | Default **`dual_fit = true`** in **both** `fit` and `gate`: after a **scored** gate skip (anything but structure alignment failed), try per-shoulder fit + interior trim, re-validated by the unchanged gate floors. Opt out with **`--no-dual-fit`**. See [Dual-fit rescue](#dual-fit-rescue-g6). |
| Program-quiet (D11)? | **Plan-time** (`b_has_energy = false` → `unfillable`) and **fingerprint/analyzer** label (`donor_interior_nominal`). Not a production pre-gate skip — nominal-hole silence alone cannot distinguish true program-quiet from patchable quiet-content pauses. See [Program-quiet (D11)](#program-quiet-d11). |

---

## Pipelines

> **Together with [pipeline.md](pipeline.md):** that doc covers phases 1–5 and the `PatchAudio` run; **this doc** covers phase 4 routing (fit vs gate), per-bracket measurement, flags, and performance.

### `fill_mode = fit` (default)

#### Per-gap setup (every bracket)

```text
1. Offset map on A → B (fill_offset_mode; see pipeline.md §3)
2. Refine A gap edges; slice B haystack from full decoded B
3. Bracket routing (G1–G4 + R) — see below
4. On gate skip (except structure alignment failed): dual-fit rescue (G6, default on) → re-validate → patch or skip
```

#### Bracket routing (`evaluate_seam_gate_fit_joint`)

Bracket strategies are tried in **strict precedence**:

| # | Exit | When it returns |
|---|------|-----------------|
| **E1** | Baseline High | Baseline throat bracket: Pearson `High`, residual finalize passes |
| **E2** | Baseline accept | `baseline_only` profile (`default`, no `--full`): baseline `High` or `Marginal` — **except** `Marginal` is deferred when `anchor_seam_mode = force` (anchor runs first) |
| **E3** | Anchor High | `anchor_seam_mode` triggered; best anchor bracket Pearson `High` (+ residual) |
| **E4** | Anchor accept | `baseline_only` + best anchor `Marginal` |
| **E5** | BaselineOnly winner | No grid: best pooled candidate (ranking + residual walk), else skip |
| — | *(grid)* | Only when `fit_boundary_search = full_grid` (`--full`): enumerate all A start/end shifts |
| **E6** | Grid High | After **full** grid: best Pearson `High` among all cells (+ residual) |
| **E7** | Grid winner | Best pooled candidate after grid, else skip |

```text
baseline throat
  → E1 / E2 short-circuits
  → editorial anchor seam (if triggered) → E3 / E4
  → E5 if baseline_only and still undecided
  → boundary grid (full_grid only) → E6 / E7
  → extract B fill → queue splice (PatchAudio splice pass)
  → on scored gate skip (not structure alignment failed): dual-fit rescue (G6) if enabled
```

- **Anchor before grid.** Anchor search does not require `--full`.
- **Grid scans all cells** when reached; E6 picks the **best** `High` by `ranking_score`, not the first `High` in walk order.
- **Default profile** (`baseline_only`): no boundary grid; marginal baseline patches without grid (unless `force` anchor defers E2).

#### Per-bracket measurement (each candidate)

Every bracket (baseline, anchor, grid cell) runs the same evaluation:

```text
border templates
  → gap signature (bool / energy / auto)
  → unified B search (structure weight + min(pre,post) waveform + repeat penalty + nominal bias)
  → structure hard gate (min_structure_match_score)
  → anchor B matchability (anchor brackets only)
  → Pearson tier: High | Marginal | dead-zone skip
  → residual veto/rescue (default on; measured lazily at pool selection when residual_gate active)
```

Unified search **jointly** scores structure and waveform when sliding B — it is not “structure match, then waveform gate” as separate placement passes. See [seam-scoring.md](seam-scoring.md) for how `pre`/`post` are built; see [archive/residual-gate-wiring-plan.md](dev/archive/residual-gate-wiring-plan.md) for headroom veto / dead-zone rescue.

- **No** structure-trust waveform skip, **no** one-strong-seam / mean-only waveform shortcuts.
- `structure_trusted` is always `false` in JSON.
- Marginal patches: `min(pre, post)` in `[min_fill_correlation - fill_marginal_margin, min_fill_correlation)` → patched with `confidence: marginal`, `!` in human output.

### `fill_mode = gate` (legacy)

```text
Per gap:
  1–2. Same mapping + structure match
  3. Waveform Pearson at structure winner (may be skipped if structure-trusted)
  4. On waveform failure only: sequential A-boundary extension retries
  5. On scored gate skip (except structure alignment failed): dual-fit rescue (G6, default on)
     → re-validate → patch or skip
  6. On gate Ok: splice
```

- Structure trust, partial soften, short-gap mean, one-strong-seam apply here.
- Dual-fit eligibility is the same as fit (`dual_fit_eligible`: flag on and failure ≠ `StructureAlignmentFailed`). Gate’s extension retries run **before** dual-fit when the failure is waveform-below-threshold.
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
| `anchor_seam_mode`, `max_anchor_bracket_secs`, `max_anchors_per_side`, `anchor_seam_min_*` | **Active** — editorial anchor bracket search when triggered | **No effect** (fit only) |
| `dual_fit` / `--no-dual-fit` | **Active** — G6 rescue after scored gate skip (except structure alignment failed) | **Active** — same G6 rescue after scored gate skip (after any gate extension retries) |

**Align / scan flags** (`--clip-length`, `--num-clips`, query-reference, high-rate, gap scan knobs) are orthogonal to `fill_mode`. **Gap selection** (`--only-gaps` / `--skip-gaps`) is also orthogonal — it filters the fill plan after scan; see [gap-repair-guide.md](gap-repair-guide.md) § Iterative subset patching.

---

## A-boundary extension (often confused with “mode”)

Extension flags control **whether A’s gap edges may move** during patch planning. They do **not** select `fit` vs `gate`.

### Fit: joint boundary search

When `gap_end_extend_on_post_seam_fail` and/or `gap_start_extend_on_pre_seam_fail` is **true** (defaults) **and** `fit_boundary_search = full_grid` (`--full`):

- Runs **after** baseline and editorial anchor seam (see [Fit-joint routing](#fill_mode--fit-default) above).
- Enumerates a grid of `(start, end)` brackets within `gap_end_extend_max_ms` (default **500 ms**) and `gap_end_extend_step_ms` (default **20 ms**), capped at ~**12 steps per axis** (~144 non-baseline cells).
- Each cell runs **full per-bracket measurement** (unified B search + gates).
- **E6:** after the full grid, the best Pearson `High` by `ranking_score` is finalized (with residual); **E7:** otherwise the best pooled candidate by ranking + residual walk.

With **`--no-gap-end-extend --no-gap-start-extend`**: only the **baseline** bracket is evaluated (no grid). Still **fit** placement and tiering.

Under **`fit_boundary_search = baseline_only`** (default profile, no `--full`): extension flags and `gap_end_extend_max_ms` do **not** run the grid or add B haystack slack; `-v` emits a `repair note:` when those settings are stored but inactive. Use **`--full`** to enable the grid and haystack slack.

### Gate: sequential retries

When waveform check **fails**:

1. Try extending **gap end** in steps (if post-seam extension enabled and candidate rules pass).
2. Else try shifting **gap start** earlier (pre-seam extension).

Gate retries use the same `gap_end_extend_*` ms limits but **different** eligibility rules (see [cli-output.md](cli-output.md) § Boundary extension retries).

---

## Program-quiet (D11)

**Program-quiet** means both masters are quiet at the same program time — there is nothing to fill. This is
**not** the same as “B’s nominal hole interior is silent,” which can also describe a **shared content pause**
that structure/energy matching should still patch.

| Layer | How it is detected | Outcome |
|-------|-------------------|---------|
| **Scan / plan** | `b_has_energy = false` on the mapped B span | `unfillable` — gap never enters patch |
| **Fingerprint / `--gap-fingerprints`** | `donor_interior_nominal.silence_fraction ≥ 0.5` | Analyzer label `program_quiet_skip` — metrics only, not a patch router |
| **Dual-fit (G6)** | Same nominal occupancy check inside `try_dual_fit` | Declines fully program-quiet donors after the primary gate already failed |
| **Production patch** | Seam gate (+ dual-fit) decides skip/patch | No pre-gate short-circuit on nominal silence |

Common on long-form pairs where A has tail padding silence B does not share — usually caught at scan as
`unfillable`. Do not lower Pearson floors for analyzer-tagged program-quiet gaps; tune scan if A should not
have been flagged (P7).

See [gap-fingerprint.md](dev/gap-fingerprint.md) § Registration & dual-fit measurements and [gap-scan.md](gap-scan.md) § Mapping to B and fillability.

---

## Dual-fit rescue (G6)

After the primary placement pipeline fails with a **scored** gate failure other than
`StructureAlignmentFailed`, dual-fit (default **on**) attempts a distinct repair path.
That applies in **both** `fill_mode = fit` and `fill_mode = gate` — `characterize_region`
always routes eligible skips through `skip_or_dual_fit`; there is no fill-mode gate on the flag.

- **Fit:** after Fit-joint routing exhausts (baseline → optional anchor → optional grid).
- **Gate:** after structure/waveform evaluation and any sequential A-boundary extension retries
  (retries only run for waveform-below-threshold; other scored failures go straight to dual-fit).

```text
seam-local peaks on A pre/post shoulders (±600 ms)
  → independent per-shoulder lag on B
  → interior trim to reconcile length
  → donor continuity + step-real checks
  → re-validate assembled fill with unchanged gate floors (Pearson + residual)
  → patch or fall through to skip
```

Dual-fit does **not** run when structure alignment failed (no bracket scored). It also declines
donor-broken bridges (internal silent run in the donor interior) and fully program-quiet donors.

| Control | Default | Opt out |
|---------|---------|---------|
| TOML | `dual_fit = true` | `dual_fit = false` |
| CLI | *(none — on by default)* | `--no-dual-fit` |
| Force on | `--dual-fit` | — |

Rescued gaps report tier/confidence from the re-validated seams like other patches, but are marked distinct
from ordinary bracket-search fits: status shows `patched (dual-fit pre→post)` (or `! patched (dual-fit …)`
when marginal), and `dual_fit_used: true` appears on `status.patched` and `tags` in JSON (verbose `gap tags:`
adds `dual_fit=true`). See [cli-output.md](cli-output.md) § Gap patch gate and skip reasons.
Typical rescue profile: scored primary-gate skips where both shoulders align at their own lag but
rigid lag-0 (fit) / structure-winner (gate) placement failed (see [gap-repair-guide.md](gap-repair-guide.md) § W7).

For calibration / regression against the pre-A3 bracket-only path, use `--no-dual-fit` (D6 byte-identical to
legacy skips on those gaps).

---

## Waveform placement details

> Full mechanics of how `pre`/`post` seams are identified and scored (border extraction, standoff/trim, channel selection, peak-normalized Pearson, tiers): [seam-scoring.md](seam-scoring.md).

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

### Multichannel seams (5.1 / surround)

Seam Pearson is **peak-normalized** (level is removed) and computed on the channel(s) that **carry signal** — those within ~20 dB of the loudest A-side border channel — taking the best match among them. Silent channels (e.g. empty surrounds/LFE, or near-silent front L/R in a **center-dominant 5.1 mix**) are skipped, so they neither veto nor inflate a splice. When every channel is near-silent the scorer falls back to the mono downmix.

This matters because seam Pearson on **near-silent audio is noise** (peak-normalized noise correlates to ~0). If scoring were locked to front L/R, a 5.1 mix with dialogue in the center channel and quiet fronts would show **pre/post ≈ 0** and skip a perfectly fillable gap. Following the signal-bearing channel(s) gives such gaps an honest seam score. (Mono/stereo content is unaffected — all channels carry signal, so all are scored as before.)

The fit-mode **residual/floor** measurement follows the *same* selected channels (not a mono downmix that quiet surrounds would dilute): cancellation depth is measured per selected channel, while the integer alignment lag is found once across all of them by summed correlation. See [seam-scoring.md](seam-scoring.md) § Residual channel policy and [archive/residual-channel-alignment-plan.md](dev/archive/residual-channel-alignment-plan.md).

---

## Residual / floor gate

**Fit mode only.** Runs as the last step of [per-bracket measurement](#per-bracket-measurement-each-candidate) (lazy at pool selection when `residual_gate` is active — see [pipeline.md](pipeline.md) §4).

| Mode | Default | Effect |
|------|---------|--------|
| `veto` | **yes** | Skip when informative floor + headroom above margin (anti-echo) |
| `veto_rescue` | no | Also upgrade Pearson dead-zone skips when cancellation is strong |
| `off` | no | Measure only when `measure_residual` / debug |

Design: [archive/residual-gate-wiring-plan.md](dev/archive/residual-gate-wiring-plan.md). JSON: `residual_band`, `residual_db` / `floor_db` / `headroom_db`; skip reason `residual_headroom_exceeded`.

```toml
[repair]
# residual_gate = "veto"          # off | veto | veto_rescue
# residual_floor_ok_db = -50.0
# residual_headroom_margin_db = 6.0
# residual_lag_secs = 0.01
```

---

## Editorial anchor seam

**Status:** shipped (fit mode). Design: [archive/TEMP-anchor-seam-plan.md](dev/archive/TEMP-anchor-seam-plan.md).

When a fillable gap has a **quiet scan throat** but salient contour nearby (speech peak, bool onset in the flanking context halves), throat-only Pearson often lands in **W5** (symmetric weak, dead zone). Anchor seam searches **editorial boundaries** on A (energy peaks, bool transitions rising pre / falling post), enumerates feasible brackets, and scores B-side matchability at those anchor windows. Context geometry: [Signature context and contour geometry](#signature-context-and-contour-geometry).

```text
Per gap (fit, when anchor search runs):
  1. Baseline throat unified search
  2. If anchor_seam_mode triggers (after E1; E2 may defer marginal baseline when force):
       list_anchor_candidates_a → feasible brackets (min move_frames first)
       score each bracket (per-bracket measurement pipeline)
  3. Best anchor High/Marginal may return (E3/E4); else continue to grid or E5/E7
```

| Setting | Default | CLI | Notes |
|---------|---------|-----|-------|
| `anchor_seam_mode` | `auto` | `--anchor-seam-mode` | `auto` = below marginal floor + contour in flanking context ([§ Signature context and contour geometry](#signature-context-and-contour-geometry)); `force` = always try anchor before grid (defers E2 marginal accept under `baseline_only`); `off` = disable |
| `max_anchor_bracket_secs` | 5.0 | `--max-anchor-bracket-secs` | Max pre↔post anchor span |
| `max_anchors_per_side` | 5 | `--max-anchors-per-side` | Cap per side (incl. scan fallback) |
| `anchor_seam_min_prominence` | 0.0 | `--anchor-seam-min-prominence` | Energy peak filter |
| `anchor_seam_min_match_pearson` | 0.12 | *(config)* | Per-anchor B matchability Pearson floor |
| `anchor_seam_min_xcorr_peak` | 0.5 | *(config)* | Tier-2 GCC-PHAT rescue when Pearson ambiguous |
| `anchor_seam_xcorr_ambiguous_band` | 0.15 | *(config)* | Pearson band below floor that may trigger xcorr |

**Orthogonal to:**

- **`--full` / boundary grid** — anchor search runs under `baseline_only` when triggered; does not require the grid.
- **Patch anchors** (`anchored_retry`) — those fix offset drift between passes; anchor seam fixes seam placement for one gap.

**Output:** `anchor_seam_used`, `anchor_bracket_move_frames` on patched gaps (JSON + `-v` `gap tags:`); human `patched (anchor …)` when anchor wins. See [cli-output.md](cli-output.md) and [gap-repair-guide.md](gap-repair-guide.md) § Editorial anchor seam.

```toml
[repair]
anchor_seam_mode = "auto"          # off | auto | force
# max_anchor_bracket_secs = 5.0
# max_anchors_per_side = 5
# anchor_seam_min_prominence = 0.0
# anchor_seam_min_match_pearson = 0.12
# anchor_seam_min_xcorr_peak = 0.5
# anchor_seam_xcorr_ambiguous_band = 0.15
```

```powershell
# W5 symmetric-weak skips with contour on energy path
clip-sync-repair a.mkv b.mkv --mux out.mp4 `
  --gap-signature-mode auto `
  --anchor-seam-mode auto `
  -v
```

`-v` emits `repair note: anchor_seam_mode=off: editorial anchor search inactive; use --anchor-seam-mode auto|force` when the mode is off.

---

## Patch anchors

**Status:** `anchored_retry` shipped (2026-06-20). See [archive/patch-anchor-offset-plan.md](dev/archive/patch-anchor-offset-plan.md).

Some runs patch several gaps cleanly (`slide=+0.35s` in verbose) while others fail seam search because the **nominal B map** from alignment is off by hundreds of ms at that point on A — the true dropout sits near the edge of `fill_border_search_secs`, not because `fit` or `gate` chose wrong.

**Patch anchors** reuse what easy gaps already measure: each successful patch records `align_adjustment_secs` (structure + waveform slide vs the mapped nominal). Flow:

```text
Pass 1: patch all gaps (clip-based offset; collect outcomes before splice)
    → build anchor table from high-confidence successes
Pass 2 (anchored_retry): retry failed gaps with improved gap_offset_secs
    → interpolate local Δ from nearby anchors (+ clip start/end when available)
    → re-run the same fill_mode (fit or gate) with a centered haystack
```

Single-pass `anchored` was removed (never wired under collect-then-splice); use `anchored_retry`.

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

**Mode-coupled nominal bias:** energy-resolved gaps use `fill_fit_energy_nominal_bias_scale` (default `0.25`) for the distance-from-nominal penalty; bool keeps the base `fill_fit_nominal_bias_scale` (default `1.0`). The lower energy scale lets a confident contour override a **drifted nominal B map** (energy mode self-corrects), while only loosening far-off candidates — sub-second offsets are unaffected. Raise toward `1.0` to restore hard anchoring for energy gaps.

**Context length (`gap_signature_context_secs`, default `3.0`):** keep 3 s. Larger values (10 / 30 s) widen the matched signature window for ambiguous/long drift gaps, but the synthetic corpus (contexts 3 / 10 / 30) showed no measurable patch benefit and a longer context costs more B decode/memory per gap. A manual knob to try on a stubborn gap, not a default to raise.

See [Signature context and contour geometry](#signature-context-and-contour-geometry) for where contour is measured on the timeline, how the >5% flat test works, and how that geometry ties to editorial anchor seam.

### Signature context and contour geometry

Fit-mode structure matching, `gap_signature_mode = auto` resolution, and editorial anchor seam (`anchor_seam_mode = auto|force`) all use the **same context halves** on A — built from the **refined** gap edges (`refine_gap_frames`), not the raw scan timestamps. Code: `build_gap_energy_signature` / `build_gap_context_signature` in `domain/gap_energy.rs` and `domain/gap_structure.rs`; anchor candidates reuse the same frame ranges (`list_anchor_candidates_a` in `domain/gap_anchor_seam.rs`).

#### Where on the timeline

```text
Timeline on A (defaults: context = 3 s, bin = 50 ms):

  |←—— up to gap_signature_context_secs ——→|  gap interior  |←—— up to gap_signature_context_secs ——→|
  [              pre half                  )[  start … end  ]([              post half                  )
       contour / energy / bool here                    NOT measured here
```

| Half | Frame range (refined edges) | Includes gap interior? |
|------|-----------------------------|-------------------------|
| **Pre** | `[gap_start − context, gap_start)` | No — ends at gap start |
| **Post** | `[gap_end, gap_end + context)` | No — starts at gap end |

Salient audio **inside** the scanned silence (the dropout throat) does **not** count toward contour or anchor candidates. Variation must appear in the **flanking** context — anywhere from immediately adjacent to each refined edge out to `gap_signature_context_secs` (default **3 s**). A speech onset **2 s before** gap start qualifies; one **4 s** before does not unless you raise `--gap-signature-context-secs`.

This is a different timescale from waveform seam Pearson (~250 ms at the border; see [seam-scoring.md](seam-scoring.md) §4).

#### Bin resolution and energy envelope

| Knob | Default | Role |
|------|---------|------|
| `gap_signature_context_secs` | `3.0` | Length of each pre/post half |
| `gap_signature_bin_ms` | `50` | One structure bin (~60 bins per 3 s at 48 kHz) |

Per bin (energy path): mono downmix → gated log-RMS (`ln(1+rms)` above silence floor) → **peak-normalize each half separately** before matching or flat tests.

#### When `auto` picks energy vs bool

`gap_signature_mode = auto` uses the **energy** envelope when `has_anchor_seam_contour()` is true; otherwise it falls back to **bool** (flat room tone, steady drone, near-uniform activity).

**Energy contour** (`energy_envelope_is_flat`): after peak-normalizing a half, if `(max − min) / peak` across all bins in that half is **≤ 5%**, the half is **flat**. Contour exists when **either** the pre half **or** the post half is non-flat.

**Bool contour** (same pre/post windows): activity **transitions** (silent ↔ active) in either half, or **mixed** active and silent bins across pre+post — not flat silence and not uniform activity throughout.

#### Uses of the same geometry

| Consumer | What contour / context controls |
|----------|--------------------------------|
| **`auto` signature mode** | Energy vs bool structure tier for unified B search |
| **`anchor_seam_mode = auto`** | Whether anchor bracket search runs (with weak throat Pearson); same `has_anchor_seam_contour()` |
| **Editorial anchor seam** | Energy-peak and bool-transition **candidate** frames on A (plus scan-refined fallback at throat edges) |

Anchor `auto` also requires baseline throat `min(pre, post) < min_fill_correlation − fill_marginal_margin` (default **0.27**). `force` skips the contour gate. Default `anchor_seam_mode` is **`auto`**. See [Editorial anchor seam](#editorial-anchor-seam).

**Practical checks:** run with `-v` and read `signature_mode=` (resolved energy vs bool). When anchor is explicitly `off`, `-v` emits `repair note: anchor_seam_mode=off`. Flat flanks → bool path, no anchor `auto` trigger; W5 rescue needs salient contour in the ±3 s **flanks**, not only inside the hole.

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

**Symmetric weak + contour (W5 anchor rescue):**

```powershell
clip-sync-repair recording_with_gaps.mp4 reference.mkv `
  --mux repaired.mp4 `
  --gap-signature-mode auto `
  --anchor-seam-mode auto `
  -v
```

Add `--full` if gaps still skip after anchor seam (boundary grid shifts A bracket; separate knob).

---

## Config keys (placement / seam)

Most keys below drive **fit** placement. `dual_fit` is shared: G6 rescue runs after scored skips in
**both** `fit` and `gate`. Extension ms/step keys also apply to gate’s sequential retries.

| Key | Default | CLI | Notes |
|-----|---------|-----|--------|
| `fill_mode` | `"fit"` | `--fill-mode` | `"gate"` for legacy |
| `fill_border_search_secs` | `10.0` | `--fill-border-search-secs` | B slide radius (unified search) |
| `fill_align_margin_secs` | `1.0` | `--fill-align-margin-secs` | Extra B extract padding |
| `gap_signature_context_secs` | `3.0` | `--gap-signature-context-secs` | Structure signature context |
| `fill_length_slack_secs` | `1.0` | `--fill-length-slack-secs` | B fill-end slide slack (end-search / `max_fill` only) |
| `fill_extract_tail_slack_secs` | `5.0` | `--fill-extract-tail-slack-secs` | B haystack tail beyond refined end (extract / fingerprint `pad_tail`; `max` with align margin) |
| `fill_repeat_penalty_weight` | `0.4` | `--fill-repeat-penalty-weight` | Penalize repeat-at-seam when seams weak (0 = off) |
| `fill_fit_structure_weight` | `0.35` | `--fill-fit-structure-weight` | Unified scorer |
| `fill_fit_waveform_weight` | `0.65` | `--fill-fit-waveform-weight` | Unified scorer |
| `fill_fit_energy_nominal_bias_scale` | `0.25` | — | Distance-from-nominal penalty for energy-resolved gaps (< base `1.0`; energy self-corrects drifted nominal) |
| `fill_marginal_margin` | `0.08` | — | Warn band below `min_fill_correlation` |
| `fill_absolute_floor` | `0.12` | — | Hard skip floor |
| `gap_end_extend_max_ms` | `500` | `--gap-end-extend-max-ms` | A-boundary grid / gate retries |
| `gap_end_extend_step_ms` | `20` | `--gap-end-extend-step-ms` | Grid/retry step |
| `max_fill_align_adjustment_secs` | `0.5` | `--max-fill-align-adjust-secs` | Legacy polish window |
| `anchor_seam_mode` | `auto` | `--anchor-seam-mode` | Editorial anchor search: `off` \| `auto` \| `force` |
| `max_anchor_bracket_secs` | `5.0` | `--max-anchor-bracket-secs` | Max anchor bracket span |
| `max_anchors_per_side` | `5` | `--max-anchors-per-side` | Anchor candidates per side |
| `anchor_seam_min_prominence` | `0.0` | `--anchor-seam-min-prominence` | Energy peak prominence floor |
| `anchor_seam_min_match_pearson` | `0.12` | — | Per-anchor B matchability Pearson |
| `anchor_seam_min_xcorr_peak` | `0.5` | — | Tier-2 xcorr rescue floor |
| `anchor_seam_xcorr_ambiguous_band` | `0.15` | — | Pearson band that may trigger xcorr |
| `dual_fit` | `true` | `--no-dual-fit` | G6 per-shoulder rescue after scored gate skip (**both** fit and gate) |

**Fit-mode short B bracket:** when structure match returns fewer frames than the A gap, fit mode greedily extends into contiguous B audio frame-by-frame while padded `min(pre, post)` does not fall and `fill_repeat_correlations` post-repeat stays bounded; remaining frames are zero-padded. Gate mode still blind-extends then pads.

CLI: `--fill-fit-structure-weight`, `--fill-fit-waveform-weight`, and the B haystack flags above override config when passed on the command line.

---

## Report / JSON

| `fill_mode` | Human patched row | JSON notes |
|-------------|-------------------|------------|
| `fit` | `patched (pre→post)` or `! patched` if marginal; `patched (anchor …)` when editorial anchor wins | `confidence`, `gap_*_adjust_frames`, `structure_trusted: false`, optional `anchor_seam_used` / `anchor_bracket_move_frames` |
| `gate` | `patched (struct …)` if structure-trusted | `structure_trusted: true` when waveform skipped |

Full field list: [json-output.md](json-output.md) § `GapPatchStatus`.
