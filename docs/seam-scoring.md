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

Residual/floor cancellation (fit mode) follows the **same** selection. `selected_seam_channels` recomputes `seam_score_channel_indices` from the identical border spec, so `seam_chosen_and_floor_multichannel` measures cancellation per selected channel against each `b_ch[ch]` instead of a mono downmix that quiet surrounds would dilute.

**Shared alignment, per-channel depth.** The integer lag is a single physical quantity (same master, same clock), so it is found **once across all selected channels** by `shared_alignment_lag` — the lag maximizing the *summed* peak-normalized correlation — then each channel fits only its scalar gain and residual at that fixed lag. Summing correlations (not downmixing waveforms) is what makes this robust to not knowing which channel carries the gap: a loud channel whose B content doesn't match correlates ~0 at every lag and never pulls the alignment, while the matching channel(s) shape a sharp peak at the true lag. (A literal mono downmix fails here — it injects the loud non-matching channels' energy into one waveform and drags the lag off the true value, so even the good channel stops cancelling.)

Aggregation: the **veto** (`worst_headroom_db`) follows the worst-headroom channel, while `informative` follows the **best-cancelling** channel so a noisy surround can't flip the same-master regime off. Empty selection falls back to the mono downmix path unchanged. Full design: [archive/residual-channel-alignment-plan.md](dev/archive/residual-channel-alignment-plan.md).

## 3. Seam correlation (peak-normalized Pearson)

`seam_pearson` correlates two equal-length windows via `normalized_correlation` (z-score Pearson). **Pearson correlation is scale-invariant**, so encode-to-encode *level* differences don't matter; *shape* does. It returns **0.0** when the windows are empty or unequal length. Because correlation keys on shape, not level, **near-silent or broadband noise-like audio correlates to ~0** — its waveform is dominated by noise, which differs sample-to-sample between two sources even when they are the same master. This is exactly why broadband seams land in the Pearson dead zone and need the residual gate (see [archive/residual-gate-wiring-plan.md](dev/archive/residual-gate-wiring-plan.md) §2).

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

## 6. Dual-fit repair — reconciling a length step across the seam

**Canonical wire spec** for the dual-fit rescue (operator/mode view: [gap-fill-modes.md](gap-fill-modes.md) § Dual-fit rescue (G6); classification tag: [gap-repair-guide.md](gap-repair-guide.md) § Dual-fit rescue (W7)). **Implemented** in production — `domain/dual_fit.rs` (shared scan + production primitive) and `application/patch_audio.rs::skip_or_dual_fit → try_dual_fit`; **default on** (`RepairConfig.dual_fit = true`, `--no-dual-fit` opt-out). The offline predictor of this repair is the fingerprint's `splice_dualfit` field — [gap-fingerprint.md](dev/gap-fingerprint.md) § Registration & dual-fit measurements.

**Mechanism.** At a repair gap, A has a quiet/silent hole between two un-stretched shoulders. Each shoulder registers against B at its **own lag**; the lags differ by a **step** (`splice.step_ms`). A single rigid donor shift cannot satisfy both seams — one seam is always off by the step. Dual-fit places each shoulder independently, then reconciles the step with a **trim or pad at the lowest-energy interior sample** of the fill — a pure length edit, not a within-side warp (the content is un-stretched within each side).

**Algorithm (per gap):**

1. **Detect** — run only on gaps that `dualfit_target()` selects (analyzer `gap_fingerprint_corpus.rs`): `skip` ∧ **bracket-exhausted** (`StructureAlignmentFailed` excluded — no bracket was ever scored) ∧ `splice_dualfit.gate_pass` ∧ **`step_is_real()`** (`post_own − post@pre ≥ DUALFIT_STEP_REAL_MARGIN` 0.15 — the step materially improves the seam, not merely clears the floor) ∧ `donor_interior.continuous` ∧ ¬program-quiet. **Do not** run on gaps that already patch (≥1 bracket passes) or on uniqueness (`dualfit_candidate`) — it does not predict placement seam viability. On the re-anchor corpus this scopes to **9 gaps** (1·g3, 1·g5, 1·g22, 2·g1, 2·g2, 5·g6, 7·g2, 7·g3, 7·g4). Note: post-±600 ms search `gate_pass` is degenerate (nearly every gap passes), so the load-bearing gates are **step-real ∧ donor-occupancy**.
2. **Fit each seam at its seam-local lag, re-anchored on nominal `b_mapped`** — search each shoulder ±`SEAM_LOCAL_SEARCH_MS` (600 ms, the `baseline_lag` range) around the nominal geometry anchor (pre butts at `b_mapped_start`, post at `b_mapped_start + gap_frames`) and take the peak; the seam **defines its own placement**. **Do NOT anchor on the gross 1 s `baseline_lag`** — it can lock onto distant content and clip a live seam (e.g. `7·g3`: gross pre −319 ms vs seam +18 ms, which a gross-anchored ±100 ms window missed entirely). `splice_dualfit.pre/post_seam_z` (whole-curve z-score) is the alias guard against the wide search locking onto a far periodic rival — **not** the ±30 ms prominence (which over-flags correct-but-periodic content).
3. **Reconcile the step** — extract the B bridge `[b_pre .. b_post]`; `trim_frames = bridge_frames − gap_frames` (= the step in samples). Trim or pad `|trim_frames|` at the **lowest-RMS interior sample** of the fill region (smallest audible splice). Interior edit only — shoulders stay at their own lags.
4. **Validate with the unchanged gate** — score pre/post seams against B at the seam-local-refined placements (step 2), using `fill_seam_search_secs` (250 ms, §4) and the existing `min_fill_correlation` / `fill_absolute_floor` thresholds (§5). A bad length edit must fail exactly as a bad shift does today — **strict gate, no loosening**. Production re-scores the assembled/trimmed fill with the real `fill_splice_seam_correlations_interleaved` + `classify_fill_waveform_confidence` (the same primitives every other fill path uses), falling back to the skip on failure — it does **not** trust an assumed confidence.
5. **Reject** to skip (as today) if post-reconciliation validation fails, the step was `edge_pinned` (GIGO), or donor continuity is false. Gate-pass alone is not sufficient — donor-BROKEN gaps (seams align but the gap interior is silent, e.g. 1·g19: seams 0.998 yet B interior silent) must stay skipped: filling them inserts silence.

**Why it is a distinct operation** (not a re-run of bracket search): the winning bracket's boundary move is *not* the throat step — no patched gap has `|step|` within 20 ms of a bracket delta. Dual-fit is an interior length edit, not another anchor/boundary search.

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
| Border frame range + silence walk-off + standoff | `domain/policies/gap_borders.rs::gap_border_frame_range` |
| Mono / per-channel templates | `policies::border_templates_for_gap`, `border_templates_per_channel_for_gap` (`gap_borders.rs`) |
| Low-energy trim | `policies/seam_splice.rs::trim_low_energy_prefix/suffix` |
| Interleaved → mono / per-channel downmix | `domain/pcm.rs::interleaved_to_mono`, `interleaved_to_channels` |
| Channel selection | `policies/seam_scoring.rs::seam_score_channel_indices` |
| Peak-normalized Pearson | `policies/seam_scoring.rs::seam_pearson` |
| Pre/post at a placement | `policies::fill_seam_correlations` (`seam_scoring.rs`) |
| Splice-time per-channel seams | `policies::fill_splice_seam_correlations_interleaved` (`seam_scoring.rs`) |
| Per-channel debug diagnostics | `policies::seam_channel_diagnostics` (`seam_scoring.rs`) |
| Fit waveform tiers | `domain/gap_fill_fit.rs::classify_fill_waveform_confidence`, `fit_mode_waveform_floor_passes` |
| Structure gate / gate-mode waveform gate | `application/patch_region.rs::structure_passes_gate`, `seams_pass_correlation_gate` |
| Dual-fit repair (§6) | `domain/dual_fit.rs`, `application/patch_audio.rs::skip_or_dual_fit`, `try_dual_fit` |

---

## Where this fits in the pipeline

Seam scoring is **phase 4, step 4d** of the repair pipeline (after structure match, before the splice). For the whole pipeline — align → scan → fill plan → per-gap patch → write/mux — see [pipeline.md](pipeline.md).

---

## Related reading

- [gap-repair-guide.md](gap-repair-guide.md) — reading a run; tiers, seam shapes, vocabulary
- [gap-fill-modes.md](gap-fill-modes.md) — `fit` vs `gate`, flag interactions, multichannel seams, performance
- [gap-fingerprint.md](dev/gap-fingerprint.md) § Registration & dual-fit measurements — the `splice_dualfit` predictor and registration fields behind §6
- [cli-output.md](cli-output.md) — repair gap outcome report layout
- [README.md](../README.md) § Gap patching pipeline
