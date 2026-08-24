# Backlog

Open follow-up work for `clip-sync`. See [PLAN.md](PLAN.md) for architecture, [docs/pipeline.md](docs/pipeline.md) for the repair pipeline (phase by phase), [docs/dev/corpus-validation.md](docs/dev/corpus-validation.md) for the test corpus, and [docs/error-mapping.md](docs/error-mapping.md) for error handling. Shipped work is recorded in `docs/dev/archive/*` and git history. Out of scope: [PLAN.md](PLAN.md) § Out of scope.

Last updated: 2026-08-23.

**How this doc works**

- **Open** — actionable items below (problem / direction kept for open work only).
- **Plans** — active drafts under `docs/dev/TEMP-*.md`; catalog: [docs/dev/README.md](docs/dev/README.md) § Plans. Archive when shipped.

**Next:** [Fill placement](#fill-placement-two-defects--registration-ignored-26g6-class-and-step-boundary-gap-31g7) — unpaid dump is a **flat** `--lag-envelope-max-lag-ms` re-fingerprint to size the step-boundary population (the adaptive ladder is **refuted**; plan archived 2026-08-23). That dump is **paid** — [TEMP-envelope-3000-corpus-2026-08-24.md](docs/dev/TEMP-envelope-3000-corpus-2026-08-24.md) (2026-08-24, 51 pairs / 1073 gaps): pair 50's radius censor is removed and the admitted sequence test is **still 0 / 0** at the 50 ms floor, so 31/g7 is the corpus's only within-gap boundary as a measured fact and item 3's wider-envelope branch is spent. Do not implement a skip verdict from current dumps, and do not re-run the widen looking for more straddlers. Rescue-branch envelope z/prominence is a separate diagnostic ([TEMP-envelope-rescue-stats-plan.md](docs/dev/TEMP-envelope-rescue-stats-plan.md)), not this dump. The step-boundary verdict (ex "collapsed-bridge skip") is **blocked on instrumentation**, not next: its old spec was refuted 2026-08-21. 26/g6 axis-2 start-sweep oracle (parallel, not this chain). [Donor registration leftovers](#donor-registration-leftovers); [Repair R6](#repair-r6-follow-ups); [Residual gate](#residual-gate-follow-ups).

---

## Open work

### Fill placement: two defects — registration ignored (26/g6-class) and step-boundary gap (31/g7)

Findings and closed sensor work: [TEMP-31g7-model-vs-findings.md](docs/dev/archive/TEMP-31g7-model-vs-findings.md) §8, [gap-vocabulary.md](docs/dev/gap-vocabulary.md) § *Known holes*, [archive/TEMP-fill-placement-registration-plan.md](docs/dev/archive/TEMP-fill-placement-registration-plan.md), map-coordinates and nominal-first (both archived). Recenter leftovers **closed 2026-08-20** as a no-op: 31/g12 post is a tail (`a_after = 0`); the 12-pair dump's empty shoulders are all start/tail geometry. Saturated-envelope unit test shipped. **S1 shipped 2026-08-20** — [archive/TEMP-registration-residual-plan.md](docs/dev/archive/TEMP-registration-residual-plan.md); real-media reproduction of 31/g7 / 26/g6's findings-§6.2 numbers via `registration_residual_ms` is still open, not blocking. Do not re-open S2b, the source/gate fork, or envelope-first ordering. Durable `lag_decision` behaviour: [gap-fingerprint.md](docs/dev/gap-fingerprint.md) § *`lag_decision`*.

**Re-aimed 2026-08-21.** The census ([TEMP-collapsed-bridge-census-2026-08-20.md](docs/dev/TEMP-collapsed-bridge-census-2026-08-20.md) Results 7–8) refuted this item's spec. `bridge_ms = gap_ms + step` where `step = post_lag − pre_lag`, so `|bridge_ms| ≈ 0` fires only when a pair's map step happens to equal minus that gap's duration (31/g7: 3.1 ms apart) — and **no positive step can ever satisfy it**, so pair 50's +2.789 s step is structurally invisible to it. The invariant is the **step boundary**: pre and post on opposite sides of a pair-level map step, sign-agnostic, no coincidence required. Result 8 then showed that boundary was **unmeasurable from `lag_decision` dumps at ±2000 ms** — 935 admitted rows, largest trustworthy step 63.9 ms, both known steps censored because a step past the search radius kills the correlation that would measure it. Dump-visible base rate **zero** on that instrument. **Sized 2026-08-24** — [TEMP-envelope-3000-corpus-2026-08-24.md](docs/dev/TEMP-envelope-3000-corpus-2026-08-24.md). The corpus-wide flat-3000 dump ran against a same-media 2000 ms arm one knob apart: pair 50's radius censor is removed (11 gaps dead → admitted, Arm A 938 → 949) and the admitted sequence test is **still 0 / 0** from the 50 ms floor up. The zero is therefore a measurement, not an instrument artifact. Two corrections it forces on the sentences above: pair 31's ~−1.15 s step was never radius-censored on the current media, and 31/g7's censor is the `SPLICE_MIN_PEAK_R = 0.85` admission floor — both its lags are right and both shoulders read `timing_offset`, they just miss the floor by 0.021 and 0.068 — so no search radius can ever admit it. Do not implement the old `|bridge_ms| ≈ 0` predicate.

| Gap | Open |
|-----|------|
| 31/g7 | **Step-boundary skip verdict** (was "collapsed-bridge"). Spec is **pre and post disagreeing by ≈ the pair's map step**, *not* `\|gap + post_lag − pre_lag\| ≈ 0` — see the re-aim above. Still: do not treat as one-seam fill; post at −1160 is in `lag_decision`. **Blocked on instrumentation**, not ready to design — do not implement from current dumps. Sizing is **paid** ([TEMP-envelope-3000-corpus-2026-08-24.md](docs/dev/TEMP-envelope-3000-corpus-2026-08-24.md), 2026-08-24): Result 8's zero survives the widen — pair 50 admitted, sequence test still 0 / 0 at the 50 ms floor — so 31/g7 is the corpus's only within-gap boundary, measured. Its own censor is the `SPLICE_MIN_PEAK_R = 0.85` admission floor (pre −38.07 / 0.8287, post −1160.25 / 0.7820, both lags right, both `timing_offset`), **not** search radius, and **not** the same mechanism as 31/g8 below. The WAV instrument now exists: the `shoulder-sweep` calibration bin, shipped 2026-08-21 ([shoulder-sweep.md](docs/dev/shoulder-sweep.md) is its contract; **it measures and records; it does not classify**). No listen tree on disk can exercise the boundary rule (pair 31 has 4 live gaps even with `--also`; pair 50 has 6 gaps but no boundary gap among them). Classifier parked until a tree exists with ≥5 live gaps in one pair, ≥2 settled on each side of a candidate; the method to apply then is census Result 8 ([archive/TEMP-step-boundary-census-plan.md](docs/dev/archive/TEMP-step-boundary-census-plan.md) §4–§5) — do not re-derive it, and do not drop the sequence test (census Question 12). The base rate is **measured and zero** ([TEMP-envelope-3000-corpus-2026-08-24.md](docs/dev/TEMP-envelope-3000-corpus-2026-08-24.md)): the flat-3000 route was the cheap one, it ran corpus-wide, and it found no second straddler. The widen supplies the sequence test's *flanks* on pair 50 but **no straddling gap** — that pair's step falls between g7 and g8 — so it does not unpark the classifier. Do not re-run it hoping for more. Note the "pair 50 has 6 gaps" count above is the **listen tree**, not the 20-gap fingerprint dump; the *no boundary gap* fact holds in both. Census: [TEMP-collapsed-bridge-census-2026-08-20.md](docs/dev/TEMP-collapsed-bridge-census-2026-08-20.md) (2 open follow-ups: Q7 census completeness, Q11 the 31/g10 audible blip); pair-50 spin-off: [TEMP-pair50-anchor-seam-outcomes-2026-08-21.md](docs/dev/TEMP-pair50-anchor-seam-outcomes-2026-08-21.md). |
| 31/g8 | **Narrow-capture pre miss.** `BorderCapture::Narrow` (shipped default) declines the pre at envelope +800 ms / r 0.816; Wide / envelope-first keep −1146.1 / 0.974 because the shared capture's 12% trim puts `D` at 1984 ms vs Narrow's 13 ms. WAV sweep on throat-adjacent clips still locks at −1146.1. Not a floor problem — left the widen plan 2026-08-22 ([archive/TEMP-envelope-adaptive-widen-plan.md](docs/dev/archive/TEMP-envelope-adaptive-widen-plan.md) deciding measurement 2). `--border-capture wide` is already a diagnostic arm; do not flip the default (it re-baselines `border_displacement_ms` corpus-wide). No TEMP yet. |
| 26/g6 | Oracle the axis-2 start sweep — what did the objective score at ≈−50 ms vs −1450 ms (findings §6.8). Do not retune `fill_fit_early_start_penalty_scale`. |

**Order of work** (do not raise the 0.70 envelope guard. **Keep the 2000 ms default; raise it when a decision depends on it, not before.** The envelope dump that phrasing was waiting on has now spoken — [TEMP-envelope-3000-corpus-2026-08-24.md](docs/dev/TEMP-envelope-3000-corpus-2026-08-24.md) — and its answer is *safe but unmotivated*: 3000 ms produces zero outcome changes across 1073 gaps, against a corpus-wide re-baseline of every `lag_decision` field. The ladder is separately **refuted** as of 2026-08-23):

1. **Adaptive widen + rescue-branch guard** — [archive/TEMP-envelope-adaptive-widen-plan.md](docs/dev/archive/TEMP-envelope-adaptive-widen-plan.md). ~~**Now next Change** (promoted 2026-08-21)~~ — **ARCHIVED 2026-08-23**; premise refuted, ladder empirically empty *at ±2000 ms* (measurement 1: pair 50's only edge-pin is g009 post, r 0.47 at +2000 ms — that shoulder locks at +2800 once the cap moves; do not cite measurement 1 as "nothing lives past 2000"). Do not start that Change. Survivor diagnostic: [TEMP-envelope-rescue-stats-plan.md](docs/dev/TEMP-envelope-rescue-stats-plan.md) (envelope z / prominence / curve length; do not retune 0.70 there).
2. **`shoulder-sweep` bin — SHIPPED 2026-08-21** ([shoulder-sweep.md](docs/dev/shoulder-sweep.md) is its contract; plan record archived 2026-08-22). Instrument, not a Change: no production behaviour moves. Makes the WAV sweep durable and CI-guarded (records `step_ms`; does not classify). Does **not** itself confirm the boundary model or size item 3.
3. **Step-boundary skip verdict** — was item 1; still **blocked**, and the wider-envelope branch of that blocker is now **spent**. [TEMP-envelope-3000-corpus-2026-08-24.md](docs/dev/TEMP-envelope-3000-corpus-2026-08-24.md) ran the flat-3000 dump corpus-wide: pair 50's step falls *between* g7 and g8 (flanks, no straddling gap), no second straddler exists anywhere in the corpus, and the admitted sequence test is 0 / 0 at the 50 ms floor. **n = 1 is now a measured property of this corpus**, not an artifact of not having looked, and that is a good reason the classifier stays parked. What remains is genuinely harder than running a wider dump: either design a skip from a single visible instance that the Arm A rule will not count (31/g7's shoulders are both correct and both under the 0.85 admission floor — `lag_decision` and `shoulder-sweep` already register it), or find a second straddler by some other means. Do not design it from the bin's row output, and do not re-derive the classifier from the corpus CSV (census Question 12).
4. **`lag_editorial` confirm** — piggyback on a diagnostics dump.
5. **`--lag-envelope-window-secs` — SHIPPED 2026-08-22.** Rescue path only, matching `--lag-envelope-max-lag-ms`; default stays `WIDE_ENV_WINDOW_SECS = 2.0` and is recipe-stamped like `border_capture`. The `wide_envelope` diagnostic remains pinned — that divergence is now filed as *Envelope width knobs move only the rescue path* below and is still undecided. Item 3's cheap route (a flat wider-envelope dump) is **run and spent** — smoke on pairs 31/50 2026-08-23 ([TEMP-envelope-3000-smoke-2026-08-23.md](docs/dev/TEMP-envelope-3000-smoke-2026-08-23.md)), corpus-wide 2026-08-24 ([TEMP-envelope-3000-corpus-2026-08-24.md](docs/dev/TEMP-envelope-3000-corpus-2026-08-24.md)). It behaves at scale: of 2020 populated shoulders, 24 differ from the 2000 ms arm — 22 are pair 50 g8–g18 going dead → rescued, and 2 are envelope-grid landings on shoulders that stay `declined` either way (49/g38 pre, 50/g19 pre). Zero rescues lost, zero alive→dead, zero outcome changes across 1073 gaps, and zero `envelope_edge_pinned` rows, so nothing in this corpus saturates 3000 ms. Neither dump needs re-running. Measured while building it, and the reason the shipped docs read the way they do: widening does **not** trade against shoulder length — the A template is clamped (`w = window.min(a_border.len())`) — it trades against **B lead-in**, since `envelope_shoulder` declines when `anchor_frame < w + max_lag`. A 4.0 s window declines at 4160 ms of lead-in and rescues at 6660 ms. Capture pads B by ~14 s at the defaults, so a flat widen is safe except for gaps near the start of B.

| Item | Direction |
|------|-----------|
| **Step-boundary skip verdict** | 31/g7 honest action. Spec: pre and post disagree by ≈ the pair's map step (sign-agnostic). The old `\|gap + post_lag − pre_lag\| ≈ 0` spec is **retired** — refuted as an arithmetic coincidence, see the re-aim above. Blocked, and the cheap route is spent: the flat `--lag-envelope-max-lag-ms` dump ran corpus-wide 2026-08-24 ([TEMP-envelope-3000-corpus-2026-08-24.md](docs/dev/TEMP-envelope-3000-corpus-2026-08-24.md)) and the base rate is **zero** — 31/g7 is the only within-gap boundary in the corpus, measured, and it sits under the 0.85 admission floor rather than beyond any search radius. The `shoulder-sweep` bin remains the WAV instrument (it does not classify). Do not implement from current dumps, do not re-derive the classifier (census Question 12), and do not re-run the widen |
| **`shoulder-sweep` bin** | **SHIPPED 2026-08-21** — `crates/clip-sync-repair/src/bin/shoulder_sweep.rs`; [shoulder-sweep.md](docs/dev/shoulder-sweep.md) is its contract (plan record: [archive/TEMP-shoulder-sweep-bin-plan.md](docs/dev/archive/TEMP-shoulder-sweep-bin-plan.md)). Promoted the gitignored Python WAV sweep into a `calibration` bin (the Python is deleted). **Measures and records; does not classify.** No listen tree on disk can exercise the boundary rule (pair 31: 4 live gaps; pair 50: no boundary gap). Classifier parked until a tree with ≥5 live gaps in one pair, ≥2 settled per side; method is census Result 8 ([archive/TEMP-step-boundary-census-plan.md](docs/dev/archive/TEMP-step-boundary-census-plan.md) §4–§5) — do not re-derive. **Cannot** produce a base rate (~38 listen gaps vs 971). Sizing was done by the flat `--lag-envelope-max-lag-ms` dump instead and is **closed** — [TEMP-envelope-3000-corpus-2026-08-24.md](docs/dev/TEMP-envelope-3000-corpus-2026-08-24.md), base rate zero |
| **Adaptive widen** | **ARCHIVED 2026-08-23** — [archive/TEMP-envelope-adaptive-widen-plan.md](docs/dev/archive/TEMP-envelope-adaptive-widen-plan.md). Ladder **refuted** (measurement 1: pair 50's only edge-pin is g009 post, r 0.47 at +2000 ms). Keep default 2000. Do not implement. **Caveat closed 2026-08-24** ([TEMP-envelope-3000-corpus-2026-08-24.md](docs/dev/TEMP-envelope-3000-corpus-2026-08-24.md)): measurement 1 was taken *at* the 2000 ms cap it was judging, so it could not speak to reach — that same g009 post shoulder reads r ≈ 1.00 at +2800. Corpus-wide, reach beyond 2000 ms is **not** empty (22 shoulders live at 2761–2816 ms) and nothing saturates 3000 ms (zero `envelope_edge_pinned` rows, down from one). What is settled is that the *ladder machinery* is unnecessary — flat is cheaper and sufficient — which is the archival verdict, unchanged. Do not cite measurement 1 as evidence about reach in either direction. Sizing: closed, base rate zero. Survivor diagnostic: [TEMP-envelope-rescue-stats-plan.md](docs/dev/TEMP-envelope-rescue-stats-plan.md) |
| **Envelope rescue stats** | [TEMP-envelope-rescue-stats-plan.md](docs/dev/TEMP-envelope-rescue-stats-plan.md) — stamp `envelope_peak_z` / `envelope_prominence` / `envelope_curve_n` on rescued/declined shoulders. Diagnostic first; do not retune 0.70 in that Change. 26/g6 is `NotConsulted`; 31/g8 is not a floor instance |
| **31/g8 Narrow-capture pre miss** | Shipped default `BorderCapture::Narrow` declines pre at envelope +800 / r 0.816; Wide keeps −1146.1 because 12% trim on the shared capture sets `D` to 1984 ms vs 13 ms. WAV still locks at −1146.1 on throat-adjacent audio. `--border-capture wide` is the diagnostic arm; do not make it the default. No TEMP yet; numbers: [archive/TEMP-envelope-adaptive-widen-plan.md](docs/dev/archive/TEMP-envelope-adaptive-widen-plan.md) deciding measurement 2, dump `gap-files/2026-08-22-31g8-capture` |
| **Envelope width knobs move only the rescue path** | `lag.rs`'s rescue branch reads `cfg.lag_envelope_max_lag_ms`, but `wide_envelope_at_placement` (`probes.rs`, the `--fingerprint-diagnostics` `wide_envelope` block) is pinned at `WIDE_ENV_MAX_LAG_MS = 400.0`. A dump taken at a raised `--lag-envelope-max-lag-ms` therefore carries a `wide_envelope` produced by a **different search**, and the two lags in one corpus are not comparable. Decide: unify both paths on the same cfg, or find a different answer (e.g. stamp the diagnostic's own reach so the split is at least legible). **Do not unify without recipe-stamping** — that would make `wide_envelope` non-comparable across runs, which is the provenance failure below in miniature |
| **`env_frames == 0` conflates two verdicts** | `rescue_shoulder` routes "the guard refused" and "no shift was needed" to the same `EnvelopeRescue::Declined`. 50/g015 post: `env_r` 0.720, **not** edge-pinned, `envelope_lag_ms` 0.0 → `Declined`. So a corpus's `Declined` count today is a mixture, not a refusal rate. Measurement 1 (2026-08-23) dodged this by filtering on `envelope_edge_pinned`, not on `Declined`. Split the verdict (or record the reason) before any `Declined` *rate* is read as evidence. That same 50/g015 post is `rescued` at +2809.6 / r 0.986 in the flat-3000 smoke and in the corpus run ([TEMP-envelope-3000-corpus-2026-08-24.md](docs/dev/TEMP-envelope-3000-corpus-2026-08-24.md)), so the cell is not inert — and note the *other* `Declined` flavour it is easily confused with: 50/g019 pre, envelope peak ≈ 1.0 at +1800, interior, still declined ([TEMP-envelope-3000-smoke-2026-08-23.md](docs/dev/TEMP-envelope-3000-smoke-2026-08-23.md) Result 5) |
| **`lag_editorial` shares `lag_at_placement`** | Piggyback on a `--fingerprint-diagnostics` dump. Confirm the diagnostic placement follows the same rescue ordering; do not treat disagreement with `lag_decision` as a gate signal |
| **S4 / `applied = 0` cohort** | Conditional ear labels for whether a residual guard's domain includes gaps that never slid. Do not run until it would change a decision. The archived §5 render list is mixed-orientation and stale |
| **Ear-pass 17/g34 and 47/g14** | Cheapest n>2. 17/g34 is the same well-posed class as 26/g6. Residual also groups it with 31/g7 — do not read that as a shared fingerprint. 47/g14 is the first failure-to-move. Labels, not a TEMP |
| **α / `border_standoff_secs`** | Fill sits ~25 ms late vs both shoulders. Hypothesis: production's 0.35 s standoff. Run existing arms through `scripts/measure/diff-patch-adjustments.ps1` or drop. Not a TEMP |
| **+39 ms fill-start detection bias** | Archived registration plan §2.1. Cosmetic for a discriminator; required before any absolute claim about delivered position |
| **Two gaps break the frame-672000 identity** | Archived registration plan §3.1, 589 and 4245 frames. Dump-only; needed before that join is used unconditionally |

### Repeat-at-seam prior is inert on every winner; one branch is unreachable

Durable reading: [gap-fill-modes.md](docs/gap-fill-modes.md) § Fit unified search and [json-output.md](docs/json-output.md) § `fit_terms` (oriented 2026-08-13 run; arm 3 unreachable; do not lower 0.55 — the live arm's conditions are anti-correlated). `anchor_penalty` is a configuration zero and needs no work.

| Item | Direction |
|------|-----------|
| **Delete arm 3** | Unreachable by construction (`REPEAT_SUM_CEILING = 2 × REPEAT_CORR_THRESHOLD`). Do not repair the ceiling |
| **Leave `REPEAT_CORR_THRESHOLD` (0.55)** | Lowering it would not have fired arm 1 on the 9 high-repeat winners. Split the constant before any retune — the same 0.55 is the short-bracket extension stop, a veto `fill_repeat_penalty_weight` does not disable |
| **Extension-loop instrumentation** | Optional. Record `prev_repeat` / `next_repeat` and stop reason (`score_ok` vs `repeat_ok` vs exhaustion). If the monotone clause always binds first, the threshold is free to move on the penalty side after the split |
| **Winner-only blindness** | `fit_terms` reports the winner, so a working penalty that demoted rivals is invisible. Needs per-candidate capture; **not proposed** |

### Fill normalize boosts against a non-local reference (ear-confirmed defect)

Durable reading: [json-output.md](docs/json-output.md) § FillLevelCheck and [pipeline.md](docs/pipeline.md) § Fill normalization (pair 19/g013, `normalize_gain = 3.88`, energy-pooled 5 s+5 s vs −52 dB local shoulders). Boost-only PR archived: [TEMP-fill-normalize-boost-only-plan.md](docs/dev/archive/TEMP-fill-normalize-boost-only-plan.md). Re-label `2026-08-08-edge-listen` (pairs 7/8/12 swapped) before quoting “lowest labelled clean” as a bound. Oriented roll-up: `gap-files/2026-08-13-repeat-terms/fill-level-rollup.csv`.

| Item | Direction |
|------|-----------|
| **Size the exposure before designing a fix** (do this first) | Do **not** design from n=1. `scripts/measure/measure-patch-outcomes.ps1` is the run (`--patch-only` → `fill-level-rollup.csv`) and needs **one column added** — it emits `PeakDeltaDb`/`EdgeDeltaDb` (`:170`, `:183`) but not `normalize_gain`. With that column: large gain **and** quiet immediate shoulders = the exposed set |
| Make the boost reference local | Candidate directions once the sweep lands: narrow the border window; take the quieter of wide-vs-local; or cap the gain by the local shoulder ratio. `normalize_window_secs` (default 5.0) is already a knob |
| Consider defaulting `normalize_fill` to false | Named because the evidence now points at it: **one** ear case where the boost causes an audible defect, **zero** ear cases where it demonstrably helps. Honest short-term answer if the sweep shows large boosts are common |
| Re-run the fill-level roll-up after any gain change | Any change here moves `peak_delta_db` / `edge_delta_db`. **Already owed once:** the boost-only PR changed the gain on every previously-attenuated gap |
| Residual seam `g` (`a ≈ g·b`) as the production gain | Estimates level from the **seam** rather than whole-fill RMS. Needs verdict plumbing plus corpus rules for when the estimate is trustworthy |
| B-relative / donor-shoulder matching as a full replacement for A-border RMS | Sidesteps the A-reference question; corrects nothing when the two masters genuinely differ in level, which is the only case normalize exists for. Evaluate against the sweep |
| The attenuation half was never ear-validated | The two affected gaps sound clean *now*; nobody has heard them sound bad *before*. Cheap to close if it ever matters: restore the symmetric clamp, drop the `region.rs` assert, render to a `-before` dir, revert |

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
trigger — none is a known defect. The **two 2026-08-22 additions** below are a different
class: both have already cost time. Shipped behaviour: [gap-fingerprint.md](docs/dev/gap-fingerprint.md)
§ *Source identity & the corpus library* and § *`measurement`*.

| Item | Direction |
|------|-----------|
| I1-class bin-divergence warn in `equivalence-calibration` (optional; was out of that plan's DoD) | Emit lives: flag gaps where `a_gap_total_blocks × measurement.bin_ms` disagrees with the geometry span. The check is documented and the fields are on both verdicts, so this is only automating a query a human can already run. Trigger: a second bin-width divergence (I1 was found by reading source, which is what made the fields Derived) |
| Row-level "no provenance" flag on `GapRow` | Deferred: `check.rs`'s health Warn plus the census's `(absent)` bucket already make an unanswerable corpus say so, and the pattern to mirror is `registration_from_legacy_lag`. Trigger: a report that needs to **filter** rows on it — nothing does today |
| **`corpus.json` carries no build stamp** (added 2026-08-22) | ~10 lines. `source` identifies the *media*; nothing identifies the *binary*. The 2026-08-19/20 border-capture arms had to be told apart by reading `border_displacement_ms` (1983.8 vs 13.0) — inference, not provenance, and it only worked because the two arms happened to differ visibly. `source.probe_recipe.border_capture` (shipped 2026-08-22) closes one axis; every other build-time constant is still invisible. Trigger is met — this cost real time on 31/g8 |
| **A behaviour change rode inside a labelled refactor** (added 2026-08-22) | `33b60e10` ("Refactor gap fingerprint measure module by splitting into multiple files") silently narrowed the shared border capture that `7e689ec6` had widened two days earlier. `--border-capture` fixes the knob; the open exposure is that **any "unchanged since those dumps" claim inferred from commit messages is unsound across that window**. No code action: this is a standing note that the `git log` shortcut failed here once, so facts re-derived in that range want a measurement, not a reading |
| `bit_depth` string → `BitDepth` parser | Deferred: the forward pin (`bit_depth_tokens_are_pinned`) is what protects corpora already on disk; a parser is dead code until a consumer reads the token, and none does. `bit_depth` is stored-for-later by design |

### Fingerprint contract follow-ups (C3)

Leftovers from [archive/TEMP-fingerprint-field-clarity-plan.md](docs/dev/archive/TEMP-fingerprint-field-clarity-plan.md)
§ 2.8 (R1–R3 + C1 + C2 shipped; all eight groups carry a contract).
**Deferred, not refused.** Shipped behaviour: [gap-fingerprint.md](docs/dev/gap-fingerprint.md) § *`_contract`*.

| Item | Direction |
|------|-----------|
| Share the contract string table with harness `legend_text()` | Goal was "dump and analyzer legend cannot drift" (§ 2.5). Sharing couples a 22-char CLI label with ≤120-char prose. Trigger: an *observed* drift, not the possibility of one |
| `--fingerprint-contracts=once\|always\|off` write flag | Deferred to "if size becomes an issue". Trigger: a corpus where `_contract` repetition is a measured problem |
| **Contract `status.patched` on the production report** | Raised from the registration plan § 0 S5. Fields that misled: `align_adjustment_secs` (signed slide, **not** a lag), `placement_slide_frames` (unsigned, and **≠ `\|align_adjustment\| × 48000` on 82/227 gaps**), `fit_path` (names the **boundary grid**), `pre/post_correlation` (min-reduced). Two design questions first: (a) `Contracted<T>` needs a `T`; `GapPatchStatus::Patched` is an enum variant with inline fields, and `contract.rs` keeps reader metadata out of domain types (`patch_result.rs`). (b) A dump is one gap per file; a report is all gaps in one — per-gap stamping repeats ~90 KB, and a report-root `_contracts` map is what § 2.6 rejected for dumps. Whether that rejection survives a report-shaped file is the open question. `dual_fit_used` / `anchor_seam_used` already always-serialized (no contract needed for that foot-gun) |

### Donor registration leftovers

From the 2026-08 equivalence-band / donor-registration review. Donor Apply and fill-level
check shipped; these remain.

| Item | Direction |
|------|-----------|
| **`equivalence-calibration --replay` reads `GapScanJson`** | Today `--replay` only loads fingerprint `corpus.json` / `GapCorpus`; plain scan JSON already carries the same registration + envelope fields on every gap (`scripts/measure/scan-registration.ps1`). Teach the reader the scan shape so Apply flip/abstain counts come from the production classifier, not a hand reconstruction. Small reader change; no new measurement |
| **Conditional donor test — investigation only** | Ask “is B non-silent *where A is silent*?” (at the registered lag) instead of independent A-floor + donor-occupancy halves. **Do not change the gate yet.** First: count A-silent∩donor-silent coincidence on existing scan JSON. Fill-level catches *loudness* damage only — see [Fill placement](#fill-placement-two-defects--registration-ignored-26g6-class-and-step-boundary-gap-31g7). No TEMP until the count says it is worth designing |
| **33/17 placement-path investigation** | Which path placed 33/17’s fill is **unrecorded**. The dump’s `brackets` array is the oracle enumeration, not the candidate production selected. **Next:** instrument the selected candidate / fit path; likely site if anything is tuned is the **acceptance floor**, not the comparator |

### Dual-fit confidence axis

**Fingerprint / analysis only** — not a production dual-fit scope change. Do not wire
into `try_dual_fit` / rescue gating. Census and “do not tighten `gate_pass`”:
[gap-fingerprint.md](docs/dev/gap-fingerprint.md) § *`splice_dualfit`*.

| Item | Direction |
|------|-----------|
| **Add fingerprint `dualfit_confident`** | Leave `gate_pass` alone (goldens and corpus history read it). Derive from min-z + min-prominence (`pre/post_seam_z` p05 3.14 / p25 4.91; `pre/post_seam_prom` p25 0.123). **Do not** consolidate end-search length into dual-fit: throat cohort n=65, `corr(fill−span, bridge−gap)` = +0.06; in the z ≥ 6 cohort, 8 of 19 gaps show a 0 ms end excursion while dual-fit trims 4–28 ms |

### Repair R6 follow-ups

From [archive/repair-write-path-plan.md](docs/dev/archive/repair-write-path-plan.md) post-ship gaps.

| Item | Direction |
|------|-----------|
| `--dry-run` / `--write` | Explicit CLI flags; today write mode is implied by `--wav` / `--mux` or TOML `dry_run = false`. **Still open, and now bigger than it was:** `--patch-only` (shipped) added a fourth run mode — patch, no sink — so the mode is no longer a `dry_run` boolean at all. Four modes (scan-only / `--repair-preview` / `--patch-only` / write) are selected today by three flags plus a TOML boolean, with the mutual exclusions enforced pairwise in `RepairConfig::validate`. If this is picked up, do it as one coherent mode selector rather than bolting `--dry-run` / `--write` onto the side — note `--patch-only` deliberately *keeps* `dry_run = true` (it writes nothing), which an explicit `--write` flag would make incoherent |
| Scratch-buffer regression test | Dedicated unit test for patch PCM path |
| Streaming / chunked WAV encode | Large multi-gap fills without holding full PCM |
| Adaptive gap-signature context (low priority) | Widen `gap_signature_context_secs` per-gap only when the score at the nominal map is below floor, instead of decoding wide B context for every gap. From [energy-signature-plan.md](docs/dev/archive/energy-signature-plan.md) Phase 4; low value since mode-coupled `nominal_bias` already handles drift at the 3 s default |
| Dual-fit oracle: unpin pre-shoulder lag from 0 (optional hardening) | `validate_dual_fit_oracle.rs` (gated `validation-tests`, needs ffmpeg + fetched corpus) only steps the **post**-shoulder (`step_ms`); the pre-shoulder is always sourced from B at lag 0 by construction (`dual_fit_oracle.rs`). The 2026-07-03/05 production bug is now covered end-to-end by a synthetic unit test (`dual_fit_result_passes_the_production_revalidation_gate`), so this isn't blocking. Direction if picked up: optional `pre_step_ms` on `DualFitOracleCase` plus a manifest case with both nonzero |

### Residual gate follow-ups

From [archive/residual-gate-findings.md](docs/dev/archive/residual-gate-findings.md) and
[archive/residual-gate-wiring-plan.md](docs/dev/archive/residual-gate-wiring-plan.md). Test inventory:
[`residual_gate_catalog/`](crates/clip-sync-repair/tests/residual_gate_catalog/).
**Not planned:** `veto_rescue` as default (G5: synthetic-only); F4 pipeline veto (M6).

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
| **Informative floor + NaN headroom** | low | `informative: true` with non-finite `worst_headroom_db` does **not** abstain. **Direction:** leave as-is unless a census shows the cell is common and harmful; if tightening, extend `gate_abstains` to require finite headroom — count first |

### Defer / opportunistic

| Item | Direction |
|------|-----------|
| Offset-mapped end placement | After start clip, place B end window at `A_end + Δ` when B has a long leader — [archive/anchored-end-extraction-plan.md](docs/dev/archive/anchored-end-extraction-plan.md) |
| Skip overlapping end fingerprint | Omit end clip when `T_anchor − L` overlaps start window (`domain/policies.rs`, `application/align_videos.rs`) |
| Weighted drift in repair warning | Down-rank end clip in instability synthesis when end confidence is low (`clip-sync-repair/src/infrastructure/cli/output.rs`) |
| Memory / PCM cloning | `Cow` / in-place prep when painful; parallel A/B decode when needed. **Decided (2026-06-11):** future streaming should reuse `scan_*_buckets` callbacks; `MediaSession: Send` allows one session per thread — [PLAN.md](PLAN.md) § Media session semantics, [archive/media-session-redesign-plan.md](docs/dev/archive/media-session-redesign-plan.md) |
| Committed test fixtures | Optional committed MP3; committed verify deferred — [tests/corpus/README.md](tests/corpus/README.md) |
| Verification cost knob | `validation.max_verification_secs` — only on demonstrated friction ([archive/verification-hardening-plan.md](docs/dev/archive/verification-hardening-plan.md)) |
| Test tier follow-ups | [docs/dev/test-tier-remainder.md](docs/dev/test-tier-remainder.md) |
