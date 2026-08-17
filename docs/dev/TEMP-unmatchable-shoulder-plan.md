# TEMP — Unmatchable-shoulder `min` reduction (S2b)

**Status:** draft, 2026-08-14; code-checked 2026-08-17. Not started.
Decisions **1, 2, 4, 6** open; **3 and 5 closed** by the code read below. Do not code until they close.
Parent: [archive/TEMP-fill-placement-registration-plan.md](archive/TEMP-fill-placement-registration-plan.md)
(§0 S2b, §4.2 min-reduction pathology, §4.3 rival refuted). Findings; do not treat §0 as a work list.

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

## Code read (2026-08-17) — where the reduction actually lives

Three distinct `min(pre, post)` sites, only the first of which is the objective:

| site | what it is | S2b touches it? |
|---|---|---|
| `gap_fill_fit.rs:634` (`unified_fit_score_with_repeat`) | **the ranked waveform term.** Both the naive path (`waveform_seams_at_start`, :1112) and the FFT band (:1230–1247) hand `consider` a `(pre, post)` *pair* and converge here | **yes — the single edit site** |
| `gap_fill_fit.rs:550` (`repeat_penalty_from_correlations`) | the repeat penalty's own `wave_min` branch | decision 6 |
| `gap_fill_fit.rs:181` (`classify_fill_waveform_confidence`), `:1906` (`fit_mode_waveform_floor_passes`) | the Phase-C hard skip / tiering | decision 4 |

`waveform_min_at_start` (:705) is **not** the objective. Its one caller is the band-vs-naive
divergence belt (:1275/:1285), which must stay as it is: it verifies the FFT against an exact
re-score of the raw correlations. An earlier draft of this plan named it (and "the FFT seam
band") as the edit site — that would have changed the belt and left the argmax where it is.

**The objective is one term short of the truth, quantitatively.** Weights are 0.35 structure /
0.65 waveform (`config.rs:454–459`); the applied 31/g7 breakdown reconstructs exactly:
`0.6146 = 0.35 × 0.9448 + 0.65 × 0.4368`, all three penalties 0.0. So

- under today's `min`, the truth needs `structure_combined ≥ ~1.61` to beat the stutter — above
  the stutter's own 0.9448, i.e. unreachable;
- under a `max`-style reduction with pre ≈ 0.8, it needs only `≥ ~0.27`.

That is the falsifiable prediction the decision-4 probe checks, and it is why decision 3 is
waveform-only.

Two non-issues, recorded so they are not relitigated:

- `search_coarse_step` (`gap_structure.rs:304`) is `bin_frames` for a ±10 s span, so −47 ms is
  on the coarse grid and genuinely scored. The coarse/refine split does not hide the truth.
- `fill_anchor_search_prior_weight` defaults to **0.0**, so shrinking `--fill-border-search-secs`
  in the probe does not perturb `AnchorSearchPrior` even though the two share
  `search_radius_frames`.

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

That is the one-strong-seam *shape*, applied at `gap_fill_fit.rs:634` (see §"Code read" — not
in `waveform_min_at_start`, and not in the FFT band, which never reduces the pair).
Always switching `min` → `max` is a different, broader change:
it would also re-rank gaps where both sides are matchable and one is merely better.

Constants are not guessed here. The live floors already in fit mode are
`fill_absolute_floor` **0.12** and `min_fill_correlation` **0.35**. 31/g7's losing post is
below both; its winning pair is 0.437 / 0.437, so both sides look matchable at the stutter —
the predicate must fire at the *truth*, not at the stutter.

### 3. Waveform only, or structure too? — **CLOSED: waveform only**

`combined_structure_score` is a **sum**, not a min (`gap_structure.rs`). The −977 ms slide is
inside the surviving zero-move bracket, so a waveform-only change can move the start without
reviving the 18 `structure_floor` brackets.

**Waveform only.** Closed by the §"Code read" arithmetic: the waveform term alone accounts for
the whole gap between the stutter and the truth (truth needs structure ≥ ~1.61 under `min`,
≥ ~0.27 under `max`), and the coarse grid does score −47 ms. Touching
`min_structure_match_score` (0.55) or the structure-floor rule re-enters this plan only if the
probe shows the winner is still ~1 s back after the reduction changes. The open "confirm the
production selection rule" item in the parent (§4.3) is a report read of `fit_terms`, not a new
dump.

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
  $rep.scan.video_a $rep.scan.video_b --patch-only --format json --only-gaps 8 `
  --fill-border-search-secs 0.20 --no-dual-fit
```

The paths are under `scan`, not at the root. `--only-gaps 8` is verified: this pair has 13
scan gaps and 13 patch gaps, and `patch.gaps[7]` is the `align_adjustment_secs = −0.9774166`
record.

Join the patched/skipped gap to the baseline on **`a_start_secs`** (the patch record's field;
`video_a_start_secs` exists only in the `scan` section), not on index. Read
`align_adjustment_secs`, `pre_correlation`, `post_correlation`, `fit_terms` — including
`repeat_pre`, `repeat_post`, `repeat_penalty` for decision 6 — and either `status.patched` or
`status.skipped.correlation_below_threshold` (that skip arm carries the same pre/post).

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

### 5. Must-not-move set, and default-on — **CLOSED (set confirmed against the baseline)**

| gap | expect |
|-----|--------|
| 31/g7 | ≈ −47 ms, still patched |
| 31/g8, g10, g11 | do not move (genuine ≈ −1.11 s) |
| 26/g6 | not required to move |
| 17/g34 | not this class; a move is inspect, not success |

Baseline `fit_terms` for the three must-not-movers: `structure_combined` 1.714 / 1.766 / 1.740
against `waveform_min` 0.313 / 0.483 / 0.350. Their margin over any rival is carried by
structure, not by the reduction, so a waveform-only change has little purchase on them — the
expectation in the table is now grounded, not assumed.

**g8 and g11 arrived via `anchor_seam_used: true`** with six-figure `gap_start_adjust_frames`
(−129600 / −14400). The exit diff must therefore cover the anchor-rescue path, not only the
plain fit path.

Default **on**. This is a defect in the shipped reduction, not an experiment with an inert
knob. A config flag is acceptable as an emergency off; it is not a reason to ship inert.
No CLI flag unless the corpus pass shows a second population this rule creates.

### 6. Does the rule feed the repeat penalty's `wave_min`? (open)

`repeat_penalty_from_correlations` (`gap_fill_fit.rs:550`) computes its **own**
`wave_min = pre_seam.min(post_seam)` and fires when
`wave_min < REPEAT_SEAM_WEAK (0.45)` **and** `max(repeat_pre, repeat_post) > REPEAT_CORR_THRESHOLD (0.55)`,
costing `repeat_penalty_weight (0.4) × waveform_weight (0.65) × repeat_max` ≥ **0.14**.

An edit confined to `:634` leaves this branch reading the raw min. That is asymmetric against
the fix: the truth candidate (post ≈ 0.075) sits squarely inside the `wave_min < 0.45` arm,
while the −977 ms stutter escapes on the other clause (baseline `repeat_pre/post` 0.249 / 0.207,
both under 0.55). If the truth's repeat correlations are high, a ≥ 0.14 penalty lands on the
candidate S2b is trying to promote and on nothing else — enough to eat the margin the
reduction change buys.

Two options, same shape as decision 4:

- **Uncoupled (default):** `:634` only. The repeat branch keeps the raw min, so a "one strong
  shoulder" candidate is still treated as seam-weak for repeat purposes. Defensible — the
  penalty is about donor self-similarity, not about which shoulder we trust — but it must be a
  stated choice, not an oversight.
- **Coupled:** pass the reduced term into `repeat_penalty_from_correlations` so one rule governs
  both. Broader blast radius: this branch fires on gaps far outside 31/g7.

Decide from the probe, which reports `repeat_pre` / `repeat_post` / `repeat_penalty` at the
near-nominal winner. If `repeat_max` there is < 0.55, the branch never fires at the truth and
**uncoupled** ships with no cost.

---

## Change (blocked on 1, 2, 4, 6)

At `gap_fill_fit.rs:634`, inside `unified_fit_score_with_repeat` — the **one** place the
objective reduces the pair, reached identically by the naive and FFT-band paths:

1. `candidate.wave_pre` / `wave_post` arrive as today (unchanged upstream).
2. Apply the decision-2 predicate; pass the decision-1 term to `unified_fit_score` instead of
   `candidate.wave_pre.min(candidate.wave_post)` when it fires.
3. Per decision 6, decide whether the same term also reaches
   `repeat_penalty_from_correlations` (:550) or whether that keeps the raw min.
4. If decision 4 is "coupled," thread the same rule through
   `classify_fill_waveform_confidence` (:181) / `fit_mode_waveform_floor_passes` (:1906) so the
   winner is not hard-skipped. Do not reach for the gate-mode fallback flag.

**Do not touch `waveform_min_at_start` (:705) or `build_wave_seam_band`.** The band produces
`(pre, post)` pairs, not a min, and `waveform_min_at_start`'s only caller is the divergence belt
at :1275/:1285, whose whole job is to compare the band against an exact naive re-score of the
raw correlations. Both stay byte-identical; the reduction change is invisible to them.

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
4. **Diff the skipped set too, in both directions.** A patched-only diff is blind to the
   population this change can create: if decision 4 lands on "coupled", a gap that today skips
   `correlation_below_threshold` can now patch on one shoulder, and it appears in *neither*
   baseline row nor the mover list. Count and classify every skipped→patched and
   patched→skipped transition; a silent skipped→patched population is a fail, not a bonus.
5. Ear-check the new 31/g7 render only after the JSON says the slide moved. If step 4 found
   any skipped→patched gaps, ear-check a sample of those as well — they are unheard by
   construction.

If 31/g7 moves to −47 ms and then skips, the exit fails — that is decision 4, not a
partial success.
