# TEMP — Collapsed-bridge census: full-corpus `bridge_ms` re-scan (2026-08-20)

**Status:** findings note, not a Change, 2026-08-20. Extends the `collapsed_bridge` predicate
(`|gap + post_lag − pre_lag| ≈ 0`, [gap-vocabulary.md](gap-vocabulary.md) § *Known holes*, Hole 2)
from its single positive example (31/g7) against a larger population, and re-validates the
14-gap census's three buckets against the current (post-map-coordinates-fix) binary. Prompted by
BACKLOG's "Collapsed-bridge skip verdict" item, which was then designed against n=1 (that item was
re-aimed and renamed **Step-boundary skip verdict** on 2026-08-21 as a result of this note; see Result 7). **This
note proposes no change.** Each open question below needs more measurement or ear-validation
before any of it becomes a Change plan.

**Headline (2026-08-21, Result 7):** the corpus contains **one** collapsed-bridge gap, and the
predicate turns out to be an arithmetic coincidence — `bridge_ms = gap + step`, so `~= 0` fires only
when a pair's map step happens to equal minus the gap's duration. The real invariant is the **step
boundary**. The BACKLOG item should be re-aimed; see Result 7 and Question 8.
**Second headline (2026-08-21, Result 8):** as measured on `2026-08-19-decision-map-oriented` at the then-current search radius, the step boundary was **invisible to those dumps** — the largest trustworthy step in the 935-row corpus is 63.9 ms, and both known map steps (~−1.15 s, ~+2.789 s) are censored, because a step past the search radius destroys the correlation that would measure it. Dump-visible base rate at the 50 ms floor was **zero**. A "one, not zero" correction was raised and withdrawn the same day; see the boxed note in Result 8 before re-raising it.
**Re-measured (2026-08-24):** the null survives with the censor removed.
[TEMP-envelope-3000-corpus-2026-08-24.md](TEMP-envelope-3000-corpus-2026-08-24.md) re-ran the whole
corpus (51 pairs / 1073 gaps) at a flat 3000 ms envelope radius against a same-media 2000 ms arm.
Pair 50's censored cluster becomes clean interior locks at 2761–2816 ms — 11 gaps, dead → admitted,
Arm A 938 → 949 — and **the admitted sequence test is still 0 / 0 from the 50 ms floor up**. So the
figure below should now be read as a measurement, not as an upper bound on a blinded instrument.
Two refinements it forces on this headline's wording: pair 31's ~−1.15 s step was **not**
radius-censored on the current media (the censoring quoted here is specific to the 08-19 dump), and
31/g7's own censor is the `SPLICE_MIN_PEAK_R = 0.85` admission floor, not search radius — see the
boxed note in Result 8. 31/g7 is now the corpus's only within-gap instance as a measured fact.
Parent: [archive/TEMP-lag-decision-map-coordinates-plan.md](archive/TEMP-lag-decision-map-coordinates-plan.md)
(the fix this census re-validates), [archive/TEMP-31g7-gap-shape-vocabulary.md](archive/TEMP-31g7-gap-shape-vocabulary.md)
§9 (origin of the predicate and the original 14-gap census).

Per [gap-vocabulary-redesign / licensed-media-names-never-recorded], gaps are referred to by
corpus pair index + gap index only. Both source dumps are gitignored (`gap-files/`) and may be
deleted; the numbers below are the durable record — see **Reproduction**.

---

## Data sources

- **14-gap wide-sweep census** (pre-map-coordinates-fix), `gap-files/2026-08-18-one-dead-listen/wide-sweep/census.json`.
  Instrument: the wide-window shoulder sweep (then a scratch Python prototype, since retired into
  the `shoulder-sweep` calibration bin), 1 s template, ±2000 ms, non-chained, 250 ms peak
  exclusion. This is the source of the original three buckets: 12 `shared_near_false_dead`, 1
  `collapsed_bridge` (31/g7), 1 `shared_far_offset` (31/g10).
- **Full-corpus post-fix re-scan**, `gap-files/2026-08-19-decision-map-oriented/` — **51 pairs** (the full deduplicated manifest; see Question 10), 971
  gaps carrying `lag_decision` on both shoulders, current binary (map-coordinates search, ±600 ms
  window). Queried ad hoc for this note (not a committed script); see **Reproduction**.
- **Production patch baseline**, `gap-files/2026-08-13-repeat-terms/` — 53 pairs,
  `measure-patch-outcomes` at production defaults (`gap-files/README.md`: "the current patch-report
  baseline"). Source for the [pair-50 note](TEMP-pair50-anchor-seam-outcomes-2026-08-21.md)'s Result 2. Records what production *actually did* with each gap, which the
  `lag_decision` dumps above do not.

**Corrections applied 2026-08-21** from a logic review of Results 5–6 against the original ear
notes and re-runs of the sweep and sliver instruments (the review note has been folded in and
deleted). Claims it overturned are struck in place below rather than deleted, with the reason. New
measurements from that pass — overlap-only remnant correlations for all 6 pair-50 gaps; sweep
re-runs on g8 and g10 including the two-plateau split — are in the [pair-50 note](TEMP-pair50-anchor-seam-outcomes-2026-08-21.md). **This census is the
durable record** for those numbers if `gap-files/` is cleaned.

---

## Result 1 — the 14-gap census's three buckets are confirmed, not just plausible

Re-running the one-dead/both-dead split on the full post-fix corpus:

- **All 12 `shared_near_false_dead` gaps are gone from the dead lists entirely** — 0 of 12 remain
  one-dead or both-dead. Both shoulders now register `timing_offset` in production. This confirms
  the map-coordinates fix resolved exactly the population it claimed to (a sensor artifact:
  walked-off template scored against a gap-edge-anchored haystack, chained by uncorrected
  `frac_lag`), not a real gap-shape class.
- **31/g7** is still one-dead, still shaped as `collapsed_bridge`: pre `timing_offset` (r 0.829,
  −38.1 ms) / post `decorrelated` (r 0.075) at ±600 ms — consistent with the wide sweep's true
  post lock at −1160 ms (`bridge_ms` = −3.4).
- **31/g10** is still flagged, now **both**-dead within ±600 ms (pre r 0.158 @ −539.1, post r
  0.187 @ −444.1) — consistent with `shared_far_offset` (~−1153 ms true, both shoulders agree,
  symmetric, genuinely outside the window).

## Result 2 — the 14-gap list undercounts the corpus: 16 more gaps are one-dead or both-dead

The 14-gap list was never a census of the whole corpus — it was built from a different, earlier
subset. Running the same predicate on all 53 pairs surfaces 16 gaps it never touched, split by
`outcome.tier` (what production actually did with the gap):

**2 new one-dead gaps**

| pair/gap | pre | post | `outcome.tier` |
|---|---|---|---|
| 25/g11 | decorrelated, r 0.209 @ +343.0 | live, r 0.998 @ +337.3 | `skip` (`waveform_floor`) |
| 50/g18 | ambiguous, r 0.306 @ −89.6 | decorrelated, r 0.142 @ +69.9 | `skip` (`structure_floor`) |

**14 both-dead gaps** — both shoulders `decorrelated`/weak within ±600 ms:

| pair/gap | gap_ms | pre (ms, r) | post (ms, r) | `outcome.tier` |
|---|---|---|---|---|
| 31/g8 | 623.8 | 550.8, 0.152 | −94.3, 0.217 | **patch** |
| 31/g9 | 641.5 | −348.5, 0.075 | −387.3, 0.056 | skip (`waveform_floor`) |
| 31/g10 | 1865.4 | −539.1, 0.158 | −444.1, 0.187 | **patch** (`shared_far_offset`, Result 1) |
| 31/g11 | 600.6 | 447.1, 0.075 | −315.8, 0.066 | **patch** |
| 50/g8 | 551.1 | 46.7, 0.221 | 66.6, 0.115 | **patch** |
| 50/g9 | 1867.9 | 77.5, 0.053 | 589.4, 0.046 | **patch** |
| 50/g10 | 2578.0 | −489.6, 0.095 | 452.8, 0.241 | **patch** |
| 50/g11 | 550.6 | −476.5, 0.121 | 563.7, 0.080 | skip (`waveform_floor`) |
| 50/g12 | 553.2 | 41.1, 0.163 | 474.5, 0.099 | skip (`waveform_floor`) |
| 50/g13 | 886.3 | 474.5, 0.090 | −457.0, 0.034 | skip (`waveform_floor`) |
| 50/g14 | 1798.9 | 138.6, 0.113 | 177.0, 0.084 | **patch** |
| 50/g15 | 1832.6 | −554.5, 0.069 | 430.5, 0.106 | **patch** |
| 50/g16 | 1268.8 | −523.2, 0.173 | −249.5, 0.158 | **patch** |
| 50/g17 | 514.0 | 292.9, 0.074 | −506.0, 0.078 | skip (`structure_floor`) |

**9 of these 16 new gaps are `patch`-tier — live production risk, same "both shoulders read
weak/decorrelated in-window yet production patched anyway" shape as 31/g7.** The other 7
(25/g11, 31/g9, 50/g11, g12, g13, g17, plus 50/g18's one-dead case) are already `skip` for
unrelated reasons and are lower priority — production already declined them regardless of what
their true bridge shape is.

Note the true pre/post lags above are still ±600 ms-window reads (`decorrelated`/`ambiguous`
means "no peak found in-window," not "the true lag is known"). The 9 `patch`-tier gaps have since
had the ±2000 ms envelope rescue run against them — see Result 4. The other 7 (5 `skip`-tier
both-dead plus 25/g11 and 50/g18's one-dead cases) have not.

## Result 3 — no near-zero `bridge_ms` hides among the "both shoulders live" gaps

Among the 952 gaps (of 971) where both shoulders register a real in-window lock (954 as first
counted; two of those carry an `ambiguous` shoulder — Question 9 — and both are `skip`-tier), the closest any
`bridge_ms` comes to zero is **505 ms** — nothing between that and true zero. This addresses the
earlier concern that a short gap could collapse its bridge with a lag swing small enough to keep
both shoulders inside ±600 ms and register as an ordinary shared-registration gap: at least in
this 53-pair corpus, that doesn't happen. The real open surface is Result 2's 16 gaps, not this
population.

---

## Result 4 — envelope rescue on the 9 `patch`-tier both-dead gaps (2026-08-20)

Production's own dead-shoulder rescue (`--lag-envelope-max-lag-ms`, default 2000 ms, [gap-fingerprint.md](gap-fingerprint.md) § dump flags) turns out to answer most of Question 1 directly — no separate wide-sweep instrument needed for these. Ran `--gap-listen` against the 9 gaps
(pair 31: gaps 8, 10, 11; pair 50: gaps 8, 9, 10, 14, 15, 16 — ~~1-based~~ **0-based, corrected
2026-08-21**) and read `envelope_rescue` / `envelope_lag_ms` off both shoulders of each:

> **Index convention (corrected 2026-08-21).** Every `g<n>` label in this note is **0-based** and
> indexes `patch.gaps[n]` of the same pair's `2026-08-13-repeat-terms/<pair>.json` directly. Verified
> two ways: Result 2's tiers only line up 0-based (31/g9 is the skip, and `gaps[9]` is the skipped
> row — `gaps[8]` is patched), and the listen stems carry both the index and the timestamp
> (`…_t01-36-04_g010_…` against `gaps[10].a_start_secs` = 5764.27 s = 01:36:04). The exported clips
> are therefore the gaps this note names; the "1-based" parenthetical was wrong but harmless.
> Matches the [pair-50 note](TEMP-pair50-anchor-seam-outcomes-2026-08-21.md)'s explicit "gap
> (0-based)" header. Check this before pulling any row — a defect investigation reading the
> neighbouring gap is worse than none.

| pair/gap | pre `envelope_rescue` | post `envelope_rescue` | reading |
|---|---|---|---|
| 31/g8 | declined (800 ms candidate, r 0.82, failed fine re-sweep) | **rescued**, −1137.7 ms, r 0.94 | ~~asymmetric — strongest new `collapsed_bridge` candidate~~ **Reclassified 2026-08-21 (review), then CONFIRMED by independent sweep (Result 7): `shared_far_offset`, not collapsed-bridge** — pre -1146.1 ms r 0.9896, post -1137.7 ms, agreeing within 8.4 ms. The envelope-first dump locks **both** g8 shoulders at −1137.7 ms (r 0.96–0.98), agreeing with g9/g11 — so the "declined pre / 800 ms candidate" here is likely a wrong coarse peak on the current nominal-first rescue path, not a genuinely unknown or near-zero pre. The ear note independently agrees: the clips are misaligned *from the opening*, which is a far-offset signature, not g7's line-up-then-diverge shape. An +800 ms pre would also predict B **earlier**, the opposite of what was heard. **Confirmed 2026-08-22:** the +800 is `BorderCapture::Narrow`'s envelope argmax (pre `D` 13.0 ms); Wide / first nominal-first-on keep −1146.1 (`D` 1983.8 ms). Not a 26/g6-class floor case — see Result 7's 2026-08-22 correction. |
| 31/g10 | rescued, −1151.6 ms | rescued, −1155.4 ms | symmetric, `bridge_ms` ≈ `gap_ms` (1861.6) — confirms `shared_far_offset`, not collapsed-bridge. |
| 31/g11 | rescued, −1158.4 ms | rescued, −1158.4 ms | symmetric, `bridge_ms` ≈ `gap_ms` (600.6) — same far-offset shape as g10, adjacent gap index. g10/g11 look like one contiguous far-offset region, not independent phenomena. |
| 50/g8, g9, g10, g14, g15, g16 | declined (all 6) | declined (all 6) | every shoulder's coarse envelope peak (r 0.44–0.72) failed fine re-sweep, even at the full ±2000 ms search. Unresolved, not confirmed either way — pair 50's cluster remains open. |

Two consequences:
- ~~**31/g8 is now the priority ear-check** (Question 2) — the only gap in this batch that plausibly repeats 31/g7's exact defect shape.~~ **Superseded 2026-08-21 (review):** per the row above, 31/g8 is most likely `shared_far_offset`. It was ear-checked anyway (Result 5, clean) — but a clean result on a non-member says nothing about collapsed-bridge either way. **This listen batch probably contained no collapsed-bridge case at all; 31/g7 was not among the nine.**
- **31/g10 and g11 are resolved out of the collapsed-bridge question** — same-offset, both-shoulders-agree far offset, not a collapsed bridge. They still may be live production risk (both `patch`-tier), but that's a `shared_far_offset` question, not this one.
- ~~**Pair 50's 6 gaps need a different approach**, not just a wider window — 2000 ms already exceeds what rescued 31's gaps, and it still found nothing durable. Possible next moves: check whether these regions are structurally different content (cuts/edits between A/B versions) rather than a lag problem at all.~~ **Struck 2026-08-21 (Result 6).** A wider window is *exactly* what was needed: the true peak is at +2.8 s and the ±3000 ms sweep found it. The "structurally different content / cuts-and-edits" speculation is refuted — the content matches at r ≈ 1.0 once you search far enough. This is an envelope-sizing question ([archive/TEMP-envelope-adaptive-widen-plan.md](archive/TEMP-envelope-adaptive-widen-plan.md), which already said pair 50 needs ~3 s), not a content question.

## Result 5 — ear-check of the 9 `patch`-tier gaps (user listen, 2026-08-20/21)

All 9 clips (`_a_surround`/`_b_surround`/`_a_patched` from the Result 4 run) were listened to by ear.
Headline: **both shoulders reading dead inside ±600 ms does not reliably predict an audible defect** —
7 of 9 patches sound clean despite grossly misregistered or fully decorrelated shoulders.

**Scope of that headline (narrowed 2026-08-21 by review).** The 9 gaps are mostly far-offset /
declined-envelope cases; per Result 4's reclassified 31/g8 row, **none of them is a confirmed
collapsed-bridge gap**, and 31/g7 (the only ear-confirmed positive) was not in the batch. So this
result undercuts using *both-dead-in-window* or *envelope-declined* as a defect predictor. It is **not**
a test of the collapsed-bridge shape and does not undercut a collapsed-bridge detector — that
predicate remains untested at n=1, neither confirmed nor refuted here.

Also note the ear notes score *audible acceptability*, not correctness. "Sounds good" ≠ "the fill is
the right material" — 50/g8's own note says as much ("does not mean it is correct… mixed voices
talking over each other"). Several "clean" pair-50 patches have A/B off by seconds on the listen clips.
Those pair-50 "B begins 1–4 s earlier" notes (g8 ~4 s, g14 >1 s, g15 3–4 s, g16 2–3 s) **are** the
already-known +2.7891 s offset ([archive/TEMP-fill-placement-registration-plan.md](archive/TEMP-fill-placement-registration-plan.md)
consensus; [archive/TEMP-envelope-adaptive-widen-plan.md](archive/TEMP-envelope-adaptive-widen-plan.md) already said
pair 50 needs ~3 s), not a separate phenomenon. `--gap-listen` cuts `_a_surround` and `_b_surround`
around the same nominal span, so the two clips are *supposed* to line up; if they don't, the map is
off by that amount. Sign matches: `frac_lag_ms` positive means the match sits later in B, so B's clip
starts with earlier program than A's — "B begins earlier." Ear estimates that coarse, on overlapping
dialogue, are expected to wander around 2.8 s.

| pair/gap | shoulder observation | patch result |
|---|---|---|
| 31/g8 | B clip starts later than A (visible misalignment) | sounds good |
| 31/g10 | (Result 4: symmetric far-offset, both rescued ~−1150 ms) | **audible defect** — small mix error/blip at the end of the patch |
| 31/g11 | (Result 4: same far-offset shape as g10) | sounds pretty good |
| 50/g8 | B begins ~4 s earlier than A | sounds pretty good (mixed/overlapping voices — hard to judge) |
| 50/g9 | — | sounds very good |
| 50/g10 | gap starts in silence — user's own hypothesis: pre shoulder has nothing to correlate against | **audible defect** — slight echo at the post |
| 50/g14 | B begins >1 s before A | sounds very good |
| 50/g15 | B begins 3–4 s before A | sounds very good |
| 50/g16 | B begins 2–3 s before A | sounds very good |

Two confirmed defects. ~~Two different mechanisms~~ — **the two-mechanism split is over-clean
(review, 2026-08-21); neither defect has a confirmed mechanism yet:**

- **31/g10** — audible blip, *compatible* with the already-known `shared_far_offset` model (Result 1),
  but not confirmed by it: **31/g11 has the same Result 4 shape** (both shoulders rescued ~−1150 ms,
  `bridge_ms` ≈ `gap_ms`) and sounds fine. A model that predicts a defect from that shape predicts g11
  wrongly. Result 5 spots this same-shape/different-outcome trap for g7/g8 and misses it here.
  **Owned by Question 11 since 2026-08-21** — until then this defect was discussed here and tracked
  nowhere. 31/g8 is a third same-shape clean case, and production's patch record ranks g10 the
  *cleanest* of the three; see that question.
- **50/g10** — audible echo. The **silent-onset hypothesis is withdrawn** (review + [pair-50 note](TEMP-pair50-anchor-seam-outcomes-2026-08-21.md)): it was
  n=1 inside the +2.8 s cluster; the envelope guesses that motivated it (pre −1600 ms, post −1800 ms)
  are the *wrong sign* against the true +2.789 s offset; all six pair-50 gaps declined, so "declined"
  cannot single g10 out; and the post-onset remnant correlates at **r = 1.000** ([pair-50 note](TEMP-pair50-anchor-seam-outcomes-2026-08-21.md)), which
  directly contradicts "no B content at the post." The pre-shoulder is also not starved — at
  `t = 2–3 s` it reads r ≈ 0.99 at +2.8 s. The remnant's 13.2 ms lag is a possible seam residual
  (same size as g10's first/second-plateau split), not yet the echo mechanism. The retargeted
  question — why is this patch's **post join** audible when the post-onset content matches at
  r = 1? — moved to the [pair-50 note](TEMP-pair50-anchor-seam-outcomes-2026-08-21.md), Question 1.
- ~~**31/g8** did not reproduce 31/g7's audible defect despite reproducing its numeric shape — the
  clearest single data point against treating the collapsed-bridge shape as a reliable defect
  predictor.~~ **Withdrawn 2026-08-21 (review; confirmed by measurement, Result 7).** 31/g8 is `shared_far_offset`, not a
  collapsed-bridge gap (Result 4's reclassified row), so it is not a collapsed-bridge counterexample
  at all — the load-bearing claim here rested on a mis-assigned class. This does not make the
  collapsed-bridge shape a good predictor; it means this batch never tested it.

## Result 6 — pair 50's cluster is one ~2.8 s map step, and production corrects it (2026-08-21)

**Split out 2026-08-21 to [TEMP-pair50-anchor-seam-outcomes-2026-08-21.md](TEMP-pair50-anchor-seam-outcomes-2026-08-21.md).**
That evidence — a sliding registration sweep over all 6 `patch`-tier gaps, the sliver correlations, the
listen-clip vs production-haystack distinction, and the production patch baseline — answers nothing
about `|gap + post_lag − pre_lag| ≈ 0` and had grown to roughly a third of this census. What this
census needs from it is two sentences:

- Pair 50's six `patch`-tier gaps (g8–g10, g14–g16) share **one pair-level map step of ~2.8 s**, and
  the matching B content is demonstrably present (r ≥ 0.991 on the true overlap). Their
  `envelope_rescue: declined` has a single sufficient cause — **2800 ms exceeds the ±2000 ms search
  radius** — so it carries no defect information. Not a content problem, not a window-length problem,
  and *not* a listen-clip margin problem (that overrun is real but affects only the export).
- All six nonetheless **patch at the correct offset in production**, via the editorial anchor-seam
  bracket search (`max_anchor_bracket_secs`, default 5.0 s) — a mechanism independent of
  `anchored-retry`'s cross-gap triangulation. The cluster is not stuck.

Both of this census's pair-50 follow-ups moved with that evidence: 50/g10's echo and the 50/g11–g13
skip anomaly are now Questions 1 and 2 of the split note.

## Result 7 — 31/g8 confirmed `shared_far_offset`, and the predicate is an arithmetic coincidence (2026-08-21)

Question 4 answered by independent measurement. The disagreement was between two *production* paths
(the envelope-first dump reading both g8 shoulders at -1137.7 ms; the nominal-first path declining the
pre with an "+800 ms candidate, r 0.82"), so neither could arbitrate the other. Ran
the wide-window shoulder sweep (now `cargo run --features calibration --bin shoulder-sweep`) —
sample-domain Pearson, 1 s template, +/-2000 ms, 250 ms exclusion — against
the existing `2026-08-20-collapsed-bridge-listen/31` clips. No new media run.

**Instrument validated first** on 31/g7's pre, per the script's own docstring: peak **0.8287 @ -38.1 ms**,
floor 0.0979 — reproducing the archived section 4.3 table exactly.

**31/g8 result:**

| shoulder | top peak | r | floor (outside +/-250 ms) | prominence |
|---|---|---|---|---|
| pre | **-1146.1 ms** | **0.9896** | 0.1911 | 0.7985 |
| post | **-1137.7 ms** | **0.9362** | 0.2171 | 0.7191 |

Both shoulders lock hard and **agree within 8.4 ms**. `bridge_ms` = 623.8 + (-1137.7) - (-1146.1) =
**+632.2 ms ~= `gap_ms` (623.8)** — the `shared_far_offset` signature, not `|bridge_ms| ~= 0`.
**31/g8 is confirmed `shared_far_offset` and leaves the collapsed-bridge population.** Question 8 stays
at n=1, now by measurement rather than by inference.

The "+800 ms candidate at r 0.82" is confirmed spurious *as a waveform lock*: at +679.8 ms the
waveform sweep reads **r 0.1777**, ordinary floor. ~~An envelope-domain r of 0.82 coexisting with a
waveform r of 0.18 is the known coarse-envelope false-peak mode (`26/g6`,
[archive/TEMP-lag-envelope-recenter-plan.md](archive/TEMP-lag-envelope-recenter-plan.md)
Result). **This is a second real instance of it**, and unlike 26/g6 it is not caught by
nominal-first ordering — g8's shoulders are genuinely dead at nominal (r -0.02), so the rescue
branch is entered legitimately and then picks the wrong peak. Handed to
[archive/TEMP-envelope-adaptive-widen-plan.md](archive/TEMP-envelope-adaptive-widen-plan.md), which owns the
rescue-branch guard.~~
**Withdrawn 2026-08-22 as a 26/g6-class / floor handoff.** HEAD
`gap-files/2026-08-22-31g8-capture` (default `BorderCapture::Narrow`) reproduces the +800 / r 0.816
decline with pre `D` = **13.0 ms**. Envelope-first and the first `2026-08-20-nominal-first-on`
dump keep −1146.1 / 0.974 with pre `D` = **1983.8 ms** — the shared wide capture's 12% trim ate
~2 s of the gap-facing pre end. The first nominal-first-on dump still rescued; `decoupled-unconditioned`
(lazy-narrow) is the split, bit-identical to HEAD. WAV −1146.1 on throat-adjacent clips still
stands. **Not a second in-branch false peak; 31/g8 left the widen plan** (that plan's deciding
measurement 2). BACKLOG owns the Narrow-capture miss.

### What this does to the predicate

Pair 31 carries a **pair-level map step of ~-1.15 s**, exactly like pair 50's +2.8 s. The far-offset
gaps are its *interior*: g8 (-1146/-1138), g10 (-1152/-1155), g11 (-1158/-1158) all sit past the step
with both shoulders agreeing. **31/g7 is the step's boundary** — its pre is still at the old
registration (-38.1 ms) and its post is already at the new one (-1160 ms). Same step, different side
of it.

So `collapsed_bridge` is not a distinct gap *class*. It is what a **step-boundary gap** looks like when
one number happens to match another:

| | 31/g7 |
|---|---|
| step size (`post - pre`) | -1121.9 ms |
| gap duration | 1118.8 ms |
| difference | **3.1 ms** |

`bridge_ms = gap_ms + post_lag - pre_lag`, and at a boundary `post_lag - pre_lag` *is* the step. So
`bridge_ms ~= 0` exactly when **step ~= -gap_duration**. 31/g7 satisfies that to 3.1 ms — a coincidence
of this gap's duration against this pair's step size, not a property of the shape.

Two consequences, both testable and neither yet tested:

- **A positive step can never produce `|bridge_ms| ~= 0`.** `bridge = gap + step` with `gap > 0`, so a
  positive step drives it *away* from zero. Pair 50's +2.789 s step therefore cannot yield a
  collapsed-bridge gap at any gap duration — consistent with Result 3 finding nothing near zero.
  A detector on `|bridge_ms| <= 50 ms` is structurally blind to every positive-step boundary.
- **The detectable thing is the step boundary, not the collapsed bridge.** A boundary gap is
  "pre and post disagree by ~= the pair's step size" — which needs no coincidence and covers both signs.
  `|bridge_ms| ~= 0` is a lossy proxy that fires on the subset where step ~= -gap.

This is the census's most consequential finding for the BACKLOG item, and it argues the item is aimed
at the wrong invariant. It rests on one boundary gap; see the revised Question 8.

## Result 8 — the dump-visible step-boundary base rate is **zero** (2026-08-21)

Question 8's replacement question — *how common is the step boundary?* — measured, using the
**shipped** `gap-fingerprint-stats` with no new code. `GAP_FP_CSV=1` over
`2026-08-19-decision-map-oriented` writes `target/gap_fingerprint_corpus.csv`; the boundary rule
from [TEMP-step-boundary-census-plan.md](archive/TEMP-step-boundary-census-plan.md) §4–§5 was then applied
offline to that CSV.

**Admission (Arm A) is an upper bound.** Rows kept: `kind == matched` and
`min(peak_r_pre, peak_r_post) ≥ 0.85` (`SPLICE_MIN_PEAK_R`). The CSV carries no `edge_pinned` and
no `peak_z`/`uniqueness_prom`, so the real `splice_diag()` gate would admit *fewer* rows, and gaps
were ordered by gap index rather than `a_start_secs`. Both simplifications can only inflate the
counts below — which matters, because the counts are a null.

| | |
|---|---|
| matched rows | 997 |
| Arm A admitted | **935** |
| Arm B censored | 62 (36 dead-shoulder, 26 no lag pair) |
| admitted rows with \|step\| > 100 ms | **0** |
| admitted rows with \|step\| > 50 ms | 2 (63.9 ms, 62.6 ms) |
| largest admitted step in the corpus | **63.9 ms** (19/g2) |

**The whole corpus contains no trustworthy step above 64 ms.** Pair 31's ~−1.15 s and pair 50's
~+2.789 s are the only known map steps, and neither is visible here — both are censored (below).

**Boundary yield vs the "large step" floor**, `6 × MAD` of the pair's own steps floored at the
named value:

| floor | pairs with ≥1 boundary | gaps at a boundary | step magnitudes |
|---|---|---|---|
| 2 ms (`CLEAN_STEP_MS`) | 18 of 50 | 29 | 2–41 ms |
| 10 ms | 9 | 11 | 17–41 ms |
| 25 ms | 6 | 6 | 25–41 ms |
| **50 ms** | **0** | **0** | — |
| 100 / 250 ms | 0 | 0 | — |

The 18-pair reading is an artifact: `6 × MAD` collapses to the 2 ms floor on most pairs (their
steps are all ~0), so what it counts is millisecond seam wobble, three orders of magnitude below
the phenomenon. There is no plateau between 2 ms and 50 ms to pick a defensible threshold from.

**Both known boundaries are censored, as predicted.** Pair 31 admits **6 of 12** matched rows —
g7, the actual boundary, is censored at post r 0.0752, and g8–g11 with it. Pair 50 admits **7 of
19**; the entire g8–g17 cluster is censored.

> **The censoring is now demonstrated, not inferred (2026-08-23).** A flat 3000 ms envelope
> re-fingerprint of pairs 31 and 50 —
> [TEMP-envelope-3000-smoke-2026-08-23.md](TEMP-envelope-3000-smoke-2026-08-23.md) — re-measured
> pair 50's named g8–g17 cluster **and** g18 (ambiguous/declined at 2000 ms, not in that named
> set). Those 22 shoulders go `decorrelated`/`declined` → `rescued`/`timing_offset` at
> **2761–2816 ms**, r 0.93–1.00, none edge-pinned; the pair's one edge pin (g009 post,
> `envelope_peak_r` 0.473 at +2000) becomes an interior lock at +2800 / r 0.997. The mechanism this
> result asserts — *a step past the search radius kills the correlation that would measure it* —
> is therefore confirmed by moving the radius, **for pair 50**. What the widen does **not** change
> is Result 8's conclusion for that pair: the step falls *between* g7 and g8 (post −48.7 / pre
> +2767.1; intra-gap `splice.step_ms` stays in the tens of ms), so the widen supplies the sequence
> test's flanks and still no straddling gap *on that pair*.
>
> **Re-measured corpus-wide, 2026-08-24** —
> [TEMP-envelope-3000-corpus-2026-08-24.md](TEMP-envelope-3000-corpus-2026-08-24.md). The flat 3000
> ms recipe ran over the whole corpus (51 pairs / 1073 gaps) against a same-media 2000 ms arm one
> knob apart. Pair 50's radius censor is removed — 11 gaps, 22 shoulders, dead → admitted, Arm A
> 938 → 949 — and **the admitted sequence test is still 0 / 0 from the 50 ms floor up**. So this
> null is no longer an upper bound taken on a blinded instrument: the blind spot was opened, 11 gaps
> walked through it, and the count did not move. Zero `envelope_edge_pinned` rows remain, so nothing
> in the corpus saturates 3000 ms either.
>
> This result's "censored by search radius" mechanism holds for pair 50 and must **not** be
> generalised to pair 31, which carries three separate censors that this box previously ran
> together:
>
> - **Pair 31's map step** was never radius-censored on the current media. g9–g11 read −1142.6 …
>   −1158.4 at `peak_r` 0.965–0.995 in *both* arms. The censoring quoted above is specific to
>   `2026-08-19-decision-map-oriented`, a different instrument.
> - **31/g7 as a straddler** stays in Arm B at *any* radius. Its two shoulders are −38.07 / 0.8287
>   and −1160.25 / 0.7820 — both lags correct, both `timing_offset`, both under
>   `SPLICE_MIN_PEAK_R = 0.85` by 0.021 and 0.068. It is an **admission-floor** censor, not a reach
>   censor, and widening can never admit it.
> - **31/g8 pre** (`peak_r` 0.152 at +550.8, envelope +800 / 0.816, `declined`) is neither: it is the
>   `BorderCapture::Narrow` pre miss BACKLOG tracks in its own row.
>
> Quote this null as *zero on the admitted sequence test, with pair 50's censor removed* — not as
> "the dump cannot see pair 31." It can; the rule declines to count it.

**The censoring is load-bearing, not conservative.** The in-window steps of those censored rows —
50/g11 **+1040.3 ms**, 50/g15 +985.0, 50/g10 +942.5, 31/g11 −762.9, 31/g8 −645.1 — are noise peaks
subtracted from noise peaks (r 0.03–0.24). They are larger than anything real in the admitted set.
Had they been admitted, pair 50 would have reported a multi-step staircase built entirely from
noise, and it would have outranked every genuine measurement in the corpus.

**Conclusion.** On that dump, the dump-visible step-boundary base rate was **zero**, on a 935-row
denominator that is an upper bound. The step boundary is real (pairs 31 and 50, measured by WAV
sweep in Result 7) but was **invisible to `lag_decision` dumps at that radius**: a step past the
search radius kills the shoulder correlation that would have measured it. **Bounded 2026-08-23:**
that zero is an upper bound on *this instrument at ±2000 ms*, not evidence that large steps are
rare — see the box above and [TEMP-envelope-3000-smoke-2026-08-23.md](TEMP-envelope-3000-smoke-2026-08-23.md).

> **A "correction" to this null was proposed and withdrawn on 2026-08-21 — read this before
> proposing it again.** A re-derivation from the same CSV appeared to show one surviving gap at the
> 50 ms floor (19/g2, a `patch`-tier +63.9 ms step with the pair's level at 6.3 ms before and ~60 ms
> after) and concluded the table's 50 ms row of `0` was inconsistent with the "2 rows above 50 ms"
> line above it. **That was wrong.** The re-derivation applied only the *step* half of the boundary
> rule (`|step| > max(CLEAN_STEP_MS, 6 × MAD)`) and silently dropped the sequence half that
> [TEMP-step-boundary-census-plan.md](archive/TEMP-step-boundary-census-plan.md) §4–§5 specifies:
> **≥2 admitted gaps settled on each side**, "settled" meaning within `max(CLEAN_STEP_MS, 2 × MAD)`
> of that side's mean. 19/g2 fails it twice — only **one** admitted gap precedes it, and its two
> following levels are 80.4 and 49.2 ms, a 31 ms spread against a 2.0 ms settle window. It is a
> lone excursion in a wandering pair, which is exactly the case that rule exists to reject.
>
> Re-running the archived rule in full reproduces this table: 17 pairs / 28 gaps at the 2 ms floor,
> 8/10 at 10 ms, 6/6 at 25 ms, **0/0 at 50 ms and above** (the ±1 against the numbers above is
> `a_start_secs` vs gap-index ordering). The two rows above 50 ms are candidates that fail the
> sequence test — the table and the prose were never in conflict. **The null stands at zero.**

Two notes for whoever picks this up:

- `uniqueness_margin < 0.30` (`LOW_UNIQUENESS_MARGIN`) covers **305 of the 935** admitted rows. A
  strict `splice_diag() == Splice` admission therefore censors a third of Arm A as `AliasSuspect`.
  Immaterial to this null, but it would matter to any future distribution over admitted steps.
- Reproduction needs no media: `GAP_FP_CSV=1 cargo run -p clip-sync-repair-harness --features
  calibration --bin gap-fingerprint-stats -- gap-files/2026-08-19-decision-map-oriented`, then the
  filter and rule above over the CSV. Re-derivable in minutes if `gap-files/` is cleaned.
- **The rule to apply over that CSV is the archived one, in full** — see
  [TEMP-step-boundary-census-plan.md](archive/TEMP-step-boundary-census-plan.md) §4–§5. Admit
  `kind == matched` and `min(peak_r_pre, peak_r_post) >= 0.85`; per pair take
  `step = frac_lag_post_ms - frac_lag_pre_ms` and `level = seam_mid_ms`; flag a gap when
  `|step| > max(CLEAN_STEP_MS, 6 * MAD(pair's steps))` **and** at least 2 admitted gaps on each side
  are settled, i.e. within `max(CLEAN_STEP_MS, 2 * MAD)` of their side's mean; refuse a verdict for
  pairs with <5 admitted gaps. Dropping the sequence half of that rule is what produced the withdrawn
  "one, not zero" correction — do not re-derive it in isolation. Ordering here is by gap index, the
  same simplification Arm A made (`a_start_secs` ordering moves the 2 ms floor by one gap).

## Follow-up questions (open, prioritized)

| # | Question | Why it matters | Priority |
|---|---|---|---|
| 1 | ~~Wide-sweep the 9 `patch`-tier both-dead gaps~~ **done 2026-08-20 via production's own `--lag-envelope-max-lag-ms` rescue** (see Result 4); pair 50's "declined" outcome fully explained by Result 6 — the 2.8 s true offset exceeds the 600 ms nominal and 2000 ms envelope search radii. **The production explanation is the search radius alone**, not clip overrun (which is a listen-export artifact) and not the 1000 ms window length. | — | Done |
| 2 | ~~Ear-check the 9 `patch`-tier gaps~~ **done 2026-08-20/21** (see Result 5) — only 2 of 9 have an audible defect. **Scope narrowed 2026-08-21 (review):** the supported claim is that *both-dead-in-window / envelope-declined* does not predict a defect. The batch contained no confirmed collapsed-bridge gap (31/g8 reclassified as `shared_far_offset`), so it does **not** undercut a collapsed-bridge detector — that predicate was never tested here. | Undercuts a detector built from the `lag_decision`/envelope shape, at n=9. Does **not** speak to `|bridge_ms| ≈ 0`. | Done |
| 3 | ~~Investigate 50/g10's echo directly~~ **Moved 2026-08-21 to [pair-50 note](TEMP-pair50-anchor-seam-outcomes-2026-08-21.md), Question 1.** It is a patch-quality question about a gap that patches at the *correct* offset, not a `bridge_ms` question — and both models this census raised for it (silent-onset, unreached region) are dead. | — | Moved |
| 4 | ~~Confirm 31/g8's pre-shoulder is the -1137.7 ms lock~~ **Answered 2026-08-21 by independent sweep - see Result 7.** Pre locks at **-1146.1 ms, r 0.9896** (prominence 0.799), post at -1137.7 ms; shoulders agree within 8.4 ms, `bridge_ms` ~= `gap_ms`. **31/g8 is `shared_far_offset`** and leaves the collapsed-bridge population. ~~The nominal-first path's "+800 ms candidate, r 0.82" is a coarse-envelope false peak (waveform r there is 0.1777) - handed to the envelope plan.~~ **Corrected 2026-08-22:** the +800 is the Narrow-capture envelope argmax (`D` 13 vs Wide 1984); 31/g8 left the widen plan. BACKLOG owns the capture miss. | Settled as `shared_far_offset`. The +800 is not a floor instance. | Done |
| 5 | ~~Is `envelope_rescue: declined` worth treating as a warning signal for pair 50~~ **answered by Result 6: no** — decline is structurally expected for the whole cluster (true offset exceeds both search radii), so it carries no defect information on its own; 5 of 6 patched cleanly anyway. | — | Done |
| 6 | Is pair 50's cluster (g8–g10, g14–g16) one map-step rather than six independent gaps? | **Answered — recorded, not "confirmed contiguous."** Same ~2.8 s offset on all 6: yes. *Contiguous*: **no** — `a_start_secs` = 2494, 2627, 2711, 3424, 3544, 3777, spanning ~21 min with `skip`-tier gaps interleaved. It is a persistent pair-level step. The coordinate check dismissed as "adding nothing" would in fact have shown this. | Done |
| 7 | Wide-sweep the remaining `skip`-tier both-dead gaps (31/g9; 50/g17) and 25/g11's coordinate-fix status. **50/g11–g13 moved 2026-08-21** to [pair-50 note](TEMP-pair50-anchor-seam-outcomes-2026-08-21.md) (Question 2) — they are inside pair 50's map-step region, flanked by correctly-resolved gaps, so their failure is an anchor-seam question, not a census gap. | Lower risk (already declined for unrelated reasons) but needed for a complete census before finalizing any detector. | Low |
| 8 | ~~Once 2-3 confirmed `collapsed_bridge` cases exist, revisit the 50 ms `bridge_ms` threshold.~~ **Re-aimed 2026-08-21 by Result 7 - the threshold is the wrong question.** `collapsed_bridge` is a **step-boundary gap** whose step size happens to equal its gap duration (31/g7: step -1121.9 ms vs gap 1118.8 ms, differing by 3.1 ms). `bridge_ms = gap + step`, so `~= 0` requires a *negative* step of exactly that magnitude, and **no positive step can ever satisfy it** (pair 50's +2.789 s cannot, at any gap duration). Replacement question: **detect the step boundary directly** - pre and post disagreeing by ~= the pair's map step - which is sign-agnostic and needs no coincidence. Verify on pair 31 (boundary at g7, interior g8-g11 confirmed) and pair 50 (interior confirmed; its boundary gap is not yet identified). **ANSWERED 2026-08-21 — see Result 8: the dump-visible base rate is zero.** 935 admitted rows, largest trustworthy step in the whole corpus **63.9 ms**, nothing above 100 ms; nothing survives the 50 ms floor once the archived run-length/settling test is applied; both known boundaries (31/g7, 50's cluster) are censored because a step past the search radius kills the correlation that would measure it. The step boundary is real but was **not measurable from `lag_decision` dumps at that radius**. **Bounded 2026-08-23:** the zero is an upper bound on ±2000 ms ([TEMP-envelope-3000-smoke-2026-08-23.md](TEMP-envelope-3000-smoke-2026-08-23.md)); size the new invariant with a corpus-wide flat-3000 dump, not by re-running this census. Next instrument is the WAV sweep (the `shoulder-sweep` bin, [shoulder-sweep.md](shoulder-sweep.md)); its queue is two pairs, 31 and 50. The `--steps` plan ([TEMP-step-boundary-census-plan.md](archive/TEMP-step-boundary-census-plan.md)) is **parked** — its contract was validated (the Arm B dead-shoulder gate is what suppressed pair 50's noise staircase) but its deliverable is this null, already obtained without it. | Redirects the BACKLOG item from a coincidence-dependent proxy to the invariant it is a proxy for — and shows the invariant is out of reach of dump-only measurement. | Done |
| 9 | ~~What does the `ambiguous` verdict mean, and does it belong in the one-dead/both-dead split?~~ **Answered 2026-08-21 — yes it is a distinct verdict, and no it does not change the counts.** `LagVerdict::Ambiguous` (`gap_fingerprint/schema.rs:932`) is a third verdict, "partial; weak shared content" — so the ad hoc script's `== "decorrelated"` branch did under-count dead shoulders. Measured on the same corpus: of 1942 shoulder verdicts, **1908 `timing_offset`, 31 `decorrelated`, 3 `ambiguous`**. Exactly **2 gaps** were bucketed "both live" despite carrying an ambiguous shoulder (32/g1 post r 0.409; 49/g37 post r 0.426) — **both `skip`-tier**, neither `patch`-tier. Result 3's "954 of 971 both-live" is properly 952; no `patch`-tier gap moves, so Result 2's 9-gap risk list is unchanged. | — | Done |
| 10 | ~~Is `2026-08-19-decision-map-oriented` (53 pairs) the largest available fingerprint corpus?~~ **Answered 2026-08-21 — it is the whole corpus, and it is already deduplicated.** It holds **51** pair dirs, 1049 `*_full.json`, 971 gaps with both anchors. The corpus manifest (`gap-files/extended-pairs.csv`) carries exactly **51 active pair rows**; labels 6 and 16 are commented out as duplicate media (same `a_id` *and* `b_id` as 22 and 28), and this run already omits them — so its 971 gaps are 971 *independent* gaps and no base rate here needs a dedup correction. Note the count in this note's Data sources ("53 pairs") is the label count, not the dir count. Nothing in the manifest is unswept. | — | Done |
| 11 | **Why is 31/g10 audible when 31/g11 and 31/g8 — same pair, same map step, same far-offset shape — are not?** Opened 2026-08-21; the census's own unexplained defect, previously discussed only in Result 5's prose with no question owning it. All three gaps are `patch`-tier interior of pair 31's ~−1.15 s step. **WAV / production / Wide dumps** have both shoulders of each at the far offset to within ~20 ms; **HEAD `lag_decision` on Narrow does not** — 31/g8 pre is declined at +800 (`D` 13.0 ms) as of 2026-08-22, while g10/g11 still rescue. Result 5 heard a blip **at the end of the patch** (the post join) on g10 alone. `shared_far_offset` therefore does not predict the defect — a model built from that shape gets g11 and g8 wrong. **Production's own records invert the intuition** (`2026-08-13-repeat-terms/31.json`, 0-based): g10 is `confidence: high`, `anchor_seam_used: false`, `gap_start/end_adjust_frames` **both 0**, pre/post correlation 0.496/0.483 — the *cleanest-looking* of the three — while g11, which sounds fine, is `anchor_seam_used: true` with a much weaker post correlation (0.574/**0.376**) and a large `gap_end_adjust_frames` (+64800; confirm the unit, but 48 kHz ⇒ ~1.35 s), and g8, which also sounds fine, is `confidence: marginal`. **Live lead:** the one structural difference at the audible locus is that g11's post join received an end adjustment from the anchor-seam path and g10's did not. That is the same mechanism the [pair-50 note](TEMP-pair50-anchor-seam-outcomes-2026-08-21.md) Q1 is circling from the other side, and **both confirmed defects in the Result 5 batch are at the post join** — worth testing as one axis across both pairs, while remembering the steps have opposite signs (−1.15 s vs +2.789 s) and the pairs share nothing else. Cheapest first steps, no media run: (a) re-listen to `…_g010_full_a_patched.wav` to pin whether the blip is at the post join or at the fill's own end boundary — Result 5's "end of the patch" does not distinguish them; (b) correlate the overlap-only post remnant for g10 **and** g11 with the pair-50 note's technique and compare residual lags; (c) if (b) shows a residual on g10 only, that is the echo/blip mechanism candidate shared with 50/g10. Do not fold g8's current Narrow `lag_decision` miss into this question — that is the capture item. | Only defect this census owns and the only one with no mechanism at all. Also a **confidence-calibration** data point: production ranked the audible gap `high` and both clean gaps lower, so `confidence` is not tracking audibility on this pair. n=1 defect against n=2 clean twins — real but thin; do not build a detector from it. | Medium |
| 12 | ~~What is 19/g2, and is a 64 ms persistent step the same phenomenon as pair 31's 1.15 s one?~~ **Opened and closed 2026-08-21 — the premise was an artifact.** 19/g2 was believed to be the corpus's only dump-visible persistent step boundary. It is not: it fails the boundary rule in [TEMP-step-boundary-census-plan.md](archive/TEMP-step-boundary-census-plan.md) §4 on both halves of the sequence test — only one admitted gap precedes it (the rule needs 2 settled per side), and the two gaps following it sit at 80.4 and 49.2 ms, a 31 ms spread against a 2.0 ms settle window. It is a lone excursion in a wandering pair, which is the case that test exists to reject. The re-derivation that promoted it had dropped the sequence test and kept only `\|step\| > floor`. **Nothing here needs doing; left in place so the same re-derivation is not attempted a third time.** | Records a withdrawn correction. | Closed |

---

## Reproduction

Both source dumps are gitignored (`gap-files/`) and not guaranteed to survive cleanup — this doc
is the durable record of what they showed. To reproduce Result 1/2/3: for each
`gap-files/2026-08-19-decision-map-oriented/<pair>/*_full.json`, read `gaps[0].geometry.duration_secs`
(→ `gap_ms`), `gaps[0].lag_decision.pre_anchor[0]` / `.post_anchor[0]` (`frac_lag_ms`, `peak_r`,
`verdict`), and `gaps[0].outcome.tier`. Compute `bridge_ms = gap_ms + post.frac_lag_ms −
pre.frac_lag_ms`. Bucket by whether either/both verdicts equal `"decorrelated"`.

---

## Non-goals

- Not a Change.
- **Do not cite Result 5 as evidence against a `collapsed_bridge` detector** (corrected 2026-08-21 by
  review — the earlier wording here did exactly that). What Result 5 supports is narrower: a
  *dead-in-window / envelope-declined* shoulder does not predict an audible defect (7 of 9 clean).
  The 9 gaps were mostly far-offset cases; **31/g8 did not reproduce 31/g7's class** — it is most
  **confirmed** `shared_far_offset` (Result 7) — and 31/g7 was not in the batch. So the collapsed-bridge
  predicate was **never tested here**: not confirmed, not refuted, still n=1. Build or reject a
  detector on its own evidence, not on this listen set.
- Do not treat `envelope_rescue: declined` as a defect signal by itself — for pair 50's 6 gaps it is
  structurally expected (true offset exceeds both the 600 ms nominal and 2000 ms envelope search
  radii), and 5 of 6 patched cleanly by ear anyway. See the pair-50 note, Result 1.
- Do not re-run the full 53-pair fingerprint dump to answer the remaining questions — production's
  own `--lag-envelope-max-lag-ms` rescue (Result 4) already does most of what a separate wide-sweep
  instrument would; reach for the `shoulder-sweep` bin /
  `scratch_patch_registration_sweep.py` only if the built-in rescue is insufficient for a specific gap.
- **Three instrument/mechanism cautions moved 2026-08-21 to
  [TEMP-pair50-anchor-seam-outcomes-2026-08-21.md](TEMP-pair50-anchor-seam-outcomes-2026-08-21.md)
  § Non-goals**, where their supporting evidence now lives. Read them before running any WAV
  comparison or proposing any wide-map-step fix: (a) **context-margin overrun** — a `_a_patched` vs
  `_b_surround` "no match" region is expected when the true offset is large relative to
  `gap_signature_context_secs`, and it is a fact about the *export*, never an explanation for a
  production decline (check the search radius instead); (b) **padded slivers** — correlate the
  overlap-only remnant, not `scratch_sliver_check.py`'s 150 ms-padded output; (c) **two correction
  mechanisms** — `fill_offset.rs`'s `AnchoredRetry` cross-gap triangulation *and* `patch_region.rs`'s
  per-gap anchor-seam bracket search. A theory built from only one of them predicted the wrong
  outcome here, and was refuted by real patch output.
