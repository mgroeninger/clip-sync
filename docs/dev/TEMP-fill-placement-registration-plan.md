# Fill placement vs local registration — plan

Status: **investigation, Phase A first pass done (2026-08-11), B and C not started.** The defect is
characterized and a candidate discriminator is measured; what is *not* settled is whether the fix
belongs in the placement search or in a new guard, and that fork gates everything downstream. Phase A
now has real evidence (§4) and it does not point the same way on both failures. **Read §0 first** —
it names the actual deliverables; §1–§7 are the evidence behind them.

Split out of [BACKLOG.md](../../BACKLOG.md) on 2026-08-11 (the section had grown to ~270 lines).
Successor to the archived [TEMP-fill-placement-axis-plan.md](archive/TEMP-fill-placement-axis-plan.md),
which made fill placement observable; this plan asks whether the observed placements are *right*.

**Conventions used throughout.** Gap numbers are the repair table's 1-based `#`, as
`--fingerprint-gap` takes them (`resolve_fingerprint_gap_select`, `composition.rs`); corpus dumps
index the same gaps **0-based**, so 26/#7 = `26/g6`. Join the two on `geometry.a_start_secs`, never
on index — on `a_start_secs` all 227 patched gaps join to the 39-pair dumps with zero misses. Bracket
placements (`start_frame`) are B-haystack frames whose nominal origin is **672000**; see §3.1 before
comparing any two of them.

**Data.** `gap-files/2026-08-07-fill-level-shape/` (38 pairs, 227 patched gaps) for scan reports
carrying `status.patched.align_adjustment_secs`; `gap-files/2026-08-08-v0.5.2-full-corpus/`
(39 pairs) for per-gap dumps carrying `lag_decision`; `gap-files/2026-08-09-drift-listen/`
(`listen.csv` + `listen-round2.csv`, 12 patched gaps rendered and labelled 2026-08-09) for the ear
pass. All of `gap-files/` is gitignored and media-derived — quote pairs by index only, never by
title or path. **The two `listen*.csv` files are render worklists carrying A/B media paths and hold
no label column** — the ear labels live only in this document, so treat §1/§2's tables as their
system of record. `drift-listen/` also keeps the rendered `_a_surround` / `_b_surround` /
`_a_patched` WAVs per gap, which is what makes S3 answerable without a media re-run.

---

## 0. What to actually do (added 2026-08-11)

**Why this section exists.** Through §1–§7 this plan subdivides and branches without ever naming a
change to the code. Phases A/B/C are *diagnostics*, and each has been answered with another fork. The
steps below are the deliverables; the phases are inputs to them, not substitutes. If a step's
precondition fails, the step is dropped — none of them may be replaced with a further investigation.

**S1 — Record the registration residual. No threshold, no veto. Unblocked; do first.**
Emit the §2 quantity (`applied − frac_lag_ms`, reduced by minimum absolute value over shoulders with
`peak_r ≥ τ`, abstain when none) as a field on the patch record, alongside the shoulder `peak_r` /
`prominence` / `top2_spacing_ms` it was reduced from. Record-only: no gate reads it. This needs
**neither** δ (§6) **nor** the source/gate verdict (§4) — the measurement is correct to record under
either answer, and it converts "found by ear" into "visible in the output", which is the thing that
actually failed. Precedent: the fill-level fields shipped record-only for the same reason.
*Exit:* the field appears on all 227 patched gaps; the two known-bad gaps are its extremes.

**S2 — Resolve `max_fill_align_adjustment_secs` (§4.1). Blocked only on confirming it is dead.**
A documented 0.5 s bound that no code reads is a defect regardless of this investigation. Two
outcomes, both terminal: wire it, or delete it. **Do not wire it as a veto** — pair 31's genuine
−1.15 s placements at #9/#11/#12 are correct and a bare cap would refuse them, converting two bad
repairs into three unrepaired holes. Wire it as a **search prior** (penalize distant candidates so a
near-nominal candidate wins ties), for which the mechanism already exists in shape:
`fill_anchor_search_prior_weight` (`infrastructure/config.rs`, "penalize unified-search candidates far
from patch-anchor prediction", 0 = off). A prior fixes 26/g6 — a placement at −15.8 ms scored 0.378
against the applied 0.366 (§4) — and is inert where nothing near nominal scores at all, which is
exactly pair 31's correct large slides.
*Exit:* the knob is enforced or removed, and 26/g6 is re-run to confirm the prior moves it.

**S2b — Fix the `min` reduction when one shoulder is unmatchable (§4.2). 31/g7's actual defect.**
31/g7 has no placement where both shoulders score well (its post border has no match in B: dual-fit
reaches 0.124 at its own best lag and declines). Reducing by `min` then *prefers* a placement that
degrades the good shoulder from 0.829 to 0.437 because it raises the minimum. Options: fall back to
the trustworthy shoulder when the other is provably unmatchable (generalizing
`short_gap_one_strong_seam_fallback`), or make the unmatchable shoulder abstain from the reduction
rather than dominate it. Either way this is a **scoring-objective** change, distinct from S2's prior.
*Exit:* 31/g7 places at ≈ −47 ms, and the change is checked against the 227-gap corpus for placements
it moves.

**S3 — Find or refute the rival peak at −977 / −1450 ms (§4.2). No re-run needed.**
`gap-files/2026-08-09-drift-listen/` already holds `_a_surround` / `_b_surround` / `_a_patched`
renders **for both failing gaps** (and 10 more). Correlating A's pre-gap shoulder against
`_b_surround` over ±2 s and reading the whole curve answers it offline — the same method §2.1 used to
locate the fill. The widened `lag_max_lag_ms` re-run (§7) becomes a confirmation, not the experiment.
*Exit:* a rival peak at −977 / −1450 ms is found or refuted, with its height against the true-lag peak.

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
  [TEMP-fill-normalize-boost-only-plan.md](TEMP-fill-normalize-boost-only-plan.md), which edits the
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
[TEMP-fill-normalize-boost-only-plan.md](TEMP-fill-normalize-boost-only-plan.md) stops
`compute_fill_gain` from ever attenuating. **It does not block S1 / S2 / S2b / S3 / S5** — the gain is
computed at `region.rs:2056`, strictly *after* the placement search and seam reconciliation, its only
consumers are `gained_fill` and `spec.normalize_gain`, and seam scores are Pearson (scale-invariant),
so this plan's whole axis is gain-invariant. What it does change, confined to fills currently
attenuated (whole-fill RMS above A-border RMS; boosted fills untouched):

- `peak_delta_db` / `edge_delta_db` on that subset — `gap-files/2026-08-07-fill-level-shape/` becomes
  a different-policy baseline. (Nothing in §0–§7 reasons from those fields, so no conclusion here moves.)
- **§5 / S4's ear labels.** `--gap-listen` renders through the normal patch path, so `normalize_fill`
  applies to `_a_patched`: a loud-fill gap rendered after the change is different audio from one
  labelled before it. Existing `2026-08-09-drift-listen/` renders remain valid as files; a re-render
  will not match its own label.

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
| the 10 clean gaps | ≤ 0.056 s (median 0.038) |
| 26/#7 (stutter) | **1.410 s** (applied −1.450, true −0.040) |
| 31/#8 (stutter) | **0.937 s** (applied −0.977, true −0.040) |

No overlap; a 17× margin. Both failures slid **backward** where the shoulders were already
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
| correlation gain (`post − pre`) | 31/#8 flat (+0.0001) and *bad*; 31/#12 *falls* 0.574 → 0.376 and is clean; 7/#7 goes 0.993 → 0.354 and is clean. Corpus-wide 67% of patched gaps move it by < 0.001 — not a quality measure |
| `\|align_adjustment\| / gap_length` as a screen | it is what put the four clean pair-31 gaps at the top |

### 1.2 Correction: the pipeline already computes local shoulder registration

Earlier text here — "nothing in the pipeline computes the latter today" — was **wrong**, and it sent
the search toward building a new envelope correlation that already exists.
`lag_decision.pre_anchor[0]` / `post_anchor[0]` carry `frac_lag_ms` (sub-sample refined), measured
over a 1000 ms shoulder window at ±600 ms max lag, with `peak_r` / `peak_z` / `prominence` /
`second_peak_r` / `edge_pinned` attached. Emitted on every Tier-`full` gap — 227/227 of the patched
corpus, no gaps missing. Commensurability with `align_adjustment_secs` is empirical, not assumed: on
the 10 labelled-clean gaps the two agree to ≤ 33 ms, same sign convention.

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

**Corpus-wide firing rate** (227 patched gaps). Symmetric `|residual| > 150 ms` fires **30/227** —
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

**The coupling to slide magnitude is definitional, not statistical.**
`placement_slide = alignment_start_frame.abs_diff(offset_nominal_start)`
(`application/patch_region.rs:1139`, in `measure_fit_residual_verdict`) is exactly
`align_adjustment_secs` in samples. **Corrected 2026-08-11:** earlier text cited
`harness/seam_residual.rs:326` for this, which is the *fixture* harness — it computes the same
formula from `fixture.true_fill_start` and zeroes the slide unless the floor anchor matches
production, so it cannot support a claim about the dumps. The identity is no longer a two-gap
numeric coincidence either; it is derived in §3.1. With `DEFAULT_RESIDUAL_LAG_SECS = 0.010`
(`residual_gate.rs:44`) → 480 frames, and `beyond_lag_reach = slide > max_lag` (`:972`) feeding
`gate_abstains = !informative || beyond_lag_reach` (`:986`):

> **`beyond_lag_reach` ⟺ `|align_adjustment| > 10 ms`.** The residual gate's domain is fills that
> were placed within 10 ms of the nominal map.

The table below is therefore a tautology, not a finding — the non-`beyond_lag_reach` bucket tops out
at 8.0 ms, under the 10 ms line, as the definition requires. Kept only to size the population
(26/g6 and 31/g7 sit at `placement_slide_frames` 69 601 and 46 916 against `max_lag_frames` 480 —
145× and 98× outside reach):

| `residual_uninformative` | n | median \|applied\| | max \|applied\| |
|---|---|---|---|
| `beyond_lag_reach` | 159 | 56.7 ms | 1450 ms |
| every other reason | 68 | **0.0 ms** | **8.0 ms** |

`residual.informative` is true on **12 of 227** gaps (5%), and only where the fill was essentially
not moved. The five largest slides in the corpus are all `beyond_lag_reach`, `informative: false`,
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

`gap-files/2026-08-07-fill-level-shape/<pair>.json` is the **production** scan+patch report, and it
carries more than `align_adjustment_secs`: each patched gap has `status.patched.pre_correlation` /
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
patched gaps and fails on the rest — e.g. 31/g6 has `align_adjustment_secs` exactly 0.0 but
`placement_slide_frames` 1734 (36 ms). The two coincide on the failures (§3.1) but the identity in §3
is not corpus-wide; the divergent set overlaps the §5 `applied = 0` cohort and should be understood
before either quantity is used as a proxy for the other.

Also useful here: only **5** of 227 gaps have `|align_adjustment| > 0.5 s`, so S2's bound — if wired
as a veto rather than a prior — would touch five placements, of which three are believed correct.

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

**It is not a saturation issue.** The placement search radius is
`max(fill_border_search_secs, fill_align_margin_secs) = 10.0 s`; a 1.45 s slide sits at 14% of reach,
nowhere near an edge. (Contrast §1.2's `donor_registration` hypothesis, where ±600 ms against a true
−1150 ms *is* a saturation candidate — different search, different window.) The shape here is an
**argmax/aliasing failure**: the score surface has a competing peak ~1–1.45 s back that outranks the
true one, which §2.2 already established independently for 26/g6 (`prominence` 0.025, material
self-similar on a ≈ 2.9 s cycle). Correct estimator, ambiguous evidence, no tie-break. The fix shape
is therefore a prior or a uniqueness term, **not** a wider or narrower window.

### 4.1 A slide cap exists in config and is not enforced

`max_fill_align_adjustment_secs` — documented as "Maximum slide (seconds) when searching for the best
B fill position", default **0.5 s** (`infrastructure/config.rs:118,394`) — would have refused both
failures (2.9× and 2.0× over). It is plumbed config → `patch_audio/request.rs:71`, is settable from
the CLI (`cli/mod.rs:111`) and from the harness, and **no code reads it to bound any search**. The one
place that mentions it declines to use it: `domain/gap_structure.rs:294` notes that applying it to the
structure polish radius "caused multi-second exhaustive loops per fit candidate". Both failures run
`signature_mode: energy` (§3), i.e. the structure/energy path — which this knob was never wired to.

Verify this before building on it (a dead knob is a strong claim), but if it holds, Phase A's answer
is **source**, and the first candidate fix is enforcing an existing documented bound rather than
adding a new guard. Note this does not retire the §2 residual guard: a magnitude cap refuses large
slides indiscriminately, whereas the guard asks whether the slide *disagrees with local
registration* — pair 31's genuine −1.15 s gaps are exactly the population a bare cap would break.

**Original question, still the fork.** Two readings, with different fixes:

- **Source.** The search should not have made that choice — it lacked a prior favouring small slides,
  or its scores genuinely preferred the wrong spot. Then the fix belongs in the search and a guard is
  a band-aid over it.
- **Gate.** The search did its job on the evidence available, and the wrongness is only visible from
  shoulder registration the search never consults. Then a guard (or a new search term) is right.

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

**Settled by:** the widened lag sweep in §7 — if no rival appears at −977 ms, aliasing is refuted for
31/g7 and the structure-signature reading is what remains.

**Exit.** A written verdict. If *source*, phases B and C are void and this plan becomes a
placement-search change. If *gate*, continue to B. Remaining to close it: confirm the production
selection rule (so `min(seam)` is not a stand-in), confirm §4.1's dead knob, and decide whether
26/g6 and 31/g7 are one defect or two.

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

- **Widened lag sweep on 26/g6 + 31/g7 (one run, answers two questions).** `lag_max_lag_ms` is a
  config default, not a const (`gap_fingerprint/contract.rs:322`), so re-fingerprinting just these two
  gaps at ~1500 ms shows (a) whether a rival peak exists at −977 / −1450 ms and how tall it stands
  against the true-lag peak — settling §4.2 — and (b) `edge_pinned` on the `lag_decision` anchors,
  testing the ±600 ms saturation hypothesis for `donor_registration` (§1.2). The `edge_pinned` half is
  dump-only and free; the rival half needs the re-run.
- **The +39 ms systematic bias** in fill-start detection (§2.1). Cosmetic for a discriminator;
  required before any absolute claim about delivered position.
- **The `applied = 0` cohort at large** — 26 gaps corpus-wide placed at nominal while confident
  shoulders report 150–430 ms of drift. Phase B labels 16 of them; the rest remain open.
- **Two gaps break the frame-672000 identity** (§3.1) by 589 and 4245 frames. Candidates: gap
  refinement moving `refined.start_frame` away from the reported gap start, or the `.max(0.0)` clamp
  on `pad_lead` near track start. Dump-only; needed before the join is used unconditionally.
- **`max_fill_align_adjustment_secs` appears to be a dead knob** (§4.1) — plumbed and CLI-settable,
  never read. Either wire it to the placement search or delete it; a documented 0.5 s bound that
  silently does nothing is worse than neither.
- ~~**`waveform_adjustment_secs` is a dead field**~~ — **deleted 2026-08-12** (§0 S5). What it *should*
  have reported is now its own open item: **surface the winning candidate's `structure_combined` and
  `waveform_min`.** Both are locals inside `unified_fit_score` (`gap_fill_fit.rs:391-399`), which
  returns only the blended `f64`; `FillAlignment` keeps neither, so nothing downstream can say which
  term carried a placement. §4.2's min-reduction diagnosis had to be reconstructed from dumps for
  exactly this reason. Cost: a return-type change on `unified_fit_score` plus threading through the
  bracket search. Not a deletion — schedule separately.
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
