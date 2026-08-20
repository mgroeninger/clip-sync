# TEMP — 31/g7 is a gap-shape problem (one-sided closeability)

**Status:** findings note, 2026-08-18. **Parallel** to
[TEMP-31g7-model-vs-findings.md](TEMP-31g7-model-vs-findings.md) **rev 2**. Descriptive: it
states where the missing word sits, and (§8) which existing or drafted repairs could take the
cell once classification is correct. It proposes no Change and no cell to ship.

> **RESOLVED same day — the cell premise broke under its own §4 reach caveat (see §9).** The
> post shoulder was found, uniquely, at **−1160 ms** by the ±2000 ms wide sweep. 31/g7 is not
> one-sided closeability (that row now has zero known members); it is the **degenerate
> silence-splice**: both shoulders at own lags, and the donor bridge between them is
> **zero-length** — the gap *is* pair 31's map step. §§0–8 stand as the derivation record;
> §9 carries the probe numbers, the reclassification, and the resolution of §8's pick
> (reasoned skip; the one-seam fills are refuted for this gap). Durable statement:
> [gap-vocabulary.md](../gap-vocabulary.md) § *Known holes*, Hole 2. **Archived 2026-08-18**
> alongside the refuted S2b plan ([TEMP-unmatchable-shoulder-plan.md](TEMP-unmatchable-shoulder-plan.md)).

Rev 2 incorporated this note's criticisms: the repeat-lock / "one period away" story is
removed (refuted by the archived registration plan §4.3), the old §12 extension is withdrawn
to a backlog sketch, and the sibling now points here from its §7.3 (choice vs shape).

Pair 31 / gap 8 (`#` 1-based; dump `g7`; `a_start_secs` 1725.6968125). Counts from the oriented
308-gap join in the sibling §8 (`gap-files/2026-08-13-repeat-terms/` ×
`gap-files/2026-08-13-v0.6.0-oriented/`).

---

## 0. The headline

**31/g7's defect is the shape of the gap: A's two surviving edges do not jointly exist as one
interval of B. Synchronizing both shoulders is ill-posed.** That is a property of the media,
visible in the shoulder sensors before the patcher chooses a fill. The sibling findings note
relocated the missing vocabulary onto "the relationship between the placement chosen and the
registration measured" because it was trying to name what 31/g7 and 26/g6 share. Those two gaps
do not share a shape. Residual can group them; shape cannot.

We can already tell 31/g7 apart. The dual-fit split (`pre_seam_r` 0.966 / `post_seam_r` 0.123,
`gate_pass: false`) is **1 of 308** patched gaps. What we lack is a **cell** those measurements
land in. Production tags are computed at the *chosen* fill, which erases the shape: 31/g7 comes
out `High` / `balanced` / Bracket patch because the stutter's joints are 0.437 / 0.437.

---

## 1. What "shape" means here

Not geometry (how long the silence is). **Closeability:** for each of A's surviving edges — the
pre shoulder and the post shoulder — does that audio exist in B at *that shoulder's own best
lag*?

A fill has to meet **both** edges. If only one is in B, there is no jointly-registered span to
copy. Joint placement is then the wrong problem. Dual-fit's *per-shoulder search* is the right
measurement of that (and already declines to repair, because the dead shoulder never recovers).
The joint gate still returns `High` because `min(pre, post)` would rather degrade the live edge
than admit the dead one. Possible repairs once the cell is named: §8.

This is [TEMP-unmatchable-shoulder-plan.md](TEMP-unmatchable-shoulder-plan.md)'s premise. It is
not a statement about the A↔B map, and it is not a statement about whether the applied slide
agrees with `frac_lag_ms`.

---

## 2. Existing cells vs the unnamed middle

[gap-vocabulary.md](../gap-vocabulary.md) already carves closeability into cells. 31/g7 is the
missing middle:

| Cell | Pre in B? | Post in B? | Action |
|---|---|---|---|
| Bracket patch | yes, same lag | yes, same lag | copy one span — today's normal path |
| Silence-splice | yes, own lag | yes, own lag (disagrees) | dual-fit: independent seams + bridge |
| Program-quiet | n/a | n/a | skip — B silent at the same program time |
| Decorrelated | no | no | skip — B occupied, different content |
| **31/g7 — unnamed** | **yes, unique (−47 ms)** | **no (0.123 even at best lag)** | joint fill is ill-posed; dual-fit declines; E1 still Highs |

Silence-splice requires **both** shoulders to recover. Decorrelated requires **neither**.
One-sided closeability has no cell, so 31/g7 falls into Bracket patch on the applied seams.

[gap-vocabulary.md](../gap-vocabulary.md)'s known-holes paragraph was rewritten 2026-08-18 to state
the split: Hole 1 (the choice axis, 26/g6-class) and Hole 2 (this cell). It is the durable
statement; this note remains the derivation and the repair survey (§8).

---

## 3. Two defects, two altitudes

| | 31/g7 | 26/g6 |
|---|---|---|
| Pre in B | unique landmark, `r` 0.829, `z` 18.3, −47 ms | unique, `r` 0.997, −60 ms |
| Post in B | no peak at nominal; 0.123 at best lag | unique, `r` 0.993, −41 ms; agrees with pre to 19 ms |
| Dual-fit | **fail** 0.966 / 0.123 | **pass** 0.993 / 0.931 |
| Joint registration | **ill-posed** | **well-posed** |
| What went wrong | `min` prefers two mediocre joints; E1 accepts | scoring walked −1.45 s from a landmark both sides agree on |
| Missing word | one-sided closeability (a **gap** cell) | placement vs registration (a **choice**, if anything) |

26/g6 is the opposite gap. We understand its **shape** as well as 31/g7's: both shoulders in B,
they agree, dual-fit passes — the ordinary well-posed cell. The 480.5 ms comb (archived §4.3)
is a lag-surface fact, not closeability, and the true tooth still ranks first. No shape word
will catch a well-posed registration that the patcher ignored. The open question is the
**scorer**: why axis 2's argmax preferred the third tooth. "Does the placement agree with the
shoulders?" is the right missing axis **there**, and only there. S2b does not own it (sibling
§8; S2b probe-result reframe).

---

## 4. We can already tell 31/g7 apart

The shoulder sensors are good. What is coarse is the production vocabulary, because it is
computed at the chosen fill.

| Instrument | What it sees on 31/g7 | Why it is or isn't the cell |
|---|---|---|
| `splice_dualfit` split (`max ≥ 0.9`, `min < 0.20`, `gate_pass: false`) | **1 of 308** — this gap only | Sharp. A production *mirror*, not a named cell. BACKLOG: do not tighten `gate_pass` itself. |
| Lag-verdict one-dead (`timing_offset` + `decorrelated`) | 13 of 308, including 31/g10 (good: post `r = 1.0` at −1152 ms) | Too coarse. "Not found at nominal ±600 ms" ≠ "unmatchable at any lag." |
| `SeamShape` / `High` / Bracket patch | `balanced`, `High` — healthiest cell | Read at the stutter (0.437 / 0.437). Applied joints hide the dead post. |
| S2b `one_strong` (proposed) | Would name 0.83 / 0.075 *at the truth*; would not fire at the stutter | Still a Pearson readout of whatever placement you give it, not a gap cell. |

The isolating fingerprint is closeability (sibling §8), not the lag-verdict pair, and not
`applied − frac_lag`.

**Reach caveat — executed 2026-08-18, and it broke the premise (§9).** "Post recovers nowhere"
was indeed the §7.2 overclaim shape: the ±2000 ms post-shoulder wide sweep found a single
confident landmark at **−1160.3 ms** (r 0.7819, prominence 0.7067), ~513 ms beyond the
pre-chained ±600 ms window. The table row above ("no, 0.123 even at best lag") silently meant
"best *in-reach* lag". Everything below §4 reads as derivation history; the resolution is §9.

---

## 5. Why production tags hide the shape

`classify_seam_shape` (`gap_tags.rs:361`) and `classify_fill_waveform_confidence` take the two
seam Pearsons **of the fill that won**. At 31/g7 that fill is the stutter. Both sides clear 0.27
and differ by < 0.15, so the label is `balanced`. `High` means `min ≥ 0.35`. Neither function
is asked whether A's post audio exists in B.

S2b's proposed `SeamShape::OneStrong` (`min` dead and `max` strong) would name the *truth*
geometry (0.83 / 0.075) and would **not** fire at the applied stutter. The S2b plan already
says the unmatchable predicate must fire at the truth, not at the stutter. A tag on the applied
seams cannot be the gap cell.

Fingerprint `lag_decision` and `splice_dualfit` *are* taken at the shoulders / at each
shoulder's own lag, independent of the fill. That is why they see the shape and `SeamShape`
does not.

---

## 6. Why the sibling note talked about A↔B instead

The sibling needed one word for two unlike failures that share a large backward slide.
`applied − frac_lag` can group them. Shape cannot. So rev 1's §§10–12 treated the missing
vocabulary as **placement provenance** — the relationship between the choice and the
measurement — and proposed `Periodic` / repeat-lock / Misregistered patch whose members are
26/g6 and 31/g7. Rev 2 withdrew the names (§7.5) and splits the hole: choice for 26, shape for
31 (§7.3).

That is the right missing word for 26/g6 and the wrong altitude for 31/g7. It is also Route 2 /
residual-as-class rebuilt as vocabulary: sibling §8 says residual must not become a gap class,
then §12 names a patch-side cell from exactly that quantity.

Worse, the 31/g7 "repeat-lock / one period away" story contradicts settled measurement.
Archived S3 / [TEMP-fill-placement-registration-plan.md](TEMP-fill-placement-registration-plan.md)
§4.3 (±2000 ms pre-shoulder sweep): 31/g7 is a **single spike on a flat floor**; the predicted
rival at −925 ms is `r` 0.087 against 0.829 at the truth. Aliasing is refuted. "31/g7 is not
periodic at all." The 480.5 ms comb belongs to **26/g6**, and even there the true tooth still
ranks first. Envelope `lag_ms = −1000` at 31/g7 is the coarse window's own edge
(`max_lag_blocks = 10`), flagged `donor_registration_unreliable`;
[TEMP-lag-envelope-recenter-plan.md](TEMP-lag-envelope-recenter-plan.md) exists because using
that reading as truth would *lose* today's correct pre −47 ms. Post `0.0752` vs `0.0741` is
`classify_lag`'s `Decorrelated` (`peak < 0.3`), not a period.

Those passages in rev 1 are why this note exists in parallel rather than as an edit of that
§12.

---

## 7. What this does not license

Parked until we decide what moves into vocabulary / S2b / backlog. Explicitly **not** proposed here:

- Shipping a new gap cell, `one_strong`, or a closeability axis.
- Tightening or loosening `splice_dualfit.gate_pass` (still a production mirror).
- Using lag-verdict one-dead (the 13-gap type) as the cell.
- A placement-provenance axis, a 250 ms residual cut, `Periodic`, repeat-lock, or a
  Misregistered-patch cell whose members are 26/g6 and 31/g7.
- Treating 26/g6 as this gap's class, or S2b as already the fix.
- Coding any of §8. Identification is necessary and not sufficient; none of those repairs is
  live in fit, and the −47 ms fill is unmeasured.

What stays true regardless of the rewrite: ~~31/g7 is one-sided closeability~~ (**superseded by
§9** — 31/g7 is a step gap with a zero-length donor bridge); 26/g6 is a healthy registration
the scorer ignored; residual detects bad *patches* and does not classify gaps; ~~S2b is scoped
to the unmatchable-shoulder mechanism~~ (**S2b's premise is refuted by §9**; the shoulder is
matchable, out of reach).

---

## 8. Possible fixes once classification is correct (2026-08-18)

No shipped **fit-mode** path takes this cell. Identifying it would not, by itself, change the
audio: E1 already accepted a `High` at −977 ms, so no rescue runs. A strategy only gets a
chance if that joint `High` is not treated as done.

There is content in B at the hole (`donor_interior_nominal` occupied, continuous). There is no
placement where *both* shoulders are good. Dual-fit was measured independently of the patch
and declined (post 0.123 at its own best lag).

### 8.1 What already exists, and why it misses

**Dual-fit (G6, silence-splice).** After a *scored skip*, fit each shoulder at its own lag,
require both to pass the unchanged floors (`try_dual_fit`: `smin ≥ min_fill_correlation` and
`smin ≥ fill_absolute_floor`), require the step to be real, extract the B bridge between those
two positions, interior-trim to gap length, re-validate with `classify_fill_waveform_confidence`
(another `min`). That is the repair for "the two edges don't share a lag, but each recovers."
31/g7 is not that gap: post never recovers. Dual-fit also only runs after a skip, so the
stutter path never asks it.

**One-strong-seam fallback** (`short_gap_one_strong_seam_fallback`). Default on; the closest
shipped analog to "one edge is enough." **Gate-mode only** — fit ignores it. It is an *accept*
rule on a placement that already exists (short gap, one seam ≥ threshold), not a search that
goes to the live shoulder's lag. At the stutter both seams are 0.437, so it would not fire. At
the truth (0.83 / 0.075) it would *accept*, but nothing in fit searches for that geometry under
this rule.

**Arm B's +36000 bracket** is not a strategy for this cell. It moved A's post border 750 ms
later, off the dead spot, and filled 1.87 s into a 1.12 s gap. Different hole. Not ear-checked.

### 8.2 Drafted: S2b (one-seam joint fill)

[TEMP-unmatchable-shoulder-plan.md](TEMP-unmatchable-shoulder-plan.md) is the intended repair
for this cell: keep a joint fill, but when one shoulder is unmatchable, score (and, if coupled,
floor) on the live one, so the search sits at ≈ −47 ms instead of raising the dead side. Goal:
still patch, do not skip, do not recast the search.

Not coded, and the probe said **do not code it as written**:

- The measured `min` pathology was on **bracket length**, not the start-search site S2b edits.
- E1 still returns `High` at full radius, so a reduction change never sees the alternative.
- The "truth" fill (nominal length at −47 ms) was **never scored**. If post there is ~0.075, a
  reduction-only change picks the right lag and then **hard-skips** (`min` < 0.12). Dual-fit
  already declined, so there is no rescue — the hole S2b's goal forbids. Closing that is
  decision 1(a)/4 (couple the floor), still open.

### 8.3 Two honest actions for the cell

Once identified, two actions reconcile. Neither is live in fit.

1. **One-seam patch** — copy B from the live shoulder's lag, accept that the dead edge will not
   match. That is S2b (plus a floor that does not then skip), or the dual-fit-adjacent branch
   in §8.4. Untested on this gap.
2. **Reasoned skip** — joint fill is ill-posed, dual-fit declined; same action as Decorrelated.
   S2b forbids this because the stutter is ear-confirmed and the goal is to patch.

### 8.4 Adjust dual-fit to handle this case directly?

Economical as a *subroutine*, wrong as a *contract change*.

Dual-fit already performs the measurement that isolates the cell (the 0.966 / 0.123 split).
Using that same search's live lag as the fill origin would not require a second hunt under
`min`. That is the attractive part.

What `try_dual_fit` does *after* the peaks is a different algorithm, and 31/g7 fails it on
purpose:

- **Both floors.** `smin = pre.min(post)` must clear `min_fill_correlation` and
  `fill_absolute_floor`. Loosening that to "one side enough" is not this gap's mark: bare
  `gate_pass: false` is 11 of 308 patched; the isolating split is `max ≥ 0.9` and `min < 0.20`
  (1 of 308). BACKLOG: do not retune `gate_pass` as a discriminator.
- **Two endpoints.** The fill is the B *bridge* `b_pre_seam .. b_post_seam`, then interior-trim
  to `gap_frames`. A dead post has no lag to reconcile to (`b_post_seam` unconstrained). You
  cannot trim a bridge; you copy `gap_frames` of B starting at the live lag. That is a
  one-seam extract that happens to share `seam_local_peak`, not dual-fit.
- **Re-validation is another `min`.** The caller still runs
  `classify_fill_waveform_confidence` on the assembled seams. A one-seam accept inside
  `try_dual_fit` would still skip unless the floor is coupled — S2b decision 4, same hole.
- **It never runs.** Dual-fit is `skip_or_dual_fit`, after a scored skip. E1 `High` means this
  gap never enters it. Folding the cell into dual-fit does not remove the need to divert
  *before* the joint `High` is accepted.

**Do not fold this cell into silence-splice.** If classification is correct first, the
dual-fit-shaped option is a **new branch next to G6**: on the isolating split, one-seam fill
from the live lag. Same open questions as S2b (beat E1; measure the −47 ms fill); different
search (per-shoulder ±600 ms, already run) rather than changing joint `min` over ±10 s.

Silence-splice's action is independent per-seam fit + length reconciliation; this cell's
action (if we patch) is one-seam from the live lag. Those are different cells. Sharing
`seam_local_peak` is fine. Sharing `try_dual_fit`'s accept contract is not. Do not loosen
`gate_pass` to get there.

Parked with the rest of this note. ~~Pick among §8.2 / this branch / skip after the rewrite,
not before.~~ **Picked by measurement — §9.**

---

## 9. Probe result 2026-08-18 — the premise broke; this note is resolved

The §4 reach caveat was executed before fixing the cell's predicate, as it required. Instrument:
offline ±2000 ms per-shoulder sweep (`gap-files/scratch_shoulder_sweep.py`, a reconstruction of
the archived registration plan §4.3 method — 1 s template, mono downmix, sample-domain Pearson,
250 ms peak exclusion), **validated first** by reproducing the archived pre-shoulder table to
the decimal (0.8287 @ −38.1 ms; floor 0.0979 / 0.0974 / 0.0924 / 0.0867). WAVs regenerated via
`--gap-listen --fingerprint-gap 8` (`gap-files/2026-08-18-post-sweep/`, stem `g007`).

Post shoulder, ±2000 ms:

| lag | r | |
|---|---|---|
| **−1160.3 ms** | **0.7819** | single spike; prominence 0.7067 |
| +330.7 ms | 0.0752 | floor top — **verbatim** production's `post_anchor` `peak_r`: the pre-chained window [−647, +553] saw only this |

### 9.1 Reclassification

Both shoulders are uniquely registered at their own lags — pre −47 ms, post −1160 ms — which is
§2's **Silence-splice** row, not the unnamed row. The unnamed cell has **zero known members**.
But it is the *degenerate* silence-splice: the donor bridge between the two matched shoulder
spans is zero-length —

```
bridge = gap + post_lag − pre_lag = 1118.8 − 1160.3 + 38.1 = −3.4 ms   (+6.0 ms via fine pre −47.5)
```

B is continuous across A's 1.12 s: the gap **is** pair 31's map step (31/g10's post landmark
−1152 ms and the −1.15 s slides of gaps #9/#11/#12 are the same step, downstream). The causal
story — A-inserted dead time, a B-side cut, break padding present in A and absent in B — is
**undetermined**; every one of them produces this geometry. Do not assert one.

### 9.2 The §8 pick, resolved by the numbers

- **§8.2 (S2b one-seam fill at −47) and §8.4 (one-seam branch): refuted for this gap.** A copy
  of B from the live lag runs directly into the material A's post shoulder already holds
  (bridge ≈ 0), so a one-seam fill *duplicates program* — the same stutter by a shorter route.
  No single lag serves both seams either: at −47 the seams read 0.83 / ~0.06, at −1160 they read
  ~0.08 / 0.78, and the shipped −977 is `min`'s best compromise (0.437 / 0.437).
- **§8.3 option 2 (reasoned skip) is the honest action**, now with a measured predicate:
  `|gap + post_lag − pre_lag| ≈ 0`, evaluable once the post landmark is in reach.
- Dual-fit with reach finds both seams and then has no interior to trim — its aligned-donor /
  bridge machinery is exactly where the zero-length reading falls out naturally.

### 9.3 What remains open — detection, not vocabulary

Production searches ±600 ms everywhere (dual-fit at each shoulder's own *nominal* edge,
`dual_fit.rs:118`; `lag_decision`'s post window chained on the measured pre lag,
`measure.rs:893`) and measures no bridge length, so this gap read `High`/`balanced` end to end.
The nominal map cannot express the step, so near a step both nominal edges carry the pre side's
registration and the far shoulder is structurally invisible. Work items (Tier-3 wide-sweep
diagnostic + `bridge_ms`, the 13-gap sort, the recenter plan's skip verdict):
[BACKLOG.md](../../../BACKLOG.md) § *Fill placement*. This note is complete.
