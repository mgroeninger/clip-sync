# TEMP — Dual-fit pre window ladder (fingerprint)

**Status:** Change plan, 2026-09-02. **Startable.** One deliverable: a fingerprint probe that
re-locks the **pre** seam at several lengths, all ending at the refined start, and writes
the curve. No production fill change. Mirror of
[TEMP-dual-fit-post-window-ladder-plan.md](archive/TEMP-dual-fit-post-window-ladder-plan.md), not of
[TEMP-dual-fit-grow-post-plan.md](TEMP-dual-fit-grow-post-plan.md).

**Tracked as:** [BACKLOG.md](../../BACKLOG.md) § *Fill placement* → *Dual-fit pre window
ladder*.

**Companions:** post ladder (shipped probe + Phase 2 go); G6
[seam-scoring.md](../seam-scoring.md) § 6; dump schema [gap-fingerprint.md](gap-fingerprint.md).
25/g11 is the named *question* (weak pre 0.410, post 0.999, `Δpre` +600 ms in
[settled.md](settled.md) § B `[0.35, 0.5)` band). It is **not** the same hint as 8/g4:
1 s `lag_decision` pre is **0.209** (dead); 8/g4's 1 s post was already 0.877.

**Media hygiene:** pair index + gap index only. `gap-files/` is gitignored; numbers below
are the durable record.

---

## 0. Architecture (normative)

This section is the plan. Later sections only implement it. If a later sentence disagrees
with this one, this one wins.

**Keep path, dual-fit extract, re-gate, and `fill_seam_search_secs` do not change. Do not
grow production pre. Measure a same-end length ladder on the pre collar during a
fingerprint dump, envelope off, ±600 ms, raw `mono(refined_start − w .. refined_start)`.**

```text
existing splice_dualfit (unchanged, 250 ms, may envelope-rescue)
  → if --fingerprint-pre-window-ladder:
       post lock = envelope-off 250 ms (production dual_fit_at_nominal)
       for w in {250, 375, 500, 625, 750, 1000} ms:
         pre lock = seam_local_peak(A[start−w .. start], B around mapped_start ±600 ms)
         pre@post = Pearson(A[start−w .. start], B[b_post_250 − G − w .. b_post_250 − G])
         record r, lag, z, prom, pre@post, step_real
```

`pre@post` is the other-shoulder lock, **not lag 0**. The post ladder scored
`post_global_r` at nominal `b_mapped_end`; do not copy that. Production `try_dual_fit`
uses `b_pre_seam + G` for post@pre; this probe's symmetric is `b_post_seam − G − w`.

`--no-dual-fit` is irrelevant: dump-only. Flag off → field absent, default dumps
byte-stable.

One sentence: **if the 250 ms pre template is quiet before the hole, growing it back from
the join might make the lock real. Measure that before touching production pre.**

---

## 1. Why a dump, not production grow-pre

25/g11: dual-fit pre **0.410**, post **0.999**, `Δpre` **+600 ms**, `trim_frames` 28796,
`gate_pass` true, `dual_fit_rescue` false, keep-path `waveform_floor`. Growing **post**
cannot help (ladder post r 0.999 at every `w`, `Δpre` stuck at +600).

That is the mirror *question* of 8/g4, not the mirror *evidence*. 1 s pre peak_r **0.209**
— growing to 1 s may still be dead. Do not ship grow-pre on 8/g4's success.

Safety is the same corpus question the post ladder answered: on gaps whose 250 ms **pre**
already locks (`r ≥ 0.5`), does a longer window keep that lag?

---

## 2. Object

**Must explain:**

| Gap | Why |
|---|---|
| 25/g11 | Weak pre, huge `Δpre`. Does any `w` snap pre lag toward post and leave `[0.35, 0.5)`? |
| Other 250 ms pre in `[0.35, 0.5)` with `gate_pass` | Same columns. Name the set in Phase 2; do not assume n=1 |

**Must not treat as this probe:** 8/g4 (post), 11/g9 (abut), grow-post production.

**Safety set:** fingerprint skips with `dual_fit_rescue` and 250 ms **pre** r ≥ 0.5.
At the `w` that would "fix" 25/g11 (if one exists), `|pre_lag(w) − pre_lag(250)|` p95 must
be seam-scale. If 25/g11 never snaps, the dump still answers "no production grow-pre."

---

## 3. Probe contract

### 3.1 Flag and recipe

CLI: `--fingerprint-pre-window-ladder` (requires `--gap-fingerprints`). Independent of
`--fingerprint-post-window-ladder` and of `--fingerprint-diagnostics`. Frozen list, stamped
on `source.probe_recipe.pre_window_ladder_ms`: `[250, 375, 500, 625, 750, 1000]`.

Both flags may be on in one run. Fields are siblings, not a combined object.

### 3.2 What each rung measures

Envelope **off**. Raw join. `seam_local_peak`, `max_lag` = `SEAM_LOCAL_SEARCH_MS` (600 ms).
A pre = `interleaved_to_mono(A[refined_start − w .. refined_start])`. B search centered on
nominal `b_mapped_start`.

**Post is not grown.** Post lock for `pre@post` / `step_real` is the envelope-off **250 ms**
post. Do not `SeamLockTemplates::build(w)` for `w ≠ 250`.

Omit a rung if `start < w` or B is OOB (`None` that rung, not a fake 0). `None` for the
whole field only when even the 250 ms collar cannot be measured.

| Field | Meaning |
|---|---|
| `window_ms` | `w` |
| `pre_r` / `pre_lag_ms` / `pre_z` / `pre_prom` | `seam_local_peak` of this template |
| `pre_global_r` | this template vs `B[b_post_250 − G − w .. b_post_250 − G]` (`NaN` if OOB) |
| `step_real` | this rung's `pre_r` vs `pre_global_r + 0.15` (`partial_cmp` as `try_dual_fit`) |
| `lag_minus_post_ms` | `pre_lag − post_lag_250` (both vs their nominals). Same quantity as post ladder `lag_minus_pre_ms`, opposite name |
| `lag_minus_w250_ms` | `pre_lag(w) − pre_lag(250)` |
| `a_pre_rms_db` | cheap; RMS is not a stop rule on the post side |

JSON: `gaps[].pre_window_ladder` (`Option<Vec<…>>`), sibling of `post_window_ladder` /
`splice_dualfit`.

**`step_real`:** own = this rung's `pre_r`. Forced = `pre_global_r` (pre template at the
post lock, gap length back). Do not score lag 0. Phase 2 still reads `lag_minus_post_ms`
as the production-like step, same as the post dump.

### 3.3 Identity

250 ms rung matches `dual_fit_at_nominal` **pre** `r` / `lag` (envelope off). May disagree
with `splice_dualfit` when envelope rescued pre — stamp that, do not turn the ladder's
envelope on.

### 3.4 Not this probe

- Production grow-pre (successor only after Phase 2 go).
- Walk-off / earlier 250 ms / max-energy tiles.
- Changing `fill_seam_search_secs`.
- Growing post (other flag / other Change).

---

## 4. Phases

### Phase 0 — wire the probe

Schema + recipe + CLI + unit test: quiet ~300 ms *before* the cut, then speech further
back; 250 ms junk pre lag; ≥500 ms snaps toward a live post. 250 ms identity vs
`dual_fit_at_nominal` pre.

### Phase 1 — licensed dump

Complete-corpus pass. Non-ladder identity vs `2026-08-29-complete-corpus` /
`2026-09-01-dual-fit-ladder-corpus` (`splice_dualfit`, `lag_decision`, `outcome`,
`post_window_ladder` if that flag is off). May share a run with
`--fingerprint-post-window-ladder`; then post-ladder fields must match the 09-01 dump.

### Phase 2 — read the dump (this plan's "done")

1. **25/g11** — first `w` where `|lag_minus_post_ms|` is seam-scale and `pre_r` leaves
   `[0.35, 0.5)`. If none, write **no-go** for grow-pre (1 s pre 0.209 is the prior).
2. **Safety set** (250 ms pre r ≥ 0.5 on dual-fit-rescued skips): `|lag_minus_w250_ms|`
   p50 / p95 / max at 625 and 1000. Movers named.
3. **Band** — every gap whose 250 ms pre is in `[0.35, 0.5)` and `gate_pass`.
4. **RMS vs r** — confirm RMS is not a stop rule.

Go for a *later* grow-pre Change only if 25/g11 (or the band) snaps **and** the safety set
stays. No-go: never snaps, or high-r pres move.

---

## 5. Acceptance

- Flag off: dumps unchanged in ladder-free fields.
- Flag on: `pre_window_ladder` present; 250 ms identity with `dual_fit_at_nominal` pre;
  `pre_global_r` is pre@post, not lag 0 (test pins a fixture where those two differ).
- Phase 1 dump under `gap-files/`.
- Phase 2 tables + go / no-go in this doc.

---

## 6. Checklist (implement)

- [ ] CLI `--fingerprint-pre-window-ladder` requires `--gap-fingerprints`.
- [ ] `probe_recipe.pre_window_ladder_ms` stamped only when the flag is on.
- [ ] Schema sibling of `post_window_ladder`; serde skip when `None`.
- [ ] Envelope-off post 250 + per-`w` `seam_local_peak` pre; `pre_global_r` at `b_post − G − w`.
- [ ] 250 ms identity vs `dual_fit_at_nominal` pre; lag-0 ≠ pre@post fixture.
- [ ] Quiet-before-gap fixture snaps.
- [ ] Phase 1 dump; non-ladder identity.
- [ ] Phase 2 tables + go / no-go.
