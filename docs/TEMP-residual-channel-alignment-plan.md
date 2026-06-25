# Residual channel alignment — plan (DRAFT)

Status: **P0 + P1 implemented** (domain per-channel measurement + pipeline wiring + unit tests).
Remaining: P2 gate veto (separate PR, see [residual-gate-wiring-plan.md](residual-gate-wiring-plan.md)),
corpus/oracle 6ch rows (§7), and doc cross-links (§5 rows E/F). Align residual/floor cancellation with
Pearson’s **energy-selected per-channel** policy so surround and center-dominant mixes are measured on
the same signal path as the seam gate — without treating “full multichannel” as a separate
discriminator. Sections marked **as built** reflect the shipped code; where it diverged from the
original sketch (verdict stays `Copy`/scalar-only; explicit `b_ch` args) the reason is noted inline.

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

Production site: `measure_fit_residual_verdict` (`patch_region.rs` ~760–820) builds `SeamFloorParams`
with `a_samples` + `cache.b_mono` (via the `floor_common` closure ~774) and calls
`seam_chosen_and_floor`. It is invoked from `finalize_fit_outcome_residual` (deferred path) and inline
in `evaluate_seam_gate_fit_candidate` (non-deferred path). Note: the per-channel border templates
(`a_pre_ch`/`a_post_ch`) are **locals of `evaluate_seam_gate_fit_candidate`** and are *not* in scope at
the `measure_fit_residual_verdict` call site — but the channel selection is a pure function of
`(params, refined)` and is recomputed there cheaply (see §4b).

## 4. Proposed design

### 4a. Principle

> **Reuse Pearson’s channel list; run scalar residual independently per selected channel; aggregate
> conservatively for veto.**

No new thresholds. Same ~20 dB energy gate, same “best / worst” philosophy as Pearson (Pearson is
optimistic per side with **best** correlation; residual veto should be conservative with **worst**
headroom across selected channels and both sides).

### 4b. Channel selection (shared)

Selection is a **pure function of `(params, refined.start_frame, refined.end_frame)`** — it depends
only on the per-channel border templates, not on the chosen placement. So we don't thread templates
through the call chain; we recompute the selection at the measurement site, which already has
`params` + `refined`. Because the inputs (and `GapBorderSpec`) are identical to Pearson's, the
selection is identical **by construction** — parity is structural, not incidental, and the recompute
is a cheap border-window pass (border_frames × channels mean-square), nowhere near the structure/fill
search cost.

Add a shared wrapper in `policies.rs` that both Pearson and residual call (this also resolves the
visibility of the currently-private `seam_score_channel_indices`):

```rust
pub(crate) fn selected_seam_channels(
    a_samples: &[f32],
    channels: usize,
    spec: &GapBorderSpec,
) -> Vec<usize> {
    let (a_pre_ch, a_post_ch) = border_templates_per_channel_for_gap(a_samples, channels, spec);
    seam_score_channel_indices(&a_pre_ch, &a_post_ch)
}
```

`measure_fit_residual_verdict` builds the same `GapBorderSpec` it shares with
`evaluate_seam_gate_fit_candidate` (extract a `gap_border_spec(params, refined)` helper to DRY the
two existing inline constructions) and calls `selected_seam_channels`.

- **Empty selection** → today’s mono downmix path unchanged (parity with Pearson fallback).
- **Non-empty** → per-channel residual only on listed indices, cancelled against `cache.b_ch`.

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

**Verdict aggregation — two quantities, deliberately different channels:**

```text
worst_headroom_db = max over (ch in selected, side in {pre, post}) of headroom_ch_side
```

The veto follows the **worst-headroom** channel (conservative). But `informative` ("did this gap
establish a same-master cancellation regime?") must follow the **best-cancelling** channel:

```text
informative = every measured side has  min over (ch in selected) floor_ch_side  ≤ floor_ok_db
```

These are **different channels** on the mixes this plan targets: on center-dominant 5.1 the veto
follows a noisy FL (worst headroom) while `informative` follows FC (lowest floor). Using the
worst-headroom channel's floor for `informative` would let a noisy surround flip the regime off and
make the gate wrongly abstain — exactly the failure Non-goal §2 forbids ("veto must not depend on"
surrounds/LFE). So a single per-side scalar cannot serve both *purposes from the same channel*: the
scalar fields follow the worst-headroom channel (veto/report) while `informative` is reduced
separately from the best-floor channel. The worst-headroom-per-side scalar choice keeps
`worst_headroom_db()` a pure max of the two side scalars **and** equal to the global max over
channels × sides (see §4e). Do **not** average headroom in dB across channels.

**Mono fallback path:** one channel-pair (downmixed A window vs `b_mono`); empty selection routes
through the existing `seam_chosen_and_floor` path, so every accessor equals today's values.

### 4e. `SeamResidualVerdict` schema (scalar-only; `Copy` preserved) — **as built**

**Implementation constraint discovered:** `SeamResidualVerdict` must stay `Copy` — `GapTagsPatchContext`
(`#[derive(Copy)]`) holds an `Option<SeamResidualVerdict>`, so a `Vec` field on the verdict is
impossible without unwinding `Copy` through a chain of structs. The per-channel breakdown therefore
does **not** live on the verdict. Instead:

- The verdict keeps its **existing four scalar fields + sources + `informative` + placement/lag**
  unchanged — no new fields, `Copy` intact, all 13 consumers untouched (zero blast radius, not even
  the `from_parts_*` builders, which stay as-is for the mono/legacy callers).
- A new builder `from_channel_residuals(pre, post, floor_ok, slide, max_lag)` takes the per-side
  `Vec<SeamChannelResidual>` (a measurement-time value, not stored) and derives the scalars below.
- The per-channel breakdown + `selected_channels` are emitted to the **debug log** at measurement
  time (`log_residual_channel_breakdown`, `RUST_LOG=debug`), not serialized on the verdict.

```rust
pub struct SeamChannelResidual {     // measurement-time only (not on the verdict)
    pub channel: usize,
    pub chosen: SeamFloorProbe,
    pub floor:  SeamFloorProbe,
}
```

Pinned aggregation semantics (resolves the §8 open question):

| Quantity | Aggregation | Why |
|----------|-------------|-----|
| scalar `chosen_pre_db`/`floor_pre_db` (+post), `floor_source_*` | the **worst-headroom channel's** chosen/floor/source on that side (NaN/`None` if no channel measured) | report prints the channel that *drove* the veto; **and** makes the next row work for free |
| `worst_headroom_db()` | **unchanged formula** — `max` of the two side `chosen−floor` scalars | because each side scalar already holds its worst-headroom channel, this equals `max` over (ch × side); no internals change |
| `informative` | per measured side, **best-floor (min) selected channel** ≤ `floor_ok_db` | regime witness; surround noise must not flip it off (§4d, Non-goal §2) |
| `worst_chosen_db()` / `worst_floor_db()` | unchanged — read the scalar summaries | JSON summary; no call-site change |

The worst-headroom-per-side scalar choice is what lets `worst_headroom_db()` stay a pure function of
the four scalars *and* equal the global max — so §4d's "no longer a pure function" caveat does **not**
apply to the shipped scalar-only design; only `informative` needs the separate best-floor reduction
(done inside `from_channel_residuals`).

**Mono fallback** (empty selection): `seam_chosen_and_floor_multichannel` returns one downmix entry,
and production routes empty selection straight through the existing `seam_chosen_and_floor` +
`from_parts_with_placement` path — today's values bit-for-bit (parity for §7 tests).

Gate composition in [residual-gate-wiring-plan.md](residual-gate-wiring-plan.md) consumes
`worst_headroom_db()` + `informative` — no change to gate rules once channel alignment lands.

### 4f. API shape (domain) — **as built**

`b_ch` and `score_channels` are passed as **explicit args** to the new entry point rather than added
to `SeamFloorParams` — this avoids touching every `SeamFloorParams` literal (7 of them, mostly tests)
and keeps the mono `seam_chosen_and_floor` signature untouched. `score_channels` must already be
filtered to channels of interest; indices `≥ b_ch.len()` are skipped (A/B channel-count mismatch).

```rust
pub fn seam_chosen_and_floor_multichannel(
    params: &SeamFloorParams<'_>,
    b_ch: &[Vec<f64>],
    score_channels: &[usize],
    side: SeamSide,
    gap_start_frame: usize,
    gap_end_frame: usize,
    chosen_delta: i64,
) -> Vec<SeamChannelResidual>;  // per-side, per selected channel (empty selection → len 1 mono)
```

New builder `SeamResidualVerdict::from_channel_residuals(pre, post, floor_ok, slide, max_lag)` takes
the pre+post `Vec<SeamChannelResidual>` and derives the scalar side summaries + `informative` per the
§4e table. `from_parts*` builders are kept unchanged for the mono/legacy/test callers.

Internals refactored to share one cancel path: `walk_reference_frames` (frame-range walk with a
pluggable energy predicate), `measure_a_win_at_delta`, and `chosen_and_floor_on_window` are now used
by both the mono `seam_chosen_and_floor` and the per-channel function.

(The dead `seam_residual_diagnostics` prototype this section once flagged is already gone from the
tree — no cleanup needed.)

## 5. Integration points

| # | Location | Change |
|---|----------|--------|
| A | `domain/policies.rs` | `select_reference_window`: energy gate on selected channels; per-channel extract + cancel; verdict aggregation; extend `SeamFloorParams`; add `pub(crate) selected_seam_channels` wrapper (§4b); make `seam_score_channel_indices` reachable via it |
| B | `application/patch_region.rs` (`measure_fit_residual_verdict` ~760) | Build `GapBorderSpec` from `(params, refined)`, call `selected_seam_channels`, pass `score_channels` + `cache.b_ch` into `SeamFloorParams`; build verdict from multichannel path. **No signature changes** to `finalize_fit_outcome_residual` or the candidate pool — selection is recomputed here, not threaded |
| C | `application/patch_region.rs` (`FitHaystackCache`) | Already has `b_ch` — no new cache; extract `gap_border_spec(params, refined)` helper to DRY the two inline `GapBorderSpec` builds |
| D | `application/patch_region.rs` (`log_residual_channel_breakdown`) | **Done** — `selected_channels` + per-channel headroom to the `RUST_LOG=debug` log (not JSON; verdict stays `Copy`, §4e) |
| E | `tests/seam_residual_corpus.rs` | *TODO* — add 5.1 center-dominant row; keep mono fixtures green |
| F | `docs/seam-scoring.md` | *TODO* — short § “Residual channel policy” pointing here |

No change to: Pearson functions, structure match, fill search, gate mode legacy path (residual still
fit-only until legacy path gets measurement — optional follow-up).

## 6. Phasing

| Phase | Deliverable |
|-------|-------------|
| **P0 — domain + unit tests** | **Done** — per-channel cancel on fixed windows; aggregation; center-dominant test (`seam_chosen_and_floor_multichannel_follows_center_when_fronts_are_noise`), stereo-equal, empty-selection, and aggregation/informative-decoupling tests in `policies.rs` |
| **P1 — pipeline** | **Done** — wired in `measure_fit_residual_verdict` (recompute selection via `selected_seam_channels`, §4b); `log_residual_channel_breakdown` debug log. *TODO:* oracle + corpus 6ch rows |
| **P2 — gate** | *TODO* — proceed with [residual-gate-wiring-plan.md](residual-gate-wiring-plan.md) veto on aligned measurements |

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

**Resolved (was open):** scalar side summary fields (`chosen_pre_db`, …) report the **worst-headroom
channel** so the `ResidualHeadroomExceeded` message names the channel that drove the veto. But
`informative` is computed separately from the **best-floor channel** per side — *not* the worst —
because a noisy surround must not flip the regime off (§4d, §4e). The two summaries deliberately
follow different channels; this is the crux of the §4e schema (matrix + derived scalars), not a
single-channel choice.

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
