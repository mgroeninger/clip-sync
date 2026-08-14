# TEMP — Envelope-recenter `lag_decision`

**Status:** draft, 2026-08-13. Not started.
Parent: [TEMP-fill-placement-registration-plan.md](TEMP-fill-placement-registration-plan.md)
(§1.2 saturation, §7 retired widened-sweep, S1 blocked on re-scoping the measurement).

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

One-pair fingerprint on **pair 31** (not a corpus pass), same binary, `lag_envelope_max_lag_ms`
on vs off:

| gap | expect |
|-----|--------|
| 31/g7 (failure, true ≈ −40 ms) | coarse edge-pinned or low-`r` → **no recenter**; `frac_lag_ms` stays ≈ −50 ms |
| 31/g8, g10, g11 (true ≈ −1.11 s) | coarse interior ≈ −1.11 s; fine `frac_lag_ms` agrees within ~50 ms; `peak_r` usable; fine `edge_pinned` false |

Then a smoke join against the fit-terms baseline on the six pairs whose consensus already
agrees inside ±600 ms (3, 6, 13, 18, 22, 32): median lag must not move by more than the
existing ~22 ms signed bias. 26/g6 must stay near-nominal (≈ −50 ms), not follow the −1.45 s
slide.

> **⚠ This exit criterion is contaminated — re-derive it before running (2026-08-14).** Both of its
> inputs come from the mixed-orientation corpus (registration plan
> [§ 0.0](TEMP-fill-placement-registration-plan.md#00-data-re-baseline-2026-08-14--read-before-any-number-below)):
>
> - The six-pair cell is **not six pairs**. Pairs 3 and 6 were scanned with A/B reversed, so their
>   consensus is of a pairing we do not ship; and pair 6 is **duplicate media of pair 22**, already in
>   the cell. What is left is `{13, 18, 22, 32}` — four distinct, correctly-oriented media.
> - The **~22 ms signed bias** is the withdrawn −23.4 / −24.8 ms figure. It is not an orientation
>   artifact on the same-A pairs (those anchors are bit-identical old to new), but its value when the
>   16 replacement pairs enter is unmeasured.
>
> **Resolved for the cell, 2026-08-14 (step 3 is done).** The rebuilt consensus set is **9 pairs,
> `{2, 3, 13, 14, 18, 22, 32, 47, 51}`, all agreeing within 50 ms** — use that as the smoke-join set.
> The bias is now **−21.9 ms** (`peak_r`) / **−26.1 ms** (`peak_z`) on the clean corpus.
>
> **Explained 2026-08-14 (step 5) — and this exit check must change.** The bias is not registration
> disagreement, and it is **two terms**. One is `lag_at_placement`'s own border displacement (silence
> walk-off + 12 %-of-peak trim pull the A template edge away from the gap; the B anchor is not pulled),
> so `pre = true − D_pre` and `post = true + D_post` — the post sweep's pre-shift is added back out, so
> post does *not* inherit `D_pre`. That probe term is `D_pre + D_post`, median **11.3 ms**, and it is
> now recorded per shoulder as `lag_decision.…border_displacement_ms`. The other, larger term is the
> applied placement itself sitting ~**25 ms** late (`−(pre + post)/2`), which is production's geometry,
> not the probe's. A recenter moves neither — it only moves the sweep's center — so "median lag must not
> move by more than the existing ~22 ms bias" is checking a quantity this change does not touch.
> Replace it with: on the six-pair smoke set the median lag must not move **at all** beyond the
> re-quantization of a shifted center (a few ms), because none of those pairs' coarse peaks should be
> interior-and-far enough to trigger a recenter in the first place. The bias itself is not a budget to
> spend: its probe half is a per-gap recorded correction (not a constant — do not subtract the median),
> and its placement half is a separate open question.
> Nothing above the Exit section depends on the corpus — the pair-31 mechanism, the guard, and the
> 31/g7 fail-open case are single-pair and correctly oriented.

> **This plan now has a second validation pair (2026-08-14).** **Pair 50** shows the same signature as
> pair 31 and more extremely: consensus **+2.7891 s**, with its last six gaps in order returning no
> usable shoulder at all (`. . . . X X X X X X`) against pair 31's `. . X X X`. So "true offset outside
> the ±600 ms sweep" is a **class, not a pair**, which is the objection this plan was most exposed to.
> Add pair 50 to the exit run: at +2.79 s it is outside even the proposed 2000 ms default
> `lag_envelope_max_lag_ms`, so it is also the test of whether that default is the right size — if the
> coarse window has to reach 3 s to see pair 50, say so and set it deliberately rather than discovering
> it later.

If pair 31’s later gaps still miss with a 2 s envelope, stop: that is a different question
(envelope locked onto the same wrong peak as the fill), not a window problem, and this plan
does not proceed to production plumbing.
