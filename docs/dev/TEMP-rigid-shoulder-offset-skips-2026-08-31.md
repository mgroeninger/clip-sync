# TEMP — Rigid shoulder-offset skips: does map-lag explain the missed fills? (2026-08-31)

**Status:** findings note, not a Change, 2026-08-31. **Startable.** Sizes and probes the
BACKLOG class "95 skips at a rigid shoulder offset the bracket sweep never reaches."
**This note proposes no production change.** It answers the row's next action (name 38/g1's
other mechanism, then a conversion fraction) and is the place results are recorded. A
register-before-sweep Change is in scope only after Question 2 states a fraction.

**Tracked as:** [BACKLOG.md](../../BACKLOG.md) § *Fill placement* → *95 skips at a rigid
shoulder offset the bracket sweep never reaches*.

**Companions:** the caution
[settled.md](settled.md) § E *A bracket sweep score cannot see a rigid map-lag error*;
11/g9 as abut false positive
[TEMP-early-shoulder-locks-collapse-skip-plan.md](TEMP-early-shoulder-locks-collapse-skip-plan.md)
§ 4 / § 8.1; drain-not-patch
[TEMP-post-splice-introduced-silence-collapse-plan.md](TEMP-post-splice-introduced-silence-collapse-plan.md)
§ 1 (11/g9 must not drain; un-abut is that plan's step-agreement filter, not this probe).

**Media hygiene:** pair index + gap index only. `gap-files/` is gitignored; numbers below
are the durable record.

**Indices are 0-based**, matching dump filenames (`…_g001_full.json`) and the rest of `docs/`.
38/g1 is dump `g001`, not a 1-based display index.

---

## What this is

The class is: fingerprint **skip**-tier gaps whose 1 s `lag_decision` shoulders are both
live and rigid (`peak_r ≥ 0.94`, `|step| ≤ 25 ms`) **and** B holds content at the nominal
span (`donor_interior_nominal.continuous`, `silence_fraction = 0`). The bracket sweep
varies span and `move_frames` but never the donor map lag, so every bracket can score a
misregistered donor and land on `waveform_floor` looking like an absent donor. 11/g9 is
the worked case (~−70 ms common offset, `lag0_r` 0.046 / −0.018, all 23 brackets 0.05–0.27).

The candidate fix in BACKLOG is **register at the shoulder-agreed lag before the sweep**,
then a plain single-lag fill — not dual-fit; there is no step. Necessary is shown
(`settled.md`). Sufficiency is not. This note is the measurement that decides whether to
build that fix, refute the class, or split it.

## What this is not

- Not a Change. Do not wire register-before-sweep from this doc.
- Not the collapse successor. Un-abutting 11/g9 so it can reach the sweep is
  [TEMP-post-splice-introduced-silence-collapse-plan.md](TEMP-post-splice-introduced-silence-collapse-plan.md),
  not a side effect of this probe.
- Not a 1 s `peak_r` census of "would convert." The class is *defined* by 1 s `peak_r ≥ 0.94`
  at the agreed lag, so that number converting 90/90 is a tautology. Conversion is a
  **250 ms production-seam** (or bracket-seam) score at `b_mapped + mid_lag`.

## Open questions

1. **Why do 38/g1's brackets fail?** Offset ≈ 0, so map-lag cannot be the reason.
   **Answered, Result 2.**
2. **What fraction of the class would convert** if the keep path scored 250 ms seams at
   the 1 s mid-lag? State n / N. **Open — Phase 1.**
3. **Is the class one mechanism?** Split by `|mid_lag|` and by `dual_fit_rescue`. If the
   far-offset members convert and the near-zero members do not, do not ship one fix.
   **Partially answered, Result 1 / 3; conversion still open.**

## Class predicate

Over `gap-files/2026-08-29-complete-corpus/<pair>/corpus.json` (51 pairs / 1073 gaps —
**1049** `full`-tier plus 24 `summary`-tier. A `summary` gap carries no `outcome` at all, so
it is neither skip nor patch: the 644 skips are drawn from the 1049, and the skip rate must
not be read against the 1073):

```text
skip                       outcome.tier == "skip"          # 644
both 1 s shoulders         lag_decision pre_anchor[0] and post_anchor[0] present
live                       min(pre.peak_r, post.peak_r) ≥ 0.94
rigid                      |post.frac_lag_ms − pre.frac_lag_ms| ≤ 25
donor present              donor_interior_nominal.continuous
                           ∧ silence_fraction == 0.0
mid_lag                    (pre.frac_lag_ms + post.frac_lag_ms) / 2
```

BACKLOG wrote **95 / 644**. Re-derived here: **90 / 644** (Result 1). Use 90 as the ship
set.

Of the 644 skips: 90 class, **78** no both-shoulder `lag_decision` (BACKLOG 78), **447**
donor silent or discontinuous at nominal (BACKLOG 436), **29** live-donor but weak or
stepped (BACKLOG 35). The 644 denominator and the 78 leg match exactly; the other three
moved.

**Where the five went (measured 2026-08-31, not speculation).** The delta is the
**donor-presence leg alone**: the earlier pass admitted 11 more gaps as donor-present
(447 → 436), and those 11 split 5 into the class and 6 into weak-or-stepped. Two causes
named in passing are **ruled out**:

- *Not an earlier dump.* The predicate above gives byte-identical 78 / 447 / 29 / **90**
  on `2026-08-23-`, `2026-08-28-` **and** `2026-08-29-complete-corpus`. The class has
  been 90 on every corpus dump in the tree.
- *Not the `r` floor on its own.* Holding donor strict: `r ≥ 0.92` → 91, `≥ 0.90` → 93,
  `≥ 0.85` → 95, `≥ 0.5` → 97, with donor pinned at 447 and weak at 24 throughout. So
  `r ≥ 0.85` reaches 95 but contradicts BACKLOG's 436 / 35. `r` is not the sharp lever;
  **`|step|` is** — `≤ 50 ms` gives 106.

No single donor loosening reproduces 436 / 35 / 95 jointly either (`silence_fraction ≤ ε`,
`longest_silence_ms ≤ t`, `continuous`-only, `continuous ∨ sf ≤ t`, and
`donor_interior_aligned` were all swept; `continuous ∨ sf ≤ 0.35` hits donor 436 but
yields 99 / 31). The reason: of the 447 donor-rejected skips that *have* shoulders, **419
would pass the live+rigid test**, so any donor loosening feeds ~9-in-11 into the class,
never a 5/6 split. The old pass therefore differed on the donor leg *and* somewhere in
live/rigid, via an ad hoc script that is not in the repo. **Do not hunt the five** —
90 is stable and is the ship set.

88 of 90 skip `waveform_floor`; 2 skip `structure_floor` (2/g20, 36/g8).

---

## Result 1 — the class is 90, and it is not all far-offset

`|mid_lag|` is bimodal, not a single −70 ms story.

| `|mid_lag|` | n | Reading |
|---|---:|---|
| < 5 ms | 14 | Already at nominal. Map-lag registration has nothing to apply. Includes **38/g1**. |
| < 25 ms | 34 | Inside a 50 ms bin; likely the same as < 5 for a 100 ms envelope. |
| > 50 ms | 49 | Offset large enough that `lag0_r` is typically dead. Includes **11/g9** (−70 ms). |
| > 200 ms | 20 | Far. Includes pair-31/g9 at **−1145 ms** (envelope dual-fit already fills this on the patch path). |

p50 `|mid_lag|` = 57 ms. Full membership: **Appendix**.

Controls:

| gap | role | mid_ms | `lag0_r` | brackets (max seam) |
|---|---|---:|---|---|
| **11/g9** | positive — map-lag exhibit | −69.8 | 0.046 / −0.018 | 23 brackets, 0.05–0.27 |
| **38/g1** | negative — offset already 0 | −0.1 | **0.985 / 0.782** | 21 brackets, **0.130 / 0.130** |

## Result 2 — 38/g1 is named, and it is not an unrepaired hole

38/g1 fails the **bracket** seams at 0.130 while the 1 s pre lock at lag 0 reads 0.985.
(0.130 is a production keep-path number: bracket seams are `evaluate_seam_gate` at the
gate's own structure-chosen placement — see § Method 1 — not a probe-window score.)
`splice_dualfit` at the 250 ms seam **passes** (0.995 / 0.982, `gate_pass` true,
`dual_fit_rescue` true). On `gap-files/2026-08-30-tier-2-on` the gap is **patched**,
`dual_fit_used` true, align 0.0, r 0.995 / 0.982.

So:

- Map-lag is **not** why the brackets fail. There is no lag to apply.
- The keep-path sweep and the dual-fit seam are different instruments — same 250 ms
  scorer, different *placements* (sweep span/`move_frames` vs `seam_local_peak`). That
  is the other mechanism.
- 38/g1 does **not** show the class stays skipped in production. Dual-fit already
  fills it. It shows only that **register-before-sweep cannot be why this gap was
  a fingerprint skip**, and cannot be the fix for near-zero members.

## Result 3 — most of the class is already a dual-fit rescue on paper

Fingerprint `dual_fit_rescue` (the "would `try_dual_fit` admit this skip" flag, not
the bracket outcome):

| `dual_fit_rescue` | n |
|---|---:|
| true | **83** |
| false | **7** |

Default production runs dual-fit. The 83 are predicted fills *without* touching the
bracket sweep. 31/g9 is in this 83 and is the gap the shoulder-locks Change newly
filled on Tier 2.

The seven that dual-fit does **not** predict:

| gap | mid_ms | 250 ms dual `r` | `gate_pass` | Why it is still open |
|---|---:|---|---|---|
| **11/g9** | −69.8 | 0.989 / **0.500** | true | `bridge ≤ 0` → now `shoulders_abut`; never reaches `try_dual_fit` |
| 1/g1 | 139.3 | 1.000 / 0.985 | true | rescue false despite pass — other `try_dual_fit` conjunct |
| 1/g15 | 158.8 | 0.999 / 0.983 | true | same |
| 20/g1 | −286.6 | 1.000 / 0.953 | true | same |
| 22/g5 | −249.8 | 0.998 / 0.982 | true | same |
| 30/g43 | 10.0 | 0.908 / 0.968 | true | near-zero mid; same |
| **22/g13** | −247.9 | 0.996 / **0.271** | **false** | post seam dead even with envelope |

11/g9's 0.500 post is the false peak one gap-length away, not the −70 ms 1 s lock.
A keep-path shift to −70 ms is the candidate that could offer a *single-lag* fill
here — and it cannot run while the gap is `shoulders_abut`.

---

## Phase 1 probe (still to run)

**Question 2.** Score the seams at `b_mapped + mid_lag`, one constant shift, no search,
no dual-fit, no envelope. Convert ⇔ both seams clear `min_fill_correlation` and
`fill_absolute_floor` (the same `gate_pass` numbers).

Do **not** use 1 s `peak_r`. Do **not** treat `splice_dualfit` at its own
(possibly rescued) lag as this probe — that is a different placement (11/g9).

### Method (normative)

Record-only call to
[`fill_seam_correlations`](../../crates/clip-sync-repair/src/domain/policies/seam_scoring.rs)
(`seam_scoring.rs:741`) — the keep path's own scorer, parameterized by
`SeamPlacement { start, .. }`, so the probe is an argument change and no new math.
Write the Pearson pair plus the gate verdict. **Do not search. Do not change skip or
patch.** Re-dump only the minimum set (below) with `measure-gap-fingerprints.ps1
--fingerprint-gap`; the flag is a corpus *filter*, so the per-bracket oracle is paid
only for the named gaps.

Use `fill_seam_correlations`, **not**
`fill_splice_seam_correlations_interleaved` — the latter scores the wrong adjacent B
window and is what produced the A7 false-skips.

Four constraints, each of which the obvious wiring gets wrong:

1. **Two scores per gap, both on production windows: `mid_lag` and 0.** The dump's
   `brackets[].seam_pre` / `seam_post` are **not** probe-window numbers.
   `measure/overlay.rs:200–291` builds `SeamGateParams` from `gate_settings` +
   `derive_seam_gate_geometry` and calls `oracle_score_fit_candidate` → production
   `evaluate_seam_gate` → `fill_seam_correlations`: `fill_seam_search_secs` templates
   (250 ms — `seam_gate_frames_for` caps `correlate_frames`, whose own floor is 0.25 s,
   so the cap binds in practice) at production `border_standoff_secs` **0.35**, evaluated
   at the gate's structure-chosen `alignment.start_frame`, not at `b_mapped`. The 150 ms
   (`bin_frames * 3`) / `PROBE_BORDER_STANDOFF_FRAMES = 0` templates belong to `place_on_b`
   alone and score nothing in that table. So 38/g1's **0.130 is already the production
   keep-path Pearson** — it is *why* the gap skipped at `waveform_floor`. A 150 ms
   at-`b_mapped` score would not reproduce it, and failing to reproduce it would prove
   nothing about the wiring. Emit production windows at `mid_lag` (Q2's answer) and at
   `0` (the at-rest control); nothing else.

   **Wiring self-check — the recorded-`start_frame` version does not run on this class.**
   `start_frame: placement.map(|a| a.start_frame)` is `Some` only on the `Ok` arm; a
   *failing* bracket keeps its `seam_pre`/`seam_post` (from `stage_of` at `WaveformFloor` /
   `Residual`) but loses the placement that produced them. Measured over every per-pair
   `corpus.json`: **10,098 skip-gap brackets carry no `start_frame`** (zero do); only 3,212
   of 7,538 patch-gap brackets do. So "replay at the dump bracket's recorded `start_frame`
   and match its `seam_pre`/`seam_post`" is unrunnable on the 90.

   It does not need to be. The provenance is exact and traced end to end:
   `evaluate_seam_gate_fit_candidate` returns `waveform_below_threshold(alignment
   .pre_correlation, alignment.post_correlation, …)` (`patch_region.rs:1670`, `:1695`,
   `:1704`), and those two numbers are
   `waveform_seams_at_start(input.waveform, best_start, &score_channels)`
   (`gap_fill_fit.rs:922`, `:1018`) = `fill_seam_correlations_with_channels(templates,
   SeamPlacement { start: alignment.start_frame, gap_frames, pre_window:
   waveform_gate_frames, post_window: post_gate_frames }, seam_score_channels(templates))`.
   `snap_fill_to_gap` never touches `start_frame` or either correlation, so a re-measure at
   that start is **bit-identical by construction** (the comment at `gap_fill_fit.rs:951–962`
   says so and an earlier revision paid for the redundant call). `fill_seam_correlations`
   computes the same `score_channels` internally, so the plain entry point is equivalent.

   Three checks, in order of what they buy:

   - **S1 — in-run identity (use this; no schema change, no recorded start).** In the probe
     process, at the same failing bracket, call `fill_seam_correlations` at
     `unified.alignment.start_frame` with the gate's own templates and
     `waveform_gate_frames` / `post_gate_frames`. It must equal the dump's `seam_pre` /
     `seam_post` **exactly**, on the class's own gaps. That is the wiring proof; Q2 is then
     the identical call with only `start` changed to `b_mapped + mid_lag`.
   - **S2 — dump-side cross-check.** Replay on a *patched* gap's bracket that does carry
     `start_frame` (3,212 available). Same scorer, same templates, must match. Cheap, and
     it checks the dump-reading half that S1 does not.
   - **S3 — zero-code fallback if the probe is not wired inside the gate.** The DEBUG
     diagnostic at `patch_region.rs:1549–1572` already logs `start_frame`, `seam_pre`,
     `seam_post` at the structure-chosen placement for every gate bracket. With
     `--fingerprint-gap` narrowing the corpus to ~20 gaps, `RUST_LOG=debug` yields the start
     the dump omits.

   **Durable fix (separate, optional).** If the class should be self-checkable from dumps
   alone, thread the placement through the failure: add `start_frame` to
   `SeamGateFailure::WaveformBelowThreshold` / `ResidualHeadroomExceeded`, carry it in
   `BracketStageDetail`, and record it on the `Err` arm in `overlay.rs`. Watch the two
   re-construction sites that today rebuild the failure from its fields
   (`patch_region.rs:427–431`, `:648–651`, and the extend loops at `:1968` / `:2031`) — they
   must carry the start too or the field is silently lost. Record-only, no behaviour change;
   it is the generalized failing-bracket instrumentation this line of work has wanted twice.
   Not a Phase 1 blocker: S1 does not need it.
2. **Record `score_pre` / `score_post` and the resolved `start`.** An unscoreable side
   returns **`0.0`, not NaN** (`seam_scoring.rs:784–804`: the bounds guards fall through
   to `0.0`). A shifted start makes that reachable — 31/g9 at −1145 ms, and anything near
   the B-haystack edge. Without the two booleans, "never scored" is indistinguishable
   from "dead seam" and the conversion fraction silently collects false negatives.
   Note the two paths disagree on the sentinel: the search calls
   `waveform_seams_at_start`, which returns **`−∞`** when `placement_in_bounds` fails
   (`gap_fill_fit.rs:1023–1031`), while a direct `fill_seam_correlations` returns `0.0` for
   the same placement. Call `placement_in_bounds` explicitly and record it as its own
   column rather than inferring bounds from either sentinel.
3. **Read the thresholds from the recipe, never as literals.** `min_fill_correlation`
   and `fill_absolute_floor` are already in the dump
   (`source.gate_recipe`; 0.35 / 0.12 today) and are recipe knobs — hardcoding 0.35 is
   a mistake this line of work has already paid for once. Compare with production's
   `smin < floor` form so a NaN does not fail the gate (the polarity `dual_fit_rescue`
   mirrors).
4. **The call sits on the fingerprint measure path, not the production keep path.**
   11/g9 is `shoulders_abut`, and that refusal happens in characterize *before* the keep
   path — a probe at the keep-path start emits no row for the class's flagship gap. The
   fingerprint path never runs the production patch gate, so it still scores.

### Minimum set

No need to media-run all 90 first (they span ~30 pairs; each pair is its own run):

1. The **seven** `dual_fit_rescue == false` (the production remainder).
2. **38/g1** as negative control — `mid_lag` is already ~0, so the two scores should
   coincide and both should land near its recorded bracket seam. (This is a *consistency*
   check, not the wiring check; the wiring check is § Method 1 **S1**, which runs on every
   probed gap.)
3. **Three per `|mid_lag|` stratum**, drawn before the run from the census strata
   (`|mid| < 5 ms` / 5–50 / 50–200 / > 200 → 14 / 27 / 29 / 20, as `census-skip-buckets.ps1` prints them), `dual_fit_rescue == true`,
   preferring members with dead `lag0_r` — asks whether the keep path would have filled
   them *without* dual-fit, which is the BACKLOG candidate's claim.

**Scope — fixed here, not after the run.** The sample is the seven + 38/g1 + 3 per
stratum ≈ 20 gaps. Result 4 states `n / N` **over that sample, by stratum**, and says so;
it does not state a fraction over the 90. Extrapolation to the 90 is allowed only if
every stratum is represented (≥3) and the strata agree; otherwise Result 4 reports the
strata separately and the class-wide fraction stays open. § *Exit* is worded to match.

Write the fraction here as Result 4. Licensed media required for the PCM score;
the dump cannot answer it.

**Limit on Result 4 — a null is not a full refute.** The probe scores one constant
shift; it does **not** re-run the bracket sweep. If the shifted seams fail, that refutes
"a constant shift alone converts them", not "register, then sweep" — structure alignment
could still move `start` after the shift and land somewhere the probe never scored.
Record that limit beside the number; only the pass direction is decisive.

If the seven-minus-11/g9 still fail at `mid_lag`, the candidate fix is the wrong
shape for the unrepaired remainder. If they convert, a **separate** Change plan
to wire register-before-sweep is in scope — and 11/g9 still needs the collapse
plan's step-agreement un-abut before that Change can see it.

## 11/g9 sequencing

| Path | What happens to 11/g9 |
|---|---|
| Today (shoulder-locks shipped) | `shoulders_abut` skip, record-only. No fill. |
| This probe | Can score seams at −70 ms offline / in a one-off. Must not change production. |
| Collapse successor | Must not drain. Step-agreement admission filter would drop it from the abut list, after which it is an ordinary skip again. |
| Register-before-sweep Change | Only then can the keep path offer a single-lag fill. |

Do not bundle those three.

## Non-goals

- Wiring the sweep.
- Dual-fit policy changes (envelope already relocates far dead locks on that path).
- Treating `dual_fit_rescue == true` as "the gap is unrepaired."
- Ear checks until Question 2 names a convert set.

## Data sources

- **`gap-files/2026-08-29-complete-corpus/`** — 51 pairs, 1073 gaps (1049 `full`-tier).
  One `corpus.json` **per pair directory**, with per-gap `…_gNNN_full.json` /
  `…_summary.json` siblings repeating the same gaps. Class membership, 38/g1,
  `dual_fit_rescue`.
- **`gap-files/2026-08-23-` and `-08-28-complete-corpus/`** — same-predicate controls for
  the 95-vs-90 delta; both give 90.
- **`gap-files/2026-08-30-tier-2-on/38.json`** — 38/g1 production patched via dual-fit.

## Reproduction

`scripts/measure/census-skip-buckets.ps1 -DumpDir <corpus> -CsvOut <path>` — dump-only,
no media, one row per skipped gap with every predicate leg's raw input beside its
`bucket`. Prefers each pair's `corpus.json`; pair index = the containing directory name.
Per gap: `lag_decision.pre_anchor[0]` / `post_anchor[0]`,
`donor_interior_nominal`, `outcome.tier`. Predicate in § *Class predicate*. 38/g1
bracket maxima from `brackets[].seam_pre` / `seam_post`. Production join on
`a_start_secs` rounded to 1 ms within a pair label.

## Exit

- Q1 named (this note, Result 2) → keep.
- Q2 states n / N on the production-windows-at-`mid_lag` instrument, **over the ~20-gap
  sample of § Minimum set, by stratum** (not over the 90) → convert ⇒ promote a Change
  plan (one deliverable: register-before-sweep on the keep path); null ⇒ file a
  `settled.md` entry that refutes *the constant-shift* form only, per § *Limit on
  Result 4*, and rewrite (not delete) the BACKLOG row unless the sweep question is also
  closed.
- This note archives when every question is promoted or closed.

---

## Appendix — the 90

`mid_ms` = mean of the two 1 s `frac_lag_ms`. `lag0_r` is correlation at the
unshifted map (the number every bracket currently resembles).

| pair/gap | mid_ms | step_ms | 1 s r | lag0_r | skip | dual_fit_rescue |
|---|---:|---:|---|---|---|---|
| 1/g1 | 139.3 | 0.0 | 1.000/0.999 | -0.137/-0.000 | waveform_floor | False |
| 1/g2 | 136.5 | 1.7 | 1.000/0.998 | 0.053/0.010 | waveform_floor | True |
| 1/g4 | 147.9 | -1.5 | 0.998/0.967 | 0.026/-0.006 | waveform_floor | True |
| 1/g5 | 179.8 | -1.9 | 0.995/1.000 | -0.035/-0.011 | waveform_floor | True |
| 1/g13 | 164.9 | 7.8 | 1.000/0.995 | -0.049/-0.002 | waveform_floor | True |
| 1/g15 | 158.8 | -0.1 | 0.999/0.994 | -0.058/0.065 | waveform_floor | False |
| 2/g18 | 127.0 | -10.2 | 0.989/0.993 | -0.067/0.005 | waveform_floor | True |
| 2/g20 | 128.5 | 6.5 | 0.983/0.997 | 0.016/0.146 | structure_floor | True |
| 3/g6 | -350.7 | 0.2 | 0.993/0.990 | 0.002/-0.027 | waveform_floor | True |
| 3/g9 | -353.6 | -1.6 | 0.983/0.974 | -0.007/-0.026 | waveform_floor | True |
| 3/g14 | -345.1 | 5.8 | 0.984/0.966 | 0.001/0.006 | waveform_floor | True |
| 7/g4 | 13.2 | -3.9 | 0.972/0.992 | -0.005/-0.029 | waveform_floor | True |
| 7/g6 | 2.6 | 5.1 | 0.965/0.957 | 0.846/-0.403 | waveform_floor | True |
| 8/g3 | 76.5 | -6.2 | 0.989/0.959 | -0.001/-0.004 | waveform_floor | True |
| 8/g7 | 36.4 | 0.3 | 0.993/0.990 | 0.060/0.128 | waveform_floor | True |
| 8/g10 | 45.9 | -5.9 | 0.988/0.974 | 0.047/-0.019 | waveform_floor | True |
| 9/g6 | -42.9 | 6.0 | 0.995/0.991 | 0.008/-0.302 | waveform_floor | True |
| 10/g1 | -4.5 | 15.2 | 0.994/0.984 | 0.088/-0.303 | waveform_floor | True |
| 10/g2 | 6.6 | 7.8 | 1.000/0.996 | -0.285/-0.149 | waveform_floor | True |
| 10/g3 | -22.7 | 13.3 | 0.999/0.996 | 0.042/0.122 | waveform_floor | True |
| 10/g5 | -0.6 | 2.5 | 0.993/0.993 | -0.083/-0.324 | waveform_floor | True |
| 11/g4 | -65.7 | -3.9 | 0.988/0.992 | 0.003/0.049 | waveform_floor | True |
| 11/g5 | -54.5 | 7.8 | 0.988/0.987 | 0.086/0.047 | waveform_floor | True |
| 11/g6 | -54.9 | -7.8 | 0.989/0.987 | -0.050/-0.024 | waveform_floor | True |
| 11/g7 | -56.9 | 5.8 | 0.981/0.991 | -0.012/-0.059 | waveform_floor | True |
| 11/g9 | -69.8 | 4.2 | 0.999/0.988 | 0.046/-0.018 | waveform_floor | False |
| 11/g11 | -40.0 | 1.7 | 0.987/0.972 | -0.132/0.000 | waveform_floor | True |
| 11/g13 | -63.6 | 22.8 | 0.993/0.984 | 0.035/0.027 | waveform_floor | True |
| 12/g1 | 141.0 | 2.1 | 0.990/0.992 | 0.121/-0.032 | waveform_floor | True |
| 12/g4 | 92.3 | 17.9 | 0.997/0.974 | 0.002/0.014 | waveform_floor | True |
| 12/g5 | 133.2 | 4.0 | 0.993/0.971 | -0.009/0.031 | waveform_floor | True |
| 13/g18 | 267.7 | -5.1 | 0.996/0.993 | 0.012/0.001 | waveform_floor | True |
| 17/g2 | -12.6 | 2.1 | 0.999/0.998 | -0.074/0.217 | waveform_floor | True |
| 17/g6 | -12.5 | 2.2 | 0.983/0.991 | -0.062/0.345 | waveform_floor | True |
| 17/g7 | 19.5 | -4.0 | 0.995/0.981 | 0.007/-0.060 | waveform_floor | True |
| 18/g26 | 316.3 | -4.0 | 0.991/0.955 | -0.122/0.032 | waveform_floor | True |
| 18/g27 | 364.3 | 3.9 | 1.000/0.999 | 0.011/0.002 | waveform_floor | True |
| 18/g33 | 334.4 | 0.2 | 0.989/0.991 | -0.010/-0.002 | waveform_floor | True |
| 18/g40 | 336.8 | 3.9 | 0.987/0.982 | 0.026/0.011 | waveform_floor | True |
| 19/g11 | 70.6 | 17.2 | 0.985/0.977 | -0.040/-0.011 | waveform_floor | True |
| 20/g1 | -286.6 | 0.2 | 1.000/0.988 | 0.042/0.006 | waveform_floor | False |
| 20/g2 | -291.5 | 2.3 | 1.000/1.000 | 0.022/0.001 | waveform_floor | True |
| 20/g6 | -293.5 | -5.8 | 0.999/0.998 | 0.001/-0.014 | waveform_floor | True |
| 22/g5 | -249.8 | -0.2 | 0.944/0.996 | 0.002/0.035 | waveform_floor | False |
| 22/g7 | -218.6 | -6.0 | 0.992/0.965 | -0.016/0.001 | waveform_floor | True |
| 22/g11 | -247.9 | -7.9 | 1.000/0.998 | -0.074/0.016 | waveform_floor | True |
| 22/g13 | -247.9 | -7.9 | 0.995/0.991 | 0.020/0.008 | waveform_floor | False |
| 23/g8 | -2.9 | 1.9 | 0.996/0.979 | -0.113/0.072 | waveform_floor | True |
| 23/g11 | -0.3 | -8.0 | 0.996/0.975 | 0.471/0.332 | waveform_floor | True |
| 23/g15 | 3.3 | -8.1 | 0.961/0.962 | -0.008/0.048 | waveform_floor | True |
| 24/g5 | -400.9 | 3.9 | 0.994/0.991 | -0.005/-0.002 | waveform_floor | True |
| 24/g31 | -395.4 | 2.2 | 0.985/0.995 | -0.004/0.022 | waveform_floor | True |
| 24/g46 | -385.7 | 0.8 | 0.998/0.977 | 0.001/-0.015 | waveform_floor | True |
| 25/g4 | 404.4 | 1.9 | 0.988/0.964 | 0.037/0.036 | waveform_floor | True |
| 27/g3 | 50.5 | -2.0 | 0.990/0.996 | 0.066/-0.015 | waveform_floor | True |
| 30/g2 | 8.2 | 18.1 | 0.988/0.973 | -0.253/0.014 | waveform_floor | True |
| 30/g6 | -4.5 | 23.8 | 0.998/0.989 | -0.348/-0.162 | waveform_floor | True |
| 30/g42 | -9.8 | 19.4 | 0.997/0.994 | 0.078/0.667 | waveform_floor | True |
| 30/g43 | 10.0 | -0.0 | 0.971/0.971 | 0.013/-0.039 | waveform_floor | False |
| 31/g9 | -1144.7 | -4.1 | 0.965/0.957 | — | waveform_floor | True |
| 32/g10 | 160.8 | 0.1 | 0.982/0.952 | 0.166/-0.075 | waveform_floor | True |
| 33/g22 | 9.3 | -1.9 | 0.991/0.994 | 0.077/-0.072 | waveform_floor | True |
| 34/g15 | 2.4 | -3.8 | 0.992/0.989 | -0.169/0.302 | waveform_floor | True |
| 35/g5 | 52.1 | 5.6 | 0.989/0.992 | 0.028/0.125 | waveform_floor | True |
| 35/g7 | 61.3 | 3.9 | 0.995/0.998 | 0.020/-0.195 | waveform_floor | True |
| 36/g4 | 13.6 | 8.7 | 0.997/0.992 | -0.207/0.004 | waveform_floor | True |
| 36/g5 | 24.6 | -5.9 | 0.969/0.961 | 0.015/0.011 | waveform_floor | True |
| 36/g8 | 1.4 | 6.1 | 0.976/0.988 | -0.020/0.229 | structure_floor | True |
| 38/g1 | -0.1 | -0.2 | 0.998/0.982 | 0.985/0.782 | waveform_floor | True |
| 39/g1 | 9.7 | 2.1 | 0.995/0.984 | -0.304/-0.026 | waveform_floor | True |
| 39/g3 | 5.1 | -3.5 | 0.965/0.990 | -0.038/-0.158 | waveform_floor | True |
| 39/g5 | 4.0 | -2.0 | 0.971/0.962 | -0.248/-0.238 | waveform_floor | True |
| 40/g2 | 13.4 | -3.9 | 0.989/0.990 | -0.059/-0.202 | waveform_floor | True |
| 40/g3 | 12.2 | 1.9 | 0.996/0.991 | 0.024/-0.110 | waveform_floor | True |
| 40/g6 | 15.2 | 4.0 | 0.985/0.988 | 0.256/-0.016 | waveform_floor | True |
| 43/g3 | -9.0 | -7.1 | 0.993/0.994 | -0.019/-0.311 | waveform_floor | True |
| 44/g7 | -7.1 | 6.0 | 0.999/0.997 | -0.139/-0.178 | waveform_floor | True |
| 44/g9 | 3.1 | -2.1 | 0.987/0.987 | 0.036/-0.394 | waveform_floor | True |
| 45/g3 | -4.4 | -5.8 | 0.996/0.999 | -0.106/0.177 | waveform_floor | True |
| 46/g2 | 90.3 | 3.9 | 0.981/0.986 | 0.001/-0.018 | waveform_floor | True |
| 46/g6 | 92.2 | -3.9 | 0.990/0.988 | 0.036/-0.009 | waveform_floor | True |
| 46/g7 | 93.2 | 5.7 | 0.993/0.973 | -0.010/0.002 | waveform_floor | True |
| 47/g9 | 172.1 | 3.8 | 0.997/0.964 | 0.046/-0.046 | waveform_floor | True |
| 47/g11 | 177.7 | -2.1 | 0.978/0.982 | 0.020/-0.064 | waveform_floor | True |
| 48/g1 | -36.4 | 0.2 | 0.978/0.977 | 0.183/0.072 | waveform_floor | True |
| 48/g2 | -37.6 | -2.0 | 0.970/0.976 | -0.024/-0.026 | waveform_floor | True |
| 48/g3 | -39.5 | 0.2 | 0.974/0.956 | 0.075/0.025 | waveform_floor | True |
| 48/g12 | -3.2 | 3.8 | 0.967/0.976 | 0.086/0.000 | waveform_floor | True |
| 48/g14 | -9.4 | 0.3 | 0.989/0.991 | 0.046/-0.004 | waveform_floor | True |
| 51/g1 | -183.3 | -5.8 | 0.997/0.994 | -0.020/-0.014 | waveform_floor | True |
