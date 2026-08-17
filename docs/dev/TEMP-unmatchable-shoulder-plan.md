# TEMP — Unmatchable-shoulder `min` reduction (S2b)

**Status:** draft, 2026-08-14. Not started. Decisions 1–5 open; do not code until they close.
Parent: [TEMP-fill-placement-registration-plan.md](TEMP-fill-placement-registration-plan.md)
(§0 S2b, §4.2 min-reduction pathology, §4.3 rival refuted).

31/g7 is an ear-confirmed stutter: production places at **−977 ms** (`align_adjustment_secs`
−0.9774, seams 0.437 / 0.437, `patch_tier: high`) where the local registration is **≈ −47 ms**.
The unified objective scores waveform as `pre.min(post)` (`waveform_min_at_start`). At the truth
the post border has no match in B, so `min` is ~0.075 and loses to a placement that degrades the
good shoulder in order to raise the minimum. Dual-fit is on, reaches post 0.124 at its own best
lag, and declines. There is no placement where both shoulders are good.

This plan changes the **scoring objective** so that gap still patches, at ≈ −47 ms. It does not
skip the gap, and it does not widen or recast the search.

---

## Goal

On 31/g7, the fit-mode search's argmax moves to ≈ **−47 ms** and the gap **still patches**.
The change is checked against the oriented 317-gap baseline
(`gap-files/2026-08-13-repeat-terms/`) for every placement it moves.

`fit_terms.waveform_min` (and the reported seams) must make the new reduction visible; S5
already emits the terms.

## Non-goals

- Do not skip 31/g7. A magnitude cap is the instrument §4.1 rejected: it would also refuse
  pair 31's genuine −1.15 s fills.
- Do not fix 26/g6. That is the source defect (near-nominal existed and lost on score). A move
  there is an accident to inspect, not success.
- Do not move 31/g8, g10, g11. Those −1.11 s placements are the pair's step, both shoulders
  matchable. 17/g34 is the 26/g6 class, not this class.
- Do not plumb `lag_decision` / S1. The production path does not compute it; this plan may not
  grow a dependency on the fingerprint sweep.
- Do not reuse `short_gap_one_strong_seam_fallback` as-is. It is **gate-mode only** and ignored
  under `fill_mode = fit`.
- Do not retune `LATE_START_PENALTY` / `fill_fit_early_start_penalty_scale`. S2(2) refuted a
  nominal-referenced prior as this fix.
- Do not change dual-fit's `SEAM_LOCAL_SEARCH_MS`, the fill search radius, or `fill_offset_mode`.

---

## Why a scoring change, not a skip

The applied placement is the **zero-move bracket** sliding ~1 s inside the nominal span (§4).
Other length-brackets died at `structure_floor`; the survivors both land ~1 s back. Inside the
surviving bracket the −47 ms start is inside the ±10 s search and is scored; `min` then prefers
−977 ms. Dual-fit cannot rescue a later veto: it already declined this gap for cause.

§4.3: no rival peak at −977 ms (r 0.087 against 0.829 at the truth, 9.6×). Aliasing is refuted.
The defect is the reduction.

**Caveat on the 0.829 / 0.075 pair.** Those are `lag_decision` (1 s templates at `b_mapped`),
not production seam windows. They give the mechanism's shape. Production seams at the applied
placement are 0.437 / 0.437; production dual-fit's best independent post is 0.124. Production
seams *at −47 ms* are not in the baseline ( `fit_terms` is re-scored at the winner only).

---

## Decisions (open — close these in order)

### 1. Fallback vs abstain

Parent named two options that may be the same ranking:

- **Fallback:** when the other shoulder is unmatchable, the waveform term is the trustworthy
  one.
- **Abstain:** the unmatchable shoulder drops out of the `min`, leaving one value.

If both mean "the term equals the good shoulder," they rank identically. They diverge at the
**floor** (decision 4) and in what `fit_terms.waveform_min` reports (the good score, or still
the numeric `min` with a flag). Pick one rule and one report. Do not ship both.

### 2. The "unmatchable" predicate

The search has to apply it **while ranking**, per candidate (or once per gap before the
start-search). Dual-fit's 0.124 is evidence after the fact: `try_dual_fit` runs in
`skip_or_dual_fit`, after the unified winner is already chosen. `lag_decision.peak_r` is not
on this path.

A predicate that can actually run:

> if `min(pre, post)` is dead and `max(pre, post)` is strong, reduce to `max`; else `min`.

That is the one-strong-seam *shape*, inside `waveform_min_at_start` (and the FFT band that
must stay identical to it). Always switching `min` → `max` is a different, broader change:
it would also re-rank gaps where both sides are matchable and one is merely better.

Constants are not guessed here. The live floors already in fit mode are
`fill_absolute_floor` **0.12** and `min_fill_correlation` **0.35**. 31/g7's losing post is
below both; its winning pair is 0.437 / 0.437, so both sides look matchable at the stutter —
the predicate must fire at the *truth*, not at the stutter.

### 3. Waveform only, or structure too?

`combined_structure_score` is a **sum**, not a min (`gap_structure.rs`). The −977 ms slide is
inside the surviving zero-move bracket, so a waveform-only change can move the start without
reviving the 18 `structure_floor` brackets.

Default: **waveform only**. Touching `min_structure_match_score` (0.55) or the structure-floor
rule is in this plan only if a waveform-only change cannot reach −47 ms because that start is
never scored. Confirm from `fit_terms` / a one-pair repair before widening scope. The open
"confirm the production selection rule" item in the parent (§4.3) is a report read of
`fit_terms`, not a new dump.

### 4. The patch must survive the floor

Fit-mode `classify_fill_waveform_confidence` **hard-skips** when `min(pre, post)` is below
`effective_fill_absolute_floor` (default **0.12**). `fit_mode_waveform_floor_passes` is the
same `min`. `short_gap_one_strong_seam_fallback` does not apply.

If production post at −47 ms is in the same ballpark as `lag_decision`'s 0.075, a
reduction-only change **selects the right lag and then skips**. Dual-fit already declined, so
there is no rescue. That is a hole, which this plan's goal forbids.

Two honest outcomes, pick one before coding:

- **Coupled:** the same unmatchable-shoulder rule applies to classification / the hard skip,
  so a one-strong fit may patch (likely `Marginal`). State the seam-step consequence, as the
  normalize plan did.
- **Measure first** (recipe below). If production `post` at the near-nominal winner is
  ≥ 0.12, the floor is not this gap's problem and the classifier stays.

Do not discover a hole at the corpus pass.

**Probe — production seams near −47 ms, no new code.** The dumps cannot answer this:
`fit_terms` is re-scored at the winner, and both surviving 31/g7 brackets landed ~1 s back
(§3.2 / §4). Cap the slide so −977 ms is unreachable and −47 ms is interior, then let
production rank inside that window.

`search_radius_frames` is `fill_border_search_secs × sr` (`patch_audio/region.rs`).
`--fill-border-search-secs 0.20` ⇒ ±200 ms. −47 ms is inside; −977 ms is not.
`--no-dual-fit` so a floor skip is not replaced by dual-fit's independent-shoulder scores
(dual-fit is only reached after a skip; today this gap *patches*, so it never ran).
`--only-gaps 8` is 1-based `#` (dump `g7`). `--patch-only` — no WAV.

```
# A/B from the oriented baseline report (gitignored; do not quote paths in this plan)
$rep = Get-Content gap-files/2026-08-13-repeat-terms/31.json -Raw | ConvertFrom-Json

cargo run --release --features he-aac -p clip-sync-repair --bin clip-sync-repair -- `
  $rep.video_a $rep.video_b --patch-only --format json --only-gaps 8 `
  --fill-border-search-secs 0.20 --no-dual-fit
```

Join the patched/skipped gap to the baseline on `video_a_start_secs`, not on index.
Read `align_adjustment_secs`, `pre_correlation`, `post_correlation`, `fit_terms`, and
either `status.patched` or `status.skipped.correlation_below_threshold` (that skip arm
carries the same pre/post).

| readout | meaning |
|---|---|
| `align_adjustment_secs` ≈ **−0.047** (interior) | probe landed; `post` is the floor number |
| `\|adj\|` within ~10 ms of **0.20** (edge-pinned) | **invalid** — `min` still wanted to walk backward; do not use those seams |
| patched, `min(pre,post) ≥ 0.12` | floor is not the problem; classifier can stay |
| skipped `correlation_below_threshold`, `min < 0.12` | reduction-only S2b ships a hole → **coupled** |
| patched at near-nominal with `min < 0.12` | should not happen; `classify_fill_waveform_confidence` hard-skips there |

Optional second run *without* `--no-dual-fit`: only if the first skips, to see whether
dual-fit would rescue. The dump already predicts it will not (`post_seam_r` 0.124,
`gate_pass: false`). Do not let a rescue on this probe stand in for the unified
classifier — S2b edits the unified term.

### 5. Must-not-move set, and default-on

| gap | expect |
|-----|--------|
| 31/g7 | ≈ −47 ms, still patched |
| 31/g8, g10, g11 | do not move (genuine ≈ −1.11 s) |
| 26/g6 | not required to move |
| 17/g34 | not this class; a move is inspect, not success |

Default **on**. This is a defect in the shipped reduction, not an experiment with an inert
knob. A config flag is acceptable as an emergency off; it is not a reason to ship inert.
No CLI flag unless the corpus pass shows a second population this rule creates.

---

## Change (blocked on 1–5)

In `waveform_min_at_start` / the FFT seam band (they must stay identical):

1. Compute `(pre, post)` as today.
2. Apply the decision-2 predicate; return the decision-1 term instead of `pre.min(post)`
   when it fires.
3. If decision 4 is "coupled," thread the same rule through
   `classify_fill_waveform_confidence` / `fit_mode_waveform_floor_passes` so the winner is
   not hard-skipped. Do not reach for the gate-mode fallback flag.

Unit test: synthetic pair, post border unmatched, pre unique at true lag; assert argmax at
true registration and that the gap patches. A second case with both shoulders matchable at a
far offset must still pick the far offset (31/g8 shape).

`fit_terms.waveform_min` reports whatever the objective used. If that is no longer a min,
the field name is a lie — either keep emitting the numeric min alongside a second term, or
rename in the same PR. Do not leave the reconstruction identity
`fit_breakdown_terms_reconstruct_the_total` describing a min that was not used.

## Exit

Same binary, `--patch-only --format json`, oriented corpus:

1. 31/g7 `align_adjustment_secs` ≈ **−0.047** (tens of ms, not −0.977). Still patched.
2. 31/g8, g10, g11 do not move.
3. Diff `align_adjustment_secs` / `fit_terms` against `2026-08-13-repeat-terms/` over all
   **317** patched gaps. Every mover is listed and classified against the table in
   decision 5.
4. Ear-check the new 31/g7 render only after the JSON says the slide moved.

If 31/g7 moves to −47 ms and then skips, the exit fails — that is decision 4, not a
partial success.
