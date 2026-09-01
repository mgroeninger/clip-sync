# TEMP — Seam-lock rigid fill (constant-shift G6 arm)

**Status:** Change plan, 2026-09-01. **Startable.** One deliverable: after a scored keep-path
skip, emit a **trim-0 fill sitting on the 250 ms seam locks** when those locks already lie a gap
apart. Same fall-through as dual-fit; not a new search.

**Tracked as:** [BACKLOG.md](../../BACKLOG.md) § *Fill placement* → *95 skips at a rigid
shoulder offset*.

**Companions:** findings
[TEMP-rigid-shoulder-offset-skips-2026-08-31.md](archive/TEMP-rigid-shoulder-offset-skips-2026-08-31.md)
(Q2 null: register-before-sweep does not convert); dual-fit wire
[seam-scoring.md](../seam-scoring.md) § 6; cell table
[gap-vocabulary.md](gap-vocabulary.md) § *Cell → action*; G6 operator
[gap-fill-modes.md](../gap-fill-modes.md) § Dual-fit rescue. Dual-fit's construction
[archive/TEMP-seam-splice-dualfit-plan.md](archive/TEMP-seam-splice-dualfit-plan.md) § 0
(silence has no landmark → encoder changes **spacing** → independent locks + length edit).
W5 reserved the constant-shift remainder for keep path
[archive/TEMP-w5-timing-offset-rescue-plan.md](archive/TEMP-w5-timing-offset-rescue-plan.md)
(haystack was supposed to own it; it does not). Opposite G6 error (false `step_real` from a
quiet 250 ms post, 8/g4):
[TEMP-dual-fit-post-window-ladder-plan.md](TEMP-dual-fit-post-window-ladder-plan.md)
(dump only; not this Change).

**Media hygiene:** pair index + gap index only. `gap-files/` is gitignored; numbers below
are the durable record.

---

## 0. Architecture (normative)

This section is the plan. Later sections only implement it. If a later sentence disagrees
with this one, this one wins.

**Keep-path search does not change. Dual-fit's length-edit theory does not change.
`skip_or_dual_fit` already has the locks. Stop throwing them away when the step is not
real. Assemble `B[b_pre_seam .. b_pre_seam + G]`, re-validate with the unchanged
interleaved gate, splice. Stamp trim-0 distinct from a length edit.**

```text
keep path skip (scored, not StructureAlignmentFailed)
  → seam_local_peak both collars (±600 ms, raw 250 ms templates)
  → gate_pass ∧ bridge > 0 ∧ donor occupied ∧ ¬program-quiet
       ├─ step_real     → extract bridge, interior trim to G, re-gate   (today's G6)
       └─ ¬step_real    → extract [pre, pre+G], trim 0, same re-gate    (this Change)
  → re-gate fail → original skip
```

`--no-dual-fit` still short-circuits the whole visit (byte-identical to today). No new CLI
flag. No third search. Register-before-sweep stays unwired.

One sentence: **the constant-shift fill W5 said keep path owned is the zero-trim arm of
the visit that already measured the locks.**

---

## 1. Why this, not a new gate or a keep-path search fix

Keep-path Pearson at the unified-search start is 0.02–0.18 on the five remainder
(`correlation_below_threshold` / `waveform_floor`). Dual-fit's 250 ms locks on the same
gaps are 0.91–1.00, and `post@pre` is within 0.15 of `post` own (`trim` 0.0–0.2 ms). The
joins exist and are already `G` apart.

Q2 ([TEMP-rigid-shoulder-offset-skips-2026-08-31.md](archive/TEMP-rigid-shoulder-offset-skips-2026-08-31.md)
Result 6) slid keep-path's extract by that lock lag and still `waveform_floor`. Sliding
the structure search does not sit it on those joins.

The 83 dual-fit patches are the existence proof for the other order: lock, assemble,
re-score with `fill_splice_seam_correlations_interleaved`. That gate can pass once the
fill sits on the locks. Today's `try_dual_fit` declines **before assemble** when
`!step_real`, so the five never reach that re-gate.

Dual-fit's rules were built as: keep path failed because silence changed the **spacing**,
so the locks will not be `G` apart. `step_real` is that clause. The five are the case
the clause said keep path would already own. Keep path does not. This Change emits that
fill without pretending there was a length step.

---

## 2. Object

**Must convert** (production skip, fingerprint `gate_pass`, `bridge_frames > 0`,
`!step_real`; 08-29 dump + 08-31 production join):

| Gap | Dual-fit 250 ms r | Δ (`post − post@pre`) | `trim` |
|---|---|---:|---:|
| 1/g1 | 1.000 / 0.985 | 0.001 | 1 frame |
| 1/g15 | 0.999 / 0.983 | 0.074 | −3 |
| 20/g1 | 1.000 / 0.953 | 0.071 | 9 |
| 22/g5 | 0.998 / 0.982 | 0.135 | −11 |
| 30/g43 | 0.908 / 0.968 | 0.022 | −1 |

**Must not convert** (same remainder, different conjunct):

| Gap | Why this arm must not take them |
|---|---|
| 11/g9 | `bridge_frames` −205 (`shoulders_abut`); collapse plan owns un-abut |
| 22/g13 | `gate_pass` false (250 ms post dead) |

**Convert set may be larger than five.** Fingerprint skips outside the 1 s-rigid class can
also be `gate_pass ∧ bridge > 0 ∧ !step_real`. Phase 0 names that set before the code
ships. Do not treat "the five" as the production blast radius.

1/g1's 1 s `peak_z` is ~5.5 / 7.4 (strong match, not unique). Residual re-gate is the
guard, same as today's G6. Ear the five; uniqueness is not a refuse.

---

## 3. Behaviour change

### 3.1 `try_dual_fit` / `dual_fit_at_nominal`

Today: `post_r` not beating `post_global + 0.15` → `None`.

This Change: that branch **does not return**. Continue to donor / program-quiet checks,
then assemble. When `!step_real`, the extract is `b_pre_seam .. b_pre_seam + gap_frames`
(equivalent to today's `trim_at_lowest_energy_interior` on a bridge that is already `G`
long, `|trim_frames|` ~ 0). Do not invent a length edit to "make dual-fit look busy."

Keep the `step_real` **measurement** (`post_seam_global_r` on the result). It becomes a
report axis, not an admission gate.

NaN `post_global` still declines (same `partial_cmp` refuse as today — unmeasured is not
"constant shift suffices").

### 3.2 Re-gate (unchanged)

`skip_or_dual_fit` already re-scores the assembled fill with
`fill_splice_seam_correlations_interleaved` + `classify_fill_waveform_confidence` and
falls back to the keep-path skip on failure. Rigid fills use that same reject. No floor
loosening.

### 3.3 Residual

Same dual-fit residual path (per-shoulder `chosen_delta` from `pre_lag` / `post_lag`).
On a rigid map the two lags agree; the path still runs. A residual veto is a skip, as
today.

### 3.4 Report — do not smear Silence-splice

Same visit, two kinds:

| Kind | `trim_frames` | Human (`-v` / status) | JSON |
|---|---|---|---|
| Length-edit (today's 83) | materially nonzero | `patched (dual-fit pre→post)` | `dual_fit_used: true`, `dual_fit_step_real: true` |
| Rigid (this arm) | ~0 | `patched (seam-lock pre→post)` | `dual_fit_used: true`, `dual_fit_step_real: false` |

`dual_fit_used` stays "this gap left keep path via the G6 visit." The new bool (name
locked at implement: `dual_fit_step_real` or `seam_lock_rigid` — pick one, populate a
golden, follow [json-output.md](../json-output.md) § *Revision procedure*) is the cell
split so analysis does not call trim-0 a silence-splice.

**Do not** add a `GapRepairCell` in this Change. Skip-with-`--no-dual-fit` may still
classify these as `Decorrelated` (today's lie: lags *did* recover). Fixing skip taxonomy
is a follow-up, not this deliverable.

Optional on `status.patched`: `dual_fit_trim_frames` (already measured). Nice; not
required if `dual_fit_step_real` is present.

### 3.5 Fingerprint predictor

`dual_fit_rescue_flag` drops conjunct 4 (`step_real`). Remaining: eligible brackets,
`gate_pass`, `bridge_frames > 0`, aligned continuous, ¬program-quiet. The five become
`Some(true)`. Tests in `schema.rs` that pin `rigid → false` flip to `true`.

The keep-path-shift overlay probe was stripped after Q2 (Result 6). Q2 stays a null on the search.

---

## 4. Phases

### Phase 0 — convert-set census (before code)

Dump-only over `gap-files/2026-08-29-complete-corpus` (and the 08-31 production join for
disposition):

```text
skip ∧ dual-fit-eligible brackets ∧ splice_dualfit.gate_pass
  ∧ bridge_frames > 0 ∧ ¬step_real ∧ aligned.continuous ∧ ¬program-quiet
```

Name every gap. Split: the five remainder vs anything else. If the tail is large or
includes known-bad joins, stop and rewrite admission — do not ship a surprise class.
11/g9 and 22/g13 must be absent from this set.

### Phase 1 — domain assemble

`domain/dual_fit.rs`: `!step_real` continues to `Some(DualFitResult)` with
`trim_frames ≈ 0`. Unit test: rigid locks (post ≈ post@pre) return a fill of
`gap_frames`; the existing step-real decline test becomes "still Some." Length-edit
fixtures stay `Some` with nonzero trim.

### Phase 2 — report stamp + CLI

Thread `step_real` (or `trim_frames` plus a derived bool) onto the patched status / tags
/ human line. JSON golden + [json-output.md](../json-output.md).
[cli-output.md](../cli-output.md) dual-fit line grows the seam-lock spelling.
[gap-fill-modes.md](../gap-fill-modes.md) G6 paragraph: two arms, one visit.

### Phase 3 — predictor + vocab

`dual_fit_rescue_flag` as § 3.5. [gap-vocabulary.md](gap-vocabulary.md): Silence-splice
row stays "real step; length reconcile." Add one sentence that a G6 visit with
`!step_real` is a **rigid seam-lock fill**, not that cell. Do not merge them.

### Phase 4 — verification

- Unit: rigid assemble; NaN global still declines; `--no-dual-fit` still skip.
- Integration: existing dual-fit jump-cut still `dual_fit_used` + step real.
- Synthetic constant-shift (delayed B, locks `G` apart): keep path skips, this arm
  patches, `dual_fit_step_real == false`.
- Media: `--patch-only` on the Phase 0 set. The five patch; 11/g9 and 22/g13 still skip;
  83 still `dual_fit_used` with step real. Count extras from Phase 0.
- Ear the five (and any extra Phase 0 member). Convert without an ear is not done.

---

## 5. Non-goals

- Register-before-sweep, keep-path template changes, standoff/walk-off edits, lowering
  Pearson floors, `--full` grid, gate-mode one-strong-seam.
- Relaxing `step_real` so a **length edit** runs on a rigid map (that is the opposite
  change).
- 11/g9 un-abut, 22/g13 dead post, 31/g7, collapse drain.
- Hoisting `post_seam_global_r` onto `SeamLocks`.
- A second CLI flag splitting rigid vs length-edit opt-out.
- A new `GapRepairCell` / skipping-taxonomy fix for `--no-dual-fit`.
- Sampling or conversion fractions over the 90.

---

## 6. Implementation checklist

Do not start this list until Phase 0 has named the convert set.

- [ ] Phase 0 table in this note (or a dated Result subsection): every convert gap, five
      vs extras. **Not done this pass — deferred, media-dependent census.**
- [x] `try_dual_fit` / `dual_fit_at_nominal`: `!step_real` assembles trim-0; NaN global
      still `None`.
- [x] `skip_or_dual_fit` re-gate unchanged; residual path runs.
- [x] Status / tags / human line distinguish rigid vs length-edit; JSON golden populated
      in both states; `json-output.md` + `cli-output.md` + `gap-fill-modes.md`.
- [x] `dual_fit_rescue_flag` drops `step_real`; schema tests updated. (NaN `post_global`
      still declines — `is_finite()` conjunct added directly, since the dropped `step_real`
      conjunct was the only thing catching it; caught before landing, see 2026-09-01 note.)
- [x] `gap-vocabulary.md` Silence-splice row not redefined; rigid arm named beside it.
- [ ] Tests in § 4 Phase 4, including `--no-dual-fit` identity. **Unit-level covered by the
      existing suite (714 passed, 0 failed); synthetic constant-shift test and the media
      `--patch-only` / ear-check runs not done this pass — media-dependent, follows Phase 0.**
- [ ] Licensed `--patch-only` on the named set; ear the converts.

---

## 7. Exit

- Phase 0 named; extras are accepted or admission was rewritten.
- The five production-skip (and every accepted extra) patch on `--patch-only` with
  `dual_fit_used` and `dual_fit_step_real == false`; re-gate Pearson High or Marginal.
- 11/g9, 22/g13 still skip. The 83 still length-edit.
- `--no-dual-fit` matches today's skip set for these gaps.
- Ear on the convert set. Fail → do not ship; name the miss (residual, false lock, join).
- This note archives with a [settled.md](settled.md) row that G6's `!step_real` arm is
  the constant-shift fill, and the BACKLOG "95 skips / rigid offset" **row is rewritten
  to the remainder that still skip** (11/g9, 22/g13, extras that failed ear) — not
  deleted until those are named closed. Do not file a class-wide refute of map-lag;
  Q2 already closed that candidate.

---

## 8. Data sources

- `gap-files/2026-08-29-complete-corpus/` — locks, `splice_dualfit`, class remainder.
- `gap-files/2026-08-31-local-gain-acceptance/` — production skip disposition.
- `gap-files/2026-08-31-keep-path-shift/` — Q2 null (do not re-run as acceptance).

---

## 9. Phase 0 census (2026-09-01)

Dump-only, `scripts/measure/dualfit-coverage-census.ps1 -DumpDir gap-files/2026-08-29-complete-corpus
-InclRigidFill`, filtered to `keep_path_tier == skip ∧ dualfit_rigid_verdict == true ∧
dualfit_alone_verdict == false` (i.e. every leg but `step_real` passes). **Six**, not five:

| Gap | pre r | post r | post@pre | trim |
|---|---|---:|---:|---:|
| 1/1 | 1.000 | 0.985 | 0.984 | +1 |
| 1/15 | 0.999 | 0.983 | 0.909 | −3 |
| 20/1 | 1.000 | 0.953 | 0.882 | +9 |
| 20/3 | 0.997 | 0.443 | 0.363 | −1 |
| 22/5 | 0.998 | 0.982 | 0.847 | −11 |
| 30/43 | 0.908 | 0.968 | 0.946 | −1 |

11/9 and 22/13 confirmed absent from this set (still blocked on `bridge_frames` and
`gate_pass` respectively, as § 2 says). 20/3 is the extra § 2 did not name; every other
admission leg passes clean (`eligible`, `gate_pass`, `bridge_frames_pos`, `donor
continuous`, `¬program-quiet` all true) — `step_real` is the only failing conjunct, same
as the other five. Do not read that as "same case as the other five," though: its
`post_seam_r` (0.443) is far weaker than the other five (0.91–1.00), and § *Companion
finding* below is why that gap gets its own ear-check rather than riding in on the
group's confidence.

### Companion finding — 8/4, a weak-lock length-edit already in production

Not a Phase 0 member (it patches today; `step_real` holds). Raised while sizing whether
weak `post_seam_r` is safe territory for dual-fit generally, since 20/3 sits there too.
Same corpus, `dualfit_alone_verdict == true` set (105 gaps: skip-tier, `step_real` holds,
every leg passes) — 8/4 is the weakest by `post_seam_r` (0.390; next-weakest is 0.566,
n=1 below 0.5). `dual_fit_rescue_field` on the row agrees (`true`), and it is a large
length edit, not a rigid map (`trim_frames = −13738`, ≈286 ms at 48 kHz). A (00:36:51.19,
duration 0.658 s); B mapped +4.244 s.

Ear-checked 2026-09-01 against a production repair run (~1 month old; dual-fit is the
only mechanism that could have produced this patch, but the run predates this note and
its exact code path was not logged). **Result: broken.** The fill sits well at the start
and leaves an audible gap at the end — the interleaved re-gate passed a fill with a
real defect at the post seam. Confirmed a second time same day directly off the
`--gap-listen` WAV (not the month-old production recollection): same defect, same
description — not a fluke of the older run or a since-fixed code path.

This is the load-bearing reason 20/3's ear-check is not routine: it is the second-weakest
`post_seam_r` adjacent to 8/4's, and the one confirmed-broken case so far is exactly the
weakest one. n=1 does not indict every weak lock (8/4 is also a large trim, unlike 20/3's
≈0), but it does mean "inside dual-fit's existing operating range" is no longer a
sufficient argument on its own — the range itself has an unverified tail.

**Action:** add 8/4 to the same `--gap-listen` run built for the Phase 0 six, so its WAV
sits beside 20/3's for a direct side-by-side (near-zero trim, weak lock vs large trim,
weak lock) instead of relying on a month-old recollection. Do not fold 8/4 into the
Phase 0 convert table above — it is a length-edit precedent, not a rigid-arm candidate.

### Phase 0 ear-check (2026-09-01, post-Phase-1 build)

`--gap-listen`/`--patch-only` against a build with the Phase 1 domain change
(`domain/dual_fit.rs`: `!step_real` assembles trim-0 instead of declining), narrowed to
the six `--fingerprint-gap` indices above, plus 8/4 alongside for the companion
side-by-side.

| Gap | Result |
|---|---|
| 1/1 | context reads as empty at normal playback gain — see caveat below |
| 1/15 | patches very well |
| 20/1 | gap falls in silence; repaired fine |
| **20/3** | **passes** — loud discordant instrument, fades through the gap, patch sounds good |
| 22/5 | gap falls in silence; repaired fine |
| 30/43 | perfect |
| 8/4 (companion) | confirmed broken a second time, independent of the month-old production run — same defect (fill covers the start, leaves an audible gap at the end) |

**20/3 is the headline result.** It's the one gap in the set whose `post_seam_r` (0.443)
sits far below the other five (0.91–1.00) — adjacent to 8/4's 0.390, the weakest lock in
the whole 105-gap `dualfit_alone_verdict` population and the one case so far confirmed
broken. 20/3 passing clean says a weak `post_seam_r` is not on its own disqualifying for
the rigid (trim-0) arm — 8/4's break lives in the length-edit branch (real `step_real`,
interior-trim search), not evidence against trim-0 assembly. Five of six Phase 0 gaps are
unambiguous passes.

**1/1 caveat, not yet resolved:** reads as "all clips empty" by ear, but the corpus row
says otherwise — `pre_seam_r` 0.9999, `post_seam_r` 0.9852, `trim_frames` +1 (as tight a
lock as the set has), and the surrounding context is genuinely quiet (context RMS ≈ −54
to −60 dB fanning down to true silence, −101 dB, inside the gap itself — a real dropout
in an otherwise quiet passage, per `equivalence_production.class = repairable_dropout`).
"Empty at normal volume" is consistent with a correctly-repaired quiet passage, not
necessarily a broken one — but it's indistinguishable by casual listen from a WAV-writing
or index-mismatch bug, so don't count it as a pass without a re-listen at higher gain (or
a level/waveform check on the three `--gap-listen` files directly).

**1/1 resolved (2026-09-01):** waveform-graph check on all three `--gap-listen` files —
digital silence sitting inside a wider normative-silence passage, patch reads correct. Not
a bug; matches the corpus row.

Net: **6/6 Phase 0 gaps pass**, including the load-bearing 20/3 case. No blocker to
proceeding to Phase 2/3.
