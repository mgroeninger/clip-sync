# TEMP — Envelope-recenter `lag_decision`

**Status:** draft, 2026-08-13. Not started.
Parent: [archive/TEMP-fill-placement-registration-plan.md](archive/TEMP-fill-placement-registration-plan.md)
(§1.2 saturation, §7 retired widened-sweep, S1 blocked on re-scoping the measurement).
Findings; do not treat §0 as a work list.

`lag_decision` is a 1 s waveform sweep at **±600 ms around mapped nominal**. That window is not a
speed limit (`lag_correlation_curve_auto` is two FFTs either way). It assumes the pair map is
already roughly right. On pair 31 the map is a **step** (~0 at 28:45, −1.11 s from 47:32 onward),
so the true peak sits outside the fine window and the probe reports an interior spurious max.
Production still places those later gaps because the fill search is ±10 s off the same map. The
measurement cannot confirm them.

This plan recenters the **measurement**, not the fill search.

---

## Goal

Keep the ±600 ms / 1 s waveform sweep. Change only its **center**, using a coarse envelope peak
that does not consult the fill search, the unified objective, or any other gap.

`frac_lag_ms` stays **gross-relative to original `b_mapped`**, so it remains commensurable with
`align_adjustment_secs`. The fine sweep’s own peak is a residual around the envelope center; the
reported lag is `envelope_center + fine_residual`.

**Non-goals**

- Do not widen `lag_max_lag_ms` as the primary fix, and do not run a corpus fingerprint dump to
  do it.
- Do not center on `align_adjustment_secs` / applied `start_frame` (that is lag-at-applied:
  useful, circular, already the shape of `lag_editorial`).
- Do not feed gap *N*’s slide into gap *N*+1. That is ungated `anchored_retry`; 31/g7’s wrong
  −977 ms would become the prior for the gaps that actually want −1.11 s (§1.1).
- Do not change `fill_offset_mode`, the fill search radius, dual-fit’s `SEAM_LOCAL_SEARCH_MS`,
  or Apply’s `max_lag_blocks = 10` (equivalence gate). The recenter probe may use a **wider**
  envelope window than Apply; that must be a separate parameter.
- Do not put the result in a gate. Record-only, same as S1.

---

## Why envelope, and why a guard

Scan-time `register_donor_window` (`domain/gap_equivalence.rs`) already cross-correlates 100 ms
dB envelopes on the **shoulders**, decode-free, independent of placement. Default reach is
±`max_lag_blocks` (10 ⇒ **±1.0 s**). Pair 31’s later gaps need ~1.15 s, so today’s envelope
saturates at the edge. Fingerprint `wide_envelope` is the wrong reuse: it is also centered on
`b_mapped` and only ±400 ms (`WIDE_ENV_MAX_LAG_MS`).

The failure gap is the load-bearing caveat. At 31/g7, true registration is **−40 ms**, but
`donor_registration.lag_ms` reads **−1000 ms** — the coarse window’s own edge, not the truth.
Blind recentering would move the fine sweep to −1.0 s and **lose** today’s correct
`lag_decision.pre` (−47.5 ms, `peak_r` 0.829). So recenter only when the coarse peak is
**interior**. An edge-pinned / low-`r` envelope falls back to today’s nominal center (fail
open). That keeps 31/g7 on nominal and still lets a *widened* coarse window unlock 31’s later
gaps, whose true peak is interior only once the envelope can see past 1.0 s.

---

## Change

In `lag_at_placement` (fingerprint path now; production path when S1 is re-scoped):

1. **Coarse.** Shoulder envelope correlation at `b_mapped`, ±`lag_envelope_max_lag_ms`
   (new, default **2000** — 20×100 ms bins, or the existing `register_donor_window` with a
   probe-only `max_lag_blocks`). Prefer the scan envelopes already in hand over a second PCM
   envelope. Record `envelope_lag_ms`, `envelope_peak_r`, `envelope_edge_pinned`.
2. **Guard.** Recenter iff `envelope_peak_r ≥ min_envelope_r` (reuse 0.70) **and** the peak is
   ≥1 bin inside the coarse window. Otherwise `start_frame` stays `b_mapped`.
3. **Fine.** Existing `lag_pair` ±`lag_max_lag_ms` (600) / 1 s template, centered on
   `b_mapped + round(envelope_lag)`. Report `frac_lag_ms` gross to original `b_mapped`.
   `edge_pinned` remains a property of the **fine** window around the new center.

Contract `_contract.placement` must say so: “fine sweep at `b_mapped + envelope_lag` when the
coarse peak is interior; else `b_mapped`.” Old corpora stay valid (`envelope_*` absent ⇒
nominal center).

S1, when unblocked, plumbs **this** measurement, not the nominal-only sweep. Cost is still two
FFTs per gap plus a decode-free envelope that scan already computed.

---

## Exit

Same binary, `lag_envelope_max_lag_ms` on vs off. Not a corpus fingerprint pass.

**Pair 31** (the original case):

| gap | expect |
|-----|--------|
| 31/g7 (failure, true ≈ −40 ms) | coarse edge-pinned or low-`r` → **no recenter**; `frac_lag_ms` stays ≈ −50 ms |
| 31/g8, g10, g11 (true ≈ −1.11 s) | coarse interior ≈ −1.11 s; fine `frac_lag_ms` agrees within ~50 ms; `peak_r` usable; fine `edge_pinned` false |

**Pair 50** (same class, larger). Consensus **+2.7891 s**, last six gaps unmeasurable under ±600 ms
(`. . . . X X X X X X` against pair 31's `. . X X X`). At +2.79 s it is outside the proposed
2000 ms default `lag_envelope_max_lag_ms`, so this is also the test of whether that default is
the right size — if the coarse window has to reach 3 s to see pair 50, set it deliberately.

**Smoke join** on the oriented load-bearing cell `{2, 3, 13, 14, 18, 22, 32, 47, 51}` (9 pairs,
all agreeing within 50 ms; rebuilt 2026-08-14, archived registration plan §0.0 Step 3). Median
`frac_lag_ms` must not move beyond re-quantization of a shifted center (a few ms): none of these
pairs' coarse peaks should be interior-and-far enough to trigger a recenter. Do not spend the
~22 ms signed bias as a tolerance — that bias is two terms (probe `D`, recorded per shoulder as
`border_displacement_ms`; production α ≈ 25 ms late) and a recenter moves neither. See archived
[§ Step 5](archive/TEMP-fill-placement-registration-plan.md#step-5-result-2026-08-14--the-bias-is-two-terms-and-only-one-is-the-probe).

26/g6 must stay near-nominal (≈ −50 ms), not follow the −1.45 s slide.

If pair 31's later gaps still miss with a 2 s envelope, stop: that is a different question
(envelope locked onto the same wrong peak as the fill), not a window problem, and this plan
does not proceed to production plumbing (S1).
