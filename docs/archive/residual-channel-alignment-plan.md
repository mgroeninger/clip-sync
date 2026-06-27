# Residual channel alignment — plan

> **Status:** **Shipped** (2026-06-26). P0–P1.5 complete, including Pearson/residual channel-selection
> parity tests. The residual gate ([residual-gate-wiring-plan.md](../residual-gate-wiring-plan.md)) was
> already default-on (`residual_gate = veto`); this work re-based its inputs (`worst_headroom_db` +
> `informative`) onto channel-aligned measurements — **live in production decisions**, not a
> report-only improvement. There was no separate “P2 gate” in this plan: the gate was wired earlier;
> this plan only changed what it measures.
>
> **Archived** (2026-06-26). Frozen record — outbound links are relative to `docs/archive/`.

Align residual/floor cancellation with Pearson’s **energy-selected per-channel** policy so surround and
center-dominant mixes are measured on the same signal path as the seam gate — without treating “full
multichannel” as a separate discriminator. Sections marked **as built** reflect the shipped code; where
it diverged from the original sketch (verdict stays `Copy`/scalar-only; explicit `b_ch` args; **shared
lag via summed correlation, not a mono downmix**) the reason is noted inline.

Companions: [seam-scoring.md](../seam-scoring.md) (Pearson channel selection),
[residual-gate-wiring-plan.md](../residual-gate-wiring-plan.md) (gate wiring),
[gap-fill-modes.md](../gap-fill-modes.md) § Multichannel seams.

---

## 1. Problem (one paragraph)

Pearson seam scoring uses `seam_score_channel_indices`: score only A-side channels within ~20 dB of
the loudest border energy, take **best** correlation per side, fall back to mono when every channel
is near-silent. Before this work, residual/floor measurement (`seam_chosen_and_floor` in
`patch_region.rs`) **downmixed all channels** into mono for both the A reference window and the B
haystack (`mono_window`, `interleaved_to_mono`). On center-dominant 5.1, quiet FL/FR/surround still
diluted the cancellation signal — the same class of failure Pearson fixed in 2026-06-23, but via
averaging instead of scoring the wrong channels. Residual headroom was therefore **misaligned** with
Pearson on the mixes where the residual gate matters most.

This is a **measurement-quality** fix, not a new scoring dimension. Per-channel residual on the
*same selected channels* does not add discrimination logic; it applies the existing discriminator
(headroom = chosen − floor) on the correct audio.

## 2. Non-goals

- **Joint / vector multichannel cancellation** (MIMO gain matrix) — scalar gain + integer lag stays
  per channel-pair.
- **Requiring all channels** to show low headroom — surrounds/LFE may not cancel even on same master;
  veto must not depend on them.
- **Channel layout mismatch** (A 5.1, B stereo) — v1 keeps today’s behavior (mono downmix or skip
  per-channel when B lacks the channel index); no upmix/downmix map in this plan.
- **Changing Pearson channel policy** — residual follows Pearson; Pearson is unchanged.
- **Replacing the raw reference window** with trimmed border templates — keep the raw-window
  chosen/floor model (headroom is a pure mapping difference); only the **channel extraction** inside
  that window changes.

## 3. Behavior

### 3a. Before (pre-ship)

| Step | Pearson (`fill_seam_correlations`) | Residual (`seam_chosen_and_floor`) |
|------|-------------------------------------|-------------------------------------|
| A audio | Per-channel border templates (trimmed) | Raw frames `[a_lo, a_hi)` via `mono_window` (equal average) |
| Channel pick | `seam_score_channel_indices` → best of selected | None — all channels averaged |
| B audio | Per-channel `b_ch[ch]` or `b_mono` fallback | `cache.b_mono` only |
| Aggregate | `best_channel_correlation` per side | Single scalar per side |
| Verdict headroom | N/A | `worst_headroom_db` = max(pre, post) side headroom |

Production site (old): `measure_fit_residual_verdict` built `SeamFloorParams` with `a_samples` +
`cache.b_mono` and called `seam_chosen_and_floor` on a mono downmix.

### 3b. As built (shipped)

| Step | Pearson (`fill_seam_correlations`) | Residual (`seam_chosen_and_floor_multichannel`) |
|------|-------------------------------------|-----------------------------------------------|
| A audio | Per-channel border templates (trimmed) | Raw frames per **selected** channel (no downmix) |
| Channel pick | `seam_score_channel_indices` | `selected_seam_channels` → same indices (recomputed) |
| B audio | Per-channel `b_ch[ch]` or `b_mono` fallback | `cache.b_ch[ch]` per selected channel; mono fallback when selection empty |
| Lag | N/A (Pearson is peak-normalized correlation) | `shared_alignment_lag` once across selected channels; per-channel gain at fixed lag |
| Aggregate | `best_channel_correlation` per side | Worst headroom across channels × sides; `informative` from best-floor channel |
| Verdict headroom | N/A | `worst_headroom_db` = max(pre, post) side headroom (worst-headroom channel per side) |

Production site: `measure_fit_residual_verdict` (`patch_region.rs` ~979–1070) builds `GapBorderSpec`
via `gap_border_spec(params, refined)`, calls `selected_seam_channels`, and routes non-empty
selection through `seam_chosen_and_floor_multichannel` → `SeamResidualVerdict::from_channel_residuals`.
Invoked from `finalize_fit_outcome_residual` (deferred path). Channel selection is a pure function of
`(params, refined)` — recomputed at the measurement site, not threaded through the call chain (§4b).

## 4. Design (shipped)

### 4a. Principle

> **Reuse Pearson’s channel list; run scalar residual independently per selected channel; aggregate
> conservatively for veto.**

No new thresholds. Same ~20 dB energy gate, same “best / worst” philosophy as Pearson (Pearson is
optimistic per side with **best** correlation; residual veto should be conservative with **worst**
headroom across selected channels and both sides).

### 4b. Channel selection (shared)

Selection is a **pure function of `(params, refined.start_frame, refined.end_frame)`** — it depends
only on the per-channel border templates, not on the chosen placement. Because the inputs (and
`GapBorderSpec`) are identical to Pearson's, the selection is identical **by construction** — parity
is structural, not incidental. Proven by unit and integration parity tests (§7).

Shared wrapper in `policies.rs`:

```rust
pub fn selected_seam_channels(
    a_samples: &[f32],
    channels: usize,
    spec: &GapBorderSpec,
) -> Vec<usize> {
    let (a_pre_ch, a_post_ch) = border_templates_per_channel_for_gap(a_samples, channels, spec);
    seam_score_channel_indices(&a_pre_ch, &a_post_ch)
}
```

`measure_fit_residual_verdict` shares `gap_border_spec(params, refined)` with seam scoring and calls
`selected_seam_channels`.

- **Empty selection** → mono downmix path unchanged (parity with Pearson fallback).
- **Non-empty** → per-channel residual only on listed indices, cancelled against `cache.b_ch`.

Per-channel breakdown + `selected_channels` are emitted to the **debug log** at measurement time
(`log_residual_channel_breakdown`, `RUST_LOG=debug`), not on the verdict (§4e).

### 4c. Reference window (frame range unchanged)

Keep `select_reference_window` / outward walk / standoff logic on **frame indices** `[a_lo, a_hi)`
shared across channels.

1. **Energy gate (walk stop condition):** pass if **any selected channel’s** peak in `[a_lo, a_hi)` ≥
   `absolute_silence_rms × 4`, or downmixed peak when selection is empty.
2. **Per-channel extraction:** for each selected `ch`, build `a_win_ch` from interleaved A samples.
3. **B side:** cancel against `b_ch[ch]`, not `b_mono`.
4. **Shared lag (`shared_alignment_lag`).** Integer lag found once by summing peak-normalized
   correlations across selected channels; each channel fits scalar gain at that fixed lag
   (`max_lag = 0`). Summing correlations (not downmixing waveforms) avoids loud non-matching channels
   pulling alignment off the true lag. Proven by
   `seam_chosen_and_floor_multichannel_shared_lag_follows_matching_channel`.

### 4d. Per-channel headroom and aggregation

```text
worst_headroom_db = max over (ch in selected, side in {pre, post}) of (chosen_ch − floor_ch)
informative = every measured side has min over (ch in selected) floor_ch ≤ floor_ok_db
```

The veto follows the **worst-headroom** channel; `informative` follows the **best-cancelling**
channel so a noisy surround cannot flip the same-master regime off.

**Mono fallback:** empty selection routes through `seam_chosen_and_floor` + `from_parts_with_placement`.

### 4e. `SeamResidualVerdict` schema (scalar-only; `Copy` preserved) — **as built**

`SeamResidualVerdict` stays `Copy` — per-channel breakdown is debug-log only
(`log_residual_channel_breakdown`). Builder `from_channel_residuals(pre, post, floor_ok, slide, max_lag)`
derives scalar side summaries + `informative` per the aggregation table in the original plan (§4e).

### 4f. API shape (domain) — **as built**

`b_ch` and `score_channels` are **explicit args** to `seam_chosen_and_floor_multichannel` (not added
to `SeamFloorParams`). `selected_seam_channels` is `pub` for the harness crate.

## 5. Integration points

| # | Location | Change |
|---|----------|--------|
| A | `domain/policies.rs` | **Done** — `selected_seam_channels`, `seam_chosen_and_floor_multichannel`, `shared_alignment_lag`, per-channel energy gate in `select_reference_window`, `from_channel_residuals` |
| B | `application/patch_region.rs` (`measure_fit_residual_verdict` ~979) | **Done** — `gap_border_spec`, `selected_seam_channels`, multichannel verdict path |
| C | `application/patch_region.rs` | **Done** — `gap_border_spec` helper; `FitHaystackCache.b_ch` unchanged |
| D | `application/patch_region.rs` (`log_residual_channel_breakdown`) | **Done** — debug log |
| E | `test_support/energy_signature_fixtures.rs` | **Done** — `overwrite_channels`, `channel_noise` |
| F | `tests/seam_residual_corpus.rs` + `clip-sync-repair-harness/src/seam_residual.rs` | **Done** — `score_placement_multichannel`, center-dominant corpus row, parity helper |
| G | `tests/seam_residual_oracle.rs` | **Done** — `build_center_dominant_oracle` (diagnostic tier) |
| H | `docs/seam-scoring.md` | **Done** — “Residual channel policy” § |

**Out of scope / follow-ups** (not part of this ship): legacy `fill_mode = gate` residual measurement;
CSV per-channel calibration columns; `fill_repeat_correlations` energy selection (see [BACKLOG.md](../../BACKLOG.md)).

## 6. Phasing

| Phase | Deliverable |
|-------|-------------|
| **P0 — domain + unit tests** | **Done** — multichannel cancel, aggregation, shared lag, informative decoupling |
| **P1 — pipeline** | **Done** — `measure_fit_residual_verdict`, `log_residual_channel_breakdown` |
| **P1.5 — multichannel fixtures** | **Done** — rows E/F/G |
| **P1.6 — selection parity** | **Done** — `selected_seam_channels_matches_pearson_diagnostics` (unit); `seam_residual_channel_selection_matches_pearson` (integration) |
| **P2 — gate** | **N/A** — gate shipped in [residual-gate-wiring-plan.md](../residual-gate-wiring-plan.md); this plan re-based gate inputs only |

## 7. Test plan

**Unit (`policies.rs`)** — all **done**:

- Center-dominant cancel (`..._follows_center_when_fronts_are_noise`)
- Stereo equal (`..._stereo_equal_matches_mono`)
- Empty selection mono fallback (`..._empty_selection_is_mono_fallback`)
- Aggregation / informative decoupling (`from_channel_residuals_worst_headroom_and_best_floor_informative`)
- Shared lag robustness (`..._shared_lag_follows_matching_channel`)
- **Channel-selection parity** (`selected_seam_channels_matches_pearson_diagnostics`) — stereo,
  center-dominant, near-silent; asserts `seam_channel_diagnostics(...).selected ==
  selected_seam_channels(...)`

**Integration (`seam_residual_corpus.rs`)** — all **done**:

- `seam_residual_channel_selection_matches_pearson` — stereo F1 + center-dominant 6ch via
  `pearson_and_residual_selected_channels` harness helper
- `seam_residual_center_dominant_follows_center_channel` — PR-CI veto guard on multichannel path
- `f4_decoy_placement_informative_with_high_headroom` — stereo decoy (unchanged mono path)

**Diagnostic (`seam_residual_oracle.rs`)** — `seam_residual_oracle_center_dominant_6ch` (~40 s,
on-demand real-pipeline confirmation).

### Tiering summary

| Coverage | Where | Tier / CI |
|----------|-------|-----------|
| Domain logic + shared lag + selection parity | `policies.rs` unit tests | lib unit — **PR CI** |
| Fixture helpers | `energy_signature_fixtures.rs` | lib unit — **PR CI** |
| Harness + corpus multichannel | `seam_residual_corpus.rs` | integration — **PR CI** (`pr-repair`) |
| Real `PatchAudio` pipeline | `seam_residual_oracle.rs` | diagnostic — on-demand |

## 8. Risks & open questions

| Risk | Mitigation |
|------|------------|
| A/B channel count differ | if `ch >= b_ch.len()`, skip channel; if none left, mono fallback |
| Border templates for selection vs raw window for cancel | Intentional; selection indices match Pearson |
| Higher cost (× #selected channels) | Typically 1–3 channels; behind lazy residual measurement |
| `fill_repeat_correlations` without energy selection | BACKLOG consistency cleanup, low priority |

**Resolved:** scalar summaries report worst-headroom channel; `informative` from best-floor channel
per side (§4d, §4e).

## 9. Success criteria

All **met** (2026-06-26):

- Center-dominant fixtures: per-channel residual headroom at truth strictly better than mono downmix
  (unit + corpus + oracle).
- Mono/stereo corpus rows unchanged (`score_placement` mono path preserved; Option A).
- Pearson and residual select the same channels — `selected_seam_channels_matches_pearson_diagnostics`
  (unit), `seam_residual_channel_selection_matches_pearson` (integration); production debug logs
  `fill seam channel diagnostics` and `fill residual channel breakdown` with matching
  `selected_channels` (`RUST_LOG=debug`).
- Gate inputs live on aligned measurements with default `residual_gate = veto`; Pearson scores
  unchanged.

---

## Related

- [seam-scoring.md](../seam-scoring.md) — Pearson channel selection (source of truth)
- [residual-gate-wiring-plan.md](../residual-gate-wiring-plan.md) — headroom veto/rescue wiring
- [gap-repair-guide.md](../gap-repair-guide.md) — surround seam note (2026-06-23)
- `domain/policies.rs` — `selected_seam_channels`, `seam_chosen_and_floor_multichannel`
- `tests/seam_residual_oracle.rs` — end-to-end plumbing oracle
