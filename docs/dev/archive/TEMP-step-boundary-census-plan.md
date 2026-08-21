# TEMP — Step-boundary census (`gap-fingerprint-stats --steps`)

> **PARKED 2026-08-21 — its deliverable was obtained without it.** The §4–§5 contract was applied
> offline to the CSV the shipped bin already exports (`GAP_FP_CSV=1`), and the answer is a null:
> 935 admitted rows, largest trustworthy step **63.9 ms**, nothing above 100 ms, both known
> boundaries censored. Recorded in the parent census, **Result 8**. Do not implement `--steps` to
> re-derive a null on a disposable corpus. What the exercise *did* establish, and what any future
> reader should take from this file: the **Arm B dead-shoulder gate (§4) is load-bearing** — without
> it pair 50 reports a 1040/985/942 ms staircase assembled from noise peaks. Next instrument is
> [TEMP-shoulder-sweep-bin-plan.md](../TEMP-shoulder-sweep-bin-plan.md); its queue is pairs 31 and 50.
> Un-park only if a `--steps` mode is wanted as the durable, CI-guarded form of that measurement.

**Status:** ~~draft, not started~~ **parked**, 2026-08-21. **One deliverable:** a new analysis mode on the
existing `gap-fingerprint-stats` bin that, per pair, orders gaps along the file and reports
where the seam registration **changes level** — the map-step boundaries — plus a corpus base
rate for them. Descriptive first: this is a census, not a detector and not a repair change.

Parent: [TEMP-collapsed-bridge-census-2026-08-20.md](../TEMP-collapsed-bridge-census-2026-08-20.md)
(Q8, re-aimed to High after Result 7). Sibling evidence:
[TEMP-pair50-anchor-seam-outcomes-2026-08-21.md](../TEMP-pair50-anchor-seam-outcomes-2026-08-21.md).
Does **not** implement the Collapsed-bridge skip verdict ([BACKLOG.md](../../../BACKLOG.md) § Fill
placement) — it establishes whether that item's population is big enough to be worth a verdict.

> **Verification rule.** `file:line` references belong only in a checklist item about to be
> executed; design sections state the decision and its reason. Before coding, re-read
> `gap_fingerprint_corpus/schema.rs` (`GapRow`, `seam_step_ms`, `seam_mid_ms`, `step_edge_pinned`,
> `splice_diag` / `SPLICE_MIN_PEAK_R`) and `report.rs`'s `mechanism_text()` — the closest existing
> analysis, and the one this mode is deliberately *not* a duplicate of.

---

## 1. Problem

Result 7 of the census settled that `collapsed_bridge` is an arithmetic coincidence, not a gap
class. `bridge_ms = gap_ms + post_lag − pre_lag`, and at a map-step boundary
`post_lag − pre_lag` **is** the step — so `|bridge_ms| ≈ 0` fires only when
`step ≈ −gap_duration`. 31/g7 hits that to 3.1 ms (step −1121.9 ms vs duration 1118.8 ms). A
**positive** step can never fire it at all, so pair 50's +2.789 s step is invisible to the
predicate at any duration.

The real invariant is sign-agnostic: **pre and post disagree by ≈ the pair's map step.** Two
pairs are known to carry one (31 at ~−1.15 s, 50 at ~+2.789 s), and both were found only
because the WAV shoulder sweep went looking. Nobody has asked the corpus how common they are.

The existing `mechanism_text()` per-pair test fits a **line** (offset vs gap time) and asks
whether the slope explains the spread — clip drift. A step boundary is a *piecewise-constant*
level change: near-zero slope, large spread, and it reports as "local (not drift)". The
staircase question has never been asked.

## 2. Goal / non-goals

**Goal.** Per pair, ordered by position in the file, report the seam-registration level series
and flag sustained level changes; roll that up to a corpus count of pairs-with-a-step and
gaps-at-a-boundary. Judge a step **against the pair's own distribution**, not a fixed ms
cutoff — pair 31's 1.15 s and a pair whose seams scatter by 40 ms are not the same question.

**Non-goals.**

- No repair-behaviour change, no new production predicate, no `collapsed_bridge` tuning. The
  memory note stands: don't build a `|bridge_ms| ≤ 50 ms` detector.
- No WAV decoding. This is a dump-only pass over JSON already on disk; the WAV instrument is
  [TEMP-shoulder-sweep-bin-plan.md](../TEMP-shoulder-sweep-bin-plan.md) and stays separate (§9).
- No golden/CI baseline in the first cut. Add one only if the numbers turn out to be worth
  regressing against.

## 3. Placement (locked)

**Rust, in `clip-sync-repair-harness`, as a new mode on the `gap-fingerprint-stats` bin**, with
the logic in a module beside `analysis.rs` (`steps.rs`) and a `steps_text()`-shaped renderer.
`required-features = ["calibration"]`, relative dirs resolving against the repo root — both
inherited from the bin as it stands.

Why here:

- The harness **already parses the two fields this needs.** `splice_step_ms` ("first-class
  splice step, `post_lag − pre_lag`") and `splice_edge_pinned` are typed `GapRow` fields today,
  with `seam_step_ms()` falling back to `frac_lag_post_ms − frac_lag_pre_ms`. Q8 is a
  grouping-and-distribution pass over rows that are already typed, already read, already tested.
- `a_start_secs` is already carried *for exactly this purpose* — `mechanism_text()` uses it to
  order gaps along the file.
- `step_edge_pinned()` maps one-to-one onto the Arm A / Arm B split below.

Rejected:

- **PowerShell.** `scripts/measure/census-residual-measured.ps1` is the precedent for *flat
  tallies over dump JSON*, not per-pair ordered distributions. Untyped traversal of ~1049 dump
  files re-derives a schema the harness already models, and this investigation has already lost
  time to an untyped script mis-guessing a nested enum shape.
- **Folding it into `shoulder-sweep`.** That plan's §2/§9 explicitly route JSON-only censuses
  away from it, and the two have *opposite* independence constraints: `shoulder-sweep` must not
  share production correlation code, whereas this census should share the production schema
  types. Sequence Q8 first; `shoulder-sweep` after.

## 4. Instrument contract (locked)

**Denominator.** `matched()` rows, as the rest of the report uses.

**Arm A — admitted rows only: `!step_edge_pinned()` *and* `splice_diag() == Some(Splice)`.**
Both conditions are required (see Arm B for why the second one is not optional). For an admitted
row `seam_step_ms()` is trustworthy.

**Level** is `seam_mid_ms()` — `(frac_lag_pre_ms + frac_lag_post_ms) / 2`, the same registration
number `mechanism_text()` plots against `a_start_secs`. The mode reads two series per pair, and
they are not the same thing: the **step** is within-gap (`post − pre`), the **level** is
between-gap. A boundary gap has a large step; its neighbours on either side have ~0 steps at two
*different* levels.

Group by pair, order by `a_start_secs`, and look for a sustained change in level: a boundary is
a gap whose step is large relative to the pair's own step distribution *and* after which the
neighbouring gaps settle at a new level rather than returning to the old one. A one-gap
excursion is an outlier, not a boundary. Report per pair: gap count, level before/after,
boundary index, step size; and per corpus: pairs with ≥1 boundary, gaps at a boundary.

Note this is **stricter than Q8's stated invariant.** Q8 phrases the test per-gap ("pre and post
disagree by ≈ the pair's map step"); the neighbour-settling requirement adds a sequence test on
top, which is what separates a boundary from a lone outlier. Deliberate, and the reason the CI
list has both an excursion case and a staircase case.

The relative-outlier threshold is judged against the pair's own step spread, but it is floored by
the report's existing `CLEAN_STEP_MS` (2.0 ms, `report.rs`) — the constant the rest of the report
already uses for "is this step real at all." A pair whose steps are all sub-2 ms has no boundary
by construction; don't let a tight distribution manufacture one out of quantization noise.

**Arm B — censored rows: `step_edge_pinned()`, *or* `splice_diag() != Some(Splice)`.** This is
**censored data** and must be labelled as such in the output. Two distinct censoring modes, and
the second is the one that matters most here:

- **Pinned.** A shoulder's `lag_decision` peak was clipped at ±max_lag, so the step's magnitude is
  GIGO — the honest reading is "the step exceeds the search window," never its size.
- **Dead shoulder(s).** Where `min(peak_r_pre, peak_r_post) < SPLICE_MIN_PEAK_R` (0.85,
  `schema.rs`), the lag is a noise peak and the step is two noise peaks subtracted. The census's
  Result 2 lists **14 both-dead gaps** (31/g8–g11, 50/g8–g17) at r 0.03–0.24 whose peaks sit
  *inside* ±600 ms — 444 to 590 ms — so most of them are **not** `edge_pinned`. Nothing was
  clipped; the search simply found nothing. 50/g10 reads pre −489.6 / post +452.8, a **+942 ms
  "step" assembled from two noise peaks.** Admitting those to Arm A would both manufacture
  boundaries and inflate the pair's own step distribution — the very yardstick Arm A judges
  against — so a real boundary elsewhere in the pair could be masked by them.

`splice_diag()` already implements exactly this floor and covers both cases, so use it rather
than re-deriving a peak_r test.

**`AliasSuspect` goes to Arm B too** — that is what `!= Some(Splice)` means, and it is the one
judgement call in this partition. Those rows lock on both shoulders but with thin uniqueness, so
the step may be a periodic alias; a census that is already a lower bound should not spend its
credibility on them. Report them as a **named third sub-bucket** inside Arm B (pinned /
dead-shoulder / alias-suspect) so the cost of that choice is visible in the output, and revisit
only if it turns out to be swallowing most of the corpus.

**Scope of the dead-shoulder bucket, worth stating once.** The census's "one-dead" means *exactly one*
shoulder decorrelated (corpus-wide: 31/g7, 25/g11, 50/g18). The schema's `SpliceDiag::DeadShoulder` is
`min`-based and fires for **both**-dead too (renamed from `OneSidedDead` 2026-08-21, which read as the
one-dead case only). This plan uses the schema predicate; the census's three-gap list is a subset of what
Arm B will hold, not its size. Arm B is a list to be named rather than a distribution to summarize, but
expect it on the order of the pinned set plus ~17 dead-shoulder gaps, not three.

**The known limit, stated up front.** `splice_step_ms` is GIGO precisely where the step is
largest — a step past the search radius reads as either a pinned peak or a dead shoulder, never
as its true size. Pair 31's −1.15 s and pair 50's +2.8 s are known only because the WAV sweep
measured them; this instrument places both in Arm B. So the census establishes a **lower bound**
on step-boundary prevalence, and its Arm B list is the work queue for the WAV instrument. That is
the honest ceiling of a dump-only test and belongs in the note before the run, not after.

The dead-shoulder censoring above *widens* that limit rather than adding a new one: the corpus's
two known map steps are exactly the regions whose shoulders read dead, so the pairs most likely
to carry a boundary are the pairs Arm A can say least about.

**Measured 2026-08-21 (Result 8), and the shape of the limit is worse than "Arm A goes thin."**
Arm A is *large* — 935 of 997 matched rows admit, only 62 censor. It is nonetheless **empty of the
phenomenon**: its largest step corpus-wide is 63.9 ms, and both real boundaries sit in the 62. So
a well-fed Arm A returning zero is the expected outcome, and it means "dump-only cannot see this,"
never "boundaries are rare." An earlier draft of this paragraph predicted a near-empty Arm A; that
was wrong in magnitude and right in consequence.

**Media hygiene.** Pair label + gap index only. No filenames, titles, or paths — same rule the
existing report and `census-residual-measured.ps1` follow.

## 5. CLI sketch

```powershell
cargo run -p clip-sync-repair-harness --features calibration --bin gap-fingerprint-stats -- `
  --steps gap-files/2026-08-19-decision-map-oriented
```

`--steps` selects the mode the same way `--check` does today: parsed in the same arg loop,
prints `steps_text()` instead of the default report stack, exits 2 on an empty corpus. Optional
env knob for the relative-outlier threshold if the first run shows one is needed; start with a
hardcoded default and a printed legend line rather than a knob nobody tunes.

**Boundary rule (concrete, so §6's tests can be written).** Decide these four numbers in step 1
and print them in the legend; they are the only free parameters in the mode:

| | value | why |
| --- | --- | --- |
| step is "large" | `\|step\| > max(CLEAN_STEP_MS, 6 × MAD(pair's admitted steps))` | MAD, not σ — the boundary gap is itself in the sample and would inflate σ. `CLEAN_STEP_MS` floors it so a tight pair can't manufacture one. |
| run length | ≥2 admitted gaps settled on each side | 1 is an excursion (the §6 case); 2 is the weakest claim that survives it. |
| "settled" | neighbour levels within `max(CLEAN_STEP_MS, 2 × MAD)` of their side's mean | same yardstick as above, so one knob moves both. |
| min gaps/pair | <5 admitted ⇒ report "too few to judge", no verdict | 2+1+2; mirrors `mechanism_text()`'s refusal to fit a line under 4. |

Order by `a_start_secs`; it is `Option<f64>`, so rows lacking it are excluded from the ordered
series and counted in the legend rather than sorted to one end.

## 6. Tests (CI) vs media check

**CI (no media).** Unit tests on synthetic `GapRow` vectors, beside the existing corpus tests:

- a flat pair (no boundary) → zero boundaries;
- a clean staircase (n gaps at level L, then m at L+Δ) → exactly one boundary, at the right
  index, with the right before/after levels;
- a single-gap excursion → zero boundaries (outlier, not a level change);
- a drifting pair (the `mechanism_text()` "drift-like" shape) → not reported as a boundary.
  Give it **≥4 gaps**: below that `mechanism_text()` itself refuses to judge line vs scatter
  ("too few to judge"), so a 3-point drift fixture tests nothing;
- a pinned row → routed to Arm B and excluded from Arm A's distribution;
- a **dead-shoulder row** (`peak_r` 0.2 both sides, lags −490 / +453, `edge_pinned` false — the
  50/g10 shape) → routed to Arm B, and *absent from the pair's Arm A step distribution*. Assert
  the second half explicitly: the regression this guards is a noise pair widening the yardstick,
  which a boundary-count assertion alone would not catch.

**Media check (one run).** The `2026-08-19-decision-map-oriented` corpus: 51 pair dirs, 1049
dumps, 971 gaps with both anchors, already deduplicated.

Expected sanity anchors, and the prediction is mostly **negative** — write it down before the run
so the result can't be re-read afterwards:

- **Pair 31** — g7 (one-dead) and g8–g11 (both-dead, r 0.05–0.22) are all censored, so pair 31's
  known −1.15 s boundary lands in Arm B, not Arm A.
- **Pair 50** — every gap in the cluster (g8–g17) reads dead in-window, so pair 50 contributes
  essentially **nothing** to Arm A. Its boundary is unobservable here because the whole cluster is
  censored — not because of where g18 sits. (The earlier draft gave g18's position as the reason;
  that was the wrong reason for the right prediction.)
- Therefore: **both known map steps appear only as Arm B queue entries.** Arm A's yield is
  whatever *other* pairs show, and that number — including zero — is the census result. Do not
  read a thin Arm A as evidence that step boundaries are rare; §4's known limit already predicts
  it.

**Outcome (2026-08-21, parent census Result 8) — the run happened offline against the CSV, and
every prediction above held.** Pair 31 admitted 6 of 12 rows (g7 censored at post r 0.0752), pair
50 admitted 7 of 19 (g8–g17 all censored), and Arm A's yield was **zero boundaries at any floor
≥ 50 ms**. At the 2 ms `CLEAN_STEP_MS` floor it reports 18 pairs / 29 "boundaries" of 2–41 ms —
`6 × MAD` collapsing to the floor on pairs whose steps are all ~0 — with no plateau between 2 ms
and 50 ms from which to pick a defensible threshold. That sensitivity is the reason this mode
would have needed a calibration argument it does not have.

## 7. Docs at implement

- Census note: mark Q8 answered, with the corpus base rate and the Arm B queue.
- `docs/dev/README.md`: index this plan while active; drop the line when it archives.
- The bin's module header + `--help`: one line for `--steps`, matching `--check`'s.
- Memory `collapsed-bridge-is-a-coincidence`: append the measured prevalence once known.

## 8. Steps

1. `steps.rs` beside `analysis.rs`: the Arm A/Arm B partition (`step_edge_pinned()` +
   `splice_diag()`, §4) and the Arm A boundary finder over `&[GapRow]` — pure and testable,
   returning per-pair boundaries + the named Arm B list. Partition first: the boundary finder must
   never see a censored row, including in the step distribution it calibrates against.
2. Unit tests from §6 CI list. Land 1–2 together, red first.
3. `steps_text()` renderer on `CorpusReport`, matching the existing `*_text() -> String` +
   `BTreeMap` tallying style; legend line naming the Arm B censoring.
4. `--steps` in the bin's arg loop + usage text; export from `mod.rs` as needed.
5. Run the media check (§6). Record numbers in the census note; do not tune the threshold to
   make a pair appear.
6. Update docs (§7).

## 9. Out of scope / parked

| Parked | Why |
| --- | --- |
| Measuring Arm B's actual step sizes | Needs the WAV instrument; that's `shoulder-sweep`, sequenced after this. |
| A production step-boundary predicate / skip verdict | Blocked on this census: build it only if the base rate justifies it. |
| Golden baseline for `--steps` | Add after the first run shows stable numbers worth guarding. |
| Correlating boundaries with anchor-seam outcomes | Different data source (patch outcomes); the pair-50 note is the current home. |
