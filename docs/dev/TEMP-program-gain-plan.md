# TEMP — Program-offset fill gain (replace default local normalize)

**Status:** draft, 2026-08-25. **Startable** as of 2026-08-26 — the capture plan § 7a Tier 3 trigger
fired (51 pairs, a single scalar exists; see § 4a), and the Tier 2 calibration § 4a redirected to has
run (§ 4b). **Rewritten to a two-knob shape 2026-08-26** after the four-threshold gate was challenged
as over-built for "does B match A? if not, add the difference": three of the four thresholds defended
against media this corpus has zero instances of, and § 4b showed the most elaborate of them leaks on
the case it was added for. § 4 is now two refusals — `max_spread_db` and `max_program_gain_db` — with
the deadband dropped and the chunk-drift gate demoted to reporting (latched on a trigger, § 4).

**Tracked as:** [BACKLOG.md](../../BACKLOG.md) § *Fill normalize boosts against a
non-local reference* → *Default `normalize_fill` off, gain fills from the program scalar*.

**Companions:** capture
[TEMP-ab-program-levels-plan.md](archive/TEMP-ab-program-levels-plan.md) § 8 (the
requirements this plan discharges); local-reference defect
[TEMP-fill-normalize-local-reference-2026-08-25.md](TEMP-fill-normalize-local-reference-2026-08-25.md);
[settled.md](settled.md) § A **B-relative / donor-shoulder fill gain** (refuted —
this plan is **not** that idea, see § 2); latched attenuation-half of *local*
normalize ([settled.md](settled.md) § C — also not this idea, see § 4).

**Media hygiene:** pair index only; no filenames / titles / paths in the repo.

---

## 1. Goal / non-goals

**Goal**

When the scan's paired A↔B offset is trusted, apply **one whole-program scalar**
to every B-derived fill so donor material sits at A's program level. Default
**off** the hole-local `normalize_fill` boost. If the operator turns local
normalize back on, it is a **residual** on the already-matched fill — additive
in dB, not a second copy of the master offset.

**The corpus argues for the default-off half directly.** Seven patched gaps sit
pinned at the 12.000 dB local `max_fill_gain_db` ceiling on a corpus whose masters
match to within 0.26 dB. A 12 dB demand there cannot be a master mismatch — there is
none to find — so it is the local 5 s + 5 s reference failing on its own. Whole-program
gain would not have prevented one of them; defaulting local off would have prevented all
seven. (Those pins also censor the distribution — [settled.md](settled.md) § E.)

**Non-goals (v1)**

- Matching fills to **B's gap shoulders** (refuted — § 2).
- Per-chunk / time-varying gain. One scalar per file or none; `chunks` are
  **reported**, never a per-gap bucket (§ 4).
- Fixing the local A-border window (5 s + 5 s energy pool). The replacement
  estimator is a separate Change:
  [TEMP-local-paired-gain-plan.md](TEMP-local-paired-gain-plan.md). Do not bundle.
- ITU / LUFS.
- Peak-aware limiting / clip protection beyond a dB clamp.
- Reopening the symmetric clamp on *local* normalize.

---

## 2. Why this is not the refuted idea

| | Refuted (settled § A) | This plan |
|---|---|---|
| Reference | B's **shoulders at the hole** | Whole-program paired `offset_db` over **undamaged** overlapping blocks |
| What it does | Reproduces the level the fill already has (the fill *is* B) | Puts B's *master* onto A's *master* |
| Same-master, A reference wrong | Leaves 19/g13's over-boost untouched | Orthogonal — that over-boost is local normalize, which this defaults **off** |
| Masters genuinely differ | Corrects nothing | This is the case the scalar exists for |

The capture TEMP already forbids reading B's gap shoulders. This plan never
does. `compute_a_border_rms` is used only by the **residual** local path, and
only after the program scalar has been applied.

---

## 3. Sign, formula, clamp

Scan convention (already shipped): `offset_db` is **B minus A**. Positive ⇒ B is
louder.

The fill is B content spliced into A, so the applied program gain is the
**inverse**:

```text
g_program = 10^(-offset_db / 20)     # linear
g_program_db = -offset_db            # dB
```

| `offset_db` | Meaning | Fill gain |
|---|---|---|
| `+6` | B 6 dB louder than A | **attenuate** 6 dB (`≈ 0.501`) |
| `−6` | B 6 dB quieter than A | **boost** 6 dB (`≈ 2.00`) |
| `≈ 0` | masters matched | `≈` unity — applied as measured, no deadband (§ 4) |

**Attenuation is load-bearing and allowed.** A louder donor must come down to
A's program. That is a different mechanism from the latched "attenuation half
of fill normalize" (settled § C): that clamp used **whole-fill RMS vs A
borders** and crushed intra-fill dynamics (the gunshot case, 24/g46). A
whole-program scalar does not — it is constant across the fill, so a loud event
that is loud on both masters stays loud relative to its neighbours.

**Bound, and it is a refusal rather than a clamp.** `|offset_db|` past `max_program_gain_db`
(**6.0**) yields unity, not 6 dB of gain — see § 4. Boosting a hot fill inside the bound can clip;
that is accepted in v1 (no limiter). Zero RMS / missing offset → `g_program = 1.0`.

The bound is **not** a measurability guard. Tier 2 recovers a flat gain exactly, in both polarities,
out to ±40 dB (§ 4b) — the estimator is not what limits it. What limits it is § 4b's headline: a
mildly compressed B reports an 8.4 dB offset that no usable spread threshold rejects. Since a large
offset is a known signature of *that* defect and never of a measurement failure, clamping to the
bound would apply 6 dB of a number we have reason to disbelieve. Refusing applies none of it.

---

## 4. Two refusals (fail closed → unity)

`PatchAudio` already holds `request.report: GapReport`. Read
`report.program_levels.offset` once per run, not per gap.

**First, the measurement has to exist.** These are not thresholds — there is no number to apply:

| Precondition | Fail → |
|---|---|
| `offset` is `Some` | unity (no pairing, or below `MIN_PAIRS`) |
| `channels_comparable` | unity (a stereo/mono confound is not a gain) |

**Then two numbers, and only two.**

| Refusal | Threshold | What it is for |
|---|---|---|
| `spread_db > max_spread_db` | **2.5** | the blocks disagree, so B is not a constant-gain copy of A and no scalar is the answer. This is the measurement's own confidence, not an extra idea |
| `\|offset_db\| > max_program_gain_db` | **6.0** | the answer is implausibly large for two masters of the same program. The backstop for what spread misses (§ 4b headline) |

Otherwise apply `g_program = 10^(-offset_db / 20)` **exactly as measured**.

```text
program_gain_max_spread_db = 2.5      # Tier 2 § 4b: corpus max 2.412, 4:1 compression 2.770
max_program_gain_db        = 6.0      # refuse past it; see below
```

**Why 6.0.** No corpus pair comes within 20× of it (max 0.260 dB), so it gates nothing real that we
have seen, and it is comfortably under § 4b's 8.4 dB phantom from a 2:1-compressed B — the only
mismatch signature this plan has ever measured. It is not inherited from the 12 dB local
`max_fill_gain_db`, which is a different quantity with a different failure mode. A genuine master
mismatch past 6 dB would be refused; that refusal is today's behaviour (unity), so the cost of being
wrong high is zero and the cost of being wrong low is 8.4 dB on every fill in the file.

**No deadband.** An earlier draft refused offsets under 0.5 dB as noise. They are not noise. The
offset is a **median** over ~500 paired blocks, so no single block moves it, and its standard error
is the per-block scatter over √n: `1.25 × (0.668 × 1.48) / √500` ≈ **0.055 dB**. A corpus pair
reading 0.26 dB is ~5 standard errors off zero — a small, consistent, whole-program bias, not
sampling noise. The mp3 control agrees from the other side: a codec round-trip moved the offset by
0.001 dB, so generation loss is not manufacturing it either. Applying 0.26 dB is inaudible and
*correct*; a deadband's only real effect was making the feature inert on every pair we own, which is
not a reason. Cost: § 13 loses its byte-identity acceptance — replaced there. If run reproducibility
is later wanted back, the honest form is a fixed 0.1 dB *don't-bother* constant just above that
standard error — **not** a config knob, and not described as a noise floor.

**Chunk drift is reported, not gated — latched.** § 4b calibrated
`program_gain_max_chunk_drift_db` = 1.5 and it does catch a class spread misses (a level change
partway through a program: drift sees ≈3.4 dB of it, spread needs ≈10 dB). It is demoted anyway,
because it is undefined for any program under ~20 minutes, its margin over the corpus is 0.209 dB
(1.291 → 1.5), and the corpus has **zero** pairs with any drift at all. A gate that thin, defending
against media we have never seen, is complexity charged to every run for a hypothetical.

The residual is explicit: a 3.4–10 dB drift now passes, and the wrong scalar it produces is bounded
by `max_program_gain_db`. `robust_drift_db` is printed on the patch summary so a human can see it.
**Trigger to re-arm the gate:** any real pair measuring robust chunk drift above 1.0 dB. Until then
it stays a reported number. (When this plan closes, that verdict belongs in
[settled.md](settled.md) § C, not in BACKLOG.)

**When drift is read at all, it is the robust statistic, not raw max-minus-min** — the range over
chunks holding at least half the median chunk population, as
`scripts/measure/census-program-levels.ps1` computes it. Raw range is set by whichever chunk is least
populated ([settled.md](settled.md) § E), and Tier 2 measures 2.195 raw vs 1.336 robust on the *same*
fixture (§ 4b).

`n_pairs` is already a capture abstention (`MIN_PAIRS = 50`); do not add a
second floor unless Tier 3 shows 50 is too low for a gain consumer.

---

## 4a. What Tier 3 calibrated, and what it could not

Tier 3 ran 2026-08-26 over 51 pairs (`--patch-only --format json`, then
`scripts/measure/census-program-levels.ps1`). All 51 measured; zero abstentions,
zero channel mismatches, B coverage 0.951-1.000.

| | Corpus reading |
|---|---|
| `offset_db` | median +0.005, range -0.221 … +0.260, sd 0.109, **no pair past 0.3** |
| `spread_db` | median 0.668, max 2.412; 34/51 at or under 1.0 |
| robust chunk drift | median 0.162, max 1.291; **50/51** within 1.0 dB |

**The trigger is answered: a single scalar exists.** It is also, on this corpus,
approximately zero everywhere.

That last fact is what limits the calibration. Only one threshold can be set from a
corpus whose true answer is uniformly zero:

| Knob | Calibrated? | Basis |
|---|---|---|
| `max_spread_db` | **Not here — see § 4b** | Every corpus offset is ≈ 0, so spread never gates anything here. Nothing in this run says where a spread stops meaning "one scalar exists" |
| `max_program_gain_db` | **Not here — see § 4b** | No corpus offset comes within 20× of the 6.0 dB § 4 settles on, and none within 45× of the 12 dB local cap. Do not inherit either number from the corpus |
| ~~`program_gain_deadband_db`~~ | **dropped** | The 0.5 dB draft value was justified as a noise floor and is not one — the floor is ~0.055 dB (§ 4). What the corpus really shows is that the true offset is small everywhere, which argues for applying it, not for refusing it |
| ~~`max_chunk_drift_db`~~ | **demoted to reporting** | Calibrated at 1.5 in § 4b, then latched (§ 4): zero corpus pairs drift, and the gate is undefined under ~20 min. What the corpus *does* fix is the **statistic** wherever drift is read: `robust_drift_db`, not raw max-minus-min — raw would have failed pair 24 on a single 359-block chunk ([settled.md](settled.md) § E) |

**What this corpus can never supply.** The open knobs bind only when a single scalar does
*not* describe the pair, and this corpus has 51 pairs where one does. A corpus of matched masters
constrains the **inaction** case exactly, and the action case not at all. That is a property of the
media, not a failed run — the calibration had to move to a rung that can manufacture the other side.

---

## 4b. What Tier 2 calibrated (2026-08-26)

Four `calibration_*` cases in `crates/clip-sync-repair/tests/corpus_program_levels.rs` over the
repo's SHA-pinned public-domain masters (`#[ignore]`, ffmpeg required, ~10 min, no licensed media).
The sweep § 4a asked for — flat gains at more depths — turned out to be **degenerate**: a flat gain
is the same number on every block, so `spread_db` is exactly 0.000 at every depth and the positive
side of the threshold carries no information. Tier 3 had already supplied the "scalar exists" side
from real pairs. What was missing was the other side, so the cases manufacture it:

| fixture | a single scalar… | what it calibrates |
|---|---|---|
| flat gain ladder, both polarities | exists | the control — and where the offset stops being measurable |
| mp3 round trip on B | exists | how much of `spread_db` a codec alone explains |
| `acompressor` on B, ratio ladder | **no** — varies with *level* | `max_spread_db` |
| linear gain ramp, doubled 666 s master | **no** — varies with *time* | `max_chunk_drift_db` |

The two defects are deliberately independent: the compressor holds the program mean and varies the
offset with level; the ramp holds dynamic range and varies it with time. Each gate is therefore
calibrated against the defect it exists for, not against a fixture that trips both.

**Controls — a scalar exists.**

| case | `offset_db` | `spread_db` |
|---|---|---|
| flat ±1 / ±6 / ±12 / ±24 / ±40 dB (GRIEG, both polarities) | recovered to **0.000** at every depth | ≤ **0.0005** |
| flat ±55 dB | **abstains** (`n_pairs` < `MIN_PAIRS`) | — |
| −6 dB through an mp3 round trip | −6.001 | **0.026** |

Two readings that outlive the thresholds. The estimator is *exact* on real orchestral dynamics in
both polarities out to ±40 dB, abstaining rather than lying at ±55 — **measurability is not what
limits `max_program_gain_db`**. And a codec explains **0.026 dB** of spread, so Tier 3's median
0.668 / max 2.412 is genuine A↔B material difference, not generation loss.

**Defects — no scalar exists.**

| fixture | `offset_db` reported | `spread_db` | robust drift (raw) |
|---|---|---|---|
| compressor 2:1 | **−8.387** | **1.838** | n/a — single chunk |
| compressor 4:1 | −12.567 | 2.770 | n/a |
| compressor 8:1 | −14.653 | 3.239 | n/a |
| ramp 3 dB over 1334 s | −1.542 | 0.753 | **1.336** (2.195) |
| ramp 12 dB over 1334 s | −6.166 | 3.013 | **5.342** (8.782) |
| flat −6 dB, same 1334 s master | −6.000 | 0.000 | 0.000 (0.000) |

**The two thresholds.** Each is set to gate **no** Tier 3 corpus pair — that corpus is 51/51
known-good, so a threshold that rejects one of them is calibrated wrong — and the detection floor is
then *read off*, not chosen:

| Knob | Value | Corpus side (scalar exists) | Defect side (it does not) | Detection floor |
|---|---|---|---|---|
| `program_gain_max_spread_db` | **2.5** | max 2.412 — nothing gated | 4:1 compression at 2.770 | ≈ **4:1** compression |
| ~~`program_gain_max_chunk_drift_db`~~ | ~~1.5~~ | max robust 1.291 — nothing gated | 3 dB ramp at 1.336 | ≈ **3.4 dB** of drift across a program — but **demoted to reporting**, § 4 |

**Headline — the spread gate has a floor, and mild compression sits under it.** A 2:1 compressed B
reports a **−8.387 dB** program offset at a spread of **1.838**: inside the range of 45 of the 51
corpus pairs, and 34 of those sit at or under 1.0. **No value of `max_spread_db` that keeps this
corpus usable rejects it.** So § 4's spread check catches gross dynamics differences and cannot be
relied on for mild ones — and what it lets through is not small. It would put +8.4 dB on every fill
in that pair, the same order as the local over-boost this plan exists to remove, and the deadband
(0.5 dB) is nowhere near it.

Three consequences, all resolved in § 4:

1. **`max_program_gain_db` is load-bearing**, not a clipping guard. It is the only bound on the
   ungated case, so it is read against 8.4 dB rather than against the 12 dB local cap → **6.0**.
2. **Refuse, do not clamp.** A large offset is a known signature of an *undetected dynamics
   mismatch* and — per the controls — never of a measurement failure. Clamping would apply 6 dB of a
   number we have reason to disbelieve; refusing applies none of it. § 3 updated.
3. The stacking story (§ 5) is unaffected: this is a program-scalar failure, and local normalize is
   default-off regardless.

**A drift note.** The 3 dB ramp reads 2.195 raw against 1.336 robust — the two statistics differ by
0.86 dB on the *same* fixture. § 4's gate must read the robust one, and a threshold copied between
them is wrong by more than the corridor it sits in.


---

## 5. Two knobs, additive stacking

Independent flags:

| Knob | Default after this Change | Role |
|---|---|---|
| `program_gain` (new) | **on**, subject to § 4 | whole-program master match |
| `normalize_fill` (existing) | **off** (was on) | hole-local residual boost |

**Order is the stacking design.** Compute local against the **already
program-matched** fill, otherwise a quieter B is counted twice:

```text
# WRONG — double-counts when B is quieter than A
g_local  = compute_fill_gain(a_border_rms, rms(raw_fill), max_db)
g_total  = g_program * g_local

# RIGHT
fill_matched = raw_fill * g_program
g_local      = normalize_fill
                 ? compute_fill_gain(a_border_rms, rms(fill_matched), max_db)
                 : 1.0
g_total      = g_program * g_local          # additive in dB
```

`compute_fill_gain` stays boost-only (`[1.0, +max_fill_gain_db]`). After a
correct master match, typical-program holes see `g_local ≈ 1`. Quiet-donor
holes (15/g6, 2/g6) still get a residual boost **if** local is on. Quiet
*passages* whose 5 s A-border window reaches louder material still get the
known over-boost **if** local is on — that is why the default is off (§ 6).

Apply `g_total` in the one place `gained_fill` already multiplies
(`RegionPatch.gain`). Do not apply two sequential PCM passes.

`--no-normalize` continues to mean `normalize_fill = false` (local only). It
must **not** disable program gain. New `--no-program-gain` (and config
`program_gain = false`) is the opt-out for the scalar.

Need a CLI on-switch for local now that the default is off: `--normalize`
(clap `overrides_with = "no_normalize"`, peer of `--dual-fit` / `--no-dual-fit`).

---

## 6. Why local defaults off (and what that gives up)

The n = 3 audible defect is **local** over-boost against a non-local A
reference, not a missing master offset. After § 5's order, that defect still
fires on quiet holes if local stays default-on — the 5 s pool is unchanged.
Default-off is therefore required for this policy, not an aesthetic.

Cost, from the 2026-08-25 sweep (Q3): **15/g6 (+1.14 dB)** and **2/g6 (+0.25 dB)**
are genuine quiet-donor boosts a default-off local gives up. They remain
available via `--normalize`. They are **not** an argument to keep local on by
default once a program scalar exists — they are the residual local is for.

Q4's acceptance test was written for a *local-reference fix* that keeps those
two boosts. This plan is a **policy replacement**, so Q4 is replaced (§ 11).
Do not treat "15/g6 must still boost under the default" as a gate here.

No silent hybrid ("if program abstains, turn local back on"). Abstention is
unity. The operator can pass `--normalize` when they want the residual.

---

## 7. Who gets the scalar

| Path | `g_program` | `g_local` |
|---|---|---|
| Bracket fill (today's `compute_fill_gain` site, `region/characterize.rs`) | yes | if `normalize_fill` |
| Dual-fit / SilenceSplice (today hard-coded `gain: 1.0`) | **yes** | **no** (unchanged: never local-normalized) |
| Skip / not-applied | n/a | n/a |

Dual-fit is still B content into A. Withholding program gain would leave
rescued holes at B's master while bracket holes match A's. Local stays off on
that path: it was never designed for a dual-fit interior.

---

## 8. Reporting

`status.patched.normalize_gain` is the splice multiplier (`gained_fill`). Keep
it as **`g_total`**, unconditionally, so `fill_level` (measured on gained
samples) stays readable. Absence still means pre-2026-08-12, never unity.

Add one field, also unconditional:

| JSON | Value |
|---|---|
| `normalize_gain` | `g_total` (product; existing contract) |
| `program_gain` | `g_program` (`1.0` when abstained / flag off) |

`g_local = normalize_gain / program_gain` when `program_gain ≠ 0`. Do not add a
third field unless a consumer appears that cannot divide.

Stamp the decision once on the patch summary (not per gap): applied /
refused-and-why (`spread` / `too-large` / `channels` / `flag-off` / `no-offset`) —
one reason per row of § 4, and no `deadband` or `chunks` reason. Print
`robust_drift_db` alongside it, since § 4 reports drift rather than gating on it.
Human stdout: one line next to the existing program-levels line.

`scan.program_levels` stays the measurement. Docs today say "report-only — no
gain is derived"; that sentence is what this Change revises.

---

## 9. Config / CLI

```toml
[repair]
program_gain = true                  # new; subject to § 4
program_gain_max_spread_db = 2.5     # Tier 2 (§ 4b): corpus max 2.412, 4:1 compression 2.770
max_program_gain_db = 6.0            # § 4: refuse past it (corpus max 0.260, 2:1 phantom 8.387)
normalize_fill = false               # was true
max_fill_gain_db = 12.0              # still max local BOOST only, and a different quantity
normalize_window_secs = 5.0          # local path only
```

| Flag | Effect |
|---|---|
| `--no-program-gain` | `program_gain = false` → `g_program = 1.0` |
| `--normalize` | `normalize_fill = true` (residual, after program) |
| `--no-normalize` | `normalize_fill = false` (default after this Change) |

`max_fill_gain_db` / `normalize_window_secs` stay config-only, as today. There is deliberately no
`program_gain_deadband_db` and no `program_gain_max_chunk_drift_db` — see § 4 for why each was
dropped, and do not add either back without the trigger named there.

---

## 10. What validation must show before this is startable

From capture § 7a / § 8. This plan does not invent a second measurement.

| Gate | Status | If it fails |
|---|---|---|
| Tier 1 + 2: known-gain recoveries, correct sign (`offset_db` = B−A) | **passed** — −6.0 dB recovered on synthetic and on both real masters | do not implement — the scalar is the wrong number |
| Tier 3: `spread_db` routinely tight **and** `chunks` stable on the pairs we would patch | **passed** — 51/51 measured, 50/51 stable within 1 dB (§ 4a) | **stop** — one scalar does not exist; local-reference fix remains the live normalize work |
| Tier 3: `\|offset_db\|` distribution | **passed, as a bound on inaction** — uniformly small (max 0.260, ~5× the 0.055 dB standard error), which fixes neither threshold but proves nothing we own needs gating. Redirected to Tier 2 (§ 4a) | would set `max_program_gain_db` directly |
| Tier 2: negative controls for the mismatched-master knobs | **passed, with a caveat** — `max_spread_db` = 2.5 gates no corpus pair (§ 4b). Caveat: 2:1 compression produces an 8.4 dB phantom offset that **no usable spread threshold catches**, which is what sets `max_program_gain_db` = 6.0 as a load-bearing backstop rather than a clipping guard | a gate that cannot separate the defect from the corpus is not a gate — say so rather than pick a median |
| Tier 3 × `normalize_gain` cross-ref | **local-only, as expected** — offsets inside 0.26 dB while seven gaps demand ≥ 12 dB of local boost. The stacking story in § 5 stands unchanged | tells whether the over-boost cohort is local-only (expected) or correlated with `offset_db` (would change the stacking story) |

A wide-spread corpus is a real negative result, not a reason to apply a bad
median anyway.

---

## 11. Tests / acceptance (once startable)

**Unit / fixture (no corpus media)**

- `g_program = 10^(-offset_db/20)` polarity: `offset_db = +6` → gain `< 1`.
- § 4 refusals: each of the four rows (no offset, channels, spread, too-large) independently
  yields `1.0`.
- No deadband: a `+0.26` dB offset **applies** `≈ 0.970`, it does not round to unity.
- Refuse, do not clamp: `offset_db = -8.387` (§ 4b's 2:1 reading) yields `1.0`, **not**
  `10^(6/20)`.
- Chunk drift does not gate: a report with robust drift 5.342 and spread inside 2.5 still applies.
- Stacking: B quieter than A, local on, local computed on matched fill →
  `g_local ≈ 1`, `g_total ≈ g_program` (not `g_program²`).
- Dual-fit path receives `g_program`, not `1.0`.
- `normalize_gain` (JSON) equals `g_program * g_local`.
- Default config: `normalize_fill == false`, `program_gain == true`.
- `calibration_compressed_b_must_show_up_as_spread` asserts against the shipped `max_spread_db`
  once it exists, not the literal it was read off (`CORPUS_MAX_SPREAD_DB` today).
- `calibration_ramped_b_must_show_up_as_chunk_drift` keeps `CORPUS_MAX_ROBUST_DRIFT_DB`: with the
  drift gate demoted there is no shipped constant for it to track, and the case now documents § 4's
  latch trigger rather than a threshold.

**Acceptance vs the 2026-08-25 cohort** (same `--patch-only` roll-up, default
flags — program on, local off):

| Gap | Expect |
|---|---|
| 45/g4, 45/g5, 19/g13, 5/g6, 15/g3, 31/g7 | local over-boost **gone** (`g_local = 1`); remaining gain is only `g_program` for that pair — under 0.3 dB on every corpus pair, but no longer exactly unity now that the deadband is gone |
| 24/g46 (gunshot) | **not crushed** — still a loud restoration; `g_program` does not attenuate intra-fill dynamics |
| 15/g6, 2/g6 | **may lose** their +1.14 / +0.25 dB under the default; must **regain** them with `--normalize` |

If Tier 3 shows those over-boost pairs carry a large `offset_db`, re-read § 5
before shipping — the stacking story assumed the defect is local, which the
sweep already claims, but the cross-ref is the check.

---

## 12. Implement checklist (do not start until § 10)

**Implemented 2026-08-26.** `domain/program_gain.rs` is the whole policy; the decision is taken once
in `PatchRequestSettings::into_request` and carried on the request.

- [x] Pure helper: `program_fill_gain(levels, enabled, params) -> ProgramGainDecision` (`Applied { gain, offset_db }` / `Refused { reason }`, five reasons — the four of § 4 plus `flag-off`, since `--no-program-gain` reaches the same fallback)
- [x] Wired once in `PatchRequestSettings::into_request` from `report.program_levels`; `g_program` threaded into bracket characterize **and** carried on `GapRepairStrategy::SilenceSplice` so execute and the preview-only `outcome_from_spec` read one source
- [x] Local `compute_fill_gain` sees `rms(fill) * g_program` (`domain::stack_fill_gain`; scaling the RMS rather than the samples — `rms_interleaved` is homogeneous of degree 1 — avoids a second PCM pass)
- [x] `RegionPatch.gain` / `normalize_gain` = product; per-gap `program_gain` emitted, plus the run-level `PatchSummary::program_gain` (`ProgramGainReport`) and one `Gain:` line on human stdout
- [x] Default `normalize_fill` false; added `program_gain`, `program_gain_max_spread_db`, `max_program_gain_db` config + `--no-program-gain` / `--normalize`. **Two knobs only** — no deadband, no drift threshold (§ 4)
- [x] Dual-fit / SilenceSplice: takes `g_program`, keeps local at `1.0`
- [x] Docs: [pipeline.md](../pipeline.md) § Fill gain (rewritten to two halves), [json-output.md](../json-output.md) (`normalize_gain` is now a product, per-gap `program_gain`, new § ProgramGainReport; `program_levels` no longer "report-only"), [cli-output.md](../cli-output.md) (the `Gain:` line, and the Levels line's "no fill gain derives from it" retracted), README flags + config block
- [x] Tests in § 11; `cargo test --workspace` green; `cargo clippy` clean
- [x] Did **not** bundle with [TEMP-local-paired-gain-plan.md](TEMP-local-paired-gain-plan.md). B shoulders unread

---

## 13. Done when

A synthetic pair whose B is a constant-gain copy of A applies
`10^(-offset_db/20)` to fills; on a same-master corpus pair the applied gain
**equals the measured offset** rather than snapping to unity (the deadband is
gone, so byte-identity with today's `--no-normalize` run is no longer the test —
gain-equals-measurement replaces it); a pair reading past `max_program_gain_db`
applies unity, not the bound; `--normalize` does not square the gain; dual-fit
fills follow the same `g_program`; and the n = 3 local-over-boost gaps no longer
carry a local boost under default flags.

**Code-side: met 2026-08-26.** Every clause above that a synthetic fixture can decide is now a test —
`patch_audio_applies_the_whole_program_gain_without_local_normalize` (a −6 dB pair applies `10^(6/20)`
end to end, and the JSON's `normalize_gain / program_gain` residual is exactly 1),
`past_the_bound_refuses_rather_than_clamping`, `small_real_offsets_apply_because_there_is_no_deadband`,
`stacking_applies_the_program_offset_once_not_twice`, and
`silence_splice_outcome_carries_the_program_gain_from_the_strategy`.

**Outstanding: the § 11 acceptance table.** The three cohort rows (over-boost gone at 45/g4, 45/g5,
19/g13, 5/g6, 15/g3, 31/g7; 24/g46 not crushed; 15/g6 and 2/g6 regained under `--normalize`) need a
real-media `--patch-only` roll-up against the 2026-08-25 cohort and have **not** been run. Until they
are, this plan is implemented but not accepted — do not archive it.

---

## Related

- [TEMP-ab-program-levels-plan.md](archive/TEMP-ab-program-levels-plan.md) — measurement, trust inputs, § 8
- [TEMP-fill-normalize-local-reference-2026-08-25.md](TEMP-fill-normalize-local-reference-2026-08-25.md) — why local defaults off
- [TEMP-local-paired-gain-plan.md](TEMP-local-paired-gain-plan.md) — residual local estimator (separate Change)
- [settled.md](settled.md) § A B-relative gain; § C local-attenuation latch; § E `normalize_gain` contaminates `peak_delta_db`
- [pipeline.md](../pipeline.md) — current boost-only local normalize
