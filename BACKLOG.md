# Backlog

Open follow-up work for `clip-sync`. See [PLAN.md](PLAN.md) for architecture, [docs/pipeline.md](docs/pipeline.md) for the repair pipeline (phase by phase), [docs/dev/corpus-validation.md](docs/dev/corpus-validation.md) for the test corpus, and [docs/error-mapping.md](docs/error-mapping.md) for error handling. Shipped work is recorded in `docs/dev/archive/*` and git history.

Last updated: 2026-08-18.

**How this doc works**

- **Open** — actionable items below (problem / direction kept for open work only).
- **Plans** — active drafts under `docs/dev/TEMP-*.md`; archive when shipped.

**Next:** [Fill placement](#fill-placement-two-defects--registration-ignored-26g6-class-and-step-gap-with-empty-donor-bridge-31g7) — 31/g7 diagnosis complete (step gap, empty bridge; S2b refuted); 13-gap census complete (12 of 13 are in-reach false-deads): [TEMP-lag-decision-map-coordinates-plan.md](docs/dev/TEMP-lag-decision-map-coordinates-plan.md) orders ahead of recenter; 26/g6 axis-2 start-sweep oracle still the cheapest open probe; [TEMP-lag-envelope-recenter-plan.md](docs/dev/TEMP-lag-envelope-recenter-plan.md) (unblocked after map-coordinates; unblocks S1); [Donor registration leftovers](#donor-registration-leftovers); [Repair R6](#repair-r6-follow-ups); [Residual gate](#residual-gate-follow-ups).

---

## Active plans

| Plan | Covers |
|------|--------|
| [TEMP-lag-decision-map-coordinates-plan.md](docs/dev/TEMP-lag-decision-map-coordinates-plan.md) | Search and chain `lag_decision` in map coordinates so in-reach locks are not reported `Decorrelated` when the A template walked off the throat — draft, not started; orders ahead of envelope-recenter |
| [TEMP-lag-envelope-recenter-plan.md](docs/dev/TEMP-lag-envelope-recenter-plan.md) | Recenter `lag_decision`'s ±600 ms sweep on a coarse envelope peak (independent of the fill search) so pair 31's −1.11 s offset is measurable — draft, not started; waits on map-coordinates (`lag_pair` is the fine sweep) |
| [TEMP-nway-donor-alignment-plan.md](docs/dev/TEMP-nway-donor-alignment-plan.md) | N-way donor alignment: repair one damaged copy from multiple donors — draft, not started |
| [TEMP-flac-output-plan.md](docs/dev/TEMP-flac-output-plan.md) | In-process `--flac` lossless output (peer of `--wav`, no ffmpeg) — draft, not started |

## Open work

### Fill placement: two defects — registration ignored (26/g6-class) and step gap with empty donor bridge (31/g7)

**Findings (live):** [TEMP-31g7-model-vs-findings.md](docs/dev/TEMP-31g7-model-vs-findings.md) —
placement model vs measurements; **gap signature vs patch disagreement** (§8). Residual
(`applied − frac_lag`) detects the two bad *patches*; the two gaps do not share a fingerprint.
31/g7's mark is `splice_dualfit` 0.966 / 0.123 fail (1 of 308 patched). Lag-verdict one-dead is a
**13-gap dump predicate, not a media class** (census 2026-08-18: 12 of 13 are in-reach
false-deads — [TEMP-lag-decision-map-coordinates-plan.md](docs/dev/TEMP-lag-decision-map-coordinates-plan.md)).
26/g6 is a healthy unique registration that scoring ignored.
**Parallel (RESOLVED 2026-08-18, §9; archived):** [archive/TEMP-31g7-gap-shape-vocabulary.md](docs/dev/archive/TEMP-31g7-gap-shape-vocabulary.md)
— the one-sided-closeability cell was refuted by the post-shoulder ±2000 ms wide sweep: the post
shoulder is **uniquely at −1160 ms**, both shoulders live at own lags, donor bridge **zero-length**
(`gap + post_lag − pre_lag` ≈ 0) — 31/g7 *is* pair 31's map step, and no donor fill can serve it
(any fill duplicates program; honest action = reasoned skip / step verdict).
Durable statement: [gap-vocabulary.md](docs/dev/gap-vocabulary.md) § *Known holes* (Hole 1 open, Hole 2 resolved).

**Findings (archived 2026-08-17):** [archive/TEMP-fill-placement-registration-plan.md](docs/dev/archive/TEMP-fill-placement-registration-plan.md).
Two ear-confirmed stutters, two distinct fixes. Do not re-open the source/gate fork — it was both, on different gaps. The archive's "residual is the only live discriminator" line is the patch check; superseded for gap classification by the live findings §8.

| Gap | What went wrong | Live work |
|-----|-----------------|-----------|
| 31/g7 (−977 ms) | **Diagnosis complete 2026-08-18:** the gap *is* pair 31's map step — post shoulder uniquely at −1160 ms (wide sweep, gap-shape §9), pre at −47 ms, donor bridge zero-length; B is continuous, so **any** donor fill duplicates program, and the shipped −977 is `min`'s best compromise between the two true lags (0.437/0.437). Neither one-seam fill nor dual-fit-with-reach can patch it; honest action = reasoned skip / step verdict | S2b **refuted; archived 2026-08-18** ([archive/TEMP-unmatchable-shoulder-plan.md](docs/dev/archive/TEMP-unmatchable-shoulder-plan.md)). **Census complete 2026-08-18** (14 rows: 13 one-dead + 31/g10 control): 12 `shared_near_false_dead`, 1 `collapsed_bridge` (this gap), 1 `shared_far_offset` (31/g10). Hole 2 still empty. Sensor honesty, in order: (1) [TEMP-lag-decision-map-coordinates-plan.md](docs/dev/TEMP-lag-decision-map-coordinates-plan.md) — the 12 are in-reach; dump searched/chained in template coordinates; (2) [TEMP-lag-envelope-recenter-plan.md](docs/dev/TEMP-lag-envelope-recenter-plan.md) — 31/g10 and pair 31's later gaps; (3) collapsed-bridge skip verdict — spec after (1) so `bridge_ms` is measured in map coordinates. Do not add a dump ±2000 field as the fix for the 12 |
| 26/g6 (−1450 ms) | a near-nominal placement existed and lost on score; fingerprint is healthy (dual-fit passes, both shoulders live) | **Cheapest open lead:** oracle the axis-2 start sweep at this gap — what did the objective score at ≈−50 ms vs −1450 ms (findings §6.8). Unambiguous right answer; not gated on S2b or the vocabulary decision. Beyond that, no plan. S2(2) refuted a nominal-referenced `LATE_START_PENALTY`. A registration-referenced prior is an idea, not a Change. Do not retune `fill_fit_early_start_penalty_scale` |

**Recenter plan unblocked (S2b refuted 2026-08-18), ordered after map-coordinates:**
[TEMP-lag-envelope-recenter-plan.md](docs/dev/TEMP-lag-envelope-recenter-plan.md) (pair 31, then
pair 50) — 31/g7's post landmark pins pair 31's step at the 28:45 gap itself (the plan's "from
47:32 onward" was where it next became measurable; reconcile when the plan starts). Its fine
sweep is `lag_pair`, so [TEMP-lag-decision-map-coordinates-plan.md](docs/dev/TEMP-lag-decision-map-coordinates-plan.md)
lands first (census: 31/g10 is the one shared-far member of the 14; the 12 are not recenter
customers). That unblocks S1. Do not write S1 until the recenter exit is met.

**Data.** Oriented baseline `gap-files/2026-08-13-repeat-terms/` (53 pairs, **317** patched gaps) and dumps `gap-files/2026-08-13-v0.6.0-oriented/`. Counts from the mixed-A/B set are superseded; both failures are correctly oriented and unchanged. See archived §0.0 before quoting any corpus rate.

Withdrawn as discriminators (archived §1.1): pair-level drift correction, correlation gain, a prominence floor, a level threshold, a magnitude cap.

| Item | Direction |
|------|-----------|
| **S1 — record the registration residual on the patch record** | Blocked on envelope-recenter. `lag_decision` is dump-only today. After pair 31's exit, a new TEMP that is plumbing only: emit `applied − frac_lag_ms` (min-abs over confident shoulders, abstain when none) on every patched gap; no gate reads it. This is a **patch diagnostic**, not a gap class (findings §8). Do not lift the nominal-only ±600 ms sweep |
| **S4 / `applied = 0` cohort** | Conditional ear labels for whether a residual guard's domain includes gaps that never slid. Do not run until it would change a decision. The archived §5 render list is mixed-orientation and stale |
| **Ear-pass 17/g34 and 47/g14** | Cheapest n>2. 17/g34 is the same well-posed class as 26/g6 (unique both-live, dual-fit pass, ignored; findings §8). Residual is what also groups it with 31/g7 — do not read that as a shared fingerprint. 47/g14 is the first failure-to-move. Labels, not a TEMP |
| **α / `border_standoff_secs`** | Fill sits ~25 ms late vs both shoulders. Hypothesis: production's 0.35 s standoff. Run existing arms through `scripts/measure/diff-patch-adjustments.ps1` or drop. Not a TEMP |
| **+39 ms fill-start detection bias** | Archived §2.1. Cosmetic for a discriminator; required before any absolute claim about delivered position |
| **Two gaps break the frame-672000 identity** | Archived §3.1, 589 and 4245 frames. Dump-only; needed before that join is used unconditionally |

S3 (rival peak), S5 (reporting foot-guns), S2(1) (dead cap), S2c (snap correlation), Route 2, and probe `D` recording shipped with the investigation — do not re-open.

### Repeat-at-seam prior is inert on every winner; one branch is unreachable

Found 2026-08-13 reading `status.patched.fit_terms` over
`gap-files/archive/mixed-ab/2026-08-12-fit-terms-baseline/`. ~~(39 pairs, **128** patched gaps — note `6.json` is
zero-length, so that pair needs re-running before the count is quoted as final)~~ **Counts corrected
2026-08-13, run complete:** 39 pairs, no zero-length files, **227 patched gaps of which 147 carry
`fit_terms`** (the other 80 are dual-fit, which sets `fit_terms: None`). `anchor_penalty` and
`repeat_penalty` are `0.0` on **all 147**. The two zeros mean different things and only one is a
question.

`anchor_penalty` is a **configuration** zero and needs no work: `anchor_search_prior_for_gap`
(`patch_audio/region.rs:158`) requires all four of `fill_mode = fit`, a weight > 0 (default `0.0`),
a non-empty `PatchAnchorTable`, and `AnchoredRetryPass::Second` — i.e. `--fill-offset
anchored-retry`. Corpus runs are default `Recommended`, so the prior was never constructed. See the
archived registration plan § 4.1a for why enabling it would test the pair-level drift model that § 1.1
already refuted.

`repeat_penalty` is a **behavioural** zero and is the item. The term is enabled (`weight 0.4`,
`repeat_window_frames = border_frames`), the correlations are genuinely measured per candidate, and
`fit_terms` is a real re-score of the applied placement (`gap_fill_fit.rs:938`) — the branch simply
never fires. `repeat_penalty_from_correlations` (`:540`) has three arms:

| arm | condition | value |
|---|---|---|
| 1 | `wave_min < 0.45` and `max(repeat_pre, repeat_post) > 0.55` | `repeat_max` |
| 2 | `repeat_post > 0.55` and `post_seam > 0.55` and `post_seam − pre_seam > 0.35` | `repeat_post` |
| 3 | `repeat_sum > 1.1` and `wave_min < 0.45` | `repeat_sum × 0.5` |

**Arm 3 is unreachable, and no configuration reaches it.** It shares arm 1's `wave_min` guard, so it
is only evaluated when `repeat_max ≤ 0.55` — and `repeat_sum ≤ 2 · repeat_max ≤ 1.1` for any pair of
reals, so its own condition cannot hold there. All three constants are private `const f64`
(`gap_fill_fit.rs:46-48`), settable from nothing; `fill_repeat_penalty_weight` scales the *result*,
after the branch. `git log -L46,48` shows they were introduced together in `92b8920a` and never
edited, so this has been dead since it was written. It misses by exactly zero:
`REPEAT_SUM_CEILING` is precisely `2 × REPEAT_CORR_THRESHOLD`, and `1.1 → 1.09` (or `0.55 → 0.56`)
makes it live — for the case it was evidently written to catch, a **symmetric** repeat where both
shoulders duplicate at ~0.55 and neither side alone clears the threshold.

**Also established: the seam-weak guard is not what blocks it.** `waveform_min < 0.45` on **124 of
147** winners (84%; median 0.362, max within that group 0.446 — the 0.600 corpus max sits in the
*other* group, an earlier draft conflated the two), so arms 1 and 3 are armed on 84% of the corpus.
The binding condition is the 0.55 threshold on the repeat correlation itself. Of the 23 winners with
`wave_min ≥ 0.45`, **none** satisfies arm 2's seam half (`post > 0.55` and `post − pre > 0.35`) — the
"exactly one" in the pre-completion draft was an artifact of the partial run.

**The emit shipped 2026-08-13.** `fit_terms.repeat_pre` / `repeat_post` are the winner's repeat-window
Pearson pair, diagnostic inputs not terms of `total`. The applied-placement re-score measures them
whenever `repeat_window_frames > 0` (including when the penalty weight is 0) and they ride the existing
`fit_terms` plumbing — omitted with it on dual-fit / legacy. A `--patch-only --format json` run, no
oracle cost. **Do not retune the constants until that run is read.** What the data still has to settle:

> #### The run is in: `gap-files/2026-08-13-repeat-terms/` (read 2026-08-14)
>
> 53 pairs, **317 patched gaps, 210 carrying `fit_terms`**, correctly oriented throughout (the
> 2026-08-12 baseline quoted above was the mixed-A/B set — see the archived registration plan § 0.0; its pairs
> 2–17 scanned `name.2.ext` as A). `repeat_penalty` and `anchor_penalty` are `0.0` on **all 210**, so
> the headline holds at 1.4× the n and in the right orientation. Three of the four questions close:
>
> - **Calibration, not the wrong window — but the guard *is* what blocks it.** Repeat pairs cluster far
>   under threshold: `repeat_pre` median **0.123** / p90 0.354, `repeat_post` median **0.124** / p90
>   0.338. Only **9 of 210** have `repeat_max > 0.55`. The sharper finding is that the two arm-1
>   conditions are **anti-correlated**: 169 of 210 winners have `wave_min < 0.45` (80%, matching the
>   old 84%), all 9 high-repeat winners have `wave_min ≥ 0.45` (0.467–0.772), and **the sets do not
>   intersect**. So it is not simply that 0.55 is set too high — on this corpus a high repeat
>   correlation goes with a *strong* seam, which is exactly what arm 1 refuses to penalize. Lowering
>   0.55 alone would not have fired arm 1 on any of the 9.
> - **Arm 3 was not pointed at a real population — delete it, do not repair it.** The predicted
>   straddling case (both shoulders ~0.5) has **zero** instances: no winner has both `repeat_pre` and
>   `repeat_post` in [0.40, 0.60], and only one has both ≥ 0.45. Exactly **one** winner exceeds
>   `repeat_sum > 1.1` — 53/g8 at 0.949 / 0.974 — and its `wave_min` is 0.772, so arm 3's own seam guard
>   excludes it anyway, and arm 1 would have caught it had the seam been weak. The `1.1 → 1.09` repair
>   buys nothing. This is the "arm 3 is redundant *and* arm 1 is mis-set" branch of the question below.
> - **The two ear-confirmed duplications score unremarkably.** 26/g6 `repeat_pre` **0.153** /
>   `repeat_post` **0.368**; 31/g7 **0.249** / **0.207** — against a 0.55 threshold and a 0.12 median.
>   The four other large backward slides agree (31/g8 0.046/0.097, 31/g10 0.113/0.530, 31/g11
>   0.295/−0.009, 17/g34 −0.028/0.121). **The conditional below therefore fires: the prior is aimed at
>   the wrong quantity.** Residual (`applied − frac_lag`) is a post-hoc *patch* check that groups the
>   two duplications; they do not share a gap signature — [findings §8](docs/dev/TEMP-31g7-model-vs-findings.md).
>
> **Still open: the winner-only blindness** (third bullet below) — `fit_terms` reports the winner, so
> none of this rules out the penalty demoting rivals invisibly. That needs per-candidate capture and is
> still not proposed.
>
> **What this does not license.** The measurement says the prior is inert *and* mis-aimed at the § Fill
> placement defect; it does not say what the right window or quantity would be. Treat "delete arm 3" as
> the actionable half and leave arm 1's threshold alone until the anti-correlation above is explained —
> and remember `REPEAT_CORR_THRESHOLD`'s second consumer below, which a retune would move.

- **Whether 0.55 is miscalibrated for this material, or the detector is looking at the wrong
  window.** Today "the penalty is zero" is all the 2026-08-12 baseline has. With the pair it becomes
  "the correlation was 0.31 against a 0.55 threshold" — a calibration statement — or "it was 0.62 and
  arm 1 *did* fire on losing candidates", which is a different finding entirely.
- **Whether arm 3 should be repaired or deleted.** If repeat pairs cluster well under 0.5 on both
  sides, arm 3 is redundant *and* arm 1 is mis-set, and the whole prior is inert for reasons
  unrelated to which arm is live. If pairs show up straddling ~0.5/0.5, arm 3 was pointed at a real
  population and the ceiling is the bug. Do not change either constant before this is measured.
- **Whether the prior does work we cannot see.** `fit_terms` reports the **winner only**, and a
  penalty that demotes a candidate below a rival removes it from the report by construction — so a
  winner-side zero is also what a *working* penalty looks like. The winner's own pair partly closes
  this (on the 107 seam-weak winners the zero does mean that placement's repeat correlation was under
  threshold), but ranking the rivals needs per-candidate capture, which is a bigger change and not
  proposed here.
- **Whether the two ear-confirmed shoulder-duplication gaps score high here.** 26/g6 and 31/g7 are
  the two defects whose shared *symptom* (shoulder duplication) this prior nominally exists to
  catch — they are not one class; the corpus baseline contains 26/g6 (backward
  1.450 s). Their `repeat_pre` is unremarkable, so the prior is aimed at the wrong quantity. Residual
  is a patch diagnostic, not a shared gap discriminator
  ([findings §8](docs/dev/TEMP-31g7-model-vs-findings.md)).

**`REPEAT_CORR_THRESHOLD` has a second consumer — do not retune it as if the penalty owned it.**
`score_extend_short_fill_to_gap_frames` (`gap_fill_fit.rs:1748-1779`) uses the same `0.55` as a
**stop condition** when a B bracket comes back shorter than the A gap: it extends one frame at a
time while `min(pre, post)` does not fall **and** `next_repeat <= REPEAT_CORR_THRESHOLD &&
next_repeat <= prev_repeat + ε`, then zero-pads whatever is left. The measurement is the same
`fill_repeat_correlations` (via `placement_repeat_post`, `:1666`) but differs in three ways worth
holding onto: it scores the **post side only**, it runs on the **assembled fill** rather than a B
haystack candidate, and it is a **veto**, not a weight — `fill_repeat_penalty_weight` does not reach
it, so setting the weight to `0` disables the penalty while leaving this one live.

The directions are opposed, which is what makes a blind retune dangerous. Lowering the threshold to
make the penalty fire **tightens** the extension stop: extension halts earlier, fills get shorter,
and more of the gap is zero-padded — a change to delivered audio on short-bracket gaps, arriving as
a side effect of a scoring experiment. Raising it loosens extension in exactly the cases where a
repeat is the reason to stop. Two consequences for whoever picks this up:

- **The instrumentation should cover both.** The emit above captures the *placement* pair; the
  extension loop's `prev_repeat` / `next_repeat` trajectory and its stop reason (`score_ok` vs
  `repeat_ok`, or the `ext_frames` exhaustion break) are separately worth recording, because today it
  is unknown whether the `0.55` arm ever binds there or whether the monotone `next_repeat <=
  prev_repeat` clause always stops first. If it is always the monotone clause, the threshold is free
  to move on this side and the calibration question collapses to the penalty alone.
- **Split the constant before changing it, not after.** If the corpus says the penalty wants a lower
  threshold, give the two consumers their own constants rather than moving the shared one — they are
  answering different questions (*"is this placement a duplicate?"* vs *"has extending this fill
  started duplicating?"*) and only coincidentally agree on a number today.

Related: `docs/json-output.md` § `fit_terms` records what each zero means; `docs/gap-fill-modes.md`
§ Fit unified search records that the thresholds are hard-coded, that the knob only scales the
result, and that the same threshold gates short-bracket extension; `docs/gap-repair-guide.md`'s knob
table carries the short form.

### Fill normalize boosts against a non-local reference (ear-confirmed defect)

**Ear-confirmed 2026-08-12.** Pair 19 gap 14 (`g013`) is the corpus's **only** "audible — too loud"
label, and the cause is normalize's own boost, not donor content: it carries
`normalize_gain = 3.88` (+11.78 dB, **unclamped** — `max_fill_gain_db` sits at 3.98), and the same
gap rendered with `--no-normalize` sits inline with its surroundings and matches the donor. Same
placement, same seam, gain the only variable. This **refutes** the shoulder-continuity explanation
previously attached to that clip (`docs/dev/archive/` fill-level notes and the 2026-08-06 labels).

**Mechanism, traced not guessed.** `compute_a_border_rms` (`region.rs:2312`) pools 5 s before + 5 s
after the gap into a single energy-weighted RMS. Energy pooling means any loud program inside those
10 s dominates it. Here the *immediate* shoulders are −52.1 / −52.0 dB, so matching that wide
reference put the fill roughly 12 dB above the audio directly either side of the seam — at a 10 ms
crossfade, an audible step. The gain was not clamped: it faithfully matched a reference that is not
the local context. The boost-only PR (2026-08-12) removed the *attenuating* half of this same
statistic and does not touch this half.

**Contaminates the fill-level corpus.** `measure_fill_level` measures `gained_fill` but takes
shoulders from ungained A, so a boost inflates `peak_delta_db` ≈1:1. Backing out this gap's gain
gives 15.93 − 11.78 = **4.15 dB** — below the lowest clip ever labelled *clean* (6.37), rather than
second-highest in the labelled set. All 227 rows of `gap-files/archive/mixed-ab/2026-08-07-fill-level-shape/`
are inflated by their own gap's unknown gain (0–12 dB). Cross-gap `peak_delta_db` comparisons are
invalid; do not rank or shortlist on that column.

**Second contamination, 2026-08-14: that roll-up is also the mixed-A/B corpus** (108 of its 227 rows are
pairs 2–17, scanned in the wrong direction — see the archived registration plan § 0.0). The current oriented
roll-up is `gap-files/2026-08-13-repeat-terms/fill-level-rollup.csv`, 317 rows. The CSV itself has no
gain column (`Label, Gap*, AStart/AEndSecs, EdgeDeltaDb, Head/Tail/ReferenceDb, RefAtFloor,
Pre/PostShoulderDb, PeakDeltaDb, PeakBinDb/Index, Bins, BinMs, DualFitUsed, Report`), but the run's
per-pair JSONs carry `status.patched.normalize_gain` on **317 of 317** patched gaps — join on
`a_start_secs` and the gain can finally be backed out per row.
**The ear labels behind this item are the worse casualty:** `2026-08-08-edge-listen` covers pairs 24, 8,
12, 7, 31 — **pairs 7, 8 and 12 are swapped**, so 3 of its 5 labels are of the reverse pairing and do not
transfer. Re-label before quoting "the lowest clip ever labelled clean" as a bound.

| Item | Direction |
|------|-----------|
| **Size the exposure before designing a fix** (do this first) | Do **not** design from n=1. `--patch-only --format json` now emits `normalize_gain` on every patched gap and runs the full patch path with **no** fingerprint/oracle cost, so a multi-pair sweep is cheap. The tooling already exists: `scripts/measure/measure-patch-outcomes.ps1` is exactly this run (`--patch-only` → `fill-level-rollup.csv`) and needs **one column added** — it emits `PeakDeltaDb`/`EdgeDeltaDb` (`:170`, `:183`) but not `normalize_gain`. With that column: large gain **and** quiet immediate shoulders = the exposed set, and every existing delta becomes divisible by its own gain (see the contamination note above). First honest count of how often this fires, and the input that decides between a reference-window fix and the blunt option below |
| Make the boost reference local | Candidate directions once the sweep lands: narrow the border window; take the quieter of wide-vs-local; or cap the gain by the local shoulder ratio. All are guesses today. Note `normalize_window_secs` (default 5.0) is already a knob, so a calibration sweep is possible without new code |
| Consider defaulting `normalize_fill` to false | Named explicitly because the evidence now points at it: **one** ear case where the boost causes an audible defect, **zero** ear cases where it demonstrably helps — the only support for boosting is a synthetic fixture (`patch_audio_normalizes_fill_loudness_to_a_border`), never a listen. The boost-only plan lists this as a non-goal, but that was written before the boost was implicated. If the sweep shows large boosts are common, this is the honest short-term answer while a better reference is designed |
| Re-run the fill-level roll-up after any gain change | Any change here moves `peak_delta_db` / `edge_delta_db` for the affected gaps, so `gap-files/archive/mixed-ab/2026-08-07-fill-level-shape/` becomes a different-policy baseline again. Same caveat the boost-only plan § 5.5 recorded. **Already owed once:** the boost-only PR itself changed the gain on every previously-attenuated gap, so that roll-up is a mixed-policy baseline *today*, before any further work here |
| Residual seam `g` (`a ≈ g·b`) as the production gain | Carried over from the boost-only plan § 1 follow-ups, which has no other home now that plan is archived. The attraction is that it estimates level from the **seam** rather than from whole-fill RMS, so neither the mid-gap-event failure (attenuation, fixed) nor the wide-window failure (boost, open above) applies. Deferred there because it needs verdict plumbing plus corpus rules for when the estimate is trustworthy — not because it was refused |
| B-relative / donor-shoulder matching as a full replacement for A-border RMS | Also from § 1 follow-ups. Matching the fill to **B's own surrounding level** sidesteps the A-reference question entirely; the open issue is that it corrects nothing when the two masters genuinely differ in level, which is the only case normalize exists for. Worth evaluating against the sweep data above rather than in the abstract |
| The attenuation half was never ear-validated | Stated so it is not mistaken for a proven win. The boost-only PR removed attenuation on evidence of mechanism (whole-fill RMS is the wrong statistic) plus earlier "loud event is correct content" labels — but no before/after A/B was ever rendered, because pair 19's only pre-change render turned out to be a boost gap. The two affected gaps sound clean *now*; nobody has heard them sound bad *before*. Cheap to close if it ever matters: restore the symmetric clamp, drop the `region.rs` assert, render to a `-before` dir, revert |

### Gap-selection parked debt (do not fold into thin v1)

From [archive/TEMP-gap-selection-sequencing-plan.md](docs/dev/archive/TEMP-gap-selection-sequencing-plan.md) §4.
Survive archival of that meta doc. Not required for `--only-gaps` / `--skip-gaps`.

| Item | Direction |
|------|-----------|
| `--gaps-from` manifest (v2) | Reuse a prior gap list + embedded `ScanRecipe`; mismatch → error on index entries. Prerequisites (ranges, recipe) shipped. Sketch: [archive/TEMP-gap-selection-deferred.md](docs/dev/archive/TEMP-gap-selection-deferred.md) §2. `--scan-window` is **refused** (identity): [gap-vocabulary.md](docs/dev/gap-vocabulary.md) § Gap numbering. |
| `limit_fill_to_mapped_region` on scan report | Wrong home; recipe plan explicitly out of scope — separate cleanup if ever moved |
| Absolute B occupancy via `BlockLevel.silent` (not aggregate RMS) | Optional; `silent` is now retained for equivalence (F2). Fillability still uses aggregate `rms_db` vs abs floor — switch if multichannel false-unfillable shows up. |

### Fingerprint provenance follow-ups

Leftovers from [archive/TEMP-fingerprint-provenance-plan.md](docs/dev/archive/TEMP-fingerprint-provenance-plan.md)
(Tracks A + B shipped 2026-07-31). All three were **deliberately deferred**, each with a stated
trigger — none is a known defect. Shipped behaviour: [gap-fingerprint.md](docs/dev/gap-fingerprint.md)
§ *Source identity & the corpus library* and § *`measurement`*.

| Item | Direction |
|------|-----------|
| I1-class bin-divergence warn in `equivalence-calibration` (optional; was out of that plan's DoD) | Emit lives: flag gaps where `a_gap_total_blocks × measurement.bin_ms` disagrees with the geometry span. The check is documented and the fields are on both verdicts, so this is only automating a query a human can already run. Trigger: a second bin-width divergence (I1 was found by reading source, which is what made the fields Derived) |
| Row-level "no provenance" flag on `GapRow` | Deferred: `check.rs`'s health Warn plus the census's `(absent)` bucket already make an unanswerable corpus say so, and the pattern to mirror is `registration_from_legacy_lag`. Trigger: a report that needs to **filter** rows on it — nothing does today |
| `bit_depth` string → `BitDepth` parser | Deferred: the forward pin (`bit_depth_tokens_are_pinned`) is what protects corpora already on disk; a parser is dead code until a consumer reads the token, and none does. `bit_depth` is stored-for-later by design |


### Fingerprint contract follow-ups (C3)

Leftovers from [archive/TEMP-fingerprint-field-clarity-plan.md](docs/dev/archive/TEMP-fingerprint-field-clarity-plan.md)
§ 2.8 (R1–R3 + C1 + C2 shipped 2026-08-07/08; all eight § 2.4 groups carry a contract).
**Deferred, not refused** — neither has a demonstrated need. Shipped behaviour:
[gap-fingerprint.md](docs/dev/gap-fingerprint.md) § *`_contract`*.

| Item | Direction |
|------|-----------|
| Share the contract string table with harness `legend_text()` | The stated goal was "dump and analyzer legend cannot drift" (§ 2.5). Cost is real: `legend_text()` is a human CLI roll-up on a **22-char label column** — R3 had to re-pad every label and continuation line for one 22-char field name — while contracts are ≤120-char prose read beside the numbers. Sharing couples the two formats, and the four axes do not fit a legend line. Trigger: an *observed* drift between a legend string and a contract, not the possibility of one |
| `--fingerprint-contracts=once\|always\|off` write flag | § 2.6 chose repeat-per-gap ("clarity beats bytes") for diagnostic dumps and deferred the knob to "if size becomes an issue". Nobody has measured the size cost of the eight contracts on a real corpus. Trigger: a corpus where `_contract` repetition is a measured problem — until then `always` is the only behaviour, and a knob is an untested branch |
| **Contract `status.patched` on the production report** | Raised 2026-08-12 from [archive/TEMP-fill-placement-registration-plan.md](docs/dev/archive/TEMP-fill-placement-registration-plan.md) § 0 S5, where misread patch-record fields produced two withdrawn conclusions. The production `--json` surface carries **no `_contract` anywhere**; the fields that misled want one — `align_adjustment_secs` (signed slide of the chosen fill start off nominal, **not** a lag), `placement_slide_frames` (unsigned, and **≠ `\|align_adjustment\| × 48000` on 82/227 gaps**), `fit_path` (names the **boundary grid**, not the fit path — `gap_tags.rs`'s `FitPathTag::from_boundary_grid_used`), `pre/post_correlation` (min-reduced into the placement objective). **Two design questions come first, which is why this is deferred and not just unwritten.** (a) `Contracted<T>` needs a `T` to flatten; `GapPatchStatus::Patched` is an enum variant with inline fields, so stamping it means extracting them into a named struct — and `contract.rs`'s own rationale deliberately keeps reader metadata *out of* domain types, which `patch_result.rs` is. (b) Per-gap stamping does not transfer: a dump is one gap per file, a report is all gaps in one, so a 227-gap report repeats the same four strings 227 times (~90 KB against files that run 78–574 KB today) — the same measurement the row above is waiting on, and the cheap alternative (one `_contracts` map at the report root) is exactly what § 2.6 rejected as "too easy to skip and too far from the number it explains". That rejection was reasoned about a per-gap file; whether it survives a report-shaped one is the open question. **Already landed separately** (no contract needed): `dual_fit_used` / `anchor_seam_used` are now always serialized, so absence no longer reads as "disabled" |

### Fingerprint dump naming (verdict token dropped; listen-manifest disposition open)

Raised 2026-08-12 while ear-checking the boost-only normalize PR: two pair-19 gaps whose files were
named `..._full_skip` are **patched in a normal run**, and the name caused a wrong inference about
production behaviour before the schema was re-read. Not a repair defect — a diagnostics-ergonomics
one — but it cost real time, so it is written down rather than remembered.

The trailing token came from `entry_verdict`, which resolved to
`gap.outcome.tier`: the bracket-gate `any_ok` result. That is *correct for what the fingerprint
measures* and is contractually documented as not-the-production-gate
(`schema.rs:1158-1169`: "a `tier: \"skip\"` fingerprint can correspond to a *patched* production
gap"). Production runs `skip_or_dual_fit` after the same failure class and often patches anyway.

**Token dropped 2026-08-12.** Stems are now `{a8}_{b4}_t{hms}_g{idx}_{detail_tier}` — `entry_verdict`
deleted (`lag_verdict_str` kept: `manifest.json` still records both verdicts), the flag-stability
axis resolved with it. Two claims in the original entry were wrong and are recorded here because
each was a "safe because…": the stem *was* parsed — `check.rs` cross-checked `_patch.json` /
`_skip.json` against `outcome.tier` (removed, deliberately not replaced), and `measure.rs`'s
`write_corpus_dir` test asserted the token. New dumps are not name-comparable with existing
`gap-files/` dirs (gitignored, regenerable, self-consistent within a dir). The 2026-08
mixed-orientation dumps live under `gap-files/archive/mixed-ab/` and are not a default input.

**Disposition manifest shipped 2026-08-12** — `listen.json` at the WAV root
(`gap_listen/manifest.rs`), one entry per exported gap: `disposition` mirroring `GapPatchStatus`
(`patched` / `skipped` / `not_planned` / `not_applied` / `absent`), the gate's `reason`, patch
`confidence`, `a_patched_wav`, and `a_window_unchanged`. A **sibling**, not a rewrite of the corpus
`manifest.json`: that one is written pre-patch and describes the fingerprint dump, which is valid
with no patch at all. Written on the empty-plan and no-PCM early returns too, so a missing
`listen.json` means an older binary rather than "nothing was decided". Both halves of this section
are now closed.

### Donor registration leftovers

From the 2026-08 equivalence-band / donor-registration review. Donor Apply and fill-level
check shipped; these remain.

| Item | Direction |
|------|-----------|
| **`equivalence-calibration --replay` reads `GapScanJson`** | Today `--replay` only loads fingerprint `corpus.json` / `GapCorpus`; plain scan JSON already carries the same registration + envelope fields on every gap (`scripts/measure/scan-registration.ps1`). Teach the reader the scan shape so Apply flip/abstain counts come from the production classifier, not a hand reconstruction. Small reader change; no new measurement |
| **Conditional donor test — investigation only** | Ask “is B non-silent *where A is silent*?” (at the registered lag) instead of independent A-floor + donor-occupancy halves — quiet periodic material can satisfy both in both masters (e.g. 10/12: 4/9 silent on each side, still `repairable_dropout`). **Do not change the gate yet.** First: count A-silent∩donor-silent coincidence on existing 39-pair scan JSON (no re-dump). That rate decides curiosity vs systematic; a wrong threshold drops real dropouts (dangerous direction). ~~Fill-level already catches the observed damage.~~ **Retracted 2026-08-06:** fill-level catches *loudness* damage only, and not reliably enough to gate on — see [Fill placement: two defects](#fill-placement-two-defects--registration-ignored-26g6-class-and-step-gap-with-empty-donor-bridge-31g7) for damage it does not see at all. No TEMP plan until the count says it is worth designing |
| **33/17 placement-path investigation** | Which path placed 33/17’s fill is **unrecorded**. The dump’s `brackets` array is the oracle enumeration (`list_feasible_anchor_brackets`), not the candidate production selected; rendered seams (`pre_seam_r` 0.998 / `post_seam_r` 0.973) match **no** bracket row (scores top out ~0.43). Bound-/price-extension proposals are dead (comparator already prices move hard; default profile never runs the grid; smaller moves failed the waveform floor). **Next:** instrument the selected candidate / fit path so a later “overrun” proposal has a target; likely site if anything is tuned is the **acceptance floor**, not the comparator |

### Dual-fit confidence axis

**Fingerprint / analysis only** — not a production dual-fit scope change. Do not wire
into `try_dual_fit` / rescue gating (seam ledger A5/D8: uniqueness stays diagnostic until a
labeled false positive). From [archive/TEMP-fill-placement-axis-plan.md](docs/dev/archive/TEMP-fill-placement-axis-plan.md)
Phase B residue (the `gate_pass` / end-search correlation). Axis semantics:
[gap-fingerprint.md](docs/dev/gap-fingerprint.md).

| Item | Direction |
|------|-----------|
| **Dual-fit `gate_pass` is a production mirror, not a discriminator — add a fingerprint confidence axis** | `gate_pass = min(pre_seam_r, post_seam_r) ≥ max(0.35, 0.12)` passes **263/263** on the 17-pair corpus. That is faithful, not broken: it reproduces the production gate exactly, and production's threshold sits far below the observed distribution (p05 of `smin` = 0.892) because `smin` is a ±600 ms argmax with no uniqueness term. Its value is provenance — "what did production decide" — so it should **not** be tightened. On the oriented 308, `gate_pass: false` is 11 patched; 31/g7's mark is the 0.966 / 0.123 *split* (1 of 308), which production already declined ([findings §8](docs/dev/TEMP-31g7-model-vs-findings.md)) — still not a reason to tighten the predicate. What's missing is a *separate fingerprint read* on whether the seam lag was unambiguous (analyzer / corpus roll-up strata — not a repair gate). The validators that discriminate are already emitted but ungated: `pre/post_seam_z` (p05 3.14, p25 4.91, median 7.86) and `pre/post_seam_prom` (p25 0.123). **Direction:** leave `gate_pass` alone (the goldens and corpus history read it); add derived fingerprint field `dualfit_confident` from min-z + min-prominence. **Do not** consolidate end-search length into dual-fit: on the throat cohort (n=65, where `span == gap`) the two disagree genuinely, not by dilution — `corr(fill−span, bridge−gap)` = +0.06, and tightening the amplitude floor drives it to −0.10; stratifying by min z reaches r = +0.68 only at n=8 / p=0.053, one of ~25 strata swept. In the z ≥ 6 cohort, 8 of 19 gaps show an end excursion of exactly 0 ms while dual-fit trims 4–28 ms. |


### Repair R6 follow-ups

From [archive/repair-write-path-plan.md](docs/dev/archive/repair-write-path-plan.md) post-ship gaps.

| Item | Direction |
|------|-----------|
| `--dry-run` / `--write` | Explicit CLI flags; today write mode is implied by `--wav` / `--mux` or TOML `dry_run = false`. **Still open, and now bigger than it was:** `--patch-only` (shipped) added a fourth run mode — patch, no sink — so the mode is no longer a `dry_run` boolean at all. Four modes (scan-only / `--repair-preview` / `--patch-only` / write) are selected today by three flags plus a TOML boolean, with the mutual exclusions enforced pairwise in `RepairConfig::validate`. If this is picked up, do it as one coherent mode selector rather than bolting `--dry-run` / `--write` onto the side — note `--patch-only` deliberately *keeps* `dry_run = true` (it writes nothing), which an explicit `--write` flag would make incoherent |
| Scratch-buffer regression test | Dedicated unit test for patch PCM path |
| Streaming / chunked WAV encode | Large multi-gap fills without holding full PCM |
| Adaptive gap-signature context (low priority) | Widen `gap_signature_context_secs` per-gap only when the score at the nominal map is below floor, instead of decoding wide B context for every gap. From [energy-signature-plan.md](docs/dev/archive/energy-signature-plan.md) Phase 4; low value since mode-coupled `nominal_bias` already handles drift at the 3 s default |
| Dual-fit oracle: unpin pre-shoulder lag from 0 (optional hardening) | `validate_dual_fit_oracle.rs` (gated `validation-tests`, needs ffmpeg + fetched corpus) only steps the **post**-shoulder (`step_ms`); the pre-shoulder is always sourced from B at lag 0 by construction (`dual_fit_oracle.rs`). The 2026-07-03/05 production bug (dual-fit's re-validation gate wrongly applying the single-rigid-lag crossfade branch, fixed via `SpliceSeamContext::single_lag_alignment`) is now covered end-to-end by a synthetic unit test (`dual_fit_result_passes_the_production_revalidation_gate`, `domain/dual_fit.rs`), so this isn't blocking. Direction if picked up: add an optional `pre_step_ms` field to `DualFitOracleCase` (default `0.0`, mirroring `step_ms`) that shifts the pre-gap portion the same way the post-gap portion is shifted, plus a manifest case with both nonzero — proving the real-codec path too, not just the synthetic gate. Not yet implemented or run (needs ffmpeg + real media, which wasn't fetched here per the licensed-media partition) |

---

### Residual gate follow-ups

From [archive/residual-gate-findings.md](docs/dev/archive/residual-gate-findings.md) and
[archive/residual-gate-wiring-plan.md](docs/dev/archive/residual-gate-wiring-plan.md). Test inventory:
[`residual_gate_catalog/`](crates/clip-sync-repair/tests/residual_gate_catalog/).

| Item | Priority | Direction |
|------|----------|-----------|
| **M3** — floor walk vs B haystack OOB | med | `walk_reference_frames` is A-energy only; B OOB → NaN at measure time today. Tighten walk only if field media hits bad geometry |
| **G1** — residual on Pearson-only skips | gap | Veto skips done (`ResidualHeadroomExceeded` + `GapPatchOutcome.residual`). Pearson/structure skips still lack residual unless we measure on last grid candidate when `measure_residual` |
| **L6** — coarse outward walk step | low | `step_frames = window` in `measure_fit_residual_verdict`; changes floors → recalibrate if touched |
| **M4** — MP3 calibration | defer | Manifest rows marked M4; gate is codec-agnostic |
| **FD-1** — fractional-delay cancellation | defer | Sub-sample lag + B resample; re-run floor calibration — see findings § FD-1 |
| **`finale_floor_nan_probe`** | optional test | Unit repro: why Grieg finale floor is NaN (M3-adjacent); catalog backlog |
| **`c1b_acoustic_echo_pipeline_veto`** | optional test | Pipeline `ResidualHeadroomExceeded` under `production_fit` on non-F4 echo fixture — optional C1b |
| **`p2_search_winner_bounds`** | optional test | Bound headroom on search winner vs truth placement — needs design |
| **Informative floor + NaN headroom** | low | `informative: true` with non-finite `worst_headroom_db` does **not** abstain: band is `correlates_only`, residual gate pass-through (no veto). Shipped MC behaviour, now shared by mono after measuredness unify. Documented in `abstention_reporting_is_decision_neutral`. **Direction:** leave as-is unless a census shows the cell is common and harmful; if tightening, extend `gate_abstains` to require finite headroom (more abstentions) — count first, like the unify plan |

**Explicitly not planned:** `veto_rescue` as default (G5: synthetic-only); F4 pipeline veto (M6).

**Shipped 2026-08-06:** mono/multichannel “measured” semantics unified toward MC
([archive/TEMP-residual-measured-unify-plan.md](docs/dev/archive/TEMP-residual-measured-unify-plan.md)) —
`ProbeNonFinite` ignored like unmeasured on both constructors via shared `combine_informative`.
Combined `uninformative_reason` prefers governing `FloorAboveOkDb` over coexisting `ProbeNonFinite`.

---

### Defer / opportunistic

| Item | Direction |
|------|-----------|
| [Offset-mapped end placement](#offset-mapped-end-placement) | After start clip, place B end window at `A_end + Δ` when B has a long leader — see [archive/anchored-end-extraction-plan.md](docs/dev/archive/anchored-end-extraction-plan.md) follow-ups |
| [Skip overlapping end fingerprint](#skip-overlapping-end-fingerprint) | Omit end clip when `T_anchor − L` overlaps start window |
| [Weighted drift in repair warning](#weighted-drift-in-repair-warning) | Down-rank end clip in instability synthesis when end confidence is low |
| [Memory / PCM cloning](#memory-use-and-pcm-cloning-on-long-clips) | `Cow` / in-place prep when painful; parallel A/B decode when needed |
| [Committed test fixtures](#committed-test-fixtures) | Optional committed MP3; committed verify deferred — see [tests/corpus/README.md](tests/corpus/README.md) |
| [Verification cost knob](#verification-cost-knob) | `validation.max_verification_secs` — only on demonstrated friction |
| [Test tier follow-ups](docs/dev/test-tier-remainder.md) | `clip-sync` ignore cleanup (~1 h), optional `pr-repair-extended` / SP on PR, Phase 2b binary split — see [test-tier-remainder.md](docs/dev/test-tier-remainder.md) |

#### Memory use and PCM cloning on long clips

15-minute default clips; full PCM in memory per extracted window; no streaming fingerprint API yet. **Decided (2026-06-11):** future streaming should reuse `scan_*_buckets` callbacks; `MediaSession: Send` allows one session per thread when parallel decode lands — see [PLAN.md](PLAN.md) § Media session semantics and [archive/media-session-redesign-plan.md](docs/dev/archive/media-session-redesign-plan.md).

**Refs:** `application/align_videos.rs`, `domain/pcm_preparation.rs`

#### Committed test fixtures

Tier B = 3× 30 s WAV pairs; ffmpeg for encoded formats. Hold-out verify on committed tier deferred — generated-only coverage documented in [tests/corpus/README.md](tests/corpus/README.md).

**Refs:** `tests/corpus/`, `Cargo.toml` features

#### Verification cost knob

Optional `validation.max_verification_secs` — deferred in [archive/verification-hardening-plan.md](docs/dev/archive/verification-hardening-plan.md). Implement only if verify decode cost becomes painful in practice.

**Refs:** [corpus-validation.md](docs/dev/corpus-validation.md) § Hold-out verification cost

#### Offset-mapped end placement

Symmetric `SharedTimeline` places end windows at the same absolute times on A and B; when B has a long leader before shared content, offset-mapped end (`[T_a−L, T_a]` on A, shifted by Δ on B) would align fingerprints to the same story region.

**Refs:** [archive/anchored-end-extraction-plan.md](docs/dev/archive/anchored-end-extraction-plan.md)

#### Skip overlapping end fingerprint

When `T_anchor − clip_length` overlaps the start window (short shared span), skip end fingerprinting to avoid comparing redundant audio.

**Refs:** `domain/policies.rs`, `application/align_videos.rs`

#### Weighted drift in repair warning

Repair instability warning treats end − start drift equally; down-rank or de-emphasize end when end confidence is low or tail decode was unreliable.

**Refs:** `clip-sync-repair/src/infrastructure/cli/output.rs`

---

## Explicitly out of scope (initial version)

From [PLAN.md](PLAN.md) — not backlog unless scope changes:

- Video frame / visual sync
- Batch processing (> two files)
- Writing aligned output files from the **analyzer** (report offset only)
- Patched repair output is **`clip-sync-repair`** only — [archive/repair-write-path-plan.md](docs/dev/archive/repair-write-path-plan.md)
- Network or streaming sources
