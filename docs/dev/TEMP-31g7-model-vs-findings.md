# TEMP — The placement model, and 31/g7's findings set against it

**Status:** findings note, **rev 2, 2026-08-18** — a full rewrite of the 2026-08-17 rev 1 (in git
history at this path) after two secondary reviews. Rev 1 carried a repeat/aliasing story for 31/g7
that archived measurement had already refuted; this revision removes every claim that failed
verification — the **Errata** at the end lists each one and why. Written after the S2b floor probe
(`gap-files/2026-08-17-s2b-floor-probe/`, arms A/B/C complete) and the g007 fingerprint
cross-check. Descriptive: it states the model and where the measurements land on it. **It proposes
no change** — S2b was refuted the same day and archived; live fix work is
[TEMP-lag-decision-map-coordinates-plan.md](TEMP-lag-decision-map-coordinates-plan.md),
[TEMP-lag-envelope-recenter-plan.md](TEMP-lag-envelope-recenter-plan.md), and BACKLOG S1.
One-sided closeability as a *gap cell* (31/g7 vs 26/g6) is parked in
[TEMP-31g7-gap-shape-vocabulary.md](archive/TEMP-31g7-gap-shape-vocabulary.md), not here.

> **Post-rev-2 measurement, 2026-08-18 (evening):** the post-shoulder ±2000 ms wide sweep found
> 31/g7's post shoulder **uniquely at −1160 ms** — one-sided closeability is refuted (the gap is
> the degenerate silence-splice: both shoulders live, donor bridge ≈ 0; 31/g7 *is* pair 31's map
> step) and **S2b is refuted with it**. This note's 26/g6 findings and the model/locks/populations
> stand; every 31/g7 "post is unmatchable" reading below silently means "within ±600 ms reach".
> Resolution: [TEMP-31g7-gap-shape-vocabulary.md](archive/TEMP-31g7-gap-shape-vocabulary.md) §9.
>
> **Same evening, 13-gap census:** the §8 lag-verdict one-dead row is a **dump predicate**, not a
> media class. A throat-adjacent non-chained ±2000 ms sweep on the 13 (plus 31/g10 as control)
> sorted **12** in-reach false-deads (walk-off template / uncorrected `frac_lag` chain), **1**
> collapsed-bridge (31/g7), **1** shared-far offset (31/g10; not in the 13 — `peak_z` 8.6 < 12).
> Hole 2 is still empty. Change:
> [TEMP-lag-decision-map-coordinates-plan.md](TEMP-lag-decision-map-coordinates-plan.md).
> Recenter still owns the far offsets. Do not read "type" in §8 as a cell or an S2b population.

Pair 31 / gap 8 (`#` 1-based; dump `g7`; `a_start_secs` 1725.6968125, A gap 1.1188 s, 48 kHz).

---

## 0. The headline

**The seam Pearson and registration are independent quantities, and the placement objective only
ever reads the first.** A seam can score well because the content genuinely continues — or because
the two windows being joined merely resemble each other (both quiet, both broadband). The gate
cannot tell those apart, because it never looks anywhere except at the joint it is proposing.
Registration — *"did this fill come from the right moment of the programme?"* — is a question the
placement objective does not ask. Two demonstrated facts carry this note:

1. **Independence.** Pair 50 scores `waveform_min` 0.52 — a comfortable `High` — at a placement
   where the shoulder-lag sensor finds no correlation peak at all (§6). Whatever the seam Pearson
   measures over its window, it is not "this is the right content".
2. **The unasked question, counted.** Comparing each applied placement against where A's own
   shoulder audio is independently found in B: **2 of the 68 measurable single-fill acceptances
   contradict a confident, in-reach landmark** (pair 26 by 1.39 s, pair 31/g7 by 0.93 s), under
   the confidence filter stated in §6.7. That count is a property of *this join under that
   filter* — a post-hoc patch check, not a corpus constant and not a gap class: the two failures
   share no fingerprint signature (§8).

A consequence worth stating up front: **no threshold on `waveform_min` can be the fix.** It is the
number that accepted the bad placements; ordering by it finds 54 of 68 gaps to catch the 2 that
are wrong (§§5–6).

Epistemic ledger — every claim below sits on one of three tiers:

| tier | contents | where |
|---|---|---|
| **code-read** | ladder, objective, penalties, the three locks | §§1, 3 (F1), 4 |
| **measured** (this dump vintage; join- and filter-scoped where flagged) | sensors, probe arms, oracle, populations, the join | §§2, 3, 5, 6, 8 |
| **inference / open** | pair-50 map-correction reading, pair-26 mechanism, 31/g7's scoring mechanism | flagged inline |

Rev 1 had a fourth, unlabelled tier — a periodicity story assembled from three weak sensors —
that the archived registration plan had already refuted by direct measurement. It is gone; §9
turns that failure into an admission criterion, and the Errata lists the pieces.

---

## 1. The model, as built

Execution order is [pipeline.md](../pipeline.md) §§1–5. Placement is decided entirely inside §4
(`prepare_region_patch`), and the part that matters here has **two nested axes of freedom**:

```text
 §1 align ──────────► global offset  (recommended_offset_secs)
 §2 scan  ──────────► gaps: A silent, B has energy
 §3 plan  ──────────► one FillRegion per selected gap (per-gap; no neighbour coupling)
 §4 per-gap patch
      offset map ──► nominal b_mapped span
      A edge refine
      B haystack   (sized by fill_border_search_secs)
      seam gate (fit) ─┐
                       │  AXIS 1 — WHICH A-SIDE BRACKET to fill
                       │    E1 baseline Pearson High             → return
                       │    E2 baseline_only High/Marginal       → return
                       │    E3 best High joint, anchor only      → may return
                       │    E4 best anchor joint candidate       → may return
                       │    E5 baseline_only pooled best         → ranking
                       │    E6 boundary grid best High (--full)  → return
                       │    E7 pooled best                       → ranking
                       │
                       │  AXIS 2 — WHERE IN B each bracket starts
                       │    unified search over ±fill_border_search_secs
                       │    argmax of 0.35·structure + 0.65·waveform − penalties
                       └─ extract fill → normalize → queue
 §5 write
```

**Axis 2** (start slide) is a *scored search*. Its objective reduces the seam pair with
`min(pre, post)` at `gap_fill_fit.rs:634`.

**Axis 1** (bracket) is **not** primarily a scored search — it is a **precedence ladder of early
returns**. Only the E5/E7 arms rank, and they rank on
`fit_candidate_ranking_score(min(pre,post), boundary_move)` =
`min_waveform − 0.0002 × boundary_move_frames` (`gap_fill_fit.rs:249`), where `boundary_move` is
`|Δstart| + |Δend|` in frames (`patch_region.rs:1708`). Anchor candidates additionally pay a
center-drift term, `ANCHOR_CENTER_DRIFT_PENALTY_PER_FRAME` = 0.00015 per frame of drift
(`gap_fill_fit.rs:236`, applied in `anchor_bracket_ranking_penalty` at `:264`) — rev 1 omitted
this; it makes lock 3 (§4) stronger than the boundary-move term alone.

Two structural consequences of that ladder, both load-bearing below:

- **The first `High` ends the search.** E1 (`patch_region.rs:972`) returns on a `High` baseline
  before anchor search at `:1006` is ever reached. Bracket quality is never compared.
- **The ranking arms cannot choose a large bracket.** At 0.0002/frame the penalty is **9.6 per
  second** of bracket move at 48 kHz. Since waveform scores live in [0, 1], any bracket move
  beyond **5000 frames (104 ms)** drives `ranking_score` negative — it can never beat a candidate
  with a positive min. A large bracket is reachable **only** by an early-return arm
  (E1/E2/E3/E4/E6), never by ranking.

So `min(pre, post)` appears at **two** sites with different roles: the axis-2 objective, and the
axis-1 pooled ranking. `TEMP-unmatchable-shoulder-plan.md` addresses only the first.

---

## 2. What the sensors say about this gap

From `2026-08-13-v0.6.0-oriented/31/919e5368_91d1_t00-28-45_g007_full.json`, the probe arms, and
the archived registration plan's wide sweep
([archive/TEMP-fill-placement-registration-plan.md](archive/TEMP-fill-placement-registration-plan.md)
§4.3 — settled measurement, quoted here, not re-run).

| sensor | what it measures | reading |
|---|---|---|
| `lag_decision.pre_anchor` | waveform lag, 1 s window at `b_mapped`, ±600 ms | **−47.46 ms**, `peak_r` 0.829, prominence 0.552, `peak_z` 18.3, `timing_offset` |
| `lag_decision.post_anchor` | same, post shoulder (window re-centered by the pre lag, §6.6) | **no peak**: `peak_r` 0.0752 vs `second_peak_r` 0.0741, prominence 0.0011, `decorrelated` |
| archived §4.3 wide sweep | pre-shoulder template Pearson at **every integer lag in ±2000 ms** | **single spike at −47 ms (r 0.829), flat floor everywhere else**. At −925.1 ms — where a one-period-back rival would sit — r is **0.0867**, 9.6× below the peak. Quote: *"31/g7 is not periodic at all"* |
| `splice.step_ms` | disagreement between the two shoulders | 378.30 ms |
| `splice_dualfit` | each shoulder at its own best lag | pre **0.966**, post **0.123**, `gate_pass: false` |
| `donor_registration` (equivalence) | **envelope** lag, 100 ms bins | **−1000.0 ms = the coarse window's own edge** (`max_lag_blocks` 10 × 100 ms bins, `gap_equivalence.rs:127`), flagged `donor_registration_unreliable` — an edge-saturated reading, not a lag estimate |
| `donor_interior_nominal` | is B occupied at the nominal span? | `silence_fraction` 0.0, `rms_db` −54.0, `continuous: true` |
| oracle `brackets[]` | seams per A-side bracket, same start | move 0 → 0.436824 / 0.436924; move 36000 → 0.478098 / 0.435196 |

The picture is asymmetric, not ambiguous. The pre shoulder is strongly and **uniquely** registered
at −47 ms — the wide sweep leaves no rival anywhere in ±2 s. The post shoulder is not registered
at all: 0.0752 against a second peak of 0.0741 is noise, not a weak match. The envelope
contributes nothing here (edge-saturated, self-flagged unreliable). One live shoulder, one dead
one — the S2b premise.

---

## 3. The findings, set against the model

### F1 — Axis 1, E1: the search stops before the alternative exists

Production at full radius applies the move-0 bracket at −977 ms, seams 0.437 / 0.437, `High`.
`High` at E1 returns immediately, so anchor search never runs: arm A records
`anchor_seam_used: false`.

The probe made the stutter unreachable (`--fill-border-search-secs 0.20`); E1/E2 then did not
fire, anchor search ran, and returned a **different bracket** — `anchor_seam_used: true`,
`anchor_bracket_move_frames` 36000 (0.750 s), start −10.1 ms, seams 0.728 / 0.421, `total` 0.773
against the stutter's 0.615.

**Against the model:** this is a *termination* property of axis 1, not a scoring defect. No change
to the axis-2 objective can expose the +36000 bracket at full radius, because nothing on axis 1 is
compared once E1 fires. Note also that 36000 frames is far past the 5000-frame point where pooled
ranking goes negative — this bracket could only ever arrive via the anchor early return, which is
exactly how it arrived.

### F2 — Axis 1 ranking: the min-reduction asymmetry, observed on the bracket axis

The oracle swept both brackets at the same start:

| bracket | seam_pre | seam_post | min | max |
|---|---|---|---|---|
| move 0 (applied) | 0.436824 | 0.436924 | **0.436824** | 0.436924 |
| move 36000 | 0.478098 | 0.435196 | 0.435196 | **0.478098** |

`min` prefers the applied bracket by **0.001628**; `max` prefers the other by **0.041174** — a
**25×** asymmetry. If these two brackets were ever ranked against each other, the min-reduction
would keep the applied one and reject the candidate that improves the good shoulder by 0.041 for
lowering the minimum by 0.002.

**Against the model:** production never ranked them — E1 had already returned (F1). This is a
property of the reduction, *observed by the oracle*, not an event on the decision path. It says
the reduction **would** also defend the stutter at the ranking sites, not that it chose it.

### F3 — Axis 2 at the nominal bracket: never measured, and its "truth" exit fails today's rules

The S2b plan predicts axis 2's argmax moves to ≈ −47 ms once the objective stops min-reducing. No
arm measured that: arm B's −10.1 ms start belongs to a *different bracket* (F1), so it is not a
reading of the start search at the nominal bracket. The prediction is untested, not disproved.

Stronger: −47 ms lies **inside** arm B's ±200 ms radius, and the baseline bracket's candidate pool
there was **empty** — no placement of the nominal bracket cleared the 0.35 floor anywhere in
±200 ms (§4, lock 2). Consistent with that, `splice_dualfit` scores the post seam **0.123** even
at its own best lag. So a nominal-bracket fill at −47 ms **fails today's acceptance outright**:
the plan's "truth" exit does not currently exist, and creating it is the plan's burden, not a
latent behaviour waiting to be unblocked. The −977 ms placement is an **ear-confirmed stutter**
(S2b; archived plan: the corpus's only two ear-confirmed wrong repairs are 26/g6 and this gap).
What has not been ear-checked is whether a −47 ms fill — or arm B's 1.87 s geometry — is an
acceptable *repair*.

### F4 — The dead shoulder is real; arm B stepped around it

`post_anchor` is `decorrelated` with no peak, and `splice_dualfit` post is 0.123 with
`gate_pass: false`. Arm B's post 0.4213 is measured at a post border **750 ms later**
(`post_time_secs` 1727.5656 vs 1726.8156) — a different location in A and B.

**Against the model:** the unmatchable-shoulder premise holds at the nominal geometry. What axis 1
did was move the post border *off* the dead spot. That is a geometry change, not evidence the
shoulder is matchable.

### F5 — Fill length is a free variable, and nobody bounds it by gap length

Arm B's accepted fill is **1.87 s for a 1.1188 s gap** (`fill_level.bins` 19 × 100 ms vs 11). The
0.750 s extension exceeds `gap_end_extend_max_ms` (500 ms) because it is the anchor bracket move,
a different mechanism with its own bounds.

**Against the model:** axis 1 changes *how much A is replaced*. The only thing restraining it is
the boundary-move penalty on the ranking arms — which the early-return arms bypass entirely.

### F6 — Downstream decisions remain open

`classify_fill_waveform_confidence`'s floor and `repeat_penalty_from_correlations` both read the
raw `min`. At arm B's geometry: `min` 0.4213 (no floor issue), `repeat_penalty` 0.0 — but
`repeat_max` **0.5482** against a 0.55 threshold is a margin of **0.0018**, with `wave_min` 0.4213
already inside the other clause. Neither is settled for a nominal-bracket fill at −47 ms, which
was never run.

### F7 — Arm C reproduces arm B exactly: the alternative bracket is a real interior optimum

Arm C (`--fill-border-search-secs 0.06`, a 3.3× smaller radius than arm B) returns a patch record
**identical to arm B in every field** — same `align_adjustment_secs` (−0.010145833), same
`anchor_bracket_move_frames` 36000, same seams 0.7276736766 / 0.4212769930, same `fit_terms`, same
`normalize_gain`, same 19 bins.

**Against the model:** two things follow. (a) Arm B's start was **not** pinned at its search
edge — −10.1 ms sits well inside both ±200 ms and ±60 ms, and shrinking the radius did not move
it, so it is a genuine argmax of axis 2 within that bracket. (b) The bracket choice on axis 1 is
**radius-invariant once E1/E2 stop firing**: the anchor arm finds the same +36000 bracket either
way. The full-radius outcome differs from both **only** because of the early return, not because a
wider search found a better candidate.

One incidental difference from arm A worth noting: `seam_shape` is `balanced` at the nominal
bracket but `not_applicable` at the anchor bracket, and `placement_slide_frames` rises 46916 →
487 — the latter still just over `max_lag_frames` 480, so the residual stays `beyond_lag_reach` /
uninformative in all three arms. No arm produced an informative residual veto.

**Re-read after §4.** F7 looked at first like a weak result (a confirmation that arm B was not
edge-pinned). Under lock 2 it is the stronger of the two arms' findings. The bracket set is
identical at ±200 ms and ±60 ms because in *both* arms the baseline fails the floor outright, so
`baseline_seam_scores` falls back to the failure scores and the Auto anchor gate opens on the same
low numbers, enumerating the same brackets. Radius does not select the bracket — **baseline
failure does.** Two radii differing 3.3× produce byte-identical output, which means the entire
difference between the probe arms and production is the single boolean "did the baseline clear
0.35". That is a much sharper statement of the defect than "E1 returns too early".

---

## 4. The three locks: what it would take to change the E1 outcome (code-read)

**E1 is not one lock. It is the first of three, and each one alone is sufficient to keep the
stutter.**

### Lock 1 — `terminates_high` is an absolute 0.35 bar

`classify_fill_waveform_confidence` (`gap_fill_fit.rs:174`) returns `High` iff
`min(pre,post) >= min_fill_correlation`, via `fit_mode_waveform_floor_passes` (`:1905`). The bar
is **absolute and comparative-free**: 0.4368 clears 0.35, so the baseline is `High`, so E1
returns. The router never asks "is anything better?" because `High` means "good enough", not
"best".

### Lock 2 — the anchor gate would not enumerate the alternative anyway

Even with E1 deleted, `should_run_anchor_seam` (`gap_anchor_seam.rs:137`) in the default
`AnchorSeamMode::Auto` requires `min(pre,post) < min_fill_correlation − fill_marginal_margin`
= 0.35 − 0.08 = **0.27**, *and* an energy contour. The baseline's 0.4368 is far above 0.27, so
`anchor_brackets()` returns empty and `try_anchor_seam_joint_search` bails at its first line. The
+36000 bracket is never constructed.

**This is why arm B worked and the full-radius run cannot.** At ±200 ms the baseline bracket found
no placement clearing the floor at all, so it never entered the pool; `baseline_seam_scores` then
fell back to the *recorded failure's* low scores (`patch_region.rs:706`), which opened the Auto
gate. The anchor path was reached by the baseline **failing**, not by anyone judging it mediocre.

### Lock 3 — the boundary-move penalty reasserts itself downstream

Suppose locks 1 and 2 are both lifted and the +36000 bracket is scored. Every remaining exit ranks
with `fit_candidate_ranking_score`:

| candidate | min | boundary_move | ranking_score |
|---|---|---|---|
| baseline (move 0) | 0.4368 | 0 | **+0.4368** |
| anchor (+36000) | 0.4213 | 36000 | **−6.78** |

(The anchor candidate also pays the center-drift term, 0.00015/frame — omitted from the table
because the boundary-move term alone already sinks it.)

- **E3** takes `best_high_joint_candidate` = `max_by(selection_cmp)` over High candidates → the
  baseline; it fires only `if candidate.outcome.anchor_seam_used`, which is false → no fire.
- **E4** takes `best_anchor_joint_candidate` = `global_best_joint_candidate` **filtered to
  anchor** → the global best is the baseline, not an anchor → returns `None`.
- **E5** sorts by `winner_cmp` (highest ranking first) → the baseline again.

So the stutter wins three more times. In arm B, E4 fired **only because the baseline was absent
from the pool**, which is why a 36000-frame move survived a 7.2-point penalty it could never
otherwise pay.

### What the locks mean

Each lock alone is sufficient; a change must clear all three at once, and the third is doing real
work elsewhere — the boundary-move penalty is the only thing stopping the early-return-free arms
from replacing arbitrarily much of A. The cost side is also real: locks 1 and 2 keep anchor search
off the hot path, and `char_gate_search` is **73.8 %** of production wall-clock
(`docs/dev/repair-perf.md`; rev 1 quoted the stale ~93 %, from before perf levers 1/2 landed).

This section is the constraint set any fix must satisfy; the fix itself belongs to the S2b plan,
not here. (Rev 1 sketched a "stop-searching bar" on `waveform_min` in this spot; withdrawn — that
is a threshold on the number §6 shows cannot order the failures. Errata.)

---

## 5. The populations (317-gap corpus scan)

Over `2026-08-13-repeat-terms/` (317 patched gaps, all `fit_path: baseline_only`):

- **210 single-fill patches**, all 210 carrying `fit_terms` — placement recording is **complete**
  on this path. 100 used an anchor seam.
- **107 dual-fit silence-splice rescues** (`dual_fit_used: true`): the seam gate **failed** and
  `try_dual_fit` fitted each seam at its own lag with a synthesized bridge between. That mechanism
  has no single fill slide, so the report arm (`patch_audio/region.rs:1409–1441`,
  `GapRepairStrategy::SilenceSplice`) hardcodes `align_adjustment_secs: 0.0`, zero adjust frames,
  `anchor_seam_used: false` — placeholders, per its own comment ("seams sit at their own lags, so
  there is no align/waveform slide"). The placeholders are byte-identical to measurements, which
  is how an earlier draft counted these rows as "175 zero-move High acceptances" (§7.4, Errata).
  Their two applied per-seam lags are reported **nowhere**, so their placement cannot be checked.

The E1/E2 population — baseline accepted at `High` with **zero** boundary move — is therefore
**68**, and it is fully measured (68/68 `fit_terms`; E1 and E2 are indistinguishable in the report
for a `High` baseline, and functionally identical: both return the baseline untouched). Their
`waveform_min` distribution:

| band | n |
|---|---|
| [0.35, 0.45) | **42** |
| [0.45, 0.55) | 12 |
| [0.55, 0.65) | 9 |
| [0.65, 0.75) | 3 |
| [0.75, 1.00] | 2 |

**31/g7 is not unusual here.** 42 of 68 clear the 0.35 bar by less than 0.10 and terminate routing
on that basis — and barely-clearing is not itself evidence of a bad placement: **40 of the 42**
are not the two known-wrong patches (§6). Crossing the band with a large applied slide
(`|align_adjustment_secs| > 0.5 s`) narrows to exactly those two:

| pair / gap | `waveform_min` | `align_adjustment_secs` | seam_shape | repeat pre/post |
|---|---|---|---|---|
| 26 @ 2151.688 | 0.3663 | **−1.4500 s** | balanced | 0.153 / 0.368 |
| **31 @ 1725.697** | 0.4368 | **−0.9774 s** | balanced | 0.249 / 0.207 |

(Note the repeat scores at the applied geometry are unremarkable on both rows — nothing here
suggests periodic material; see §9.)

Across all 317 patched gaps, 12 have a slide over 0.5 s; 7 of those went through the anchor path.
Large slides are usually **correct**: pair 31's own gaps #9/#11/#12 applied −1.15 s and are
genuine — the archived registration plan measured the pair's true registration stepping from
−0.04 s to −1.15 s across the file, and the renders are clean by ear. This is why the archived
§4.1 withdrew a bare cap on `|applied|` (`max_fill_align_adjustment_secs` exists in config,
unenforced), and why §6 checks *agreement with a landmark*, not magnitude.

---

## 6. Placement vs registration: the join, measured

### 6.1 The metric

    disagreement = | align_adjustment_secs×1000 − nearest of {pre_anchor, post_anchor} frac_lag_ms |

`align_adjustment_secs` is the slide the patcher applied off the nominal map; `frac_lag_ms` is
where the fingerprint's waveform lag sensor says the content actually sits, also off the nominal
map. **"Nearest shoulder" is deliberate generosity**: when the two shoulders disagree
(`splice.step_ms`) there is no single truth, so the metric asks only *"did the placement land on
some defensible registration?"* A gap fails only by agreeing with **neither**.

### 6.2 Two rulers on B, from the same zero

A gap is a hole in **A**; repair copies audio out of **B** into it. Both terms of the metric are
displacements along the B timeline from the same fiducial — the nominal map position
`b_extract.b_mapped_start_frame` — which is the only reason subtracting them is legitimate:

```text
A:  ──────── pre shoulder ────┤##### hole #####├──── post shoulder ────────
                        a_start                a_end

              a_start + fill_offset_secs
                        │
B:  ─────────────────────┼──────────────────────────────────────────────────
                    b_mapped_start          ← where the GLOBAL OFFSET says to look
              ↑         ↑
      alignment.start   │                   ← where the fill was ACTUALLY cut from
              └─────────┘
              align_adjustment_secs  (negative = cut from EARLIER in B)
```

- **The patcher's ruler** — `align_adjustment_secs = (alignment.start_frame −
  b_mapped_start_frame) / sample_rate` (`patch_audio/region.rs:1084`, `:1248` on the spec path).
  It searched ±`fill_border_search_secs` (default **10 s**) around `b_mapped_start` and cut the
  fill where its combined score peaked. *An action, recorded after the fact.*
- **The sensor's ruler** — a template of A's real shoulder audio is slid across B around
  `b_mapped`, and `frac_lag_ms` is the parabolic-interpolated correlation peak. *An independent
  observation, computed by the fingerprint path with no knowledge of what the patcher chose.*

The difference is therefore, literally: **how far along B the fill was cut from, versus where A's
surviving audio says that moment actually lives in B.**

31/g7 in absolute B time (`fill_offset_secs` +1.3627): mapped 1727.0595 → the sensor finds the pre
shoulder's content at **1727.0120** (−47.5 ms); the fill was cut from **1726.0821** (−977.4 ms).
The patcher reached **930 ms earlier in B** than the surviving audio points to.

26/g006 (`fill_offset_secs` +16.2100): mapped 2167.8985 → sensor finds the content at
**2167.8387** (pre) and **2167.8576** (post); fill cut from **2166.4485** — 1.39 s earlier than
*both* shoulders agree on.

### 6.3 Why it is not an exact identity

1. **Different objectives.** The lag sensor maximizes one shoulder's correlation over a 1 s
   window; the patcher maximizes `0.35·structure + 0.65·waveform − penalties` over *both* borders
   jointly. Exact agreement is not expected even when both are right — this is the tens-of-ms
   scatter.
2. **A per-gap zero offset.** `frac_lag_ms` is not raw registration: the A template's gap-facing
   edge is pulled outward by the silence walk-off, the standoff, and the 12 %-of-peak trim, while
   the B anchor sits on the gap edge itself. A perfectly registered pair reads `−D_pre` / `+D_post`
   (`schema.rs:975`). The correction field, `border_displacement_ms`, is **absent from all 1063
   `2026-08-13` dumps** (it postdates them), so every number below is uncorrected.
3. **The corrections are tens of milliseconds.** The outliers below are 930–3057 ms — 40–100×
   larger. Applying the correction would sharpen the core and cannot move the outlier set.

### 6.4 The corpus reproduces the documented decomposition, which validates the axis

On the 64 agreeing gaps:

| quantity | median | mean | documented |
|---|---|---|---|
| `post_lag − pre_lag` (= `D_pre + D_post`, the probe's own displacement) | **+7.81 ms** | — | median 11.3 ms |
| `applied − pre_lag` | **+26.87 ms** | +24.70 ms | "the other ~25 ms is the applied placement sitting late" |
| `applied − post_lag` | +17.03 ms | +13.47 ms | — |

Both halves of the registration plan's `−21.9 ms` corpus bias fall out of this join independently
(archived plan, "Step 5 result"). The p50 of 22.0 ms below **is** that known bias, not noise — and
an axis whose bulk re-derives known physics from data it was not fitted on is measuring something
real. If the two terms had different bases, no peak at zero could exist; the pair-26
"incommensurate bases" escape is closed by this table (Errata).

### 6.5 The distribution

| percentile | disagreement (ms) |
|---|---|
| p10 | 1.6 |
| p25 | 5.9 |
| p50 | 22.0 |
| p75 | 46.1 |
| p90 | **126.9** |
| p95 | **930.0** |
| p100 | 3056.8 |

58 of 68 agree within 100 ms; 64 within 250 ms; then **nothing between 250 ms and 930 ms**, and
four gaps beyond:

| pair @ a_start | `waveform_min` | applied | pre_lag | post_lag | disagreement | prominence pre/post | verdicts | dualfit |
|---|---|---|---|---|---|---|---|---|
| 50 @ 3777.50 | 0.5167 | +2807.4 ms | −526.11 | −249.37 | **3056.8** | 0.005 / 0.003 | deco/deco | fail |
| 50 @ 3424.14 | 0.5365 | +2823.8 ms | +135.17 | +177.79 | **2646.0** | 0.009 / 0.006 | deco/deco | fail |
| 26 @ 2151.69 | 0.3663 | −1450.0 ms | −59.77 | −40.89 | **1390.2** | 0.025 / 0.027 | timi/timi | **pass** |
| 31 @ 1725.70 | 0.4368 | −977.4 ms | −47.46 | +330.84 | **930.0** | 0.552 / 0.001 | timi/deco | fail |

**`waveform_min` does not order this set.** Two of the four (0.5167, 0.5365) score *better* than
31/g7 and sit in a band a bar at 0.55 would sweep up alongside 50 other gaps. Ordering by
disagreement finds 4 of 68; ordering by `waveform_min` finds 54 to catch the same 4 — the same
error family as the refuted `fill_level` cutoff.

### 6.6 Reach: the sensor's window is sequential, not symmetric

The patcher searches ±10 s. The sensor does **not** have one fixed window:

- The **pre** shoulder is swept ±600 ms around nominal (`lag_max_lag_ms`, default pinned at
  `contract.rs:363`).
- The **post** sweep is **re-centered by the measured pre lag** (sequential registration, ledger
  A2; `lag_pair` at `measure.rs:893`), and its `frac_lag_ms` is reported **gross** — relative
  to nominal, not to its own window center.

Rev 1 described both shoulders as ±600 ms at nominal; that is wrong, and 31/g10 is the proof: its
post reads `frac_lag_ms` **−1152.1 ms** (`peak_r` 0.995) under a 600 ms max lag, by chaining off a
weak pre (−553.6 ms, `peak_r` 0.158). Consequences: "in reach" must be judged **per shoulder**; a
confident post landmark can legitimately sit beyond ±600 ms of nominal; and a dead pre does not
widen the post's reach — it only recenters it by whatever the pre reported.

Beyond its window the sensor cannot report a misplacement, only silence about it. `decorrelated`
means *"not found in reach"*, never *"the patcher is wrong"* (§7.2).

### 6.7 The classification, and its scope

Scored with a landmark-confidence test that handles broad peaks —
`peak_r ≥ 0.5` **and** (`prominence ≥ 0.10` **or** `top2_spacing_ms ≤ 5 ms`) — and an agreement
tolerance of 250 ms (inside the unpopulated 250→930 ms band):

| | n |
|---|---|
| gaps with ≥1 confident in-reach landmark | **65 / 68** |
| of those, applied agrees within 250 ms | **63** |
| **applied contradicts a confident landmark** | **2** — pair 26, pair 31/g7 |
| no confident landmark (sensor has no opinion) | 3 (both pair-50 gaps; pair 53 @ 7250.59 is a rule artifact — `peak_r` 0.999 and it agrees at +69.6 ms) |

**Scope, stated plainly: this is a finding of this join, under this filter, on this vintage.** The
confidence rule and the tolerance were written *after* inspecting the outliers (the broad-lobe
clause exists because pair 26's 0.9969 landmark otherwise read "weak"). The 63/2/3 split therefore
awaits prospective validation on the next dump vintage before any of its numbers is treated as a
corpus constant. What does **not** depend on the filter: pairs 26 and 31/g7 disagree with
landmarks of `peak_r` 0.997 and 0.829 by 1.39 s and 0.93 s — no defensible confidence rule
excludes those two.

The check is **agreement, not magnitude**: pair 31's genuine −1.15 s placements (#9/#11/#12, §5)
agree with their own shoulders' landmarks and read as confirmed. A magnitude cap would refuse
them; an agreement check does not — which is the archived §4.1 lesson, kept.

### 6.8 The four beyond the dead band, one at a time

The sensor's output is a curve of correlation vs lag; its **shape** is what licenses (or forbids)
each reading:

| shape | fields | physically |
|---|---|---|
| one tall isolated spike | high `peak_r`, high `prominence` | a **unique landmark**; its position is unambiguous |
| one tall **broad** spike | high `peak_r`, low `prominence`, `top2_spacing_ms` ≈ 0 | still unambiguous — the "rival" is the same lobe a few samples over. **`prominence` alone misreads this as weak** |
| many near-equal **tall** peaks | `peak_r` ≈ `second_peak_r`, both well above the floor, spacing = a real period | self-similar / periodic material; every tooth is a legitimate-looking answer |
| flat | low `peak_r`, `prominence` ≈ 0 | **not found in reach.** No opinion — and the spacing between its noise bumps is **not** a period |

The last cell's caveat is load-bearing: rev 1 read 31/g7's post curve (`peak_r` 0.075 vs 0.074,
bumps 921 ms apart) as the third row and built a repeat story on it. The archived wide sweep shows
the surface is flat, not periodic (§2, §9). The corpus's real third-row exhibit is pair 26's
shoulder surface — found by the archived ±2000 ms sweep, not by `top2_spacing_ms` at noise level.

- **Pair 50 ×2 — flat both sides → no opinion.** `peak_r` 0.084–0.173, prominence ≤ 0.009: A's
  shoulders are not found within either sweep window, and the applied slides (+2.8 s) are far
  beyond both. The two gaps are **353 s apart with the same `fill_offset_secs` (0.5966) and
  slides matching to 16 ms** — consistent with a regional map correction (*inference*).
  Classification: no opinion — explicitly neither a defect claim nor an exoneration;
  unverifiable at this vintage.
- **Pair 26 @ 2151.69 — contradicted; the open scoring question.** Every sensor is healthy and
  agreeing: `peak_r` 0.9969 / 0.9930, both `timing_offset`, `step_ms` 18.9, dual-fit **passes**
  (0.993 / 0.931), envelope `lag_ms` 0.0 with `peak_r == nominal_r` 0.8036. The archived wide
  sweep adds the mechanism-relevant fact: this shoulder surface genuinely **is** periodic — a
  **480.5 ms comb** — and the applied −1.45 s sits on the **third tooth**; but the true peak
  still **ranks first** in that sweep, so even the real comb did not out-score truth for the
  sensor. Zero boundary move ⇒ E1 acceptance ⇒ the −1.45 s is **axis 2's argmax**: the joint
  objective, searching ±10 s, preferred a 0.366-min seam 1.4 s away over a first-ranked
  0.997-correlation landmark ~50 ms away. **Why is undiagnosed** — one probe (oracle the axis-2
  start sweep at this gap) resolves it, and this is the cleanest source-defect lead in the corpus
  because "correct" has an unambiguous answer here.
- **Pair 31/g7 @ 1725.70 — contradicted; mechanism unexplained.** One live shoulder (−47 ms,
  unique in ±2 s), one dead. The gate accepted `min` **0.437** at −977 ms, where the wide sweep
  reads **r 0.087** for the pre-shoulder template. The two numbers come from different windows (a
  seam window at the proposed joint vs a 1 s shoulder template), so they are not formally the same
  measurement — but whatever the seam window matched at −977 ms, it was not the shoulder's content
  found again. With aliasing refuted, **the mechanism of a 0.437 Pearson at a wrong placement is
  an open scoring question on this gap**, not the same question as pair 26. Here the shape is
  ill-posed (`min` over a dead post); there the shape is healthy and axis 2's argmax is
  undiagnosed. Different windows still apply: seam Pearson at the joint vs 1 s shoulder template.
- **17/g34** (not in the 68-gap zero-move join; the archived plan's other residual giant) is a
  third member of pair 26's class: `peak_r` 0.990 / 0.987, `peak_z` 28.7 / 26.5, prominence
  0.79 / 0.66, dual-fit pass, envelope 0 — healthy, unique, agreeing registration, and a ~1.15 s
  backward slide anyway.

### 6.9 What this changes

1. Among the 68, the S2b *symptom* (one confident landmark, large backward slide) is **1 of 68**.
   That is not the same as the lag-verdict one-dead **dump predicate**, which is 13 of 308 patched
   (§8) — and is mostly sensor error, not a class (banner).
2. **Pair 26 is the better lead for the source defect**: every sensor healthy and agreeing, a
   measured comb, a first-ranked truth — and a defined one-probe experiment. It is not the S2b
   plan's class.
3. Pair 50 is the independence proof (§0): the gate's score is not a registration measure.
4. Recording `applied − landmark` inside the patcher is **BACKLOG S1** (blocked on
   envelope-recenter): a **patch diagnostic**, descriptive only. It is not a gap discriminator and
   not a gate (§8).

---

## 7. The vocabulary findings

This is the measured form of what `gap-vocabulary-redesign` predicted from the fingerprint side;
the new part is that it shows up on the **placement** side, where the vocabulary has no words at
all.

### 7.1 `SeamShape` inherits the blindness, and its calmest word marks the worst gaps

`classify_seam_shape(pre, post)` (`gap_tags.rs:361`) takes **only the two seam Pearsons**. Every
seam label — `balanced`, `asymmetric_pre/post`, `symmetric_weak` — is a function of the one number
§6 shows is registration-blind. So the vocabulary cannot describe a misplacement; it can only
describe the join.

Both demonstrably-misplaced gaps are tagged **`balanced`** (`pre, post ≥ 0.27`,
`|pre − post| ≤ 0.15`) — the label that means "both sides join equally well, nothing to see". Pair
26 is `balanced` at 0.3663 while sitting 1.39 s from a 0.9969-correlation landmark. **The most
reassuring word in the vocabulary is applied to the corpus's only two known-wrong placements.**
That is not a threshold that needs moving; it is a term measuring the wrong thing. (`SeamShape` is
report-only — nothing gates on it — so it is the reporting tail of the chain, not the decision
head; the decision path reads the same Pearsons directly.)

### 7.2 `LagVerdict::Decorrelated` claims more than the instrument can support

Its definition is *"No shift recovers correlation — **sources genuinely differ**"*
(`schema.rs:929`). That is a claim about the **media**. The measurement behind it is a bounded
sweep (§6.6). Those are not the same statement, and pair 50 is where they come apart: labelled
"sources genuinely differ" at two gaps whose content is, on the evidence, ~2.8 s away — outside
the sweep, in a pair whose slides agree to 16 ms across 353 s.

`edge_pinned` exists as the guard for exactly this ("the true optimum may lie beyond `±max_lag`"),
but it fires only when the integer peak lands **at the sweep edge**. A flat noise curve peaking
somewhere in the middle — which is what out-of-reach content actually produces — never sets it.
**The guard misses precisely the case it was written for.** Pair 50 reads `edge_pinned: false` on
all four shoulders.

The vocabulary needs a verdict that means **"no opinion — not found within reach"**, distinct from
**"looked, and the sources differ"**. Today one word carries both, and only one of them is
falsifiable. This is a definitional fix requiring no new measurement.

### 7.3 Pair 26's missing word is the choice; 31/g7's is the gap's shape

The vocabulary names two things:

- **the quality of the join** — `seam_shape`, `patch_tier`, `residual_band`, `fill_level`
- **the relationship of the sources** — `LagVerdict`, `donor_relation`, `splice_dualfit`

Nothing names **the relationship between the placement chosen and the registration measured.**
That is pair 26's hole: the healthiest fingerprint in the outlier set and the worst placement in
the corpus is *unsayable* in current terms — every tag it carries is accurate and every one is
reassuring. 31/g7 is a different hole. Joint registration is ill-posed there (one live shoulder,
one dead); the missing word is one-sided closeability, a *gap* cell, parked in
[TEMP-31g7-gap-shape-vocabulary.md](archive/TEMP-31g7-gap-shape-vocabulary.md). Residual can group the
two as patches; it cannot give them one vocabulary axis.

That is why it took a post-hoc join of two separate artifacts to find two unlike gaps; the
choice-vs-measurement concept exists nowhere in the running system, and neither does the
closeability cell.

### 7.4 It is also an *observability* failure, which is the more fixable half

**Every dual-fit patch (107 of 317) records placeholders where its placement would go**:
`align_adjustment_secs: 0.0`, zero adjust frames, no `fit_terms` (`region.rs:1409–1441`, §5). A
dual-fit placement is two per-seam lags plus the step between them, and **none of the three is
reported anywhere**. The fingerprint side records what dual-fit *would* choose (`splice_dualfit`,
per seam), but the applied values never surface, so application cannot be verified — exactly the
blind spot the A7 re-validation bug lived in. And the placeholders are indistinguishable from
measurements, which is how this doc's own first draft mistook 107 rescues for E1 acceptances.
Single-fill, by contrast, is fully recorded (210/210 `fit_terms`).

### 7.5 What this argues for — and what was withdrawn

2 of 68 argues for description, not enforcement:

- **Record what dual-fit applied** (§7.4): the two per-seam lags and the reconciliation step, as
  `Option`s so placeholders cannot masquerade as measurements. Needed only to *extend* the check
  to the splices; nothing in this note is blocked on it. (The question changes form there —
  per-seam, not per-patch.)
- **Fix the `Decorrelated` overclaim** (§7.2) — definitional, no new measurement.
- **Emit the residual descriptively** — that is **BACKLOG S1** (blocked on envelope-recenter),
  already filed: a patch diagnostic, not a gap class (§8), not a veto.

Rev 1 went further: a named placement-provenance axis (`confirmed` / `contradicted` /
`unlandmarked` / `unmeasured`), a `Periodic` lag verdict, and two patch-side cells
(`Misregistered patch`, `Unverifiable patch`). **Withdrawn to a backlog sketch.** Reasons: the
`Periodic` verdict's exhibit was the refuted repeat reading of 31/g7 (§9); the cells' membership
predicate is exactly the §6.7 filter, which is post-hoc until prospectively validated on a second
vintage; and 2-of-68 does not justify new vocabulary ahead of that validation. The **findings**
stand — placement-vs-registration is real, measurable, and unnamed **for the well-posed
failures** (26/g6, 17/g34); 31/g7's missing word is the gap cell, not that axis
([TEMP-31g7-gap-shape-vocabulary.md](archive/TEMP-31g7-gap-shape-vocabulary.md)). The **names** wait.

What none of this argues for is re-tuning `min(pre, post)` at either call site. That number is
not weak here — it is answering a different question than the one being asked of it.

---

## 8. No shared gap signature (oriented 308-gap join)

§6 finds the two labelled failures by `applied − frac_lag`. That quantity needs the production
decision, so it cannot be what the *gap* looks like. Route 2 and the archived parent already
concluded residual was "the only live discriminator" — that conclusion detects bad **patches**; it
does not classify the gaps.

Join `gap-files/2026-08-13-repeat-terms/` patches to `gap-files/2026-08-13-v0.6.0-oriented/` dumps
on `geometry.a_start_secs` (pairs 6 and 16 dropped as duplicates of 22 and 28). **308** patched
gaps carry both shoulders. No `align_adjustment_secs` in these cuts:

| fingerprint test (no `applied`) | n | who |
|---|---|---|
| one unique (`peak_r ≥ 0.8`, `peak_z ≥ 12`) + one no-peak (`prominence < 0.02` or `peak_r < 0.3`) | **13** | lag-verdict one-dead **dump predicate** — includes 31/g7; **not** 31/g10 (`peak_z` 8.6 < 12). Census 2026-08-18: 12 of 13 are in-reach false-deads, not a media class (banner) |
| `splice_dualfit` `max ≥ 0.9` and `min < 0.20` and `gate_pass: false` | **1** | **31/g7** (0.966 / 0.123) |
| `splice_dualfit.gate_pass: true` | 297 / 308 | so a bare `gate_pass: false` is 11 gaps, not a mark |

`gate_pass` itself stays a production mirror (BACKLOG dual-fit confidence axis: do not tighten
it). The isolating read is the **split** — one seam near-perfect, the other dead — which
production already declined.

**31/g7** is closeability: one uniquely live pre (`r` 0.829, `z` 18.3, prominence 0.552) and a
post with no peak (`r` 0.075 vs 0.074, prominence 0.001), dual-fit 0.966 / 0.123 fail. (The
envelope reads the window edge and is self-flagged unreliable — a caveat, not a signature; rev 1's
`|lag| ≥ 900` signature row is withdrawn, see Errata.) S2b's "one dead shoulder ⇒ change `min`"
was aimed at this dump predicate, not at this mark — and the predicate itself is mostly sensor
error (banner), so it was never a population for changing `min`.

**26/g6** is the opposite fingerprint: both shoulders live, dual-fit **passes** (0.993 / 0.931),
envelope `lag_ms` 0 with `peak_r == nominal_r`. §6.8's curve shape is a **broad unique spike**
(`top2_spacing_ms` 0.104 ms = the same lobe, five samples). Do **not** treat
both-prominence-tiny (0.025 / 0.027) as an isolator — that is the misreading the broad-spike row
warns about — and `peak_z` ~6 on both sides is common (60 of 308 both-live gaps have `z < 12`).
The gap looks fillable. The failure is that scoring walked −1.45 s anyway (§6.8).

**17/g34** is the same *gap* class as 26/g6: both-live, dual-fit pass, unique registration
ignored (`peak_r` 0.990 / 0.987, `peak_z` 28.7 / 26.5, prominence 0.79 / 0.66, envelope 0) —
sharper uniqueness than 26's broad lobe, same well-posed shape. Residual is what also groups it
with 31/g7, which is why residual *looked* like a shared discriminator of unlike gaps, and why
it is the wrong altitude for "discriminators of the gap".

**There is no shared gap signature to harvest.** The shared thing is only the patch symptom — a
large backward slide onto already-played A. S1 may still emit residual as a descriptive patch
diagnostic after envelope-recenter; it must not be promoted to a gap class or a veto.

---

## 9. The admission test: how a term (or a mechanism) earns a place

The standing hazard when a vocabulary is extended off two exemplars is bolting on a **secondary
correlation** (a new word for the same number), a **weak fixture** (a class whose extension is
"the gaps we found it on") — or, as rev 1 demonstrated, a **mechanism the archive had already
refuted**. Four criteria:

0. **Check settled measurement first.** Before asserting a mechanism or admitting a term, grep the
   archive for a measurement that already answers the question. Rev 1's repeat story died here:
   the archived registration plan §4.3 had already swept the pre shoulder at every integer lag in
   ±2000 ms and found a single spike on a flat floor — *"31/g7 is not periodic at all"* — before
   rev 1 re-asserted periodicity from weaker sensors.

1. **Independence, proven by contradiction.** A term is admitted only if it is rooted in a
   measurement that *can* disagree with the existing terms — and demonstrably does on real data.
   The lag sensor answers "where is A's shoulder content found in B" from its own sweep; the joint
   Pearson answers "does the proposed splice correlate". On 2 of 68 gaps they disagree while every
   joint-shaped tag stays green — that disagreement is the admission evidence. A secondary
   correlation can never contradict its parent: `patch_tier` re-bins the same Pearsons and so
   cannot ever say something `SeamShape` doesn't already imply; it added a word, not information.

2. **Physical predicate on the shared fiducial, not a threshold on a score.** Every quantity is
   defined in media units — displacements along B from `b_mapped_start_frame` (§6.2) — and its one
   tolerance (250 ms) sits inside a measured unpopulated dead band (250–930 ms), so no tuning knob
   carries the definition. The axis must also re-derive known physics from data it was not fitted
   on: the bulk's p50 = 22 ms reproduces the documented ~−21.9 ms placement bias, and post−pre
   reproduces the border-displacement decomposition (§6.4). A term whose distribution the model
   cannot predict is decoration. Where a predicate is post-hoc anyway (the §6.7 confidence rule),
   say so, and demand prospective validation before it graduates.

3. **Defined by predicate, validated descriptively, pre-committed deletable.** The exemplars
   motivate a term; they must not define it. "Applied contradicts a confident in-reach landmark"
   is meaningful on gaps that resemble neither exemplar — and indeed the two members share no
   fingerprint signature (§8). Ship any tag descriptively first (the redesign plan's P4), watch a
   full corpus, and delete it if it turns out reducible to an existing axis — the same
   scaffolding discipline as migration differentials. The predicate must bend only to physics,
   never to the exemplars: when pair 26's broad lobe broke the first confident-landmark predicate,
   the repair (`top2_spacing ≤ 5 ms`) came from what a broad single lobe *is* (rival peaks inside
   one lobe), not from adjusting until the known answer came out.

### The negative exhibit: repeat-lock

Rev 1 proposed a `repeat-lock` mechanism subtype ("placement lands k periods from truth") with
31/g7 as its exhibit. It failed three of the four criteria:

- **Criterion 0:** the archived ±2000 ms sweep had already measured the question directly — the
  predicted rival at −925.1 ms reads r 0.0867 against 0.829 at truth, flat floor between. (The
  corpus *does* contain a real comb — 480.5 ms, on pair 26, applied placement on the third
  tooth — but there the true peak still ranks first, so even the real comb does not produce a
  repeat **lock** in the sensor.)
- **Criterion 2:** its evidence was three weak sensors fused: a 921 ms spacing between two
  noise-floor bumps (r 0.075 vs 0.074 — not peaks) that rhymed with −977; an envelope "agreement"
  at −1000 ms that is the coarse window's own edge, self-flagged unreliable; and repeat scores
  (0.548) measured at a different bracket geometry than the stutter's.
- **Criterion 3:** it fused those to tell one exemplar's story — a weak fixture wearing a physical
  costume.

Named rejects, for contrast:

- **"LowMarginHigh"** (`waveform_min ∈ [0.35, 0.45)`): a threshold on the number that already
  failed to ask the registration question. 42/68 gaps carry it; 1 of the 42 is misplaced. Pure
  secondary correlation — same family as the refuted `fill_level` cutoff and the equivalence
  margin band.
- **"StutterRisk"** ("looks like 31/g7"): weak fixture — its extension is its exemplar. The
  ±25 ms cross-codec bucket was exactly this failure and had to be retired.

---

## Errata — what rev 1 claimed and rev 2 removed (2026-08-18)

Rev 1 is in git history at this path. Removed, with reasons:

1. **The repeat/aliasing story for 31/g7** — the headline clause "…or both a repeat of the same
   periodic phrase", old F5 ("the applied placement has independent support"), the one-period
   reading of the post curve, the proposed `Periodic` verdict and `repeat-lock` subtype, and its
   curated fixture. Refuted by the archived registration plan §4.3's ±2000 ms sweep (§2, §9).
2. **"A fix must beat −977."** There is no rival waveform peak at −977 ms to beat; the 0.437 seam
   score *is* the defect, and its mechanism is open (§6.8).
3. **The envelope as a "third sensor"**, and the old §11 signature row `envelope |lag| ≥ 900`.
   The −1000 ms reading is the coarse window's own edge (`max_lag_blocks` 10), self-flagged
   unreliable; the row's other member (2/g18) was noise membership.
4. **The "stop-searching bar" fix sketch (0.55) and its band sizing.** A threshold on
   `waveform_min` — the number §6.5 shows cannot order the failures. Fix design belongs to the
   S2b plan.
5. **"175 baseline-accepted / 61 % unmeasured."** Dual-fit placeholder contamination; the real
   populations are in §5 (68 accepted, fully measured; 107 splices whose applied per-seam lags
   are unrecorded).
6. **Pair 50 as misplacements.** The sensor has no opinion there; the map-correction reading is an
   inference, not an exoneration, and neither direction is claimable at this vintage (§6.8).
7. **Pair 26's "maybe incommensurate bases" escape.** Both terms are displacements from
   `b_mapped_start_frame`, and the bulk's 22 ms agreement forecloses a base mismatch (§§6.2,
   6.4). What remains for pair 26 is the sharper open question in §6.8.
8. **Symmetric "±600 ms on both shoulders" reach.** The post sweep is re-centered by the measured
   pre lag and reported gross (§6.6); 31/g10 reads a post landmark at −1152 ms under a 600 ms max
   lag. Conclusions survived; the reach arithmetic did not.
9. **The old §12 vocabulary extension as a proposal** — the provenance axis with values, the
   `Misregistered` / `Unverifiable` patch cells. Withdrawn to a backlog sketch pending prospective
   validation of the §6.7 filter (§7.5). The findings stand; the names wait.
10. **Stale or incomplete numbers:** `char_gate_search` ~93 % → **73.8 %** (perf levers 1/2
    landed); the §1 ladder omitted E3; lock 3 omitted the anchor center-drift penalty
    (0.00015/frame).
