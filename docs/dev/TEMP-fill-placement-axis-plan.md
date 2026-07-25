# Fill placement axis — measure the end search before changing it

> **Status:** Proposed (2026-07-25). No code written. Prerequisite for any change to the end
> search's scoring; **not** a prerequisite for lever 1b(c) (the end-search repeat hoist), which is
> byte-identical and already landed.

**Problem:** the unified fill search picks `end` (and therefore fill length) over a ±5 s slack using
**structure evidence only** — every waveform term in `unified_search_best_fill_end` is loop-constant
(see [repair-perf.md §1a](repair-perf.md)). Whether that is right is an open question. We cannot
answer it, because **no harness records the chosen fill placement.** A change that moved every fill
length on the corpus would leave the golden diff green.

**Goal:** make fill placement an observable, then use it to A/B the end-search scoring on media.

**Non-goals:** changing the end search's objective in Phase A or B — those are pure measurement.
Re-tuning `fill_length_slack_secs`. Touching the seam gate or the dual-fit path.

---

## Why nothing measures it today

| Layer | Why it is blind |
|-------|-----------------|
| Fingerprint corpus (`gap-files/anchor-bracket-corpus`) | `measure.rs:551` runs the search with `waveform_weight: 0.0`. Waveform-side changes are invisible **by construction**. |
| Fingerprint dump | `PlacementScores` (`measure.rs:460`) keeps `start_frame` only. `fill_frames` / `end_frame` never leave `place_on_b`. Schema scan across all 17 pair dirs: no fill-length field exists. |
| Golden baseline | `GoldenRecord` (`golden_baseline.rs:23`) has no `start_frame`, `fill_frames`, or chosen-bracket `pre/post_correlation`. Only verdict + gate axes. |
| `fft_seam_search_matches_naive_placement` | Synthetic broadband noise, single fixture, `repeat_penalty_weight: 0.0`. The corpus placement-diff that [the perf plan asked for](archive/TEMP-production-repair-perf-plan.md) (line 324, "naive vs FFT `start_frame`/`end_frame` on the corpus") was never built. |
| `patch_audio_integration` 26/26 | Real byte-parity, but sine fixtures. |
| `gap_repair_spec_diff` | Projection differential, media-free by design. |

**The observable already exists in the type system.** `GapRepairStrategy::Bracket { alignment, .. }`
carries `FillAlignment { start_frame, fill_frames, pre_correlation, post_correlation }` — exactly the
four numbers an end-search change moves. Phase A is a projection, not new machinery.

---

## Should the fingerprint get a production-weights mode?

**Yes — as an additional placement, not as a flag that flips `waveform_weight` in place.**

### The corpus is already multi-placement, and seam-chosen placement is already in it

`splice_dualfit_at` (`measure.rs:967`) places **each shoulder independently at the lag maximizing its
own seam** (`seam_local_peak` over ±`SEAM_LOCAL_SEARCH_MS`), and derives
`bridge_frames = b_post_seam - b_pre_seam`, `trim_frames = bridge_frames - gap_frames` — a
*discovered* gap length, deliberately unequal to the original. It runs unconditionally for every gap
(`measure.rs:1785`). So "the best seam we could find anywhere in the search radius" is not a
contaminant the fingerprint avoids; it is an axis the fingerprint already ships, because for gaps
whose surroundings are silent there is no structurally-correct placement to measure against.

**The estimator-bias concern is handled by publishing validators, not by abstaining.** Directly below
that argmax: `pre_seam_prom` / `post_seam_prom` (*"is each seam a unique (non-periodic) match?
Prominence of the placement peak over its best rival within ±30 ms"*), `pre_seam_z` / `post_seam_z`,
and `post_seam_global_r` (*"is the step necessary?"*). Prominence is the guard against max-of-noise
inflation. Any seam-chosen placement axis must ship the same guards — see Phase B.

### What the `waveform_weight: 0.0` is actually protecting

One narrow thing: `brackets[].seam_pre` / `seam_post` feed `classify_bracket_stage`
(`measure.rs:1427`), and **those two fields carry no prominence or z companion.** Measured at a
structure-chosen placement they mean "structure found a placement; does the waveform corroborate?" —
which is what makes `waveform_floor` a meaningful failure stage. Overwrite them with an unguarded
argmax and that distinction is gone. That is an argument about two specific fields, not about
seam-chosen placement in general, and it is satisfied by adding fields rather than replacing them.

The `0.0` carries no comment and `gap-fingerprint.md` does not mention it — **Phase A documents it,
scoped to that claim and no wider.**

### The two objections that survive

1. **Golden churn.** If the *existing* placement became a function of `fill_fit_waveform_weight`,
   `fill_repeat_penalty_weight`, and the FFT flag, every retune would churn the committed goldens —
   the durable reference. Additive fields avoid this entirely.
2. **Cost.** `place_on_b` runs **per bracket** in the `Full` tier (`measure.rs:1409`) — 4.3–5.0
   s/bracket over ~5061 brackets. `wf > 0.0` activates the per-candidate repeat penalty, 72% of
   production repair time. Lever 1b(c) removes the end-side half; the start-side half remains. Hence
   opt-in, paid once per A/B.

**Conclusion:** record `placement_structure` (existing, unchanged) *and* `placement_production` (new,
opt-in, own fields, own validators), alongside the `splice_dualfit` seam-placed axes that are already
there. Three placement models, three repair models, each measured on its own terms.

---

## Phase A — project the placement (no behavior change)

- [ ] Add `fill_frames` (and `end_frame`) to `PlacementScores`; carry `matched.alignment.fill_frames`
      through `place_on_b`.
- [ ] Emit per-bracket `fill_frames` in the fingerprint JSON alongside `seam_pre` / `seam_post`.
- [ ] Add to `GoldenRecord`: `fill_start_frame`, `fill_frames` (**Tier-1**, bit-exact) and
      `fill_pre_r`, `fill_post_r` (**Tier-2**, within ε). Update the `schema` string.
- [ ] Document the `waveform_weight: 0.0` rationale in `gap-fingerprint.md` and as a comment at
      `measure.rs:551`.
- [ ] Re-baseline the goldens (additive fields ⇒ existing axes must not move; assert that).

**Exit:** a golden diff turns red when a fill length changes. That is the whole point of Phase A.

## Phase B — production-weights second placement (opt-in)

- [ ] `place_on_b` optionally computes a **second** match with production `UnifiedFitWeights` (from
      config, not hardcoded), emitted as `placement_production.{start_frame,fill_frames,seam_pre,seam_post}`.
- [ ] **Ship the validators with it.** A seam-influenced placement without a prominence companion is
      the max-of-noise estimator, and the dual-fit path already sets the precedent for how to avoid
      that: emit `seam_prom` / `seam_z` for the production placement, reusing `seam_prominence` and the
      `DUALFIT_SEAM_UNIQ_LAG_MS` convention so the numbers are comparable to `splice_dualfit`'s.
      Non-negotiable — without it the Phase B roll-up cannot distinguish a real placement gain from
      search freedom.
- [ ] Gate behind a fingerprint flag (default **off**) — the default dump must stay byte-identical to
      Phase A, and the cost objection must not land on routine runs.
- [ ] Roll-up: report the `structure` vs `production` vs `splice_dualfit` placement delta
      distribution over the corpus — three placements, one table.

**Exit:** we can state, on real media, how far production placement diverges from structure-only, and
how much of the ±5 s slack is actually being used. **This is the number that decides whether Phase C
is worth doing at all.** If the end sweep rarely leaves nominal, the whole question is moot and the
plan stops here.

## Phase C — end-dependent seam (behavior change, gated on B)

Only if Phase B shows the slack is materially used.

- [ ] Make the post seam a function of the candidate's `end` in `unified_search_best_fill_end` —
      i.e. build the `SeamPlacement` from `end - start` rather than `ctx.gap_frames`.
- [ ] Affordability: the end-swept post seam is affine in the sliding index, so
      `fill_seam_correlations_band` (lever 1 Part B) applies. Without banding this is not shippable —
      it puts back the per-candidate Pearson lever 1 removed.
- [ ] Same for the repeat-post window: the origin plan
      ([archive/fill-fitting-plan.md](archive/fill-fitting-plan.md), Phase D) specified
      `repeat_post = corr(A_post_border, B_fill_tail)` — the *fill tail*. Pinning it at
      `start + gap_frames` is exact only when `fill_len == gap_frames`.
- [ ] A/B on the corpus using the Phase A axes: placement deltas, patch-rate delta, seam-r delta.
- [ ] **Listening pass.** Per the dual-fit precedent, a placement change is validated by ear, not by
      a green harness. Phase A/B make the change *visible*; they do not make it *correct*.

**Interaction with the two mechanisms that already do this.** Phase C must not be a third
implementation of length reconciliation:

- `fit_fill_length_for_gap` → `pick_fill_length_anchor` scores trim-head vs trim-tail on the actual
  trimmed fill at splice time.
- **`splice_dualfit_at` already solves the general form.** Two independent shoulder fits, and
  `trim_frames = bridge_frames - gap_frames` *is* the length delta implied by them. An end-dependent
  post seam in `unified_search_best_fill_end` would be re-deriving the post-shoulder fit inside the
  bracket search.

So the honest Phase C question may not be "should the end search's post seam move with `end`" but
**"is the bracket path's end search vestigial for gaps where dual-fit applies?"** Phase B's
three-placement roll-up is what tells us: if the production placement's fill length tracks
`splice_dualfit`'s `bridge_frames`, the mechanisms are redundant and the answer is consolidation, not
a new seam term. Settle that before writing any Phase C code.

---

## Risks

| Risk | Mitigation |
|------|------------|
| Phase A goldens churn on unrelated axes | Fields are additive; assert existing axes bit-identical in the re-baseline commit |
| Phase B doubles fingerprint runtime | Default off; run once per A/B, not per sweep |
| Phase C ships on a green harness with no listening | Listening is an explicit Phase C exit item, not optional |
| Phase C double-counts with the splice trim | Called out above; resolve before implementing |

## Related reading

- [repair-perf.md §1a](repair-perf.md) — the measurement that raised this, and the git-history finding
  that the end search's waveform terms were loop-constant from `e849e64` onward
- [archive/fill-fitting-plan.md](archive/fill-fitting-plan.md) — origin design, Phase B objective and
  Phase D repeat-window spec
- [archive/TEMP-production-repair-perf-plan.md](archive/TEMP-production-repair-perf-plan.md) — levers 1
  / 1b, and the corpus placement-diff that was specified but never built
- [gap-fingerprint.md](gap-fingerprint.md), [corpus-validation.md](corpus-validation.md)
