# TEMP — Seam-lock rigid fill (constant-shift G6 arm)

**Status:** Change plan, 2026-09-01. **Startable.** One deliverable: after a scored keep-path
skip, emit a **trim-0 fill sitting on the 250 ms seam locks** when those locks already lie a gap
apart. Same fall-through as dual-fit; not a new search.

**Tracked as:** [BACKLOG.md](../../BACKLOG.md) § *Fill placement* → *95 skips at a rigid
shoulder offset*.

**Companions:** findings
[TEMP-rigid-shoulder-offset-skips-2026-08-31.md](TEMP-rigid-shoulder-offset-skips-2026-08-31.md)
(Q2 null: register-before-sweep does not convert); dual-fit wire
[seam-scoring.md](../seam-scoring.md) § 6; cell table
[gap-vocabulary.md](gap-vocabulary.md) § *Cell → action*; G6 operator
[gap-fill-modes.md](../gap-fill-modes.md) § Dual-fit rescue. Dual-fit's construction
[archive/TEMP-seam-splice-dualfit-plan.md](archive/TEMP-seam-splice-dualfit-plan.md) § 0
(silence has no landmark → encoder changes **spacing** → independent locks + length edit).
W5 reserved the constant-shift remainder for keep path
[archive/TEMP-w5-timing-offset-rescue-plan.md](archive/TEMP-w5-timing-offset-rescue-plan.md)
(haystack was supposed to own it; it does not).

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

Q2 ([TEMP-rigid-shoulder-offset-skips-2026-08-31.md](TEMP-rigid-shoulder-offset-skips-2026-08-31.md)
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

`keep_path_shift` overlay is **untouched**. Q2 stays a null on the search.

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
      vs extras.
- [ ] `try_dual_fit` / `dual_fit_at_nominal`: `!step_real` assembles trim-0; NaN global
      still `None`.
- [ ] `skip_or_dual_fit` re-gate unchanged; residual path runs.
- [ ] Status / tags / human line distinguish rigid vs length-edit; JSON golden populated
      in both states; `json-output.md` + `cli-output.md` + `gap-fill-modes.md`.
- [ ] `dual_fit_rescue_flag` drops `step_real`; schema tests updated.
- [ ] `gap-vocabulary.md` Silence-splice row not redefined; rigid arm named beside it.
- [ ] Tests in § 4 Phase 4, including `--no-dual-fit` identity.
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
