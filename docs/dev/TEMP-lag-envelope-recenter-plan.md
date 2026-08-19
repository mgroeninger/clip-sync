# TEMP — Envelope-recenter `lag_decision`

**Status:** draft, 2026-08-19. Not started. Inherits map-coordinate `lag_pair` from the 14-gap
census re-run (clean 2026-08-19); map-coordinates smoke-join still open (6/9).
Parent: [archive/TEMP-fill-placement-registration-plan.md](archive/TEMP-fill-placement-registration-plan.md)
(§1.2 saturation, §7 retired widened-sweep, S1 blocked on re-scoping the measurement).
Findings; do not treat §0 as a work list.

`lag_decision` is a 1 s waveform sweep at **±600 ms around mapped nominal**. That window is not a
speed limit (`lag_correlation_curve_auto` is two FFTs either way). It assumes the pair map is
already roughly right. On pair 31 the map is a **step** (~0 at 28:45, −1.11 s from 47:32 onward),
so the true peak sits outside the fine window and the probe reports an interior spurious max.

> **Measured 2026-08-18 (tightens the step location):** 31/g7 — the 28:45 gap itself — has its
> post shoulder uniquely at **−1160 ms** (±2000 ms offline sweep; pre −47 ms), so the step onset
> is at that gap's exit, not somewhere in (28:45, 47:32); "from 47:32 onward" is where it next
> became measurable. Same measurement: the donor bridge across that gap is **zero-length**, so
> for 31/g7 recentering enables *detection* (and a skip verdict on `bridge ≈ 0`), not a fill —
> see [archive/TEMP-31g7-gap-shape-vocabulary.md](archive/TEMP-31g7-gap-shape-vocabulary.md) §9. This plan's
> record-only scope is unchanged. S2b is refuted.
>
> **Measured 2026-08-19 (map-coordinates census exit):**
> [TEMP-lag-decision-map-coordinates-plan.md](TEMP-lag-decision-map-coordinates-plan.md) landed
> haystack-shift + landmark-gated chain. The 12 `shared_near_false_dead` rows are rescued (not
> this plan's). 31/g7 pre is a unique map-true lock at **−38.1 ms** (moved from −47.5 by exactly
> `D_pre` = 9.4 ms); post still `decorrelated` at nominal ±600 (true −1160). 31/g10 is the current
> world this plan walks into: both shoulders dead at nominal; the old post lock at −1152 ms (`r`
> 0.995) was a weak-pre chain and is gone (`decorrelated` at −444 ms, `r` 0.19). Do not merge the
> plans.
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
- Do not treat lag-verdict one-dead as this plan's population. The 12 in-reach false-deads are
  rescued; this plan owns **shared far offsets** (31/g10, pair 31's later gaps, pair 50) plus
  31/g7's **post shoulder** specifically (true −1160 ms, outside ±600 — detection only, not a
  fill; see the skip-vs-one-seam non-goal below). 31/g7's pre shoulder is already a map-true
  landmark and is not this plan's business.
- Do not decide 31/g7 skip-vs-one-seam. Unlocking post at −1160 is detection, not a fill.
  Collapsed-bridge is a separate predicate (`|gap + post_lag − pre_lag| ≈ 0`).

---

## Why envelope, and why a guard

Scan-time `register_donor_window` (`domain/gap_equivalence.rs`) already cross-correlates 100 ms
dB envelopes on the **shoulders**, decode-free, independent of placement. Default reach is
±`max_lag_blocks` (10 ⇒ **±1.0 s**). Pair 31’s later gaps need ~1.15 s, so today’s envelope
saturates at the edge. Fingerprint `wide_envelope` is the wrong reuse: it is also centered on
`b_mapped` and only ±400 ms (`WIDE_ENV_MAX_LAG_MS`).

**Edge-pin is the fail-open for a saturated envelope, not a 31/g7-specific guard.** At 31/g7,
scan `donor_registration.lag_ms` reads **−1000 ms** — the ±1.0 s window’s own edge, not the
truth. Blind reuse of that *single* number would move the fine sweep to −1.0 s and lose the
unique pre lock (now **−38.1 ms**, `peak_r` 0.829-class). An edge-pinned / low-`r` envelope still
falls back to `b_mapped`.

Once this plan widens the coarse window to 2 s, 31/g7’s post peak (−1160) is interior (~840 ms
from the edge). Edge-pin will not fire on that shoulder. Map-coordinates made the disagreement
dump-visible; that is the remaining protection:

| gap | after map-coordinates (current) | what recenter does |
|-----|---------------------------------|--------------------|
| 31/g7 | pre landmark **−38.1 ms**; post dead at nominal (true −1160); `bridge_ms` ≈ 0 | **Per-shoulder.** Pre envelope ~−40: recenter is a no-op for ±600. Post envelope ~−1160: unlocks record-only detection. Sharing one gap-level lag would throw away the unique pre lock. |
| 31/g10 | both dead at nominal; both true ~−1153. Pre looking non-landmark is window starvation, not a weak shoulder | **Shared far.** Both shoulders recenter onto the interior coarse peak. |

Do not reuse the single scan `donor_registration.lag_ms` as the recenter center. Shoulder
envelopes (Change step 1) are load-bearing.

---

## Change

In `lag_at_placement` (fingerprint path now; production path when S1 is re-scoped):

1. **Coarse.** *Per-shoulder* envelope correlation at `b_mapped`, ±`lag_envelope_max_lag_ms`
   (new, default **2000** — 20×100 ms bins, or the existing `register_donor_window` with a
   probe-only `max_lag_blocks`). Prefer the scan envelopes already in hand over a second PCM
   envelope, but only as per-shoulder series — not the one gap-level `donor_registration.lag_ms`.
   Record `envelope_lag_ms`, `envelope_peak_r`, `envelope_edge_pinned` on each shoulder.
2. **Guard.** Recenter **that shoulder** iff `envelope_peak_r ≥ min_envelope_r` (reuse 0.70)
   **and** the peak is ≥1 bin inside the coarse window. Otherwise that shoulder’s `start_frame`
   stays `b_mapped`. Fail open is per-shoulder: 31/g7 pre can stay near nominal while post
   recenters.
3. **Fine.** `lag_pair` ±`lag_max_lag_ms` (600) / 1 s template, centered on
   `b_mapped + round(envelope_lag)`, **after** map-coordinates (search and chain in map
   coordinates; post chained only if pre is a unique landmark). Report `frac_lag_ms` gross to
   original `b_mapped`. `edge_pinned` remains a property of the **fine** window around the new
   center. Map-coordinates census is landed: 31/g10’s both-shoulder dead-at-nominal is the
   current world, not a transitional prediction. After this plan, both should lock near −1153.

Contract `_contract.placement` must say so: “per-shoulder fine sweep at `b_mapped + envelope_lag`
when that shoulder’s coarse peak is interior; else `b_mapped`.” Old corpora stay valid
(`envelope_*` absent ⇒ nominal center).

S1, when unblocked, plumbs **this** measurement, not the nominal-only sweep. Cost is still two
FFTs per gap plus a decode-free envelope that scan already computed.

---

## Exit

Same binary, `lag_envelope_max_lag_ms` on vs off. Not a corpus fingerprint pass.

**Pair 31** (the original case):

| gap | expect |
|-----|--------|
| 31/g7 (failure; pre map-true **−38.1 ms**, post true −1160) | Per-shoulder. Pre stays the unique landmark (~−38 ms — not the old template-coordinate −47.5 / ≈ −50). A saturated / edge-pinned envelope still fails open. Post may unlock at −1160 as record-only detection. **Fail if pre is lost** (recenter yanked onto a shared −1160 / −1000). Skip-vs-fill is not this exit. |
| 31/g10, current world (this plan off) | both shoulders dead at nominal ±600. Pre non-landmark; post **not** the old chained −1152. Not a regression. |
| 31/g8, g10, g11 (true ≈ −1.11 s), after this plan | coarse interior ≈ −1.11 s on each live shoulder; fine `frac_lag_ms` agrees within ~50 ms; `peak_r` usable; fine `edge_pinned` false. 31/g10 specifically: **both** shoulders ~−1153 (pre was window-starved, not weak). |

**Pair 50** (same class, larger). Consensus **+2.7891 s**, last six gaps unmeasurable under ±600 ms
(`. . . . X X X X X X` against pair 31's `. . X X X`). At +2.79 s it is outside the proposed
2000 ms default `lag_envelope_max_lag_ms`, so this is also the test of whether that default is
the right size — if the coarse window has to reach 3 s to see pair 50, set it deliberately.

**Smoke join** on the oriented load-bearing cell `{2, 3, 13, 14, 18, 22, 32, 47, 51}` (9 pairs,
all agreeing within 50 ms; rebuilt 2026-08-14, archived registration plan §0.0 Step 3). Recenter
itself must not move median `frac_lag_ms` beyond re-quantization of a shifted center (a few ms):
none of these pairs' coarse peaks should be interior-and-far enough to trigger a recenter.

Compare against the **post-map-coordinates** baseline, not Step 5’s −21.9 ms. Map-coordinates
already spent the probe-`D` half: on the 6/9 fingerprinted so far (`{2,3,13,14,18,22}`),
measured-lag medians moved peak_r −16.0 → **−8.6 ms**, peak_z −15.8 → **−8.4 ms**. Recenter
moves neither remaining term. Do not spend that ~`D` move as a recenter failure, and do not
spend production α (~25 ms late) as a tolerance either. Full 9/9 (pairs 32, 47, 51) is still
open on the map-coordinates smoke-join; this plan’s own join re-checks the same cell. See
[TEMP-lag-decision-map-coordinates-plan.md](TEMP-lag-decision-map-coordinates-plan.md) Exit —
results, and archived
[§ Step 5](archive/TEMP-fill-placement-registration-plan.md#step-5-result-2026-08-14--the-bias-is-two-terms-and-only-one-is-the-probe).

26/g6 must stay near-nominal (map-coordinates left it at −41.75 / −45.43 ms), not follow the
−1.45 s slide.

If pair 31's later gaps still miss with a 2 s envelope, stop: that is a different question
(envelope locked onto the same wrong peak as the fill), not a window problem, and this plan
does not proceed to production plumbing (S1).
