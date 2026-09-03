# TEMP — Dual-fit pre window ladder (fingerprint)

**Status:** Closed, 2026-09-03 — **no-go** (§7.5). Probe shipped and stays as a diagnostic;
grow-pre production Change is refuted by the full 51-pair corpus. See
[settled.md](settled.md) § A. One deliverable: a fingerprint probe that
re-locks the **pre** seam at several lengths, all ending at the refined start, and writes
the curve. No production fill change. Mirror of
[TEMP-dual-fit-post-window-ladder-plan.md](archive/TEMP-dual-fit-post-window-ladder-plan.md), not of
[TEMP-dual-fit-grow-post-plan.md](archive/TEMP-dual-fit-grow-post-plan.md).

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

**Anchor note:** the shipped post ladder's `post_global_r` took a nominal lag-0 anchor as a
shortcut, not the true `b_pre_seam + G` conjunct its own architecture called for (see its
Phase 2 notes). Do not mirror that shortcut here. `pre_global_r` must use the true
conjunct (`b_post_250 − G − w`, the position implied by post's *actual measured* lock),
not nominal — `step_real` only tells us whether production's own-vs-forced gate would
accept a rescued rung if it is computed against the real post lag. A nominal anchor would
make Phase 2's go/no-go read a rescue that production's actual gate might not.

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

- [x] CLI `--fingerprint-pre-window-ladder` requires `--gap-fingerprints`.
- [x] `probe_recipe.pre_window_ladder_ms` stamped only when the flag is on.
- [x] Schema sibling of `post_window_ladder`; serde skip when `None`.
- [x] Envelope-off post 250 + per-`w` `seam_local_peak` pre; `pre_global_r` at `b_post − G − w`.
- [x] 250 ms identity vs `dual_fit_at_nominal` pre; lag-0 ≠ pre@post fixture.
- [x] Quiet-before-gap fixture snaps.
- [x] Phase 1 dump; non-ladder identity.
- [x] Phase 2 tables + go / no-go.

---

## 7. Phase 2 results (2026-09-03, 51-pair corpus, 971 gaps carrying `pre_window_ladder`)

**Seam-scale line: 10 ms** (same line the grow-post plan used).

### 7.1 25/g11

| `w` (ms) | `pre_r` | `lag_minus_post_ms` |
|---|---|---|
| 250 | 0.410 | −599.9 |
| 375 | 0.214 | −727.0 |
| 500 | 0.187 | −727.0 |
| 625 | 0.168 | −727.0 |
| 750 | 0.127 | −727.0 |
| 1000 | 0.209 | **+5.75** (seam-scale) |

At `w = 1000` the lag snaps to seam-scale agreement with post, but `pre_r` (0.209) never
re-enters `[0.35, 0.5)` from above — it's still *below* 0.35, worse than the 250 ms
baseline. No rung both snaps the lag and clears the band upward. Matches the 1 s prior
(0.209) exactly. Independently corroborated: `contour.pre_flatness = 1.0` over the 3 s
pre-collar, no `energy_peak` anchor candidate on the pre side at all, and
`equivalence_production`/`equivalence_diagnostic` both classify this gap `shared_silence`
— A and B are both quiet here, not a recoverable dropout. **No-go on 25/g11.**

### 7.2 Safety set — 110 gaps (dual-fit-rescued skips, 250 ms pre `r ≥ 0.5`)

| `w` | n | p50 `\|Δw250\|` | p95 | max | movers (> 10 ms) |
|---|---|---|---|---|---|
| 625 | 110 | 0.000 ms | 0.021 ms | 0.104 ms | 0 |
| 1000 | 110 | 0.000 ms | 0.021 ms | 0.167 ms | 0 |

Already-good locks do not move — max drift across the entire safety set is 0.167 ms, two
orders of magnitude under the seam-scale line. **Safety set holds cleanly.**

### 7.3 Band — every gap with 250 ms pre `r ∈ [0.35, 0.5)` and `gate_pass`

**Size: 1.** 25/g11 is the entire population — not a sample of a larger effect, the whole
effect. It does not snap (§7.1). **Band does not rescue.**

### 7.4 RMS vs r

Pearson(`a_pre_rms_db`, `pre_r`) over all rungs/gaps = **−0.058** (n = 5826) — no
relationship. 2259 rungs are quiet (< −60 dB) yet strongly locked (`r > 0.7`); only 6 are
loud (> −40 dB) yet weakly locked (`r < 0.3`). **RMS is confirmed not a stop rule** —
level does not predict, and should not be used to predict, lock quality.

### 7.5 Verdict: **NO-GO**

Neither 25/g11 nor the band (its only member) snaps into `[0.5, 1.0]` at any ladder width,
so the plan's go condition is not met. The safety set result is a genuine positive (widening
would not have destabilized existing good locks), but it is moot without a rescue to weigh
it against. **Do not open a grow-pre production Change.** The `--fingerprint-pre-window-ladder`
probe stays as a diagnostic; this plan is closed.
