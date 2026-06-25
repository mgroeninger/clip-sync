# Seam identification & scoring

How the gap-fill **seam correlations** (`pre` / `post`) you see in repair output are built and what makes them pass or skip. This is the reference behind the score bands in [gap-repair-guide.md](gap-repair-guide.md) § Seam patterns and the scoring formula in [gap-fill-modes.md](gap-fill-modes.md) § Waveform placement details.

A seam is the **boundary between A's real audio and the spliced B fill** — one at the gap start (`pre`) and one at the gap end (`post`). Each is a peak-normalized Pearson correlation between A's border audio and B's audio at the chosen fill placement. They answer "does the splice line up?", not "where on B does the content live?" (that is the structure tier).

Source: `domain/policies.rs` (border extraction, channel selection, Pearson) and `domain/gap_fill_fit.rs` (tiers). Per-gap orchestration: `application/patch_region.rs` (`evaluate_seam_gate`).

---

## Where seams sit in the per-gap flow

```text
refine gap edges on A
  → slice B haystack (context + border search + margin)
  → structure match on B          → placement (fill start/end on B)
  → SEAM SCORING  ← this document
       build A border templates → select channels → peak-normalized Pearson → pre, post
  → structure gate + waveform tier → patch / marginal / skip
  → splice + crossfade + normalize
```

See [pipeline.md](pipeline.md) for the whole repair pipeline.

---

## 1. Border template identification

For each gap, A's audio on both sides is captured into **border templates** (`gap_border_frame_range`, `border_templates_for_gap`):

1. **Capture window** — `border_frames` of A immediately before the gap (`pre`) and after it (`post`).
2. **Silence walk-off** — frames adjacent to the gap that are below the silence floor (`silence_peak_fraction`, `absolute_silence_rms`) are walked off the gap-facing edge, so the template starts at real audio, not the leading/trailing silence of the dropout.
3. **Standoff** — `border_standoff_frames` (`--border-standoff-secs`, default 0.35 s) drops the audio *immediately* adjacent to the dropout, so the seam is not graded against the click/fade right at the gap edge.
4. **Low-energy trim** — `trim_low_energy_suffix` (pre) / `trim_low_energy_prefix` (post) drop the quiet tail/head below **12 % of the template's own peak**, so a fade into/out of the dropout doesn't dominate the template.

The result is the loudest, gap-adjacent A audio available on each side. Templates are built both as a **mono downmix** and **per channel**.

> If a side has no audio above the floor within the window, its template comes back **empty** — and an empty template scores **0** (see §3). This is a real source of `pre`/`post ≈ 0`.

## 2. Channel selection (multichannel)

`seam_score_channel_indices` chooses *which* channels to score from the A-side templates:

- Per-channel **mean-square energy** is computed; channels within **~20 dB of the loudest** (mean-square ratio ≥ 0.01) are kept. Silent surrounds/LFE, or near-silent front L/R in a **center-dominant 5.1 mix**, are dropped — they neither veto nor inflate the splice.
- The kept channels are each scored; the seam takes the **best** (`best_channel_correlation`).
- If **every** channel is near-silent, the selection is empty and scoring falls back to the **mono downmix**.

This is why a 5.1 mix with dialogue in the center and quiet fronts is scored on the **center** channel rather than front-channel noise. Mono/stereo content is unaffected — all channels carry signal, so all are scored. (See [gap-fill-modes.md](gap-fill-modes.md) § Multichannel seams.)

### Residual channel policy

Residual/floor cancellation (fit mode) follows the **same** selection. `selected_seam_channels` recomputes `seam_score_channel_indices` from the identical border spec, so `seam_chosen_and_floor_multichannel` measures cancellation per selected channel against each `b_ch[ch]` instead of a mono downmix that quiet surrounds would dilute. Aggregation: the **veto** (`worst_headroom_db`) follows the worst-headroom channel, while `informative` follows the **best-cancelling** channel so a noisy surround can't flip the same-master regime off. Empty selection falls back to the mono downmix path unchanged. Full design: [TEMP-residual-channel-alignment-plan.md](TEMP-residual-channel-alignment-plan.md).

## 3. Seam correlation (peak-normalized Pearson)

`seam_pearson` correlates two equal-length windows. **Pearson correlation is itself scale-invariant**, so encode-to-encode *level* differences don't matter; *shape* does. (The `peak_normalize_f64` call in this path is therefore a no-op — it cannot change the correlation — and is dead work; see [residual-gate-findings.md](residual-gate-findings.md) G2. The level-invariance is a property of Pearson, not of that call.) It returns **0.0** when the windows are empty or unequal length. Because correlation keys on shape, not level, **near-silent or broadband noise-like audio correlates to ~0** — its waveform is dominated by noise, which differs sample-to-sample between two sources even when they are the same master. This is exactly why broadband seams land in the Pearson dead zone and need the residual gate (see [residual-gate-wiring-plan.md](residual-gate-wiring-plan.md) §2).

## 4. Pre/post windows at a placement

For a structure placement at B frame `start` with `gap_frames` length (`fill_seam_correlations`):

| Seam | A side | B side |
|------|--------|--------|
| `pre`  | last `seam_gate_frames` of the A **pre**-border  | `b[start − w .. start]` |
| `post` | first `seam_gate_frames` of the A **post**-border | `b[start + gap_frames .. start + gap_frames + w]` |

`seam_gate_frames` (`w`) is the seam comparison window, capped by **`fill_seam_search_secs`** (default 0.25 s ≈ 250 ms). Note this is *narrow* — only the immediate ~250 ms at the boundary — whereas the **energy signature** matches a much wider envelope (`gap_signature_context_secs`, default 3 s). A difference that lives outside the 250 ms border won't move the waveform seam.

## 5. Gates & tiers — how pre/post decide the outcome

Two gates consume the seam scores; both treat **short gaps** (≤ `short_gap_mean_correlation_secs`, default 2.0 s) more leniently.

**Structure gate** (`structure_passes_gate`) — the structure-tier `pre`/`post`:
- short gap: `(pre + post) / 2 ≥ min_structure_match_score` (default 0.55)
- else: both `≥ min_structure_match_score`

**Fit-mode waveform tier** (`classify_fill_waveform_confidence`) — uses `min(pre, post)`:

| Band | Condition on `min(pre, post)` | Outcome |
|------|-------------------------------|---------|
| **High** | ≥ `min_fill_correlation` (0.35) | patch |
| **Marginal** | ≥ `min_fill_correlation − fill_marginal_margin` (0.27) | warn-patch |
| **Dead zone** | ≥ hard floor and < 0.27 | **skip** (`boundary correlation below threshold`) |
| **Hard skip** | < hard floor | **skip** |

Hard floor = `min(fill_absolute_floor, min_fill_correlation)` = **0.12** by default. Fit is deliberately **`min`-based** (both seams must hold) — its anti-echo guard. It has **no** one-strong-seam shortcut.

**Gate-mode waveform** (`seams_pass_correlation_gate`) — for short gaps, passes on the **mean** of `pre`/`post`, or on a **one strong seam** when `short_gap_one_strong_seam_fallback` is set. This is what lets `gate` patch an **asymmetric** gap (strong `post`, weak `pre`) that `fit` skips — at the cost of accepting a weak seam.

The `pre`/`post` pair also maps to a **`seam_shape`** tag (`balanced`, `asymmetric_post`, `asymmetric_pre`, `symmetric_weak`) — see [gap-repair-guide.md](gap-repair-guide.md) § Vocabulary and the W1–W6 patterns.

---

## Diagnostics

Per-gap seam detail (channels selected and per-channel correlations at the winning placement) logs at **debug** from `evaluate_seam_gate`:

```powershell
$env:RUST_LOG = "warn,clip_sync_repair::application::patch_region=debug"
clip-sync-repair A.mkv B.m4v --gap-signature-mode bool 2>&1 | Select-String "seam channel diagnostics"
$env:RUST_LOG = ""
```

Each line reports `start_frame`, `seam_pre`/`seam_post`, `structure_pre`/`structure_post`, the resolved `signature`, `selected_channels`, `per_channel` `(pre, post)` for every channel (`NaN` where the window didn't fit), and the `mono` fallback. Use it to tell a real content mismatch from a channel-selection artifact (e.g. low front-channel scores but a high center-channel score).

A companion `fill residual channel breakdown` line (same debug target) reports `selected_channels` and per-channel `(channel, headroom_db)` for the residual/floor measurement, so you can see which channel drove a residual headroom veto.

## Config knobs that shape seams

| Knob | Effect |
|------|--------|
| `--border-standoff-secs` (`border_standoff_secs`, 0.35) | Audio excluded immediately adjacent to the dropout |
| `fill_seam_search_secs` (0.25) | Seam comparison window `seam_gate_frames` (~250 ms) |
| `silence_peak_fraction`, `absolute_silence_rms` | Silence floor for the gap-edge walk-off |
| `min_fill_correlation` (0.35) | High-tier floor on `min(pre, post)` |
| `fill_marginal_margin` (0.08) | Marginal band below High |
| `fill_absolute_floor` (0.12) | Hard-skip floor (with `min_fill_correlation`) |
| `min_structure_match_score` (0.55) | Structure-tier gate |
| `short_gap_mean_correlation_secs` (2.0) | Short-gap mean leniency (both gates) |
| `short_gap_one_strong_seam_fallback` | One-strong-seam acceptance (**gate mode only**) |

## Code map

| Step | Function (crate `clip-sync-repair`) |
|------|-------------------------------------|
| Border frame range + silence walk-off + standoff | `domain/policies.rs::gap_border_frame_range` |
| Mono / per-channel templates + low-energy trim | `policies::border_templates_for_gap`, `border_templates_per_channel_for_gap`, `trim_low_energy_prefix/suffix` |
| Channel selection | `policies::seam_score_channel_indices` |
| Peak-normalized Pearson | `policies::seam_pearson` |
| Pre/post at a placement | `policies::fill_seam_correlations` |
| Splice-time per-channel seams | `policies::fill_splice_seam_correlations_interleaved` |
| Per-channel debug diagnostics | `policies::seam_channel_diagnostics` |
| Fit waveform tiers | `domain/gap_fill_fit.rs::classify_fill_waveform_confidence`, `fit_mode_waveform_floor_passes` |
| Structure gate / gate-mode waveform gate | `application/patch_region.rs::structure_passes_gate`, `seams_pass_correlation_gate` |

---

## Where this fits in the pipeline

Seam scoring is **phase 4, step 4d** of the repair pipeline (after structure match, before the splice). For the whole pipeline — align → scan → fill plan → per-gap patch → write/mux — see [pipeline.md](pipeline.md).

---

## Related reading

- [gap-repair-guide.md](gap-repair-guide.md) — reading a run; tiers, seam shapes, vocabulary
- [gap-fill-modes.md](gap-fill-modes.md) — `fit` vs `gate`, flag interactions, multichannel seams, performance
- [cli-output.md](cli-output.md) — repair gap outcome report layout
- [README.md](../README.md) § Gap patching pipeline
