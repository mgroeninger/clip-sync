# TEMP — Dual-fit post window ladder (fingerprint)

**Status:** Change plan, 2026-09-01. **Phase 2 done.** Probe shipped. Production grow and
pre-collar dump are successor TEMPs, not this deliverable.

**Tracked as:** [BACKLOG.md](../../BACKLOG.md) § *Fill placement* → *Dual-fit post window
ladder*.

**Companions:** G6 [seam-scoring.md](../seam-scoring.md) § 6; dump schema
[gap-fingerprint.md](gap-fingerprint.md); rigid-fill (the **other** G6 error, `!step_real`)
[TEMP-seam-lock-rigid-fill-plan.md](archive/TEMP-seam-lock-rigid-fill-plan.md); 1 s vs 250 ms
disagreement on 8/g4 in [settled.md](settled.md) § B (`[0.35, 0.5)` band). Dual-fit's
construction [archive/TEMP-seam-splice-dualfit-plan.md](archive/TEMP-seam-splice-dualfit-plan.md)
§ 0. Successors: [TEMP-dual-fit-grow-post-plan.md](TEMP-dual-fit-grow-post-plan.md),
[TEMP-dual-fit-pre-window-ladder-plan.md](TEMP-dual-fit-pre-window-ladder-plan.md).

**Media hygiene:** pair index + gap index only. `gap-files/` is gitignored; numbers below
are the durable record.

---

## 0. Architecture (normative)

This section is the plan. Later sections only implement it. If a later sentence disagrees
with this one, this one wins.

**Keep path, dual-fit extract, re-gate, and `fill_seam_search_secs` do not change. Do not
flip the default 250 ms window. Measure a same-start length ladder on the post collar
during a fingerprint dump, envelope off, ±600 ms, raw `mono(refined_end + w)`.**

```text
existing splice_dualfit (unchanged, 250 ms, may envelope-rescue)
  → if --fingerprint-post-window-ladder:
       pre lock = envelope-off 250 ms (production dual_fit_at_nominal)
       for w in {250, 375, 500, 625, 750, 1000} ms:
         post lock = seam_local_peak(A[end .. end+w], B around mapped_end ±600 ms)
         post@pre = Pearson(A[end .. end+w], B[pre_seam + G .. pre_seam + G + w])
         record r, lag, z, prom, post@pre, step_real
```

`--no-dual-fit` is irrelevant: this is dump-only. Default fingerprint dumps stay byte-stable
when the flag is off (`skip_serializing_if` / absent field).

One sentence: **the 250 ms post template can be quiet after the hole; growing it from the
join can make the lock real. Measure that on the corpus before changing the lock.**

---

## 1. Why a dump, not a production Change

8/g4 was dual-fit patched at post **0.390** with trim **−286 ms**. Ear: slight hole at the
end, missing syllable. 1 s `lag_decision` post is **0.877 @ +75 ms**, same lag as pre
(~+74 ms, ~2 ms step). Dual-fit never reads `lag_decision`; it locked a weak 250 ms peak
and treated it as a length step.

On the 2026-09-01 listen clips (16-bit, channel-average mono, same ±600 ms), same-start
windows from the join:

| `w` | peak r | lag | vs today's lock |
|---|---:|---:|---|
| 250 ms | 0.390 | −213 ms | today's class (dump r matches; lag ±70 ms vs production −286, 16-bit clip) |
| 375 ms | 0.481 | **+75 ms** | right place, still weak |
| 500 ms | 0.682 | +75 ms | real lock |
| 625 ms | 0.873 | +75 ms | matches 1 s post |
| 750 / 1000 ms | 0.874 / 0.877 | +75 ms | no further gain |

Pre on those clips: 0.985 @ +74 ms. Once the post window contains the returning speech, it
agrees with pre → `!step_real` → extract is `pre+G` (the syllable stays). Whole-window RMS
does **not** pick `w` (250 and 500 are both ~−55 dBFS). Stop on lag agreement + r, not energy.

That is n=1 and 16-bit. The corpus question the clips cannot answer: on the **83** that
already lock at 250 ms, does a longer window **keep that lag**, or pull to a later landmark
and mint new false steps?

A global `fill_seam_search_secs = 0.5` run confounds keep-path seams, re-gate, and trim
edge guards. This probe is the lock only.

Re-gate is **out of scope**. Even a correct `pre+G` fill can still fail today's post gate
(8/g4 `post@pre` on the 250 ms template was −0.07). Fingerprint will not show that. After
go, a successor Change owns lock **and** gate; this plan owns the curve.

---

## 2. Object

**Must explain (fingerprint, not a convert set):**

| Gap | Why it is on the ladder |
|---|---|
| 8/g4 | False `step_real` from a dead 250 ms post; listen clips say 500–625 ms snaps |
| 20/g3 | Same `[0.35, 0.5)` post band (0.443). Unheard. Same mechanism or not? |
| 22/g13 | `gate_pass` false (250 ms post dead). Does a longer window revive it? |

**Must not treat as this probe:**

| Gap | Why |
|---|---|
| 25/g11 | Weak **pre** (0.410), not post. Growing post does not address it |
| 11/g9 | Abut / collapse plan |
| The five rigid remainder | `!step_real` already; [TEMP-seam-lock-rigid-fill-plan.md](archive/TEMP-seam-lock-rigid-fill-plan.md) |

**Safety set:** production dual-fit length-edits (the 83). At the `w` that fixes 8/g4,
`post_lag(w)` must stay on `post_lag(250)` (see §5). If it does not, do not grow in
production.

---

## 3. Probe contract

### 3.1 Flag and recipe

CLI: `--fingerprint-post-window-ladder` (requires `--gap-fingerprints`). No length
arguments in v1. Frozen list, stamped on `source.probe_recipe.post_window_ladder_ms`:
`[250, 375, 500, 625, 750, 1000]`.

Bulk: `measure-gap-fingerprints.ps1 -ScanArgs "--fingerprint-post-window-ladder"` on
`gap-files/2026-08-29-complete-corpus`'s manifest (or the current complete dump's pair
list). Do **not** require `--fingerprint-diagnostics`.

Off: field absent. Old corpora deserialize. Default dumps / goldens that do not pass the
flag do not grow.

### 3.2 What each rung measures

Envelope **off**. Raw join. `seam_local_peak`, `max_lag` = `SEAM_LOCAL_SEARCH_MS` (600 ms).
A post = `interleaved_to_mono(A[refined_end .. refined_end + w])`. B search centered on
nominal `b_mapped_end`.

**Pre is not grown.** Pre lock for `post@pre` / `step_real` is the envelope-off **250 ms**
pre from `nominal_first_seam_locks` / `dual_fit_at_nominal` (production, not the
fingerprint envelope). Do not call `SeamLockTemplates::build(w)` for `w ≠ 250`: that type
requires `a_pre.len() == a_post.len()`, and widening pre is a different experiment.

Per rung, omit if `end + w` runs off A or the B window is OOB (`None` that rung, not a
fake 0).

| Field | Meaning |
|---|---|
| `window_ms` | `w` |
| `post_r` / `post_lag_ms` / `post_z` | `seam_local_peak` of this template |
| `post_prom` | ±30 ms prominence, same helper as `splice_dualfit` |
| `post_global_r` | this template vs `B[b_pre_250 + G .. + w]` (`NaN` if OOB) |
| `step_real` | `post_r` vs `post_global_r + DUALFIT_STEP_REAL_MARGIN` (0.15), same `partial_cmp` refuse as `try_dual_fit` |
| `lag_minus_pre_ms` | `post_lag − pre_lag_250` (both vs their nominals) |
| `lag_minus_w250_ms` | `post_lag(w) − post_lag(250)` |

Optional but cheap: `a_post_rms_db` of the template (the listen clips showed RMS will not
choose `w`; keep it so that stays visible on the corpus).

JSON home: `gaps[].post_window_ladder` (`Option<Vec<…>>`), sibling of `splice_dualfit`,
not inside it. `splice_dualfit` stays the 250 ms production-predictor with envelope.

### 3.3 Identity

The 250 ms rung is **production-like** (`EnvelopeSearch::off()`). It must match
`dual_fit_at_nominal`'s post `r` / `lag` on the same decode (test on a fixture + one
licensed gap). It is allowed to disagree with `splice_dualfit` when envelope rescued the
250 ms post — stamp that, do not "fix" it by turning the ladder's envelope on.

### 3.4 Not this probe

- Walk-off / later 250 ms / max-energy tiles (locks the speech, not the join).
- Max-r across start offsets.
- Changing `fill_seam_search_secs` on keep-path or re-gate.
- Production grow-on-weak, rigid-from-pre, or skip-on-weak-post.
- Growing the **pre** collar.

---

## 4. Phases

### Phase 0 — wire the probe

Schema + `probe_recipe` + `splice_dualfit_at` neighbour (or a dedicated function next to
it) + CLI + a unit test that a quiet-then-speech post snaps lag between 250 and 625 ms
and that 250 ms matches `dual_fit_at_nominal`. No corpus run in this phase.

### Phase 1 — licensed dump

One complete-corpus pass with the flag. Compare unshifted identity of **non-ladder**
fields against `2026-08-29-complete-corpus` (flag must not move `splice_dualfit` /
`lag_decision` / `outcome`).

### Phase 2 — read the dump (this plan's "done")

Tables, pair/gap only:

1. **8/g4, 20/g3, 22/g13** — first `w` where `|lag_minus_pre_ms|` is seam-scale (~1 s
   agreement) and `post_r` leaves `[0.35, 0.5)`. Does `step_real` flip false?
2. **The 83** — at that `w` (and at 625 / 1000), distribution of `|lag_minus_w250_ms|`.
   p50 / p95 / max. How many would change `step_real`?
3. **Band** — every gap whose 250 ms post is in `[0.35, 0.5)` and `gate_pass`. Same
   columns as (1).
4. **RMS vs r** — confirm whole-template RMS is not a stop rule on more than 8/g4.

**Dump:** `gap-files/2026-09-01-dual-fit-ladder-corpus` (51 pairs, 1073 gaps, 971 with all
six rungs; 102 have no ladder — same as no `splice_dualfit`). Recipe
`post_window_ladder_ms = [250, 375, 500, 625, 750, 1000]`.

**Phase 1 identity vs `2026-08-29-complete-corpus`:** `splice_dualfit.post_seam_r` max
|Δ| = 0 (n=971); `outcome.tier` 0/1049 disagree; `lag_decision` post `peak_r` max |Δ| = 0
(n=998). Flag did not move non-ladder fields.

**250 ms identity vs `splice_dualfit`:** `post_r` p95 |Δ| = 0. Envelope-off vs
envelope-on outliers are pair 31 (g7–g11) and 11/g9 — expected (§3.3), not a probe bug.

**Do not read dumped `step_real` / `post_global_r` as production's conjunct.** The probe
scores `post_global_r` at **nominal** `b_mapped_end` (lag 0), not at `b_pre_250 + G`
(plan §3.2). Smoking gun 8/g4 250 ms: ladder `post_global_r` = 0.152 vs
`splice_dualfit.post_seam_global_r` = −0.070. 941/971 rungs disagree by >0.02. Dumped
`step_real` stays true whenever the peak beats lag-0, including after 8/g4's lag has
snapped to pre. Production `step_real` is `|lag_minus_pre_ms|` (and
`splice_dualfit.post_seam_global_r` at 250 ms).

#### 1. Named gaps

| Gap | 250 ms | First `|Δpre|<10` ms and `post_r≥0.5` | Production-like step |
|---|---|---|---|
| 8/g4 | r 0.390 @ −213 ms, `Δpre` −286 ms (matches `trim_frames` −13738) | **500 ms** (lag already +75 / `Δpre` +1.8 at **375 ms**, r still 0.481) | false from 375 ms (`Δpre` 1.8 ms) |
| 20/g3 | r 0.443, `Δpre` **0**, `trim` −1 | 375 ms (r only — lag was already pre) | already false (df Δ 0.080 < 0.15). Extra `!step_real` beyond the five remainder |
| 22/g13 | r 0.271, `gate_pass` false, `Δpre` −7.9 ms | **1000 ms** (r 0.991; lag never moved) | already ~rigid; dead post until 1 s of program is in the template |
| 25/g11 | post r 0.999, `Δpre` **+600 ms** | never (weak **pre**) | not this probe |

Listen clips predicted 8/g4 exactly (250 ms −212.6 ms / 0.390 → 625 ms +75.4 / 0.873).

#### 2. Length-edit safety set

Fingerprint `skip ∧ dual_fit_rescue ∧ splice_dualfit step_real`: **105** (predictor, not
the 83 production patches — rescue over-predicts re-gate; conjunct 4 is already dropped).
Conservative: larger than 83.

`|lag_minus_w250_ms|` on those 105:

| `w` | p50 | p95 | max | `≥5` ms | `≥50` ms |
|---|---:|---:|---:|---:|---:|
| 500 | 0.00 | 0.02 | 288 | 2 | 1 |
| 625 | 0.00 | **0.02** | 288 | 2 | 2 |
| 1000 | 0.00 | 0.02 | 288 | 2 | 2 |

The two movers at 625/1000: **8/g4** (+288 ms, the target) and **31/g9** (envelope-off
post r 0.13, noise lag; `splice_dualfit` envelope-rescued 0.90 — not a production
`dual_fit_at_nominal` lock).

`|Δpre|<5` ms on this set: 48/105 at 250 ms, **49/105** at 375+. The one new agreement is
8/g4. The other ~56 keep a real step at every `w` — growing post does not collapse genuine
length edits.

#### 3. Band (`gate_pass` and 250 ms post r in `[0.35, 0.5)`)

Only **8/g4** and **20/g3**. (25/g11's band member was the pre shoulder.)

#### 4. RMS

argmax(RMS) = argmax(r) on 342/971. 8/g4 RMS stays ~−54 to −55 dBFS from 250–1000 ms while
r goes 0.39 → 0.88. Not a stop rule.

#### Go / no-go

**Go** for a successor that grows the dual-fit **post** template when 250 ms is weak /
`Δpre` is not seam-scale (8/g4). The 105 stay (p95 0.02 ms at 625). 20/g3 is already
rigid (r climb is a gate window, not a lock). 22/g13 stays dead unless `w` reaches 1 s.

**Not go** for "always 625 ms" as the first Change — inert on the 83, but 31/g9-class
dead envelope-off posts would wander, and 22/g13 still needs 1 s. Re-gate is still
untested (Phase 3 / ear on 8/g4).

### Phase 3 — not this plan

Production grow-post: [TEMP-dual-fit-grow-post-plan.md](TEMP-dual-fit-grow-post-plan.md).
Pre collar dump: [TEMP-dual-fit-pre-window-ladder-plan.md](TEMP-dual-fit-pre-window-ladder-plan.md).

---

## 5. Acceptance

- Flag off: fingerprint dump identical in ladder-free fields to today (existing tests).
- Flag on: `post_window_ladder` present; 250 ms rung identity with
  `dual_fit_at_nominal` post; omitted rungs are absent, not zero.
- Phase 1 dump exists under `gap-files/` (gitignored).
- Phase 2 tables in this doc (or archived findings) with a go / no-go sentence.

---

## 6. Checklist (implement)

- [x] CLI `--fingerprint-post-window-ladder` requires `--gap-fingerprints`; reject otherwise.
- [x] `probe_recipe.post_window_ladder_ms` stamped only when the flag is on.
- [x] Schema field + serde skip when `None`; harness/analyzer ignore unknown-absent.
- [x] Envelope-off pre 250 + per-`w` `seam_local_peak` post; do not widen `SeamLockTemplates`.
- [x] 250 ms identity test vs `dual_fit_at_nominal`.
- [x] Fixture: post collar quiet ~300 ms then speech → 250 ms junk lag, ≥500 ms snaps.
- [x] Phase 1 complete-corpus run; non-ladder identity vs 08-29 dump.
- [x] Phase 2 tables + go / no-go.
