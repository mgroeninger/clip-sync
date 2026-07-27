# Lever 1b(b) — band the start-search repeat window — plan

**Status: Phase 1–2 DONE (2026-07-26); Phase 3–4 next.** Successor to lever 1
(`archive/TEMP-production-repair-perf-plan.md` §2.5) and lever 1b(c) (`repair-perf.md` §1b). Largest remaining
single perf target on the production repair path. Flag `fft_repeat_band` is wired but **defaults OFF** until
§4 (belt) + §6 (corpus) are green.

**Media hygiene:** as everywhere in this repo, no media filenames / titles / paths. Corpus pairs by index only;
raw logs stay in gitignored `gap-files/`.

---

## 0. Measurement — 17 pairs, production path (2026-07-26)

Source: `gap-files/fill_length_slack_secs_narrow_perf/{1..17}.log`, the post-`fill_length_slack_secs`-narrowing
run (the current shipped default). Aggregated by span name over all 17 pairs; 1758 brackets.

**Total `patch_audio` busy: 5724 s (1.59 h).**

| stage | time | % root | inner split |
|---|---:|---:|---|
| `unified_refine_start` | 2541 s | **44.4%** | `repeat_us` **~2529 s (99.5%)**, structure ~7.6 s (0.3%), seam ~0, score ~2.5 s (0.1%) |
| `patch_decode_a` + `_b` | 2145 s | **37.5%** | sequential, ~1072 s each |
| `anchor_matchability` | 368 s | 6.4% | `local_anchor_xcorr` 365 s |
| `unified_coarse_start` | 322 s | 5.6% | repeat 205 s, seam 116 s |
| `unified_fine_polish` | 211 s | 3.7% | repeat 135 s, seam 75 s |
| `unified_refine_end` | 2.9 s | 0.05% | repeat **0** (hoisted, 1b(c)) |
| `unified_coarse_end` | 0.003 s | ~0% | — |
| fill assembly + splice | 9.6 s | 0.17% | — |

**`repeat_us` across all phases: ~2870 s ≈ 50% of root** (refine-start ~2529 + coarse-start ~205 +
fine-polish ~135). This plan targets the ~2529 s in `unified_refine_start`.

**Candidates per phase** (total / per bracket): `unified_refine_start` 8,440,158 / 4801 · `unified_refine_end`
8,440,158 / 4801 · `unified_coarse_start` 706,716 / 402 · `unified_fine_polish` 451,806 / 257 ·
`unified_coarse_end` 73,836 / 42.

**The two contrasts that motivate the lever:**

1. `unified_refine_end` evaluates the **same 8,440,158 candidates** as `unified_refine_start` for **2.9 s vs
   2541 s** — because 1b(c) proved its repeat penalty loop-invariant and hoisted it. The start side cannot
   hoist (the repeat window moves with `start`), so it needs banding instead.
2. Within the start search, the **banded** seam correlation costs 0.3 s / 8.44 M candidates (~0.04 µs each)
   while the **naive** seam in `unified_coarse_start` costs 115.8 s / 706 K (~164 µs each) — ~4600× on the
   identical operation. The repeat correlation is currently paying the naive rate.

> The earlier "43.1% of root, 99.4%" figure was the **§1b 4-pair post-hoist spot check**, not §1a (that was
> pre-hoist, ~30.7%). This supersedes it with the full 17-pair run on the current default; the conclusion is
> unchanged.

---

## 1. Why banding works here — the shape argument

`fill_repeat_correlations` (`policies/seam_scoring.rs:100`) is the **same form** the lever-1 seam band already
handles: a fixed A-side template slid against B at every `start`.

| | template | B base | start-dependent |
|---|---|---|---|
| `repeat_pre` (`:130`) | `a_pre` tail, len `w` | `start` | base only |
| `repeat_post` (`:143`) | `a_post` head, len `w` | `start + gap_frames − w` | base only (constant offset) |

The precondition is met: **window lengths are start-independent.** `effective_repeat_window_frames` (`:80`)
derives `w` from `repeat_window_frames`, `gap_frames`, `border_len`, `seam_window` — none move with `start`. All
start-dependent bounds are monotonic in `start`, so lever 1's "uniform across the band or return `None`" guard
(`fill_seam_correlations_band`, `:877-880`) ports directly.

Underlying primitive is unchanged: `seam_correlation_over_bases` (`domain/seam_local.rs:143`) →
`lag_correlation_curve_auto` (FFT numerator + prefix-sum denominator). Do not hand-roll.

---

## 2. Phase 1 — `fill_repeat_correlations_band` **(DONE 2026-07-26)**

New fn in `seam_scoring.rs`, modeled on `fill_seam_correlations_band` (`:852`), signature
`(templates, gap_frames, pre_window, post_window, repeat_window_frames, start_lo, start_hi) -> Option<Vec<(f64, f64)>>`.

**Landed as designed.** `fill_repeat_correlations_band` + the private `repeat_side_band` helper (one side, with
`tail` selecting both template end and base offset — that pairing is what lets one helper serve pre and post),
plus `fill_repeat_correlations_band_matches_per_start`. `FFT_CROSSOVER_OPS` is now `pub(crate)` so the test can
**assert** its branch rather than assume it. Lib tests pass; both CI clippy invocations clean. The temporary
`#[allow(dead_code)]` from the un-wired land was **removed in Phase 2** when `build_repeat_band` wired the call
site.

**Mutation-checked, not just green.** Two deliberate breaks were confirmed to fail the test rather than slip
through: (a) dropping the outer channel gate (§2.1 #3) → case C returned real correlations 0.024 / 0.079 where
naive returns 0.0; (b) swapping mono's failure value `0.0` → `NEG_INFINITY` (§2.1 #5) → case C returned `-inf`
vs 0. Both are exactly the silent-divergence classes this fn was at risk of.

### 2.1 Where repeat differs from seam — the exactness traps

A copy-paste of the seam band is wrong in six specific ways. Each is invisible in the common bracket and wrong
exactly at the edges the band is supposed to decline.

1. **All channels, no `score_channels` filter.** `fill_repeat_correlations` iterates
   `a_pre_ch.iter().zip(b_ch.iter())` (`:156-157`) over every channel. The seam band takes a `score_channels`
   list; the repeat band must not.
2. **Per-channel window lengths differ.** Each channel recomputes `w` from its own `border_len` (`:161-166`),
   so one template length cannot be shared across channels — it is one FFT per channel per side
   (**up to `2×(1 + N_ch)`**, ~14 on 6-channel media; not the seam band's shared-`w` count), versus ~34 K
   naive Pearsons per bracket today. The saving survives this easily, but it does mean the realized ratio
   will land below the seam band's.
3. **The outer channel gate uses the *mono* window.** `:155` / `:181`:
   ```rust
   let ch_pre = if start + pre_repeat_window <= b_mono.len() { /* fold over channels */ }
                else { f64::NEG_INFINITY };
   ```
   `pre_repeat_window` is derived from `a_pre.len()`, while each channel's `w` may be **shorter**. So a channel
   whose own window would fit is still excluded when the mono window overruns. Post side likewise: the outer
   gate's `tail_start` is built from the mono `post_repeat_window` (`:138`) but the inner `tail` uses the
   channel's `w` (`:193`) — two different bases, both required. **Banding only the per-channel
   `start + w <= b_ch.len()` checks silently changes results.**

   This gates a *channel* decision on the *mono* buffer's length, which reads as a latent oddity in the
   original. Exactness is the requirement — reproduce it as-is. **Not a cleanup to fold into this change.**
4. **Three uniformity checks per side, not one.** The outer channel gate (`:155`) and the mono scoring gate
   (`:127`) share the same start-dependent term `start + pre_repeat_window <= b_mono.len()`, differing only in
   the start-independent conjunct `pre_repeat_window <= a_pre.len()`. So one monotonic check covers both — but
   it feeds **two different failure values** (see #5). Plus the per-channel check. Same three on the post side.
5. **Failed gates yield `0.0` on mono (`:135`, `:148`), `NEG_INFINITY` on the channel set (`:178`, `:205`)** —
   and both still feed the max. Crossing these is the most likely silent break.
6. **The combiner is not `combine_seam_band`.** Repeat uses `best_channel_correlation(&[repeat_pre, ch_pre])`
   (`:209-210`), where mono is a *participant* in the max and `ch_pre` folds to `NEG_INFINITY` on an empty
   channel set. The seam's combiner treats mono as a fallback used only when no channel scored. Needs its own
   combiner.

### 2.2 Test (lands with the fn)

Modeled on `seam_correlation_over_bases_matches_naive` (`seam_local.rs:253`): band vs per-start naive across
the band, asserting **both** branches of `lag_correlation_curve_auto` (below and above `FFT_CROSSOVER_OPS`, with
the branch asserted so the case can't silently drift), multi-channel with **unequal per-channel windows**, and
explicit coverage of each decline path in §2.1 #3-#5 (outer gate fails / mono gate fails / per-channel gate
fails, band must decline or match).

---

## 3. Phase 2 — thread it into the start refine **(DONE 2026-07-26)**

**Landed as designed.** `build_repeat_band` sits beside `build_wave_seam_band` (`:597` / `:622`); both are built
alongside each other at the start refine (`wave_band` `:987`, `repeat_band` `:995`). The two bands are
**independent**: either may return `None` (flag off, non-uniform edge, or zero-weight early-out) without
forcing the other onto the naive path. `consider` takes an optional looked-up `(repeat_pre, repeat_post)` per
start (`precomputed_repeat`) the same way `precomputed_wave` works — the banded path does not re-enter
`fill_repeat_correlations`.

**`None` → naive fallback, not "no penalty".** When the band is absent the refine loop passes
`precomputed_repeat = None` → `RepeatPenaltySource::PerCandidate`. That covers both `None` meanings:
zero-weight/zero-window (early-out returns 0.0) and a non-uniform decline (full naive Pearson).

**Split `repeat_penalty_at_placement` (`:478`) at the seam between expensive and cheap** — this is what keeps
the two penalty-source meanings from being overloaded:

```rust
struct RepeatCorrelations { pre: f64, post: f64 }

// expensive: the Pearson pair (`:412`)
fn repeat_correlations_at_placement(..) -> RepeatCorrelations

// cheap: the branch logic (`:444`), UNCHANGED
fn repeat_penalty_from_correlations(
    corr: RepeatCorrelations, pre_seam: f64, post_seam: f64,
) -> f64

enum RepeatPenaltySource {                       // `:515`
    PerCandidate,               // correlate, then combine
    Banded(RepeatCorrelations), // look up, then combine
    Fixed(f64),                 // finished penalty — no combine (1b(c) end-search hoist)
}
```

**`Banded` carries correlations, `Fixed` carries a finished penalty.** They are not interchangeable: the banded
path must still run the `wave_min` / `asymmetric_post_dup` / `repeat_sum` branching in
`repeat_penalty_from_correlations`, because that branching is `wave_min`-dependent and therefore genuinely
per-candidate. Carrying a distinct type (not a second `f64`) makes skipping it inexpressible rather than merely
discouraged.

The branch logic itself stays untouched and per-candidate — it is pure arithmetic, already measured inside
`score_us` at ~2.5 s total across 17 pairs. Only the two Pearson calls become lookups.

The `repeat_penalty_weight <= 0.0 || repeat_window_frames == 0` early-out also lives on the band **build**, so
a zero-weight bracket does not pay for an FFT it discards (the per-candidate path still holds the same guard).

The old "no hoist available" comment on the start refine was rewritten to name banding as the other route.

Flag plumbing from §5 landed early with this phase (`RepairConfig.fft_repeat_band` default `false`,
`--fft-repeat-band`, harness mirror) so the wired path is opt-in until §4 + §6.

---

## 4. Phase 3 — extend the exact re-score belt **(DONE 2026-07-26)**

The belt re-scores the winner's `wave_min` naively and, on divergence beyond `FFT_SEAM_DISCREPANCY_TOL`,
degrades that gap to a naive refine with a `warn`. It did **not** check the repeat term, which after Phase 2
also feeds the score.

**Landed as designed.** The repeat check (`:1091`) re-scores via `repeat_correlations_at_placement` at the
winner and compares against `repeat_band[best_start - refine_min]` **pre-branch** — the correlation pair
itself, not the penalty. That matters concretely: `repeat_penalty_from_correlations` is a step function around
`REPEAT_CORR_THRESHOLD` / `REPEAT_SEAM_WEAK`, so a penalty-level comparison would read as *agreement* for any
FFT error contained within one branch, and as a *large disagreement* for a 1e-12 error that happens to straddle
a threshold. Neither says anything about the FFT. Intrinsic, not flagged (lever 1's rule).

**Two checks, one fallback** (`:1063` / `:1115`). Each band is checked independently — either divergence
condemns the winner, since both feed the same score — but they share a single naive re-refine, which passes
`None` for both bands and is therefore exact for whichever diverged *and* for the other. A double divergence
costs one re-refine, not two.

**Shared agreement predicate** `band_agrees_with_naive` (`:43`), extracted so the two bands cannot drift apart
on edge handling. Its `naive == band` arm is load-bearing rather than a fast path: both bands legitimately
produce infinities at declined placements (the seam band's out-of-bounds `NEG_INFINITY`, the repeat band's
empty-channel-set fold), and `inf − inf` is `NaN`, which no threshold comparison can classify.

> **One deliberate behaviour change on the seam side.** The old inline test was
> `!both_neg_inf && (naive − band).abs() > TOL`. A `NaN` on either side made that comparison `false` and was
> silently accepted as agreement. The extracted predicate reports `NaN` as a **divergence**. Every other case
> (both `−inf`, equal finites, `abs == TOL` exactly, one side infinite) is bit-identical to the old test. The
> naive path does not produce `NaN` (`seam_pearson` returns 0.0 on degenerate input), so this cannot fire on
> correct behaviour — it closes a hole rather than changing a decision.

Pinned by `band_agreement_treats_infinities_as_equal_and_nan_as_divergence` (`:2367`).

---

## 5. Phase 4 — flag and defaulting

**Plumbing landed with Phase 2** (`RepairConfig.fft_repeat_band`, `--fft-repeat-band`, harness mirror of the
production default). Still **default `false`**. §4 (belt) is now green, so the only remaining precondition for
flipping to `true` is **§6 (3)** — the 17-pair corpus run diffed against the committed goldens.

- Config: `infrastructure/config.rs` (`fft_repeat_band` beside `fft_seam_search`). Harness:
  `clip-sync-repair-harness/src/patch_audio.rs`.
- Same scoping rule as lever 1 (`archive/TEMP-production-repair-perf-plan.md` §2.4): **production search only.**
  The fingerprint dump / committed corpus / goldens stay on the naive path — no golden re-freeze.

---

## 6. Validation order

1. Band-vs-naive ε unit test (§2.2), both crossover branches, unequal per-channel windows, all decline paths.
2. **DONE (Phase 2).** Placement-diff test — naive vs banded `start_frame`, modeled on
   `fft_seam_search_matches_naive_placement` (`gap_fill_fit.rs:2074`):
   `fft_repeat_band_matches_naive_placement` (`:2198`) holds the *seam* band OFF in both arms so a divergence
   can only be the repeat band, uses `repeat_penalty_weight = 0.4` (at 0.0 the band is never built and the test
   would be vacuous), and gives the templates **two channels of unequal border length** — the §2.1 #2/#3
   configuration a per-channel-bounds-only band would get wrong.
3. 17-pair corpus run flag-on, gap outcome tables diffed against the **committed goldens** (the durable
   reference; `gap-files/` is ephemeral). Placement must be **identical**, not merely close — the belt makes
   divergence loud, but the goldens are what proves it.
4. Re-run the §0 recipe (`measure-repair-perf.ps1`) for the measured speedup, and record it in
   `repair-perf.md` §3 no-regression record.

Like lever 1, this changes the numeric path at ~1e-10, so it is **not** strictly byte-identical and is the same
declared exception to the `patch_audio_integration` byte-parity rule — gated by (1)+(2)+(3) instead.

---

## 7. Expected payoff

If the band approaches the seam band's per-candidate cost, `unified_refine_start` drops from 2541 s toward its
`structure_us` + `score_us` floor (~10 s): **~44% off total wall**, 1.59 h → ~0.89 h across 17 pairs. Discount
for §2.1 #2 (per-channel FFTs). The FFT *build* cost lands in `bracket_unified_search`'s own exclusive time
(same place as the seam-band build today, currently ~26 s) — so the loop floor is not the whole story, but
build cost should still be small vs 2529 s. Measure the span partition, not just wall clock.

---

## 8. Non-goals / deferred (do not re-propose without new measurement)

- **`unified_coarse_start` (205 s repeat) and `unified_fine_polish` (135 s repeat) stay naive.** Lever 1
  deliberately kept the coarse pass naive so the coarse winner anchoring the refine window is bit-identical,
  which bounds any FFT-induced move to *within* the ±`coarse_step` refine band. Banding the coarse pass would
  give that invariant up for 3.6% of root. `fine_polish` is a separate, smaller follow-up at most.
- **End-side anything.** `refine_end` + `coarse_end` are 0.05% combined; 1b(c) already took that cost to zero,
  and the `fill_length_slack_secs` narrowing confirmed the remainder is noise (coarse-end candidates
  **202 → 42** on the 4-pair hoist spot check; end-search busy **−40 ms / 4 pairs** — not a meaningful win
  post-1b(c)).
- **Decode parallelism (2145 s, 37.5%, strictly sequential — `application/patch_audio/decode.rs:24-80`).** A
  real and larger-than-expected target, orthogonal to this plan; B's track selection needs only A's track
  *metadata*, not `a_pcm`, so the two `extract_interleaved` calls are separable. Needs its own measurement
  first (both files on one drive — if decode is I/O-bound the win collapses). **Tracked separately; do not fold
  into this plan.**
- **`local_anchor_xcorr` (365 s, 6.4%)** — the remaining item above 1%, still unattacked. Not blocked by this.

---

## 9. References

- [repair-perf.md](repair-perf.md) — live profile; §1a Level F buckets, §1b the 1b(c) hoist, §5 open candidates.
- [archive/TEMP-production-repair-perf-plan.md](archive/TEMP-production-repair-perf-plan.md) §2.5 — lever 1
  design (FFT + exact re-score belt), §2.4 calibration-safety scoping.
- `policies/seam_scoring.rs:100` `fill_repeat_correlations` · `:852` `fill_seam_correlations_band` (the model)
  · `:347` `fill_repeat_correlations_band`.
- `domain/seam_local.rs:143` `seam_correlation_over_bases` · `:255` its band-vs-naive test (the model).
- `domain/gap_fill_fit.rs:472` `repeat_penalty_at_placement` · `:509` `RepeatPenaltySource` · `:616`
  `build_repeat_band` · `:1032` the (still seam-only) belt.
