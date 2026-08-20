# TEMP — Envelope-recenter `lag_decision`

**Status:** **superseded 2026-08-19, archived** — see the closing note at the end of this file.
The Change below **shipped** (`lag_at_placement` envelope recenter, default 2000 ms) and the
mechanism was validated on pair 31, but the smoke join found the *ordering* is wrong (26/g6
false lock, § Result); the Exit was **not** met as written and S1 never unblocked under this
plan. Successor: [../TEMP-lag-nominal-first-plan.md](../TEMP-lag-nominal-first-plan.md)
(measure → triage → rescue → accept; inherits this plan's dumps as baselines). Inherits
map-coordinate `lag_pair` from the 14-gap census re-run (clean 2026-08-19); map-coordinates
smoke-join closed at full scope, 9/9 (2026-08-19) — that plan is done.
Parent: [TEMP-fill-placement-registration-plan.md](TEMP-fill-placement-registration-plan.md)
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
> see [TEMP-31g7-gap-shape-vocabulary.md](TEMP-31g7-gap-shape-vocabulary.md) §9. This plan's
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

Same binary, `--lag-envelope-max-lag-ms` **2000** (or omitted) vs **0**. Not a corpus fingerprint pass.

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

Compare against the **post-map-coordinates** baseline, not Step 5’s −21.9 ms / −26.1 ms. Full
9/9 (`{2,3,13,14,18,22,32,47,51}` peak_r; 8/8 dropping 22 under peak_z), map-coordinates already
spent the probe-`D` half: measured-lag medians moved peak_r −21.9 → **−18.8 ms**, peak_z
−26.1 → **−18.0 ms**. (The 6-pair partial subset checked mid-run had suggested a larger move,
−16.0→−8.6 / −15.8→−8.4 — that overstated the full-corpus shift because it over-represented the
larger-`D` pairs; the number above supersedes it.) Recenter moves neither remaining term. Do not
spend that ~`D` move as a recenter failure, and do not spend production α (~25 ms late) as a
tolerance either. This plan’s own smoke join re-checks the same cell against these numbers, not
Step 5’s. See
[TEMP-lag-decision-map-coordinates-plan.md](TEMP-lag-decision-map-coordinates-plan.md) Exit —
results, and archived
[§ Step 5](TEMP-fill-placement-registration-plan.md#step-5-result-2026-08-14--the-bias-is-two-terms-and-only-one-is-the-probe).

26/g6 must stay near-nominal (map-coordinates left it at −41.75 / −45.43 ms), not follow the
−1.45 s slide.

If pair 31's later gaps still miss with a 2 s envelope, stop: that is a different question
(envelope locked onto the same wrong peak as the fill), not a window problem, and this plan
does not proceed to production plumbing (S1).

---

## Result (2026-08-19, `gap-files/2026-08-19-recenter-on` vs `-off`)

Fingerprint runs (`--features calibration,he-aac`) on pairs 31 and 50, `--lag-envelope-max-lag-ms
2000` vs `0`. `manifest.json` patch/skip outcomes are byte-identical on vs off for both pairs —
confirmed record-only, no gate effect yet.

**31/g7 (the original failure).** Pre stays the unique landmark: envelope recenters by only
−100 ms (`envelope_peak_r` 0.90, guard passes) and the fine sweep still lands at **−38.07 ms**
(off-run: −38.07 ms, unchanged) — the landmark is not lost. Post flips from `decorrelated`
(off: `peak_r` 0.075, floor reading, +330.8 ms garbage) to `timing_offset` at **−1160.25 ms**
(`peak_r` 0.78, `envelope_peak_r` 0.94, not edge-pinned) — matches the offline ±2000 ms sweep's
−1160 ms exactly. Both exit-table requirements met; pre not yanked onto post's −1160/−1000.

**31/g10.** Off: both shoulders `decorrelated` at nominal (pre −539.1 ms `r` 0.16, post
−444.1 ms `r` 0.19 — matches the memory'd "weak-pre chain is gone" reading). On: both lock to
`timing_offset`, pre −1151.6 ms (`r` 0.99), post −1155.4 ms (`r` 1.00), agreeing within 4 ms,
neither edge-pinned. Confirms pre was window-starved, not weak — not a regression, a rescue.

**31/g8, g9, g11.** All timing_offset on both shoulders, −1137.7 to −1158.4 ms, `r` 0.96–0.98,
none edge-pinned, shoulders agree within ~12 ms. Matches "≈ −1.11 s, agree within ~50 ms" bar.

**No-op check.** g1–g6 (pre-step, already-correct gaps): `envelope_lag_ms` = 0 on every shoulder,
`frac_lag_ms` identical to the off-run bit-for-bit (e.g. g6 pre −41.9→ same to full precision).
Recenter does not perturb gaps it has no business touching.

**Pair 50.** g1–g7 unaffected (small nominal lags, envelope 0, identical to off-run). g8–g17 stay
`decorrelated` on both on and off — recenter does not rescue them; envelope itself is
low-confidence there (`envelope_peak_r` 0.41–0.71, scattered, not a clean single offset) so the
guard mostly declines, and where it does recenter (g12, g18, g19: pre swings from
+41/−90/+582 ms off to −1353/−1301/+1386 ms on) the fine result still lands `decorrelated` at
low `r` (0.07–0.27) — the envelope found *something* but not the true consensus lag. Exactly one
shoulder edge-pins: g9 post at the +2000 ms window edge (`envelope_peak_r` 0.47, guard would
decline anyway). This is the predicted outcome: pair 50's true offset (+2.789 s) is outside the
2000 ms default reach, so the envelope saturates rather than lying about a false interior peak.
Per the exit criteria, this is "the test of whether that default is the right size," not a
recenter failure — pair 31 (true ≈ −1.15 s, inside 2 s) is fully rescued, so the mechanism itself
is validated. Pair 50 needs a deliberately wider window (~3 s) as separate follow-up, not a
reason to stop before S1.

**Verdict:** recenter mechanism validated on pair 31 (the design case) at every checked gap;
no regressions on near-nominal gaps; pair 50 correctly exposes the 2000 ms default as too narrow
for its own +2.79 s offset rather than mis-locking. Smoke-join cell
`{2,3,13,14,18,22,32,47,51}` and pair 26/g6 were not part of this run (scoped to 31/50 only) and
still need checking before closing S1.

## Result (2026-08-19, smoke join: `gap-files/2026-08-19-recenter-smoke-join-on` vs `-off`)

Fingerprint runs on the 9-pair load-bearing cell `{2,3,13,14,18,22,32,47,51}` plus pair 26,
joined against `gap-files/2026-08-13-repeat-terms/` scan/patch reports via
`scripts/measure/route2-consensus.ps1` (both `peak_r` and `peak_z` gates, 160 shoulder-rows
each). `manifest.json` patch/skip outcomes byte-identical on vs off, same as pairs 31/50 —
still record-only.

**Cell membership and bias: pass.** peak_r 9/9, peak_z 8/8 (pair 22 drops under peak_z in both
runs, expected/pre-existing). Bias moved within noise: peak_r −8.6 → −7.9 ms, peak_z
−8.4 → −8.2 ms. Two shoulders wobble `peak_z` across the 12.0 gate line by floating-point-scale
amounts with the `frac_lag_ms` itself unchanged to 4+ decimals (3/7 pre: z 12.72→11.24, lag
−372.293 ms both runs; 18/39 post: z 12.26→10.90, lag 334.838 ms both runs) — re-quantization
noise from the shifted sub-sample center, within the "a few ms" tolerance this exit already
allows.

**26/g6: fails the named exit bar.** Off-run matches the bar exactly (pre −41.754 ms `r` 0.997
`z` 6.13, post −45.428 ms `r` 0.993 `z` 6.45 — near-nominal, as expected). On-run does **not**
stay near-nominal and does **not** follow the −1.45 s applied slide the bar warns against —
it locks onto a third, wrong peak: pre +1880.15 ms (`envelope_lag_ms` 1400, `envelope_peak_r`
0.946), post +1876.47 ms (`envelope_lag_ms` 1400, `envelope_peak_r` 0.961). Both clear the 0.70
guard, neither edge-pinned, both shoulders agree with each other (step 3.7 ms) — a confident,
self-consistent, **wrong** lock. `route2-consensus`'s independent ±600 ms plausibility guard
(`PreGuardMs`/`PostGuardMs`, unrelated to this plan) happens to reject both shoulders as
implausible, which is why pooling didn't see it and production's patch/skip outcome is
unaffected — but the diagnostic measurement itself is corrupted for this gap, exactly the failure
mode the exit bar was written to catch.

This is a second concrete case (after 50/g009's honest edge-pin) of the coarse envelope's
`envelope_peak_r ≥ 0.70` guard alone being insufficient: 50/g009 was a *true* peak that failed to
clear 0.70 and correctly stayed unrecentered; 26/g6 is a *false* peak that cleared 0.70
comfortably and got recentered anyway. Both point the same direction as
[TEMP-envelope-adaptive-widen-plan.md](../TEMP-envelope-adaptive-widen-plan.md)'s Problem §2: a bare
correlation-coefficient floor on the coarse pass can't tell a real peak from a plausible-looking
wrong one, and 26/g6 is now the concrete "cleared the floor, still wrong" fixture that plan's
Change step 1 diagnostic dump should include and its Change step 2 guard revision must reject
(likely via `envelope_peak_z`/`envelope_prominence` — 26/g6's false peak has notably lower
prominence, 0.0137/0.0127, than either of its own true near-nominal readings, 0.0254/0.0271, or
pair 31/50's genuine locks, 0.6–0.7 range — though both are numerically "low" so this alone isn't
proven sufficient, only suggestive).

**Verdict: smoke join does not close S1.** Do not treat the 9-pair cell as done — it passed, but
26/g6 (inside the same corpus, explicitly named in this plan's own exit criteria) did not. Recenter
should not proceed to production plumbing until the adaptive-widen plan's guard revision (peak_z /
prominence on the coarse pass) is in place and re-validated against 26/g6 specifically, not just
pairs 31/50/the 9-pair cell.

---

## Closing note (2026-08-19, archived)

**Archived as superseded, not as finished.** This plan's original definition of done — recenter
validated on pairs 31/50 *and* the smoke join clean, unblocking S1 — was **not met**: 26/g6
failed its named exit bar (above), so S1 never proceeded under this plan. It is archived anyway
because the smoke join's diagnosis changed what the fix is. The verdict paragraph above (written
when the failure was found) still frames the remedy as a *guard revision* — a better coarse-pass
floor (peak_z / prominence) in the adaptive-widen plan. Further analysis the same day reframed it
as an **ordering** defect: 26/g6 is alive at nominal (`r` 0.997/0.993), so under this plan's own
population statement ("shared far offsets... dead at nominal") the envelope should never have
been consulted for movement at all — no coarse-pass statistic is needed to protect a shoulder
whose nominal fine sweep already answers at landmark quality. Chasing a floor that can tell
26/g6's false peak from pair 31's true ones would have been solving the hard version of a problem
whose easy version (measure at nominal first; rescue only dead shoulders; accept only
improvement) is strictly safer.

What survives, and where it went:

- **The mechanism** (per-shoulder coarse envelope, 0.70/interior guard, `envelope_*` wire
  fields, `--lag-envelope-max-lag-ms`) shipped under this plan and stays — demoted from
  unconditional pre-step to rescue path by the successor,
  [../TEMP-lag-nominal-first-plan.md](../TEMP-lag-nominal-first-plan.md).
- **The validation runs** (`gap-files/2026-08-19-recenter-on`/`-off`,
  `-smoke-join-on`/`-off`) are the successor's diff baselines; its exit bars are this plan's,
  tightened (alive shoulders must be bit-identical to the off-run, not merely within
  re-quantization noise).
- **The window-sizing question** (pair 50 needs ~3 s) and any residual coarse-pass guard
  refinement live in [../TEMP-envelope-adaptive-widen-plan.md](../TEMP-envelope-adaptive-widen-plan.md),
  re-scoped to the rescue branch.
- **Until the successor lands**, the shipped default (2000 ms, envelope-first) corrupts 26/g6's
  `lag_decision` row in any fingerprint dump — pass `--lag-envelope-max-lag-ms 0` on runs whose
  lag rows must be trusted.
