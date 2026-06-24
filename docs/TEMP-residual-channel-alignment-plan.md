# Residual channel alignment — plan (DRAFT)

Status: **draft / not started**. Align residual/floor cancellation with Pearson’s **energy-selected
per-channel** policy so surround and center-dominant mixes are measured on the same signal path as
the seam gate — without treating “full multichannel” as a separate discriminator.

Companions: [seam-scoring.md](seam-scoring.md) (Pearson channel selection),
[residual-gate-wiring-plan.md](residual-gate-wiring-plan.md) (gate wiring — independent but should
ship channel alignment **before** default-on veto), [gap-fill-modes.md](gap-fill-modes.md) §
Multichannel seams.

---

## 1. Problem (one paragraph)

Pearson seam scoring uses `seam_score_channel_indices`: score only A-side channels within ~20 dB of
the loudest border energy, take **best** correlation per side, fall back to mono when every channel
is near-silent. Residual/floor measurement (`seam_chosen_and_floor` in `patch_region.rs`) **downmixes
all channels** into mono for both the A reference window and the B haystack (`mono_window`,
`interleaved_to_mono`). On center-dominant 5.1, quiet FL/FR/surround still dilute the cancellation
signal — the same class of failure Pearson fixed in 2026-06-23, but via averaging instead of
scoring the wrong channels. Residual headroom is therefore **misaligned** with Pearson on the mixes
where the residual gate matters most.

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

## 3. Current behavior (exact)

| Step | Pearson (`fill_seam_correlations`) | Residual (`seam_chosen_and_floor`) |
|------|-------------------------------------|-------------------------------------|
| A audio | Per-channel border templates (trimmed) | Raw frames `[a_lo, a_hi)` via `mono_window` (equal average) |
| Channel pick | `seam_score_channel_indices` → best of selected | None — all channels averaged |
| B audio | Per-channel `b_ch[ch]` or `b_mono` fallback | `cache.b_mono` only |
| Aggregate | `best_channel_correlation` per side | Single scalar per side |
| Verdict headroom | N/A | `worst_headroom_db` = max(pre, post) side headroom |

Production site: `evaluate_seam_gate_fit_candidate` (~523–556) builds `SeamFloorParams` with
`a_samples` + `b_mono`. Comment already notes “channel-following is the next refinement.”

## 4. Proposed design

### 4a. Principle

> **Reuse Pearson’s channel list; run scalar residual independently per selected channel; aggregate
> conservatively for veto.**

No new thresholds. Same ~20 dB energy gate, same “best / worst” philosophy as Pearson (Pearson is
optimistic per side with **best** correlation; residual veto should be conservative with **worst**
headroom across selected channels and both sides).

### 4b. Channel selection (shared)

Call `seam_score_channel_indices(a_pre_ch, a_post_ch)` using the **same** per-channel border
templates already built in `evaluate_seam_gate_fit_candidate` (no second energy pass on different
audio).

- **Empty selection** → today’s mono downmix path unchanged (parity with Pearson fallback).
- **Non-empty** → per-channel residual only on listed indices.

Expose selection on the verdict for debug/JSON (see §4e).

### 4c. Reference window (frame range unchanged)

Keep `select_reference_window` / outward walk / standoff logic on **frame indices** `[a_lo, a_hi)`
shared across channels — the chosen/floor model requires the same raw window at two B mappings.

Change **only** how we test energy and extract samples inside that range:

1. **Energy gate (walk stop condition):** when evaluating candidate windows during the outward
   walk, pass if **any selected channel’s** peak in `[a_lo, a_hi)` ≥ `absolute_silence_rms × 4`, *or*
   if selection is empty, keep downmixed peak (today’s behavior). Rationale: don’t walk past usable
   center audio because downmix peak is dominated by silent surrounds.

2. **Per-channel extraction:** for each selected `ch`, build `a_win_ch` from interleaved A samples
   (same helper shape as `interleaved_channel_timeline_f64` / existing border per-channel builders —
   do not re-downmix).

3. **B side:** cancel against `b_ch[ch]`, not `b_mono`.

4. **Lag + gain:** existing `seam_residual_for_side` unchanged (scalar LSQ + integer lag search).

### 4d. Per-channel headroom and aggregation

For each selected channel `ch` and side `pre`/`post`:

```text
headroom_ch_side = chosen_ch_side_db − floor_ch_side_db
```

**Verdict aggregation (veto-safe):**

```text
worst_headroom_db = max over (ch in selected, side in {pre, post}) of headroom_ch_side
```

Also retain **side summaries** for reporting as today (worst channel per side, or min chosen / max
floor — pick one schema in §4e). Do **not** average headroom in dB across channels.

**Mono fallback path:** one channel-pair (downmixed A window vs `b_mono`); schema identical to today.

### 4e. `SeamResidualVerdict` schema (extend)

Add fields (names tentative; `skip_serializing_if` where appropriate):

| Field | Meaning |
|-------|---------|
| `selected_channels: Vec<usize>` | Same indices as Pearson would score |
| `channel_headroom_db: Vec<(usize, f64, f64)>` | Optional debug: `(ch, headroom_pre, headroom_post)` |
| `chosen_pre_db`, … | **Side summaries** = worst headroom channel’s chosen/floor on that side (or NaN if unmeasured) |

Existing `worst_headroom_db()` becomes the max over all channel×side headrooms (or unchanged if
side fields already store the worst-channel values).

Gate composition in [residual-gate-wiring-plan.md](residual-gate-wiring-plan.md) consumes
`worst_headroom_db()` + `informative` — no change to gate rules once channel alignment lands.

### 4f. API shape (domain)

Prefer extending `SeamFloorParams` rather than parallel structs:

```rust
pub struct SeamFloorParams<'a> {
    // existing …
    /// When `Some`, per-channel measurement on these indices; when empty/`None`, mono downmix.
    pub score_channels: Option<&'a [usize]>,
    pub b_ch: Option<&'a [Vec<f64>]>,  // required when score_channels is non-empty
}
```

New entry point (or extend `seam_chosen_and_floor`):

```rust
pub fn seam_chosen_and_floor_multichannel(
    params: &SeamFloorParams<'_>,
    side: SeamSide,
    gap_start_frame: usize,
    gap_end_frame: usize,
    chosen_delta: i64,
) -> (SeamFloorProbe, SeamFloorProbe, Vec<SeamChannelResidual>); // or fold into verdict builder
```

Keep `seam_chosen_and_floor` as mono wrapper for tests/corpus until migrated.

**Delete or redirect** dead prototype `seam_residual_diagnostics` (±64 lag, trimmed templates) when
this ships — production already uses `seam_chosen_and_floor` only.

## 5. Integration points

| # | Location | Change |
|---|----------|--------|
| A | `domain/policies.rs` | `select_reference_window`: energy gate on selected channels; per-channel extract + cancel; verdict aggregation; extend `SeamFloorParams` |
| B | `application/patch_region.rs` | Pass `a_pre_ch`, `b_ch`, `score_channels` into floor params; build verdict from multichannel path |
| C | `application/patch_region.rs` (`FitHaystackCache`) | Already has `b_ch` — no new cache |
| D | `domain/patch_result.rs` / JSON | Optional `selected_channels` on residual block |
| E | `tests/seam_residual_corpus.rs` | Add 5.1 center-dominant row; keep mono fixtures green |
| F | `docs/seam-scoring.md` | Short § “Residual channel policy” pointing here |

No change to: Pearson functions, structure match, fill search, gate mode legacy path (residual still
fit-only until legacy path gets measurement — optional follow-up).

## 6. Phasing

| Phase | Deliverable |
|-------|-------------|
| **P0 — domain + unit tests** | Per-channel cancel on fixed windows; aggregation; 5.1 center test mirroring `fill_seam_correlations_follows_center_channel_when_front_is_silent` |
| **P1 — pipeline** | Wire in `evaluate_seam_gate_fit_candidate`; debug log `selected_channels` + per-channel headroom; oracle + corpus rows |
| **P2 — gate** | Proceed with [residual-gate-wiring-plan.md](residual-gate-wiring-plan.md) veto on aligned measurements |

Channel alignment is **not blocked** on lag-radius unification or `informative` — but those should
land in gate PR; this plan can merge independently as report-only measurement improvement.

## 7. Test plan

**Unit (`policies.rs`)**

- Center-dominant 5.1: FL/FR noise on B, signal on FC — per-channel headroom ≈ 0 at truth,
  mono-downmix path shows **worse** cancellation (documents the fix).
- Stereo equal energy: both channels selected; result matches mono fallback within ε.
- Empty selection → mono fallback identical to current `seam_chosen_and_floor` tests.
- Aggregation: one bad channel (high headroom) drives `worst_headroom_db` even if another cancels.

**Corpus (`tests/seam_residual_corpus.rs`)**

- New synthetic fixture or extend broadband builder with 6ch center-dominant layout.
- CSV column `selected_channels` for calibration runs.

**Integration**

- Extend `seam_residual_oracle_csv` (optional 6ch variant) — headroom stays < 6 dB at true fill.

All new tests run in CI (not `#[ignore]`); diagnostics stay ignored.

## 8. Risks & open questions

| Risk | Mitigation |
|------|------------|
| A/B channel count differ | v1: if `ch >= b_ch.len()`, skip that channel; if none left, mono fallback |
| Energy gate uses border templates for selection but raw window for cancel | Intentional — same as today’s template vs raw split; selection indices still match Pearson |
| Slightly higher cost (× #selected channels × lags) | Typically 1–3 channels; same order as Pearson; still behind `measure_residual` / debug |
| `fill_repeat_correlations` still scores all channels | Out of scope here; note in BACKLOG as separate Pearson/residual repeat alignment |

**Open:** Should side summary fields (`chosen_pre_db`, …) report the **worst headroom channel** or
the **loudest channel**? Recommendation: **worst headroom channel** — matches veto aggregation and
debug interpretation.

## 9. Success criteria

- On center-dominant 5.1 fixtures, residual headroom at truth ≤ mono-downmix headroom (strictly
  better when FL/FR carry noise).
- Mono/stereo corpus rows unchanged within floating tolerance.
- `selected_channels` in debug/JSON matches Pearson `seam channel diagnostics` for the same gap.
- No change to Pearson scores or patch decisions until residual gate is enabled separately.

---

## Related

- [seam-scoring.md](seam-scoring.md) — Pearson channel selection (source of truth)
- [residual-gate-wiring-plan.md](residual-gate-wiring-plan.md) — headroom veto/rescue wiring
- [gap-repair-guide.md](gap-repair-guide.md) — surround seam note (2026-06-23)
- `domain/policies.rs` — `seam_score_channel_indices`, `seam_chosen_and_floor`
- `tests/seam_residual_oracle.rs` — end-to-end plumbing oracle
