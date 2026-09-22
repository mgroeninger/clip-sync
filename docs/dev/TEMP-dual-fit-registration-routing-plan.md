# TEMP — Dual-fit registration routing

**Status:** drafted 2026-09-22, not started. Production arm (`patch_audio`
characterize → dual-fit). Record-only through Phase 1; flag-gated, default
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
- **Preliminary corpus distribution (2026-09-22, provisional).**
  - Source: rebuilt from the 2026-09-18 `--patch-only` reports plus the
    2026-09-20 fingerprint dump's `lag_decision`. The method is in § 3.
  - Scope: patched single-lag gaps with both shoulders at r ≥ 0.35, with
    duplicate pairs 6 and 16 dropped. 196 gaps resolved exactly. Of the 13
    unresolved, 12 have a step ≤ 8 ms, so their max error is pinned to within
    8 ms regardless of side. 51/g4 (87 ms, common-mode) is the only one
    above 75 ms. 32/g2 is genuinely ambiguous but ≤ 50 ms. Every patched
    single-lag gap has both shoulders licensed, so invariant 4 excludes
    nothing.
  - Frame: the audio frame, in which 13/g16 reads −72.8 / −109.8.

  | | ms |
  |---|---|
  | max seam error, median | 44.4 |
  | p75 | 61.2 |
  | p90 | 74.1 |
  | p95 | 93.4 |
  | max | 894.5 (26/g6) |

  Direction: 175 fills are late of both shoulders, 11 early of both, and 10
  have errors of opposite sign. 47 gaps have a shoulder step larger than
  20 ms. Routed count by candidate bound:

  | Bound | Routed | Step-kind | Common-mode |
  |---|---|---|---|
  | > 50 ms | 79 gaps / 39 pairs | — | — |
  | > 75 ms | 20 / 15 | 5 | 15 |
  | > 100 ms | 7 / 6 | 2 | 5 |

  The two kinds within the > 75 ms set:
  - **Step:** 13/g16, 19/g2, 29/g20, 44/g11, 5/g5.
  - **Common-mode, grossly off:**
    - 26/g6 at −894 ms (heard audible);
    - 17/g34 at −520 ms;
    - 3/g10, 13/g30 and 53/g2, all 115–150 ms late.
  - **Common-mode, modestly off:**
    - 1/g14 at −88 ms (early);
    - 13/g28, 51/g4, 53/g8, 9/g7, 31/g8, 11/g10, 3/g7, 3/g2 and 23/g4, all
      74–93 ms late.

    31/g8 was heard clean on 2026-09-21. α's 5 clips were heard clean up to
    +125 ms late.

  Most of the > 75 ms tail is common-mode, not step. The late skew means a
  single bound mostly catches late fills. **Magnitude alone does not predict
  audibility:** α's late +125 was clean, and 13/g16's +88 / +51 was not. Phase
  1 therefore sizes a *candidate* bound, and Phase 3's ear pass is the test.
  Phase 1 does not settle the bound.

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
  per-seam values cannot be rebuilt from the report alone.
- **The instrument is already settled; the frame is not.**
  - The S1 shoulder read equals `lag_decision` exactly: on every trim-free
    gap, `registration_residual_ms` = `align_adjustment − lag_decision` for
    one side to within 0.1 ms.
  - The open item is a **constant 10 ms**. On every trim-free gap of the
    43-gap audio census, the report-side residual is exactly 10.0 ms below
    the audio measurement (fill measured in the `--gap-listen` WAV minus
    `lag_decision`). The dual-fit `seam_locks` lags on 13/g16 sit the same
    10 ms late of `lag_decision`.
  - Find where the 10 ms lives: a splice or crossfade convention in the
    written fill, or the template origin in one of the two reads. Then state
    the routing bound in one named frame.
  - The offset is constant, so it does not reorder gaps, but it moves a bound
    by 10 ms. Until it is pinned, bounds are quoted in the audio frame (§ 1).
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
  - The 2026-09-18 run took about 6.5 h for 53 pairs, with no oracle cost.
  - The rerun replaces the provisional § 1 table and resolves the 13 gaps.
  - It doubles as a placement check: every placement must be bit-identical
    to the 2026-09-18 run's, apart from builds that changed placement
    between the two.
- **Done when:**
  - all three fields appear in `--patch-only --format json` on the corpus;
  - the 10 ms is attributed and the bound's frame is named;
  - the goldens are regenerated and `.\scripts\test-tier.ps1 -Tier pr` is
    green.

## 4. Phase 1 — sizing and scope (corpus, `--patch-only`, no oracle)

**Preliminary pass done 2026-09-22** (§ 1 table, provisional; the Phase 0
reconstruction). Redo the counts on the Phase 0 rerun. For every gap that is
patched single-lag with both shoulders licensed, compute
`max_seam_err = max(|pre|, |post|)` and the kind (§ 0).

- Report the distribution per kind: n, median, p90 and max.
- **Pre-registered candidate bounds:** 50, 75 and 100 ms, per kind. For each,
  list the routed gaps by pair/gap index.
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
- **Scope decision (replaces the earlier stop condition).** The preliminary
  pass leaves no doubt a cohort exists: 20 gaps at > 75 ms and 7 at > 100 ms.
  The open question is which kind the rule is for. Choose one of:
  - (a) step-kind only;
  - (b) both kinds, with a separate bound per kind;
  - (c) common-mode only above a gross bound.

  Phase 1b's ear probe decides this. Record the choice and its reason in
  § 7.
- **Done when:**
  - the per-kind routed-count table from the rerun is recorded in § 7;
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

*(empty — fill per phase)*

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

## 9. Close

On close:
1. Archive this file to `archive/`.
2. Write the verdict row in `settled.md` with an `arm: production` tag.
3. Delete the Hole 1 "Next" text from BACKLOG. This is the step that gets
   skipped.
