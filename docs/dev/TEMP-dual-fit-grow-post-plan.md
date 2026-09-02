# TEMP — Dual-fit grow post (production)

**Status:** Change plan, 2026-09-02. **Startable.** One deliverable: when the 250 ms post
lock is uninformative, re-lock post at a longer same-start window, then assemble and
stamp as today. Keep path and `fill_seam_search_secs` do not change. Do not grow pre.

**Tracked as:** [BACKLOG.md](../../BACKLOG.md) § *Fill placement* → *Dual-fit grow post*.

**Companions:** measurement
[TEMP-dual-fit-post-window-ladder-plan.md](archive/TEMP-dual-fit-post-window-ladder-plan.md)
(Phase 2 **go**); dump `gap-files/2026-09-01-dual-fit-ladder-corpus`; G6
[seam-scoring.md](../seam-scoring.md) § 6; rigid-fill (the other G6 error)
[archive/TEMP-seam-lock-rigid-fill-plan.md](archive/TEMP-seam-lock-rigid-fill-plan.md).
Pre-side *question* is a dump, not this Change:
[TEMP-dual-fit-pre-window-ladder-plan.md](TEMP-dual-fit-pre-window-ladder-plan.md).

**Media hygiene:** pair index + gap index only. `gap-files/` is gitignored; numbers below
are the durable record.

---

## 0. Architecture (normative)

This section is the plan. Later sections only implement it. If a later sentence disagrees
with this one, this one wins.

**Keep-path search does not change. Dual-fit extract stays `step_real` / `!step_real` on
the templates that were actually used. Do not replace that fork with `|lag_minus_pre_ms|`.
`|Δpre|` and post `r` decide whether to grow and when to stop.**

**Locked before coding (the visit that exists, not the ladder):**

1. **Admit on the locks already in hand.** Production 250 ms locks are **not**
   envelope-off. `characterize_region` calls `nominal_first_seam_locks` with the envelope;
   `skip_or_dual_fit` consumes those locks. 8/g4 is safe either way (0.390 ≥ triage, so
   the envelope cannot move it). 31/g9 is not: envelope-off post is ~0.13 and wanders;
   production post is the rescued ~0.90 that already patches. Grow rungs are envelope-off.
   If no rung stops, **keep those original locks** (including an envelope rescue). Do not
   replace 250 ms with a fresh envelope-off re-measure.

2. **Do not relax `SeamLockTemplates`.** Its invariant is `a_pre.len() == a_post.len() ==
   seam_window_frames`. `try_dual_fit` scores `post@pre` with `templates.a_post()` and that
   single `w`. Do not call `SeamLockTemplates::build(w)` for `w ≠ 250`. Cut the grown A
   post the same way `post_window_ladder_at` already does (raw slice + `seam_local_peak`).
   Thread that `a_post` into `post@pre` / `step_real` only. Residual post window = chosen
   `w`; pre residual stays 250 ms — `measure_dual_fit_residual_verdict` today uses one
   `seam_window_frames` for both sides and must split.

3. **Grow before `dual_fit_from_locks`'s `gate_pass` reject.** Hook after a successful
   `dual_fit_from_locks` and 8/g4 never grows (it already returns `Some` with the junk
   250 ms lag). Hook only on `None` and 22/g13 never grows (`gate_pass` is already false).
   After swapping post, recompute `b_post_seam` / `bridge_frames` / `gate_pass`, then fit.
   No-stop ⇒ original 250 ms locks, including a failing `gate_pass`.

```text
characterize_region: nominal_first_seam_locks (envelope on) → locks in hand
skip_or_dual_fit (scored skip, dual-fit on)
  → if post_r < 0.5:                          // original locks, not a re-measure
       for w in {375, 500, 625, 750, 1000} ms, same start, envelope off, ±600 ms:
         lock post = raw slice + seam_local_peak
         stop at the shortest w with post_r ≥ 0.5
              and, if 250 ms |Δpre| > 10 ms, |Δpre| ≤ 10 ms
       if a rung stops → swap post; recompute b_post_seam, bridge, gate_pass
       else → original locks (rescue included)
  → dual_fit_from_locks (gate_pass / bridge reject, then try_dual_fit)
       post@pre uses the post template that was actually locked (grown slice or
       templates.a_post()), at b_pre + G, not lag 0
  → extract, residual (post window = chosen w, pre = 250), classify(lock r)
```

`--no-dual-fit` still short-circuits the whole visit. No new CLI flag. Frozen `w` list is
the ladder recipe, not `fill_seam_search_secs`.

One sentence: **8/g4's post join is quiet for 250 ms; lock the same start with enough
program that Pearson agrees with pre, then take `pre+G`.**

---

## 1. Why this, not always-625 and not `|Δpre|` as extract

Phase 2 ([TEMP-dual-fit-post-window-ladder-plan.md](archive/TEMP-dual-fit-post-window-ladder-plan.md)):

- 8/g4 250 ms: r 0.390 @ −213 ms, `Δpre` −286 ms (the −286 ms trim). 375 ms snaps lag to
  +75 (`Δpre` +1.8). 500 ms r 0.682. 625 ms r 0.873. Implied production `step_real` false
  from 375 ms.
- Safety set (105 fingerprint length-edit predictors): at 625 ms, `|Δw250|` p95 **0.02 ms**.
  Genuine steps stay steps at every `w`.
- Always-625 was **not go**: admission is **post_r < 0.5** on the locks in hand, not
  `|Δpre|` large (that would enter the grow loop on real length-edits). Replacing those
  locks with a fresh envelope-off 250 ms would wander 31/g9 (§0 lock 1).
- Whole-template RMS does not pick `w`. Stop on `r` and `|Δpre|`, not energy.

`|Δpre|` is the grow/stop signal. After a stop, production `step_real` (`post_r` vs
`post@pre` on **that** `w`) should go false on 8/g4 because the lags already agree. Extract
stays the existing fork. The ladder dump's `step_real` is **not** this conjunct (it scored
lag 0); do not copy that bug. `try_dual_fit` already uses `b_pre_seam + G`.

Dual-fit re-gate is already the **lock scores** (`skip_or_dual_fit` does not
`fill_splice_seam_correlations_interleaved` on fill head/tail). Growing the lock *is* the
seam gate. Do not invent a second post Pearson at the quiet 250 ms hole. Residual post
window follows the chosen `w` (same start); pre residual stays 250 ms.

---

## 2. Object

| Gap | Expected |
|---|---|
| 8/g4 | Grow to 500 ms (lag already right at 375, r still 0.481). `!step_real`, `pre+G`, syllable stays. **Must ear.** |
| 20/g3 | Already `Δpre` 0, trim −1, r 0.443. Grows to 375 ms for r 0.513. Fill should be the same rigid extract. Ear optional |
| 22/g13 | 250 ms `gate_pass` false (r 0.271), `Δpre` −7.9 ms. May convert at **1000 ms** (r 0.991). **Ear if it patches.** BACKLOG still lists it as a skip remainder |
| 25/g11 | Weak **pre**. This Change must not move it |
| The 83 / 105 | Do not enter grow (`post_r ≥ 0.5` on the locks in hand). Byte-identical dual-fit fills |
| 31/g9 | **Already a production patch** (envelope-rescued post ~0.90). Locks in hand have `r ≥ 0.5` → do not grow. The ladder's envelope-off ~0.13 / wander is not this visit |

11/g9 stays out (`bridge_frames` ≤ 0). The five/six rigid remainder already `!step_real`
at 250 ms; post_r is high except 20/g3.

---

## 3. Behaviour change

### 3.1 Where

On the locks `characterize_region` already measured (envelope on), **before**
`dual_fit_from_locks`'s `gate_pass` reject (§0 locks 1 and 3). Fingerprint
`post_window_ladder` is diagnostic; production grow rungs re-measure envelope-off via
`seam_local_peak` on a raw A slice, not via `SeamLockTemplates::build` (§0 lock 2). Do not
read the dump at patch time. Do not re-measure a 250 ms envelope-off pair to "admit."

### 3.2 Stop rule (locked)

Shortest `w` in `{375, 500, 625, 750, 1000}` such that `post_r ≥ 0.5`, **and** if the 250 ms
`|Δpre|` was `> 10` ms, that rung's `|Δpre| ≤ 10` ms. Else keep the original locks.

10 ms is the Phase 2 snap line (seam-scale, not speech-offset-scale). 0.5 is leave the
`[0.35, 0.5)` band — 0.35 is what let 8/g4's junk 0.390 through.

### 3.3 Stamp

`dual_fit_used` unchanged. When a grown `w` was adopted: JSON `status.patched.dual_fit_post_window_ms`
(absent = 250). Human `-v` may say `dual-fit post 500ms`. Follow
[json-output.md](../json-output.md) § *Revision procedure*.

### 3.4 Feature docs (standing, present tense)

This process is a facet of dual-fit, not a new mode and not a changelog. Same commit as the
code: update the operator/wire docs so a reader who never opens this TEMP learns how G6
places a quiet post join. Present tense. No “this Change”, no pair/gap indices, no ladder
tables.

What they must say (the process):

1. Dual-fit still locks both shoulders at `fill_seam_search_secs` (250 ms) first, with the
   same envelope the visit already uses. Keep path is unchanged.
2. When the **post** lock is uninformative (`r < 0.5`), it re-locks the **same start** at a
   frozen longer window `{375, 500, 625, 750, 1000}` ms (envelope off, ±600 ms) and takes the
   shortest `w` with `post_r ≥ 0.5` and, if 250 ms `|Δpre|` was `> 10` ms, `|Δpre| ≤ 10` ms.
   No stop → keep the original 250 ms locks (envelope rescue included). Pre is not grown.
   No extra CLI flag; `--no-dual-fit` still kills the visit.
3. Extract and re-gate are the existing dual-fit fork: `step_real` / `!step_real` on the
   templates that were actually used (`post@pre` at `b_pre + G`, not lag 0); classify on
   lock scores. Residual post window follows the chosen `w`; pre residual stays 250 ms.
4. A grown window is visible as `dual_fit_post_window_ms` on the patched record (absent =
   250 ms).

Homes (edit, do not add a new page):

| Doc | What to add |
|---|---|
| [gap-fill-modes.md](../gap-fill-modes.md) § Dual-fit rescue (G6) | One step on the ASCII visit + a short paragraph. This is the operator “how G6 works.” |
| [seam-scoring.md](../seam-scoring.md) § 6 | Wire spec: after the 250 ms per-shoulder lock, optional same-start post grow, then reconcile. Canonical mechanism. |
| [gap-repair-guide.md](../gap-repair-guide.md) § Dual-fit rescue (W7) | Operator: a weak post join can still patch if a longer same-start template locks. |
| [cli-output.md](../cli-output.md) dual-fit metadata | Stamp / optional `-v`. Point at G6, do not retell the stop rule. |
| [json-output.md](../json-output.md) | The field (§3.3). Contract, not the process write-up. |

[pipeline.md](../pipeline.md) already defers phase 4 to those pages — no extra dual-fit
section there.

### 3.5 Not this Change

- Growing **pre**.
- Always-625 / flipping `fill_seam_search_secs` (keep-path seams, trim guards on other paths).
- Replacing `step_real` with `|Δpre|` as the extract fork.
- Walk-off / later 250 ms / max-energy tiles.
- Loosening floors.
- A new operator page, a TEMP excerpt in `docs/`, or documenting grow-pre.

---

## 4. Phases

### Phase 0 — implement

Fixture: quiet ~300 ms after the cut, then speech; 250 ms junk lag **and** `gate_pass`
true; grow still runs and stops at 500 ms; `!step_real`; extract `pre+G`. Second fixture:
`gate_pass` false at 250 ms, grow still runs. Identity: locks in hand already `post_r ≥
0.5` (including a rescued post) → no extra `seam_local_peak`. `--no-dual-fit` byte-identical.
Feature docs as §3.4 (same commit as the code).

### Phase 1 — ear

`--gap-listen` / `--patch-only` on 8/g4 (must keep the syllable). If 22/g13 patches, ear it.
20/g3 optional (fill should not move).

### Phase 2 — safety spot-check

Not a full 51-pair re-fingerprint. Confirm a handful of high-r 250 ms dual-fit patches
still report no `dual_fit_post_window_ms` and unchanged seams. 25/g11 still skip / still
weak pre.

---

## 5. Acceptance

- 8/g4 patched, `dual_fit_post_window_ms` 500, ear: no missing syllable, no new hole at the
  end.
- High-r dual-fit gaps unchanged (no grown window stamp).
- 25/g11 not converted by this Change.
- `--no-dual-fit` still byte-identical to today.
- G6 operator/wire docs describe the post-grow as current dual-fit behaviour (§3.4).

---

## 6. Checklist (implement)

- [ ] Admit on locks in hand (`post_r < 0.5`); do not replace 250 ms with envelope-off.
- [ ] Grow before `dual_fit_from_locks` `gate_pass` reject; recompute `b_post_seam` / bridge / `gate_pass`; no-stop keeps original locks.
- [ ] Grown A post = raw slice + `seam_local_peak`; do not relax `SeamLockTemplates`; thread into `post@pre` / `step_real` only.
- [ ] Residual post window = chosen `w`, pre stays 250 (`measure_dual_fit_residual_verdict` splits).
- [ ] Frozen `w` list; stop rule as §3.2.
- [ ] `post@pre` at `b_pre_seam + G` with the chosen template (not nominal).
- [ ] Extract still `step_real` on those scores; `!step_real` → `pre+G`.
- [ ] Dual-fit classify still uses lock `r`.
- [ ] Stamp `dual_fit_post_window_ms` when grown.
- [ ] Quiet-then-speech fixture (`gate_pass` true at junk 250 ms, grow still runs); 22/g13-shaped fixture (`gate_pass` false, grow still runs); no-grow when locks in hand already `r ≥ 0.5` (rescued post included).
- [ ] Ear 8/g4; ear 22/g13 if it patches.
- [ ] Feature docs as §3.4: present-tense G6 process in `gap-fill-modes.md`,
      `seam-scoring.md` § 6, `gap-repair-guide.md` W7, `cli-output.md`; field in
      `json-output.md`. No TEMP narrative, no pair indices.
