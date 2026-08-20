# TEMP — Envelope adaptive widen + self-referential floor

**Status:** draft, not started, 2026-08-19. **Re-scoped 2026-08-19 to the rescue branch:**
[TEMP-lag-nominal-first-plan.md](archive/TEMP-lag-nominal-first-plan.md) landed 2026-08-20 (it needed no new
threshold and no dump) and confines everything in this plan — the widen ladder *and* any guard
refinement — to shoulders that are already dead at nominal (`peak_r` < 0.5). That collapses the
stakes of Problem §2: a wrong guard decision inside the rescue branch costs a wasted fine sweep
or an honestly-kept `decorrelated` row, never a corrupted healthy measurement. 26/g6's fixture
obligation transfers to nominal-first as "must never *enter* the rescue branch" (enforced by
ordering, tested by its bit-identical exit bar), not to a coarse-pass floor here.
Parent: [archive/TEMP-lag-envelope-recenter-plan.md](archive/TEMP-lag-envelope-recenter-plan.md)
(superseded — Change landed, ordering refuted by 26/g6; Result 2026-08-19 pair 31/50 dump).
Corresponds to BACKLOG item "Pair 50 envelope ~3 s" — this
plan replaces that item's flat "dump at 3000 ms" direction with an adaptive one and adds the
floor question that item didn't cover.

## Problem

The recenter plan's own Result shows the mechanism works when the coarse window reaches the true
peak (pair 31, ≤1.16 s, inside the 2000 ms default) and saturates honestly when it doesn't (pair
50, true offset **+2.789 s**, outside 2000 ms — `50/g009` post pins at exactly `envelope_lag_ms =
2000`). Two separate open questions follow from that, and this plan is about not conflating them:

1. **Sizing.** A single fixed `lag_envelope_max_lag_ms` can't serve both pairs without either
   starving pair 31-shaped gaps (narrow) or paying full-width correlation cost on every gap for a
   pair-50-shaped case that's rare (wide). Gap *duration* does not predict the needed width —
   confirmed on both pairs in the 2026-08-19 dump: pair 31's g8/g9/g11 (durations 0.6–0.64 s) carry
   the same ≈−1.15 s lag as g10 (1.87 s); pair 50's g012 (0.55 s) and g010 (2.58 s) sit in the same
   offset regime. The lag is a property of the pair's map step at that point in the timeline, not
   of the gap sitting in front of it — so don't scale the window by gap duration.
2. **Floor.** The existing recenter guard (`min_envelope_r`, reused at 0.70) is an absolute
   correlation-coefficient threshold. Every attempt at an absolute floor elsewhere in this project
   has been rejected: a media-spec-based floor requires tracking per-media technical properties
   (cumbersome, error-prone, refuted for `anchor_seam_min_prominence`); a floor derived from A's
   content is unavailable exactly where it's needed most, since the gap is missing information by
   construction, and using content around the gap either includes wrong-region audio or excludes
   the only audio available to judge against. `min_envelope_r = 0.70` is exactly this kind of
   absolute floor and has never been calibrated against real content for the coarse pass
   specifically (only reused from context elsewhere). **Now confirmed insufficient, not just
   unvalidated:** the 2026-08-19 smoke-join dump found `26/g6` clears 0.70 comfortably on both
   shoulders (`envelope_peak_r` 0.946 / 0.961, neither edge-pinned, shoulders agree within 3.7 ms)
   while locked onto a wrong peak — the diagnostic reports a confident, self-consistent +1.88 s
   lag where the true value is −41.75 / −45.43 ms (see
   [archive/TEMP-lag-envelope-recenter-plan.md](archive/TEMP-lag-envelope-recenter-plan.md)
   Result, "smoke join"). This is the coarse-pass mirror of `50/g009`'s failure mode: that gap is
   a true peak that failed the floor and correctly stayed unrecentered; `26/g6` is a false peak
   that passed the floor and got recentered anyway. A single scalar threshold on `peak_r` alone
   cannot separate these two cases from the same guard. **Re-scope note:** under nominal-first
   ordering, 26/g6 never reaches this guard (alive at nominal, `r` 0.997/0.993 — the fine sweep
   itself is the protection), so this plan no longer has to solve the "confident false peak vs
   healthy shoulder" version of the floor problem. What remains for the floor is cheaper and
   lower-stakes: declining doomed rescue candidates early (pair 50's g12/g18/g19 all land
   `decorrelated` at fine `r` 0.07–0.27 and are already rejected by nominal-first's acceptance
   rule; a coarse-pass z/prominence check would just save the wasted fine sweep). One more
   caution from the 26/g6 fine-level data: the false lock's fine `peak_z` (6.41/6.40) *exceeds*
   the true lock's (6.13/6.45) — z alone may not separate populations at envelope scale either;
   weigh `envelope_prominence` at least equally when reading the step-1 dump.

## Direction

**Both questions have the same answer: stop using absolute floors and let each correlation sweep
self-calibrate against its own noise floor.** The fine `lag_pair` sweep already does this —
`peak_z = (peak_r − mean) / std` over the *whole* correlation curve at that shoulder
(`measure.rs:375`), plus `prominence` (`peak_r − second_peak_r`, `:376`) and `top2_spacing_ms`
(`:377-378`). None of these read anything about the media (codec, bitrate, source) or about gap
content (the curve is built entirely from the two shoulders' real audio, `lag_correlation_curve`
over `ea`/`eb`). They ask a self-contained question — "does this peak stand out from the rest of
*this* sweep" — which is exactly the question a floor needs to answer without global tuning.

The coarse envelope pass (`envelope_peak_at`, `measure.rs:974-1010`) computes the identical
`curve` via `lag_correlation_curve` (`:994`) but only keeps `max_by` — `peak_r` and `edge_pinned`,
nothing else. The reduction the fine pass already does is one function reuse away.

### Change

1. **Extend `EnvelopePeak`.** Add `peak_z` and `prominence` (second-peak lookup + z-score, same
   formulas as `LagSummary`) computed over the envelope-resolution curve. Reuse `secondary_peak`
   if its lag-bin type lines up, or a thin envelope-scale copy if not. Record
   `envelope_peak_z` / `envelope_prominence` alongside the existing `envelope_lag_ms` /
   `envelope_peak_r` / `envelope_edge_pinned` fields. Diagnostic first: land this and re-dump pair
   31/50/9-pair before touching the guard, so `min_envelope_r = 0.70` can be checked against
   `envelope_peak_z` on real data (does 0.70 correspond to a high z, or is the coefficient
   threshold gating on a different population than the z-score would?).
2. **Guard, still per-shoulder — rescue branch only.** Once the diagnostic dump is read, decide
   whether `min_envelope_r` is replaced or joined by an `envelope_peak_z` floor. Do not guess the
   replacement threshold before the dump — this repeats the recenter plan's own discipline (reuse
   0.70 provisionally, don't retune blind). Under nominal-first this guard is only ever consulted
   for shoulders already dead at nominal, so its job is declining doomed candidates early, not
   protecting healthy measurements (see the Status re-scope note).
3. **Adaptive widen loop.** Start each dead shoulder's coarse pass at the current default (2000 ms).
   If the result is `envelope_edge_pinned` **and** clears whatever floor step 2 lands on (i.e. it
   looks like a real signal running off the edge, not noise pinned at the boundary — see
   `50/g009`'s edge-pin at `envelope_peak_r` 0.47, likely low-`z` too and exactly the case that
   must *not* trigger a widen), retry at the next step (candidate ladder: 2000 → 3000 → 4000 ms,
   capped) and take the best result. Otherwise stop at the first pass — most gaps (pair 31 g1–g6,
   pair 50 g1–g7 in the 2026-08-19 dump) lock inside 2000 ms and never pay for a retry.
4. **Cost.** Only edge-pinned-and-plausible shoulders pay extra correlation passes. Per the recenter
   plan's own baseline, the coarse pass is decode-free (reuses scan envelopes) — cost is bounded by
   how many shoulders actually saturate, which the 2026-08-19 dump puts at a small fraction (2/40
   shoulder-sides across pairs 31+50 edge-pinned; only one of those had reasonable `r`).

## Non-goals

- Do not scale the coarse window by gap `duration_secs`. Confirmed uncorrelated with lag magnitude
  on both pairs in the 2026-08-19 dump (see Problem §1) — this would starve short gaps that need a
  large offset and over-widen long gaps that don't.
- Do not derive the floor from A's content in or around the gap. The self-referential curve
  statistics use only the two shoulders' existing correlation sweep, never gap content.
- Do not tune `min_envelope_r` or any `envelope_peak_z` threshold before the diagnostic dump in
  Change step 1 is read. This plan's first deliverable is the field, not the number.
- Do not feed one gap's widened result into a neighboring gap (same anchored_retry risk the
  recenter plan already ruled out) — the ladder is per-shoulder, per-gap, stateless across gaps.
- Do not raise the ladder's cap speculatively. 4000 ms is a guess for pair 50 (+2.789 s needs
  ≥2789 ms); confirm with a dump before assuming it's enough, and do not chase a widen ceiling
  past what's demonstrated necessary.

## Exit

1. Diagnostic dump (Change step 1) on pair 31 + pair 50 + the smoke-join cell (including pair 26),
   no guard change: confirm `envelope_peak_z` / `envelope_prominence` are populated and inspect
   whether 0.70 on `envelope_peak_r` tracks a sensible z/prominence split — in particular, does
   `50/g009`'s edge-pin (r 0.47) read as low-z (noise) while a genuine but weak far-peak would read
   differently, **and does `26/g6`'s false lock (r 0.946/0.961) read as low-z/low-prominence
   despite the high r** (its raw `prominence` is 0.0137/0.0127, roughly half its own true-peak
   reading of 0.0254/0.0271 off-run — suggestive but not yet proven as a clean separator)?
   26/g6 stays in this dump as a *calibration point* for the statistics; its must-not-recenter
   obligation now lives in [TEMP-lag-nominal-first-plan.md](archive/TEMP-lag-nominal-first-plan.md)'s
   exit bar (bit-identical to off-run), which protects it whether or not any revised guard here
   would.
2. Ladder dump: pair 50 at the 2000→3000→4000 ladder vs the flat 2000 baseline. Confirm the true
   +2.789 s lock appears at the correct rung, edge-pinned-and-implausible shoulders (low z) do not
   widen past 2000, and pair 31 / the smoke-join cell are unchanged (first rung already resolves
   them, so the ladder must be a no-op there — same bar the recenter plan's own smoke join used).
   `26/g6` is not a widen case (its false peak is interior at 1400 ms, well inside 2000) and not
   the ladder's problem — under nominal-first it never enters the rescue branch at all.
3. Only after both dumps land: decide the guard threshold change (if any) and update
   `BACKLOG.md`'s "Pair 50 envelope ~3 s" entry to point here instead of the flat-3000-dump
   direction it currently describes.

**Ordering:** [TEMP-lag-nominal-first-plan.md](archive/TEMP-lag-nominal-first-plan.md) landed
2026-08-20; this plan's dumps run on that binary so the ladder and guard
statistics are measured in the rescue branch they will actually live in.
