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

**Reopen as a gate when** § 4a's census shows `|pre − post|` large on materially
more than the one collar § D predicts, or shows it correlated with the gaps the
ear labels. The
rule to write then is a refusal (`refused_straddle`), never a per-side
selection — picking the "better" side is § 1's refuted *quieter-of* idea wearing
a new hat.

Two alternatives were considered and are **not** being built. Refusing on
disagreement now: no calibration exists, see above. Mapping the collar through
the per-clip / interpolated offset instead of the global one (`alignment.clips`,
`offsets_consistent`, `offset_drift_secs` are all on the report at scan time):
that fixes the cause rather than detecting it, but it changes the collar's own
map relative to the whole-file offset the residual is taken *against*, which is
a different Change and must not ride inside this one.

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
| How often do the sides disagree? | `\|pre_offset_db − post_offset_db\|` (§ 3a) | Decides whether the straddle detector needs to become a gate |

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
- [ ] `scripts/measure/census-collar-levels.ps1` — per-**gap** sibling of `census-program-levels.ps1` (which is per-pair and cannot answer § 4a)
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
