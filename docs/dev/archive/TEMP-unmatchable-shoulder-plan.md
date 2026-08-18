# TEMP — Unmatchable-shoulder `min` reduction (S2b)

**Status: REFUTED 2026-08-18 — do not code; archived 2026-08-18.** The premise ("the post
shoulder is unmatchable") is dead by direct measurement: a ±2000 ms wide sweep found the post
shoulder **uniquely at −1160 ms** (r 0.782, prominence 0.707) — beyond the ±600 ms reach of
every instrument this plan reasoned from
([TEMP-31g7-gap-shape-vocabulary.md](TEMP-31g7-gap-shape-vocabulary.md) §9). "There is no
placement where both shoulders are good" survives, but for the opposite reason: the gap is a
**step with a zero-length donor bridge** (`gap + post_lag − pre_lag` ≈ 0; B is continuous
across A's 1.12 s), so this plan's goal — *still patch, at ≈ −47 ms* — is the wrong action: a
fill at the live lag duplicates program (the same stutter by a shorter route). The honest
action is a reasoned skip / step verdict. Detection work:
[BACKLOG.md](../../../BACKLOG.md) § *Fill placement*;
[TEMP-lag-envelope-recenter-plan.md](../TEMP-lag-envelope-recenter-plan.md). Open decisions
1(a)/2/4/6 are moot. Everything below is the historical record.

<details><summary>Superseded status (2026-08-14 … 2026-08-17)</summary>

Draft, 2026-08-14; code-checked 2026-08-17. Probe 2026-08-17: reduction defect
CONFIRMED, but on the bracket axis, not the start axis — do not code as written (see "Probe result"
below). Not started.
Decisions **3 and 5 closed** by the code read below; **1(b) closed** — report via
`fit_terms.waveform_term` plus a fifth `SeamShape` (`one_strong`), **not** a tier pin. Still
open: **1(a)** (abstain vs fallback — follows the decision-4 probe), **2**, **4**, **6**.
Do not code until they close.

</details>

Scope note: this plan now touches `gap_tags.rs` and `gap-repair-guide.md` as well as the
objective. That is deliberate — S2b creates the one-strong population, so S2b names it (see
decision 1(b)) — and it is one bucket, not a vocabulary redesign.
Parent: [archive/TEMP-fill-placement-registration-plan.md](TEMP-fill-placement-registration-plan.md)
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

## Probe result 2026-08-17 — mechanism CONFIRMED, but on the wrong axis

Full interpretation against the placement model: [TEMP-31g7-model-vs-findings.md](../TEMP-31g7-model-vs-findings.md).
The *cell* this plan repairs — one-sided closeability, and why it is not 26/g6's defect — is
described in [TEMP-31g7-gap-shape-vocabulary.md](TEMP-31g7-gap-shape-vocabulary.md) §§2–5.
Arm C (±60 ms) is field-for-field identical to arm B — the alternative bracket is a real interior
optimum, radius-invariant, and arm B was not edge-pinned.

### The finding this plan actually sits inside

This plan repairs **one-sided closeability** (31/g7): one live shoulder, one dead, dual-fit
split 0.966 / 0.123 — **1 of 308** patched, not the 13-gap lag-verdict type. See
[TEMP-31g7-gap-shape-vocabulary.md](TEMP-31g7-gap-shape-vocabulary.md) §§2–5 and findings §8.

A seam can score well because the content genuinely continues — or because A's shoulder and B's
fill merely resemble each other (both quiet, both broadband). The gate cannot tell those apart,
because it never looks anywhere except at the joint it is proposing. That independence is real
(pair 50: `waveform_min` 0.52 with no landmark). It is **not** a licence to treat this gap and
26/g6 as one class. The join finds two patches that contradict a landmark (findings §6.7); they
share no gap signature. 26/g6 is well-posed and ignored — a scorer question, not this cell.
This plan does not own it.

That reframes the plan twice over:

- **Scope.** Among residual-measurable High-baseline gaps this unmatchable-shoulder *symptom* is
  **1 of 68**, not the general placement problem. Among all 308 patched, lag-verdict one-dead
  (`timing_offset` + `decorrelated`) is **13**. This gap's mark is `splice_dualfit` 0.966 / 0.123
  with `gate_pass: false` — **1 of 308**. Re-specify against that closeability split (and the
  bracket axis / E1 screen below), not against the 13-gap verdict type. See
  [TEMP-31g7-model-vs-findings.md](../TEMP-31g7-model-vs-findings.md) §8.
- **Instrument.** This plan proposes to change how the `min(pre, post)` seam reduction behaves. The
  seam Pearson is the number that *accepted* the bad placement here (`waveform_min` 0.4368, a
  comfortable `High`), so tuning its reduction is tuning the instrument that failed. Do not plumb
  `lag_decision` / residual as the fix — that is a post-hoc patch check, not this gap's signature
  (findings §8; already a non-goal below).

Arms A and B ran (`gap-files/2026-08-17-s2b-floor-probe/`). Arm A reproduces the 2026-08-13 baseline
bit-for-bit, so the binary has not drifted and arm B is interpretable. Arm B was then checked against
the g007 fingerprint dump (`2026-08-13-v0.6.0-oriented/31/...g007_full.json`), which changes how it
reads.

**Correction to a first reading of arm B.** Arm B's `post_correlation` 0.4213 does *not* show the post
shoulder is alive. Its post border sits **750 ms later** than the baseline's
(`brackets[].post_time_secs` 1727.5656 vs 1726.8156) — a different location in both A and B. At the
**nominal** post border the fingerprint is unambiguous that there is no match:
`lag_decision.post_anchor` `peak_r` **0.0752** against `second_peak_r` **0.0741** (prominence
**0.0011** — no peak at all), `peak_z` 3.83, verdict **`decorrelated`**; `splice_dualfit.post_seam_r`
**0.1235**, `gate_pass: false`. **The unmatchable-shoulder premise stands.** Arm B escaped the dead
shoulder by moving the border off it, not by disproving it.

**The min-reduction pathology is real, and the fingerprint measures it directly.** The oracle swept
both brackets at the same start:

| bracket | seam_pre | seam_post | min | max |
|---|---|---|---|---|
| `move_frames` 0 (applied) | 0.436824 | 0.436924 | **0.436824** | 0.436924 |
| `move_frames` 36000 (arm B's) | 0.478098 | 0.435196 | 0.435196 | **0.478098** |

`min` prefers the applied bracket by **0.001628**; `max` prefers the other by **0.041174** — a 25×
asymmetry, and exactly §4.2's "degrade the good shoulder to raise the minimum". So the reduction
defect is **confirmed by an independent sensor**.

**But it operates on bracket length, not on the start slide.** This plan targets the start-search
argmax and predicts a move to ≈ −47 ms. Arm B's winner moved the **bracket** (+36000 frames = 0.750 s
at 48 kHz, beyond `gap_end_extend_max_ms` 500 ms — it is the anchor bracket move) and landed its start
at **−10.1 ms**, not −47 ms. The fingerprint's pre-shoulder registration is a strong **−47.46 ms**
(`pre_anchor` `peak_r` 0.829, prominence 0.552, `peak_z` 18.3, `timing_offset`), so arm B's start does
not match the truth either. Goal and §4.2 describe the same reduction on two different axes.

**Two further items from the dump — one fact, one retired claim:**

- **Retired 2026-08-18** (findings rev 2 Errata, items 2–3): this section previously read the
  `equivalence_production.donor_registration` peak at `lag_ms` −1000.0 (`peak_r` 0.576) as a second
  sensor independently preferring ~−1 s, and concluded "any fix must beat a rival that a second
  sensor likes." That reading is dead: the −1000 ms value is the coarse envelope window's own edge
  (`max_lag_blocks` 10), self-flagged `donor_registration_unreliable` — not a sensor opinion. The
  archived registration plan §4.3's ±2000 ms waveform sweep shows a single spike at −47 ms on a flat
  floor (the −925 ms repeat rival reads r 0.087). **There is no rival to beat**; the 0.437 seam
  score *is* the defect, and its mechanism is open (findings §6.8). The peak is still in the dump —
  do not re-derive the old conclusion from it.
- **A fill is available at nominal**: `donor_interior_nominal` `silence_fraction` 0.0, `rms_db` −54.0,
  `continuous: true`.

**And the reduction is not the only blocker.** At ±10 s the stutter classifies `High`, so
`patch_region.rs:972` (E1) / `:987` (E2) return before `gate_anchor_search` at `:1006` runs — arm A
has `anchor_seam_used: false`. The +36000 bracket is never offered at full radius no matter how the
pair is reduced. Fixing the reduction alone cannot reach it.

**Status: do not code the change below as written** — its edit site (`gap_fill_fit.rs:634`, the
start-search pair) is not where the measured pathology acts, and decisions 4 and 6 remain open
because the plan's "truth" (a nominal-length fill at ≈ −47 ms) was never measured. Decision 6 is
additionally fragile: `repeat_max` 0.5482 against the 0.55 threshold, margin 0.0018, with `wave_min`
0.4213 already inside the other clause. Re-specify against the bracket axis and the E1 screen, then
re-open.

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
  matchable. 17/g34 is residual-class with 26/g6 (findings §8), not this closeability class.
- Do not plumb `lag_decision` / S1. The production path does not compute it; this plan may not
  grow a dependency on the fingerprint sweep. Residual is a patch diagnostic, not this gap's mark
  ([TEMP-31g7-model-vs-findings.md](../TEMP-31g7-model-vs-findings.md) §8).
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

They rank identically — both yield `max(pre, post)`. There is nothing to choose on ranking.
The choice lives in two places, and one of them is bigger than it looks.

**(a) The floor — decision 1 largely settles decision 4.** *Abstain* is a claim about
**evidence**: the dead shoulder is not a measurement, so nothing downstream may read it. That
carries into `classify_fill_waveform_confidence` by construction — if it is not evidence for
ranking, it is not evidence for the floor. *Fallback* is a claim about the **objective** only
and leaves the floor on the raw min, which means that if the probe returns post < 0.12 at the
truth, 31/g7 selects −47 ms and then hard-skips — the hole the Goal forbids, and decision 4
reopens as "coupled" anyway. Pick abstain unless the probe shows `min ≥ 0.12`.

**(b) The report — a dedicated marker, NOT a tier pin.**

An earlier draft of this decision proposed forcing `FillConfidence::Marginal` whenever the rule
fires, so a one-seam patch is never reported `High`. **Rejected on a code trace (2026-08-17):
`Marginal` is a routing state, not a label.** Recorded so it is not re-proposed:

| consumer | effect of a tier pin |
|---|---|
| `fit_routing.rs:54` `terminates_high` (E1/E3/E6 screen) | a `High` baseline short-circuits at `patch_region.rs:973`; `Marginal` falls through. On 31/g7's own config (`fit_path: baseline_only`, `anchor_seam_mode` default `Auto`) E2 then accepts it without the grid — same candidate, same placement, inert. Under **`FullGrid`** it is not inert: the boundary grid runs, costing time in the phase that is already ~93% of production wall-clock, and the winner comes from `select_joint_fit_winner_with_residual`'s walk — which can hand the gap to a different placement and **undo this fix** |
| `patch_region.rs:584` (`anchor_seam_mode=force`) | explicitly withholds a `Marginal` baseline "so anchor search runs first" ⇒ the gap enters `gate_anchor_search`, the path that gave g8/g11 six-figure bracket moves. Worst combination |
| `patch_anchor.rs:107` | offset-anchor donation requires `High`. Excluding these gaps is *desirable* (it is the "do not feed gap N's slide into gap N+1" hazard) but redundant: `:114` already requires `min_score ≥ fill_anchor_min_correlation`, which a raw 0.075 min fails. Load-bearing only if decision 4 couples what that min means |
| `anchor_retry.rs:90` | Patched+`Marginal` is retried iff `fill_anchor_retry_marginal` — default **false**, inert today; whoever enables it later would silently inherit this new population |
| `patched_marginal_count`, `patch_tier` tag, `gap_listen` manifest, `log.rs:99` | corpus triage keys on `patch_tier`; a tier move shifts counts in every prior comparison, including this plan's own exit diff |

The motivating concern was purely **reporting** — a one-seam patch must not be indistinguishable
from a clean two-seam `High`. So report it directly and leave routing alone:

- `fit_terms.waveform_min` keeps its current meaning (the **raw** numeric min), so exit
  criterion 3's field-by-field diff against `2026-08-13-repeat-terms/` still compares.
- Add `fit_terms.waveform_term` — what the objective actually used — and repoint the
  reconstruction identity `fit_breakdown_terms_reconstruct_the_total` at it. This is the
  answer to the "the field name is a lie" problem below: nothing is renamed, so no baseline
  is invalidated.
- **No new gap tag.** `waveform_term != waveform_min` *is* the record that the predicate
  fired, so the population is already greppable from `fit_terms`. A `waveform_reduction` tag
  would restate a value computed from two others in the same record — a second surface that
  can drift from the first. `gap_tags` is a legitimate derived layer (its module doc:
  "orthogonal facts derived from plan/patch outcomes and seam scores", and
  `anchor_seam_used` / `dual_fit_used` are deliberately emitted in both places), so this is a
  judgement about *this* fact, not about the layer.
- **Tier is computed as it is today.** If one-seam patches later prove to deserve less trust
  in *routing*, that is a deliberate follow-up keyed on the `fit_terms` inequality — not a
  side effect of a reporting choice.

**`seam_shape` needs a fifth bucket, and S2b adds it.**

`classify_seam_shape` (`gap_tags.rs:361`) has no bucket for a one-strong seam. 31/g7 at the
truth (0.83 / 0.075) is not `AsymmetricPre` (`pre < 0.85`), not `Balanced` (`post < 0.27`),
not `SymmetricWeak` (needs *both* < 0.27) — so it falls through to `NotApplicable`. That is
not a neutral "other": the guide defines `not_applicable` as **"structure fail or no scores"**
(`gap-repair-guide.md:380`), and `SeamShape::status_label` returns `None` for it
(`gap_tags.rs:193`), so the gap table prints no seam descriptor at all. The code's fall-through
and the guide's definition already disagree; this closes that too.

**S2b creates the population, so S2b names it.** 31/g7 is `seam_shape: balanced` today
(0.437 / 0.437, matching the baseline); after the fix it would become `not_applicable` — a
patched gap whose seam descriptor *disappears* from the table, indistinguishable from a bug.

Add `SeamShape::OneStrong` (`one_strong`):

> `min(pre, post) < fill_absolute_floor` **and** `max(pre, post) ≥ min_fill_correlation`

— deliberately the **same two constants as the decision-2 predicate**, so the tag and the
objective can never disagree about what "one strong shoulder" means. Wire them from the same
place rather than repeating literals.

**Arm order is load-bearing:** test `OneStrong` **before** `AsymmetricPre` / `AsymmetricPost`.
`asymmetric_*` means both shoulders were measured and one is better; `one_strong` means one
shoulder has no match at all — the more specific fact, so it wins.

**What the tag means.** `one_strong` is a statement about the **seam measurement**, not about
the objective's decision: it will also tag gaps that are correctly *refused* (see the F4 decoy
below), and gaps S2b never touches. Whether the one-strong *rule fired* is recorded separately
by `waveform_term != waveform_min` in `fit_terms`. Do not read either one as the other.

**Measured blast radius (oriented baseline, 317 patched gaps, 2026-08-17).** Projecting the
predicate over the corpus: **zero** gaps re-tag. Today's distribution is `balanced` 275,
`not_applicable` 35, `asymmetric_pre` 6, `asymmetric_post` **1** — the asymmetric buckets are
nearly empty in real media, and nothing in the code branches on them (`AsymmetricPost` has no
consumer beyond `as_str` / `status_label`; the `asymmetric_post_dup` at `gap_fill_fit.rs:557`
is an unrelated repeat-penalty branch). So the new arm cannot disturb the corpus.

**It does move one test, deliberately.** `anchor_seam_oracle.rs:295` characterizes the F4 decoy
as "correlation HARD-SKIP (`min(pre,post) < 0.12`), asymmetric-post shape" — which *is* the
`one_strong` predicate. That assertion becomes `SeamShape::OneStrong`. Update it in the same PR
with a note that the decoy's dead shoulder is now named: it is the bucket being more accurate,
not a regression. Check `diag_anchor_seam` and the C3 fixture assertions for the same flip.

**What this does NOT fix.** 35 of the 317 patched gaps tag `not_applicable` today with ordinary
mid-range seams (0.596/0.872, 0.285/0.460, …) — they fall between `balanced`'s ±0.15 band and
`asymmetric_*`'s 0.85 floor, and land in a bucket the guide defines as "structure fail or no
scores". `one_strong` does not reach them. That 11% remains the redesign's subject.

Also update `gap-repair-guide.md`: the tag-axes row (:288) and the `seam_shape` threshold
table (:373). Corpus tooling that matches on `seam_shape` strings sees a new value.

The deeper re-rooting of this vocabulary stays out of scope — see
[archive/TEMP-gap-vocabulary-redesign-plan.md](TEMP-gap-vocabulary-redesign-plan.md)
§2. This adds one accurate bucket; it does not redesign the axis.

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

Results are saved to `gap-files/2026-08-17-s2b-floor-probe/`, whose `README.md` carries the
question, the binary rev, this argv and the readout table below. Write that file before running:
`gap-files/` is deletable and a probe dir without its own provenance is unattributable — see
`2026-08-14-standoff/`, kept but no longer joinable to any question.

**Arm A is not optional.** The baseline is from 2026-08-13 and commits have landed since; if the
control does not reproduce `align_adjustment_secs = -0.9774166` with seams 0.4368 / 0.4369, the
binary has drifted and arm B's seams answer nothing.

**Arm A must match how the baseline was produced, which is not the obvious command.**
`2026-08-13-repeat-terms/` came from `scripts/measure/measure-patch-outcomes.ps1`, which supplies
`--patch-only --format json` and builds with **`--features he-aac,ac3`** — its default. Building
`--features he-aac` alone drops the AC-3 decoder (`clip-sync/Cargo.toml:15`), so on AC-3 media the
control does not merely differ, it fails to decode. Use `he-aac,ac3` in every arm.

**Compare the gap record, not the pair.** The baseline ran the whole pair; the arms run one gap, so
`patched_count` (5 → 1), `skipped_count`, `not_planned_count` and `donor_relation` will all differ by
construction — `derive_donor_relation` (`gap_tags.rs:89`) pools over every gap carrying a residual
verdict. None of that is drift. Join on `a_start_secs` = 1725.6968125 and compare that record only.

**`--only-gaps` does not perturb the surviving gap** — checked, because arm A would otherwise
confound "binary drifted" with "filtering changed the answer". Two couplings could have done it and
neither is live: cross-gap offset-anchor donation is gated on `fill_offset_mode ==
AnchoredRetry` (`patch_audio/mod.rs:431`) and the default is `Recommended`
(`repair_profile.rs:323`); and `FillRegion` geometry is built per gap from that gap's own scan
fields with no neighbour coupling (`gap_fill.rs:563`). Dropped gaps become `GapNotSelected` skips
and touch nothing else.

**One residual unknown.** The shared `-RepairArgs` of the baseline run was never recorded (pair 31's
manifest row carries no per-pair `extra` — verified, three fields). If arm A mismatches, suspect an
unrecorded baseline arg before concluding the binary drifted.

```powershell
# From the oriented baseline report (gitignored; do not quote paths in this plan)
$rep = Get-Content gap-files/2026-08-13-repeat-terms/31.json -Raw | ConvertFrom-Json
$out = "gap-files/2026-08-17-s2b-floor-probe"

# Build first so cargo's own output can never enter the redirect (report JSON is stdout,
# cli/output.rs:33; build and log noise is stderr).
cargo build --release --features he-aac,ac3 -p clip-sync-repair --bin clip-sync-repair
$bin = "./target/release/clip-sync-repair"

# A — control at the default +/-10 s radius: does this binary still reproduce the baseline?
& $bin $rep.scan.video_a $rep.scan.video_b --patch-only --format json --only-gaps 8 `
  > "$out/a-default.json"

# B — the probe
& $bin $rep.scan.video_a $rep.scan.video_b --patch-only --format json --only-gaps 8 `
  --fill-border-search-secs 0.20 --no-dual-fit `
  > "$out/b-radius0.20.json"

# C — only if B is edge-pinned (see readout): -47 ms is still interior at +/-60 ms
& $bin $rep.scan.video_a $rep.scan.video_b --patch-only --format json --only-gaps 8 `
  --fill-border-search-secs 0.06 --no-dual-fit `
  > "$out/c-radius0.06.json"
```

`search_coarse_step` (`gap_structure.rs:304`) is `bin_frames` at both the +/-10 s and the +/-0.20 s
span, so narrowing the radius does not change grid resolution and the arms stay comparable.

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
| `\|adj\|` within ~10 ms of **0.20** (edge-pinned) | **invalid** — `min` still wanted to walk backward; do not use those seams. Run arm C (±60 ms) |
| patched, `min(pre,post) ≥ 0.12` | floor is not the problem; classifier can stay |
| skipped `correlation_below_threshold`, `min < 0.12` | reduction-only S2b ships a hole → **coupled** |
| patched at near-nominal with `min < 0.12` | should not happen; `classify_fill_waveform_confidence` hard-skips there |
| patched at the truth but **residual-vetoed** | a distinct outcome from a floor skip, and not in an earlier draft of this table. At −47 ms the slide is inside `residual_max_lag_frames` (480) for the first time — the baseline record is `residual_uninformative: beyond_lag_reach` at `placement_slide_frames` 46916 — so `--residual-gate` (default `veto`) becomes live at the truth and inert at the stutter. Record it as its own result; it is not evidence about the floor |

Save the closing numbers into this plan — close decisions 4, 6, 2 and 1(a) inline with the measured
values and the binary rev, as decisions 3 and 5 were closed. The JSON is a convenience; this document
is the durable record.

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

5. Emit the decision-1(b) report: add `fit_terms.waveform_term` (what the objective used).
   `fit_terms.waveform_min` keeps its current meaning — the raw min — and is **not** renamed.
   Repoint `fit_breakdown_terms_reconstruct_the_total` at `waveform_term`, so the identity
   describes the value that was actually summed. **Do not pin the tier** (decision 1(b)).
6. Add `SeamShape::OneStrong` in `gap_tags.rs`, tested **before** the `AsymmetricPre` /
   `AsymmetricPost` arms, on the same two constants the decision-2 predicate uses (share the
   source; do not repeat literals). Give it a `status_label` — `Some("one strong seam")` or
   similar — so it is never a silent cell like `NotApplicable`. Update the two
   `gap-repair-guide.md` tables (:288, :373) and the F4 characterization assertion at
   `anchor_seam_oracle.rs:306` (`AsymmetricPost → OneStrong`, with its comment at :295).

Unit test for the tag: 0.83 / 0.075 ⇒ `one_strong`; 0.90 / 0.05 ⇒ `one_strong`, **not**
`asymmetric_pre` (arm order); 0.437 / 0.437 ⇒ `balanced` (unchanged); 0.90 / 0.40 ⇒
`asymmetric_pre` (unchanged — the weak side is alive).

Unit tests: synthetic pair, post border unmatched, pre unique at true lag; assert argmax at
true registration and that the gap patches. A second case with both shoulders matchable at a
far offset must still pick the far offset (31/g8 shape). A third asserting that a `one_strong`
outcome still classifies through the normal tier path — a routing regression here is the
failure mode decision 1(b) rejects.

## Exit

Same binary, `--patch-only --format json`, oriented corpus:

1. 31/g7 `align_adjustment_secs` ≈ **−0.047** (tens of ms, not −0.977). Still patched.
2. 31/g8, g10, g11 do not move.
3. Diff `align_adjustment_secs` / `fit_terms` / **`patch_tier`** / **`seam_shape`** against
   `2026-08-13-repeat-terms/` over all **317** patched gaps. Every mover is listed and
   classified against the table in decision 5. `patch_tier` is in the diff because decision
   1(b) declines to pin it: tiers are computed exactly as today, so a tier move that is not
   explained by a placement move is evidence of an unintended routing change (the
   `terminates_high` / `FullGrid` path), not a cosmetic difference.

   **`seam_shape` has a measured prediction: the only movers are placement movers.** The
   predicate was projected over these same 317 gaps (decision 1(b)) and re-tags **zero** of
   them, so any gap that changes `seam_shape` with an **unchanged** `align_adjustment_secs`
   contradicts that projection and must be explained before the exit passes — it means the
   predicate is firing wider than measured. 31/g7 is a placement mover; its expected
   transition is `balanced → one_strong`.
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
