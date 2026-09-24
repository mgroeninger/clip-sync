# TEMP — Dual-fit registration routing

**Status:** drafted 2026-09-22. Phase 0 code landed 2026-09-22 (per-seam
fields, PR gate green); the corpus `--patch-only` rerun is running, with its
processing validated on the first 33 pairs (§ 7). The coverage census is done
(§ 1b, § 7). Production arm (`patch_audio`
characterize → dual-fit), plus one measurement-arm Change in Phase 1c
(fingerprint dump fields). Record-only through Phase 1; flag-gated, default
off, through Phase 3.

**Tracked as:** [BACKLOG.md](../../BACKLOG.md) § Fill placement, Hole 1 row
("**Next:** design the routing rule").

**Companions:**
- [settled.md](settled.md) § B step row. The 2026-09-22 cross-link says
  "single-lag keep safe" holds for the 35 late fills as heard, not as a rule.
  Its reopen trigger is a late-of-both step fill heard as audible in this
  plan's Phase 3.
- [settled.md](settled.md) § C α row. The 2026-09-22 caution says "late up to
  +125 ms clean" is not a bound.
- [archive/TEMP-dual-fit-grow-post-plan.md](archive/TEMP-dual-fit-grow-post-plan.md)
  covers the post-lock ladder. This plan reuses it unchanged.
- Memory threads: seam-splice dual-fit (the validated W5 mechanism) and the A7
  re-validation window fix. Re-validation uses `try_dual_fit`'s own seam
  scores; this plan must not bypass that.

**Media hygiene:** use the pair index and gap index only. Listen and render
artefacts stay in gitignored `gap-files/`.

---

## 0. What this tests (normative)

**When both shoulders are licensed and the placed single-lag fill misses
them by more than a bound, does trying dual-fit first produce fills that are
ear-better, and never ear-worse? "Trying dual-fit first" means keeping the
single-lag fill whenever dual-fit declines.**

This sentence is the whole plan; later sections only implement it. If a later
sentence disagrees with it, this sentence wins.

**The defect, stated.** Dual-fit is reachable only as a gate-fail fallback
(`skip_or_dual_fit`, entered on `SeamGateFailure`). A single-lag fill that
clears the seam gate is kept even when it contradicts both licensed shoulder
lags. The placement objective (0.35 × structure + 0.65 × waveform_min) is not
a registration objective. On voiced speech the seam Pearson is saw-toothed
with a 20–30 ms period, so the search picks whichever tooth wins, and a tooth
can sit well off both shoulders while still scoring above the gate.

The miss comes in two kinds, and the trigger catches both:

- **Step straddle.** The shoulders disagree (a map step), so no single lag
  can satisfy both, and the two seam errors always differ by the step. This
  is the 13/g16 case. Dual-fit's step arm (`dual_fit_step_real: true`)
  remedies it.
- **Common-mode.** The shoulders agree, and the fill sits off both by about
  the same amount. A single-lag fill at the shared shoulder lag would fit
  both seams. The 26/g6 case is this kind. Dual-fit's rigid seam-lock arm
  (`dual_fit_step_real: false`) remedies it.

A gap is **step-kind** when |post lag − pre lag| > 20 ms, and **common-mode**
otherwise. Every count, tag and decision below is kept per kind.

## 1. Evidence (why this is motivated)

| Case | Placement | Seam errors (pre / post, ms) | Ear |
|---|---|---|---|
| 13/g16 production | single-lag, gate pass (r 0.62 / 0.997) | −73 / −110 | **audible**: 73 ms repeat at the pre seam |
| 13/g16 widened refine (counterfactual) | single-lag | +88 / +51 | **audible**: hard stop mid-word ("better but not right") |
| 13/g16 forced dual-fit | SilenceSplice, 37 ms step inside speech | ≈10 / ≈10 | **clean**: indistinguishable from B |
| 26/g6 | single-lag | −901 | audible |
| 41/g5 | single-lag | −30 | clean |
| α's 5 clips | single-lag, late | ≤ +125 at the nearer seam | clean |

- 13/g16 shoulders (`lag_decision`, B-at-nominal frame): pre +236.1, post
  +273.1 (r 0.98 / 0.99), a 37 ms step. `ffprobe` cannot attribute the step to
  A or B from the containers; see the BACKLOG row. Attribution is not needed
  to route.
- 43-gap audio census, far-seam error: median +54.5 ms, max +96.7 ms. Only 3
  of these gaps have been heard.
- **Corpus distribution (2026-09-22, measured — replaces the provisional
  reconstruction).**
  - Source: the Phase 0 `--patch-only` rerun, 53 pairs, read straight from
    `registration_residual_pre_ms` / `_post_ms`. No fingerprint-dump join and
    no head-trim solve, so the reconstruction's 13 unresolved gaps are gone:
    **0 patched gaps are missing a per-seam value.**
  - Scope: 327 patched gaps over the 52 pairs that ran (labels go to 54; 6
    and 16 are duplicate media and were never run, so `-ExcludePairs 6,16` is
    a no-op on this run and the earlier diff);
    118 took the dual-fit arm and are excluded per § 3's filter
    (`dual_fit_used == false`), leaving **209 single-lag gaps over 48 pairs**.
    Every one has both shoulders licensed, so invariant 4 excludes nothing.
  - Frame: the **written frame** (report value + the 10.0 ms crossfade width,
    § 3), in which 13/g16 reads
    −72.8 / −109.8. The rerun confirms the constant on the whole corpus, not
    just the 43-gap census: 13/g16's report pair is −82.79 / −119.79.

  | | ms | provisional |
  |---|---|---|
  | max seam error, median | 44.3 | 44.4 |
  | p75 | 61.2 | 61.2 |
  | p90 | 73.5 | 74.1 |
  | p95 | 90.3 | 93.4 |
  | max | 894.5 (26/g6) | 894.5 |

  Per kind: step-kind n = 48, median 56.8, p90 79.2, max 174.0; common-mode
  n = 161, median 39.0, p90 71.1, max 894.5. Step-kind sits about 18 ms
  higher at the median — the step is a real contributor to the far-seam
  error, not a bystander.

  Direction: 187 fills are late of both shoulders, 12 early of both, and 10
  have errors of opposite sign. 48 gaps have a shoulder step larger than
  20 ms. Routed count by candidate bound:

  | Bound | Routed | Step-kind | Common-mode |
  |---|---|---|---|
  | > 50 ms | 84 gaps / 42 pairs | 34 | 50 |
  | > 75 ms | 20 / 15 | 5 | 15 |
  | > 100 ms | 7 / 6 | 2 | 5 |

  **The > 75 ms and > 100 ms rows are unchanged from the provisional table,
  gap for gap** — same 20 gaps, same 15 pairs, same 5/15 split. The
  reconstruction was accurate where it mattered; the correction is confined
  to the > 50 ms row (79 → 84 gaps, 39 → 42 pairs), whose per-kind split was
  previously unavailable.

  The two kinds within the > 75 ms set, now with both seams measured
  (pre / post, written frame, ms):
  - **Step:** 19/g2 (−110 / −174), 13/g16 (−73 / −110), 29/g20 (+62 / +97),
    44/g11 (+94 / +57), 5/g5 (+55 / +91). Steps 34–64 ms.
  - **Common-mode, grossly off:**
    - 26/g6 at −894 / −891 (heard audible);
    - 17/g34 at −520 / −514;
    - 3/g10 (+135 / +150), 13/g30 (+143 / +141) and 53/g2 (+120 / +117).
  - **Common-mode, modestly off:**
    - 1/g14 at −88 / −82 (early);
    - 13/g28, 51/g4, 53/g8, 9/g7, 31/g8, 11/g10, 3/g7, 3/g2 and 23/g4, all
      73–93 ms late.

    31/g8 was heard clean on 2026-09-21. α's 5 clips were heard clean up to
    +125 ms late.

  Most of the > 75 ms tail is common-mode, not step. The late skew means a
  single bound mostly catches late fills. **Magnitude alone does not predict
  audibility:** α's late +125 was clean, and 13/g16's +88 / +51 was not. Phase
  1 therefore sizes a *candidate* bound, and Phase 3's ear pass is the test.
  Phase 1 does not settle the bound.

### 1b. Coverage is not the obstacle (measured 2026-09-22)

A standing question — *is dual-fit simply more capable than the bracket path,
since its assumptions are weaker?* — was sized from existing fingerprint dumps
with `scripts/measure/dualfit-coverage-census.ps1` (52 pairs, 1063 gaps, 982
measurable; consumer-only, no media, no repair). The script had been built for
exactly this and never run to a verdict.

Read the census's default predicate in two halves, because it mixes
**capability** legs with **routing** legs:

| Population (keep-path `tier: patch` — the bracket gate in isolation, **not** the gaps production patches; see the caution below) | n |
|---|---|
| all | 412 |
| program-quiet donor (B empty too) | 202 |
| **real donor** | **210** |
| real donor, dual-fit capability legs pass (`gate_pass` + `bridge_frames > 0` + `donor_continuous`) | **207 (99%)** |

Only 3 real-donor gaps fail a capability leg (1 gate, 2 continuity). The
declines that make the raw quadrant look bad (171 accept / 241 decline) are
routing legs that do not belong in a counterfactual:

- `eligible` reads the *keep path's* bracket failure stage. For a gap the keep
  path patched, it says nothing about dual-fit (24 gaps).
- `step_real` has been stale since the rigid-fill arm shipped 2026-09-01
  ([settled.md](settled.md) § "G6's `!step_real` arm"): `!step_real` is a
  rescue, not a decline (16 gaps). Pass `-InclRigidFill`, or fix the script's
  default.

**A misregistration explanation was tested and refuted.** The predicate reads
continuity from `donor_interior_aligned` but program-quiet from
`donor_interior_nominal`, so the obvious suspicion was that the quiet read sits
at the wrong registration. Recomputing every donor leg at the **aligned** span
changes nothing: 202 program-quiet at nominal, 204 at aligned, and **zero** gaps
flip from quiet to usable. Those donors are genuinely empty — aligned donor
`rms_db` median **−82 dBFS** (p90 −72) against −44 for the rest. The nominal
read is doing its documented job. This is *not* the known donor defect, which
lives in the scan-time equivalence gate's `donor_silence_fraction`
(`gap_equivalence.rs`) — a different test on a different code path. Do not
merge the two.

**The program-quiet half is not a coverage deficit at all — production never
reaches those gaps.** `tier` is the bracket gate *in isolation*, upstream of the
scan-time equivalence gate (`skip_equivalent_gaps`, on by default since
2026-07-20), so a `tier: patch` gap is not a gap production patches. The dump
already records that gate's own verdict per gap in `equivalence_production`
(`class` + `drop` — by contract "the class the production gate acted on"):

| | n |
|---|---|
| gaps with an equivalence verdict | 1087 |
| `shared_silence` (drop) / `ambient_quiet` (drop) | 662 / 39 |
| `repairable_dropout` / `not_evaluated` (kept) | 288 / 98 |
| `tier: patch` | 413 |
| …of those, `drop: true` | **203** |
| …of the 202 program-quiet-donor gaps, dropped | **199** (185 `shared_silence`, 14 `ambient_quiet`) |

413 − 203 ≈ **210**, which is the real-donor population above and matches the
209 bracket patches in the 2026-09-18 `--patch-only` reports. So the hypothesis
that "bracket patches non-dropouts" is **refuted, not open**: those gaps are
skipped before the patch path runs. It also means dual-fit's donor decline and
the equivalence gate's drop independently agree on 199 of the same gaps — two
different tests, same verdict.

**Consequence for this plan.** Dual-fit's admission test accepts essentially
every gap the bracket gate patches with a real donor, so the capability
question reduces entirely to **placement and fill quality** — which is what this
plan already tests. It also means invariant 1's "decline is harmless" arm will
rarely fire on routed gaps: expect routing to reach dual-fit, not bounce off it.

**Caution for any future census.** Never read `tier: patch` as "production
patched this". Join it to `equivalence_production.drop` first, or the
population is inflated by ~50%. That error is what made the raw quadrant look
like a coverage deficit.

**What no dump can answer today:** whether dual-fit's *placement* would be
better on a gap the bracket path already filled. Phase 1c adds that.

## 2. Invariants (hard; any violation stops the plan)

1. **Routing never converts a patch into a skip.** If dual-fit declines (no
   rescue, a failed `classify_fill_waveform_confidence`, a structure
   failure), the single-lag fill is emitted byte-identical to today's output.
2. **Off means off.** Under `--no-dual-fit`, or with the bound unset
   (default), output is bit-identical to HEAD. Phase 2 verifies this with
   `golden_baseline_invariance` plus a corpus `--patch-only --format json`
   diff on 3 pairs.
3. **The same gate for dual-fit.** A routed dual-fit fill passes the same
   `min_fill_correlation` / marginal / absolute-floor classification as a
   gate-fail rescue. It uses `try_dual_fit`'s own seam scores, and never
   `fill_splice_seam_correlations_interleaved` (the A7 bug).
4. **Both shoulders licensed.** The trigger needs both shoulder lags to clear
   the confident floor. One-sided registration never routes, because
   dual-fit's premise is two known lags.

## 3. Phase 0 — per-seam record (record-only, additive JSON)

Today `registration_residual_ms` keeps the smaller-|·| licensed shoulder
(`select_registration_residual_ms`, `dual_fit.rs:237`). The routing trigger
is about the **larger** one: the far seam is where 13/g16 failed. The
selection throws that information away.

- Add optional per-seam fields next to `registration_residual_ms`:
  `registration_residual_pre_ms` and `registration_residual_post_ms`
  (`None` when that shoulder is unlicensed). They are additive and optional,
  so the JSON contract stays v1 per
  [../json-output.md](../json-output.md). Follow the revision procedure:
  change application-layer DTOs only, regenerate goldens, and update the
  doc's field table.
- Also add `registration_applied_ms`, the written fill's start that the
  residuals subtract from (`align_adjustment_secs` × 1000 plus the Fit head
  trim). `align_adjustment_secs` does not carry the trim, so today the
  per-seam values cannot be rebuilt from the report alone. Measured on the
  2026-09-22 rerun, the trim is nonzero on **25 of 127** bracket gaps (up to
  610 ms), so a fifth of the population would be unreconstructable without it.
- **The field is overloaded on the dual-fit arm — filter before any census.**
  The two arms feed the same helper two different quantities:

  | | shoulder lag fed in | `applied_ms` | so the residual is |
  |---|---|---|---|
  | bracket (`characterize.rs:828–861`) | `seam_local_peak`, an **independent** registration search | real placement | **the seam error** |
  | dual-fit (`dual_fit.rs:480–495`) | `r.pre_lag` / `r.post_lag`, dual-fit's **own** placement | hardcoded `0.0` | **minus the displacement from nominal** |

  `applied_ms = 0.0` is deliberate — dual-fit has no single rigid applied lag,
  so the field mirrors the `align_adjustment_secs: 0.0` that the SilenceSplice
  arm records. The consequence is that a dual-fit fill's *seam error* is ~0 by
  construction and never appears, while a large |residual| means dual-fit
  **corrected** a large offset. The sign of merit is inverted between the arms:
  on bracket, large is the defect; on dual-fit, large is a success.

  This is not a corner case — 84 of 211 patched gaps (40%) took the dual-fit
  path, median |residual| 133.7 ms. Including them in § 4's table takes the
  > 75 ms routed set from **12 to 62**. Pair 31 shows both readings side by
  side: a bracket gap at applied −1073.8 with residual +72.3 (it landed 72 ms
  short), and the next gap, dual-fit, reporting +1146.7 — which is just
  `0 − (−1146.7)`, the pair's known −1.15 s map step read off dual-fit's own
  placement. Same physical offset, opposite meanings.

  Every consumer must filter on `dual_fit_used == false`. Better, before the
  field acquires more readers: suppress it on the dual-fit arm, or give the
  dual-fit quantity its own name. Phase 1c item 3 needs dual-fit's *error*,
  which this field does not contain on either arm.
- **The instrument is settled, and the frame is now attributed (2026-09-22).**
  The constant 10 ms is the **seam crossfade**. `apply_seam_crossfade`
  (`domain/policies/seam_splice.rs:116`) consumes `b_fill[..cf]` into A's
  pre-gap tail and starts the gap interior at `b_fill[cf]`, so B content index
  `k` lands at A frame `gap_start + k - cf`. `registration_applied_ms` is
  assembled plan-side (`align_adjustment_secs` plus Fit's trim head,
  `patch_result.rs:602`) before the writer runs and carries no crossfade term,
  so the report describes the **planned** placement and the WAV carries the
  **written** one, differing by exactly `cf`. At the `crossfade_ms` default of
  10 that is 480 frames at 48 kHz; `effective_seam_crossfade_frames` clamps
  only below `gap_frames / 2`, and with a 500 ms silence hold no corpus gap is
  short enough, which is why the offset is constant across gaps, pairs and
  both error signs rather than varying. `fill_level.rs`'s module note records
  the same displacement independently ("the bin grid sits one crossfade width
  off A's timeline"), from the other side and without connecting it to the
  residual.

  Mechanism and sign are pinned in-process by
  `apply_seam_crossfade_displaces_fill_content_by_the_crossfade_width`, a ramp
  fill asserting `k -> gap_start + k - cf`; the pre-existing crossfade tests
  fill with a constant and are structurally blind to displacement. Mutation-
  checked (dropping the `+ cf` fails it).

  **Name the two frames and stop converting by an unexplained constant:**
  - **planned frame** — what the patch report emits;
  - **written frame** — planned + crossfade width, what a listener hears.

  § 1 quotes bounds in the written frame, since audibility is a property of
  the written fill. Still open, and now a BACKLOG row rather than a Phase 0
  blocker: the advance is **uncompensated**, so every patch's fill carries B
  material 10 ms ahead of plan. Against a 50-100 ms bound that is 10-20% of
  the threshold. Absorbing it by cutting `b_fill` `cf` earlier would make the
  two frames coincide.

  Confirmation left for the § 1b listen run, which needs no extra sweep: render
  one gap at `--crossfade-ms 0`, `10` and `40`. If the written-minus-planned
  offset tracks `cf` 1:1, the crossfade accounts for all of it and not merely
  most.
- **The instrument's shoulder read is settled.**
  - The S1 shoulder read equals `lag_decision` exactly: on every trim-free
    gap, `registration_residual_ms` = `align_adjustment − lag_decision` for
    one side to within 0.1 ms.
  - The constant was measured before it was attributed: on every trim-free gap of the
    43-gap audio census, the report-side residual is exactly 10.0 ms below
    the audio measurement (fill measured in the `--gap-listen` WAV minus
    `lag_decision`). The dual-fit `seam_locks` lags on 13/g16 sit the same
    10 ms late of `lag_decision`.
  - ✅ Attributed above: the splice convention, not a template origin. The
    routing bound is stated in the **written frame**.
- **Preliminary source (done 2026-09-22).** Per-seam values rebuilt from
  existing reports:
  - The other seam's residual = `align_adjustment + T − other_lag`, where the
    head trim T ≥ 0 is solved from `registration_residual_ms`.
  - This resolved 196 of 209 gaps (27 of them needed T recovered).
  - It is ambiguous on 10 gaps and inconsistent on 3.
    - Ambiguous means either side fits the min-selection. This is mostly an
      artefact of a zero step: when the step is 0 both sides give the same
      error, so the error is still known. Only 32/g2 (step 32 ms) is truly
      ambiguous.
    - Inconsistent is probably from mixing a 2026-09-18 report with a
      2026-09-20 dump. The rescue fix (7062431) moved `lag_decision` on 14
      shoulders.
  - Production placement code is unchanged since the 2026-09-18 run. The
    later commits touch `lag_decision`, the fingerprint measure and
    `seam_locks` tests, and 31's placement was checked bit-identical across
    7062431.
  - It is good enough for sizing, not for the record.
- **Accurate source: one corpus `--patch-only` rerun** after the fields land.
  **Done 2026-09-22** — it replaced § 1's table, resolved all 13 gaps, and
  came back bit-identical at the gap level (§ 7). The 2026-09-18 run took
  about 6.5 h for 53 pairs, with no oracle cost.
- **Done when:**
  - ✅ all three fields appear in `--patch-only --format json` on the corpus;
  - ✅ the 10 ms is attributed and the bound's frame is named;
  - the goldens are regenerated and `.\scripts\test-tier.ps1 -Tier pr` is
    green.

## 4. Phase 1 — sizing and scope (corpus, `--patch-only`, no oracle)

**Sizing pass done 2026-09-22 on the Phase 0 rerun** — § 1's table is now
measured, not reconstructed. For every gap that is patched single-lag with
both shoulders licensed, `max_seam_err = max(|pre|, |post|)` and the kind
(§ 0) are computed from the report's per-seam fields.

- ✅ Distribution per kind: recorded in § 1 (step n = 48, median 56.8, p90
  79.2, max 174.0; common-mode n = 161, median 39.0, p90 71.1, max 894.5).
- ✅ **Pre-registered candidate bounds** 50, 75 and 100 ms, per kind, with the
  routed gaps listed by pair/gap index: § 1.
- **Dual-fit feasibility on the routed set.** Until Phase 2's flag exists, a
  forced-dual-fit run is the only way to know whether dual-fit passes its own
  gate there. That is the 13/g16 recipe: patch at a raised floor so the gate
  fails and dual-fit runs.
  - It costs about 6 min and up to 19.8 GB peak per pair, so run one pair at
    a time under a memory watchdog.
  - Count route → rescue against route → decline, per kind. A decline is
    harmless by invariant 1 but is wasted work.
  - The fingerprint's dual-fit reproduction already passes the gate on 13/g16
    and 19/g2 (BACKLOG Hole 1 row).
  - Once the flag exists, re-read these counts from a flagged run.
- **Named gaps.**
  - 13/g16 is the known step positive.
  - 26/g6 is the known common-mode positive (audible at −894).
  - 1/g14 is common-mode at −88 / −82 and routes at 50 and 75.
  - **32/g13 does not route at any candidate bound:** max 34 ms, −26 / −34,
    step 8 ms. BACKLOG named it for the ear pass as one of the 7 early fills
    at or below −30 ms. It stays in § 6 as a **negative control**, not a
    cohort member.
- **Scope decision (replaces the earlier stop condition).** The measured
  table leaves no doubt a cohort exists: 20 gaps at > 75 ms and 7 at > 100 ms.
  The open question is which kind the rule is for. Choose one of:
  - (a) step-kind only;
  - (b) both kinds, with a separate bound per kind;
  - (c) common-mode only above a gross bound.

  Phase 1b's ear probe decides this. Record the choice and its reason in
  § 7.
- **Done when:**
  - ✅ the per-kind routed-count table from the rerun is recorded (§ 1, § 7);
  - the feasibility counts for the chosen scope are recorded;
  - the scope is chosen.

### 4b. Phase 1b — common-mode ear probe (production fills only)

**Question.** Are the common-mode fills that a 75–100 ms bound would route
audible as they stand? If they are clean, routing them spends dual-fit work
and risk for nothing, and the common-mode bound should sit above them.

This is a **single-arm** listen of production's own fill. No dual-fit render
is needed, so it is cheaper than § 6's paired listen and can run before
Phase 2 exists.

- **Cohort.**
  - The modestly-off common-mode set (74–93 ms): 13/g28, 51/g4, 53/g8, 9/g7, 11/g10,
    3/g7, 3/g2 and 23/g4, all late, and 1/g14, the only early one.
  - The grossly-off late set (115–150 ms): 3/g10, 13/g30 and 53/g2.
  - About 12 gaps, filtered with `--fingerprint-gap` so each pair pays only
    for the named gaps.
- **Already heard, not re-rendered:**
  - 31/g8 (late about 82 ms, clean);
  - α's 5 clips (late up to +125, clean);
  - 26/g6 (−894, audible);
  - 17/g34 (−520, ear *cannot tell*).
- **Render.** Default 3 s `--gap-listen` context (widening it moves the
  patch). Mix in 2–3 clean-by-ear controls from the < 50 ms bulk, blind to
  the listener, so "clean" is calibrated against the listener's own
  false-alarm rate.
- **Score.** Clean / audible / can't tell, blind to each gap's error value.
- **Pre-registered reading:**
  - **All modestly-off late gaps clean:** there is no motivation to route
    late common-mode fills under 100 ms. The common-mode bound goes above the
    late set, or common-mode stays out of scope (scope (a) or (c)).
  - **Any modestly-off late gap audible:** common-mode enters scope (b), and
    those gaps join § 6's paired cohort.
  - **Early (1/g14) heard separately:** early misses repeat material (the
    13/g16 pre seam), while late ones skip it. Direction may matter more than
    magnitude, so a clean late set does not clear early fills.

  Record the outcome in § 7 and update the § C α row either way. This is the
  first listen set on late fills chosen by error magnitude rather than by
  standoff.

### 4c. Phase 1c — dual-fit counterfactual in the fingerprint (Change, additive dump fields)

**Question this closes.** For a gap the keep path patched, *where would
dual-fit have put the fill?* § 1b shows dual-fit would accept almost all of
them; nothing on disk says whether its placement differs, or by how much. That
delta is the routing rule's own evidence, and it is measurable without a single
listen.

**Why it is cheap.** `compute_region_measurements` (`overlay.rs`) already
computes `splice_dualfit` **unconditionally**, patched or skipped, and the
`SeamLocks` it is built from carry everything below. Nothing here needs a new
decode or a new search, so there is no measurable cost on a corpus fingerprint
run — but items 2 and 3 are **new emissions, not un-suppressions**. An earlier
draft of this section claimed they were already computed and merely hidden;
that was wrong, and § 4c's cost was understated as a result. See the correction
under item 2.

What *is* suppressed is only the derived verdict: `dual_fit_rescue_flag`
returns `None` when `patched` (`gap_fingerprint/schema.rs`, "a patched gap never
reaches `skip_or_dual_fit`"). That is a statement about production control flow,
not about data availability, and it is item 1.

**Change, three additive fields/blocks:**

1. **Emit the verdict on patched gaps.** ✅ **Done 2026-09-22**, as a new
   top-level `dual_fit_counterfactual` block rather than by un-suppressing
   `dual_fit_rescue_flag`: un-suppressing would change the meaning of a shipped
   field, a new block does not. The flag keeps its production meaning (`None`
   once a gap is patched) and is now *expressed on top of* the counterfactual
   (`dual_fit_rescue_flag` returns `counterfactual.would_rescue`), so the two
   definitions cannot drift. The block carries `would_rescue` plus all six legs,
   `reached_in_production` (`false` ⇔ the gap was patched ⇔ the flag is `None`),
   and `program_quiet_silence_frac`, the threshold the one thresholded leg was
   decided at. Top-level rather than inside `outcome` because `outcome`'s
   contract scopes it to the bracket gate, and this block deliberately speaks
   about gaps that gate never declined. Written on the projection path, which is
   the single write site both the from-decode dump and the spec projection pass
   through. Contract `DUAL_FIT_COUNTERFACTUAL_CONTRACT`, `not:` naming
   `outcome.dual_fit_rescue`. Documented in `docs/dev/gap-fingerprint.md`
   § *`dual_fit_counterfactual`*; `docs/json-output.md` needed no change (it is
   the CLI patch-report contract, not a dump key). The point is that the
   predicate lives in Rust, evaluated once, rather than being re-implemented by
   each consumer — § 1b's stale `step_real` leg is what that costs.

   **Including the donor legs.** `donor_interior_aligned` and
   `donor_interior_nominal` already carry `rms_db`, `silence_fraction`,
   `longest_silence_ms`, `continuous` and `basis` — the *numbers* are not
   missing. The **decision** is: `program_quiet` is computed at
   `gap_fingerprint/schema.rs:1131` for the rescue flag and discarded, and
   production's `domain::donor::program_quiet_at_nominal` never reaches the
   dump. § 1b's analysis therefore had to re-threshold
   `PROGRAM_QUIET_SILENCE_FRAC` (0.5) in a throwaway script — the same
   re-derivation this item exists to stop. Emit the program-quiet and
   continuity verdicts production acts on, next to the raw blocks.

   **The equivalence gate is already done right — copy it.** `skip_equivalent_gaps`
   emits its verdict per gap as `equivalence_production` (`class`, `drop`, both
   sides' silent-block counts, the floors, the thresholds it used and its
   `donor_registration`), with `equivalence_diagnostic` as a second opinion
   marked "gates nothing". That is the pattern the donor and dual-fit verdicts
   should follow: the decision, the inputs it was made from, and the thresholds,
   all emitted together. Nothing new is needed for the equivalence gate itself.

   **Do not add an aligned program-quiet read.** The policy is nominal by name
   and by construction, and `donor_interior_nominal`'s contract calls that
   registration-independence deliberate. An aligned variant would be a policy
   nothing reads, and § 1b measured that it would decide nothing: zero of 412
   gaps flip between the two registrations.
2. **Emit dual-fit's absolute seam placement.** ✅ **Done 2026-09-23** as the
   top-level `dual_fit_placement` block (`DUAL_FIT_PLACEMENT_CONTRACT`). This is
   the item the phase turns on, and the one an earlier draft got wrong.

   **The existing `*_seam_lag_ms` keys are not placements — do not build on
   them.** `splice_dualfit.pre_seam_lag_ms` / `post_seam_lag_ms` are
   `frac_lag_ms` from a Pearson peak measured **at dual-fit's own seam lock**
   (`measure/probes.rs:442`), so they are the residual error *after* placement,
   which is ~0 by construction. Measured over the 2026-09-20 dump, 983 gaps:

   | | med | p99 | max | > 1 ms |
   |---|---|---|---|---|
   | `\|pre_seam_lag_ms\|` | 0.0052 | 0.0103 | **0.0104** | 0 |
   | `\|post_seam_lag_ms\|` | 0.0051 | 0.0103 | **0.0104** | 0 |

   0.0104 ms is exactly half a sample at 48 kHz: the integer lag is *always*
   zero and only the sub-sample interpolation term survives. These keys are a
   seam-lock quality check, and they are already emitted on every gap, patched
   included — nothing about them is suppressed, and nothing about them answers
   this phase's question.

   **Emit the locks instead.** `SeamLocks` (`domain/seam_locks.rs:126–151`)
   carries `pre.lag` / `post.lag` — the absolute accepted lag per shoulder — and
   `b_pre_seam` / `b_post_seam`, documented as "the matched B frame for each
   shoulder — **the placement a fill would be cut at**". That is exactly the
   quantity wanted, and `locks` is already in scope at the emission site:
   `probes.rs:446–449` reads `locks.pre.rescued` and `locks.pre.edge_pinned`
   from the same struct. Emit the lag (and the B frame) beside them, with
   `pre.r` / `pre.z` so the placement can be weighted by how well it is
   determined.

   **State the frame in the `_contract`.** Three frames are already in play —
   `lag_decision`'s B-at-nominal, the patch report's frame (§ 3's unattributed
   −10 ms, now attributed to the seam crossfade), and the written frame § 1's
   bounds are quoted in. A placement emitted
   without its frame named is not comparable to anything.

   **As built.** A new top-level `dual_fit_placement` block, *not* new fields on
   `splice_dualfit`: that block's own contract says its `*_seam_lag_ms` are
   residual **at** the lock and never a placement, so an absolute placement
   inside it would contradict the contract it ships under. Fields: `pre_lag_ms`
   / `post_lag_ms` and the same two in frames, `b_pre_seam_frames` /
   `b_post_seam_frames` (+ a `_secs` view), and the **unclamped** pair, kept
   because the `.max(0)` clamp can manufacture an abut. `r` / `z` are
   deliberately *not* duplicated — `splice_dualfit.pre_seam_r` / `pre_seam_z`
   already **are** these locks' own, single-sourced from `SeamLocks`, so the
   weighting the item asked for is available without a second definition to
   drift. No `step_ms` either: `trim_frames` is already the step.

   `splice_dualfit_at` now returns `DualFitProbe { summary, placement }` — both
   readings off the one `SeamLocks`, so no second ±600 ms seam search is paid.
   It travels as a **from-decode overlay** (`RegionMeasurements` → stamped onto
   `fp` in `characterize.rs` beside the window ladders), not through the domain
   spec: only the decode path measured locks, so the spec-projection and oracle
   paths honestly emit nothing rather than a reconstruction.

   The contract states the frame on its `placement` and `window` axes: B
   timeline, each lag relative to that shoulder's nominal center, **planned**
   frame with no crossfade term — the same frame `registration_applied_ms` is
   quoted in, so the two difference directly. Its `not:` names
   `splice_dualfit.pre_seam_lag_ms` outright. Documented in
   `docs/dev/gap-fingerprint.md` § *`dual_fit_placement`*; `docs/json-output.md`
   needed no change (it is the CLI patch-report contract, not a dump key).

   The unit test is the one fixture whose geometry is known by construction
   (B continuous, A with 1.2 s spliced in): pre locks at nominal, the rescued
   post lock reads exactly −1200 ms, and both cut B at the same frame with
   nothing clamped. That is the frame claim being checked, not just the
   plumbing.
3. **Emit the per-shoulder delta against the keep-path fill.** ✅ **Done
   2026-09-23**, by join, in `census-registration-residual.ps1
   -FingerprintDir`. Paired with
   Phase 0's `registration_residual_pre_ms` / `_post_ms`, this puts **both arms'
   placements on the same gap** — the direct comparison, with no listening.

   The dump has no keep-path placement to difference against: its `outcome`
   block carries `plan_kind` / `tier` / `skip_reason` / `dual_fit_rescue` and no
   geometry, and its characterize path is a probe that never splices. So either
   emit the keep-path bracket placement into the dump as well, or compute the
   delta in the census script by joining the dump to the `--patch-only` report
   on `geometry.a_start_secs` rounded to 1 ms. **Prefer the join**: differencing
   two absolute, frame-stamped numbers is arithmetic, not a re-derived decision,
   and it is decisions (item 1) that must not live in a consumer.

   **As built.** `-FingerprintDir` joins the patched gaps to a fingerprint dump
   on pair label + A start to the millisecond (never the gap index — the two
   dumps enumerate different populations) and prints, over bracket gaps,
   `delta_s = dual_fit_placement.<s>_lag_ms − registration_applied_ms` per
   shoulder, plus their mean (the common-mode half) and difference (the step
   half `trim_frames` already gave), and `delta_s + registration_residual_<s>_ms`
   — ~0 exactly when dual-fit lands where the keep path's own residual says it
   should have. Both terms are planned-frame, so `-FrameOffsetMs` is *not*
   applied to them; the seam census keeps its written frame. Dual-fit rescues
   are tallied apart, since `applied_ms` is hardcoded 0.0 on that arm.

   **The join key is verified, the payload is not yet available.** Run against
   the 2026-09-18 patch dump and the 2026-09-20 fingerprint dump, all **327**
   patched gaps join against 1087 fingerprint gaps with **zero** misses — two
   independent runs, so the key is sound. Both dumps predate the placement
   block, so the section correctly reports that there is nothing to difference
   yet. It needs the filtered run below.

   **What is recoverable today, without any of this.** `trim_frames =
   bridge_frames − gap_frames` is the *difference* between the two seam lags.
   Verified on the 2026-09-22 reports: 84 dual-fit gaps joined on
   `a_start_secs`, and `|trim_ms|` equals the patch-side `|post − pre|` within
   1 ms on **83 of 84**. So dual-fit's step geometry is already on disk
   corpus-wide; only the common-mode component — both seams displaced together,
   and the larger half of § 1's routed set — needs item 2.

**Cost:** emission plumbing for items 1–3, one `_contract` entry per new key
(the macro requires `measures` / `placement` / `window` / `not` — the
aligned-vs-nominal confusion in § 1b and the `*_seam_lag_ms` misreading above
are both exactly what those lines exist to prevent; the `not:` line on a new
placement key should name `*_seam_lag_ms` directly), and golden regeneration.
Larger than the "un-suppress a flag" framing of the earlier draft, though still
small. Fingerprint dumps are not the frozen v1 CLI contract, but check
`docs/json-output.md` before assuming a key is free.

**Sequencing.** Fold this into the same Change as Phase 2, so **one** corpus
fingerprint run serves both this and the routing work. A corpus run costs hours
and ~15 GB RSS; two would be a self-inflicted cost. It does not block Phase 0 or
Phase 1, which read `--patch-only` reports.

- **Done when:**
  - a patched gap's dump carries dual-fit's verdict, its **absolute** per-shoulder
    placement with the frame named in the `_contract`, and the per-shoulder delta
    against the keep-path fill;
  - ✅ `dualfit-coverage-census.ps1` reads the emitted verdict instead of
    re-deriving it (done 2026-09-22): it reports `verdict_source` per gap and
    rolled up, so a mixed corpus cannot average an emitted verdict against a
    re-derived one, and its fallback for pre-2026-09-22 dumps mirrors the
    **current** predicate — `step_real` is gone, replaced by the
    `post_seam_global_r` finiteness leg production actually applies, and
    `-StepRealMargin` / `-InclRigidFill` are removed (the relaxed "rigid-fill"
    predicate they contrasted against *is* the predicate now). On the 2026-09-20
    dump the corrected predicate moves the decisive quadrant to 229 of 412
    keep-path patches, which is a measurement of the stale leg's cost, not a
    change in production;
  - the delta distribution over the corpus is recorded in § 7, per kind, with
    the common-mode half distinguished from the step half that `trim_frames`
    already gives.
- **Reads out as:** the § 6 cohort can then be sampled on *measured placement
  disagreement* rather than on the single-lag residual alone. Where both arms
  land within a few ms of each other, routing cannot change what is heard, and
  those gaps need no listen.

## 5. Phase 2 — implementation (flag, default off)

**Knob.** `dual_fit_registration_bound_ms: Option<f64>` on
`PatchAudioRequest`. It is exposed as TOML `dual_fit_registration_bound_ms`
and CLI `--dual-fit-registration-bound-ms <MS>` in
`infrastructure/cli/args.rs`, next to `--dual-fit` (~250). If Phase 1 picks
scope (b), it becomes two knobs, `…_step_bound_ms` and `…_common_bound_ms`.
Size the knob shape on the scope decision, not before. Precedence follows
the standard defaults → TOML → CLI order. `None` means no routing. The flag is
ignored with a warning when `--no-dual-fit` is set.

**Refactor, `dual_fit.rs`.** Split `skip_or_dual_fit` (`:261`) so that the
attempt does not need a `SeamGateFailure`:

```text
try_dual_fit_rescue(progress, request, region, attempt: DualFitAttempt)
    -> Option<(DualFitRescue, SeamScoreAttempt)>
        // grow-post ladder + dual_fit_from_locks_with_grown_post
        // + classify_fill_waveform_confidence — the body that is
        // today inside `if dual_fit_eligible(..)`
skip_or_dual_fit(..fail..)  // unchanged behaviour: eligibility check,
                            // then try_dual_fit_rescue, else skip
```

This is a pure extraction. `dual_fit.rs`'s existing unit tests and
`patch_audio_dual_fit_rescues_*` (`patch_audio_integration.rs:1737`, `:1823`)
must pass unmodified; that is the proof the extraction is behaviour-neutral.

**Hook, `characterize.rs`.** Put it on the gate-Ok arm, after
`registration_residual_ms` is computed (~804–858). `seam_locks` and
`dual_fit_input` are still in scope there (they are built ~395–470, before the
gate).

```text
if let Some(bound) = request.dual_fit_registration_bound_ms
   && request.dual_fit
   && both shoulders licensed
   && max(|pre_residual|, |post_residual|) > bound
{
    if let Some(rescue) = try_dual_fit_rescue(..) {
        emit the dual-fit fill via the existing finalize_dual_fit path,
        tagged route = registration_step | registration_common (§ 0 kind)
    }
    // else: fall through to the single-lag fill, unchanged (invariant 1)
}
```

The predicate is a pure function
`registration_routes_to_dual_fit(bounds, pre, post) -> Option<RouteKind>` in
`dual_fit.rs`. The kind comes from |post − pre| > 20 ms, since the residual
difference equals the shoulder step. Unit-test it:
- both licensed and above the bound → `Some(kind)`, for each kind;
- one unlicensed → `None`;
- at the bound → `None` (strict `>`);
- `None` bound → `None`;
- the 20 ms kind boundary.

**Report tag.** Add an optional `dual_fit_route` field to the patch record,
`"gate_fail" | "registration_step" | "registration_common"`, and fill it for every `dual_fit_used:
true` patch. It is additive, so the contract stays v1. Regenerate goldens and
document the field in `json-output.md`. The human report
([../cli-output.md](../cli-output.md)) gets a one-word suffix on the existing
dual-fit line; keep the layout otherwise unchanged.

**Tests.**
- Unit: the predicate (above) and the extraction-neutrality tests (existing).
- Integration, `patch_audio_integration.rs`: a synthetic step fixture where
  the single-lag fill passes the gate but straddles a known step. Assert:
  - with the bound set → `dual_fit_used: true`, `dual_fit_step_real: true`
    and `dual_fit_route: registration_step`;
  - with the bound unset → today's single-lag patch, bit-identical.
- Integration, common-mode: a fixture with agreeing shoulders where the
  single-lag fill is forced off both. If voiced content cannot make the gate
  pick a wrong tooth reproducibly, use a periodic signal at a known period.
  Assert `dual_fit_step_real: false` and `dual_fit_route:
  registration_common`. Needed only if the scope includes common-mode.

  No step fixture exists in `clip-sync-repair-fixtures` yet. Build it there
  (voiced periodic content on both shoulders, a B-side step of about 40 ms
  within ±600 ms reach), not in `tests/common/`.
- Integration: a decline case. The step is present but dual-fit's seams fail
  the floor; assert that the single-lag fill is emitted unchanged
  (invariant 1).
- Gate: `.\scripts\check-repair-test-manifest.ps1`,
  `.\scripts\test-tier.ps1 -Tier pr`, fmt and clippy.

**Done when:**
- the flag lands default off;
- invariant 2 is verified (goldens plus the 3-pair diff);
- the tests above are green.

## 6. Phase 3 — paired ear pass and default decision

Render the routed cohort for the scope chosen in § 4 as **paired** clips:
single-lag (production today) and routed dual-fit, at the default 3 s
`--gap-listen` context. Do not widen the context knob; that moves the patch.
Splice into a wider raw clip if lead-in is needed.

**Stratify by kind.** Use at least 3 gaps of each in-scope kind, plus the
32/g13 negative control, which should route nowhere and sound the same.

- **Step:** 19/g2, 29/g20, 44/g11 and 5/g5.
  - 19/g2 is masked by a B-side voice; the ear was *unsure*.
- **Common-mode:** from § 4b's audible set, plus 26/g6.
- **Already scored, not re-rendered:** 13/g16 (dual-fit clean, single-lag
  audible).

The listener scores each pair as dual-fit better / same / worse, blind to
which arm is which.

**Pre-registered decision rule, applied per kind:**
- **Adopt as default** at that kind's bound if at least one routed gap is
  dual-fit-better (besides 13/g16 for step-kind, and besides 26/g6 for
  common-mode) **and** zero are dual-fit-worse.
- **Tighten** (raise the bound to exclude them, then re-listen only the
  excluded set's neighbours) if any are dual-fit-worse.
- **Keep opt-in** (settled § C, latched on "a second audible fill of that
  kind") if all are "same".
- **Refute** (settled § A) if dual-fit-worse outnumbers dual-fit-better.

The kinds can land differently, for example step adopted and common-mode
opt-in; the knob shape from § 5 allows that.

Any late-of-both single-lag fill heard as audible here, or in § 4b, fires the
§ B step row's reopen trigger. Update that row in the same close.

## 7. Results

**Phase 1c item 1 landed — `dual_fit_counterfactual` (2026-09-22).** Dual-fit's
admission decision and all six of its legs now ship on **every** gap, patched
included, from one Rust predicate that `dual_fit_rescue_flag` is expressed on
top of. Details and the reasoning in § 4c item 1. Two consequences worth
recording here:

- **The stale leg had a measurable cost.** `dualfit-coverage-census.ps1` now
  reads the emitted verdict; correcting its fallback predicate (drop
  `step_real`, add the `post_seam_global_r` finiteness leg) moves the decisive
  "keep-path patches / dual-fit declines" quadrant to 229 of 412 keep-path
  patches on the 2026-09-20 dump. That is the census being wrong before, not
  production changing — exactly the class of error § 4c item 1 exists to stop,
  and the reason the predicate now lives in one place.
- **No corpus number in § 1b is invalidated.** § 1b's verdict rests on the
  capability legs (`gate_pass`, donor, shoulders), which are unchanged; it is
  the raw 42% accept rate — already identified there as an artefact — that the
  correction quantifies.

Still open in Phase 1c: items 2 (the absolute per-shoulder locks) and 3 (the
per-shoulder delta, by join). No corpus run has yet written the new block, so
every existing dump reads as `verdict_source = derived`.

**Phase 1c items 2 and 3 landed (2026-09-23).** `dual_fit_placement` ships the
absolute per-shoulder lock placement — planned frame, stated in its contract —
and `census-registration-residual.ps1 -FingerprintDir` differences it against
the keep path's `registration_applied_ms` by join. Details in § 4c items 2 and
3. What this does **not** yet give is the distribution: no dump on disk carries
the block, so the remaining Phase 1c done-when (the delta distribution per kind,
common-mode distinguished from the step half) is waiting on the filtered run,
not on any further code. The join key itself is already proven — 327 of 327
patched gaps join across two independent runs, zero misses — so that run is the
only thing between here and the number.

**Coverage census (§ 1b) — done 2026-09-22.** Full numbers and the two refuted
explanations are in § 1b. Verdict: dual-fit's admission test is not what limits
it — 207 of 210 keep-path patches with a real donor pass its capability legs.
The raw 42% accept rate is an artefact of two routing legs in the census
predicate (`eligible`, and the stale `step_real`) plus an inflated population
(`tier: patch` counts 203 gaps the equivalence gate drops). Two side
hypotheses died here and should not be re-proposed: donor misregistration
(zero of 412 gaps flip between registrations) and "bracket patches
non-dropouts" (199 of those 202 never reach the patch path). The remaining
question is placement quality, which Phase 1c instruments.

**Processing validation on the partial rerun (33 pairs, 211 patched gaps) —
done 2026-09-22.** Run at pair 33 of 53; every check below is independent of
the remaining pairs, and every one of them held on the full run below.

- **Placement is bit-identical.** `diff-patch-outcomes.ps1` over 746 joined
  gaps: **0 changed**. The only differences are four run-level fields, all of
  pair 31's `injected_time` block disappearing — the known, already-validated
  consequence of 7062431. Phase 0 changed no placement; invariant 2's spirit
  holds on the real corpus, not just the goldens.
- **The § 1 reconstruction was accurate.** Restated in the written frame, the
  measured bracket set tracks the provisional table closely (median 43.5 vs
  44.4, p75 60.5 vs 61.2, p95 96.7 vs 93.4, max 894.5 vs 894.5 exactly).
  Remaining differences are subset effects. § 1's table can be replaced
  wholesale when the run finishes, not reinterpreted.
- **The −10 ms is confirmed, constant and signed:** planned (report) frame =
  written frame − 10.0 ms, on all 14 plan-named gaps, both error signs, 10 pairs. 13/g16
  reads −72.8 / −109.8 and 32/g13 reads −26.0 / −34.0 — both exact. Still
  unattributed (§ 3), but it is a fixed frame offset, not an artefact of the
  new fields, and a bound converts by +10 ms with no ambiguity.
- **The selection and licensing assumptions hold.** `registration_residual_ms`
  is the smaller-|·| seam in 211/211, and `one_seam` **never occurs** — both
  shoulders are licensed on every patched gap, so invariant 4 excludes nothing
  and the census `kind` needs only two values, not three.
- **Two processing defects found, both recorded where they bite.** The patch
  field is overloaded on the dual-fit arm (§ 3) and the dump's
  `*_seam_lag_ms` keys are post-placement residuals, not placements (§ 4c
  item 2). The second one invalidated Phase 1c's central premise and would
  have surfaced as a column of zeros *after* a corpus fingerprint run.
- **Gap indices do not join.** The dump's `gap.index` is 0-based and the patch
  report carries no index at all, so plan `gN` = dump index `N` = report array
  position `N+1`, confirmed on all 14 named gaps. Report position 6 in pair 26
  is a +25.8 / +56.4 gap, so rendering "26/g6" by position would put the wrong
  clip in front of § 4b's listener. Join on `geometry.a_start_secs` rounded to
  1 ms, always.

**Phase 0 rerun complete, and Phase 1's table is measured (52 pairs) — done
2026-09-22.** The `--patch-only` corpus rerun finished. It carries all three
Phase 0 fields on every patched gap. Regenerate the table with
`scripts/measure/census-registration-residual.ps1 -DumpDir <run>`, whose seam
census section is the durable form of the numbers below.

- **Placement is bit-identical over the whole corpus.**
  `diff-patch-outcomes.ps1` over 52 pair labels and **1087 joined gaps: 0
  changed**, with the three Phase 0 fields excluded by `-IgnorePaths` (981
  leaf hits, all new-field additions). The only differences are the same four
  run-level fields as the partial: pair 31's `injected_time` block
  disappearing, the already-validated consequence of 7062431. The script
  exits 1 only because that run-level count is not zero; the gap-level bar —
  unexplained = 0 — is met. **Invariant 2 holds on the real corpus.**
- **The reconstruction is retired.** § 1's table is now read straight from
  `registration_residual_pre_ms` / `_post_ms`, with no head-trim solve and no
  dump join. All 209 single-lag gaps resolve; the 13 unresolved and the
  ambiguity on 32/g2 are gone. 327 patched gaps, 118 of them dual-fit and
  filtered out per § 3.
- **The > 75 ms cohort is confirmed gap-for-gap.** The measured > 75 and
  > 100 ms rows are identical to the provisional ones — same 20 gaps, same 15
  pairs, same 5 step / 15 common split, same 7 at > 100 ms. Distribution
  moved by under 3 ms at every quantile (median 44.3 vs 44.4, p90 73.5 vs
  74.1, p95 90.3 vs 93.4). **The correction is confined to the > 50 ms row**
  (79 → 84 gaps, 39 → 42 pairs), which is also where the per-kind split was
  previously unavailable: 34 step / 50 common.
- **Step-kind is not a bystander.** Split by kind for the first time on the
  full corpus: step-kind n = 48, median 56.8 ms; common-mode n = 161, median
  39.0 ms. The step-kind median sits ~18 ms higher. Common-mode still owns
  the extreme tail (894.5 and 520.4 are both common-mode), so a single bound
  catches mostly common-mode gaps while step-kind is the denser cohort just
  above 50 ms.
- **Phase 0 is done.** All three fields are present across the corpus, the
  goldens are regenerated, the PR tier is green, and the −10 ms is attributed
  to the seam crossfade with the two frames named (§ 3). § 1's bounds are
  quoted in the written frame. The one thing that outlived the phase is not a
  blocker but a question: the crossfade advance is uncompensated on every
  patch, which is now a BACKLOG row under *Fill placement*.
- **Phase 1's first done-criterion is met**: the per-kind routed-count table
  is recorded (§ 1). Still open: the feasibility counts and the scope choice,
  which wait on Phase 1b's ear probe.

**Contracts for the three blocks that caused the misreads — done 2026-09-22.**
Both defects above were *reader* defects: nothing in the dump said what
`splice_dualfit.*_seam_lag_ms` was measured against, or that `outcome.tier` is
the bracket gate in isolation. `splice`, `splice_dualfit` and `outcome` now
ship a `_contract` (C3 in `docs/dev/gap-fingerprint.md`), joining the nine
existing groups. The three strings say, respectively: `splice` is the
single-lag registration step derived from `lag_decision`, not `splice_dualfit`;
`splice_dualfit` places each shoulder at its **own** seam-local lock and its
`*_seam_lag_ms` is residual **at** that lock (~0 by construction), never a
placement; `outcome` is the bracket gate, upstream of `skip_equivalent_gaps`,
so `tier: patch` is not `equivalence_production`'s disposition.

A second round widened the coverage to **seventeen** groups. Five more were
added — `geometry` (the gap's edges, and the statement that
`a_start_secs` rounded to 1 ms is the join key while the 0-based index joins
nothing), `levels` and `b_levels` (A's and B's RMS envelopes; that
`noise_floor_db` is a **median** of context bins while `gap_floor_db` is the
**max** of the gap's bins — different reductions, not a range — and that
`b_levels` is measured at the nominal `b_mapped` span with no lag adjustment,
Tier-3 only), and `structure` and `seams` (which share the field names
`baseline_pre` / `baseline_post` one tier apart, are **absent on every
from-decode dump** under `skip_baseline_placement`, and on the projection path
fabricate `pre == post == mono == the stored min` with `per_channel` empty).
That last pair is a third latent reader trap, found by reading the write site
rather than the data.

Two existing contracts were also **corrected**, which matters more than the
additions: `donor_interior_nominal` claimed `silence_fraction ~1` meant
"program-quiet, not a dropout", and `equivalence_production` described the
donor window as the core "offset-mapped onto B" with no caveat. Both assert
readings this corpus has already refuted (see the misregistration memo — map
error to ~400 ms observed). A contract that states a refuted reading is worse
than no contract.

Caveat that matters for Phase 1: **contracts are write-time.** The dumps
already on disk do not gain them — only a fingerprint run made from this build
carries them. The census scripts must keep reading the values, never the
contract.

### Phase 1b — common-mode ear probe: NULL, with a contaminated control arm (2026-09-23)

15 gaps over 8 pairs, rendered by the 1c-cohort fingerprint run at default 3 s
`--gap-listen` context, scored blind. Verdicts by selection stratum — the
stratum is the **keep-path common-mode error** the cohort was chosen on, not
the `dual_fit_placement` common-mode the same run measured:

| stratum | n | audible | gaps |
|---|---|---|---|
| modestly-off late (74–93 ms) | 8 | **7** | 3/g7 echo, 9/g7, 11/g10, 13/g28, 23/g4 slight echo, 51/g4 maybe, 53/g8; **3/g2 clean** |
| grossly-off late (115–150 ms) | 3 | **2** | 3/g10 (both seams), 13/g30 barely; **53/g2 clean** |
| early (the only one) | 1 | 0 | 1/g14 clean |
| **controls (< 50 ms bulk)** | 3 | **2** | 23/g10, 9/g4; **13/g27 clean** |

**The probe does not answer its question, and the reason is the control arm.**
Two of three controls — drawn from the < 50 ms bulk precisely to calibrate the
listener's false-alarm rate — read audible. The audible rate is 88% modestly-off,
67% grossly-off, 67% control: **flat, and if anything inverted**. A common-mode
error that caused the audibility would have to show dose-response across a
range spanning 3× in magnitude, and there is none. The pre-registered reading
("any modestly-off late gap audible ⇒ common-mode enters scope (b)") fires on a
literal reading, but it presumed clean controls; fired on this arm it would
route a cohort on a defect the bulk has at the same rate. **Scope (b) is not
licensed by this probe.** Common-mode magnitude is refuted as the audibility
axis here, the way `peak_delta_db` was refuted as the level axis.

`dual_fit_placement`'s own readings, available for the first time on this run,
separate the verdicts no better. Over the 15, the clean four span **essentially
the full range of both axes** — common-mode |7.2| to |395.5| ms, step |0.2| to
|19.3| ms — and the largest step in the set (3/g2, 19.33 ms) is clean while the
smallest (53/g8, 0.08 ms) is audible. Neither half of the placement orders
audibility. `seams_clamped` is false on all 15, so nothing here is a clamp
artifact, and `post_seam_r` is ≥ 0.89 on every gap except 3/g10 (0.65) — the one
gap whose *pre* seam was also heard.

**What the probe did find, unanticipated: the defect is the post seam.** Of the
12 audible gaps, 11 name the post seam; exactly one (3/g10) names the pre seam,
and it names both. **Zero are pre-only.** Common-mode error displaces both
shoulders equally and cannot produce a one-sided defect, which is the
independent reason the null above is not merely underpowered — the thing being
heard has the wrong symmetry to be the thing being routed. This is consistent
with the keep path's fill being **head-anchored**: `pick_fill_length_anchor`'s
trim-head choice moves the fill's start and "extension and trim-tail keep it"
(`gap_fill_fit.rs`), and the fill is then forced to exactly `gap_frames`. One
lag serves both seams, so when B carries a step between the shoulders the
anchored end is satisfied and the residual falls at the other. Stated as a
mechanism it is a hypothesis, not a measurement; what is measured is the
asymmetry itself.

Two reports place an artifact **well after the gap end** — 51/g4 at ~4.5 s
(gap ends ~3.65 s in a 3 s-context clip, so ~0.85 s downstream) and 53/g8
"some distance after". Both are far beyond any crossfade width and beyond the
fill, which ends at the gap edge. Unexplained; worth a targeted look before
either is counted as a seam verdict, since 51/g4's "maybe" rests on it.

**Consequence for this plan.** The motivating premise of § 4b was that routing
common-mode fills buys audible quality. On this evidence it does not: the gaps
a 75–100 ms bound would route sound like the bulk it would leave alone. That
pushes scope toward (a) or (c) and makes § 6's paired listen the wrong next
spend — a paired A/B against production fills cannot separate arms on a defect
both arms would inherit from the shared head-anchored splice. The better next
question is the one the probe raised by accident: **why the post seam**, and
whether the anchoring end is a free choice.

### The post-seam asymmetry has a cause, and it is not registration (2026-09-23)

The "why the post seam" question above resolved by code inspection, confirmed by
running the function. `apply_seam_crossfade`'s post branch writes the **same**
blended block to two adjacent destinations — `[gap_end-cf, gap_end)` and
`[gap_end, gap_end+cf)` — rewinding the fill by `cf` to build the second. At the
shipped default `cf` = 10 ms that is ~20 ms of repeated audio at **every** post
seam, unconditional on registration. The pre seam has no equivalent and is
guarded by `apply_seam_crossfade_is_continuous_at_pre_seam`; no post-seam
continuity test existed.

Why it exists: a double-ended crossfade's write region spans `gap_frames + 2*cf`,
but every fill path terminates in `fit_fill_to_gap_frames`, which pads or
truncates to exactly `gap_frames`. The fill is structurally short by `2*cf`, and
the shipped code covers the shortfall by replaying, twice over.

This explains the whole Phase 1b shape at once: the post-only symmetry (11 of 12
audible gaps, zero pre-only), the user's two uses of "echo", the absence of
dose-response over a 3× magnitude range, and the 2-of-3 control failure. The
nulls above were **not** underpowered — a defect orthogonal to the axis under
test was swamping it. No conclusion about common-mode routing can be drawn from
that probe, in either direction.

**Fix, landed behind `--seam-extend-fill` (default off).** The seam margins are
taken from B either side of the fill (`seam_margin_from_b`, cut at
`b_extract.start_frame + alignment.start_frame + head_trim_frames`, since the
head-anchored trim moves the written fill's start), and
`apply_seam_crossfade_extended` writes each destination frame once. Bracket path
only — a dual-fit `SilenceSplice` bridge is synthesized, not a contiguous B
slice, so it has no margin to take and keeps the shipped path. A short or absent
margin falls back to `apply_seam_crossfade` byte-for-byte.

Two invariants are pinned by test: `plain_post_seam_duplicates_the_blended_block`
holds the current behaviour so a cleanup cannot re-land it as intended, and
`seam_extend_fill_changes_the_fill_and_nothing_outside_it` proves end to end that
the flag reaches written audio and touches only the fill.

Side effect worth noting for § 3's frame semantics: with margins the gap interior
starts at `b_fill[0]`, not `b_fill[cf]`, so the +1 crossfade displacement is gone
on the flag arm and the report's frame **is** the audio frame — exactly the
condition `apply_seam_crossfade_displaces_fill_content_by_the_crossfade_width`
anticipates. Default-off keeps goldens and `registration_applied_ms` semantics
unchanged (`golden_baseline_invariance` passes).

Phase 1b is therefore **void, not null**, and must be re-run on the fixed arm
before § 4b's premise can be judged.

### Fixed-arm render, and a second bracket-path defect (2026-09-23)

The 15-gap cohort was re-rendered with `--seam-extend-fill`, same manifest, same
3 s context. All 15 patched; write extents are **identical** between arms to
within a frame, which is the precondition for a fair A/B — the flag changes fill
content, not region bounds.

**The duplication is gone on real media.** Comparing each patched clip against
its own `_a_surround.wav`, then testing `[seam-cf, seam) == [seam, seam+cf)` with
a +-6 frame search (exact equality alone is alignment-sensitive and miscounts by
one or two either way):

| arm | duplicated post seam |
|---|---|
| baseline | 11 / 15 |
| `--seam-extend-fill` | **0 / 15** |

Four gaps (13/g28, 13/g30, 23/g10, 53/g2) show no duplication in **either** arm.
Three of those four were nonetheless scored audible in the Phase 1b probe, so the
duplication does not account for every verdict — worth remembering before reading
the re-listen as a clean sweep.

**Not a defect: anchor-seam bracket moves.** Measuring each write against its
gap, 11 of 15 write exactly `gap + cf` at each edge. Four run far past it, over
**non-silent** A:

| gap | start vs gap start | end vs gap end | overwritten raw RMS | silent blocks |
|---|---|---|---|---|
| 3/g7  | -10 ms   | **+2110 ms** | 122.8  | 0 / 106 |
| 11/g10| **-1710 ms** | +10 ms   | 763.4  | 0 / 86 |
| 9/g7  | **-1560 ms** | **+660 ms** | 1490.3 | 0 / 78 |
| 51/g4 | -10 ms   | **+1460 ms** | 141.7  | 0 / 73 |
| 1/g14 | -510 ms  | +10 ms       | -  | - |

This is the **anchor-seam rescue**, working as designed. Pair 51's `--patch-only`
report names it on gap #5: `anchor_seam_used: true`,
`anchor_bracket_move_frames: 69600` and `gap_end_adjust_frames: 69600` --
69600 / 48000 = 1450 ms, exactly the measured write minus the crossfade. The same
pair's #3 (`anchor_bracket_move 62400`) and #6 (`72000`) follow the pattern:
every large adjust carries `anchor_seam_used: true`.

The governing bound is **`max_anchor_bracket_secs`, default 5.0** ("Maximum A
bracket span when searching editorial seam anchors"), not
`gap_end_extend_max_ms`, which bounds the waveform-retry path only. All four
moves sit well inside 5 s. When the waveform seams fail, the rescue extends the A
region to an editorial anchor and fills across it, on the theory that a cut at an
editorial boundary is inaudible -- so replacing loud A is the intended trade, not
a bug. An earlier revision of this section called it a defect; that was a wrong
budget comparison.

**It still matters for the probe.** An anchor-seam gap replaces 0.66-2.11 s of A,
where a plain bracket fill replaces 0.65 s. Those are different listening objects
and a different failure surface, and all four were scored audible in Phase 1b. The
cohort should be **stratified on `anchor_seam_used`**, which the report already
tags, rather than pooled -- and with only four of them, the anchor-seam arm is too
small to read on its own. The registration question should be read from the 11
plain-bracket gaps.

### Shoulder level is a confound in the listen, and it outranks registration (2026-09-23)

The fixed-arm cohort was read two more ways before concluding anything from its flatness.

**Gain: refuted as an explanation for the anchor-seam arm.** Anchor-seam gaps are the only gaps
where gain error is directly measurable — the rescue writes B over *non-silent* A, so patched/raw
RMS isolates level with content held constant. Whole-span deltas over the four anchor-seam spans
were −0.12, +1.66, +0.27 and −0.02 dB, and they do not order the verdicts (the +0.27 dB gap is
clean; the −0.02 dB gap is audible). Consistent with the existing refutation that `peak_delta_db`
does not order audibility.

**What that measurement did find: a systematic ~70 ms registration error inside the anchor span.**
The per-200 ms level spread was large about a near-zero mean, which is the signature of a time
offset rather than a gain error. Envelope cross-correlation of patched against raw A over the four
spans:

| gap | best lag | r @ best | r @ 0 | verdict |
|---|---|---|---|---|
| 11/g10 | −68 ms | 0.923 | 0.603 | audible |
| 3/g7 | −64 ms | 0.979 | 0.533 | audible |
| 51/g4 | −76 ms | 0.934 | 0.431 | clean |
| 9/g7 | −70 ms | 0.838 | 0.348 | audible |

A null control on untouched audio from the same clips returns +0.0 ms at r = 1.000, so the lag is
real. Note the consistency: four unrelated pairs land within 12 ms of each other, which reads as a
common-mode offset rather than per-pair alignment error. **Anchor-seam gaps are therefore a direct
instrument for fill-offset registration error** — the one gap class where A holds the same
programme under the write — and they are worth using as such in Phase 1c. But note again that the
lag does not order audibility either: the cleanest of the four carries the largest lag.

**The confound.** Sorting the cohort by shoulder level (raw A, 1 s either side of the clip) orders
the verdicts better than any registration stratum has:

| | audible mean | clean mean | difference | permutation p |
|---|---|---|---|---|
| shoulder level | −40.9 dB | −34.8 dB | **+6.2 dB** | 0.105 |
| busyness (mean abs log-envelope step per 10 ms) | 1.38 | 1.37 | −0.01 | 0.514 |

By level the quiet half of the cohort is 5/7 audible and the loud half 2/8. It is *level*, not
busyness — envelope flux is dead flat, so the masking that matters here is not measurable as
activity. At n = 13 scored gaps this is suggestive, not significant, and it is a post-hoc split of
a cohort selected on registration, so it is a hypothesis for the next listen, not a result.

**Corrected by the full `--patch-only` join (all 8 pairs, join error ≤ 0.5 ms).** Three revisions to
the paragraphs above:

1. **There are five anchor-seam gaps, not four.** 1/g14 also carries `anchor_seam_used: true`
   (`anchor_bracket_move_frames` 24000 = 500 ms) and is clean. The WAV-measured stratification
   missed it because its bracket move is small. Anchor-seam splits 3/5 audible vs plain 4/8 —
   no separation (permutation p = 0.587).
2. **The ~70 ms lag is already a reported field**, `registration_residual_ms`. The envelope
   cross-correlation reproduces it to ~1 ms with the opposite sign convention (51/g4 reported
   +76.6 vs measured −76; 3/g7 +63.7 vs −64; 11/g10 +67.6 vs −68; 9/g7 +71.0 vs −70). That is a
   clean mutual validation — the field is trustworthy and the envelope method is sound — but it
   means the measurement was redundant, and future registration reads should take the field.
3. **"Common-mode" was an artifact of the four-gap sample.** Across all 15 the residual spans
   −92.2 ms (1/g14) to +131.3 ms (13/g30). The anchor-seam four merely clustered.

With the corrected strata, the three candidate factors rank:

| factor | audible | clean | difference | permutation p |
|---|---|---|---|---|
| shoulder level | −40.9 dB | −34.8 dB | +6.2 dB | **0.105** |
| \|registration residual\| | 68.5 ms | 57.4 ms | +11.1 ms | 0.308 |
| anchor-seam | 0.43 | 0.33 | +0.10 | 0.587 |

Registration remains flat on the corrected strata, and its two extremes fall on the wrong side:
23/g10 is audible at a −1.6 ms residual, 1/g14 is clean at −92.2 ms. Shoulder level and
anchor-seam are independent (mean shoulder −36.3 vs −37.6 dB), so the level effect is not the
anchor split in disguise. `dual_fit_used` is false on all 15, as expected for counterfactual
records.

**Consequence for the design.** A registration stratum drawn without controlling for shoulder level
is confounded by an effect larger than the one being measured: the strata would have to differ by
more than ~6 dB of shoulder level before a flat verdict pattern could be read as "registration does
not predict audibility". Phase 1b's flatness is therefore still uninterpretable *after* the
instrument fix — first it was void (the seam duplication), now it is confounded. Any successor
listen should either level-match the strata or score enough gaps to regress the two factors
jointly.

### Third defect: short fills are silently zero-padded into a hole at the gap tail (2026-09-23)

Chasing 23/g10 — the gap that stayed audible with a −1.6 ms registration residual — found a defect
independent of both the seam duplication and the anchor-seam rescue, and the best predictor of
audibility yet measured.

**The observation.** The patched clip carries **digital silence** (exact zeros) over 4.8934–4.9030 s
in clip coordinates. B has continuous ~−46 dB material across that span, so it is not donor
silence. The gap ends at 4.9030 — the zero run is **flush to the gap end**.

**The mechanism.** `fit_fill_to_gap_frames` (`domain/gap_fill_fit.rs:1633-1635`) zero-pads the tail
whenever the fill is shorter than `gap_frames`:

```rust
let mut out = vec![0.0f32; target_frames * channels];
out[..samples.len()].copy_from_slice(samples);
out
```

The bracket path reaches it from `application/patch_audio/region/bracket.rs:108-118`, which tops a
short fill up from `b_extension` but only by `need_samples.min(b_extension.len())`. When the
extension is exhausted the remainder falls through and becomes silence in the written output.
`extended_frames` records how much *was* extended; **nothing records the shortfall that was
zero-padded**, so this is invisible in every report and dump field.

**Prevalence and correlation with the ear.** Scanning all 15 fixed-arm patched clips for zero runs
≥ 2 ms inside the fill:

| gap | gap ms | hole ms | placement | verdict |
|---|---|---|---|---|
| 13/g28 | 644.6 | 9.4 | flush to gap end | audible (pre and post) |
| 13/g30 | 1936.7 | 7.9 | flush to gap end | audible (post) |
| 23/g10 | 1903.0 | 9.6 | flush to gap end | audible (post) |
| *(other 12)* | — | 0.0 | — | 4 audible, 6 clean, 2 unsure |

**Three holes, three audible gaps, zero holes in any clean gap** (3/7 audible vs 0/6 clean,
permutation p = 0.122 — the perfect separation cannot reach 0.05 at n = 13). All three sit flush to
the gap tail, and all three gaps were reported as **post**-seam complaints, which is where the hole
is. This outranks shoulder level (p = 0.105) as an explanation and is mechanistically specific
rather than a content correlation.

**Consequences.**
- 23/g10 is explained, and it was never a registration failure: near-perfect registration, an
  unfilled 9.6 ms hole. Its status as a failing blind control is resolved.
- Phase 1b's cohort contained a third confound. Of the 7 audible gaps, 3 are holes; re-reading the
  registration question needs them excluded or fixed first.
- The hole is **not** caused by `--seam-extend-fill` — it is present in both arms (baseline at
  4.8834–4.8930, fixed at 4.8934–4.9030; the 10 ms shift is the duplication displacement being
  removed). It is older than both.

**Next step,** smallest first: record the zero-pad shortfall as a diagnostic field so it is
visible at all, then decide whether a short fill should be held, stretched, or refused rather than
padded with silence.

### Where the padded silence comes from: nothing is mismeasured (2026-09-23)

The question was whether the hole means the gap is measured wrong, or frames are handled strangely
somewhere. Neither. Reading the chain end to end, the shortfall is a **decision taken three times
over** and then silently converted into silence by a primitive with no policy behind it.

**The chain.** `gap_frames` (the A hole, sample-exact off `refined`) is the target the fill must
reach. The fill's length is `alignment.fill_frames`, and `b_fill_raw` is exactly that many frames
sliced out of B (`characterize.rs:687-705`, re-sliced identically in `bracket.rs:197-199`) — so
`source_frames == alignment.fill_frames` by construction. There is no arithmetic slack anywhere in
that slice: both ends are `frame * channels`, no rounding, no block quantization.

So the only way the fill is short is `alignment.fill_frames < gap_frames`, and three stages in a row
are each entitled to leave it that way:

1. **The end search is *allowed* to return a short bracket.** `unified_search_best_fill_end`
   (`gap_fill_fit.rs:1358-1362`) searches ends over
   `[start + gap_frames − fill_length_slack_frames, start + gap_frames + fill_length_slack_frames]`
   — `fill_length_slack_secs` defaults to **1.0 s**. This is deliberate and correct: B's dropout
   need not be exactly as long as A's hole, so the search is free to pick the end that scores best
   rather than the end that happens to be `gap_frames` away. Nothing in the objective rewards
   reaching gap length.
2. **The snap that would stretch it back usually cannot fire.** `snap_fill_to_gap`
   (`gap_signature.rs:221`) stretches `fill_frames` to `gap_frames` when the structure post-match at
   the stretched end is within `SNAP_TOLERANCE = 0.08` of the current post — but it is
   `if let (Bool, Bool)`, so on an **Energy**-signature gap it is a silent no-op. Energy gaps have
   no length-reconciliation step at all.
3. **The extension loop is guaranteed to stop early when it runs.**
   `score_extend_short_fill_to_gap_frames` (`gap_fill_fit.rs:1819-1850`) grows the fill **one frame
   at a time** from contiguous B, accepting a frame only when
   `min_score >= best_min − SCORE_TIE_EPSILON` **and** the post-repeat correlation stays under 0.55
   and does not rise. `SCORE_TIE_EPSILON` is **1e-9** — so `score_ok` is effectively "the worse seam
   correlation must not get worse, at all" — and `best_min` is *ratcheted* upward on every accept.
   Closing a 9 ms hole means passing that test ~400 consecutive times on real audio. It does not
   happen. The first frame that trips either test `break`s, and everything after it is padding.

**Why the score cannot object.** `score_at_length` pads the trial to `gap_frames` *before* scoring
it, so every candidate is judged with its own hole already in place. The post seam reads the last
`post_window` frames — `fill_seam_search_secs` defaults to **0.25 s** — so a 9 ms hole is ~4% of the
scoring window. The objective is therefore nearly blind to the very thing the loop exists to fix: it
cannot see closing the hole as an improvement, and a single frame moves it by roughly one part in
twelve thousand. The accept/reject decision at that granularity is being made on noise.

**So the defect is the last step, not the measurement.** Stages 1–3 are all defensible in isolation.
What is not defensible is `fit_fill_to_gap_frames` turning "the search declined to go further" into
**digital silence flush to the gap end**, with no record and no acceptance test on the result. A
refusal to extend should not be spendable as silence.

**What this predicts, and what would confirm it.** The cohort should be gaps where the end search
settled short — so `zero_pad_frames > 0` should correlate with Energy-mode signatures (no snap) far
more than with Bool, and the hole sizes should sit well inside the 1.0 s slack, which the three
known cases (7.9–9.6 ms) do. Both are corpus questions the new field answers directly; the sweep is
blocked on media (below), not on analysis.

**Blocked: the corpus sweep cannot run.** Every corpus pair on the media volume currently fails to
read — `os error 415`, "the specified data could not be read from any of the copies", raised at
probe before decode. Confirmed outside the tool: the files are present and the volume reports
`HealthStatus: Healthy`, but a bare 4 KiB read of the first five pairs' A files fails on all five,
and a 64 KiB read hangs for minutes before failing. This is storage, not clip-sync. The
`zero_pad_frames` sweep and the outstanding registration delta read-out both wait on it.

### The pad is gone: extend unconditionally, and never write the zeros (2026-09-23)

Two changes, both landed, both strictly silence-removing.

**1. The Fit-mode extension no longer refuses.** `score_extend_short_fill_to_gap_frames` is now
`extend_short_fill_to_gap_frames`: it takes `min(shortfall, contiguous B)` frames outright instead of
growing one frame at a time behind the `min(pre, post)` + repeat gate. The gate could not do what it
looked like it was doing — `score_at_length` padded *every* trial out to `gap_frames` before scoring,
so each candidate on the ballot already contained the hole and the hole-free candidate was never
scored. A refusal therefore spent digital silence in preference to donor audio already in hand. Gate
mode has always extended unconditionally (the else arm of `assemble_bracket_fill`); Fit now agrees
with it. `zero_pad_frames > 0` consequently means one thing only: **B ran out** (end of extract, end
of file). The `borders`/`repeat_window_frames`/`seam_ctx` plumbing the gate needed is removed from
`fit_fill_length_for_gap` and `BracketFillAssembly`, and `placement_repeat_post` with it.

**2. When B really has run out, the pad is not written.** `execute_bracket_output` shortens the
patch destination to `refined.start + covered_frames`. The point is that the pad was never a no-op:
the gap is *below the silence threshold*, not literally zero, so writing zeros over A's gap tail cut
A's own noise floor to digital black for the padded span, flush against the post seam — which is a
far more audible event than the missing programme it stood in for. Shortening the destination writes
the same repair minus the black stripe and leaves A's tail byte-identical: an honestly unrepaired
sliver rather than a hole. `gained_fill` already cuts `b_samples` to the destination, so the padded
tail reaches neither the splice nor the fill-level measurement, and `zero_pad_frames` still carries
the shortfall onto the record. Two guards: `covered_frames == 0` is left alone (that is a geometry
defect upstream, and `splice_into_a`'s own bails should report it, not an empty destination
manufactured here), and a short write drops `seam_margin`, whose tail was cut `zero_pad_frames` past
where the fill now ends and is no longer contiguous with what is written.

`splice_into_a`'s exact-length contract and its `debug_assert` are untouched — the fill still arrives
exactly as long as the destination it is given. What changed is which destination it is given.

**Not done, deliberately:** over-building *past* `gap_frames` so `pick_fill_length_anchor` chooses the
cut. Its only gain over the above is enabling trim-**head**, which slides the whole fill later in B —
a registration change, entangled with the open question this plan is built around. Held.

**Unvalidated on media.** PR gate green (819 lib tests, fmt, both clippy gates) and two new test
pairs cover each change, but the corpus is unreadable (below), so there is no re-run and no ear
check. Both changes only ever replace silence with donor audio or with A's own material, which is why
they were taken under that constraint; the over-build variant was not.

*(other phases — fill as they land)*

## 8. Non-goals

- **Pre-gate routing** from `seam_locks` alone, which would skip the gate
  search on routed gaps. It is a perf idea, not a correctness one. Latch it
  behind this plan's adoption.
- **Jump placement at low energy** inside the bridge. BACKLOG records it as
  unmotivated: the 13/g16 jump landed inside speech and was inaudible.
- **Fixing the start-search grid aliasing** (the 50 ms coarse grid with ±1
  refine). The defect is real, but widening refine just picked another wrong
  tooth (+88 / +51, audible). Registration is the remedy; search density is
  not.
- **Attributing the step to A or B.** Routing does not need it, and
  `ffprobe` cannot decide it.
- **Single-shoulder routing** (invariant 4).
- **Making dual-fit the primary mechanism.** § 1b sizes the coverage half of
  that question and leaves it unfalsified, not supported: the accept/decline
  axis cannot say whether dual-fit's fill would *sound* as good on the gaps
  both arms would take. This plan routes a bounded cohort and keeps the
  single-lag fill everywhere else (invariant 1). Phase 1c's delta plus § 6's
  paired listen are what a later plan would need to argue the general case;
  swapping the primary mechanism on coverage numbers alone is not licensed.

## 9. Close

On close:
1. Archive this file to `archive/`.
2. Write the verdict row in `settled.md` with an `arm: production` tag.
3. Delete the Hole 1 "Next" text from BACKLOG. This is the step that gets
   skipped.
