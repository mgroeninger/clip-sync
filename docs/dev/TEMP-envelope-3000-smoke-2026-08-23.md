# TEMP — Flat 3000 ms envelope smoke: the 2000 ms cap was censoring pair 50 (2026-08-23)

**Status:** findings note, not a Change, 2026-08-23. No production behaviour moves; no default is
flipped. Two pairs only (31, 50) — this is a **smoke of the recipe**, not a corpus sizing run.

> **The corpus run this smoke cleared has landed (2026-08-24):**
> [TEMP-envelope-3000-corpus-2026-08-24.md](TEMP-envelope-3000-corpus-2026-08-24.md). That note owns
> the base rate; this one still owns the narrower question — *does a flat 3000 ms recipe behave on
> the two pairs whose map steps are already known?* Read them in that order and do not quote this
> note for a sizing figure.

**The question this note owns:** BACKLOG § *Fill placement* order-of-work item 3 names a **flat**
`--lag-envelope-max-lag-ms` re-fingerprint as the cheap route to sizing the step-boundary
population, after the *adaptive ladder* was refuted and archived. Before paying for the corpus, run
the recipe on the two pairs whose map steps are already known (31 ≈ −1.15 s, 50 ≈ +2.789 s) and ask:
does a flat widen behave — does it reach the far step without disturbing anything that already
worked?

**Answer: yes, and it un-censors pair 50.** Every shoulder alive at 2000 ms is bit-identical at
3000 ms; 22 shoulders that were `decorrelated` / `declined` now lock at r 0.93–1.00 on pair 50's
+2.8 s step; the pair's one edge pin is gone; patch/skip outcomes are unchanged gap-for-gap.

**What this does not do:** it does not size a corpus-wide base rate (2 pairs of 48), does not produce
a boundary gap to test a classifier on, and does not change a single fill.

Per [gap-vocabulary-redesign / licensed-media-names-never-recorded], gaps are referred to by corpus
pair index + gap index only. Source dumps are gitignored (`gap-files/`) and may be deleted; the
numbers below are the durable record — see **Reproduction**.

**Related:** [BACKLOG.md](../../BACKLOG.md) § *Fill placement* (owns the sizing item);
[TEMP-pair50-anchor-seam-outcomes-2026-08-21.md](TEMP-pair50-anchor-seam-outcomes-2026-08-21.md)
Result 1 (predicted this outcome — see Result 2 below);
[TEMP-collapsed-bridge-census-2026-08-20.md](TEMP-collapsed-bridge-census-2026-08-20.md) Result 8
(the zero base rate this note bounds);
[archive/TEMP-envelope-adaptive-widen-plan.md](archive/TEMP-envelope-adaptive-widen-plan.md)
(the refuted ladder — see Result 5).

---

## Data sources

- **The smoke**, `gap-files/2026-08-23-envelope-3000/` — pairs 31 (13 gaps) and 50 (20 gaps), full
  tier, `--lag-envelope-max-lag-ms 3000`, everything else at defaults. **33 per-gap JSONs**
  (13 + 20) plus a `corpus.json` and `manifest.json` per pair — 37 files, not 37 gaps. Pair list:
  `gap-files/envelope-3000-smoke.csv`.
- **The 2000 ms comparison arm**, `gap-files/2026-08-23-complete-corpus/` — same day, same binary,
  48 pairs / **1043** gaps (plus an empty `51/`), `lag_envelope_max_lag_ms: 2000` and
  `border_capture: narrow` stamped on every pair. Both 31 and 50 are present in it. Diffing against
  this arm makes the comparison **provenance-backed**: the recipes differ in exactly one field.

**Recipe as run** (`source.probe_recipe`, identical on both smoke pairs):

| field | smoke | comparison arm |
|---|---|---|
| `lag_envelope_max_lag_ms` | **3000** | **2000** (default) |
| `lag_envelope_window_secs` | 2.0 | 2.0 |
| `lag_max_lag_ms` | 600 | 600 |
| `lag_window_secs` | 1.0 | 1.0 |
| `border_capture` | `narrow` | `narrow` |
| `noise_collar_secs` / `border_standoff_secs` | 5.0 / 0.0 | 5.0 / 0.0 |

One knob moved. That is the point of the smoke — and unlike the 2026-08-19/20 arms, which had to be
told apart by reading `border_displacement_ms`, this one is readable from the recipe stamp
(`source.probe_recipe.border_capture`, shipped 2026-08-22).

---

## Result 1 — pair 31 is a null control: 3000 ms changes nothing

All **24** shoulder entries on pair 31 (13 gaps; g0 carries no `pre_anchor`, g12 no `post_anchor`)
are identical between the two arms on every compared field — `frac_lag_ms`, `peak_r`,
`envelope_rescue`, `envelope_lag_ms`, `envelope_peak_r`, `envelope_edge_pinned`,
`border_displacement_ms`, `verdict`. Zero changed fields. Its −1.15 s step was already inside the
2000 ms envelope, so widening had nothing to reach for.

The pair's landmarks are unmoved:

| shoulder | 3000 ms reading (= 2000 ms reading) |
|---|---|
| 31/g7 pre | −38.1 ms / r 0.829 / `not_consulted` |
| 31/g7 post | −1160.3 ms / r 0.782 / `rescued` at −1200 / not edge-pinned |
| 31/g8 pre | 550.8 ms / r 0.152 / `declined` at +800 — still the Narrow-capture miss, not a reach failure |
| 31/g8 post | −1137.7 ms / r 0.939 / `rescued` at −1100 |
| 31/g12 pre | −565.1 ms / r 0.934 / `not_consulted` — inside the 600 ms nominal window, unrelated to the step |

Pair 31's 8 rescued shoulders land at −1100 / −1200, `peak_r` floor 0.782, `env_r` floor 0.763.
`splice.step_ms`: 31/g7 **−1122.19**, 31/g8 **−1688.44**. 31/g7 is the within-gap boundary on this
pair; g8's −1688 is that false pre, not the map step. Whether it is the only within-gap instance
in the corpus is what the unpaid 3000 ms run sizes.

**Reading:** a flat widen is not a corpus-wide re-baselining. Where the step is already in reach,
the shoulders are unchanged (the recipe stamp moves).

**Scope note on "censored".** Census Result 8 listed pair 31's step as censored, but that was measured
on `gap-files/2026-08-19-decision-map-oriented`. The **current** 2000 ms dump already sees 31's step
(g7 post rescues at −1200). So the un-censoring below is a pair-50 result only.

## Result 2 — pair 50's +2.8 s step becomes an interior lock; 22 shoulders come alive

The 2000 ms arm **did run** pair 50, on all 20 gaps. It did not miss them — it measured them as
noise, which is precisely census Result 8's stated mechanism (*a step past the search radius kills
the correlation that would measure it*). At 3000 ms the same shoulders read as one clean offset.

| gap / side | 2000 ms: lag / r / verdict | 3000 ms: lag / r / env lag / rescue |
|---|---|---|
| 7 pre | −16.6 / 0.985 / timing_offset | −16.6 / 0.985 / — / not_consulted |
| 7 post | −48.7 / 0.996 / timing_offset | −48.7 / 0.996 / — / not_consulted |
| 8 pre | 46.7 / 0.221 / decorrelated | **2767.1 / 0.928 / 2800 / rescued** |
| 8 post | 66.6 / 0.115 / decorrelated | **2805.6 / 0.971 / 2800 / rescued** |
| 9 pre | 77.5 / 0.053 / decorrelated | **2805.4 / 0.995 / 2800 / rescued** |
| 9 post | 589.4 / 0.046 / decorrelated | **2805.4 / 0.997 / 2800 / rescued** |
| 10 pre | −489.6 / 0.095 / decorrelated | **2805.2 / 0.993 / 2800 / rescued** |
| 10 post | 452.8 / 0.241 / decorrelated | **2761.3 / 0.995 / 2700 / rescued** |
| 11 pre | −476.5 / 0.121 / decorrelated | **2761.4 / 0.997 / 2700 / rescued** |
| 11 post | 563.7 / 0.080 / decorrelated | **2761.4 / 0.998 / 2800 / rescued** |
| 12 pre | 41.1 / 0.163 / decorrelated | **2761.4 / 0.995 / 2800 / rescued** |
| 12 post | 474.5 / 0.099 / decorrelated | **2761.4 / 0.996 / 2800 / rescued** |
| 13 pre | 474.5 / 0.090 / decorrelated | **2761.4 / 0.996 / 2800 / rescued** |
| 13 post | −457.0 / 0.034 / decorrelated | **2761.4 / 0.986 / 2800 / rescued** |
| 14 pre | 138.6 / 0.113 / decorrelated | **2773.7 / 0.992 / 2800 / rescued** |
| 14 post | 177.0 / 0.084 / decorrelated | **2815.8 / 0.992 / 2800 / rescued** |
| 15 pre | −554.5 / 0.069 / decorrelated | **2815.2 / 0.995 / 2800 / rescued** |
| 15 post | 430.5 / 0.106 / decorrelated | **2809.6 / 0.986 / 2800 / rescued** |
| 16 pre | −523.2 / 0.173 / decorrelated | **2795.3 / 0.992 / 2800 / rescued** |
| 16 post | −249.5 / 0.158 / decorrelated | **2803.8 / 0.984 / 2800 / rescued** |
| 17 pre | 292.9 / 0.074 / decorrelated | **2803.7 / 0.989 / 2800 / rescued** |
| 17 post | −506.0 / 0.078 / decorrelated | **2803.7 / 0.987 / 2800 / rescued** |
| 18 pre | −89.6 / 0.306 / ambiguous | **2811.7 / 0.987 / 2800 / rescued** |
| 18 post | 69.9 / 0.142 / decorrelated | **2805.7 / 0.972 / 2800 / rescued** |

**22 shoulders** (g8–g18, both sides), all `rescued` / `timing_offset`, all landing in
**2761–2816 ms** — a ~55 ms spread around the pair's named +2.789 s step. Envelope lags are **+2700
or +2800** (positive only; the −1100 / −1200 rescues belong to pair 31). Fine-search `peak_r` floor
is **0.928** (g8 pre) and ceiling 0.998; envelope `env_r` floor is **0.819**. None edge-pinned.

**The named edge pin is gone.** 50/g009 post was the pair's **only** edge-pinned shoulder at 2000 ms
(`envelope_edge_pinned: true`, `envelope_peak_r` 0.473 at +2000, `declined` / `decorrelated`). At
3000 ms: `envelope_lag_ms` 2800, `envelope_edge_pinned: false`, `envelope_peak_r` 0.999,
`frac_lag_ms` 2805.4, `peak_r` 0.997, `rescued`. An edge pin at 0.47 became an interior lock at 1.00
by moving the wall — the cleanest available demonstration that the wall was the constraint.

**No alive shoulder was disturbed.** Every g0–g7 shoulder (15 of them, all `peak_r ≥ 0.5`,
−7.1…−48.7 ms) is identical across the two arms on every compared field. Zero losses, zero drift.
The 22 new alives are exactly the previously censored cluster.

**This confirms a standing prediction.**
[TEMP-pair50-anchor-seam-outcomes-2026-08-21.md](TEMP-pair50-anchor-seam-outcomes-2026-08-21.md)
Result 1 concluded, from listen-clip sweeps, that production's decline on this cluster had a single
sufficient cause — 2800 ms exceeds the ±2000 ms search radius — and that "if production searched to
+2.8 s the post-onset would lock." It does. That conclusion is now confirmed in production
coordinates on production's own B haystack, rather than inferred from truncated listen clips.

## Result 3 — the step is still *between* gaps, so there is still no boundary gap to classify

g0–g7 stay on the ~0 ms map (`not_consulted`, −7.1 to −48.7 ms). g8–g18 sit at ~+2800 ms. The
transition happens **in the media between g7 and g8**, not inside any gap:

- 50/g7 post: −48.7 ms. 50/g8 pre: +2767.1 ms.
- Intra-gap `splice.step_ms` across the cluster stays in the tens of milliseconds — g8 +38.5,
  g9 −0.02, g10 −43.8, g14 +42.1, g16 +8.4, g18 −6.1. Nothing near 2.8 s.

So the widen supplies the sequence test's **flanks** — ≥2 admitted *gaps* settled on each side of
the transition (canonical rule: [archive/TEMP-step-boundary-census-plan.md](archive/TEMP-step-boundary-census-plan.md)
§4; "settled" is a gap, not a shoulder), which the fingerprint did not show on the far side at
2000 ms — but not a straddling gap. The classifier stays parked for the reason already recorded:
no tree exercises the boundary rule. Pair 50 still has none; whether 31/g7 is the only within-gap
instance in the corpus is Result 1's unpaid question, not an answer from these two pairs.

**Do not read this against BACKLOG's "pair 50 has 6 gaps but no boundary gap among them."** That
sentence counts the **listen tree**, not these 20 fingerprint gaps. The *fact* — no boundary gap in
pair 50 — holds in both places; the number does not transfer.

## Result 4 — nothing moved downstream

Manifest `outcome` is identical gap-for-gap on both pairs between the two arms. Pair 50: g3, g5–g10,
g14–g16 `patch`; the rest `skip`. Pair 31: g4, g5, g7, g8, g10, g11 `patch`; the rest `skip`.

Notably **50/g11–g13 still skip `waveform_floor`** even though both their shoulders now lock at
+2761 with r ≈ 1.00. Envelope reach was never what that skip was about — consistent with the
pair-50 note's open Question 2, which remains open and is not advanced by this dump.

**Do not attribute the unchanged patches to a specific path from this dump.** The fingerprint's
`splice` block carries only `step_ms` / `pre_peak_r` / `post_peak_r` / `pre_peak_z` / `post_peak_z` /
`edge_pinned` — no `anchor_seam_used`, no `dual_fit_used`, no field naming the selected candidate.
That is BACKLOG § *Donor registration leftovers* → *33/17 placement-path investigation* ("which path
placed the fill is **unrecorded**"). The pair-50 note's Result 2 established the anchor-seam
attribution for this cluster from a **patch-report** baseline
(`gap-files/2026-08-13-repeat-terms/50.json`), not from a fingerprint dump; that is the source to
cite for it.

## Result 5 — two numbers that are not the pass bar

**The `Declined` mixture is real, and its exemplar is 50/g015 post.** At 2000 ms that shoulder reads
`envelope_peak_r` 0.720, **not** edge-pinned, `envelope_lag_ms` **0.0** → `Declined`: the guard
refused and "no shift was needed" are the same verdict, which is BACKLOG's *`env_frames == 0`
conflates two verdicts*. At 3000 ms it is `rescued` at +2809.6 / r 0.986. A separate leftover is
**50/g019 pre**: `envelope_peak_r` 0.993 at +1900 (2000 ms arm) → 0.998 at +1800 (3000 ms arm),
interior and not edge-pinned in both, still `Declined`, with the fine search unchanged at 581.9 /
r 0.235 / `decorrelated`. Same moral — a `Declined` count is a mixture, not a refusal rate — but it
is **not** the lag-0 case; do not cite it as the conflation exemplar. g019 pre is also the only
shoulder in either pair whose envelope fields moved without being rescued, and that is a grid
landing shift from the changed radius, not a decision change.

**The ladder's refutation was measured through the same cap it was judging.** The archived
adaptive-widen plan's deciding measurement 1 read pair 50's only edge pin as `r 0.47 at +2000` and
concluded the ladder had nothing to climb toward. That same shoulder is r ≈ 1.00 at +2800 (Result 2).
This does **not** revive the ladder — flat is cheaper, is the route BACKLOG chose, and this smoke
shows flat is sufficient — but "ladder empirically empty" is weaker than it reads and should not be
cited as evidence that widening finds nothing. What is settled is that the *ladder machinery* is
unnecessary, not that the reach beyond 2000 ms is empty. It is not empty.

---

## What this unlocks, and what it does not

**Unlocked.** The recipe is validated: a flat 3000 ms envelope reaches a 2.8 s step, leaves alive
shoulders untouched, and moves no outcomes. The corpus run can be paid for with confidence. Census
Result 8's "dump-visible base rate zero" is now **bounded**: on pair 50 the censoring it hypothesised
is demonstrated rather than inferred — the same gaps that read as noise at 2000 ms read as one clean
+2.8 s offset at 3000 ms. The zero is an upper bound on *that instrument at that radius*, not a
statement that steps are rare.

**Not unlocked.**

- **No base rate.** Two pairs out of 48. **Paid 2026-08-24** —
  [TEMP-envelope-3000-corpus-2026-08-24.md](TEMP-envelope-3000-corpus-2026-08-24.md) ran the same
  recipe over the whole corpus and the admitted sequence test is still 0 / 0 at the 50 ms floor,
  now with pair 50 admitted rather than censored. Neither dump needs re-running.
- **No skip verdict.** Result 3 — no gap straddles a step here, so nothing exercises the classifier's
  sequence test. The rule to apply when a tree exists is
  [archive/TEMP-step-boundary-census-plan.md](archive/TEMP-step-boundary-census-plan.md) §4–§5; do
  not re-derive it, and do not drop the sequence half.
- **No default flip.** 2000 stays the default. Nothing here argues about the 0.70 rescue guard
  either.
- **No `wide_envelope` unification.** The `--fingerprint-diagnostics` `wide_envelope` block is still
  pinned at `WIDE_ENV_MAX_LAG_MS = 400.0`, so a dump taken at a raised `--lag-envelope-max-lag-ms`
  carries a `wide_envelope` produced by a *different* search. BACKLOG § *Envelope width knobs move
  only the rescue path* owns that; it is unchanged and still undecided. Nothing in this note used
  `wide_envelope`.

---

## Reproduction

Source dumps are gitignored (`gap-files/`) and not guaranteed to survive cleanup — this doc is the
durable record.

- **The smoke:** `--gap-fingerprints` with `--features calibration,he-aac`, one pair at a time,
  `--lag-envelope-max-lag-ms 3000`, all other knobs at defaults, over
  `gap-files/envelope-3000-smoke.csv` → `gap-files/2026-08-23-envelope-3000/`. Memory-footprint
  constraints for corpus-scale runs: [gap-fingerprint.md](gap-fingerprint.md) § *Producing
  fingerprints* (bulk runs via `measure-gap-fingerprints.ps1`).
- **The diff:** compare `source.probe_recipe` and each gap's `lag_decision.pre_anchor[0]` /
  `post_anchor[0]` against `gap-files/2026-08-23-complete-corpus/{31,50}/corpus.json`. Fields
  compared: `frac_lag_ms`, `peak_r`, `envelope_rescue`, `envelope_lag_ms`, `envelope_peak_r`,
  `envelope_edge_pinned`, `border_displacement_ms`, `verdict`. Expected: **0** changed fields on
  pair 31 (24 shoulders), **135** on pair 50, all in g8–g18 plus g19 pre's two envelope fields.
- **Outcomes:** `manifest.json` `entries[].outcome` per pair; skip reasons are in each gap's
  `outcome.skip_reason` in `corpus.json`.
- **A caution about reading a corpus dump mid-run.** During this review `2026-08-23-complete-corpus/50/`
  was briefly observed empty and read as "pair 50 was never dumped" — it was being written at the
  time. Check the run has finished (or that a pair's `corpus.json` exists) before concluding a pair
  is absent; an in-progress corpus root also under-reports its gap total (1023 vs the final 1043).

## Non-goals

- Not a Change. No code moves for this note.
- Not a sizing result. Two pairs cannot produce a base rate; do not quote Result 2's 22 shoulders as
  a population.
- Not a licence to design the step-boundary skip verdict — Result 3 is why.
- Not an argument for raising the 2000 ms default, and not an argument about the 0.70 rescue guard
  (that is [TEMP-envelope-rescue-stats-plan.md](TEMP-envelope-rescue-stats-plan.md)'s territory; keep
  them separate).
- Do not read `envelope_rescue: declined` as a refusal rate anywhere in this dump — Result 5.
- Do not infer the patch path from a fingerprint dump — Result 4.
