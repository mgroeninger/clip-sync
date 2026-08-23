# TEMP — Envelope adaptive widen + self-referential floor

**Status:** draft, **PREMISE PARTIALLY REFUTED 2026-08-22 — do not start as written.** Not started.
A review against the post-nominal-first binary found that this plan's unifying premise ("both
questions have the same answer") no longer holds, that two of its three exit criteria are
unobtainable or demoted, and that its motivating defect is not addressable by its own Change. The
corrections are inline below, marked **[2026-08-22]**; nothing has been deleted, so the refuted
reasoning stays readable. See § *Correction pass* immediately below for the standing verdict.
**Deciding measurement 2 is answered (2026-08-22):** 31/g8 is a `BorderCapture::Narrow` trim split,
not a floor problem, and **leaves this plan**. Measurement 1 (pair-50 edge-pin vs 0.70) is still
open and is what remains before any rewrite. **Re-scoped 2026-08-19 to the rescue branch:**
[TEMP-lag-nominal-first-plan.md](archive/TEMP-lag-nominal-first-plan.md) landed 2026-08-20 (it needed no new
threshold and no dump) and confines everything in this plan — the widen ladder *and* any guard
refinement — to shoulders that are already dead at nominal (`peak_r` < 0.5). That collapses the
stakes of Problem §2: a wrong guard decision inside the rescue branch costs a wasted fine sweep
or an honestly-kept `decorrelated` row, never a corrupted healthy measurement. 26/g6's fixture
obligation transfers to nominal-first as "must never *enter* the rescue branch" (enforced by
ordering, tested by its bit-identical exit bar), not to a coarse-pass floor here.
Parent: [archive/TEMP-lag-envelope-recenter-plan.md](archive/TEMP-lag-envelope-recenter-plan.md)
(superseded — Change landed, ordering refuted by 26/g6; Result 2026-08-19 pair 31/50 dump).
Corresponds to the fill-placement leftover that was "Pair 50 envelope ~3 s" — this
plan replaces that item's flat "dump at 3000 ms" direction with an adaptive one and adds the
floor question that item didn't cover. ~~**Keep the default 2000 ms until this plan's ladder dump
locks pair 50's +2.79 s interior**~~ — **[2026-08-22] struck.** Keeping the 2000 ms default is
still right, but not for this reason: the stakes note below already established that production
places pair 50's six `patch`-tier gaps correctly via anchor-seam, so nothing waits on a ladder
locking +2.79 s. The replacement of the backlog's flat "dump at 3000 ms" direction with an adaptive
one is also **withdrawn** — see the Correction pass, verdict 5. Do not raise the 0.70 envelope
guard here either (unchanged).

## Correction pass (2026-08-22)

Standing verdict after review against the binary that shipped nominal-first. **Every numbered
verdict below overrides the section it names.** Nothing here is a Change; it is the record of
what this draft may and may not still claim.

1. **The unifying premise is broken.** § Direction's "both questions have the same answer" is the
   part that failed. Sizing is a **search-radius** question. The floor's remaining job is the much
   cheaper one of **declining doomed rescue candidates** early. Using that same floor as the
   **widen trigger** is a *third* job, and it is the one that contradicts the plan's own named
   target (verdict 4). These are three deliverables in one plan, against this folder's
   "one deliverable per plan" rule — whatever survives should be split.
2. **26/g6 is no longer an in-branch calibration point.** Under nominal-first it is
   `EnvelopeRescue::NotConsulted`, and a `NotConsulted` row stamps **no** `envelope_*` fields at
   all — there is nothing to calibrate against, not merely a weaker reading. The 0.946 / 0.961
   figures quoted in Problem § 2 and Exit 1 come from the superseded envelope-first binary and
   **cannot be reproduced by a production dump**. Reproducing them would require a diagnostic-only
   forced envelope pass, which this plan does not propose.
3. **The floor cannot recover 31/g8 — and 31/g8 is not this plan's defect.** The Change is a
   scalar go/no-go applied *downstream of* the envelope curve's `max_by`. If the curve ranks
   +800 ms above −1146 ms, no statistic computed on the argmax can select the runner-up.
   z/prominence can buy an honest `Declined` instead of a confidently wrong lag; it cannot buy the
   right lag. **That is a selection failure, not a threshold failure** — and measurement 2 then
   showed the two paths do not even share a curve: 31/g8 **leaves this plan** (see the
   deciding-measurement close-out below). Do not cite it as the floor's second instance.
4. **Change step 3's widen trigger contradicts its own exemplar.** The trigger is
   `edge_pinned` **and** clears the floor. Step 3 names `50/g009` (r 0.47) as "exactly the case
   that must *not* trigger a widen" — but 50/g009 *is* the pair-50 exemplar whose true peak sits
   outside the window, and Exit 2 requires the ladder to find it. The plan asks one gap to be both
   the thing that must widen and the thing that must not. Note also that the existing guard already
   treats `edge_pinned` as a hard disqualifier, so the widen population is by construction the set
   the current guard rejects: 50/g009 fails *both* halves of the proposed trigger.
5. **The adaptive ladder is not justified on cost.** `lag_envelope_max_lag_ms` is already a config
   knob, so a flat wider re-fingerprint costs a flag rather than a Change. The ladder was only ever
   justified by per-gap cost, and step 4's own numbers (2/40 shoulder-sides edge-pinned) say that
   cost is negligible. Prefer a flat re-fingerprint as instrumentation.
6. **Envelope z is not fine-pass z.** The envelope curve is ~41 points (±2000 ms at 100 ms bins)
   against the fine sweep's ~57.6k (±600 ms at sample resolution), and `peak_z` uses a population
   std, so `z_max = sqrt(n − 1)` — about **6.32** at envelope scale versus about **240** at fine
   scale. A fine-pass or `splice_dualfit` threshold (the 6.41/6.40 figures in Problem § 2, or
   p05 3.14 / p25 4.91) is therefore meaningless here: 6.4 is an ordinary fine value and a
   structurally *impossible* envelope one. Any ladder also changes `n` per rung (41 → 61 → 81 at
   2000 → 3000 → 4000 ms), so envelope z is not comparable across rungs either. If the field is
   added, record the curve length beside it.

### What is still true

- The envelope radius still censors pair-50-shaped offsets in `lag_decision` dumps; Result 8's
  dump-visible step-boundary base rate of zero is a search-radius effect, and a wider-envelope
  re-fingerprint is still the cheap way to size that population.
- `min_envelope_r = 0.70` is still an uncalibrated absolute floor inside the rescue branch.
- Both § Non-goals entries on gap duration and on A-content-derived floors stand unchanged.

### Deciding measurements

1. **Does any pair-50 edge-pinned shoulder actually clear 0.70?** Still open. `envelope_peak_r` and
   `envelope_edge_pinned` are already serialized, and `Declined` rows still carry them, because
   `rescue_shoulder` stamps the peak before returning. Answerable from an existing dump on the
   current binary. **If nothing clears it, the widen trigger has no members and the ladder is
   refuted before anyone writes it.**
2. **Do the two paths disagree about 31/g8's envelope curve?** **Answered 2026-08-22 — yes, and
   it is a capture split, not a floor problem. 31/g8 leaves this plan.** HEAD dump
   `gap-files/2026-08-22-31g8-capture` (`--fingerprint-gap 9`, default `BorderCapture::Narrow`) is
   bit-identical to `2026-08-20-nominal-first-on-decoupled-unconditioned`: pre `border_displacement_ms`
   **13.0**, envelope **+800 ms / r 0.816**, `declined`, kept +550.8 / 0.152. Envelope-first
   (`2026-08-19-recenter-on`) and the *first* `2026-08-20-nominal-first-on` dump (shared wide
   capture): pre **D = 1983.8 ms**, envelope −1200 / r 0.821, kept **−1146.1 / 0.974**. Post
   barely moved (D 5.2 → 0.7 ms) and still rescues at −1137.7 on every on-arm. Ordering is not
   the defect — the first nominal-first-on dump still rescued pre. What moved is the **primary
   capture**: unconditional wide trimmed ~2 s off the gap-facing pre end; lazy-narrow barely
   trims. Envelope r of the two coarse peaks is almost the same (0.821 vs 0.816); only the wide
   template then fine-locks. The independent WAV sweep (−1146.1 on throat-adjacent clips) still
   stands. Owned by BACKLOG's 31/g8 Narrow-capture item, not by a floor or a ladder. Do not
   re-open it here.

## Problem

**Second false-peak instance (2026-08-21).** `31/g8`'s pre-shoulder: the rescue branch reports a
declined "+800 ms candidate, r 0.82", but an independent sample-domain sweep locks the shoulder at
**-1146.1 ms, r 0.9896** (prominence 0.799), agreeing with its own post within 8.4 ms — and reads only
**r 0.1777** at +680 ms. An envelope r of 0.82 sitting where the waveform reads 0.18 is the same
coarse-envelope false peak as `26/g6`. **The difference matters for this plan's re-scope:** 26/g6 is
now handled by nominal-first ordering (it was alive at nominal, so it never enters the branch), but
31/g8's shoulders are genuinely dead at nominal (r -0.02) — it enters the rescue branch *legitimately*
and then picks the wrong peak. So the "confident false peak" problem is **not** fully retired by
nominal-first; it survives inside the branch, which is exactly where Problem section 2's floor question
still lives. Evidence:
[TEMP-collapsed-bridge-census-2026-08-20.md](TEMP-collapsed-bridge-census-2026-08-20.md) Result 7.

> **[2026-08-22] This paragraph is withdrawn as motivation for this plan — Correction pass
> verdict 3, then deciding measurement 2.** The +800 / r 0.82 reading is real on
> `BorderCapture::Narrow`, and the WAV sweep still locks at −1146.1. That is **not** a second
> 26/g6-class in-branch false peak: Wide / envelope-first keep −1146.1 because the shared capture
> trims 1.98 s off the pre template (`D` 1984 vs 13). 31/g8 leaves this plan. The floor still
> could not have selected −1146 from an argmax at +800; that hypothetical no longer matters.

**Stakes note (2026-08-21).** [TEMP-pair50-anchor-seam-outcomes-2026-08-21.md](TEMP-pair50-anchor-seam-outcomes-2026-08-21.md)
measured what production actually does with pair 50's +2.789 s region: all six `patch`-tier gaps there
are already placed at the correct offset by the editorial anchor-seam bracket search
(`max_anchor_bracket_secs`, default 5.0 s), independently of `lag_decision`. So the sizing question
below is about **diagnostic honesty and the rescue branch**, not about a region production cannot
otherwise reach — a widen here would not be the cluster's only rescue. That note's own open failures
(50/g11–g13 skipping between correctly-resolved flanks) are downstream of anything this plan changes.


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

   > **[2026-08-22] Correction pass verdicts 2 and 6.** The 0.946 / 0.961 readings above are from
   > the superseded envelope-first binary; under nominal-first 26/g6 is `NotConsulted` and stamps
   > no `envelope_*` fields at all, so **this comparison is not reproducible from a production
   > dump**. The closing caution is right that z may not separate the populations, but the reason
   > given is wrong: fine `peak_z` and envelope `peak_z` are computed over curves three orders of
   > magnitude apart in point count, so 6.41 is not a value envelope z can take — its ceiling is
   > about 6.32. Do not carry any fine-scale number across. The remaining floor job named in this
   > paragraph — declining doomed candidates to save a wasted fine sweep — is the only part that
   > survives intact.

## Direction

> **[2026-08-22] The heading claim of this section is REFUTED — Correction pass verdict 1.** The
> two questions do **not** have the same answer. Sizing is a search-radius question; the floor's
> remaining job is declining doomed candidates; and using that floor as the widen trigger is a
> third job that contradicts the plan's own exemplar (verdict 4). The self-calibration argument
> below is still a good argument *for recording the statistics* — it is not an argument that one
> statistic resolves both questions. Read the rest of this section as motivation for Change step 1
> only.

~~**Both questions have the same answer:**~~ **stop using absolute floors and let each correlation sweep
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
   **[2026-08-22]** This step is the only part of the Change that survives review intact. Two
   amendments: (a) record the **curve point count** beside the statistics — envelope z is bounded
   by `sqrt(n − 1)` and is not comparable to fine z, nor across ladder rungs (verdict 6); (b) the
   populated population is only shoulders dead at nominal, so expect a small n and do not describe
   the result as a base rate. The `secondary_peak` reuse question resolves cleanly: it already
   takes the same `&[(i64, f64)]` curve type the envelope pass builds, so no envelope-scale copy
   is needed.
2. **Guard, still per-shoulder — rescue branch only.** Once the diagnostic dump is read, decide
   whether `min_envelope_r` is replaced or joined by an `envelope_peak_z` floor. Do not guess the
   replacement threshold before the dump — this repeats the recenter plan's own discipline (reuse
   0.70 provisionally, don't retune blind). Under nominal-first this guard is only ever consulted
   for shoulders already dead at nominal, so its job is declining doomed candidates early, not
   protecting healthy measurements (see the Status re-scope note).
3. **Adaptive widen loop.** **[2026-08-22] BLOCKED — self-contradictory as written (Correction pass
   verdicts 4 and 5).** The trigger below rules out `50/g009` explicitly, yet 50/g009 is the very
   pair-50 shoulder Exit 2 asks the ladder to rescue; and the ladder's cost justification does not
   survive the observation that a flat wider re-fingerprint costs a CLI flag. Do not implement this
   step. Deciding measurement 1 (Correction pass) may refute it outright by showing the trigger has
   no members. Text kept below as the record of what was proposed.
   Start each dead shoulder's coarse pass at the current default (2000 ms).
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

> **[2026-08-22] Exit 1's 26/g6 clause is unobtainable and Exit 2 is demoted — Correction pass
> verdicts 2 and 4.** 26/g6 emits no `envelope_*` fields under nominal-first, so the calibration
> question below cannot be answered by the dump it names. Exit 2 survives **only as a test oracle**
> — pair 50 is the corpus's one independently-confirmed far-offset ground truth — and no longer as
> a justification, since production already places those gaps via anchor-seam. Deciding
> measurement 2 is **done** (31/g8 left this plan). Do not start work; run measurement 1 first.

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
3. Only after both dumps land: decide the guard threshold change (if any). Do not raise the
   production default off this plan's draft.

**Ordering:** [TEMP-lag-nominal-first-plan.md](archive/TEMP-lag-nominal-first-plan.md) landed
2026-08-20; this plan's dumps run on that binary so the ladder and guard
statistics are measured in the rescue branch they will actually live in.
