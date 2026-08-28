# TEMP — Local paired-shoulder fill gain (replace `compute_fill_gain`)

**Status:** draft, 2026-08-26. **Startable** as a **separate Change** from
[TEMP-program-gain-plan.md](archive/TEMP-program-gain-plan.md) — do not bundle. The
measurement it reuses (`paired_level_offset`) and the program prior
(`program_levels.offset`) already exist on the scan report. Reviewed against
the landed program-gain tree the same day: § 6 (scan threading + helper), § 7
(`stack_fill_gain`), § 8 (apply signature), § 9 (`normalize_window_secs`) are
the corrections; nothing in the estimator itself moved.

**Revised 2026-08-27** after a second review against the tree. Five corrections:
§ 3 / § 6 / § 8 (there is **no report-reload path** — the "old report" case does
not exist), § 4 (the residual gets its **own** bound, not `max_fill_gain_db`,
plus § 4a's calibration pass for the small-*n* problem), § 6 (steadiness must
read the **full** A timeline, not a sliced collar), § 6 / § 8
(`channels_comparable` hoisted onto `ProgramLevels` instead of inferred at
apply), § 11 (the doc surface is wider than the gap table). The review's one open
question — the map-straddle exposure — was decided the same day: **§ 3a, per-side
medians as a detector, no gate.**

**Tracked as:** [BACKLOG.md](../../BACKLOG.md) § *Fill normalize boosts against a
non-local reference* → *Local paired-shoulder gain*.

**Companions:** program scalar
[TEMP-program-gain-plan.md](archive/TEMP-program-gain-plan.md) § 5 (stacking this
discharges); capture
[archive/TEMP-ab-program-levels-plan.md](archive/TEMP-ab-program-levels-plan.md);
local-reference defect
[TEMP-fill-normalize-local-reference-2026-08-25.md](archive/TEMP-fill-normalize-local-reference-2026-08-25.md);
[settled.md](settled.md) § A **B-relative / donor-shoulder fill gain** (refuted —
this plan is **not** that idea, see § 2); latched attenuation-half of *RMS* local
normalize (§ C — also not this idea, see § 5).

**Media hygiene:** pair index only; no filenames / titles / paths in the repo.

---

## 1. Goal / non-goals

**Goal**

When `normalize_fill` is on, stop matching whole-fill RMS to a 5 s + 5 s A-border
energy pool. Instead, at each gap, run the **same paired B−A block difference**
the program scalar uses, restricted to **collar blocks around the hole**, and
apply only the **residual against the whole-program offset**. That is the
hole-local master relationship, informed by the file average, not a second copy
of it.

**Non-goals (v1)**

- Bundling with the program-gain Change (apply site overlap; two knobs vs this
  estimator).
- Flipping the `normalize_fill` default. Program-gain defaults it **off**; this
  Change only replaces the estimator behind the flag. Whether default-on becomes
  safe again is a later decision, after this estimator has a roll-up.
- Keeping `BlockLevel` timelines on `GapReport`.
- Recomputing the collar from patch PCM / refined edges.
- Matching the fill to **B’s gap shoulders** (refuted — § 2).
- Pairing **inside** the gap core (A is inactive; those blocks never pair).
- Shrinkage / James–Stein blend toward the prior when `n_pairs` is tiny — v1
  abstains below a floor (§ 4). Revisit if the floor is hit often.
- Per-side “quieter of pre/post”, or any per-side *selection*. One median over
  the pooled collar decides. (Per-side medians are still **emitted** as a
  straddle detector — § 3a. Reporting them is not selecting on them.)
- ITU / LUFS, peak limiting.
- Narrowing `normalize_window_secs` or otherwise patching `compute_a_border_rms`.
  That function loses its **fill-gain** caller. The field stays the A-border
  template / correlate window (§ 9) — do not treat it as unused.

---

## 2. Why this is not the refuted idea

| | Refuted (settled § A) | This plan |
|---|---|---|
| Reference | B’s **shoulders at the hole**, A ignored | Paired **B minus A** on the same mapped collar blocks |
| What it does | Reproduces the level the fill already has (the fill *is* B) | Reads the local A↔B master relationship, then scales the fill by the inverse residual |
| Fill RMS | unused | **unused here too** — that is the point |
| Same-master, quiet passage | leaves 19/g13’s over-boost untouched | A quiet vs B quiet → `d_local ≈ offset_program ≈ 0` → residual 0 |

The fill is never the reference. The collar is undamaged program on **both**
sides, the same predicate as the whole-file reduce. B’s gap interior is not
read.

---

## 3. What “relevant blocks” are

`paired_level_offset` requires A **active** and **steady**. The gap core fails
both: A is inactive, and the first active block that *abuts* the silent core
fails steadiness (`|Δrms|` ≫ `STEADY_TOLERANCE_DB`). That walk-off is wanted
(same job as `border_standoff`). Do not special-case it.

**Collar (A timeline), default `local_pair_collar_secs = 2.0`:**

```text
pre  = [gap.start − W, gap.start)
post = (gap.end, gap.end + W]
```

Use scan `Gap.video_a_start_secs` / `video_a_end_secs`, not patch-refined
edges (100 ms blocks make tens-of-ms refine noise). A block is in the collar
if it **overlaps** pre or post and does **not** overlap `[gap.start, gap.end]`.

Then the existing pairing steps (active / steady / map / B prefix / B active /
`d_i = rms_B − rms_A`) run **only** on those A blocks. Reuse the loop; do not
copy it. File-edge and one-sided collars are valid — an empty pre is not an
error.

`W = 2.0` matches the registration envelope context, not the 5 s normalize
window. The 5 s pool was the defect; this is a different estimator, not a
shorter RMS. Config-only; no CLI in v1.

Stamping is **scan-time**, so `ScanGapsRequest` must carry `local_pair_collar_secs`
(`composition.rs` today passes no gain knob into scan). The reason is not
`--patch-only`: that flag re-runs the scan in-process (`composition.rs:348`
takes the write arm with every sink absent — there is no report-reload path,
see § 6), so one `RepairConfig` field reaches both halves either way. The reason
is that the **block timelines exist only inside `ScanGaps`**;
`PatchRequestSettings` never sees them, so a width that arrives at patch time
has nothing to measure. Patch never recomputes the collar.

---

## 3a. Per-side medians — the map-straddle detector

**Decided 2026-08-27: emit them. Detector only; the decision stays pooled.**

> **REFUTED as a detector by the 2026-08-27 census — they stay, as provenance.**
> Over 920 measured collars (51 pairs), side disagreement is a pure function of
> `spread_db` and of nothing else: banded by spread, `|pre − post|` medians run
> 0.15 / 0.74 / 1.16 / 3.67 dB across the < 0.5, 0.5–1, 1–2, > 2 dB bands, and
> over well-populated collars **zero of 76** under 0.5 dB spread disagreed by
> more than 1 dB against 12 of 14 above 2.0 dB. There is no dependence on gap
> length at all. The corpus's one real straddle, 31/g7, reads 2.61 dB at n = 25
> — mid-pack among the noise, because at that spread the per-side uncertainty is
> the size of the step being hunted. Detecting a map step in the level domain
> needs quieter shoulders than real holes have. The § 4a census reports the
> banded profile instead of a flag count; a *flat* profile across those bands
> would reopen this. Nothing below changes except the claim that the statistic
> is diagnostic of registration — the ROOT registration problem was real, and
> § 3b is what came of it.

Collar pairing maps A→B through the single global `offset_secs`, exactly as the
whole-file reduce does. That is safe over ~500 pairs — a wrong-material region
becomes outliers the median discards and `spread_db` records. It is **not**
obviously safe over ~10–40: a gap sitting on a **map step** puts pre and post on
different lags, and one side of the pooled median is then differencing A against
the wrong B material with nothing to out-vote it.

**The corpus base rate is one gap.** 31/g7 (straddling a −1.15 s step) is the
only within-gap boundary measured, and the sequence test reads 0 / 0 elsewhere —
[settled.md](settled.md) § D. That is what makes this a detector and not a gate:
a refusal rule calibrated against n = 1 is not calibrated. It also means the
detector has a **known expected count** for § 4a's census — roughly one — so a
census that flags many collars is saying the collar is misregistering for some
other reason, which is the more valuable reading.

The two are the same statistic, which is the other reason to emit it: § D
specifies a step boundary as *"pre and post disagreeing by ≈ the pair's map
step, sign-agnostic"*. `pre_offset_db` / `post_offset_db` are that disagreement
in the level domain, computed per gap, for free, on every scan.

Note this is **not** the § 1 non-goal "per-side *quieter of* pre/post" — that was
a *selection* rule on level. This is about whether the two sides are measuring
the same thing at all. And 31/g7 is already the gap the ear-labelled n = 5 set
flags as confounded, so the detector's first output is checkable against a
verdict that exists.

**What ships:** `CollarLevelOffset` carries `pre_offset_db` / `post_offset_db`
(§ 6) — the median of each side's pairs, `None` on a one-sided collar. The pair
vec is already binned by A-start for `n_pre` / `n_post`, so this is one extra
median per side and no second pass.

**What does not ship:** nothing gates on them in v1. `offset_db` stays the
pooled median and § 4's residual is computed from it. A disagreement threshold
would be a **third** uncalibrated constant on an estimator that already owes
§ 4a two, and the whole point of § 4a is that the dump comes before the
constants. Per-side medians make the straddle *visible* on the scan JSON — and,
because § 4a's pass reads every collar, countable.

**Reopen as a gate when** § 4a's census shows `|pre − post|` large *independently
of* `spread_db` — a flat profile across the spread bands — or shows it
correlated with the gaps the ear labels. The 2026-08-27 census showed neither:
the profile rises with spread and is flat in gap length. The rule to write if it
ever does reopen is a refusal (`refused_straddle`), never a per-side selection —
picking the "better" side is § 1's refuted *quieter-of* idea wearing a new hat.

Two alternatives were considered here. Refusing on disagreement now: no
calibration exists, see above — and the census has since shown there is nothing
to calibrate. Mapping the collar through a better offset than the global one:
deferred as "a different Change", and then the census made it the *necessary*
one — **that is § 3b**, which took the per-gap shoulder registration rather than
the per-clip interpolation floated here. It does change the collar's map
relative to the whole-file offset the residual is taken against; § 3b argues
that is the point, and `map_lag_ms` keeps the two maps distinguishable.

---

## 3b. The map each collar is read through

**Decided 2026-08-27, and shipped in the measurement half: map each gap at its
own registered shoulder lag, not at the whole-program `offset_secs`.**

The census found the real defect behind the thin, noisy collars. The estimator
mapped A→B through the single global alignment offset — one scalar for the whole
programme. Joining `donor_registration.lag_ms` (the per-gap shoulder-envelope
fit, already computed on every scan, 100 ms block granularity) across all 51
pairs shows that map sits at the shoulder-correlation peak on only **116 of 273
repairable dropouts (42%)**, and is off by **≥ 100 ms on 59%**, out to 400 ms.

That is fatal at collar scale and harmless at programme scale, which is why it
survived this long: the whole-file reduce pools ~500 pairs and a wrong-material
region becomes outliers the median discards, while a collar of ~10 has nothing
to out-vote them with. Conditioned on the fitted lag, at n ≥ 8:

| fitted lag | median `spread_db` | median `\|residual\|` |
|---|---|---|
| 0 ms | 0.25 | 0.08 |
| 100–200 ms | 0.89 | 0.22 |
| ≥ 200 ms | 1.40 | 0.42 |

Every quantity § 4a wants to set a constant from degrades monotonically with
misregistration. Choosing `MIN_LOCAL_PAIRS`, `local_pair_collar_secs` or
`MAX_LOCAL_RESIDUAL_DB` off the nominal-map census would be calibrating against
the defect — the `max_fill_gain_db` precedent.

**What ships.** `collar_level_offset` takes a `map_lag_secs` and records it as
`CollarLevelOffset::map_lag_ms`; `scan_gaps` supplies
`DonorRegistration::trusted_lag_secs`, which believes a fit at or above
`min_envelope_r` (0.70, unchanged) with at least `MIN_REGISTRATION_BINS` behind
it, and `None` otherwise. `peak_r` is correlated over the **shoulders only** —
exactly the material a collar reads — so it is the right confidence for this
consumer.

**Not behind a flag, and not conditioned on `--no-apply-donor-registration`.**
That flag decides whether a registration may move a *verdict*. This is a
measurement with no verdict to protect, choosing between the right map and one
known to be wrong; `Observe` does not mean "measure through the wrong map".
`map_lag_ms` records which map produced each number, so a nominal reading and a
corrected one are never confused in a dump — `Some(0.0)` (believed, said zero)
and `None` (nothing to believe) are deliberately different.

**Open, and named.** The fit is quantized to 100 ms blocks, so ±50 ms of
residual misregistration survives by construction. A single core-fitted lag may
not describe both shoulders (see `seam-splice-dualfit`). And the direction of
causation between lag and spread is suggestive, not proven — the decisive test
is the re-census: the corrected cohort should converge on what the lag = 0
cohort already reads (spread 0.25 / |resid| 0.08), and if it does not, the
residuals are real material.

**Answered 2026-08-27 by arm 3 — § 4b Result 3.** It converges: matched over the
gaps whose map moved, spread ×0.293 and |residual| ×0.403, with every lag band
improving and the worst-misregistered band improving most (0.605 → 0.129 median
|residual| at ≥ 300 ms). The causation is settled in the direction assumed here.
The two caveats above are **not** discharged, and neither one explains the ~1.3×
spread / ~2× residual that remain against the lag = 0 cohort: the leftover peaks
at 200 ms rather than staying flat (against quantization) and side disagreement
is *lowest* at the largest lags (against one-lag-two-shoulders). The live
hypothesis is selection — that a large lag clears the trust floor only on strong
shoulder correlation — which needs `peak_r` joined per gap to test.

---

## 4. Residual, prior, and who applies

Scan convention stays **B minus A**.

```text
d_local        = median(d_i) over collar pairs          # None only if NOTHING paired
apply_local    = d_local is not None and n_pairs >= MIN_LOCAL_PAIRS
offset_prior   = offset_db if g_program was Applied, else 0
residual_db    = d_local − offset_prior
g_local_db     = −residual_db
g_local        = 10^(g_local_db / 20)
```

**Why the prior is the *applied* program offset, not the raw measurement.** If
program gain was refused (spread, `|offset| > 6`, no pairing, `--no-program-gain`),
there is no trusted average to subtract. Treating the prior as 0 makes local
`−d_local` — a hole-local master match on its own. Subtracting a *refused*
`offset_db` would bake the phantom (e.g. the −8.4 dB compressed-B reading in the
program plan's § 4b) into every collar.

**Identity when both apply.** `g_program_db + g_local_db = −d_local`. The file
average cancels; the hole’s own paired offset is what lands on the fill. Program
gain is then the **fallback** for collars that abstain, not a second copy of the
same number.

| Collar | Program | `g_total_db` |
|---|---|---|
| enough pairs | Applied | `−d_local` |
| enough pairs | Refused / off | `−d_local` (prior 0) |
| abstain | Applied | `−offset_db` |
| abstain | Refused / off | 0 |

**`MIN_LOCAL_PAIRS = 8` — provisional, see § 4a.** Whole-file `MIN_PAIRS` is 50;
a 2 s + 2 s collar at 100 ms is 40 blocks *before* active/steady/B filters. Eight
is 0.8 s of accepted pairs — enough for a median to mean something, low enough
that a one-sided file-edge hole can still speak. Below it: local abstains
(`g_local = 1`). No shrinkage in v1.

**The floor is checked here, at apply — never inside the estimator.** *(Decided
2026-08-27, during the measurement half.)* `collar_level_offset` reports a
collar of three pairs as readily as one of thirty, and returns `None` only when
nothing paired at all. A floor inside the estimator would censor the dump at
exactly the value § 4a exists to choose: the `n_pairs` distribution would be
truncated at 8, the census could confirm the guess but never see the shape below
it, and 8 would become another `max_fill_gain_db` — a rail that survives two
policy changes because the reading stops there (`settled.md` § E). Estimator
measures, apply decides.

That argument sets an order of magnitude, not a number, and it is the weakest
load-bearing claim in this plan. **Program gain's "no deadband" reasoning does
not transfer.** That argument is `SE ≈ 1.25 × MAD × 1.48 / √n` ≈ 0.055 dB over
~500 pairs. At `n = 8` the same expression over corpus-typical material is
**0.5–1.5 dB** — the same order as the residuals this estimator exists to
correct (`pipeline.md` § Fill gain, 2026-08-27 `--normalize` arm: 111 of 308
gaps moved, median local residual **+1.91 dB**). A v1 that abstains at 8 can
therefore inject seam steps of roughly the size it removes, in the name of
removing them. `MIN_LOCAL_PAIRS`, `local_pair_collar_secs` and
`MAX_LOCAL_RESIDUAL_DB` are all three set by § 4a's dump, not by this paragraph.

**No local spread refusal in v1.** MAD over ~10 samples is not the same
statistic as MAD over ~500. The `n_pairs` floor at apply is the trust gate. Emit
`spread_db` on the collar object so a later Change can gate it.

**Refusal, not clamp, on a huge residual — against its own constant.**
`|residual_db| > MAX_LOCAL_RESIDUAL_DB` → `g_local = 1.0`. Same shape as
program-gain’s refuse-not-clamp: past the bound the collar is reaching the
wrong material, and applying the bound's worth of a number you disbelieve is
worse than applying none.

**Do not reuse `max_fill_gain_db` for this.** An earlier draft did. The 12.0 has
never been calibrated: it entered in `f6167fcf` (2026-06-08) with the original
`compute_fill_gain`, as a round-number `±12 dB` safety rail on an **RMS ratio**,
and the boost-only Change kept it only to avoid coupling two decisions
(`archive/TEMP-fill-normalize-boost-only-plan.md` § *Config*). It bounds a
different quantity in a different unit, and `program_gain.rs` says so at the
declaration of `DEFAULT_MAX_PROGRAM_GAIN_DB`: 6.0 is *"deliberately not
inherited from the 12 dB local `max_fill_gain_db`, which bounds a different
quantity with a different failure mode."* This plan makes the local quantity
**the same quantity** as the program one — a paired master difference, same
estimator, narrower window — so that sentence now cuts the other way: 12 has no
claim on it either. `max_fill_gain_db` stays where it is, meaning what it means,
and dies with `compute_fill_gain`.

**Interim value: `MAX_LOCAL_RESIDUAL_DB = 6.0`, pending § 4a.** Not inherited
from program gain — argued the same way. On a same-master pair the residual is
~0 by construction; corpus program offsets sit inside 0.26 dB; a collar reading
several dB against the file average means the collar reached other material
(map straddle, a neighbouring hole, B holding a different edit), which is a
*detection*, not a level to apply. A bound at 12 would essentially never fire
and would forfeit that. § 4a sets the shipping number from where real readings
stop.

**Attenuation is allowed.** A constant scalar from paired shoulders does not
crush intra-fill dynamics (24/g46). That is a different mechanism from the
latched RMS-ratio attenuation (settled § C). Drop the `debug_assert!(gain >= 1.0)`
on the **local** factor; keep it on nothing that this estimator produces. The
product `g_total` may be below 1.

**Boost-only `compute_fill_gain` is no longer on the production path** when this
ships. Leave the function for now (tests / history); do not call it from
`region`. Replace the RMS half of `domain::stack_fill_gain` — do not compose
with it. That helper scales fill RMS into `compute_fill_gain` so the *old*
estimator would not square the program offset; this estimator’s double-count
guard is the residual formula, and feeding it a scaled RMS would be a third
path. `debug_assert!(stacked.local >= 1.0)` in `region/characterize.rs` goes
with it.

---

## 4a. The calibration pass (measure before the apply half is trusted)

Three constants above are arguments, not measurements: `MIN_LOCAL_PAIRS`,
`local_pair_collar_secs`, `MAX_LOCAL_RESIDUAL_DB`. This is the same order of
operations `max_program_gain_db` got, and the same order `max_fill_gain_db`
never got — which is how it survived two policy changes at a number nobody
chose.

**The pass is free.** Stamping is unconditional at scan (§ 6), `normalize_fill`
defaults **off**, and this Change does not flip it. So the measurement half can
land, run over the 51-pair corpus with default flags, and be read *before*
anything applies a local gain. No oracle cost, no listening cohort, no
`--gap-fingerprints` — a normal scan already writes `collar_level` on every gap.

**What the dump must answer, over the corpus:**

| Question | Read it off | Why it decides a constant |
|---|---|---|
| How often does the collar exist at all? | `collar_level` present / total gaps | An estimator that abstains on most holes does not exist; program gain silently carries and this Change bought nothing |
| What does `n_pairs` actually look like? | its distribution, vs the 40-block ceiling — **uncensored**, since the estimator applies no floor | Sets `MIN_LOCAL_PAIRS` and `local_pair_collar_secs` from the yield curve, not from "0.8 s sounds like enough". Score each candidate floor by what it would cost |
| When a collar is thin, *which* filter spent it? | `n_steady / n_active` vs `n_pairs / n_steady` | Decides between the two remedies: steadiness loss says widen `local_pair_collar_secs`, B-coverage loss says a wider collar will not help and the floor must carry it |
| How noisy is one collar? | `spread_db`, as `1.25 × spread × 1.48 / √n_pairs` | The per-collar SE. If it lands near the +1.91 dB residuals being corrected, the floor is too low — raise it, widen `W`, or add an SE-based abstention |
| Where do real residuals stop? | `d_local − offset_db` distribution | Sets `MAX_LOCAL_RESIDUAL_DB` from where readings stop, not from a rail |
| How often do the sides disagree? | `\|pre_offset_db − post_offset_db\|` banded by `spread_db` (§ 3a) | **Answered, refuting § 3a's premise:** it tracks spread and nothing else, so it never becomes a gate. A flat profile across the bands would reopen it |
| Which map was each collar read through? | `map_lag_ms` present / zero / non-zero (§ 3b) | The corrected and already-registered cohorts must converge; if they do not, the residuals are real material and not misregistration |

**It needs a script, and there isn't one.** `scripts/measure/census-program-levels.ps1`
is per-**pair** (one row per `scan.program_levels`); every question above is
per-**gap**. Write a sibling — `census-collar-levels.ps1`, same dump-only shape,
same `<DumpDir>/<pair>.json` layout, one row per gap with `n_pairs`,
`n_active`/`n_steady`, `n_pre`/`n_post`, `offset_db`, `pre`/`post`, `spread_db`,
the derived SE, and the residual against that pair's
`program_levels.offset.offset_db`. It must also score each candidate pair floor
by what that floor would cost, since choosing one is the point. Reuse the
existing script's state vocabulary (`stale` / `unaligned` / …) rather than
inventing a second one. This is part of the measurement half, not a follow-up.

**Gate on it.** Do not enable a local gain on real media — and do not quote a
cohort table (§ 10) — until this dump exists and the three constants are stated
against it. If the SE column says the estimator is noisier than the thing it
corrects, the answer is a wider collar or a higher floor, not shipping and
listening.

---

## 4b. Census results — what the arms have answered so far

Corpus: 51 pairs, 1073 gaps (`gap-files/extended-pairs.csv`). Pair **indices only**; the dumps live
under `gap-files/`, which is gitignored and ephemeral — **these numbers are the durable record, not
the CSVs**.

| Arm | `local_pair_collar_secs` | Map | Dump dir | Status |
|---|---|---|---|---|
| 1 | 2.0 | nominal (global `offset_secs`) | `2026-08-27-collar-census` | complete |
| 2 | 4.0 | nominal | `2026-08-27-collar-census-4s` | complete |
| 3 | 2.0 | **corrected** (§ 3b, per-gap `map_lag_ms`) | `2026-08-27-collar-census-lag-work` | complete |
| 4 | 4.0 | corrected | `2026-08-28-collar-census-4s-lag` | in flight; the shipping configuration |

Arms 1 and 2 share an identical gap list (1073 in both, none measured only at 2.0), so they compare
**matched per gap**. Read matched ratios, never the ratio of aggregate medians: arm 2 measures 86
gaps arm 1 abstained on, and those are the thin ones, so they drag every aggregate. The aggregate SE
median moves 0.329 → 0.310 and looks flat; the matched median ratio is 0.74×. The aggregate is the
artefact.

### Result 1 — W = 4.0 dominates W = 2.0. **Final; map-independent.**

| Matched median (920 gaps in both) | W = 2.0 | W = 4.0 | per-gap ratio |
|---|---|---|---|
| `n_pairs` | 8 | 20 | **×2.08** |
| `n_active` | 30 | 60 | ×2.00 |
| `n_steady` | 9 | 21 | ×2.33 |
| `spread_db` | — | — | **×1.00** |
| `se_db` | — | — | **×0.74** (improved on 609/815, 74.7%) |

Pairs ×2.08 ⇒ √n = 1.44 ⇒ SE ×0.69 predicted, ×0.74 observed. Spread is **unchanged**, so the wider
window reaches material just as similar to B — count is bought without buying noise. Abstentions fall
153 → 67; 86 gaps gain a collar and **none loses one**.

Cohort clearing `n_pairs ≥ 8` **and** SE ≤ 1 dB: **450 (41.9%) → 646 (60.2%)** of all gaps. At
`n_pairs ≥ 16`: 256 (23.9%) → 516 (48.1%). At `≥ 4`: 563 (52.5%) → 724 (67.5%). *(Reporting cuts, not
chosen constants — see the standing caution below.)*

The cost is negligible: 161 of 815 gaps get a worse SE (median 1.30×, and they are the thin ones —
median W = 2.0 `n_pairs` of 7 against 10 for those that improved), and only **3 gaps** cross from
clearing both cuts to not clearing them.

### Result 2 — steadiness does not see the window, and its bin-dependence is **unmeasured**

`n_steady / n_active` is 0.342 → 0.333 across the doubling: flat. That is not a corpus quirk but
structural — `is_steady` resolves `levels[i±1]` out of the **full** `a_levels` slice, and `W` enters
`collect_pairs` only through the `emit` range, selecting which `i` are examined and never what the
predicate returns for one. Widening therefore buys *absolute count*, not a better pass rate. § 4a's
"steadiness loss says widen `local_pair_collar_secs`" is confirmed in effect, by that mechanism.

What is **not** established: that steadiness depends on `scan_block_ms`. Every measurement here is at
100 ms; the bin has never been varied. The sign is not even predictable a priori — a longer block puts
more time between the neighbours being differenced (fewer steady) but averages more samples into each
RMS (more steady), and which dominates at 100 ms on this material is exactly what is unmeasured. Do
not assert a direction without an arm.

If the two thirds of active blocks that steadiness spends is worth attacking, **`steady_tolerance_db`
(3.0) is the clean lever and `scan_block_ms` is not.** The tolerance is a bare constant in
`program_levels`, touching only this module — its one coupling is that `collect_pairs` serves the
whole-program estimator too, so loosening it also moves the offset the residual is measured *against*.
`scan_block_ms` is a **detection** knob: it changes which gaps are found (no matched comparison
possible), is constrained to divisors of `silence_hold_ms`, bins the whole equivalence overlay (I1,
2026-07-30), and its 100 ms value is the validated sensitive-recipe default from the 2026-07-20
8-pair/121-gap work. Sweeping it for collar reasons reopens a settled detection calibration.

### Result 3 — the corrected map **converges**. § 3b confirmed; arm 3 complete.

Arm 3 held `W` at 2.0 and moved only the map, so it isolates § 3b; arms 1→2 held the map constant and
isolated `W`. 940 of 1073 gaps carry a collar (87.6%, up from 920); 830 of 940 had a trusted lag, 110
fell back to nominal with no fit above the trust floor, and 523 of the 830 actually moved.

**Read the moved-only matched cohort, not the unfiltered one.** The unfiltered matched per-gap ratio
is `1.000` for spread, residual *and* SE — an artefact, not a null: 307 of 830 trusted collars fitted
a lag of **zero**, so those rows are bit-identical between the arms and swamp the median. This is the
§ E trap in a new dress — the aggregate is real, it just answers a question nobody asked. On the 246
gaps whose map actually moved (n ≥ 8 in both arms):

| matched, moved, n ≥ 8 (246 gaps) | nominal | corrected | median per-gap ratio |
|---|---|---|---|
| `spread_db` | 0.942 | 0.344 | **×0.293** (improved on 216 of 246, 88%) |
| `se_db` | 0.411 | 0.145 | **×0.288** |
| `\|residual\|` | 0.371 | 0.163 | **×0.403** (improved on 177 of 246) |

Reproduce with `./scripts/measure/compare-collar-arms.ps1 -BaseDir gap-files/2026-08-27-collar-census
-NewDir gap-files/2026-08-27-collar-census-lag-work`. On these two arms the "changed" cohort and the
"map moved" cohort coincide exactly (502 of 919 matched gaps, 246 at n ≥ 8), which is its own check:
the map is the only thing that differs between the dumps.

**Every lag band improves, and the improvement grows with the lag.** Banded at n ≥ 8, with the
nominal column taken from **arm 1's own rows for the same gaps** (not § 3b's 273-dropout join, which
is a different population and whose numbers do not belong in a matched table):

| fitted lag | n | nominal spread / `\|resid\|` | corrected spread / `\|resid\|` |
|---|---|---|---|
| 0 ms | 163 | 0.262 / 0.084 | 0.262 / 0.084 |
| 100 ms | 142 | 0.774 / 0.290 | **0.367 / 0.153** |
| 200 ms | 47 | 1.334 / 0.418 | **0.351 / 0.256** |
| ≥ 300 ms | 67 | 1.355 / 0.605 | **0.307 / 0.129** |

The lag = 0 row is the control: the correction is a no-op there, the two columns are identical by
construction, and it reproduces the recorded 0.250 / 0.080 baseline. Under the nominal map the
degradation is monotone in lag; corrected, the band that was worst improves most.

**Bins are half-open, and the edge matters.** An earlier version of this table used `> 200` under a
`≥ 200` label, which parked the 47 collars sitting at exactly 200 ms — the worst remaining residual,
0.256 — in the band below and manufactured an inversion (0.171 vs 0.129) that does not exist. Moving
them across the edge simply moves the 0.171 with them. Census Q6 now emits these bands so the split
is reproducible rather than re-derived by hand.

**Convergence is substantial, not complete, and nothing here explains the remainder.** Pooled
`lag > 0` at n ≥ 8 reads 0.349 / 0.165 against lag = 0's 0.262 / 0.084 — ~1.3× spread and ~2×
residual outstanding. The honest profile of that remainder is **0.084 / 0.153 / 0.256 / 0.129**: it
peaks at 200 ms and falls again. Neither of § 3b's named caveats accounts for that shape:

- **Not a fixed quantization floor.** ±50 ms surviving by construction predicts a *flat* leftover.
  This one is not flat, and it is not monotone in lag either.
- **Not one lag failing to describe both shoulders** (`seam-splice-dualfit`). That predicts side
  disagreement growing with lag. `|pre − post|` runs 0.154 / 0.249 / 0.236 / **0.129** — lowest in
  the ≥ 300 ms band.
- **Selection remains the live hypothesis**, and this arm cannot test it. A gap whose fit reads
  exactly 0 is one where registration was easy, so `lag > 0` was never obliged to land on `lag = 0`;
  and a *large* lag may only clear the trust floor when the shoulder correlation is strong, which
  would explain the ≥ 300 ms band reading better than the 200 ms one. Testing it needs `peak_r`
  joined per gap, which the collar CSV does not carry. **Do not read the outstanding 2× as a
  measurement of anything** — of the instrument or of the programme — until that join exists.

**Q5's profile survives the correction; the reopen condition did not fire.** Across the spread bands
`|pre − post|` medians run 0.154 / 0.587 / 1.229 / 3.668 dB — still rising, so misregistration was
not manufacturing the relationship, and § 3a stays refuted on its own terms. But this is a
*repetition*, not a strengthening, and the counts must be read at one filter: § 3a's recorded
"0 of 76" was `n_pre ≥ 8 && n_post ≥ 8`, and on that same cut the corrected arm reads **0 of 124** —
unchanged. What the correction did change is the *population*: two-sided collars above 2.0 dB spread
fall from 78 to 26 unfiltered (14 → 4 on the well-populated cut), so much of the high-spread
material was misregistration, and what remains still tracks spread.

One provisional worry from arms 1–2 is answered: a wider collar reaching further from where the lag
was fitted might amplify misregistration. The matched spread ratio of ×1.00 was weak evidence against
it because the nominal-map spread floor may have hidden the bias — arm 4 is the real test, and it now
runs on a map that is right to within a block.

**Still not constants.** Arm 3 is the right map at the wrong width. `MIN_LOCAL_PAIRS`,
`local_pair_collar_secs` and `MAX_LOCAL_RESIDUAL_DB` come from arm 4. What arm 3 does fix is the
floor's cost basis: at W = 2.0 corrected, `n_pairs < 8` sends **437 of 940 measured collars (46.5%)**
back to program gain, and together with the 133 gaps that abstain outright that is **570 of 1073
(53.1%) of all gaps** carried by program gain rather than by a collar — the two denominators are
different questions and the census prints the second. `n_steady / n_active` is 0.333 and
`n_pairs / n_steady` is 1.000 — B coverage
costs nothing, steadiness costs everything, so Result 2's remedy points at `W` and not at the floor.

**Standing caution, unchanged:** do not write `MIN_LOCAL_PAIRS`, `local_pair_collar_secs` or
`MAX_LOCAL_RESIDUAL_DB` into the code from arms 1–2. Result 1 is strong enough that 4.0 is the
expected answer for the collar width, but the two level-domain constants must come from a corrected
arm — the `max_fill_gain_db` precedent (12.0, uncalibrated since 2026-06-08) is what happens
otherwise.

### Running the remaining arms

**`local_pair_collar_secs` has no CLI flag** (§ 9: "No new CLI") — it is a `[repair]` TOML field, so
an arm at a different `W` needs a config file passed through the runner's `-ScanArgs`:

```toml
# gap-files/collar-4s.toml   (gitignored dir; the value is the ONLY line that differs between arms)
[repair]
local_pair_collar_secs = 4.0
```

```powershell
# 1. Scan. Do NOT add -SkipBuild: the default build is what puts the map correction in the binary.
#    Default -Features 'he-aac,ac3' is correct — he-aac registers the only AAC decoder, including
#    for AAC-LC. `calibration` is not needed; this path never passes --gap-fingerprints.
./scripts/measure/scan-registration.ps1 `
    -Manifest gap-files/extended-pairs.csv `
    -OutDir   gap-files/<date>-collar-census-<arm> `
    -ScanArgs "--config gap-files/collar-4s.toml"     # omit entirely for a W = 2.0 arm

# 2. Census the dump. Consumer-only; re-runnable as often as you like.
./scripts/measure/census-collar-levels.ps1 `
    -DumpDir gap-files/<date>-collar-census-<arm> `
    -CsvOut  gap-files/<date>-collar-census-<arm>/collar-levels.csv
```

**Verify the arm before spending the full run.** The scan's built-in precheck validates
`donor_registration.envelopes`, *not* `map_lag_ms`, so a stale binary yields well-formed JSON with
every collar silently at the nominal map. As soon as the first `<label>.json` is non-empty (a
zero-byte file is the pair currently being written, not a failure):

```powershell
Select-String -Path gap-files/<dir>/*.json -Pattern '"map_lag_ms"' -List
```

A hit means the correction is live. `map_lag_ms` has no `skip_serializing_if`, so the key is present
on every collar of a correct run even where no fit was trusted (it serializes as `null` there).

**And confirm the arm is the arm you meant.** Since 2026-08-27 the scan stamps the width it measured
at as `scan.local_pair_collar_secs`, so a dump is no longer attributable to a `W` only
by the directory it landed in — which is what the first two arms rested on. The census prints the
stamped width in its header and refuses to pool a directory holding more than one:

```powershell
Select-String -Path gap-files/<dir>/*.json -Pattern '"local_pair_collar_secs"' -List
```

Arms 1–3 predate the stamp; the census prints `n/a` for them and the config that was passed stays
their only record. Arm 4 onward carries it.

**Then read Q6's `n>=8` block first**, against Result 3's branches. The cut matters: the 0.25 / 0.08
baseline was taken at n ≥ 8, and comparing an unfiltered cohort against it reads two different
populations. Q5's banded profile is the secondary read — a flat profile there would reopen § 3a.

**Comparing two arms: `./scripts/measure/compare-collar-arms.ps1 -BaseDir <a> -NewDir <b>`.** Each
side is a dump dir or a per-gap CSV; a dump dir is run through `census-collar-levels.ps1` rather than
re-parsed, so the two scripts can never disagree about the same dump. It reproduces every recorded
number in Result 1 (gained 86 / lost 0, abstentions 153 → 67, cohort clearing 450 → 646, SE improved
on 609 gaps), which is what makes it the record rather than the CSVs.

It exists because both rules below have already produced a wrong reading, and neither is visible in
the output that gets them wrong:

- **Median of the per-gap ratios, never the ratio of aggregate medians** (`settled.md` § E). This
  misread the `W` arms: aggregate SE 0.329 → 0.310 looks flat, matched is ×0.74.
- **Exclude rows the change did not touch.** Where an arm moves only some gaps, the rest are
  bit-identical and contribute an exact 1.000, so the median is 1.000 at any effect size. This
  misread arms 1 → 3: unfiltered reads ×1.000 for spread, residual *and* SE against the true
  ×0.293 / ×0.403. The script finds untouched rows by their *readings* being identical, so it needs
  no knowledge of what the arms differ in, and it prints the unchanged count rather than hiding it —
  "almost nothing changed" is a finding about the change, and the one shape that looks like a bug.

---

## 5. Who gets `g_local`

Today dual-fit / SilenceSplice hard-code local unity because `compute_fill_gain`
needs fill RMS. **This estimator does not** — the collar is scan-time, fill-agnostic.

| Path | `g_program` (landed) | `g_local` (this plan) |
|---|---|---|
| Bracket fill | yes | if `normalize_fill` and collar did not abstain |
| Dual-fit / SilenceSplice | yes | **same** — the old reason to skip it is gone |
| Skip / not-applied | n/a | n/a |

---

## 6. When to stamp (scan, not patch)

`ScanGaps` already holds `a_levels`, `b_levels`, `offset_secs`,
`b_scanned_end_secs`, and the gap list at the `program_levels` site, then
**drops** the timelines. Stamp each gap there. Do not keep the arrays. Do not
wait for patch decode.

```rust
/// Collar pairing at one gap. Same sign as AbLevelOffset (B minus A).
pub struct CollarLevelOffset {
    pub offset_db: f64,
    pub spread_db: f64,
    pub n_pairs: usize,
    pub n_pre: usize,
    pub n_post: usize,
    /// Median of the pre-side pairs alone; `None` on a one-sided collar. Detector, not a gate
    /// — see § 3a.
    pub pre_offset_db: Option<f64>,
    /// Median of the post-side pairs alone; `None` on a one-sided collar.
    pub post_offset_db: Option<f64>,
    pub first_pair_secs: f64,
    pub last_pair_secs: f64,
}
```

`n_pre` / `n_post` are provenance (is this a one-sided collar?), not a gate —
counts of **accepted pairs** whose A-block start falls in pre vs post.
`pre_offset_db` / `post_offset_db` are the § 3a straddle detector over the same
binning; nothing branches on them in v1. `first_pair_secs` / `last_pair_secs`
are A-timeline, same as `AbLevelOffset`. No `chunks`. Type home:
`program_levels.rs`, next to `AbLevelOffset`.

**`channels_comparable` moves up to `ProgramLevels`.** It is already computed in
`scan_gaps.rs` from `track_compatibility`, *independently* of whether the
whole-file offset survived `MIN_PAIRS` — but today it is only readable off
`ProgramLevels::offset`, which is `None` whenever pairing abstained. Hoisting it
to a field on `ProgramLevels` (keep the one on `AbLevelOffset`; it is the same
fact and consumers already read it there) is a two-line change that removes an
inference this plan would otherwise have to make and get wrong: with it only on
the offset object, a pair whose channels differ **and** whose whole-file pairing
abstained reports the collar's absence as `abstained_n` when the true cause is
`channels`.

If channels are incomparable, local pairing is the same confound: **do not stamp
a collar**, so JSON absence means “no pairing”, same as whole-file. Apply reads
the hoisted `program_levels.channels_comparable` for the `channels` reason
(§ 8) — never inferred from absence, which is also “nothing in the collar
paired”, “no alignment offset”, and “B not scanned”. Absence is **not**
`n_pairs < MIN_LOCAL_PAIRS`: a thin collar is present on the JSON and refused at
apply, so `abstained_n` reads the count rather than the absence.

Home: `Gap.collar_level: Option<CollarLevelOffset>` (`skip_serializing_if =
None`). Index-parallel extra vec is the wrong shape — this *is* the gap’s
level reading. Mechanical `Gap { … }` literals get `collar_level: None`.
`GapScanJson` already serializes `Vec<Gap>`, so the field appears without a
DTO change.

Extract a shared pair-collection helper from `paired_level_offset`. The loop
already accumulates `(a.start_secs, d_i)`. Split there:

- **collect** — optional A-block predicate (collar = overlaps pre∪post and
  does not overlap the core; whole-file = all blocks). Returns the pair vec.
- **summarize** — one median/MAD path. Whole-file wraps as `AbLevelOffset`
  with `MIN_PAIRS = 50`; the collar wraps as `CollarLevelOffset` with **no
  floor**, then bins the same pair vec into `n_pre` / `n_post` and their medians
  (§ 3a) by A-start. A `fn(block) -> bool` cannot produce those counts by itself.
- Both wrappers carry `n_active` / `n_steady`. On the collar these are the only
  way to read *why* a thin collar is thin — `n_steady / n_active` is what
  steadiness cost (remedy: widen `W`), `n_pairs / n_steady` is B-coverage yield
  (remedy: none, a wider collar will not help). A bare `n_pairs` names no cause,
  and § 4a has to choose between those two remedies.

**The predicate gates *pairing*, never the timeline.** `collect` must keep
iterating the **whole** `a_levels` slice with its real indices, because
`is_steady` reads block `i`'s time-adjacent neighbours out of that slice, and a
neighbour that does not abut counts as *absent*, which is **permissive**. Slice
the timeline down to pre/post first and the property § 3 relies on inverts: the
A block abutting the silent core loses its core-side neighbour, reads as steady,
and the fade/transient block that walk-off exists to reject gets paired — the
exact failure the collar was designed around. Filter which blocks may *emit a
pair*; never which blocks may *be seen*.

Per-gap cost is a binary search for the collar bounds plus ≤ `2W / block` blocks
— do not walk the full timeline once per gap. `max_overlap_block` already
binary-searches; the collar window wants the same.

Patch reads `request.report.gaps[i].collar_level` plus the already-decided
`g_program` / applied `offset_db`. No second reduce. **There is no
report-reload path** — `GapReport` and `Gap` are `Serialize`-only, and
`--patch-only` re-runs the scan in-process (`composition.rs:348`), so "a scan
that predates this field" cannot reach the apply site. Every run in a binary
carrying this Change either stamps a collar or has one of § 8's stated reasons.
The mixed-vintage hazard is entirely consumer-side, in `gap-files/` dumps —
same as `normalize_gain` and `program_gain` before it.

---

## 7. Stacking at the splice (single multiply)

Same as program-gain § 5: one `RegionPatch.gain`. Program-gain has **already
landed** in this tree (`domain::program_gain`, `stack_fill_gain`,
`request.program_gain: ProgramGainDecision`). Implement against that, replacing
the RMS local half. Do not bundle the two Changes into one commit/PR.

```text
g_program  = request.program_gain.gain()          # 1.0 on any refusal
offset_prior = Applied { offset_db } else 0
g_local    = collar_residual_gain(...)            # 1.0 when flag off / abstain / refuse
g_total    = g_program * g_local
```

**Do not** compute local from `rms(fill * g_program)` — that was the fix for
the *RMS-ratio* estimator double-counting. The residual formula is the
double-count guard for *this* estimator. `stack_fill_gain` today *is* that RMS
path; rewrite it (or replace the call site) so the local factor comes from
`collar_residual_gain`, not `compute_fill_gain`.

If this Change is ever replayed onto a tree **without** program-gain:
`g_program = 1`, prior = 0, `g_local = 10^(-d_local/20)`. A constant-gain donor
is then matched from the collar (holes with enough pairs) and left unmatched
where the collar abstains. That is acceptable; program-gain later fills those
abstentions. A same-master quiet hole still gets residual 0.

`--no-normalize` remains local-off. It must not clear `collar_level` from the
scan report (measurement stays; apply does not).

Program-gain’s remaining media acceptance still expects `--normalize` to
restore 15/g6 and 2/g6 via the RMS residual. **This plan withdraws that row**
(§ 10). Run program-gain’s default-flags cohort (local off) first if that
table is still owed; once this ships, the `--normalize` regain is obsolete.

---

## 8. Reporting

Keep `normalize_gain` = `g_total` (splice multiplier; `fill_level` contract).

| JSON | Value |
|---|---|
| `scan.gaps[].collar_level` | measurement; omit when `None` |
| `status.patched.normalize_gain` | `g_total` (existing) |
| `status.patched.program_gain` | landed program-gain field |
| `status.patched.local_gain` | `g_local` (`1.0` when off or abstained), **unconditional** once this ships |

`g_local = normalize_gain / program_gain` only when program-gain is present and
non-zero. Emit `local_gain` anyway — dividing is how the last plan avoided a
third field, but this estimator’s abstention vs “on and unity residual” are
different facts; `1.0` from residual-zero and `1.0` from abstention would
collapse. **Stamp the reason** on the patch record next to `local_gain`.

Mirror `program_fill_gain`: flag first (reason, not outcome — every failure is
unity), then preconditions, then the residual bound.

```text
collar_residual_gain(
    collar: Option<&CollarLevelOffset>,
    enabled: bool,                 # normalize_fill
    offset_prior: f64,             # Applied offset_db, else 0
    max_residual_db: f64,          # MAX_LOCAL_RESIDUAL_DB (§ 4) — NOT max_fill_gain_db
    channels_comparable: bool,     # program_levels.channels_comparable (hoisted, § 6)
) -> LocalGainDecision
```

| Reason (`snake_case`) | When |
|---|---|
| `flag_off` | `enabled == false` (collar may still be on the scan JSON) |
| `channels` | collar `None` and `channels_comparable == false` |
| `abstained_n` | collar present but `n_pairs < MIN_LOCAL_PAIRS`, **or** collar `None` otherwise (nothing paired, no alignment offset, B not scanned, no A timeline) |
| `refused_residual` | collar present, `\|d_local − offset_prior\| > MAX_LOCAL_RESIDUAL_DB` |
| `applied` | collar present, residual inside the bound (including ~0) |

No `stale_report` reason and no `serde(default)` anywhere: nothing reads a
report back (§ 6).

`LocalGainDecision::{Applied { gain }, Refused { reason }}` — same shape as
program gain, per-gap rather than per-run. Thread `gain` + reason onto bracket
`RegionPatch` **and** `GapRepairStrategy::SilenceSplice` (today the splice
only stores `normalize_gain = request.program_gain.gain()`).

---

## 9. Config / CLI

```toml
[repair]
normalize_fill = false          # unchanged by this Change (program-gain owns the default)
local_pair_collar_secs = 2.0    # new; config-only; **scan** reads this (ScanGapsRequest)
                                # provisional — § 4a's yield curve sets the shipping value
# MIN_LOCAL_PAIRS      = 8   code constant, provisional (§ 4a); read at APPLY, not by the
#                            estimator — a floor there would censor § 4a's own dump
# MAX_LOCAL_RESIDUAL_DB = 6.0 code constant, provisional (§ 4a); refuse past it, do not clamp
max_fill_gain_db = 12.0         # UNTOUCHED. Still the compute_fill_gain boost ceiling and
                                # nothing else; it is not this estimator's bound (§ 4) and
                                # dies with compute_fill_gain, not with this Change
normalize_window_secs = 5.0     # **keep reading** — A-border template / correlate_frames /
                                # fingerprint noise_collar_secs. Named for normalization,
                                # used as a border size. Not this estimator’s window.
```

Stop using `normalize_window_secs` for `compute_a_border_rms` / fill gain.
Do **not** drop it from `PatchRequestSettings`, `FillWindowFrames`,
`FillGeometryRecipe`, or the fingerprint probe. A checklist item that says
“stop reading it in patch” is wrong and would break placement. Leave the TOML
field; document that the name is historical. No new CLI. `--normalize` /
`--no-normalize` stay the on/off for this path.

---

## 10. Tests / acceptance

**Unit (synthetic `BlockLevel` timelines, no media)**

- Gap in a flat A=B program: `d_local ≈ 0`, `n_pairs` from both collars, core
  blocks contribute **zero** pairs.
- B = A + 6 dB everywhere: `d_local ≈ 6`, independent of gap length.
- Quiet A passage around the hole, B matches it, loud material 3 s out: `d_local`
  stays near 0 (the 5 s RMS pool would not). This is the defect test.
- Leading-edge gap: pre empty, post pairs, still `Some`; `pre_offset_db` is
  `None`, `post_offset_db` is `Some`.
- Fewer than `MIN_LOCAL_PAIRS` pairs → **still reported**, with `n_pairs` below
  the floor. Pins the un-censoring above; the refusal is apply-side.
- Nothing pairs at all → `None`. The one case the counters cannot explain,
  pinned so the limit is a decision rather than a surprise.
- Counters name the cause: two collars with identical `n_pairs`, one spent by
  steadiness (`n_steady ≪ n_active`) and one by B coverage
  (`n_pairs ≪ n_steady`).
- Steadiness: the A block abutting the core is excluded; the next steady block
  can pair. **Pin the slicing hazard** (§ 6): build a timeline where the
  abutting block is a fade — it must not pair, which only holds if `is_steady`
  saw the core blocks. A collar built over a *sliced* timeline passes every
  other test here and fails this one.
- Straddle (§ 3a): B is `A + 0` before the hole and `A + 6` after →
  `pre_offset_db ≈ 0`, `post_offset_db ≈ 6`, and `offset_db` is still the pooled
  median (detector, not a gate — nothing refuses).
- Residual: `offset_prior = 6`, `d_local = 6` → `g_local = 1`, reason `applied`.
- Residual: `offset_prior = 0`, `d_local = 6` → `g_local ≈ 0.501`.
- Refuse, do not clamp: `|residual|` just past `MAX_LOCAL_RESIDUAL_DB` →
  `g_local = 1.0`, reason `refused_residual`. Assert against the constant, not a
  literal, so § 4a can move it without rewriting the test.
- `enabled = false` → `g_local = 1`, reason `flag_off`, collar still `Some` on the gap.
- Collar `None`, `channels_comparable = false` → reason `channels`, not
  `abstained_n` — **including** when the whole-file offset is `None` too (the
  case the hoist in § 6 exists for).

**Apply / stacking**

- `normalize_fill` off → `local_gain = 1`, collar still on the scan JSON.
- Product: program Applied at +6 dB, collar `d_local = +6` → `g_total ≈ 0.501`,
  not `0.501²`.
- Dual-fit path receives `g_local` when the collar exists.

**Cohort (after § 4a's census, then a `--patch-only` roll-up with `--normalize`)**

Not before § 4a. The constants this table's numbers depend on are set by that
dump; running the cohort first measures an estimator nobody has calibrated and
produces a table that has to be thrown away. The § 4a pass runs with default
flags (local **off**), so it costs one ordinary corpus scan.

The natural before/after baseline already exists and should be reused rather
than re-measured: the 2026-08-27 pair of 51-pair / 308-gap roll-ups one
`--normalize` apart (`pipeline.md` § Fill gain) — 111 of 308 gaps moved, all
boosts, median +1.91 dB, seven pinned at the 12 dB clamp. **State what happens
to those 111 and those 7**, not only to the named gaps below. The expectation is
that most collapse toward unity (they were fill RMS vs a wide A window, not a
master difference) and that the seven pins become `applied ≈ 1` or
`abstained_n`, never `refused_residual` at a new rail.

Q4 of the findings note was written for an RMS-window tweak that had to keep
15/g6 and 2/g6 boosts. **That test is withdrawn** — those boosts were fill RMS
vs a wide A window, which this estimator does not compute.

| Gap | Expect with `--normalize` |
|---|---|
| 45/g4, 45/g5, 19/g13, 5/g6, 15/g3 | `local_gain ≈ 1` — either reason. `applied` with residual ~0 is the designed path (a quiet A collar is still *active*: the threshold is ≈ −60 dBFS and 19/g13's shoulders read −52), but `abstained_n` is an equally passing outcome and the more likely one on a short or transient-heavy collar. The defect is dead either way; **record which**, because a corpus that mostly abstains is § 4a's first question answered the wrong way |
| 24/g46 | not crushed (scalar from shoulders, not fill RMS) |
| 15/g6, 2/g6 | **may sit at unity**; not a failure |

Default flags (local off) are program-gain’s acceptance, not this plan’s.

---

## 11. Implement checklist

**Measurement half (lands first; local stays off, so this is shippable alone)**

- [ ] Extract collect + summarize from `paired_level_offset`; collar predicate gates **pairing only** — `collect` keeps iterating the full `a_levels` slice so `is_steady` sees the core blocks (§ 6); binary-search the collar bounds rather than walking the timeline per gap
- [ ] Bin `n_pre`/`n_post` **and** `pre_offset_db`/`post_offset_db` from pair A-times (§ 3a)
- [ ] `CollarLevelOffset` in `program_levels.rs` (incl. `n_active` / `n_steady`) + `MIN_LOCAL_PAIRS` + `MAX_LOCAL_RESIDUAL_DB` + collar window helper (pre/post, exclude core); mark all three constants provisional per § 4a. `MIN_LOCAL_PAIRS` is **not** read by the estimator — no floor there, or the census is censored at the constant it sets
- [ ] Hoist `channels_comparable` onto `ProgramLevels` (keep it on `AbLevelOffset`; same fact, existing consumers)
- [ ] Thread `local_pair_collar_secs` onto `ScanGapsRequest` (`composition.rs` and every request literal) — because the timelines live in `ScanGaps`, **not** because of `--patch-only` (§ 3)
- [ ] Stamp `Gap.collar_level` in `ScanGaps` next to `program_levels` (timelines still live); skip when channels incomparable. ~64 `Gap { … }` literals need the new field — add a constructor or `Default` rather than 64 mechanical edits
- [ ] Gap object table in [json-output.md](../json-output.md); omit when `None` (`GapScanJson` serializes `Gap` already)
- [x] `scripts/measure/census-collar-levels.ps1` — per-**gap** sibling of `census-program-levels.ps1` (which is per-pair and cannot answer § 4a)
- [x] `scripts/measure/compare-collar-arms.ps1` — matched two-arm join; no single-dump census can answer "did it help?", and both of the rules that answer it wrong are silent failures (§ 4b)
- [ ] **§ 4a: run the corpus census and set the three constants against it.** Everything below waits on this

**Apply half**

- [ ] Pure `collar_residual_gain` per § 8 (`enabled`, `offset_prior`, `max_residual_db`, `channels_comparable`)
- [ ] Replace the RMS half of `stack_fill_gain` / `compute_fill_gain` production call; drop the local `debug_assert!(gain >= 1.0)`
- [ ] Thread `g_local` + reason onto bracket **and** dual-fit / `SilenceSplice` (today splice stores only `program_gain`)
- [ ] Emit `local_gain` + reason; `normalize_gain` remains the product
- [ ] Config `local_pair_collar_secs`; **keep** reading `normalize_window_secs` for border geometry / fingerprint collar; leave `max_fill_gain_db` alone (§ 4)
- [ ] Tests in § 10; `cargo test`; clippy clean

**Docs — wider than the gap table; these all go actively false**

- [ ] [json-output.md](../json-output.md) § `GapPatchStatus`: the `normalize_gain` narrative asserts *"never below 1.0"*, *"`g_local` (never serialized separately)"* and *"there is deliberately no third field"* — all three die with `local_gain`
- [ ] [pipeline.md](../pipeline.md) § Fill gain: the *Hole-local* paragraph (RMS vs A-border, `[1.0, max_fill_gain_db]` clamp), the *How they stack* paragraph (dual-fit takes program only), and *What `--normalize` now means* (the boost-only regime + the 111/308 measurement — supersede it, do not delete the number)
- [ ] [gap-scan.md](../gap-scan.md) *"`normalize_gain` is censored at its clamp"* → refusal, not clamp
- [ ] [settled.md](settled.md) § E *The fill-gain distribution is censored at the clamp*: still true for old dumps, no longer describes current behaviour — date-scope it
- [ ] `patch_result.rs` doc comment on `normalize_gain` (*"Never below 1.0 since 2026-08-12"*) and `config.rs` on `max_fill_gain_db`
- [ ] README `--normalize` row; note `max_fill_gain_db` no longer touches the production path

**Do nots**

- [ ] Do **not** bundle with program-gain. Do **not** keep `BlockLevel` arrays. Do **not** read B gap interior. Do **not** stop reading `normalize_window_secs` as a border size. Do **not** reuse `max_fill_gain_db` as the residual bound. Do **not** slice the A timeline before the steadiness check

---

## 12. Done when

Scan JSON for a normal A+B run carries `collar_level` — with per-side medians —
on gaps that cleared `MIN_LOCAL_PAIRS`; a synthetic quiet-hole-in-loud-program
fixture reports `offset_db ≈ 0` on that collar (the RMS pool’s failure mode);
**§ 4a's census exists and the three constants are stated against it, including
the corpus abstention rate and the per-collar SE**; `--normalize` on a
same-master pair does not revive the n=5 over-boost; a constant-gain donor with
program-gain off is matched from the collar where it has pairs; and
`g_program * g_local` equals `10^(-d_local/20)` when both apply, not the square.

An abstention rate high enough that program gain carries most holes is a
**result**, not a failure — but it must be stated, because it is the difference
between "this Change replaced the estimator" and "this Change deleted it".

---

## Related

- [TEMP-program-gain-plan.md](archive/TEMP-program-gain-plan.md) — file-wide scalar, stacking identity
- [archive/TEMP-ab-program-levels-plan.md](archive/TEMP-ab-program-levels-plan.md) — pairing rules
- [TEMP-fill-normalize-local-reference-2026-08-25.md](archive/TEMP-fill-normalize-local-reference-2026-08-25.md) — defect this replaces the RMS fix for
- [settled.md](settled.md) § A B-relative gain; § C RMS-attenuation latch
- [pipeline.md](../pipeline.md) — current two-half fill gain (program scalar + RMS local residual)
