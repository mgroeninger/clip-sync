# TEMP — Collapsed-bridge census: full-corpus `bridge_ms` re-scan (2026-08-20)

**Status:** findings note, not a Change, 2026-08-20. Extends the `collapsed_bridge` predicate
(`|gap + post_lag − pre_lag| ≈ 0`, [gap-vocabulary.md](gap-vocabulary.md) § *Known holes*, Hole 2)
from its single positive example (31/g7) against a larger population, and re-validates the
14-gap census's three buckets against the current (post-map-coordinates-fix) binary. Prompted by
BACKLOG's "Collapsed-bridge skip verdict" item, which is currently designed against n=1. **This
note proposes no change.** Each open question below needs more measurement or ear-validation
before any of it becomes a Change plan.
Parent: [archive/TEMP-lag-decision-map-coordinates-plan.md](archive/TEMP-lag-decision-map-coordinates-plan.md)
(the fix this census re-validates), [archive/TEMP-31g7-gap-shape-vocabulary.md](archive/TEMP-31g7-gap-shape-vocabulary.md)
§9 (origin of the predicate and the original 14-gap census).

Per [gap-vocabulary-redesign / licensed-media-names-never-recorded], gaps are referred to by
corpus pair index + gap index only. Both source dumps are gitignored (`gap-files/`) and may be
deleted; the numbers below are the durable record — see **Reproduction**.

---

## Data sources

- **14-gap wide-sweep census** (pre-map-coordinates-fix), `gap-files/2026-08-18-one-dead-listen/wide-sweep/census.json`.
  Instrument: `scratch_shoulder_sweep.py`, 1 s template, ±2000 ms, non-chained, 250 ms peak
  exclusion. This is the source of the original three buckets: 12 `shared_near_false_dead`, 1
  `collapsed_bridge` (31/g7), 1 `shared_far_offset` (31/g10).
- **Full-corpus post-fix re-scan**, `gap-files/2026-08-19-decision-map-oriented/` — 53 pairs, 971
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

Among the 954 gaps (of 971) where both shoulders register a real in-window lock, the closest any
`bridge_ms` comes to zero is **505 ms** — nothing between that and true zero. This addresses the
earlier concern that a short gap could collapse its bridge with a lag swing small enough to keep
both shoulders inside ±600 ms and register as an ordinary shared-registration gap: at least in
this 53-pair corpus, that doesn't happen. The real open surface is Result 2's 16 gaps, not this
population.

---

## Result 4 — envelope rescue on the 9 `patch`-tier both-dead gaps (2026-08-20)

Production's own dead-shoulder rescue (`--lag-envelope-max-lag-ms`, default 2000 ms, [gap-fingerprint.md](gap-fingerprint.md) § dump flags) turns out to answer most of Question 1 directly — no separate wide-sweep instrument needed for these. Ran `--gap-listen` against the 9 gaps
(pair 31: gaps 8, 10, 11; pair 50: gaps 8, 9, 10, 14, 15, 16 — 1-based) and read `envelope_rescue` /
`envelope_lag_ms` off both shoulders of each:

| pair/gap | pre `envelope_rescue` | post `envelope_rescue` | reading |
|---|---|---|---|
| 31/g8 | declined (800 ms candidate, r 0.82, failed fine re-sweep) | **rescued**, −1137.7 ms, r 0.94 | ~~asymmetric — strongest new `collapsed_bridge` candidate~~ **Reclassified 2026-08-21 (review): almost certainly `shared_far_offset`, not collapsed-bridge.** The envelope-first dump locks **both** g8 shoulders at −1137.7 ms (r 0.96–0.98), agreeing with g9/g11 — so the "declined pre / 800 ms candidate" here is likely a wrong coarse peak on the current nominal-first rescue path, not a genuinely unknown or near-zero pre. The ear note independently agrees: the clips are misaligned *from the opening*, which is a far-offset signature, not g7's line-up-then-diverge shape. An +800 ms pre would also predict B **earlier**, the opposite of what was heard. |
| 31/g10 | rescued, −1151.6 ms | rescued, −1155.4 ms | symmetric, `bridge_ms` ≈ `gap_ms` (1861.6) — confirms `shared_far_offset`, not collapsed-bridge. |
| 31/g11 | rescued, −1158.4 ms | rescued, −1158.4 ms | symmetric, `bridge_ms` ≈ `gap_ms` (600.6) — same far-offset shape as g10, adjacent gap index. g10/g11 look like one contiguous far-offset region, not independent phenomena. |
| 50/g8, g9, g10, g14, g15, g16 | declined (all 6) | declined (all 6) | every shoulder's coarse envelope peak (r 0.44–0.72) failed fine re-sweep, even at the full ±2000 ms search. Unresolved, not confirmed either way — pair 50's cluster remains open. |

Two consequences:
- ~~**31/g8 is now the priority ear-check** (Question 2) — the only gap in this batch that plausibly repeats 31/g7's exact defect shape.~~ **Superseded 2026-08-21 (review):** per the row above, 31/g8 is most likely `shared_far_offset`. It was ear-checked anyway (Result 5, clean) — but a clean result on a non-member says nothing about collapsed-bridge either way. **This listen batch probably contained no collapsed-bridge case at all; 31/g7 was not among the nine.**
- **31/g10 and g11 are resolved out of the collapsed-bridge question** — same-offset, both-shoulders-agree far offset, not a collapsed bridge. They still may be live production risk (both `patch`-tier), but that's a `shared_far_offset` question, not this one.
- ~~**Pair 50's 6 gaps need a different approach**, not just a wider window — 2000 ms already exceeds what rescued 31's gaps, and it still found nothing durable. Possible next moves: check whether these regions are structurally different content (cuts/edits between A/B versions) rather than a lag problem at all.~~ **Struck 2026-08-21 (Result 6).** A wider window is *exactly* what was needed: the true peak is at +2.8 s and the ±3000 ms sweep found it. The "structurally different content / cuts-and-edits" speculation is refuted — the content matches at r ≈ 1.0 once you search far enough. This is an envelope-sizing question ([TEMP-envelope-adaptive-widen-plan.md](TEMP-envelope-adaptive-widen-plan.md), which already said pair 50 needs ~3 s), not a content question.

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
consensus; [TEMP-envelope-adaptive-widen-plan.md](TEMP-envelope-adaptive-widen-plan.md) already said
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
  predictor.~~ **Withdrawn 2026-08-21 (review).** 31/g8 is most likely `shared_far_offset`, not a
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

## Follow-up questions (open, prioritized)

| # | Question | Why it matters | Priority |
|---|---|---|---|
| 1 | ~~Wide-sweep the 9 `patch`-tier both-dead gaps~~ **done 2026-08-20 via production's own `--lag-envelope-max-lag-ms` rescue** (see Result 4); pair 50's "declined" outcome fully explained by Result 6 — the 2.8 s true offset exceeds the 600 ms nominal and 2000 ms envelope search radii. **The production explanation is the search radius alone**, not clip overrun (which is a listen-export artifact) and not the 1000 ms window length. | — | Done |
| 2 | ~~Ear-check the 9 `patch`-tier gaps~~ **done 2026-08-20/21** (see Result 5) — only 2 of 9 have an audible defect. **Scope narrowed 2026-08-21 (review):** the supported claim is that *both-dead-in-window / envelope-declined* does not predict a defect. The batch contained no confirmed collapsed-bridge gap (31/g8 reclassified as `shared_far_offset`), so it does **not** undercut a collapsed-bridge detector — that predicate was never tested here. | Undercuts a detector built from the `lag_decision`/envelope shape, at n=9. Does **not** speak to `|bridge_ms| ≈ 0`. | Done |
| 3 | ~~Investigate 50/g10's echo directly~~ **Moved 2026-08-21 to [pair-50 note](TEMP-pair50-anchor-seam-outcomes-2026-08-21.md), Question 1.** It is a patch-quality question about a gap that patches at the *correct* offset, not a `bridge_ms` question — and both models this census raised for it (silent-onset, unreached region) are dead. | — | Moved |
| 4 | ~~Why did 31/g8 reproduce 31/g7's exact numeric shape without reproducing its audible defect?~~ **Mis-aimed — replaced 2026-08-21 (review).** The premise is probably false: 31/g8 looks like `shared_far_offset`, not collapsed-bridge. Replacement question: **confirm 31/g8's pre-shoulder is the −1137.7 ms lock** the envelope-first dump reports (r 0.96–0.98, agreeing with g9/g11), rather than the unresolved/near-zero pre the nominal-first rescue path reported. If confirmed, 31/g8 leaves the collapsed-bridge population entirely and Question 8's n stays 1. | Decides whether this census has *any* second collapsed-bridge candidate, or whether the predicate is still n=1. Also a check on the nominal-first rescue path picking a wrong coarse peak. | **High** |
| 5 | ~~Is `envelope_rescue: declined` worth treating as a warning signal for pair 50~~ **answered by Result 6: no** — decline is structurally expected for the whole cluster (true offset exceeds both search radii), so it carries no defect information on its own; 5 of 6 patched cleanly anyway. | — | Done |
| 6 | Is pair 50's cluster (g8–g10, g14–g16) one map-step rather than six independent gaps? | **Answered — recorded, not "confirmed contiguous."** Same ~2.8 s offset on all 6: yes. *Contiguous*: **no** — `a_start_secs` = 2494, 2627, 2711, 3424, 3544, 3777, spanning ~21 min with `skip`-tier gaps interleaved. It is a persistent pair-level step. The coordinate check dismissed as "adding nothing" would in fact have shown this. | Done |
| 7 | Wide-sweep the remaining `skip`-tier both-dead gaps (31/g9; 50/g17) and 25/g11's coordinate-fix status. **50/g11–g13 moved 2026-08-21** to [pair-50 note](TEMP-pair50-anchor-seam-outcomes-2026-08-21.md) (Question 2) — they are inside pair 50's map-step region, flanked by correctly-resolved gaps, so their failure is an anchor-seam question, not a census gap. | Lower risk (already declined for unrelated reasons) but needed for a complete census before finalizing any detector. | Low |
| 8 | Once ≥2–3 confirmed `collapsed_bridge` cases exist, revisit the `|bridge_ms| ≤ 50 ms` threshold. **Still n=1 — but for a different reason than first recorded (review, 2026-08-21):** not "31/g8 failed to confirm," rather 31/g8 was probably never a member (see Question 4). This census did not add a second case and did not refute the first. | Given Result 5, the bar for "confirmed" is audible defect + shape, not shape alone. | Low (blocked) |
| 9 | What does the `ambiguous` verdict (seen on 50/g18's pre-shoulder) mean, and does it belong in the one-dead/both-dead split at all? | This note's ad hoc script only branched on `== "decorrelated"`; `ambiguous` may be a distinct verdict that changes the counts above if mis-bucketed. | Low |
| 10 | Is `2026-08-19-decision-map-oriented` (53 pairs) the largest available fingerprint corpus, or is there a bigger one not yet swept? | [[corpus-fingerprint-run-protocol]] — runs are expensive, one pair at a time; worth confirming before assuming 53 pairs is the full population for base-rate claims. | Low |

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
  likely `shared_far_offset` (Question 4) — and 31/g7 was not in the batch. So the collapsed-bridge
  predicate was **never tested here**: not confirmed, not refuted, still n=1. Build or reject a
  detector on its own evidence, not on this listen set.
- Do not treat `envelope_rescue: declined` as a defect signal by itself — for pair 50's 6 gaps it is
  structurally expected (true offset exceeds both the 600 ms nominal and 2000 ms envelope search
  radii), and 5 of 6 patched cleanly by ear anyway. See the pair-50 note, Result 1.
- Do not re-run the full 53-pair fingerprint dump to answer the remaining questions — production's
  own `--lag-envelope-max-lag-ms` rescue (Result 4) already does most of what a separate wide-sweep
  instrument would; reach for `scratch_shoulder_sweep.py` / `scratch_patch_registration_sweep.py`
  only if the built-in rescue is insufficient for a specific gap.
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
