# Fill placement axis — measure the end search before changing it

> **Status (2026-07-25):** Phase A is **code-complete, exit criterion unmet.** The placement is
> emitted, carried to `GapRow`, recorded in `GoldenRecord`, and the golden is re-baselined — but
> `fill_start_frame` / `fill_frames` are **null on all 10 curated gaps**, because the committed
> fixtures predate the emit. So the tripwire is proven at unit level (`diff_catches_tier1_flip`) and
> **not** end-to-end. `curated_golden_fill_placement_is_not_yet_armed` asserts that null column so the
> gap stays loud; it is designed to fail when the fixtures are regenerated. Arming it needs media —
> see *Remaining work* below.
>
> Prerequisite for any change to the end search's scoring; **not** a prerequisite for lever 1b(c)
> (the end-search repeat hoist), which is byte-identical and already landed.

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

**But placement is an output, not a decision — do not confuse the strategy with the tags.**
`GapRepairStrategy` is the *executor's input* and carries the alignment. `GapRepairTags`
(`{ registration, seam_local, donor_nominal, donor_aligned, gate, levels }`) is the *decision*
carrier, and deliberately does not: no gate, tier, or verdict reads a frame index. So the media-free
`gap_repair_spec_diff` differential — whose contract is *tags are a complete decision carrier* —
must **not** assert on `fill_*`, and widening `GapRepairTags` to make it able to would be inverting
the test's purpose. The placement tripwire belongs on the **media corpus** golden
(`baseline_from_report`), where the values are measured. `GoldenBaseline::without_placement()`
scopes the projection path accordingly.

---

## Should the fingerprint get a production-weights mode?

**Yes — as an additional placement, not as a flag that flips `waveform_weight` in place.**

### The corpus is already multi-placement, and seam-chosen placement is already in it

`splice_dualfit_at` (`measure.rs:985`) places **each shoulder independently at the lag maximizing its
own seam** (`seam_local_peak` over ±`SEAM_LOCAL_SEARCH_MS`), and derives
`bridge_frames = b_post_seam - b_pre_seam`, `trim_frames = bridge_frames - gap_frames` — a
*discovered* gap length, deliberately unequal to the original. It runs unconditionally for every gap
(`measure.rs:1809`). So "the best seam we could find anywhere in the search radius" is not a
contaminant the fingerprint avoids; it is an axis the fingerprint already ships, because for gaps
whose surroundings are silent there is no structurally-correct placement to measure against.

**The estimator-bias concern is handled by publishing validators, not by abstaining.** Directly below
that argmax: `pre_seam_z` / `post_seam_z` (whole-curve z of each seam's peak over the ±
`SEAM_LOCAL_SEARCH_MS` search — the **primary** alias guard: a search that locked onto a far
*periodic* rival reads low), `pre_seam_prom` / `post_seam_prom` (the ±30 ms single-rival margin —
**secondary**, and it false-flags correct-but-periodic content, e.g. 5·g6), and `post_seam_global_r`
(*"is the step necessary?"*). z is the guard against max-of-noise inflation; prominence is a
tiebreaker that must not be read alone. Any seam-chosen placement axis must ship both — see Phase B.

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

## Phase A — project the placement (no behavior change) — **code complete 2026-07-25**

- [x] Add `fill_frames` to `PlacementScores`; carry `matched.alignment.fill_frames` through
      `place_on_b`. (`end_frame` dropped as redundant — it is `start_frame + fill_frames`.)
- [x] Emit per-bracket `start_frame` / `fill_frames` in the fingerprint JSON alongside `seam_pre` /
      `seam_post` (`BracketInfo`, `Option`, `skip_serializing_if` ⇒ old dumps still parse).
- [x] Carry them to `GapRow` as `best_bracket_{start_frame,fill_frames,seam_pre,seam_post}`, selected
      by the **same highest-min-seam rule** `best_bracket_seam` and `closest_failure_stage` already
      use (`best_seam_bracket`), so "the chosen bracket" means one thing across the analyzer.
- [x] Add to `GoldenRecord`: `fill_start_frame`, `fill_frames` (**Tier-1**, bit-exact — frame indices
      are integers, so there is no ε to hide in) and `fill_pre_r`, `fill_post_r` (**Tier-2**).
      `schema` string updated.
- [x] Document the `waveform_weight: 0.0` rationale in `gap-fingerprint.md` (new *Bracket placement*
      section) and as a comment at the `UnifiedFitWeights` literal in `measure.rs`.
- [x] Tripwire test: `diff_catches_tier1_flip` now also asserts a **one-frame** change to
      `fill_frames` or `fill_start_frame` produces exactly one diff error.
- [x] **Emit the *production-weights* placement, not the structure-only one.** There are two dump
      paths. `characterize_gap_region` (`measure.rs:1447`) places via `place_on_b`, which runs at
      `waveform_weight: 0.0` — blind to end-search scoring changes by construction (the very blindness
      this plan exists to fix), so a placement recorded there guards nothing. The from-decode path
      `compute_region_measurements` scores each bracket with `oracle_score_fit_candidate`, i.e. the
      **production gate**, and its `SeamGateOutcome.alignment` already carries `start_frame` /
      `fill_frames`. `oracle_score_fit_candidate` now returns `OracleFitScores` (named struct — the
      5-tuple was already at its limit) including that alignment, and `compute_region_measurements`
      writes it onto each `BracketInfo`. Cost: zero — same search, one field stops being discarded.
      `None` on gate failure, where there is no chosen placement.
- [x] Scope the projection differential with `GoldenBaseline::without_placement()` — placement is a
      search output, not a tag-carried decision (see above).
- [x] **Re-baseline the golden** (`CURATED_GOLDEN_REGEN=1`). Purely additive: 40 new keys plus the
      `schema` line, **zero pre-existing values changed** — property 1 below holds exactly, because the
      fixtures were not touched. Populated: `fill_pre_r` / `fill_post_r` on the 6 gaps with a bracket
      carrying a complete seam pair (they read the pre-existing `seam_pre` / `seam_post`, so this is a
      re-read of committed data through a new selector, not a new measurement). Null: both Tier-1
      placement axes, everywhere.
- [x] Assert the null column (`curated_golden_fill_placement_is_not_yet_armed`) rather than leaving it
      to be discovered — a null Tier-1 column is indistinguishable from a passing one.
- [ ] **Regenerate the curated fixtures from media, then re-arm.** The only remaining step; needs media
      this repo does not record. See *Remaining work*.

**Exit:** a golden diff turns red when a fill length changes. **Proven at unit level; NOT proven
end-to-end** — and it cannot be until the fixtures carry non-null `fill_start_frame` / `fill_frames`.

### Remaining work — needs media

The fixtures come from three provenances (`curated/manifest.json`): `re-anchor-dual-fit-on-nominal`
(pairs 1, 4, 6, 7), `equiv-coarse-vs-fine` (pairs 1, 2) — **different media, so two runs, not one** —
and one synthetic (`10_decorrelated`, `derived-from`, may need no run at all). Neither corpus survives
in `gap-files/`; both are ephemeral. Steps: run those pairs → re-extract the 10 fixtures and update
manifest provenance → review as a **content** change (see the property-1 caveat below) → regenerate
the golden → replace `curated_golden_fill_placement_is_not_yet_armed` with its positive form.

**Deferred deliberately.** The payoff lands only when the end search's scoring changes — Phase C,
which is gated on Phase B. If Phase B shows the ±5 s slack is barely used, the plan stops and this
work is never needed.

### The re-baseline is a separate commit — and needs the fixtures regenerated first

**`CURATED_GOLDEN_REGEN=1` alone is not sufficient.** `curated_gap_cell_rows()` reads *committed
fixture JSONs*, which predate `start_frame` / `fill_frames`. Regenerating the golden against them
populates only `fill_pre_r` / `fill_post_r` (which read the pre-existing `seam_pre` / `seam_post`) and
re-baselines the Tier-1 placement axes as `null` — an all-null column, i.e. exactly the silent-failure
mode property 2 below is meant to catch. Order: **regenerate the curated fixtures from media, then
regenerate the golden.** Current `golden_baseline_invariance` failure is 12 mismatches, all
`fill_*_r`, with `fill_start_frame` / `fill_frames` absent from the list — that is this problem, not a
regression.

`curated.golden.json` currently has no `fill_*` keys. Regenerating adds them, and the values arrive
non-null only where a bracket was actually measured. Two properties to assert in that commit:

1. **Every pre-existing axis is bit-identical.** The fields are additive; if any Tier-1 axis moves,
   something is wrong with the projection, not with the baseline.
   **This held for the golden re-baseline (fixtures untouched) and will NOT hold for the fixture
   refresh.** Regenerating fixtures from media re-runs code and config that have moved since they were
   extracted — `skip_equivalent_gaps` flipped on-by-default (2026-07-20) and dual-fit re-anchored onto
   nominal `b_mapped`. Unrelated axes *will* move. The reviewable question for that commit is not
   "did anything change" but **"did every axis that moved have a known cause"** — so the fixture
   refresh must be its own commit, not folded into a code change.
2. The new `fill_*` fields are populated for gaps with measured brackets — an all-null column would
   mean the projection silently failed and the tripwire guards nothing.

Because a golden re-baseline is the durable reference, that diff deserves review on its own rather
than buried in the code change.

## Phase B — production-weights second placement (opt-in)

- [ ] `place_on_b` optionally computes a **second** match with production `UnifiedFitWeights` (from
      config, not hardcoded), emitted as `placement_production.{start_frame,fill_frames,seam_pre,seam_post}`.
- [ ] **Ship the validators with it.** A seam-influenced placement without an alias companion is the
      max-of-noise estimator, and the dual-fit path already sets the precedent for how to avoid that.
      Emit **`seam_z` (primary)** — whole-curve z over the placement search, the periodicity-robust
      guard — and `seam_prom` (secondary, ±30 ms single-rival margin) for the production placement,
      reusing `seam_prominence` and the `DUALFIT_SEAM_UNIQ_LAG_MS` convention so the numbers are
      comparable to `splice_dualfit`'s. Non-negotiable — without `seam_z` the Phase B roll-up cannot
      distinguish a real placement gain from search freedom, and prominence alone cannot: it reads low
      on correct-but-periodic content.
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
| Golden re-baseline churns unrelated axes | Held: purely additive, zero pre-existing values changed (fixtures untouched) |
| **Fixture refresh** churns unrelated axes | Expected, not preventable — separate commit, reviewed as a content change with a known cause per moved axis |
| Tier-1 placement ships as a null column that reads as coverage | `curated_golden_fill_placement_is_not_yet_armed` asserts the null and fails when fixtures are regenerated |
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
