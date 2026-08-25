# TEMP — Program-offset fill gain (replace default local normalize)

**Status:** draft, 2026-08-25. **Not startable.** Gated on the capture validation
ladder in [TEMP-ab-program-levels-plan.md](TEMP-ab-program-levels-plan.md) § 7a
Tier 3 answering *does a single scalar exist?* Capture itself is already on
`GapReport` / scan JSON as `program_levels`; this plan is the first consumer.

**Tracked as:** [settled.md](settled.md) § C until that trigger fires, then
[BACKLOG.md](../../BACKLOG.md) § *Fill normalize*.

**Companions:** capture
[TEMP-ab-program-levels-plan.md](TEMP-ab-program-levels-plan.md) § 8 (the
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

**Non-goals (v1)**

- Matching fills to **B's gap shoulders** (refuted — § 2).
- Per-chunk / time-varying gain. If `chunks` disagree, **abstain** the whole
  file (unity), do not pick a bucket per gap.
- Fixing the local A-border window (5 s + 5 s energy pool). That remains the
  [BACKLOG](../../BACKLOG.md) *Make the boost reference local* item, and it only
  matters if local is re-enabled.
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
| `≈ 0` | masters matched | unity (also the deadband) |

**Attenuation is load-bearing and allowed.** A louder donor must come down to
A's program. That is a different mechanism from the latched "attenuation half
of fill normalize" (settled § C): that clamp used **whole-fill RMS vs A
borders** and crushed intra-fill dynamics (the gunshot case, 24/g46). A
whole-program scalar does not — it is constant across the fill, so a loud event
that is loud on both masters stays loud relative to its neighbours.

**Clamp.** Bound `|g_program_db|` to `max_program_gain_db`. Default is **not
invented here** — take it from the Tier 3 `|offset_db|` distribution (a starting
candidate is the existing 12 dB local cap, but only if the corpus offsets
actually live under it). Boosting a hot fill can clip; that is accepted in v1
(no limiter). Zero RMS / missing offset → `g_program = 1.0`.

---

## 4. Trust gate (fail closed → unity)

`PatchAudio` already holds `request.report: GapReport`. Read
`report.program_levels.offset` once per run, not per gap.

Apply `g_program` only when **all** of:

| Check | Fail → |
|---|---|
| `offset` is `Some` | unity (no pairing / below `MIN_PAIRS`) |
| `channels_comparable` | unity (stereo/mono confound is not a gain) |
| `spread_db ≤ max_spread_db` | unity (one scalar does not exist) |
| chunk drift `max(chunks.offset_db) − min(…) ≤ max_chunk_drift_db` | unity (offset not constant; empty `chunks` is not a fail — short programs) |
| `|offset_db| ≥ deadband_db` | unity (measurement noise, not a master) |

Thresholds are **calibrated from Tier 3**, not guessed in this draft. Until
that table exists this plan does not ship. Suggested starting names:

```text
max_spread_db
max_chunk_drift_db
program_gain_deadband_db
max_program_gain_db
```

`n_pairs` is already a capture abstention (`MIN_PAIRS = 50`); do not add a
second floor unless Tier 3 shows 50 is too low for a gain consumer.

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

Stamp the trust decision once on the patch summary (not per gap): applied /
abstained-and-why (`spread` / `chunks` / `channels` / `deadband` / `flag-off` /
`no-offset`). Human stdout: one line next to the existing program-levels line.

`scan.program_levels` stays the measurement. Docs today say "report-only — no
gain is derived"; that sentence is what this Change revises.

---

## 9. Config / CLI

```toml
[repair]
program_gain = true                 # new; subject to the trust gate
program_gain_deadband_db = ?        # from Tier 3
program_gain_max_spread_db = ?      # from Tier 3
program_gain_max_chunk_drift_db = ? # from Tier 3
max_program_gain_db = ?             # from Tier 3 |offset_db|
normalize_fill = false              # was true
max_fill_gain_db = 12.0             # still max local BOOST only
normalize_window_secs = 5.0         # local path only
```

| Flag | Effect |
|---|---|
| `--no-program-gain` | `program_gain = false` → `g_program = 1.0` |
| `--normalize` | `normalize_fill = true` (residual, after program) |
| `--no-normalize` | `normalize_fill = false` (default after this Change) |

`max_fill_gain_db` / `normalize_window_secs` stay config-only, as today.

---

## 10. What validation must show before this is startable

From capture § 7a / § 8. This plan does not invent a second measurement.

| Gate | If it fails |
|---|---|
| Tier 1 + 2: known-gain recoveries, correct sign (`offset_db` = B−A) | do not implement — the scalar is the wrong number |
| Tier 3: `spread_db` routinely tight **and** `chunks` stable on the pairs we would patch | **stop** — one scalar does not exist; local-reference fix remains the live normalize work |
| Tier 3: `|offset_db|` distribution | sets `max_program_gain_db` and the deadband |
| Tier 3 × `normalize_gain` cross-ref | tells whether the over-boost cohort is local-only (expected) or correlated with `offset_db` (would change the stacking story) |

A wide-spread corpus is a real negative result, not a reason to apply a bad
median anyway.

---

## 11. Tests / acceptance (once startable)

**Unit / fixture (no corpus media)**

- `g_program = 10^(-offset_db/20)` polarity: `offset_db = +6` → gain `< 1`.
- Trust gate: each fail row in § 4 independently yields `1.0`.
- Stacking: B quieter than A, local on, local computed on matched fill →
  `g_local ≈ 1`, `g_total ≈ g_program` (not `g_program²`).
- Dual-fit path receives `g_program`, not `1.0`.
- `normalize_gain` (JSON) equals `g_program * g_local`.
- Default config: `normalize_fill == false`, `program_gain == true`.

**Acceptance vs the 2026-08-25 cohort** (same `--patch-only` roll-up, default
flags — program on, local off):

| Gap | Expect |
|---|---|
| 45/g4, 45/g5, 19/g13, 5/g6, 15/g3, 31/g7 | local over-boost **gone** (`g_local = 1`); remaining gain is only `g_program` for that pair, which should be small if those pairs are same-master |
| 24/g46 (gunshot) | **not crushed** — still a loud restoration; `g_program` does not attenuate intra-fill dynamics |
| 15/g6, 2/g6 | **may lose** their +1.14 / +0.25 dB under the default; must **regain** them with `--normalize` |

If Tier 3 shows those over-boost pairs carry a large `offset_db`, re-read § 5
before shipping — the stacking story assumed the defect is local, which the
sweep already claims, but the cross-ref is the check.

---

## 12. Implement checklist (do not start until § 10)

- [ ] Pure helper: `program_fill_gain(offset: Option<&AbLevelOffset>, cfg) -> ProgramGainDecision` (`Applied(f32)` / `Abstained { reason }`)
- [ ] Wire once in `PatchAudio` from `request.report.program_levels`; thread `g_program` into bracket characterize **and** dual-fit execute
- [ ] Local `compute_fill_gain` sees `rms(fill * g_program)`, not raw fill RMS
- [ ] `RegionPatch.gain` / `normalize_gain` = product; emit `program_gain`
- [ ] Default `normalize_fill` false; add `program_gain` config + `--no-program-gain` / `--normalize`
- [ ] Dual-fit / SilenceSplice: stop hard-coding `1.0` for program; keep local at `1.0`
- [ ] Docs: [pipeline.md](../pipeline.md) § Fill normalization, [json-output.md](../json-output.md) (`normalize_gain` + `program_gain`; `program_levels` no longer "report-only"), README flags, scan paragraph that currently promises no gain consumer
- [ ] Tests in § 11; `cargo test`; clippy clean
- [ ] Do **not** bundle with the local-window fix. Do **not** read B shoulders

---

## 13. Done when

Default patch of a same-master pair with a trusted near-zero offset is
byte-identical to today's `--no-normalize` run; a synthetic pair whose B is a
constant-gain copy of A applies approximately `10^(-offset_db/20)` to fills;
`--normalize` on that pair does not square the gain; dual-fit fills follow the
same `g_program`; and the n = 3 local-over-boost gaps no longer carry a local
boost under default flags.

---

## Related

- [TEMP-ab-program-levels-plan.md](TEMP-ab-program-levels-plan.md) — measurement, trust inputs, § 8
- [TEMP-fill-normalize-local-reference-2026-08-25.md](TEMP-fill-normalize-local-reference-2026-08-25.md) — why local defaults off
- [settled.md](settled.md) § A B-relative gain; § C local-attenuation latch; § E `normalize_gain` contaminates `peak_delta_db`
- [pipeline.md](../pipeline.md) — current boost-only local normalize
