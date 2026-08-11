# Fill placement vs local registration — plan

Status: **investigation, phases not started.** The defect is characterized and a candidate
discriminator is measured; what is *not* settled is whether the fix belongs in the placement search
or in a new guard, and that fork gates everything downstream.

Split out of [BACKLOG.md](../../BACKLOG.md) on 2026-08-11 (the section had grown to ~270 lines).
Successor to the archived [TEMP-fill-placement-axis-plan.md](archive/TEMP-fill-placement-axis-plan.md),
which made fill placement observable; this plan asks whether the observed placements are *right*.

**Conventions used throughout.** Gap numbers are the repair table's 1-based `#`, as
`--fingerprint-gap` takes them (`resolve_fingerprint_gap_select`, `composition.rs`); corpus dumps
index the same gaps **0-based**, so 26/#7 = `26/g6`. Join the two on `geometry.a_start_secs`, never
on index — on `a_start_secs` all 227 patched gaps join to the 39-pair dumps with zero misses.

**Data.** `gap-files/2026-08-07-fill-level-shape/` (38 pairs, 227 patched gaps) for scan reports
carrying `status.patched.align_adjustment_secs`; `gap-files/2026-08-08-v0.5.2-full-corpus/`
(39 pairs) for per-gap dumps carrying `lag_decision`; `gap-files/2026-08-09-drift-listen/`
(`listen.csv` + `listen-round2.csv`, 12 patched gaps rendered and labelled 2026-08-09) for the ear
pass. All of `gap-files/` is gitignored and media-derived — quote pairs by index only, never by
title or path.

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
on a ≈ 2.9 s cycle; the +496 ms was measurement error, not pipeline behaviour. **Independently
confirmed from the dumps, no WAV needed:** `lag_decision` already records ambiguity as `prominence`
(= `peak_r − second_peak_r`). Reduced over *confident* shoulders only (τ ≥ 0.8 — reducing over all
shoulders is wrong, since a `peak_r` 0.07 shoulder has trivially ~0 prominence, conflating "cannot
see" with "sees ambiguously"), 26/g6 scores **0.025** against a floor of 0.282 across the other
labelled gaps — 11×, rank-consistent with the WAV margin. Its `pre_anchor` is
`peak_r 0.997 / second_peak_r 0.972`: a near-perfect match in *many* places.

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
`confidence: high`. `fit_path` is `baseline_only` on **all 227** patched gaps, so dual-fit was not
enabled in these runs and `splice_dualfit` in the dumps never drove placement. To every production
signal these look routine.

**The residual gate does not check placement — by design, not by defect.** On both failures it
abstains with `residual_uninformative: beyond_lag_reach`. Read the code before treating this as a
gap: [docs/json-output.md](../json-output.md) § residual distinguishes `beyond_lag_reach` (the
gate's **deliberate refusal**) from `no_reference_window` / `probe_non_finite` (true "could not
measure" abstentions), and `policies/seam_residual.rs:980` gives the reason — headroom is
`chosen − floor`, the floor is anchored at the **nominal** placement and measured within a lag radius
around it, so a chosen placement outside that radius has no valid reference to subtract.

**The coupling to slide magnitude is definitional, not statistical.**
`placement_slide_frames = |chosen_delta − nominal_delta|` (`harness/seam_residual.rs:326`) is exactly
`align_adjustment_secs` in samples — confirmed in the dumps: 1450 ms × 48 kHz = 69 600 vs recorded
69 601; 977.4 ms × 48 = 46 915 vs 46 916. With `DEFAULT_RESIDUAL_LAG_SECS = 0.010`
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

## 4. Phase A — source or gate? (do this first)

**Question.** A 1450 ms slide is the placement search choosing a distant match over a near one. Two
readings, with different fixes:

- **Source.** The search should not have made that choice — it lacked a prior favouring small slides,
  or its scores genuinely preferred the wrong spot. Then the fix belongs in the search and a guard is
  a band-aid over it.
- **Gate.** The search did its job on the evidence available, and the wrongness is only visible from
  shoulder registration the search never consults. Then a guard (or a new search term) is right.

**Method.** Answerable from the existing dumps, no new media run: compare what the placement search
scored at the nominal position vs at the chosen point for 26/#7 and 31/#8. If nominal scored close,
the search had the information and a small-slide prior would have fixed it (source). If nominal
scored far worse, the search was misled and only registration disambiguates (gate).

**Exit.** A written verdict. If *source*, phases B and C are void and this plan becomes a
placement-search change. If *gate*, continue to B.

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

- **`edge_pinned` on the `lag_decision` anchors** — tests the ±600 ms saturation hypothesis for
  `donor_registration` (§1.2). Cheap, dump-only.
- **The +39 ms systematic bias** in fill-start detection (§2.1). Cosmetic for a discriminator;
  required before any absolute claim about delivered position.
- **The `applied = 0` cohort at large** — 26 gaps corpus-wide placed at nominal while confident
  shoulders report 150–430 ms of drift. Phase B labels 16 of them; the rest remain open.
