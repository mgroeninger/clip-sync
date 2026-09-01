# TEMP — Dual-fit post window ladder (fingerprint)

**Status:** Change plan, 2026-09-01. **Startable.** One deliverable: a fingerprint probe that
re-locks the **post** seam at several lengths, all starting at the refined end, and writes
the curve. No production fill change.

**Tracked as:** [BACKLOG.md](../../BACKLOG.md) § *Fill placement* → *Dual-fit post window
ladder*.

**Companions:** G6 [seam-scoring.md](../seam-scoring.md) § 6; dump schema
[gap-fingerprint.md](gap-fingerprint.md); rigid-fill (the **other** G6 error, `!step_real`)
[TEMP-seam-lock-rigid-fill-plan.md](archive/TEMP-seam-lock-rigid-fill-plan.md); 1 s vs 250 ms
disagreement on 8/g4 in [settled.md](settled.md) § B (`[0.35, 0.5)` band). Dual-fit's
construction [archive/TEMP-seam-splice-dualfit-plan.md](archive/TEMP-seam-splice-dualfit-plan.md)
§ 0.

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

Write the tables into this doc (or a successor findings note) and a go / no-go for a
production Change.

Go: 8/g4 snaps; the 83 stay (p95 `|lag_minus_w250_ms|` at the chosen `w` is seam-scale,
not speech-offset-scale); 20/g3 / 22/g13 named as convert / still-dead / unknown.

No-go: the 83's lags move, or growing post creates more false `step_real` than it
removes.

### Phase 3 — not this plan

If go: a **separate** Change. Candidates (pick from the tables, do not pre-commit):

- Grow the dual-fit **post** template only when 250 ms r is weak / lag disagrees with pre.
- Always use a longer post window (only if Phase 2 says the 83 are inert).
- Refuse weak post (brake) and take `pre+G` (`!step_real` extract) — re-gate still uses
  250 ms and may skip; that Change must say so.

Ear 8/g4 (and any Phase 2 convert) on a patch run. Fingerprint cannot.

---

## 5. Acceptance

- Flag off: fingerprint dump identical in ladder-free fields to today (existing tests).
- Flag on: `post_window_ladder` present; 250 ms rung identity with
  `dual_fit_at_nominal` post; omitted rungs are absent, not zero.
- Phase 1 dump exists under `gap-files/` (gitignored).
- Phase 2 tables in this doc (or archived findings) with a go / no-go sentence.

---

## 6. Checklist (implement)

- [ ] CLI `--fingerprint-post-window-ladder` requires `--gap-fingerprints`; reject otherwise.
- [ ] `probe_recipe.post_window_ladder_ms` stamped only when the flag is on.
- [ ] Schema field + serde skip when `None`; harness/analyzer ignore unknown-absent.
- [ ] Envelope-off pre 250 + per-`w` `seam_local_peak` post; do not widen `SeamLockTemplates`.
- [ ] 250 ms identity test vs `dual_fit_at_nominal`.
- [ ] Fixture: post collar quiet ~300 ms then speech → 250 ms junk lag, ≥500 ms snaps.
- [ ] Phase 1 complete-corpus run; non-ladder identity vs 08-29 dump.
- [ ] Phase 2 tables + go / no-go.
