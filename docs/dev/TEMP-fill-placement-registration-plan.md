# Fill placement vs local registration — plan

Status: **investigation → delivery. S3 and S5 done (2026-08-12); S2(1) done; S2c done (2026-08-12);
S2(2) superseded 2026-08-13 — sweep refuted, prior must be registration-referenced, not nominal-referenced;
S2b unblocked — tracked in [TEMP-unmatchable-shoulder-plan.md](TEMP-unmatchable-shoulder-plan.md)
(decisions open, not started); S1 blocked on re-scoping; S4 conditional and unstarted.
Data re-based 2026-08-14 (§0.0) — every corpus count below is superseded; the per-gap conclusions are not.**

**The source-vs-gate fork is retired — the answer was "both, on different gaps".** Earlier drafts of
this document (and §4's exit, since rewritten) treated a single verdict as gating everything
downstream. It does not: §4's first pass and §4.3's sweep together establish **two distinct defects**
that need two distinct fixes and share only a symptom.

| | 26/g6 | 31/g7 |
|---|---|---|
| what went wrong | a near-nominal placement existed and **lost on score** | **nothing near nominal was ever scored** — 18 of 20 brackets died at `structure_floor` |
| reads as | **source** | **gate** / scoring-objective |
| the fix | ~~**S2** — a near-nominal prior, via backward `LATE_START_PENALTY` (§4.1c)~~ → **superseded 2026-08-13**: the prior must reference the pair's measured registration, not nominal (§0 S2(2)) | **S2b** — the `min` reduction over shoulders |
| aliasing? | surface *is* periodic (480.5 ms comb, §4.3) but the true peak still ranks first | no — flat floor, 9.6× margin (§4.3) |

Consequences for sequencing, all already reflected in §0: **S1 is correct to record under either
verdict** (and is blocked on its own precondition, not on this fork); **Phase B/C are not voided by a
"source" answer** — Phase B was demoted to the separate `applied = 0` domain question and Phase C's δ
remains blocked on it. **Read §0 first** — it names the actual deliverables; §1–§7 are the evidence
behind them, and where an older section still argues the fork, §0 and §4.3 win.

Split out of [BACKLOG.md](../../BACKLOG.md) on 2026-08-11 (the section had grown to ~270 lines).
Successor to the archived [TEMP-fill-placement-axis-plan.md](archive/TEMP-fill-placement-axis-plan.md),
which made fill placement observable; this plan asks whether the observed placements are *right*.

**Conventions used throughout.** Gap numbers are the repair table's 1-based `#`, as
`--fingerprint-gap` takes them (`resolve_fingerprint_gap_select`, `composition.rs`); corpus dumps
index the same gaps **0-based**, so 26/#7 = `26/g6`. Join the two on `geometry.a_start_secs`, never
on index — on `a_start_secs` all patched gaps join to the dumps with zero misses (227/227 on the old
mixed set; re-establish on the oriented pair of runs as part of §0.0 step 3). Bracket
placements (`start_frame`) are B-haystack frames whose nominal origin is **672000**; see §3.1 before
comparing any two of them.

**Data. Re-based 2026-08-14 onto oriented runs — see §0.0, which supersedes every corpus count in
§0–§7.** The plan's measurements now come from exactly two dirs, both correctly oriented:
`gap-files/2026-08-13-repeat-terms/` (53 pairs, **317 patched gaps**, 210 carrying `fit_terms`) for
scan+patch reports, and `gap-files/2026-08-13-v0.6.0-oriented/` (as of 2026-08-14 morning: pairs 1–42
complete, 43 writing, 44–53 remaining) for per-gap dumps carrying `lag_decision`.
**`archive/mixed-ab/` is retired as a data source** — a hand-written name-sorted manifest scanned
`name.2.ext` as A on pairs 2–17, and the two oriented dirs reproduce its usable half exactly (§0.0),
so there is nothing left to read there. Do not join against it.

The one exception is `archive/mixed-ab/2026-08-09-drift-listen/`, which keeps the rendered
`_a_surround` / `_b_surround` / `_a_patched` WAVs behind the ear labels — audio, not measurement, and
not re-derivable without a media re-run. Its pairs 18, 24, 25, 26, 31 and 37 are correctly oriented and
their labels stand; **pairs 3 and 6 were rendered from the reversed pairing and their labels do not
transfer** (both were clean labels, so the clean cohort shrinks by two — see §2.3). The two
`listen*.csv` files are render worklists carrying A/B media paths and hold **no label column** — the
ear labels live only in this document, so treat §1/§2's tables as their system of record.

New runs go at `gap-files/<date>-…` from a `repair-directory-pairs.ps1 -Recurse -LabelByFolder`
manifest. All of `gap-files/` is gitignored and media-derived — quote pairs by index only, never by
title or path.

---

## 0.0 Data re-baseline (2026-08-14) — read before any number below

A hand-written manifest (folder group, `Sort-Object Name`, first file as A) reversed A/B on
**pairs 2–17**: those runs scanned `name.2.ext` as A, so they measured the *other* member's dropouts.
`repair-directory-pairs.ps1` was not that generator. Everything in §0–§7 that aggregates across pairs
was computed over that mixed set and is superseded. Everything scoped to a single correctly-oriented
pair — which is both ear-confirmed failures and most of this plan's reasoning — is untouched.

### What was verified, and how

Two independent tests, both scripted from the files, agreeing exactly on all 39 pairs:

| test | result |
|---|---|
| `manifest.json` `a_id` / `b_id` content hashes, old vs oriented | pairs **2–17**: `old.a_id == new.b_id` and vice versa → **swapped**. Pairs **1, 18–39**: both match → **same A**. No pair unclassified. |
| gap sets joined on `a_start_secs` per label | swapped pairs share **0** of 108 old patched gaps; same-A pairs share **all 119**. Perfectly bimodal, no partials. |

The gap-set test works because a gap *is* "A silent, B has energy", so swapping A and B produces a
near-disjoint set by construction. It is corroborating evidence; the hash test is authoritative and is
the one to run on any dir of unknown provenance.

**The oriented dirs reproduce the old data exactly where the old data was valid.** Over pairs 1 and
18–39: **1036/1036** `lag_decision` shoulder anchors identical (`frac_lag_ms`, `peak_r`, `peak_z`)
between `2026-08-08-v0.5.2-full-corpus/` and `2026-08-13-v0.6.0-oriented/`, no gap present in old and
missing from new; and all **119/119** patched gaps carry an identical `align_adjustment_secs` (< 1e-6)
between `2026-08-12-fit-terms-baseline/` and `2026-08-13-repeat-terms/`. So nothing between the
2026-08-12 baseline commit and `f3ebebab` moved a placement — `repeat_pre`/`repeat_post` are pure
reporting — and the re-analysis can be built on **those two dirs** with the old dirs retired rather
than cross-linked.

**Pairs 2–17 are new data, not corrected data.** Their old measurements were internally valid
observations of the reverse pairing (B filled from A), a direction we do not ship. Findings there
(12/g4, 16/g11) are **withdrawn, not refuted** — re-seek them in the production orientation; if the
same phenomenon reappears it is a stronger result than the original, because it would hold both ways.

### The new baseline

`gap-files/2026-08-13-repeat-terms/` — 53 pairs, 52 with patched gaps, policy-identical to the
2026-08-12 baseline plus the repeat-term reporting.

| | old (mixed, 39 pairs) | **new (oriented, 53 pairs)** |
|---|---|---|
| patched gaps | 227 | **317** |
| carrying `fit_terms` | 147 | **210** |
| `dual_fit_used` | 80 | **107** |
| `anchor_seam_used` | 75 | **100** |
| `signature_mode` energy / bool | 225 / 2 | **315 / 2** |
| `patch_tier` high / marginal / anchor_trusted | 179 / 37 / 11 | **244 / 51 / 22** |
| `residual.informative` | 16 | **26** (8%) |
| `\|align_adjustment\| > 0.5 s` | 5 | **12** |
| slide identity `placement_slide_frames == \|adj\|×48000` | 145 / 225 | **208 / 315** |
| `beyond_lag_reach` — n, median, max \|applied\| | 159, 56.7 ms, 1450 ms | **215, 43.8 ms, 2833 ms** |
| every other `residual_uninformative` — n, median, max | 68, 0.0 ms, 8.0 ms | **102, 0.0 ms, 9.8 ms** |

The `beyond_lag_reach` / other split stays a tautology of the 10 ms definition (§3), as before: the
non-`beyond` bucket still tops out under 10 ms. `dual_fit_used` continues to equal the no-`fit_terms`
count *exactly* in every subset (107/107, and 47/47, 38/38, 22/22 on the three cohorts below), which
re-confirms "dual-fit sets `fit_terms: None`" as an identity rather than an observation.

The corpus now decomposes into three cohorts, worth reporting separately until the new ones have been
looked at:

| cohort | pairs | patched gaps | `fit_terms` |
|---|---|---|---|
| same-A, directly comparable to the old work | 1, 18–39 (22 with gaps) | **119** | 72 |
| re-oriented, never measured this way | 2–17 | **111** | 73 |
| never measured at all | 40–53 | **87** | 65 |

Ear labels exist only in the first cohort, and only a dozen of those 119 were labelled. The other
**198** gaps (2–17 and 40–53) have no ear labels; most of the 119 same-A gaps don't either.

### The recount answers the repeat-prior run that BACKLOG was waiting on

This baseline is the `--patch-only` run that [BACKLOG.md § Repeat-at-seam prior is inert on every
winner](../../BACKLOG.md#repeat-at-seam-prior-is-inert-on-every-winner-one-branch-is-unreachable) shipped
the `repeat_pre` / `repeat_post` emit for. Full answers are recorded there; the one that lands **in this
plan** is its fourth open question:

**The repeat window does not see the two duplications this prior exists to catch.** On 26/g6 the pair
is `repeat_pre` 0.153 / `repeat_post` 0.368; on 31/g7 it is 0.249 / 0.207 — against a 0.55 threshold, on
the two gaps that are *ear-confirmed shoulder repeats*, and against a corpus median of 0.12. The other
four large backward slides agree (31/g8 0.046/0.097, 31/g10 0.113/0.530, 31/g11 0.295/−0.009, 17/g34
−0.028/0.121). BACKLOG's own conditional therefore fires: *"if their `repeat_pre` is unremarkable, the
prior is aimed at the wrong quantity and the registration residual is the only live discriminator."*

**A repeat-correlation term is not the fix for the §1 defect at this window size** — the discriminating
information is registration, not self-similarity, which is where §2 arrived from the other end. This
constrains S2/S2b rather than adding work to them.

(For the record, the recount also confirms `anchor_penalty` 0.0 on all 210 — expected, §4.1a: the
anchor prior needs `--fill-offset anchored-retry` — and `late_start_penalty` firing on 140 of 210, so
the forward prior is the only live one of the three.)

### What survives without re-measurement

- **Both ear-confirmed failures.** 26/g6 (`align_adjustment_secs` −1.4500, `pre` 0.3663 / `post`
  0.4942, tier `high`, `energy`, dual-fit declined) and 31/g7 (−0.9774, 0.4368 / 0.4369) are present in
  the new baseline with **identical values**. §1, §2, §2.1, §2.2, §2.4 stand.
- **S3 / §4.3** — the ±2000 ms sweeps refuting the rival peak, measured on 26 and 31 only.
- **S2b** and its exit criterion (31/g7 → ≈ −47 ms).
- **S2(2)'s refutation of the backward-penalty sweep** — the scale-invariance argument is 26/g6 vs
  31/g8 and 31/g11, all same-A.
- **Every code-derived fact:** §3's dual-nominal mechanism, §3.1's frame-672000 origin, §3.2, §4.1a's
  prior table, §4.1b, S2c, S5, S2(1).

### What is withdrawn pending recomputation

- ~~**Route 2's consensus** (§0 S2(2))~~ — **rebuilt, see "Step 3 result" below: it survives at
  34/34 in-window pairs and a 9-pair load-bearing cell.** The 77.9% slide-variance figure is the one
  part not yet recomputed.
- ~~**The `peak_z ≥ 12` re-gate** (§0 (3))~~ — **rebuilt: 30/31 agree, cell 8/8.** Its suspect list is
  re-derived below; 12/g4 and 16/g11 are gone with their swapped pairs.
- **The −23.4 / −24.8 ms signed bias.** Note it is *not* an orientation artifact on the same-A pairs —
  those anchors are bit-identical old to new. It **survived the rebuild** at −21.9 ms (`peak_r`) /
  −26.1 ms (`peak_z`), negative on 32/34 and 28/31 pairs, so step 5 is unchanged and now better
  supported.
- **§2's corpus firing table** (δ 150/300/500 ms → 20/9/2) and the 30/227 `|residual| > 150 ms` count —
  both need `lag_decision`, so both wait on the join.
- **§4's zero-move bracket distribution** (132 of 291, median +12.4 ms) — dump-side, same wait.
- **§2.3's 3/g9 caveat.** Pair 3 is swapped; that gap does not exist in the production orientation. It
  only ever reduced n, so drop it and recount the clean cohort. The "do not turn this into a level
  threshold" ruling stands on its own argument.
- **Two of the twelve ear labels.** The `drift-listen` worklists cover pairs 3, 6, 18, 24, 25, 26, 31,
  37; **pairs 3 and 6 are swapped**, so their renders are of the reverse pairing. Both were *clean*
  labels, so the labelled cohort loses two clean gaps and no failures — every margin in §1/§2 is
  unaffected or slightly better-supported, and n drops. (The separate `2026-08-08-edge-listen`
  worklist, which belongs to the fill-level work rather than to this plan, loses 3 of its 5 — pairs 7,
  8 and 12. The archive README's "3 / 6 / 10" was wrong: no pair 10 appears in any worklist.)

### The fingerprint run is complete (verified 2026-08-14)

`2026-08-13-v0.6.0-oriented/` holds **53 pair dirs, 53 `manifest.json`, 1090 `*_full.json` dumps**, and
**every one of the 317 patched gaps has a dump**. The only gaps without one are 26 leading `g0`s whose
status is `not_planned` — expected, not truncation. Step 3 can run at full corpus scope in one pass.

### Two pairs are duplicate media — dedup before quoting any corpus rate

The `a_id`/`b_id` sweep that settled orientation also shows **pair 6 ≡ pair 22** and **pair 16 ≡ pair
28**: identical content hashes on *both* members. Their new outputs are bit-identical (6 vs 22: 5/5
patched gaps matching on `align_adjustment_secs`, `pre`/`post_correlation`, tier and `dual_fit_used`;
16 vs 28: 4/4). So 53 labels are **51 distinct media**, and 317 gaps are **308 independent** ones.

| | as labelled | deduped (drop 6, 16) |
|---|---|---|
| pairs with patched gaps | 52 | **50** |
| patched gaps | 317 | **308** |
| carrying `fit_terms` | 210 | **203** |
| `dual_fit_used` | 107 | **105** |
| `anchor_seam_used` | 100 | **97** |
| `residual.informative` | 26 | **26** |
| `\|adj\| > 0.5 s` | 12 | **12** |

The duplication **straddles the cohort table above**: 6 and 16 are counted as "re-oriented, never
measured this way", but their content was already measured, correctly oriented, as 22 and 28. That
cohort is **102** genuinely new gaps, not 111. Drop 6 and 16 rather than 22 and 28 — the latter carry
the same-A history the regression column needs.

Two things fall out for free:

- **A determinism check passed.** Two independent invocations on identical input produced identical
  placements — worth keeping as the cheapest available regression on the analysis pipeline.
- **A third, stronger confirmation of the swap.** In the old archive, pair 6 was scanned inverted and
  pair 22 was not, on *the same two files* — a controlled A/B-swap experiment we did not know we had.
  Old-6 has 9 patched gaps, old-22 has 5, and they share **zero** `a_start_secs`; old-16 vs old-28 is
  8 vs 4, also zero shared. Identical media, opposite direction, disjoint gap sets — exactly what "a
  gap is *A silent, B has energy*" predicts, measured rather than argued.

### Step 3 result (2026-08-14) — Route 2 rebuilt: it survives, and it is stronger

Recomputed from scratch over `2026-08-13-v0.6.0-oriented` × `2026-08-13-repeat-terms`, joined on
`geometry.a_start_secs`: **317/317 gaps joined, 0 unjoined, 52 pairs.** Corpus-wide figures below are
**deduped** (pairs 6 and 16 dropped, 308 gaps).

**The analysis code was calibrated before it was trusted.** Re-run against the *old* pair
(`2026-08-08-v0.5.2-full-corpus` × `2026-08-12-fit-terms-baseline`) it reproduces the published
`peak_r` result exactly — **26/26 in-window pairs agree**, load-bearing cell **6/6 = {3, 6, 13, 18, 22,
32}** — and the `peak_z` cost figures exactly: **432 → 270** shoulders kept, gaps with no usable
shoulder **4 → 58**. Recovering that took two rules the prose had left implicit, both now explicit in
the code: a pair needs **≥3 unified gaps** before its consensus is quoted, and a **big-step gap is
excluded from the pair pool**, not merely from its own reduction. (Using the archive to calibrate
*code* is not using it as a data source; it is the regression fixture §0.0 asked for.)

**The rebuild is committed as [`scripts/measure/route2-consensus.ps1`](../../scripts/measure/route2-consensus.ps1)**,
so every number in this section is re-derivable without re-running the corpus:

```
./scripts/measure/route2-consensus.ps1 -ScanDir gap-files/2026-08-13-repeat-terms `
    -DumpDir gap-files/2026-08-13-v0.6.0-oriented -ExcludePairs 6,16
```

Its header carries both calibration targets (archive and oriented); it accepts both the pre- and
post-`1e4fef21` dump filenames, and `-PairTable` / `-OutOfWindow` / `-ShoulderMap` /
`-ReductionRobustness` / `-CsvOut` reproduce the supporting tables below.

| | old mixed (published) | **oriented, deduped** | same-A cohort |
|---|---|---|---|
| joined gaps | 227 | **308** | 119 |
| shoulders kept (`peak_r`) | 432 / 454 | **580 / 616** | 222 / 238 |
| in-window pairs agreeing ≤ 50 ms | 26 / 26 | **34 / 34** | 12 / 12 |
| load-bearing cell (\|consensus\| ≥ 150 ms) | 6 / 6 | **9 / 9** | 3 / 3 |
| big-slide median \|adj\| → \|adj − lag\| | 0.2713 → 0.0210 s | **0.2479 → 0.0241 s** (10.3×) | 0.3073 → 0.0209 s (14.7×) |
| signed bias (`peak_r`) | −23.4 ms | **−21.9 ms**, negative on 32/34 | −15.7 ms, 11/12 |
| signed bias (`peak_z`) | −24.8 ms, 20/22 | **−26.1 ms**, negative on 28/31 | −26.1 ms, 9/11 |

**The regression column passes.** Running the *same* code over old dumps and new dumps restricted to
the 22 same-A pairs gives **identical per-pair `(consensus, pooled)` on every pair, under both gates**
— as it must, the anchors being bit-identical, but it is now checked rather than assumed.

**The load-bearing cell went from 6 pairs to 9 and only 3 of the old 6 were real.** It is now
`{2, 3, 13, 14, 18, 22, 32, 47, 51}` — 9/9 agreeing — and it draws from all three cohorts (2, 3, 13, 14
re-oriented; 18, 22, 32 same-A; 47, 51 never measured). Route 2's core claim is no longer resting on a
cell that was half swapped; it is reproduced independently on media the original never saw.

**The `peak_z ≥ 12` re-gate also survives, at the same steep cost.** 616 → **373** shoulders, gaps
with no usable shoulder 10 → **75**; **30/31** in-window pairs agree and the load-bearing cell is
**8/8** `{2, 3, 13, 14, 18, 32, 47, 51}` (pair 22 leaves for lack of a confident shoulder, exactly as
before). The big-slide collapse is 0.2204 → 0.0221 s, **10.0×**. Conclusion unchanged: the gate costs
coverage and buys sorting, and the registration result does not depend on it.

**Robustness.** Pair-level agreement, the cell, and the bias are identical under all four reductions
(mean / pre / min-abs / best-r) because pooling is over shoulders, not reduced gaps. The per-gap
decisive cell does move with the reduction — `pre`-only is clearly the worst (`peak_z`: 13/24 confirmed,
median residual 0.0454 s, versus 22–24 confirmed and 0.0198–0.0227 s for the other three) — which is
itself the reduction-discipline point, measured.

#### A second pair-31

**Pair 50's consensus is +2.7891 s**, and its last six gaps in order read `. . . . X X X X X X` — no
usable shoulder at all. Pair 31 reproduces its published signature exactly (`. . X X X`, consensus
−1.1086 s). So the "true offset outside the ±600 ms sweep" class has **two** members, one of them in
the never-measured cohort, and it is no longer a single-pair observation. This is direct, independent
support for [TEMP-lag-envelope-recenter-plan.md](TEMP-lag-envelope-recenter-plan.md) and it gives that
plan a **second** validation pair.

#### The suspect list, re-derived (step 4)

The old list does not transfer: **12/g4 and 16/g11 are both from swapped pairs** and neither exists in
the production orientation. Under `peak_z ≥ 12`, deduped, the coherent contradictions are now:

| gap | `align_adjustment` | measured lag | discrepancy | cohort |
|---|---|---|---|---|
| **17/g34** | −1149.6 ms | −19.7 ms | **−1130.0** | re-oriented |
| **31/g7** | −977.4 | −47.5 | −930.0 | same-A (known, S2b) |
| **27/g2** | +83.4 | −549.2 | +632.6 | same-A |
| 45/g5 | +50.0 | −221.2 | +271.1 | never measured |
| 5/g2 | +24.6 | +268.4 | −243.8 | re-oriented |
| 33/g21 | −49.9 | −285.1 | +235.2 | same-A |
| 47/g14 | **+0.0** | +207.6 | −207.6 | never measured |
| 13/g16 | +153.3 | +257.0 | −103.7 | re-oriented |

Three of these deserve naming:

- **17/g34 is a third member of the 26/g6 class** — a ~1.15 s backward slide that the registration
  measurement flatly contradicts. It is *not* ear-labelled, and §0.0 already noted its repeat terms are
  unremarkable (−0.028 / 0.121). It is the strongest new candidate failure in the corpus.
- **27/g2** is a 633 ms discrepancy on a **correctly-oriented, always-valid pair** — it was reachable
  in the old data and was not on the published list, because the published suspect threshold was never
  written down. Treat it as a genuine miss of the old analysis, not as new data.
- **47/g14 fails in the opposite direction**: `align_adjustment_secs` is **exactly 0.0** while the
  shoulders measure +207.6 ms. Every failure this plan has chased so far is *slid too far*; this one
  did not move when it should have. If it holds up it widens §1's defect statement.

26/g6 is absent from this list under `peak_z`, exactly as the old run found: at `peak_z` 6.1 its lag is
unmeasurable, so it is refuted by §4.3's sweep, not by this route.

### Restart sequence

1. ~~Record the split.~~ **Done — this section.**
2. ~~Re-baseline the patch-side counts.~~ **Done — the tables above.**
3. ~~Rebuild Route 2 and the `peak_z` re-gate from scratch.~~ **Done 2026-08-14 — "Step 3 result"
   above.** One full-corpus pass, 317/317 joined; the code was calibrated against the archive first
   (it reproduces the published `peak_r` result exactly) and the same-A regression column is identical
   old-to-new under both gates.
4. ~~Re-derive the suspect list.~~ **Done — table above.** 12/g4 and 16/g11 are gone with their swapped
   pairs; the new list is headed by **17/g34**, **27/g2** and **47/g14**.
5. ~~**Then the −24.8 ms bias**, which is what blocks the lag becoming a scoring input.~~
   **Done 2026-08-14 — "Step 5 result" below.** It splits in two: ~11 ms is an artifact of
   `lag_at_placement`'s own window construction, and the remaining ~25 ms is the applied placement
   sitting **late** relative to where both shoulders register. Both are now separately measurable.

### Step 5 result (2026-08-14) — the bias is two terms, and only one is the probe

**The measurement's zero is not "A's gap edge against B's gap edge."** It is *A's border-template
edge* against *B's gap edge*. Only one of the two is pulled away from the dropout:

- **A side.** `lag_at_placement` (`measure.rs:874-882`) builds its templates with
  `border_templates_for_gap`. That construction walks the gap-facing edge back through silence
  (`gap_borders.rs:166-192`) and then trims the low-energy tail/head at 12 % of the template's own
  peak (`seam_splice.rs:24-55`). Both only ever move the edge **away** from the gap. Call the
  displacements `D_pre`, `D_post` — both ≥ 0 by construction. (`border_standoff_frames` is **0** on
  this path, so the 0.35 s standoff is not part of it.)
- **B side.** The anchors are `start_frame = b_mapped(refined.start)` and, for post,
  `start_frame + gap_frames` — the mapped gap edges exactly, undisplaced.

The sweep convention is "A's window flush with the anchor at lag 0" (`seam_local.rs:21`,
`base = max_lag + lag`), so a displaced A edge is reported as a lag of exactly that displacement.
The post shoulder is searched around `+round(L_pre)` and then shifted **back** (`measure.rs:800`), so
it does *not* inherit the pre displacement — the shift cancels. Against a true registration `T`:

```
pre lag   =  T − D_pre
post lag  =  T + D_post
```

Locked by `lag_at_placement_reads_border_displacement_at_true_registration`: a synthetic pair with
`T = 0` and a known fade on each shoulder reads exactly −`D_pre` / +`D_post`, not 0.

**So the two shoulders separate the two terms.** Writing the applied slide as `adj = T + α`:

```
post − pre      =  D_pre + D_post      ← the probe's displacement, α cancels
−(pre + post)/2 =  α ∓ (D_post−D_pre)/2 ← how late the placement sits, D mostly cancels
```

Corpus (183 gaps with both shoulders, non-dual, |adj| ≤ 600 ms):

| quantity | prediction | median | quartiles | sign held |
|----------|-----------|--------|-----------|-----------|
| `D_pre + D_post` (probe) | ≥ 0 | **11.3 ms** | 3.7 / 11.5 / 25.8 | 155/183 |
| `α` (placement) | — | **25.4 ms** | 7.3 / 25.3 / 42.2 | 156/183 > 0 |

**Only about half the bias is the probe.** The larger half is `α`: the fill is placed ~25 ms *later*
than the point at which A's two shoulders both register against B. That is production's own version
of the same artifact — its search templates carry the **0.35 s** `border_standoff`
(`patch_region.rs:453`, `gap_borders.rs:195-200`) on top of walk-off and trim, and the B windows it
scores them against are flush at `start` / `start + gap_frames` with no compensation
(`seam_scoring.rs:789-801`). The two shoulders pull the placement in opposite directions by roughly
±(350 ms + their own walk/trim); `α` is where that tug settles, and it settles on the post side.

**Two rival explanations were killed, not argued away:**

- **Gap-edge refinement.** Production records its own refinement (`gap_start_adjust_frames`,
  `gap_end_adjust_frames`); it is non-zero on 68/317 patched gaps. On the 101-gap subset where it is
  **exactly zero** — both paths agreeing on the gap edges to the sample — the bias is unchanged
  (pre −30.4, post −19.6). Code agrees: production's reference `offset_nominal_start` is itself built
  from the refined start (`patch_region.rs:1431-1436`), so the term cancels.
- **Crossfade midpoint.** Default `crossfade_ms` is 10; half of it is 5 ms, not 22.

**Consequences.**

1. **The probe half is now recorded, not inferred. DONE (2026-08-14).** `D` is known for free where
   the templates are built, so `border_edge_displacement_frames` (`gap_borders.rs`, co-located with the
   construction it mirrors) returns it and `lag_at_placement` stamps it per shoulder as
   `LagSummary::border_displacement_ms`. Reading a recorded `frac_lag_ms` against its true zero is
   `frac_lag_ms + border_displacement_ms` on the pre shoulder and `− border_displacement_ms` on the
   post one. It is content-dependent per gap (do not substitute the median), absent on paths that build
   no border templates, and absent on pre-2026-08-14 corpora — which stay readable, because nothing
   about the recorded lag changed. Re-anchoring the B window on the border edge would also have worked
   but silently redefines every historical `frac_lag_ms`.
2. **`α` is the open question, and it is production's, not the probe's.** Correcting `D` removes
   ~11 ms of the ~22; the remaining ~25 ms is the fill genuinely sitting late. The suspected mechanism
   above (0.35 s standoff on both search templates against flush B windows) is a code reading, not yet
   a measurement — it predicts `α` should scale with `border_standoff_secs`, which is testable by
   sweeping that one config value on a handful of gaps. Nothing downstream should treat `α` as a probe
   artifact now that the probe's share is separately accounted.
3. **Neither term is a tolerance budget.** Neither may be spent as one — including in the
   envelope-recenter exit criterion, which currently says the median lag "must not move by more than
   the existing ~22 ms signed bias". A recenter cannot change `D`, and `α` is a property of where the
   fill was placed, so that check is measuring the wrong thing at the wrong size.
4. **S1 is affected as a units bug, not a blocker.** Feeding a raw `frac_lag_ms` into a score feeds
   `true_lag − D_pre` (or `+ D_post`), content-dependent gap to gap. Any threshold calibrated on
   uncorrected numbers bakes the probe's border policy into the gate; S1 should score the corrected
   value.

Two follow-ons this step created, neither of them blocking step 5:

- **17/g34 and 47/g14 want an ear pass.** 17/g34 is a third candidate of the 26/g6 class; 47/g14 is the
  first *failure-to-move* candidate. Both are unlabelled. This is the cheapest way to test whether the
  registration residual orders audibility, which is the question §1 has never been able to answer at
  n=2.
- **The 77.9% slide-variance / leave-one-out-median result** (S2(2)) is the one Route 2 figure still on
  mixed data. It is corroborating rather than load-bearing, so it can wait.

Unchanged by any of this: S2b can proceed in parallel, the envelope-recenter plan is still what
unblocks S1, and S4 is still conditional.

---

## 0. What to actually do (added 2026-08-11)

**Why this section exists.** Through §1–§7 this plan subdivides and branches without ever naming a
change to the code. Phases A/B/C are *diagnostics*, and each has been answered with another fork. The
steps below are the deliverables; the phases are inputs to them, not substitutes. If a step's
precondition fails, the step is dropped — none of them may be replaced with a further investigation.

**S1 — Record the registration residual. No threshold, no veto. ~~Unblocked; do first.~~ Blocked —
its precondition is false (found 2026-08-12).**

> **`lag_decision` is not computed on the production path.** §1.2's "emitted on every Tier-`full` gap —
> 227/227 of the patched corpus" is true *of the dump corpus*, which is a different run than the scan
> reports S1 would write into. `lag_at_placement` is reached only via `build_gap_fingerprint`
> (`measure.rs:1460`) ← `characterize_gaps_from_decode`, whose only production-binary callers are
> `composition.rs:241` (inside `--gap-fingerprints`) and `gap_listen/mod.rs:161`. `GapRepairTags`
> states it outright (`gap_repair_spec.rs:129`): *"production doesn't compute the full D/R set until
> step 8; both spec-build sites use `GapRepairTags::default()`"* — so `registration` is all-`None` on a
> plain repair run, and there is no `frac_lag_ms` to difference against `applied`.
>
> S1 is therefore **"run a new measurement in production"**, not "add a field". **Cost is not the
> obstacle:** `lag_side_sweep` calls `lag_correlation_curve_auto` (`measure.rs:774`), the FFT
> auto-selector, so the ±600 ms / 1 s sweep is two FFTs per gap, not a naive 57 601-lag scan. The work
> is *plumbing* — lifting the shoulder-lag measurement out of `application/gap_fingerprint/` so the
> patch path can call it, and deciding whether it runs on every patched gap or only where a slide was
> applied. Re-scope before starting; do **not** start it as written.
>
> **Re-scope 2026-08-13:** the measurement S1 would plumb is itself blind to pair 31's genuine
> −1.11 s offset (fine window ±600 ms around `b_mapped`; fill search ±10 s). Do not lift the
> nominal-only sweep. Recenter the fine sweep on a coarse envelope peak first —
> [TEMP-lag-envelope-recenter-plan.md](TEMP-lag-envelope-recenter-plan.md) — then S1 records that.

Emit the §2 quantity (`applied − frac_lag_ms`, reduced by minimum absolute value over shoulders with
`peak_r ≥ τ`, abstain when none) as a field on the patch record, alongside the shoulder `peak_r` /
`prominence` / `top2_spacing_ms` it was reduced from. Record-only: no gate reads it. This needs
**neither** δ (§6) **nor** the source/gate verdict (§4) — the measurement is correct to record under
either answer, and it converts "found by ear" into "visible in the output", which is the thing that
actually failed. Precedent: the fill-level fields shipped record-only for the same reason.
*Exit:* the field appears on all patched gaps (**317** on the current baseline, §0.0); the two
known-bad gaps are its extremes.

**S2 — Resolve `max_fill_align_adjustment_secs` (§4.1). ~~Blocked only on confirming it is dead.~~
Confirmed dead 2026-08-12 — unblocked.** Workspace-wide search: the value flows config → `AppConfig`
(`config.rs:118`) → `patch_audio/request.rs:71` and **stops at that field definition — no read, in any
crate**. It is nonetheless validated (`config.rs:922`), CLI-settable (`cli/mod.rs:111`), harness-settable
(`repair-harness/src/patch_audio.rs:332`), and documented as live in `README.md:535` and
`docs/gap-fill-modes.md:562`. The only mention inside a placement-search file is `gap_structure.rs:294`,
a comment *declining* to use it. A documented 0.5 s bound that no code reads is a defect regardless of
this investigation. ~~Two outcomes, both terminal: wire it, or delete it.~~ **Do not wire it as a veto**
— pair 31's genuine −1.15 s placements at #9/#11/#12 are correct and a bare cap would refuse them,
converting two bad repairs into three unrepaired holes. Dual-fit does **not** rescue them: it is on by
default, but its seam-local search is ±600 ms around nominal and those slides are double that
(§4.1b).

> **Re-scoped 2026-08-12 (§4.1a).** `fill_anchor_search_prior_weight` is a **template, not a switch**:
> it is fully plumbed but gated behind `AnchoredRetryPass::Second` + a populated anchor table + a
> non-default `--fill-offset`, and it anchors on the editorial anchor-table prediction rather than on
> nominal — the pair-level drift model §1.1 refuted. Separately, the objective's one hard-coded
> directional prior (`LATE_START_PENALTY`) penalizes **forward** slides only, i.e. the direction that
> §1 says cannot cause this defect. **Extending that penalty to the backward side is the cheaper first
> experiment** than building a new prior. ~~**Do §7's score-term instrumentation first**~~ — **done
> 2026-08-12**: the terms and all three subtractive priors now ship as `status.patched.fit_terms`
> (§7), so an S2 experiment is checkable against a repair report rather than by ear. S2 is unblocked.

> **Settled 2026-08-12 (§4.1c) — the two outcomes collapse to one.** "Wire it as a prior" buys a single
> measured gap (26/g6, a 0.012 margin), does nothing for pair 31 (whose placements are *correct*), and
> would leave a knob named `max_…` that maxes nothing — the field name, its validation and both doc sites
> all promise a hard bound. The prior it would build already exists in better shape as
> `LATE_START_PENALTY`. **So: (1) delete the knob** — field, validation (`config.rs:922`), CLI flag
> (`cli/mod.rs:111`), harness setter (`repair-harness/src/patch_audio.rs:332`), and both doc claims
> (`README.md:535`, `docs/gap-fill-modes.md:562`); **(2) separately, extend `LATE_START_PENALTY` to
> backward slides** and check the effect by diffing `status.patched.fit_terms` over the 227 patched gaps.
> If that needs its own tuning, add `fill_fit_early_start_penalty_scale`, not a seconds-valued cap.

*Exit:* **(1)** `max_fill_align_adjustment_secs` is gone from config, validation, CLI, harness and docs;
**(2)** the backward-penalty experiment has a `fit_terms` diff — baseline vs non-zero scale, **same
binary** — showing whether 26/g6 moves to the −15.8 ms placement and pair 31 #9/#11/#12 do **not** move.
(The 227-gap figure comes from the 2026-08-05 report, which predates `fit_terms`; the baseline run
re-establishes the count.) A null result on (2) is a
real answer — it retires the small-slide-prior hypothesis for 26/g6 and hands it to S2b/§2 — but (1)
lands either way.

**(1) DONE 2026-08-12.** `max_fill_align_adjustment_secs` is gone: the config field, its default, the
`normalized()` copy, the profile-diff row, the `< 0.0` validation, `PatchAudioRequest`'s field, the
`--max-fill-align-adjust-secs` flag and its help default, the harness `PatchTestOptions` field and its
two fixture overrides, the `repair.toml` test fixture line, the README flag-mapping row and config
sample, and the `gap-fill-modes.md` table row. `gap_structure.rs:294`'s comment — the one place that
*mentioned* the knob, to decline using it — now records the constraint (a small fixed polish clamp,
because driving it from a configurable span caused multi-second per-candidate loops) without naming a
symbol that no longer exists. Workspace builds, full test suite, clippy and fmt all green; no behaviour
changed, because nothing read the value.

**(2) IMPLEMENTED 2026-08-12 — shipped inert, awaiting the sweep.** `LATE_START_PENALTY` now applies in
both directions, with the two sides independently scaled: `fill_fit_late_start_penalty_scale` (forward,
default **1.0**, unchanged) and the new `fill_fit_early_start_penalty_scale` (backward, default
**0.0 = off**). Plumbed config → `PatchAudioRequest` → `UnifiedFitWeights`, plus the harness
`PatchTestOptions`; config-only, no CLI flag. One term, one reported field — both directions land in
`fit_terms.late_start_penalty`, and `align_adjustment_secs` already carries the sign, so splitting the
report would have bought nothing and broken the reconstruction identity the unit test guards.

*Why inert by default.* The baseline run and the sweep runs must be the **same binary and the same
commit**, differing only by config value — otherwise every difference is confounded with the fortnight
of other changes since the last corpus report (normalize boost-only, `waveform_adjustment_secs`
removal, `fit_terms` itself). A `0.0` default is what makes the baseline comparable. The unit test
`early_start_penalty_is_inert_at_default_and_mirrors_the_late_one` pins exactly that: the term fires
backward when opted in, scales linearly, leaves the forward side untouched, and is **exactly** zero at
the default.

*The tension the sweep has to resolve.* `offset_frac` is normalized by `gap_frames` and is **not
clamped**, so a slide longer than the gap charges more than `LATE_START_PENALTY`. One term is being
asked to do two opposing things: be large enough to close 26/g6's **0.012** margin, and small enough
that pair 31 #9/#11/#12 — genuine −1.15 s placements, correct and clean by ear — still win. At large
scales an unbounded proportional penalty *is* a soft veto, the instrument §4.1 rules out. Whether it
needs a cap, and whether `gap_frames` is even the right normalizer on the backward side (where the risk
scales with the **repeat window**, not the gap), are questions for the corpus, deliberately not guessed
at in a default.

**Next: the runs.** (a) a fresh baseline at scale `0.0` — also the first corpus report to carry
`fit_terms` at all, and the re-validation of S2c's `energy`/`bool` split, both of which currently rest
on a 2026-08-05 report; (b) the same binary at a few non-zero scales, diffed against (a). These are
repair-report runs (`--patch-only --format json`), not fingerprint runs — no per-bracket oracle cost, so
several scale values are affordable.

##### (2) SUPERSEDED 2026-08-13 — the sweep is refuted, and nominal is the wrong origin

Baseline (a) completed: `gap-files/archive/mixed-ab/2026-08-12-fit-terms-baseline/`, **39 pairs, 227 patched gaps, 147
carrying `fit_terms`** (the other 80 are dual-fit, which sets `fit_terms: None` and reports
`align_adjustment_secs == 0.0` by construction — verified 80/80). Sweep (b) **should not be run.**

> **Re-based 2026-08-14 (§0.0).** That baseline is the mixed-orientation set. Replaced by
> `gap-files/2026-08-13-repeat-terms/` — **53 pairs, 317 patched gaps, 210 with `fit_terms`**,
> policy-identical (all 119 shared placements bit-identical), so **the refutation below is unaffected**:
> it rests on 26/g6 vs 31/g8 / 31/g11, all correctly-oriented pairs, and their values are unchanged.
> **Route 2's consensus statistics further down are withdrawn** — half its load-bearing cell was swapped.

**The sweep is refuted by arithmetic, before any config value.** The penalty is
`LATE_START_PENALTY × offset_frac × scale` with `offset_frac = |offset_frames| / gap_frames`, so the
ordering between gaps is scale-invariant. At *every* scale, 31/g8 (`offset_frac` 0.1428) and 31/g11
(0.1470) are charged **strictly more** than 26/g6 (0.0465) — roughly 3× — so no scale closes 26/g6's
0.012 margin without hitting the three must-not-move placements harder. This is the tension §4.1c
flagged, resolved against the instrument. The knob stays shipped and inert; it is not the fix.

**Why: the penalty measures the slide against the wrong origin.** Referencing `|align_adjustment_secs|`
to **nominal** assumes nominal is where a correct fill sits. It isn't. Per-pair, slides are coherent:
77.9% of slide variance is explained by pair identity (19 pairs / 106 unified gaps) against a
shuffled-label null of 17.0% (p95 25.7%), p < 0.0005. Re-reference each gap to the **leave-one-out
median slide of the other unified gaps in its pair** and the separation inverts —
26/g6 residual −1.4541 → penalty 0.0466, versus 31/g7 0.0097, 31/g8 0.0013, 31/g10 0.0007,
31/g11 0.0014: a 5–68× separation, in the right direction, with no tuning. Ranked by residual rather
than by `|adj|`, pair 31's only flagged gap is **31/g7** — which §4.2 independently identifies as the
`min`-reduction defect (S2b), not a placement-prior defect.

**Route 2 — the consensus is independently confirmed, so this is not the search agreeing with itself.**
**⚠ Superseded 2026-08-14 — rebuilt on the oriented corpus (§0.0 "Step 3 result"): the claim holds at
34/34 in-window pairs and a 9-pair load-bearing cell `{2, 3, 13, 14, 18, 22, 32, 47, 51}`. The numbers
below are the mixed-corpus originals, kept for provenance; read the rebuilt ones instead.**
The obvious objection is circularity: `align_adjustment_secs` is the search's own argmax, so a
consensus built from it cannot justify it. Joined against `lag_decision.frac_lag_ms` from the
2026-08-08 dumps (`measure.rs:1662`, Full tier) — a waveform-correlation lag measured at `b_mapped`
**nominal**, which reads neither the fill search, the unified objective, nor any other gap. Joined on
`geometry.a_start_secs`, never on gap index; 227/227 joined, 35 pairs.

- **26 of 26 in-window pairs agree within 50 ms.** Load-bearing cell (`|consensus| ≥ 150 ms`, where
  agreement is not free): **6/6** — pairs 3, 6, 13, 18, 22, 32, e.g. 18 `+0.3308` vs `+0.3197`,
  22 `−0.2458` vs `−0.2469`.
- **Nominal is wrong by ~13× on the large slides.** For `|adj| > 150 ms` (n=33), median `|adj|`
  0.2713 s → median `|adj − own measured lag|` **0.0210 s**.
- **29 of 32 gaps in the decisive cell are confirmed correctly registered** (big `adj`, small residual —
  the placements a nominal-referenced penalty would have moved), 3 near, **zero contradicted**.
- **80 dual-fit gaps corroborate independently.** They carry no slide at all, so their lag is
  uncontaminated by construction; their per-pair medians sit on the same offsets (pair 18 `+0.3150`
  vs consensus `+0.3308`; pair 22 `−0.2302` vs `−0.2458`).
- **26/g6 is confirmed misplaced without reference to any label**: measured lag `−0.041 s` against a
  `−1.450 s` slide. Caveat (§4.3's territory): its `second_peak_r` is 0.972 against `peak_r` 0.997 at
  0 ms spacing and `peak_z` is only 6.1, so the lag curve has a near-tie — this corroborates the
  ±2000 ms sweep rather than adding an independent strong measurement.

*Reduction discipline (the first cut of this join got it wrong).* Both shoulders are absolute lags on
one scale — post is **gross** (`L_pre + L_post_fine`, gap-fingerprint.md:720-739), so its magnitude
guard is ±1200 ms, not ±600, and averaging the two shoulders is invalid whenever `step_ms` is large
(3/g9: pre +10 ms, post +354 ms, mean +182 ms — a number describing neither). Pair offsets are
estimated by pooling **confident** shoulders across the pair; big-step gaps are reported, not reduced.
Under the fix, every previous "disagreement" resolved: pair 12 went −113.8 ms → −29.2 ms, and the
decisive cell went 31/38-with-7-exceptions to 29/32-with-none. The load-bearing cell agrees under all
five reductions tried (pooled / mean / pre / min-abs / best-r), so the result does not rest on the
choice. `peak_z` (≥12 ≈ unique, gap-fingerprint.md:725) is the correct addressability gate and is
stricter than the `peak_r ≥ 0.5` used here — re-gating on it is free and re-sorts the suspect list.

**Still open, and bounding the above.** (i) A **signed −23.4 ms bias**: pooled lag ≈ consensus − 23 ms
on 24 of 26 pairs, in both slide directions, surviving every reduction. Not fine polish
(`structure_fine_polish_frames` caps at 128 frames ≈ 2.7 ms); most likely a window-centring convention
between the 1 s lag template and the seam the objective scores. **This must be explained before the lag
becomes a scoring input** — it is fine as a diagnostic today. (ii) **Pair 31 remains unvalidated by this
route**: its −1.109 s consensus is outside the ±600 ms sweep, and its late-half gaps return a *flat*
lag curve (`second_peak_r ≈ peak_r`, `prominence ≈ 0`, `peak_z` 3.6–5.4) while B is not silent
(rms −37 to −51 dB, `silence_fraction` 0.00) — the signature of a true offset outside the window.
Pair 31 splits cleanly in time: measured at ~0 lag with `peak_r` 0.83–0.99 up to t≈1726 s, unmeasurable
from t≈2852 s on. That is a **step**, now supported by independent measurement rather than by the
dual-fit zeros §1.1 had to retract. (iii) Six gaps where a coherent measurement contradicts the slide:
26/g6, 3/g2, 12/g4, 10/g1, 19/g2, 16/g11 — of which only **12/g4** (`peak_z` 44.0) and **16/g11**
(21.7) clear the `peak_z` bar.

*Next, in order:* re-gate the join on `peak_z ≥ 12` (free); read the widened-envelope measurement plan
([TEMP-lag-envelope-recenter-plan.md](TEMP-lag-envelope-recenter-plan.md)) before any pair-31 run; then
explain the −23 ms bias by reading `lag_at_placement`'s window construction against the objective's
seam window. The backward-penalty sweep is **not** on this list.

##### (3) `peak_z ≥ 12` re-gate — 2026-08-13, the result survives at a third of the coverage

> **⚠ Superseded 2026-08-14 — rebuilt (§0.0 "Step 3 result").** The gate's cost/benefit reproduces on
> the oriented corpus (30/31 in-window pairs agree, cell 8/8, 10.0× big-slide collapse), so the
> conclusion stands; the counts below are the mixed-corpus originals. The suspect list **is** replaced:
> 12/g4 and 16/g11 are from swapped pairs and do not exist in the production orientation. The
> *structural* read at the end stands and got stronger — pair 31's `. . X X X` reproduces exactly, and
> **pair 50 is a second instance** (`. . . . X X X X X X`, consensus +2.79 s).

Re-ran the join with `peak_z ≥ 12` replacing `peak_r ≥ 0.5` as the confidence gate. The cost is
steep and the conclusion does not move.

*Cost.* 162 of 454 shoulders drop (432 → **270** kept); gaps with **no** usable shoulder go 4 → **58**
of 147. Per-pair coverage falls from 26 in-window pairs to **22**, and section B's per-gap cell from
n≈120 to **n=82**.

*What survives.* 22 of 22 in-window pairs still agree within 50 ms (`AGREE` on every row). The
load-bearing cell (|consensus| ≥ 150 ms) is 5 pairs — 3, 6, 13, 18, 32 — and **5/5** agree; pair 22
left the cell for lack of a confident shoulder, not by disagreeing. Per-gap: |adj| against nominal
median 0.0532 s vs |adj − own measured lag| **0.0238 s**; on the big-slide subset (|adj| > 150 ms,
n=16) 0.2623 s → **0.0260 s**, a 10× collapse. The decisive cell is **14/16 confirmed** with the two
misses (13/g34 +64.6 ms, 32/g9 +76.2 ms) *near*, not contradicting. All 15 dual-fit pairs still
corroborate.

*What the gate re-sorts.* The suspect list drops from six to **two** — 12/g4 (+188.8 ms, `peak_z` 44.0)
and 16/g11 (+112.9 ms, 21.7) — exactly the two predicted. **26/g6 is no longer a contradiction**: at
`peak_z` 6.1 it is now *unmeasurable*, which is the honest reading of a `second_peak_r` 0.972 near-tie.
3/g2, 10/g1 and 19/g2 leave the same way. So the only genuinely coherent disagreements in the corpus
are 12/g4 and 16/g11, and 16/g11 carries a 99 ms step.

*What it does not fix.* The signed bias is unchanged and slightly larger: median **−24.8 ms**, negative
on **20 of 22** pairs. It is not a low-confidence artifact.

*A structural read of the failures.* The 58 no-confidence gaps are not uniform — they cluster at pair
edges and in runs (pair 5 `. X X X . . . .`, pair 13 `. . . . . X X X . .`, pair 31 `. . X X X`).
Pair 31's is the plan's step: the last three gaps are exactly the ones the ±600 ms window cannot see.
This is direct support for the envelope-recenter plan and against any fix that only tightens gates.

**S2c — `post_correlation` carries a structure score on snapped Bool gaps (found 2026-08-12 reviewing
the §7 instrumentation). ~~Not a blocker for S2(2)'s energy-gap `fit_terms` diff — see the corpus split
below — but **land before reading `seam_post` against `fit_terms.waveform_min` as if they share a
domain.**~~ **DONE 2026-08-12.**
~~`gap_structure::snap_structure_fill_to_gap:223` does `alignment.post_correlation = snapped_post`, where
`snapped_post` comes from `score_post_match` — a **structure** match score.~~ (That write is gone —
snap only mutates `fill_frames` now.) Everywhere else `post_correlation` is a waveform seam Pearson:
`patch_region.rs:1561` reports it as `seam_post`, and `:2123` casts it as the gate's post-seam input.
So on a snapped gap the gate's post check **was** made against a quantity from the wrong domain, while
`fit_terms.waveform_min` stayed honest (it is taken from the seam pair, not from `alignment`). That
disagreement was type confusion on a shared field, not a schema gap.

*Confirmed live, not theoretical.* Over the 227-patched-gap corpus report, one Bool gap reports
`post_correlation: 1.0` exactly, against `pre_correlation: 0.270` (`confidence: marginal`). A waveform
Pearson of exactly 1.0 on real audio is essentially impossible; a structure score of 1.0 is
unremarkable. That gap's post-seam check passed on a number that is not a seam correlation. ~~**Re-check
that gap on the next corpus report**~~

> **Re-check done 2026-08-14 — the gap is 32/g3, and it re-adjudicated.** Pair 32 is correctly oriented
> in both runs, so this is a clean before/after. Pre-fix (`2026-08-07-fill-level-shape`): pre 0.2703 /
> post **1.0000**, `marginal` / `marginal`. Post-fix (both the 08-12 baseline and the oriented
> `2026-08-13-repeat-terms`): pre 0.6053 / post **0.4069**, `high` / `high`. So the field now carries a
> real seam Pearson, and the gap passes *more* strongly than before rather than failing — the structure
> `1.0` was not what was carrying it. Note `pre_correlation` moved too (0.270 → 0.605), which the fix's
> stated scope does not predict: snap's write was evidently also changing the placement the pre-seam was
> scored at, not only mislabelling the post field. Worth a look if `snap_fill_to_gap` is touched again;
> n = 1, and Bool is 2 of 317 gaps, so not worth chasing on its own.

*Why it did not block S2(2).* `snap_fill_to_gap` is a **no-op outside the Bool signature**
(`gap_signature.rs:227` matches only `Bool`/`Bool`), and the corpus is overwhelmingly `energy`:

| `signature_mode` | patched gaps (mixed, 227) | **oriented, 317** |
|---|---|---|
| `energy` | 225 | **315** |
| `bool` | **2** | **2** (32/g3, 48/g2) |

So S2c reached at most 2 of 227 gaps (0.9%) — **2 of 317 (0.6%) on the oriented corpus**, the rate
holding as the corpus grew — and 26/g6, S2's exit criterion, is `energy` (§3). An S2 `fit_terms` diff
over the energy gaps cannot be confounded by it.

> **Settled 2026-08-12 — stop snap from writing correlation fields.** This is accidental dual meaning
> *on the unified path*, where `post_correlation` is supposed to stay waveform. (The intentional dual
> meaning under structure-trust — `GapPatchStatus` already says "structure or waveform Pearson" — is a
> separate, documented case; do not broaden that contract further to paper over snap.)
>
> **Preferred fix (landed):**
> 1. In `snap_structure_fill_to_gap`: keep the accept/reject logic on `snapped_post`, but **only set
>    `alignment.fill_frames`**. Do not assign `post_correlation`.
> 2. On the **structure-only** call site (where pre/post are already structure scores), re-score after
>    snap if accepted: `alignment.post_correlation = score_post_match(..., start + fill_frames, ...)`.
> 3. On the **unified** path: do nothing else — `wave_pre` / `wave_post` stay; `structure_post` is
>    already re-derived after snap in `gap_fill_fit.rs`.
>
> That restores one meaning per path without new fields, without re-plumbing `FillAlignment`, and
> without touching S2/S2b. Rejected alternatives: restore `wave_post` after snap in unified only
> (works, leaves the landmine in snap); add a separate report field / rename (overkill for n=2 —
> structure after snap already exists as `structure_post`); re-measure waveform post at the snapped end
> (different seam semantics — don't fold that in here); doc-only (insufficient — the gate reads the
> same field).
>
> **Safety:** unit test `snap_does_not_overwrite_waveform_post_correlation` plants a waveform
> `post_correlation`, forces snap to accept on a higher structure score, and asserts `fill_frames`
> snaps while `post_correlation` is unchanged.

*Exit:* **met** — `snap_structure_fill_to_gap` never writes `pre_correlation` / `post_correlation`; on
the unified path those fields remain waveform Pearson through report and gate; the structure-only path
re-scores post after a successful snap. Corpus re-adjudication of the previously `1.0` Bool gap waits
on the next report (same run as S2(2)'s baseline).

**S2b — Fix the `min` reduction when one shoulder is unmatchable (§4.2). 31/g7's actual defect.**
**Plan: [TEMP-unmatchable-shoulder-plan.md](TEMP-unmatchable-shoulder-plan.md)** — draft, decisions
open, not started. Do not implement from this paragraph.
31/g7 has no placement where both shoulders score well (its post border has no match in B: dual-fit
reaches 0.124 at its own best lag and declines). Reducing by `min` then *prefers* a placement that
degrades the good shoulder from 0.829 to 0.437 because it raises the minimum. That is a
**scoring-objective** change, distinct from S2's prior. The child owns the options (fallback vs
abstain), the unmatchable predicate, waveform-vs-structure scope, the fit-mode floor, and the
must-not-move set.
*Exit (held in the child):* 31/g7 places at ≈ −47 ms **and still patches**; checked against the
**317**-gap oriented corpus (§0.0) for placements it moves.

**S3 — ~~Find or refute~~ the rival peak at −977 / −1450 ms (§4.2). DONE 2026-08-12 — REFUTED on both.
No re-run needed, and none is now warranted.** Full curves and method in **§4.3**. Headline: at both
failures the pre-shoulder's true-registration peak is the tallest thing in a ±2000 ms sweep, and the
predicted rival is **0.214 lower (26/g6)** and **0.742 lower (31/g7)**. The §7 widened-`lag_max_lag_ms`
re-run is **retired as an experiment** — it would confirm a settled answer at the cost of a full
per-bracket oracle pass. Its second, independent half (`edge_pinned` for §1.2's saturation hypothesis)
is dump-only and survives.
*Exit:* met — rival refuted at both, with heights recorded against the true-lag peak (§4.3).

**S4 — Phase B's render (§5), only if S1–S3 leave a decision that labels would change.** This is the
expensive step and the plan has reached for it repeatedly without establishing what it would decide.
Its honest current value is the `applied = 0` cohort question, not δ.

**S5 — Close the reporting foot-guns this investigation tripped over (2026-08-12). Independent of
S1–S4; each sub-item lands alone.** Every wrong turn in §3.3b/§4.2 came from reading the patch record
as if field names were the schema. The patch record carries **no `_contract` block at all** — the
system exists only in the fingerprint dump (`application/gap_fingerprint/contract.rs`, eight groups).

- ~~**Delete `waveform_adjustment_secs`**~~ **DONE 2026-08-12.** Removed from `GapPatchStatus::Patched`
  and `RegionPatchOutcome::Patched`, both emit sites collapsed to
  `align_adjustment_secs = (alignment_start − offset_nominal) / sr` (one subtraction, not a sum of two),
  and `format_patch_slide_suffix`'s unreachable `(wf …)` branch deleted. Two further dead artifacts fell
  out with it: `ExecuteBracketOutputCtx.structure_start_frame` (carried *only* to compute the split) and
  the verbose log's `structure slide … , waveform slide …` pair, now one `slide`. `docs/json-output.md`
  records the removal. **`structure_combined` / `waveform_min` were NOT emitted in its place — they are
  not in scope.** `unified_fit_score` (`gap_fill_fit.rs:399`) computes both as locals and returns a bare
  `f64`; `FillAlignment` (`policies/gap_borders.rs:15`) carries only `start_frame` / `fill_frames` /
  `pre_correlation` / `post_correlation`, so not even the winning scalar survives the search. Emitting
  the terms means changing that return type and threading them through — a separate change, not a
  deletion. Filed as its own §7 item.
- ~~**Always serialize `dual_fit_used` and `anchor_seam_used`**~~ **DONE 2026-08-12.**
  `skip_serializing_if = "is_false"` dropped from both bools on `GapTags` *and* on
  `GapPatchStatus::Patched`; `false` now always appears and means *declined / baseline*, never
  *disabled* — the error that produced the first version of S2b. The golden fixture
  (`tests/fixtures/full_surface_repair.json`) was revised deliberately at three insertion points, both
  `is_false` helpers went with it, and `docs/json-output.md` replaced its "omitted when false"
  sentence. Landed alongside a `LAG_DECISION_CONTRACT` fix in the dump: its `not` key now says
  *"prominence is not uniqueness — no exclusion radius, see `top2_spacing_ms`"*, the misreading §3.3b
  made.
- ~~**Contract `status.patched`.**~~ **Moved 2026-08-12 to [BACKLOG.md](../../BACKLOG.md) §
  "Fingerprint contract follow-ups (C3)"** — it is a reporting-system question that outlives this
  investigation, and it sits next to the deferred `--fingerprint-contracts` knob because both wait on
  the same measurement. Two obstacles, not just unwritten code: `Contracted<T>` needs a `T` to flatten
  and `Patched` is an enum variant with inline fields in a *domain* type (`contract.rs` deliberately
  keeps reader metadata out of those); and per-gap stamping that costs nothing in a one-gap dump file
  repeats four strings 227× (~90 KB) in a whole-corpus report, while the cheap root-level `_contracts`
  map is exactly what § 2.6 rejected. The fields that want the contract are recorded there verbatim.
- ~~**Emit `normalize_gain` on the patch record.**~~ **Moved 2026-08-12** into
  [TEMP-fill-normalize-boost-only-plan.md](archive/TEMP-fill-normalize-boost-only-plan.md), which edits the
  site that computes it. It belongs to S5's family — the gain is on the spec
  (`gap_repair_spec.rs:197`) but serialized nowhere, so [pipeline.md](../pipeline.md) § Fill-level
  check can say `fill_level` measures `gained_fill` (*gain applied*) while nothing records what the
  gain was — but it is not this plan's to land. Tracked here only so the foot-gun audit stays complete.
- ~~**Contract `prominence` in `lag_decision`.**~~ **DONE 2026-08-12**, folded into the bullet above:
  `peak_r − second_peak_r` carries **no exclusion radius** (`measure.rs:366`), so the `not` key now
  points the reader at `top2_spacing_ms` (§2.2 withdrew a conclusion for want of it). Both edited
  strings stay under `CONTRACT_MAX_LEN`, and `not` still opens with the sibling wire key
  `lag_editorial` that `every_not_names_a_sibling_first` requires.

*Exit:* **reached 2026-08-12.** Three bullets landed (`waveform_adjustment_secs` deleted, both bools
always serialized, `lag_decision`'s `not` fixed); two were declined here with a reason and re-filed
where the work belongs — `normalize_gain` to the normalize PR, `status.patched` to BACKLOG § C3. No
new measurement was required for any of them, and none of it touched S1–S4.

**Not doing yet:** δ calibration (§6) — n = 2 cannot fix a threshold, and S1 makes waiting free; a
prominence floor (§2.2 constraint); anything corrective from the residual (§2.1).

**Sequencing against the normalize boost-only change (settled 2026-08-12).**
[TEMP-fill-normalize-boost-only-plan.md](archive/TEMP-fill-normalize-boost-only-plan.md) stops
`compute_fill_gain` from ever attenuating. **It does not block S1 / S2 / S2b / S3 / S5** — the gain is
computed at `region.rs:2056`, strictly *after* the placement search and seam reconciliation, its only
consumers are `gained_fill` and `spec.normalize_gain`, and seam scores are Pearson (scale-invariant),
so this plan's whole axis is gain-invariant. What it does change, confined to fills currently
attenuated (whole-fill RMS above A-border RMS; boosted fills untouched):

- `peak_delta_db` / `edge_delta_db` on that subset — the fill-level roll-up (now
  `gap-files/2026-08-13-repeat-terms/`; the mixed `archive/mixed-ab/2026-08-07-fill-level-shape/` is
  not a live baseline) becomes a different-policy corpus. (Nothing in §0–§7 reasons from those
  fields, so no conclusion here moves.)
- **§5 / S4's ear labels.** `--gap-listen` renders through the normal patch path, so `normalize_fill`
  applies to `_a_patched`: a loud-fill gap rendered after the change is different audio from one
  labelled before it. Existing `archive/mixed-ab/2026-08-09-drift-listen/` renders remain valid as
  files for the correctly-oriented pairs (18, 24, 25, 26, 31, 37); pairs 3 and 6 were reversed and
  do not transfer. A re-render will not match its own label.

**Decision: the normalize change ships first, before S4 renders anything new** — S4 is conditional and
has not started, so landing it first keeps the forward label set under one policy rather than
straddling it. If S4 is ever run *before* that change lands, record which policy each label was taken
under. That PR also carries the `normalize_gain` emit (folded in 2026-08-12), so by the time S4 runs
the affected subpopulation is visible in the patch record — which is what makes any before/after
fill-level comparison readable at all.

---

## 1. The defect

**The corpus's only ear-confirmed wrong repairs: pair 26 gap 7 and pair 31 gap 8.** Both are
shoulder duplications — the fill leads with a repeat of the material immediately before the gap —
and both were accepted at `patch_tier: high` / `confidence: high`. Found by ear, not by any gate.

**The mechanism.** Each fill carries an `align_adjustment_secs`. Measured against the *true* local
registration — envelope Pearson over the 2.6 s of pre-gap context in the rendered `_a_surround` /
`_b_surround` clips, 10 ms bins, r 0.929–0.998 on all 12 — the adjustment is **correct on 10 of
12**, faithfully tracking a real misregistration that ranges from 0 to −1.16 s:

| | \|applied − true registration\| |
|---|---|
| the 10 clean-by-ear gaps — **9 informative, see §2.3** | ≤ 0.056 s (median 0.038) |
| 26/#7 (stutter) | **1.410 s** (applied −1.450, true −0.040) |
| 31/#8 (stutter) | **0.937 s** (applied −0.977, true −0.040) |

No overlap; a 17× margin. (**The "10 of 12" is this measurement's own count and stands**, but 3/g9 is
later reclassified *uninformative* — §2.3 — so only **9** of them can corroborate anything. Every
downstream count in this document is 9.)

Both failures slid **backward** where the shoulders were already
registered. A backward slide with no drift to correct lands the donor window on material A has
already played — a shoulder repeat by construction. A forward slide cannot duplicate. So the
defect is not the size of the adjustment, it is that **a large displacement is accepted without
being checked against what the local shoulders say the offset should be.**

### 1.1 What this refutes

The previous reading of this item — "a pair-level drift the per-gap search re-discovers instead of
correcting once" — is wrong, and acting on it would make things worse. Pair 31's ≈ −1.11 s at gaps
9/11/12 is *genuine*: those gaps really are misregistered by −1.15 s and are correctly placed and
clean by ear. Re-fitting the pair alignment once would break them and would not help #8, which needs
≈ 0. Within pair 31 the true registration *steps* from −0.04 s at #8 (28 min) to −1.15 s at #9
(86 min); #8's failure is the post-step value applied one gap early.

Also refuted as discriminators, each by a labelled counterexample:

| Refuted signal | Counterexample |
|---|---|
| seam correlation, `splice_dualfit.gate_pass` | pair 31's broken seams sound fine; clean-seam gaps stutter |
| `confidence` / `patch_tier` | both `high` on both failures |
| donor-envelope self-similarity | 26/#7 = 0.366 vs clean median 0.160 — wrong direction |
| spectral flux | 26/#7 = 2.91, inside the clean range 1.13–3.50 |
| correlation gain (`post − pre`) | 31/#8 flat (+0.0001) and *bad*; 31/#12 *falls* 0.574 → 0.376 and is clean. Corpus-wide 67% of patched gaps move it by < 0.001 — not a quality measure. ~~7/#7 (0.993 → 0.354, "clean")~~ **withdrawn 2026-08-14**: pair 7 was reversed; that label does not transfer |
| `\|align_adjustment\| / gap_length` as a screen | it is what put the four clean pair-31 gaps at the top |

### 1.2 Correction: the pipeline already computes local shoulder registration

Earlier text here — "nothing in the pipeline computes the latter today" — was **wrong**, and it sent
the search toward building a new envelope correlation that already exists.
`lag_decision.pre_anchor[0]` / `post_anchor[0]` carry `frac_lag_ms` (sub-sample refined), measured
over a 1000 ms shoulder window at ±600 ms max lag, with `peak_r` / `peak_z` / `prominence` /
`second_peak_r` / `edge_pinned` attached. Emitted on every Tier-`full` gap — 227/227 of the patched
corpus, no gaps missing. Commensurability with `align_adjustment_secs` is empirical, not assumed: on
the **8 labelled-clean gaps that have a confident shoulder** the two agree to ≤ 33 ms, same sign
convention.

> **Corrected 2026-08-12.** This read "on the 10 labelled-clean gaps the two agree to ≤ 33 ms", which
> **contradicted §2's own table** and was the stronger claim of the two. Only 8 of the 10 have a
> shoulder at `peak_r ≥ τ`; the other two (31/g8, 31/g11) are clean by ear yet disagree by ≈ 1100 ms,
> and §2 excludes them by confidence alone. Commensurability is therefore established on **8** gaps,
> not 10 — and after §2.3 the informative clean cohort is **9**, so the two numbers are not the same
> subset either. The claim survives at the size that was actually measured; nothing downstream rested
> on the larger count.

`equivalence_production.donor_registration.lag_ms` measures the same thing at 100 ms bins and agrees
on 9 of 12; it reads −1000 ms at 31/#8 against a true −40 ms. **Cheap hypothesis to check:** pair
31's true registration at gaps #9–#12 is ≈ −1150 ms, outside the ±600 ms search — so the anomaly may
be *saturation*, not a bug, and `edge_pinned` on the `lag_decision` anchors is the field that would
say so. If confirmed the fix is range/saturation detection, not debugging the estimator. Either way
`lag_decision` supersedes it as the guard input: finer, sub-sample, already carrying confidence terms.

---

## 2. The candidate: registration-residual guard (measured 2026-08-10)

**Definition.** For each shoulder in `lag_decision` with `peak_r ≥ τ`, take
`applied − frac_lag_ms` where `applied = status.patched.align_adjustment_secs × 1000`. Reduce by
**minimum absolute value** over those shoulders — a placement is vindicated if *any* trustworthy
shoulder agrees with it — then apply a **signed backward** threshold `residual < −δ`. No confident
shoulder ⇒ abstain (never veto).

**Separation on the 12 labelled gaps** (τ = 0.8):

| | residual |
|---|---|
| 8 labelled clean, confident shoulder | ≤ 33 ms (but see §2.3 — 3/g9 does not corroborate) |
| 2 labelled clean, no confident shoulder | abstain |
| **26/g6 (stutter)** | **−1390 ms** |
| **31/g7 (stutter)** | **−930 ms** |

28× margin. The confidence gate is load-bearing, not cosmetic: 31/g8 and 31/g11 are *clean by ear*
yet show ≈ 1100 ms disagreement, and are excluded only because their best `peak_r` is 0.075–0.152.
Both abstentions are clean gaps, so abstain lands in the safe direction.

**Corpus-wide firing rate** (227 patched gaps). **⚠ Withdrawn 2026-08-14 (§0.0) — mixed-orientation
corpus, and it needs a `lag_decision` join, so it recomputes as part of step 3. The labelled-gap
separation above survives the loss of the two swapped-pair labels (pairs 3 and 6): both were *clean*
labels, so removing them shrinks the clean cohort and cannot weaken the margin — both failures and the
abstention cases are pairs 26 and 31.**
Symmetric `|residual| > 150 ms` fires **30/227** —
but 26 of those 30 have `applied = 0`, a different population (no local slide applied where the
shoulders do see drift), not a misplacement. Signing the residual separates them:

| backward δ | fires |
|---|---|
| 150 ms | 20 |
| 300 ms | 9 |
| **500 ms** | **2 — exactly the two known-bad gaps** |
| 800 ms | 2 |

Natural break in the data is −430 → −930 ms. Forward fires at the same δ: 10 / 5 / 1 / 0.

### 2.1 Direct audio verification of the mechanism (2026-08-10)

The displacement was measured against the rendered clips rather than inferred, by locating the fill
inside `_b_surround` (r = 0.984/0.994) and independently locating A's 1 s pre-gap shoulder inside
`_b_surround`, then differencing. Both failures are confirmed **backward in B**, so the direction
half of the mechanism holds.

| | predicted residual | measured B-side displacement |
|---|---|---|
| 31/g7 | −930 ms | **−929 ms** (exact) |
| 26/g6 | −1390 ms | −894 ms — **superseded**: an alias, not a real 496 ms shortfall |

**The guard's residual is not the physical displacement.** It agrees exactly on 31/g7 and overstates
26/g6 by 496 ms. `applied` is evidently not a pure slide of the delivered fill — seam trim /
dual-fit / crossfade plausibly absorb the difference. This does not weaken the guard (it is a
*discriminator*, and the ranking is unaffected) but the residual must not be reported to a user as
"the fill is off by N ms", and any future *corrective* use — sliding the fill by −residual — is
unfounded until this is understood.

**`applied` tracks delivered position within ~±70 ms where the measurement is trustworthy.**
Predicted residual vs measured B-side displacement initially appeared to diverge on two
(26/g6 +496 ms, 3/g9 −749 ms), suggesting decision space and delivery space were distinct. A
peak-uniqueness screen (`r1 − r2`, second peak outside ±250 ms) explains both and **withdraws that
reading**: on the 9 gaps with a unique match the delta is +1…+68 ms, no exceptions. Do not treat
`align_adjustment_secs` as unmoored from the delivered fill.

- 31/g8 margin 0.047 at r1 0.078 — measurement failed outright; its −1201 ms is meaningless. Same
  gap the confidence gate already abstains on, from independent evidence.
- A small **systematic positive bias** remains (all 9 deltas positive, median ≈ +39 ms), most likely
  the diff-based fill-start detector catching the leading crossfade ramp. Cosmetic for a
  discriminator; worth pinning before any absolute claim about delivered position.

### 2.2 Ambiguity, and why no prominence floor

26/g6's margin is **0.083** (r1 0.994 / r2 0.911) — an alias, consistent with material self-similar
on a ≈ 2.9 s cycle; the +496 ms was measurement error, not pipeline behaviour. ~~**Independently confirmed from the dumps, no WAV needed:**~~ **Withdrawn 2026-08-11.** The claim
was that `lag_decision.prominence` (= `peak_r − second_peak_r`) confirms the ambiguity from the dumps:
26/g6 scores 0.025 against a floor of 0.282 across the other labelled gaps, `pre_anchor` being
`peak_r 0.997 / second_peak_r 0.972` — read as "a near-perfect match in *many* places". It does not
say that. `secondary_peak` (`gap_fingerprint/measure.rs:366`) takes the tallest competing local
maximum **anywhere in the curve with no exclusion radius**, and 26/g6's runner-up sits at
`top2_spacing_ms` **0.104** — five samples, the same lobe. So its `prominence` measures peak
width/ripple, not long-range uniqueness. Always read `prominence` together with `top2_spacing_ms`;
alone it conflates a broad peak with a periodic one.

26/g6's aliasing therefore rests on the WAV measurement (margin 0.083, rival outside ±250 ms, ≈ 2.9 s
cycle), which stands. The dump-only shortcut does not.

So **`peak_r ≥ 0.8` alone is an insufficient confidence gate**: 26/g6 passes at 0.997 while being
maximally ambiguous. "Matches well" and "matches uniquely" came apart on one of the two failures.
The guard caught it regardless (residual −1390 ms), so this is **not a hole**.

> **Constraint (checked 2026-08-11): do not add a prominence floor to any gate yet.** Two prior
> rulings apply. (a) [Dual-fit confidence axis](../../BACKLOG.md#dual-fit-confidence-axis):
> uniqueness stays *diagnostic* until a **labeled false positive** exists — 26/g6 is a labeled
> *true* positive of the residual guard, not a false positive of the uniqueness axis, so the trigger
> has not fired. (b) `analysis.rs:314` records that the analyzer gates on `peak_z`, not `prominence`.

Note the two "prominence" quantities are **not** the same and the `anchor_seam_min_prominence`
refutation does *not* transfer: that one is `peak − local_median` in energy-bin `ln(1+rms)` units
(amplitude-domain, floor-dependent, correctly refuted at ≤ 0.073), whereas `lag_decision.prominence`
is `peak_r − second_peak_r`, a difference of *normalized* correlations and therefore gain-invariant.
Use it to explain measurements, not to gate.

### 2.3 Silence makes an ear label uninformative

3/g9's margin is 0.755, so its −725 ms backward displacement is *real and unique*; it is nonetheless
ear-clean because the passage is deeply quiet — fill −56.1 dB, clip −60.6 dB, **14–18 dB below every
other labelled gap** (next quietest clip: −47.3 dB). All three rendered clips sound alike. It would
have sounded clean at *any* residual, so it cannot corroborate the guard and should be marked
uninformative rather than counted among the 10 clean. **The clean cohort is 9, not 10.**

**Do not turn this into a level threshold.** The obvious action — "screen labelled gaps for level
headroom and treat sub-threshold ones as unlabelable" — was proposed and **withdrawn the same day**.
It is the refuted-cutoff shape again (`peak_delta_db` does not order audibility: corpus max 24.3 dB
clean, an audible case 0.6 dB from a clean twin), and the supporting claim ("−56 dB is inaudible")
smuggles in an absolute playback reference the pipeline does not have. Treat 3/g9 as a caveat on n,
not as a new axis. A principled version would have to come from **shoulder continuity**, not level.

### 2.4 "Repeats the material immediately before the gap" is content-dependent

On 31/g7 it is literal: the fill matches A's own audio at −929 ms, r = 0.789, next peak r = 0.104.
On 26/g6 it is not recoverable from correlation — the top A-side peaks are +2953 ms (r = 0.876),
−2816 ms (r = 0.865), +2472 ms (r = 0.807), the signature of ≈ 2.9 s periodic material. **Backward
displacement is what is established**; *which* earlier material a listener hears is not, on periodic
content. Relevant to the periodic-ambiguity work.

---

## 3. How the two failures are patched today (production, 2026-08-11)

Both carry an *identical* and entirely ordinary tag signature — `plan_kind: fillable`,
`patch_tier: high`, `seam_shape: balanced`, `fit_path: baseline_only`, `signature_mode: energy`,
`confidence: high`. To every production signal these look routine.

> **Correction (2026-08-11).** Earlier text here read "`fit_path` is `baseline_only` on all 227
> patched gaps, so dual-fit was not enabled in these runs". Both halves are wrong. `fit_path` reports
> **boundary-grid** usage, not dual-fit (`FitPathTag::from_boundary_grid_used`, `gap_tags.rs:325`), so
> it never spoke to dual-fit at all. And dual-fit *was* enabled: `dual_fit_used` is true on **80 of
> 227** patched gaps — it is `skip_serializing_if`-omitted when false, so its absence on a gap means
> declined, not disabled. It is absent on both failures, and §4.2 shows 31/g7's decline was for cause.

**The residual gate does not check placement — by design, not by defect.** On both failures it
abstains with `residual_uninformative: beyond_lag_reach`. Read the code before treating this as a
gap: [docs/json-output.md](../json-output.md) § residual distinguishes `beyond_lag_reach` (the
gate's **deliberate refusal**) from `no_reference_window` / `probe_non_finite` (true "could not
measure" abstentions), and `policies/seam_residual.rs:980` gives the reason — headroom is
`chosen − floor`, the floor is anchored at the **nominal** placement and measured within a lag radius
around it, so a chosen placement outside that radius has no valid reference to subtract.

**The coupling to slide magnitude is definitional in *form*, but the two quantities do not share a
nominal — corrected 2026-08-12.** Earlier text here read "`placement_slide` … is exactly
`align_adjustment_secs` in samples … the identity is derived in §3.1". **Withdrawn:** §3.3's caveat
already showed it fails on **82 of 227** gaps, and the reason is now traced.

Both are `chosen − nominal`, but they take *different* values of each:

| | chosen | nominal |
|---|---|---|
| `placement_slide` (`patch_region.rs:1139`) | `outcome.structure_start_frame` (`:519`) | `fit_residual_geometry` (`:455-471`) |
| `align_adjustment_secs` (`patch_audio/region.rs`) | `alignment.start_frame` | `ctx.offset_nominal_start` |

The *chosen* operands do agree — S5 established `structure_start_frame = alignment.start_frame` on
both gate paths (that assignment is what made `waveform_adjustment_secs` identically zero, §3.3b). The
**nominals do not**: `fit_residual_geometry` shifts the B nominal by
`refined.start_frame − baseline.start_frame` (`:466-468`), so it **tracks gap refinement**, while
`align_adjustment_secs`'s nominal is the mapped one and does not move. The two coincide exactly when
refinement moved nothing — 145 of 227, and **208 of 315** on the oriented corpus (§0.0), so the
identity holds at the same 64–66% rate in both.

That predicts §3.3's counterexample rather than merely tolerating it: **31/g6 has
`align_adjustment_secs` exactly 0.0 with `placement_slide_frames` 1734 (36 ms)** — a 36 ms refinement
move, comfortably inside `GAP_EDGE_REFINE_SECS` (0.75 s). It is very likely the same cause as §7's two
frame-672000 outliers (589 and 4245 frames); worth confirming when that item is picked up.

**Scope the implication accordingly.** With `DEFAULT_RESIDUAL_LAG_SECS = 0.010`
(`residual_gate.rs:44`) → 480 frames, and `beyond_lag_reach = slide > max_lag` (`:972`) feeding
`gate_abstains = !informative || beyond_lag_reach` (`:986`):

> **`beyond_lag_reach` ⟺ the *refinement-relative* slide exceeds 10 ms** — which equals
> `|align_adjustment| > 10 ms` only on the 145/227 where refinement moved nothing. It holds on both
> failures (§3.1). The residual gate's domain is fills placed within 10 ms of **its own**
> refinement-tracking nominal, not of the mapped one.

*(The earlier `harness/seam_residual.rs:326` citation was already corrected on 2026-08-11: that is the
*fixture* harness, which computes the same formula from `fixture.true_fill_start` and zeroes the slide
unless the floor anchor matches production, so it never supported a claim about the dumps.)*

The table below is therefore a tautology, not a finding — the non-`beyond_lag_reach` bucket tops out
at 8.0 ms, under the 10 ms line, as the definition requires. Kept only to size the population
(26/g6 and 31/g7 sit at `placement_slide_frames` 69 601 and 46 916 against `max_lag_frames` 480 —
145× and 98× outside reach):

| `residual_uninformative` | n | median \|applied\| | max \|applied\| |
|---|---|---|---|
| `beyond_lag_reach` | 159 | 56.7 ms | 1450 ms |
| every other reason | 68 | **0.0 ms** | **8.0 ms** |
| **oriented, 317 gaps — `beyond_lag_reach`** | **215** | **43.8 ms** | **2833 ms** |
| **oriented — every other reason** | **102** | **0.0 ms** | **9.8 ms** |

The tautology holds on the oriented corpus too: the non-`beyond` bucket still tops out under the 10 ms
line, as the definition requires.

`residual.informative` is true on **12 of 227** gaps (5%) — **26 of 317 (8%) oriented**; the 08-12
baseline read 16/227, so the 12 came from the older 08-07 report — and only where the fill was
essentially not moved. The five largest slides in the corpus are all `beyond_lag_reach`, `informative: false`,
and all `patch_tier: high`. That is the mechanical explanation for "found by ear, not by any gate" —
but the correct reading is that **no component owns the question "is this placement plausible?"**,
not that the residual gate fails at it. The residual gate measures same-source cancellation depth at
a placement already taken as given; validating the placement was never its job.

**Widening `max_lag_frames` is the wrong fix, and for a sharper reason than "recalibration risk".**
The floor stays anchored at nominal, so a wider radius does not extend the check — it invalidates it,
subtracting a reference that no longer covers the point being tested. Measuring at a slid placement
requires re-anchoring the floor *there*, at which point the metric stops asking "did this land where
the map says" and starts asking "does this cancel wherever it happens to be" — a different question,
and not the one that catches these two. (Also see
[BACKLOG.md § Residual gate follow-ups](../../BACKLOG.md#residual-gate-follow-ups) L6: changing walk
parameters recalibrates the floors.)

---

### 3.1 The dump join: nominal fill start is frame 672000 (settled 2026-08-11)

`brackets[].start_frame` is in the shared per-gap **B-haystack** coordinate, whose origin is
`b_start − pad_lead` (`gap_fingerprint/measure.rs:2441`), with

```
pad_lead = gap_signature_context_secs + fill_border_search_secs + fill_align_margin_secs
         = 3.0 + 10.0 + 1.0 = 14.0 s   →   672 000 frames @ 48 kHz
```

So in every full-tier dump the nominal fill start sits at frame **672000**, and the *signed* slide of
any bracket is `start_frame − 672000` (negative = backward). Corpus check over the 39-pair dumps: 130
of 132 gaps carrying a zero-move placement satisfy
`|zero_move.start_frame − 672000| == residual.placement_slide_frames` — 70 exact, 60 off by one frame
(rounding). Two outliers remain (589 and 4245 frames), plausibly gap refinement moving the start or
the `.max(0.0)` clamp near track start; see §7.

| | zero-move `start_frame` | − 672000 | |
|---|---|---|---|
| 26/g6 | 602 399 | −69 601 | **−1450.0 ms** |
| 31/g7 | 625 084 | −46 916 | **−977.4 ms** |

Exactly the `applied` values from the scan reports. This is the join that makes Phase A answerable
from dumps; without it `placement_slide_frames` cannot be matched to any bracket.

### 3.2 What the dumps do and do not capture

The bracket sweep that produced these dumps is `oracle_score_fit_candidate`
(`gap_fingerprint/measure.rs:1998`), **not** the `place_on_b` sweep at `:1673`. The comment at `:1995`
is load-bearing: the recorded `placement` is *the gate's own chosen alignment at production weights* —
"the only placement in the dump that can observe an end-search scoring change", whereas `place_on_b`
runs at `waveform_weight: 0.0` and is blind to those terms. The signature confirming which path wrote
a dump: on a passing bracket `structure_pre/post` are `None` and `seam_*` carry `sc.report_pre/post`.

Captured: per-bracket seam scores, and the production-weight chosen alignment (`start_frame`,
`fill_frames`). Not captured:

- **which bracket production selected** — the sweep scores all of them and records no choice;
- **structure scores on passing brackets** — `None` by construction, so passers cannot be ranked on
  structure (only 89 of 13 619 corpus brackets carry structure, all `structure_floor` failures);
- **the end search's score surface** — only its argmax survives.

### 3.3 The scan reports settle two things the dumps could only infer (2026-08-11)

`gap-files/2026-08-13-repeat-terms/<pair>.json` is the current production scan+patch report (the
2026-08-11 reading below used `archive/mixed-ab/2026-08-07-fill-level-shape/`, mixed-orientation, and
is kept as the historical source of the 26/g6 / 31/g7 digits — those two pairs match the new
baseline). It carries more than `align_adjustment_secs`: each patched gap has `status.patched.pre_correlation` /
`post_correlation`, a `residual` block (including `placement_slide_frames`) written by production
rather than by the fingerprint oracle, `tags`, and the `fill_level` shape fields.

**(a) The applied bracket is the zero-move bracket — confirmed, not inferred.** Production's
`pre_correlation` / `post_correlation` for 26/g6 are 0.3663 / 0.4942, matching the zero-move bracket's
`seam_pre` / `seam_post` in the dump to every recorded digit; likewise 31/g7 at 0.4368 / 0.4369. §4's
central claim no longer rests on the frame-672000 arithmetic.

**(b) `waveform_adjustment_secs` is 0.0 on all 227 patched gaps — because it cannot be anything else.**
An earlier draft of this section read that as "the waveform stage never moves a placement, so every
slide is decided in the structure/energy domain". **That was wrong and is withdrawn.** The field is
`(alignment.start_frame − structure_start_frame) / rate` (`patch_audio/region.rs:1073`), and both gate
paths assign `let structure_start_frame = alignment.start_frame`
(`patch_region.rs:1626` and `:1810`). On the `:1626` path `alignment` is immutable; on the `:1810`
path the only later mutations are `alignment.pre_correlation` / `post_correlation` — `start_frame` is
never reassigned. **The field subtracts a value from itself: it is identically zero by construction**,
and the corpus-wide zero is a tautology, not a measurement. The `(wf …)` branch in
`cli/output.rs:528` is unreachable code for the same reason.

The decomposition is vestigial, left from an older structure-then-waveform-refine design. The real
placement search is **unified**: `fit_score = structure_weight × structure_combined +
waveform_weight × waveform_min` (`domain/gap_fill_fit.rs:399`) with `waveform_weight` **0.65** by
default (`:293`). Waveform is not a downstream floor check — it carries most of the selection score.
S2's prior therefore belongs in that unified score, not in "the structure stage".

**Caveat: `placement_slide_frames` ≠ `|align_adjustment| × 48000` in general.** It holds on 145 of 227
(oriented: **208 of 315**) patched gaps and fails on the rest — e.g. 31/g6 has `align_adjustment_secs` exactly 0.0 but
`placement_slide_frames` 1734 (36 ms). The two coincide on the failures (§3.1) but the identity in §3
is not corpus-wide. **Explained 2026-08-12 — §3 now carries the mechanism:** the two use *different
nominals*, and `placement_slide`'s tracks gap refinement (`fit_residual_geometry`,
`patch_region.rs:466-468`) while `align_adjustment_secs`'s does not, so they agree exactly when
refinement moved nothing. 31/g6's 1734 frames is a 36 ms refinement move. What remains open is only
the overlap with the §5 `applied = 0` cohort — the *explanation* no longer blocks using either
quantity, provided the reader knows which nominal each is relative to.

Also useful here: only **5** of 227 gaps have `|align_adjustment| > 0.5 s` (**12 of 317 on the oriented
corpus**, §0.0), so S2's bound — if wired as a veto rather than a prior — would touch a dozen
placements, of which at least three (pair 31 #9/#11/#12) are believed correct. The rate is essentially
unchanged, 2.2% → 3.8%, so the shape of the argument does not move.

## 4. Phase A — source or gate? (do this first)

**Re-posed 2026-08-11.** The original framing — "a 1450 ms slide is the placement search choosing a
distant match over a near one" — aims at the wrong stage. Applying §3.1 to the two failures:

**In both failures the applied placement is the zero-move bracket** (span = the nominal gap). The
displacement did not come from choosing a distant *bracket*; it came from the alignment/end search
sliding ~1 s within the nominal bracket. Any source/gate verdict must be posed against
`gate_structure_align`'s search inside that bracket, not against bracket ranking.

This is **not** selection-worthy on its own: the zero-move bracket scores `Ok` on 132 of 291 patched
dumps (45%), so it is the ordinary path. What is anomalous is the slide at it — corpus distribution
of `start_frame − 672000` over those 132: median **+12.4 ms**, p10 −233 ms, p90 +109 ms, min −1450 ms,
max +726 ms. Only **3** gaps slide beyond −500 ms, and the two failures are two of them.

> **⚠ Distribution withdrawn 2026-08-14 (§0.0)** — mixed-orientation dumps. It recomputes with step 3
> off `2026-08-13-v0.6.0-oriented`. The per-failure facts in the paragraph above (both applied
> placements are the zero-move bracket) are pair 26 / pair 31 and stand; note the oriented corpus has
> **12** gaps at `|adj| > 0.5 s` against the old 5, so "only 3 beyond −500 ms" will not survive as
> stated.

**It is not a saturation issue.** The placement search radius is
`max(fill_border_search_secs, fill_align_margin_secs) = 10.0 s`; a 1.45 s slide sits at 14% of reach,
nowhere near an edge. (Contrast §1.2's `donor_registration` hypothesis, where ±600 ms against a true
−1150 ms *is* a saturation candidate — different search, different window.) **The fix shape is
therefore a prior or a uniqueness term, not a wider or narrower window** — that conclusion survives
everything below.

> **Corrected 2026-08-12, twice over.** This paragraph read: *"The shape here is an **argmax/aliasing
> failure**: the score surface has a competing peak ~1–1.45 s back that outranks the true one, which
> §2.2 already established independently for 26/g6 (`prominence` 0.025, material self-similar on a
> ≈ 2.9 s cycle)."* Both halves are dead.
>
> - **The citation was withdrawn before it was written.** §2.2 explicitly retracts the dump-only
>   `prominence` reading: `secondary_peak` (`measure.rs:366`) has **no exclusion radius**, and
>   26/g6's runner-up sits `top2_spacing_ms` **0.104** away — the same lobe. That 0.025 measures peak
>   width, not long-range uniqueness, and cannot establish aliasing.
> - **The claim itself is now refuted by measurement.** §4.3's ±2000 ms sweep finds **no competing
>   peak that outranks the true one at either failure** — the true registration is the tallest peak
>   in the sweep on both, by 0.214 (26/g6) and 0.742 (31/g7).
>
> What replaces it is *two* mechanisms, not one (see the status table at the top). 26/g6's surface
> **is** periodic — a 480.5 ms comb, and the applied placement sits on its third tooth (§4.3) — but
> the pre-shoulder still ranks the true tooth first, so the information to reject it was present and
> unused: **source**, and S2's prior is the fix. 31/g7 is not periodic at all (flat floor, 9.6×
> margin); its defect is the `min` reduction (§4.2): **S2b**.

### 4.1 A slide cap exists in config and is not enforced

`max_fill_align_adjustment_secs` — documented as "Maximum slide (seconds) when searching for the best
B fill position", default **0.5 s** (`infrastructure/config.rs:118,394`) — would have refused both
failures (2.9× and 2.0× over). It is plumbed config → `patch_audio/request.rs:71`, is settable from
the CLI (`cli/mod.rs:111`) and from the harness, and **no code reads it to bound any search**. The one
place that mentions it declines to use it: `domain/gap_structure.rs:294` notes that applying it to the
structure polish radius "caused multi-second exhaustive loops per fit candidate". Both failures run
`signature_mode: energy` (§3), i.e. the structure/energy path — which this knob was never wired to.

**Confirmed dead 2026-08-12** (S2's precondition — workspace-wide search: the value stops at the
`request.rs:71` field definition, read by nothing in any crate).

**Do not read this as "Phase A's answer is source, so enforce the bound".** Earlier text here did, and
it is wrong twice: the fork is retired (both answers occur — see the status table), and **S2 supersedes
the "enforce it" framing**. A bare magnitude cap is the wrong instrument in either direction:

- it would refuse **both** failures (2.9× and 2.0× over) — but also pair 31's **genuine** −1.15 s
  placements at #9/#11/#12, which are correct and clean by ear, converting two bad repairs into three
  unrepaired holes;
- and it does nothing for 31/g7's actual defect, which is the `min` reduction (§4.2/S2b) — nothing
  near nominal was ever *scored* there, so bounding the slide only removes the one candidate that
  survived.

~~**S2's resolution is therefore: wire it as a search prior, or delete it — never as a veto.**~~
**Superseded 2026-08-12 — see §4.1c. The resolution is: delete it, and express the near-nominal
preference as a backward `LATE_START_PENALTY` instead.** The "wire it as a prior" branch survived one
question too few; the prior it would build already exists in better form.

#### 4.1a What the objective already contains (read 2026-08-12, before S2 starts)

`unified_fit_score` (`gap_fill_fit.rs:376-406`) already carries **three** placement priors. None of
them is visible in any output (§7), which is why this had to be read out of the source.

| prior | where | anchored on | live by default? |
|---|---|---|---|
| `nominal_bias_scale` | `:396`, into `combined_structure_score` | nominal | yes (1.0) |
| `LATE_START_PENALTY` = 0.08 | `:28`, applied `:400-403` | nominal | yes (scale 1.0) |
| `AnchorSearchPrior` | `:342`, built `region.rs:155-175` | **patch-anchor table prediction** | **no** |

**(a) `fill_anchor_search_prior_weight` is fully plumbed but unreachable on a default run.** The chain
is complete — `config.rs:269` → `request.rs:146` → `geometry.rs:15` → `region.rs:155` →
`patch_region.rs:99` → `gap_fill_fit.rs:342` — and it is *not* the "just set a weight" fix an earlier
draft of S2 assumed. Four independent gates (`region.rs:156-161`): `FillMode::Fit`, weight > 0
(default **0.0**, `config.rs:599`), a non-empty `PatchAnchorTable`, **and
`anchored_retry_pass == AnchoredRetryPass::Second`**. `repair_profile.rs:289` says the same in user
terms: *"only applies with `--fill-offset anchored-retry`"*. The corpus runs are default
`FillOffsetMode::Recommended`, so this prior was **off for every gap in this investigation**.
>
> More important than the gating: **it is anchored on the wrong thing for S2.** `predicted_start_frame`
> comes from `interpolate_anchored_offset_secs` over the editorial anchor table — the *pair-level
> drift prediction*, not the nominal map. §1.1 already established that pair-level drift is the wrong
> model here (pair 31's steps from −0.04 s to −1.15 s between adjacent gaps). So S2 must either move
> the corpus to anchored-retry *and* accept an anchor-table-anchored prior, or add a **nominal**-anchored
> prior in the same shape. The existing code is a **template**, not a switch.

**(b) The one hard-coded directional prior points away from the defect.** `LATE_START_PENALTY` fires
only `if placement.start > placement.nominal_start` — it penalizes **late/forward** starts and leaves
backward slides free. §1: *"A backward slide … lands the donor window on material A has already played
— a shoulder repeat by construction. A forward slide cannot duplicate."* Both failures slid backward.
**The objective penalizes the safe direction and not the dangerous one.** ~~Whether the right S2 is a
new symmetric prior or simply extending this penalty to the backward side is now an open design
question~~ — **settled 2026-08-12 (§4.1c): extend this penalty.** The term, its scale knob
(`late_start_penalty_scale`) and its normalization already exist, and it is the near-nominal prior S2
was going to build by other means.

This also does not retire the §2 residual guard: a cap refuses large slides indiscriminately, whereas
the guard asks whether the slide *disagrees with local registration* — again, pair 31's genuine
−1.15 s gaps are the population a bare cap breaks.

**Original question — retired, kept for provenance.** Two readings were posed, with different fixes;
§4.3 showed the corpus contains **both**, one per failure, which is why §0 carries two deliverables:

- **Source.** The search should not have made that choice — it lacked a prior favouring small slides.
  *This is 26/g6 → **S2**.*
- **Gate.** The search did its job on the evidence available, and the wrongness is only visible from
  shoulder registration the search never consults. *This is 31/g7 → **S2b** (the reduction, more
  precisely, than a guard).*

**Method.** Answerable from the existing dumps, no new media run: read each bracket's signed slide as
`start_frame − 672000` (§3.1) and its `min(seam_pre, seam_post)`, then ask whether a near-nominal
placement was available and how it scored. If nominal scored close, the search had the information
and a small-slide prior would have fixed it (source). If nominal scored far worse, the search was
misled and only registration disambiguates (gate).

**First pass (2026-08-11) — the two failures answer differently.** `min(seam)` is this plan's
reduction, not a confirmed production selection rule; treat the ranking as indicative.

*26/g6 — reads **source**.* Near-nominal placements exist, and one outscores the applied placement:

| bracket span | slide | min(seam) |
|---|---|---|
| 3.746 s | −15.8 ms | **0.378** |
| 2.796 s | −44.8 ms | 0.297 |
| 3.496 s | −546 ms | 0.491 |
| **2.496 s (zero-move, applied)** | **−1450 ms** | 0.366 |

*31/g7 — reads **gate**, or rather: nothing near nominal was ever scored.* The other 18 brackets all
died at `structure_floor`, and both survivors land ~1 s back:

| bracket span | slide | min(seam) |
|---|---|---|
| **1.119 s (zero-move, applied)** | **−977 ms** | 0.437 |
| 1.869 s | −1160 ms | 0.435 |

The split is itself informative: on 26/g6 the evidence for a near-nominal placement was present and
lost on score; on 31/g7 the structure stage eliminated every near-nominal candidate before seam
scoring. Those are different defects and may need different fixes — do not average them into one
verdict.

#### 4.1b Dual-fit cannot rescue a vetoed placement here (checked 2026-08-12)

The "three unrepaired holes" claim above was made inside the bracket path and said nothing about the
fallback. Checked directly, the claim survives, for a reason worth recording so nobody re-derives it:

- **`dual_fit` is on by default** (`config.rs:176`, `#[serde(default = "default_true")]`; `--no-dual-fit`
  disables). So a bracket refusal *does* reach the fallback on a default run.
- **But dual-fit's reach is ±600 ms.** Each shoulder is fitted seam-locally within
  `max_lag_frames = 0.6 × sample_rate` (`patch_audio/region.rs:546`, mirroring the analyzer's
  `SEAM_LOCAL_SEARCH_MS = 600`), centred on the **nominal** `b_mapped_start` /
  `b_mapped_start + gap_frames`. Pair 31's −1.15 s placements are roughly **double** that half-width, so
  dual-fit declines those gaps whether or not the bracket path refused them. There is no rescue to hope
  for.
- **Eligibility for a cap-veto is undefined today.** `dual_fit_eligible` (`region.rs:584`) is
  `request.dual_fit && !matches!(fail, StructureAlignmentFailed)`, and no `SeamGateFailure` variant means
  "slid too far". Reusing `StructureAlignmentFailed` would additionally falsify the A2 comment there
  (*"structure search never scored a bracket, so there is nothing exhausted for dual-fit to rescue"*) —
  a capped bracket **did** score.

**Scope note.** "A veto turns a bad repair into an unrepaired hole" is therefore only unconditionally
true **outside** ±600 ms of nominal. Inside that band dual-fit may well rescue a vetoed placement, and
the argument would have to be remade. This is itself an argument for the prior over the veto on cleaner
grounds than were given: a prior never produces a refusal, so it needs no interaction analysis with the
fallback at all.

#### 4.1c Decision: delete the knob; put the prior on `LATE_START_PENALTY` (2026-08-12)

Pressing on what "wire it as a prior" actually buys:

- **It does nothing for pair 31 #9/#11/#12.** Those placements are *correct*. Leaving them alone was the
  entire point of preferring a prior to a veto — so none of the benefit is on that side.
- **The measured benefit is one gap.** 26/g6, where a −15.8 ms placement scored 0.378 against the applied
  0.366 — a margin of **0.012** — plus whatever unmeasured gaps share that shape.
- **The name would become a lie.** `max_fill_align_adjustment_secs` is a seconds-valued *bound*; the
  field name, the validation (`config.rs:922`) and both doc sites (`README.md:535`,
  `docs/gap-fill-modes.md:562`) all promise a hard cap. Wiring it as a soft prior means rewriting all
  three to say something the name contradicts — a knob called `max_…` that maxes nothing.
- **The mechanism already exists in the right shape.** `LATE_START_PENALTY` (`gap_fill_fit.rs:28`,
  applied `:445` as `LATE_START_PENALTY * late_frac * weights.late_start_penalty_scale`) *is*
  "penalize distance from nominal": proportional, per-candidate, already config-scaled, and now visible
  in `fit_terms` (§7).

**So S2's two outcomes are not "wire it or delete it". They are:**

1. **Delete `max_fill_align_adjustment_secs`** — the field, its validation, the CLI flag
   (`cli/mod.rs:111`), the harness setter (`repair-harness/src/patch_audio.rs:332`), and both doc claims.
   A documented 0.5 s bound that no code reads is a defect on its own terms, independent of this
   investigation's outcome.
2. **Separately, run the symmetric-penalty experiment** (§4.1a(b)): extend `LATE_START_PENALTY` to
   backward slides and diff `status.patched.fit_terms` across the corpus's 227 patched gaps. If it closes
   26/g6 without moving pair 31, the near-nominal prior exists and lives where it belongs. If it needs
   its own tuning, add **`fill_fit_early_start_penalty_scale`** — a name that is true of what it does.

**Unverified:** whether a backward penalty at the default 0.08 scale is large enough to close 26/g6's
0.012 margin. That is exactly what the experiment measures, and `fit_terms` makes it a report diff
rather than a listening session.

### 4.2 31/g7 is *not* an aliasing failure (2026-08-11)

Asked directly: does 26/g6's argmax/aliasing reading transfer to 31/g7? **No, and the dumps cannot
make it transfer.** `lag_decision.pre_anchor` at the nominal placement:

| | `peak_r` | `second_peak_r` | `prominence` | `peak_z` | `top2_spacing` | `frac_lag` |
|---|---|---|---|---|---|---|
| 26/g6 | 0.997 | 0.972 | 0.025 | 6.1 | 0.10 ms | −59.8 ms |
| 31/g7 | 0.829 | 0.277 | **0.552** | **18.3** | 3.27 ms | −47.5 ms |

At 31/g7 the true registration is strong and unrivalled inside ±600 ms — the opposite of an ambiguous
surface. Two apparent counter-arguments are void:

- The rival, if any, sits at −977 ms, **outside the ±600 ms sweep**. `lag_decision` cannot see it, and
  no dump field records the score surface — only its argmax. The dumps can say "unique within
  ±600 ms" and nothing at all about −977 ms.
- §2.4's "the fill matches A's own audio at −929 ms, r = 0.789" is a **tautology of the displacement**,
  not evidence of periodicity: the fill is B's material from nominal −977 ms and A ≈ B up to the
  −47 ms registration, so it must match A at ≈ −930 ms. It confirms the slide, not self-similarity.

**What the dumps do say — the min reduction explains the choice.** The pre-shoulder scores 0.829 at
the true lag but the **post** shoulder there is `peak_r` **0.075**, `verdict: decorrelated`, with
`splice.step_ms` 378. The unified search reduces by minimum
(`waveform_min`, `gap_fill_fit.rs:399`), so it compares roughly:

| placement | pre | post | `waveform_min` |
|---|---|---|---|
| true registration (−47 ms) | 0.829 | **0.075** | **0.075** |
| chosen (−977 ms) | 0.437 | 0.437 | **0.437** |

The slid placement wins, and *legitimately* under that objective: two mediocre shoulders beat one
excellent and one broken. This is a **min-reduction pathology**, not an aliasing failure — a
placement is preferred because it degrades the good shoulder rather than because it matches better.
(The numbers come from different windows at different placements, so this is the mechanism's shape,
not an exact recomputation of the score.)

**Dual-fit is the right shape and it declines for cause.** A clean pre shoulder, a decorrelated post
shoulder and a 378 ms step is the seam-splice signature `try_dual_fit` was built for, and dual-fit was
**enabled** in these runs (used on 80 of 227 gaps — see the §3 correction). It was not used here:
31/g7's `splice_dualfit` reads `pre_seam_r` 0.966 / `post_seam_r` **0.124** / `gate_pass: false`, with
`post_seam_global_r` 0.006 and `post_seam_prom` 0.033. Fitting the post shoulder independently and at
its own best lag still reaches only 0.124 — **B has no match for A's post border anywhere nearby.**

That completes the mechanism: there exists **no placement where both shoulders are good**, so the
`min` reduction cannot express the right answer. Faced with (0.829, 0.075) at the truth and
(0.437, 0.437) at −977 ms, it takes the second — *degrading the one trustworthy shoulder to raise the
minimum*. The defect is the reduction, not the search: when one shoulder is unmatchable the placement
should anchor on the shoulder that is trustworthy and ignore the other. The gate already has a rule
of this shape for a different case (`short_gap_one_strong_seam_fallback`, true in the recipe), which
is the natural place to look for a generalization.

**Settled by:** ~~the widened lag sweep in §7~~ — **settled 2026-08-12 by §4.3, offline.** No rival
appears at −977 ms (r 0.087 against 0.829 at the truth), so **aliasing is refuted for 31/g7** and the
min-reduction reading above is what remains.

### 4.3 S3 — the ±2000 ms sweep: no rival at either failure (2026-08-12)

**Method.** Both surround renders are padded by `gap_signature_context_secs` = 3.0 s per side
(`gap_listen/mod.rs:281,304`) — A around `[a_start, a_end]`, B around `[b_mapped_start, b_mapped_end]`
— so the nominal throat sits at exactly 3.0 s in *both* clips and **lag 0 of this sweep is the nominal
map**. Template = A's last 1.0 s before the throat (`lag_window_secs`); haystack = `_b_surround`;
mono downmix, sample-domain Pearson at every integer lag in ±2000 ms, matching `lag_side_sweep`
(`measure.rs:746`). Peak picking uses a **250 ms exclusion radius** — deliberately the thing
`lag_decision.prominence` lacks (§2.2).

**Cross-validated against production before use:** the offline curve reproduces
`lag_decision.pre_anchor[0]` to **≤ 0.003 in `peak_r`** on both gaps (0.9949 vs 0.997 at 26/g6;
0.8287 vs 0.829 at 31/g7), at lags within 18 ms. The widening is the only difference; ±600 ms cannot
see −977 ms, which is why the dumps could not answer this.

*31/g7 — rival refuted outright.* The curve is a single spike on a flat floor:

| lag | r | |
|---|---|---|
| **−38.1 ms** | **0.8287** | true registration (`frac_lag` −47.5 ms) |
| +1835.5 ms | 0.0979 | |
| −301.0 ms | 0.0974 | |
| +740.9 ms | 0.0924 | |
| **−925.1 ms** | **0.0867** | **the predicted rival — 9.6× below the truth** |

Nothing outside nominal clears 0.10. There is no competing peak to mistake, at −977 ms or anywhere in
±2 s. **§4.2's min-reduction pathology is what remains, and S2b is well-posed.**

*26/g6 — the predicted rival is refuted, but the surface is genuinely periodic.* Nominal still wins by
0.214, so the pre-shoulder alone disambiguates it — yet unlike 31/g7 the floor is not flat:

| lag | r | |
|---|---|---|
| **−41.8 ms** | **0.9949** | true registration (`frac_lag` −59.8 ms) |
| +1880.1 ms | 0.9190 | |
| −522.2 ms | 0.8669 | |
| −1002.7 ms | 0.8301 | |
| +1399.7 ms | 0.8233 | |
| −1963.6 ms | 0.8088 | |
| −1483.2 ms | 0.7806 | best within ±120 ms of the applied −1450 ms |

**Those peaks are a comb with a 480.5 ms tooth spacing** — successive gaps 480.4 / 480.5 / 480.4 /
1441.5 (= 3×) / 480.4 / 960.9 (= 2×). The applied placement is not an arbitrary slide: **−1483.3 ms is
exactly the third tooth** (−41.8 − 3 × 480.5), 33 ms from the −1450 ms actually applied.

- **Reconciles §2.2/§2.4's "≈ 2.9 s cycle" rather than contradicting it.** Those peaks (+2953 /
  −2816 / +2472) came from a *different* measurement (fill vs A). 6 × 480.5 = 2883 ms, so 2.9 s is
  plausibly the 6th tooth of this same comb and 480.5 ms is the fundamental. Offered as a
  reconciliation, not a proof — it was not measured on the A-side autocorrelation.
- **This does not resurrect aliasing as 26/g6's defect.** The comb is real, but the 1 s pre-shoulder
  ranks the true tooth first by a clear margin, so the information needed to reject −1450 ms was
  present and unused. That is §4's **source** reading, and it is what S2's near-nominal prior is for.
  The distinction that matters: 31/g7's search had no good answer available; 26/g6's search had one
  and lost it on score.

**Exit — met 2026-08-12. The verdict is *two defects*, one of each kind.** 26/g6 reads **source**
(S2's prior); 31/g7 reads **gate**/scoring-objective (S2b's reduction). Of the three items left open
here, two are closed and one survives:

- ~~decide whether 26/g6 and 31/g7 are one defect or two~~ — **two** (§4.3, and the status table).
- ~~confirm §4.1's dead knob~~ — **confirmed dead** (§4.1).
- **confirm the production selection rule** — still open. §4's first-pass tables rank by
  `min(seam_pre, seam_post)`, which is *this plan's* reduction, not a verified production rule; the
  real objective is `structure_weight × structure_combined + waveform_weight × waveform_min`
  (§3.3b), and neither term survives the search (§7). Treat those two tables as indicative. **This
  gates S2b's validation, not its diagnosis** — §4.2/§4.3 establish the mechanism independently.

**The old "if *source*, phases B and C are void" rule is withdrawn.** It presumed a single verdict.
Phase B was demoted (§0/S4) to the separate `applied = 0` domain question, which a source answer does
not touch, and Phase C's δ stays blocked on B regardless.

**Why first.** Phase C (δ) presupposes a gate with a threshold. Sequencing it before A would encode
an answer the evidence has not given — the same failure mode as the refuted readings in §1.1.

## 5. Phase B — the guard's domain (`applied = 0`?)

**The 150–500 ms band is almost entirely the `applied = 0` cohort (found 2026-08-11).** Re-deriving
the render list showed **16 of the 18** in-band gaps have `applied = 0` — placed exactly at the
nominal map while a confident shoulder (`peak_r` 0.90–1.00) reports 156–400 ms of local drift. Only
3/#3 (`applied` +70 ms) and 10/#2 (−146 ms) involve an actual slide. So the render does not calibrate
δ first; it decides the guard's **domain**:

- If the 16 sound **clean**, a veto at δ = 150 would refuse 16 correctly-placed fills, and δ must
  clear the whole cohort (> 400.3 ms). The honest conclusion is then probably not "δ = 500" but
  **restrict the guard to `applied ≠ 0`** — no slide was made, so there is no placement decision to
  second-guess — leaving δ calibrated on a labelled population of **two**, i.e. still unknown.
- If any of the 16 sound **bad**, the `applied = 0` cohort is a second defect in its own right (the
  placement search declining to move when the shoulders say it should), not a threshold question.

**Method.** Render the 18 and label each by ear. `--gap-listen` requires `--gap-fingerprints`; the
selector bounds the fill plan, so only the named gaps pay the per-bracket anchor oracle
(`args.rs:36`) — the difference between minutes and hours. **`--fingerprint-gap` is 1-based** (repair
table `#`); emitted files are 0-based `gNNN`, so locate output by the timestamp in the filename.

```
cargo run --release --features calibration,he-aac -p clip-sync-repair --bin clip-sync-repair -- \
    <A> <B> --gap-fingerprints gap-files/<run>/<pair> --fingerprint-gap <list> --gap-listen
```

| pair | `--fingerprint-gap` |
|---|---|
| 1 | 14 |
| 3 | 3, 6, 7, 8, 9, 12 |
| 6 | 7, 9, 10 |
| 10 | 2 |
| 18 | 27, 28, 34, 35, 41 |
| 25 | 3, 5 |

18 in-band gaps + the 2 known-bad below −500 = the 20 the δ table reports at 150 ms. (An earlier
list, "20 gaps in pairs 3, 6, 13, 18, 22, 25, 27", was wrong on both count and pairs.)

**Selection lesson from the round-2 render:** the fingerprint dump's `manifest.json`
`outcome: patch` is the *fingerprint's own* gate simulation and does not predict production — 6 of
11 gaps chosen that way produced no patch because the scan-time equivalence gate declined them.
Select on the scan report's `patch.gaps[i].status.patched`.

**Exit.** Each of the 18 labelled clean/bad, and a decision on whether `applied = 0` is in the
guard's domain.

## 6. Phase C — δ (only if A says *gate* and B leaves a population)

Blocked on A and B. What is already known:

| Question | Why it blocks |
|---|---|
| **δ is unlabelled between 430 and 930 ms** | The threshold sits in an empty band, justified by a gap in the data rather than by any labelled gap inside it |
| **τ = 0.8 is fragile** | 31/g7's only confident shoulder is `peak_r` 0.829. At τ = 0.9 that failure abstains and is **missed**. The usable window is roughly (0.7, 0.83] — thin, set by one sample. Understand *why* that shoulder reads 0.83 before pinning τ |
| **"0 false positives on 225" is weaker than it sounds** | Only 9 of the 225 non-failing gaps are informatively ear-confirmed clean (§2.3). The corpus result establishes the guard is **rare**, not that every non-fire is correct |
| **Veto vs record-only** | A false positive is an unrepaired hole — the same asymmetry that kept the fill-level fields record-only |

**Caveat: n = 2.** Two labelled failures against nine informative labelled clean gaps is enough to
identify the mechanism, kill the old framing, and rank candidate signals — not enough to fix a
threshold.

---

## 7. Loose ends (not blocking, don't lose)

- ~~**Widened lag sweep on 26/g6 + 31/g7**~~ — **half done, half retired 2026-08-12.** (a) The rival
  question is **settled offline by §4.3** at zero cost; re-running the fingerprint at
  `lag_max_lag_ms` ≈ 1500 would now buy a confirmation of a settled answer in exchange for a full
  per-bracket oracle pass, so **do not run it**. (b) The `edge_pinned` half stands and is still free:
  it is dump-only, and it tests §1.2's ±600 ms saturation hypothesis for `donor_registration`. Note
  the two failures already read `edge_pinned: false` (§4.3's dumps), but they are the wrong gaps for
  it — the hypothesis is about pair 31 **#9–#12**, whose true registration is ≈ −1150 ms.
  **Successor 2026-08-13:** do not widen the fine sweep; recenter it on a coarse envelope peak.
  [TEMP-lag-envelope-recenter-plan.md](TEMP-lag-envelope-recenter-plan.md). Pair 31 only to start;
  not a corpus pass.
- **The +39 ms systematic bias** in fill-start detection (§2.1). Cosmetic for a discriminator;
  required before any absolute claim about delivered position.
- **The `applied = 0` cohort at large** — 26 gaps corpus-wide placed at nominal while confident
  shoulders report 150–430 ms of drift. Phase B labels 16 of them; the rest remain open.
- **Two gaps break the frame-672000 identity** (§3.1) by 589 and 4245 frames. Candidates: gap
  refinement moving `refined.start_frame` away from the reported gap start, or the `.max(0.0)` clamp
  on `pad_lead` near track start. Dump-only; needed before the join is used unconditionally.
- ~~**`max_fill_align_adjustment_secs` appears to be a dead knob**~~ — **DELETED 2026-08-12** (§0 S2(1));
  the bullet below is kept for provenance. Confirmed dead 2026-08-12
  (§4.1): plumbed, validated, CLI- and harness-settable, documented as live in `README.md:535` and
  `docs/gap-fill-modes.md:562`, and **read by nothing**. Not a loose end any more — it is **S2**, and
  S2's rule is narrower than the "wire it to the placement search or delete it" this bullet used to
  say: ~~wire it as a search prior or delete it, never as a veto~~ — **settled 2026-08-12 (§4.1c):
  delete it.** Never a veto (a bare cap would break pair 31's genuine −1.15 s placements, which
  dual-fit cannot rescue — §4.1b), and not a prior either, because the prior belongs on
  `LATE_START_PENALTY` rather than on a knob whose name promises a hard bound. A documented 0.5 s
  bound that silently does nothing is still worse than neither.
- ~~**`waveform_adjustment_secs` is a dead field**~~ — **deleted 2026-08-12** (§0 S5). What it *should*
  have reported is now its own open item: **surface the winning candidate's `structure_combined` and
  `waveform_min`.** Both are locals inside `unified_fit_score` (`gap_fill_fit.rs:391-399`), which
  returns only the blended `f64`; `FillAlignment` keeps neither, so nothing downstream can say which
  term carried a placement. §4.2's min-reduction diagnosis had to be reconstructed from dumps for
  exactly this reason. Cost: a return-type change on `unified_fit_score` plus threading through the
  bracket search. Not a deletion — schedule separately.

  **Promoted 2026-08-12 to "do before S2/S2b".** Both remaining deliverables *edit this function*, and
  nothing it computes is observable: not the two score terms, and not any of the **three priors**
  §4.1a found (`nominal_bias_scale`, `LATE_START_PENALTY`, `AnchorSearchPrior`). A scoring change that
  cannot be seen in the output can only be validated by ear or by re-deriving dumps — which is how
  §4.2 had to be argued, and what S5 spent a session removing elsewhere. Scope: the winning
  candidate's `structure_combined`, `waveform_min`, and the applied prior deltas. **`waveform_min` is
  already an input parameter** (`:379`), so only `structure_combined` and the penalty terms need
  lifting; the return-type change is confined to this function plus its bracket-search callers.

  **DONE 2026-08-12.** `UnifiedFitBreakdown` (`gap_fill_fit.rs`) now carries `total`,
  `structure_combined`, `waveform_min`, and each prior as the **positive amount subtracted**
  (`late_start_penalty`, `anchor_penalty`, `repeat_penalty`); `unified_fit_score` and
  `unified_fit_score_with_repeat` return it. It reaches the production report as
  `status.patched.fit_terms` (documented in `docs/json-output.md`, pinned in the
  `full_surface_repair.json` golden), so **S2/S2b are now verifiable from a repair run instead of by
  ear.** Four things worth knowing before reading the field:

  - **Selection is untouched.** The search ranks on `.total` and never reads the other fields, so
    widening the return type cannot move a placement. The whole suite passes unchanged.
  - **The breakdown is re-derived at the APPLIED placement**, once, at the end of
    `match_gap_fill_unified_in_b_with_timeline` — not captured from the start search's winner. The end
    search, `unified_fine_polish_start` and `snap_fill_to_gap` each still move `start_frame` after the
    start search decides, so an in-search capture would describe a superseded candidate. Costs one
    extra candidate evaluation per gap against the thousands the search runs.
  - **`nominal_bias_scale` is not a separate field** — it is folded inside `combined_structure_score`,
    so it shows up in `structure_combined`, not as a fourth penalty. Only the three subtractive priors
    are separable.
  - **Omitted, not zeroed, where the objective did not run**: dual-fit / silence-splice rescues and the
    legacy structure-only gate path write no `fit_terms` at all. A zeroed breakdown there would read as
    "the objective ran and every term was 0".

  Guarded by `fit_breakdown_terms_reconstruct_the_total`, which asserts the terms **account for**
  `total` (not merely accompany it) on a candidate built so all three penalties are non-zero — the
  failure mode being a term that is computed but never subtracted, which leaves `total` correct and the
  attribution silently wrong.
- **Audit for other structurally-constant reported fields — first pass run 2026-08-11.** Distinct
  value counts over the 227 patched gaps, counting *presence* separately (fields omitted when false
  look constant otherwise — this is what made `dual_fit_used` read as "always true" on first pass
  when it is really true on 80/227):

  | field | present | value |
  |---|---|---|
  | `status.patched.waveform_adjustment_secs` | 227/227 | 0.0 — dead by construction (§3.3b); **field deleted 2026-08-12** |
  | `status.patched.structure_trusted` | 227/227 | false — **unexplained, check next** |
  | `fill_level.reference_at_floor` | 227/227 | false — **unexplained, check next** |
  | `tags.fit_path` | 227/227 | `baseline_only` — boundary grid never used in these runs? |
  | `fill_level.bin_ms` / `residual.max_lag_frames` | 227/227 | 100.0 / 480 — config constants, expected |

  The two marked unexplained are the same shape as `waveform_adjustment_secs` and deserve the same
  code read before anything reasons from them. `fit_path` is now known not to mean what §3 assumed;
  whether `baseline_only` on 227/227 is real or structural is still open.
- **The third large-slide gap.** Three of 132 zero-move placements slide beyond −500 ms; two are the
  known failures. The third is unlabelled and is the cheapest candidate for a third data point
  against n = 2 (§6).
