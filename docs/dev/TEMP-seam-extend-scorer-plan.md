# TEMP — Score the splice that `--seam-extend-fill` writes

**Status:** implemented 2026-10-21 (uncommitted), zero-pad guard included. Flag-on corpus run done
2026-10-22 (§ 7): no verdict changes, 68 fills move, and the move interacts with dual-fit
registration routing. Open: read § 7's findings before closing. One Change.
Tracked as [BACKLOG.md](../../BACKLOG.md) § *Fill placement*, row *Default-on `--seam-extend-fill`*:
this plan clears that row's blocker. Flipping the default is a **separate** step, gated on a
bracket-path ear pass, and is not part of this Change.

Companion: [gap-fill-modes.md](../gap-fill-modes.md) § *Seam crossfade and `--seam-extend-fill`*,
[seam-scoring.md](../seam-scoring.md) § 4.

---

## 1. Problem

The splice-aware seam scorer (`fill_splice_seam_correlations[_interleaved]`, its
`single_lag_alignment` crossfade-window branch) scores the **plain** splice's geometry:

| Seam | A window | B window (plain) |
|------|----------|------------------|
| pre  | `A[gs − w .. gs]` | `fill[cf − w .. cf]` (the fill head, bled over A's pre tail) |
| post | `A[ge .. ge + w]` | `fill[len − cf .. len − cf + w]` (the rewound fill tail) |

with `w = min(seam_window, cf)`. Under `--seam-extend-fill` the splice writes something else —
`head` (B immediately **before** the fill) over A's pre tail and `tail` (B immediately **after**
it) over A's post head. The scorer has no branch for that, so on the flag arm two decisions are made
on a layout that is never written:

1. **Fit trim-head vs trim-tail** — `pick_fill_length_anchor`, through `fill_anchor_better`. This
   changes which B frames are written.
2. **Fit report-vs-splice reconciliation** in characterize. Adopts the splice score only when
   `min(splice) ≥ min(report)`, so it can upgrade a tier on the flag arm, never cause a skip. It
   also sets `used_splice` provenance and the reported `pre/post_correlation`.

In-process probe, 2026-10-21 (harmonic + noise, 48 kHz, `cf` = 480, throwaway test, reverted):

| Fill shift (frames) | Plain scorer pre / post | Extended geometry pre / post |
|---|---|---|
| −480 | **+1.000** / −0.665 | +0.192 / +0.054 |
| **0 (exact)** | **+0.156 / +0.054** | **+1.000 / +1.000** |
| +480 | −0.578 / **+1.000** | +0.156 / +0.232 |

The extended geometry peaks at the registration the placement search already assumes
(`b[start − w .. start]` against A's pre border, seam-scoring.md § 4). The plain geometry peaks at
`−cf` on one seam and `+cf` on the other, because the plain post blend rewinds the fill by `cf`.

**Out of scope.** Dual-fit passes `single_lag_alignment: false` and scores border templates, so it
is unaffected. Gate mode never calls the splice scorer. The plain arm is left byte-identical: its
scorer does describe what the plain splice writes, however incoherent that write is.

## 2. Design decisions

### D1 — Replace `single_lag_alignment: bool` with a geometry enum

`SpliceSeamContext` gets one field naming what the splice will write, instead of a bool plus a new
optional margin:

```rust
pub enum SpliceSeamGeometry<'a> {
    /// Dual-fit: shoulders at their own lags; score the border templates (today's `false`).
    BorderTemplate,
    /// `apply_seam_crossfade` (today's `true`).
    PlainCrossfade,
    /// `apply_seam_crossfade_extended`: B either side of the fill, cut from this timeline.
    ExtendedCrossfade { b_full: &'a [f32], fill_start_frame: usize },
}
```

**Why an enum:** the three are mutually exclusive, and "extended" is only meaningful for a
single-lag fill. A bool plus `Option<margin>` would allow `single_lag_alignment: false` with a
margin, which is a state no splice produces. Same reasoning as BACKLOG's `DualFitPlacement` row.
There are nine construction sites, all mechanical.

**Why a B timeline plus a start, and not pre-cut margins:** the context is built once per gap, but
`pick_fill_length_anchor` scores **two** candidates, whose margins differ (trim-head's fill starts
`skip` frames later). Holding the timeline lets the picker derive each candidate's geometry with
`fill_start_frame + skip`. The context stays `Copy`.

### D2 — One margin cutter, shared by the splice and the scorer

The scorer must fall back to plain **exactly** when the splice does. The splice falls back when
`cf == 0` or a margin is short, and the margin is `None` at the edges of B. To make the two agree
by construction, move the cut into the domain (`seam_splice.rs`) as a borrowing function:

```rust
pub fn seam_margin_slices(b_full: &[f32], channels: usize, fill_start_frame: usize,
                          gap_frames: usize, cf: usize) -> Option<(&[f32], &[f32])>
```

`pcm::seam_margin_from_b` becomes a `.to_vec()` wrapper over it. The scorer calls it with the same
`cf` (`ctx.seam_cf`, from the same `effective_seam_crossfade_frames` call the executor's margin cut
uses) and the fill's length. A `None` result means score as `PlainCrossfade`, which is the splice's
own fallback.

### D3 — Margins come from full B, matching the executor

The executor cuts margins from **full B** at
`b_extract.start_frame + alignment.start_frame + head_trim_frames`. The scorer must read the same
frames, so `ExtendedCrossfade` carries full B, not the extract, and the start is computed by **one**
helper used at all three sites (executor margin cut, characterize reconciliation, assembly). Using
the extract instead would disagree with the splice whenever the fill sits within `cf` of an extract
edge where full B continues.

### D4 — Extended scoring windows

The same `w` as the plain branch, and only the B side changes:

| Seam | A window (unchanged) | B window (extended) |
|------|----------|------------------|
| pre  | `A[gs − w .. gs]` | `head[cf − w .. cf]` |
| post | `A[ge .. ge + w]` | `tail[.. w]` |

Per-channel and mono variants both get the branch. The interleaved entry point de-interleaves the
margins the same way it already de-interleaves the fill.

### D5 — The flag reaches every scorer that feeds a written fill

The executor **re-derives** the fill (the live path); characterize's assembly is a scored-and-discarded
shadow, checked by a `debug_assert_eq!`. Both must build `ExtendedCrossfade` under the flag. If only
characterize does, the executor writes a different trim than was scored. If only the executor does,
the debug shadow fires on every flag-arm trimmed gap. So `ExecuteBracketFillCtx` and the characterize
assembly both take `seam_extend_fill` plus full B and the extract start. The reconciliation call takes
the same geometry, at the assembled fill's start (`+ head_trim_frames`).

## 3. Behaviour contract

- **Flag off:** byte-identical. `PlainCrossfade` is today's `true` branch, unchanged. Goldens must
  not move. This is the acceptance check that the refactor (D1, D2) is pure.
- **Flag on:** trim choice and reconciliation scores may change, and that is intended. JSON shape is
  unchanged (v1 frozen); only values move, and only under a non-default flag.
- **Fallback:** where margins cannot be cut, the flag arm scores and splices plain, as today.

## 4. Tests

1. **Geometry pin (domain):** the probe as a permanent test. Deterministic harmonic + LCG noise,
   a fill at exact registration: `ExtendedCrossfade` scores ≥ 0.99 on both seams, and `PlainCrossfade`
   scores below 0.5 on at least one seam on the same fixture. A ramp is useless here, because Pearson
   is blind to shifting a ramp.
2. **Fallback parity:** a fill within `cf` of the end of B. `ExtendedCrossfade` returns bit-identical
   scores to `PlainCrossfade`, and `seam_margin_slices` returns `None`.
3. **Cutter parity:** `pcm::seam_margin_from_b` equals `seam_margin_slices(...).to_vec()` on a ramp
   (replaces nothing; guards D2).
4. **Trim pick:** a bracket `skip` frames longer than the gap, truth = one trim. Pick a `skip` on the
   noise fixture where plain and extended disagree (find it empirically when writing the test), and
   assert the extended picker chooses the true trim.
5. **Shadow parity under the flag:** a Fit bracket integration case with `seam_extend_fill = true`
   and a trimmed fill, run in debug so the characterize/executor `debug_assert_eq!` is exercised.
   `seam_extend_fill_changes_the_fill_and_nothing_outside_it` keeps passing.
6. **Flag-off goldens:** `test-tier.ps1 -Tier pr` with no golden diffs.

## 5. Checklist

- [x] `domain/policies/seam_scoring.rs`: introduce `SpliceSeamGeometry`, replace `single_lag_alignment`
      at all `SpliceSeamContext { .. }` sites (`rg "SpliceSeamContext \{"`), with `true` → `PlainCrossfade` and
      `false` → `BorderTemplate`. No behaviour change, so commit separately and run the PR tier.
- [x] `domain/policies/seam_splice.rs`: add `seam_margin_slices`. Make `apply_seam_crossfade_extended`'s
      `have_margin` predicate and the scorer's fallback agree (same `cf`, same lengths).
      `application/patch_audio/region/pcm.rs`: `seam_margin_from_b` wraps it.
- [x] Extended branch in `score_splice_pre_seam[_channel]` / `score_splice_post_seam[_channel]`, plus
      margin de-interleave in `fill_splice_seam_correlations_interleaved`.
- [x] `domain/gap_fill_fit.rs` `pick_fill_length_anchor`: derive the trim-head candidate's geometry as
      `fill_start_frame + skip`. Trim-tail keeps `fill_start_frame`.
- [x] One helper for the bracket fill's full-B start. Use it in `region/execute.rs` (margin cut), in
      `region/bracket.rs` `execute_bracket_fill_detail` (new ctx fields), and in `region/characterize.rs`
      (assembly + reconciliation call).
- [x] Tests 1–4; test 5 only partly (the existing flag-on integration case runs the debug shadow, but
      nothing forces its fill to be trimmed). Then the PR tier, `cargo clippy -p clip-sync-repair --all-targets -- -D warnings`,
      and `cargo fmt --all -- --check`.
- [x] Docs: gap-fill-modes.md § *Seam crossfade* (the flag now also changes scoring); seam-scoring.md
      § 4 (splice-aware windows per geometry).
- [ ] Close: update the BACKLOG row (blocker cleared, next = bracket-path ear pass + default flip),
      archive this plan, and add the settled.md row.

## 6. Open questions

- **Short (zero-padded) fills under the flag.** The executor cuts `tail` from full B at
  `fill_start + gap_frames`, even when the fill was padded because the **extract** ran out. Full B
  may continue there, so the tail margin would be B content discontinuous with the padding. The
  2026-09 census found 0 short fills / 209, so this is latent. D3 makes the scorer inherit the same
  behaviour, which keeps the two consistent. Fixing it (no margin when `zero_pad_frames > 0`) is a
  one-line guard in the shared helper; decide whether it rides this Change. **Decided: it rides.**
  `bracket_fill_margin_start` returns `None` when `zero_pad_frames > 0`, for the splice and the
  scorer alike.
- **How often decision 1 actually flips on the corpus.** It is unknown until a flag-on corpus run. Not
  needed to land this Change, because the flag is off by default. It is needed before the default flip.

## 7. Corpus run (2026-10-22)

Two `measure-patch-outcomes.ps1` arms over the full manifest (85 pairs), one release binary of this
tree: `gap-files/2026-10-21-sef-off` (default) and `gap-files/2026-10-21-sef-on`
(`-RepairArgs "--seam-extend-fill"`). Every pair exited 0 in both. Diffed with
`diff-patch-outcomes.ps1 -NoFailOnUnexplained` (its CHANGED PATHS table is the summary below).

- **Gap set and verdicts: identical.** 1775 joined gaps, 0 unjoined, 0 status flips, 0 reason
  changes, 0 independent run-level changes. Only the derived `patched_marginal_count` moved.
- **188 patched gaps changed fields.**
  - **120 score-only.** Reported `pre_correlation` / `post_correlation` moved, nothing about the
    placement did. Those are the reconciliation scores re-measured in the new geometry. They are not
    comparable across arms, because the two arms score different frames.
  - **68 moved the fill.** `registration_applied_ms` changed, which is
    `alignment.start_frame + head_trim_frames − nominal`.
- **Confidence tier:** marginal → high on 27 gaps, high → marginal on 15.
- **33 of the 68 are pure trim flips.** Their placement-search seam scores are bit-identical and only
  the trim moved, i.e. decision 1 flipped. Against the cross-arm-comparable residual (the max
  `|registration_residual_{pre,post}_ms|`), 14 got better, 18 got worse and 1 tied.
  - Worst: 13/g27 went 11.0 → 127.3 ms; 33/g21 went 11.9 → 77.4 ms; 85/g10 went
    12.9 → 60.8 ms.
  - Best: 1/g14 went 98.1 → 7.6 ms; 80/g5 went 62.4 → 3.8 ms.
  - The synthetic fixture's "extended is always right" does **not** carry over to real content as a
    clean win.
- **35 of the 68 moved the placement-search seams as well.** Of the 30 that are comparable and not
  dual-fit, the residual got better on 19, worse on 10 and tied on 1.
- **Dual-fit registration routing changed on 4 gaps.** The single-lag fill moved, so its residual
  against the shoulders crossed the routing bound (`dual_fit_registration_bound_ms` /
  `_common_bound_ms`) in the other direction:
  - **Lost the rescue:** 85/g86 (common), 13/g16 (step) and 19/g2 (step). The flag-on arm
    keeps a single-lag fill, with seams of 0.48/0.48, 0.82/0.79 and 0.59/0.58. The flag-off arm's
    dual-fit fills scored 0.999/0.954, 0.994/0.984 and 0.992/0.964 (border templates, the same
    geometry in both arms). For example, 13/g16's flag-on single-lag fill sits 41.8 / 4.8 ms off its
    shoulders, a 37 ms step that is under the 75 ms step bound, so routing never tries dual-fit.
  - **Gained the rescue:** 3/g12 (step). The flag-off single-lag fill scored 0.28/0.29; flag-on
    dual-fit scores 0.89/0.97.
  - The routing bounds were set by ear on flag-off placements
    ([archive/TEMP-dual-fit-registration-routing-plan.md](archive/TEMP-dual-fit-registration-routing-plan.md)),
    so they are not validated under the flag.

**What this means for the default flip** (BACKLOG § *Fill placement*): the scorer change does what it
says, but its corpus effect is mixed by the one cross-arm measure we have. The routing bounds also need
a re-read under the flag. Before the default flip:
1. ear-check the worst trim flips (13/g27, 33/g21) and the three lost rescues;
2. decide whether routing should compare against dual-fit's own seam scores rather than a residual
   bound alone.

### 7a. Listen set (rendered 2026-10-22)

`--gap-listen` renders of the five gaps, at the same build in both arms (`off` = default, `on` =
`--seam-extend-fill`). Renders are in `gap-files/2026-10-22-listen-sef/`. The blind set is
`gap-files/2026-10-22-blind-sef/`: 12 trials, made of 2 real trials per gap plus one null per arm. The
key is in `gap-files/2026-10-22-blind-sef-key/`. Scoring follows the routing plan § 6d: a gap counts
only if its two trials agree.

Render checks: the A and B clips are bit-identical across arms, and so is the patched A outside the
written region. The measured fill lag (40 ms windows just inside each end of the write, against
`b_surround`) matches the report on every gap:

| Gap | Stratum | off: fill lag (head / tail) | on: fill lag | on − off |
|-----|---------|-----------------------------|--------------|----------|
| 13/g27 | trim flip | +289.6 / +289.6 ms | +163.4 ms | −126 ms |
| 33/g21 | trim flip | +25.6 / +25.6 ms | −49.9 ms | −76 ms |
| 85/g86 | lost rescue (common) | −322.5 / −318.4 ms (dual-fit) | −356.1 ms | — |
| 13/g16 | lost rescue (step) | +246.1 / +283.1 ms (dual-fit) | +277.8 ms | — |
| 19/g2 | lost rescue (step) | +16.3 / +80.2 ms (dual-fit) | +67.3 ms | — |

On both trim flips the off-arm audio sits exactly one crossfade (+10 ms) past its reported
`registration_applied_ms`. This is the plain-crossfade +1 displacement documented in
[gap-fill-modes.md](../gap-fill-modes.md). The on arm matches its report to the frame. 13/g16 also
writes a longer region on the on arm: 1.042 s against 0.642 s.

Confounds to keep out of the README:
- **Trim flips:** the off arm also carries the post-seam crossfade duplication (~20 ms replayed). A
  verdict for `on` there is not attributable to placement alone.
- **Lost rescues:** the off arm is a dual-fit bridge, which the flag does not change. That comparison is
  dual-fit against the flag's single-lag fill.

### 7b. Listen result (2026-10-22, blind, 12 trials)

| Gap | Stratum | Trials (unblinded) | Verdict |
|-----|---------|--------------------|---------|
| 13/g27 | trim flip | same, same | **no audible difference** |
| 33/g21 | trim flip | on, on | **on better** (confounded, see below) |
| 85/g86 | lost rescue (common) | on, same | does not count (disagree) |
| 13/g16 | lost rescue (step) | on, on | **on better**: losing dual-fit helped |
| 19/g2 | lost rescue (step) | off, off | **off better**: a real loss |
| nulls | off/off, on/on | same, same | clean |

Noise k = 1: no null was heard as different, and 85/g86's two trials disagreed.

Readings:
- **The cross-arm residual metric does not predict the ear.** The "worst" trim flip, 13/g27 (11 → 127 ms),
  is inaudible. 33/g21 (12 → 77 ms) is *preferred* on. The 33/g21 win may come from removing the
  off arm's post-seam crossfade duplication rather than from the placement, but it is not a placement
  loss either. Neither gap supports holding the default flip on trim-flip grounds.
- **The lost rescues split 1 – 1 – 1.** That is no evidence that the flag's routing changes hurt in
  aggregate. Question 2 above needs care: a rule comparing dual-fit's own seam scores keeps dual-fit on
  both 19/g2 (0.99/0.96 vs 0.59/0.58) and 13/g16 (0.99/0.98 vs 0.82/0.79). It would get 19/g2 right and
  13/g16 wrong, so seam scores alone do not order these either. 85/g86 stays a weak-signal gap, as in
  the routing plan § 6e.
- The one real loss, 19/g2, is a candidate for a targeted look at why the flag's residual lands under
  the step bound there. Its off-arm dual-fit lags are 16 / 80 ms, a 64 ms step.
