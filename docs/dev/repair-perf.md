# Repair performance — measurement reference

**Durable reference, not a plan.** Where repair time goes, how to measure it, and
which candidates are already settled. Last full 17-pair measurement
**2026-07-25** (§1a, production `--wav` path, Level F instrumentation).
**Everything measured on 17 pairs predates lever 1b(c)** — the only post-hoist
data is the 4-pair spot check in §1b. Read §1a + §1b together for the current
shape; §1 (2026-07-24) is kept for the pre-Level-F span partition.

Update this doc when a new sweep lands — do not start a second baseline
elsewhere. Campaign-specific reasoning belongs in a `TEMP-*` plan; the *numbers*
belong here.

---

## Media handling — read before recording anything

All figures here come from runs over **licensed media**, referenced only by
gap-fingerprint corpus **pair index**. The pair-index → media mapping is
deliberately not in this repository; it exists only in the gitignored source map
(`*.sources.local.toml`) alongside the gitignored `gap-files/`.

Raw logs contain absolute media paths and **must not be committed**. The logs
backing the tables below live under `gap-files/`, which `.gitignore` covers —
`perf-baseline-2026-07-23/` (§2), `perf-gate-2026-07-24/` (§1, §3),
`perf-gate-2026-07-25-2/` (§1a), `perf-gate-hoist/` (§1b), plus the fingerprint
dumps in `fingerprint-corpus/` (§5 #3). When recording a result, carry over the
derived numbers and the pair index — nothing else. Grep before committing.

## How to measure

`scripts/measure-repair-perf.ps1`. Two modes: `-Manifest pairs.csv` runs the
pairs, `-Logs dir/` rolls up logs you already have (tolerates a run still
appending). `-Focus <spans>` puts named spans against the root; `-MinPct` trims
deep trees. Every report prints the span tree, the exclusive by-name roll-up, and
the Level F candidate-loop component split (§1a).

The harness strips ANSI escapes before parsing. It has to: the stderr fmt layer
is always colored (only the `--log-file` layer sets `with_ansi(false)`) and this
harness captures stderr, so an un-stripped log parses as zero span lines and
reports as empty.

Non-negotiables — the measurement is invalid otherwise:

- **`--release`.** Debug timings are meaningless, and debug-only `debug_assert`
  shadows re-run real work (the fill-assembly shadow calls
  `execute_bracket_fill` an extra time).
- **A real write path (`--wav`).** `--repair-preview` stops after characterize,
  so nothing on the executor side runs at all. `--gap-fingerprints` is the
  dump/oracle path and enumerates brackets **exhaustively** instead of
  short-circuiting at the first winner — an upper bound, not the production
  shape (287 vs 192 brackets on pair 1). For bulk fingerprint dumps (same
  manifest format, no span timing), use `scripts/measure-gap-fingerprints.ps1`
  — see [gap-fingerprint.md](gap-fingerprint.md).
- **`CLIP_SYNC_SPAN_TIMING=1`**, or no timings emit. It switches the fmt
  subscriber to `FmtSpan::CLOSE` (`clip-sync/src/infrastructure/logging/mod.rs`).

**Reading the output.** Spans nest, and a parent's `time.busy` **includes** its
children's — so inclusive time never sums to 100% and cannot answer "where does
the time go". **Exclusive** time (own busy minus direct children's) is a
partition of the root and does. A large exclusive cost on a span that *has*
children is an **instrumentation gap**, not a leaf hotspot; the harness flags any
parent holding ≥10%.

---

## 1. Where the time goes — 17 pairs, production path (2026-07-24)

**Historical: pre-Level-F and pre-lever-1b(c).** The span names here no longer
exist (see "Span renames" below) and the end loops this table charges for have
since been hoisted away. Kept because it is the last full 17-pair *tree*, and
because §3's no-regression rows are measured against it. For the current shape
read §1a (full corpus, Level F) and §1b (post-hoist, 4 pairs).

Exclusive time, merged by span name. Root = `patch_audio`, **9215 s**.

| Span | Exclusive | Share | Calls |
|------|-----------|-------|-------|
| `unified_refine` | 5198.1 s | **56.4%** | 3516 |
| `patch_decode_a` | 1197.2 s | **13.0%** | 17 |
| `patch_decode_b` | 1157.5 s | **12.6%** | 17 |
| `gate_anchor_search` (own) | 910.8 s | **9.9%** | 81 |
| `unified_coarse` | 408.4 s | 4.4% | 3516 |
| `unified_fine_polish` | 216.5 s | 2.4% | 1758 |
| `patch_audio` (own) | 40.1 s | 0.4% | 17 |
| `bracket_unified_search` (own) | 26.8 s | 0.3% | 1758 |
| everything else | 59.6 s | 0.6% | — |

`char_gate_search` is **73.8%** of these runs (inclusive), down from 93.3% on the
2026-07-20 baseline — not a regression, see §3.

`unified_refine` at 56.4% is the single dominant cost (1.47 ms/call over 3516
calls). It is what lever 1's FFT already attacked; the `ExclSecs` column is what
a future attempt must be measured against.

### Span renames since this table (re-measure before comparing)

The candidate loops now carry a `_start` / `_end` suffix — `unified_refine_start`
/ `unified_refine_end`, `unified_coarse_start` / `unified_coarse_end`. The two
searches run **structurally different loop bodies**: the end search hoists the pre
structure score and *both* seam values out of its loop
(`unified_search_best_fill_end`), so its per-candidate cost is far lower. The old
shared name made the by-name roll-up average two populations, and the resulting
1.47 ms/call figure above describes neither of them. `unified_fine_polish` is
unchanged (one loop, no ambiguity).

## 1a. Level F — what *inside* a candidate loop costs (2026-07-25)

`unified_refine` has no child spans, so the tree can only attribute its 56.4% to
"the loop body". The body is not uniform work: it is four separable pieces, and
each candidate loop now reports them as span fields in **microseconds**
(`CandidateTimers`, `domain/gap_fill_fit.rs`). The harness sums them into the
`--- candidate-loop component split (Level F) ---` table.

| Field | What it times |
|-------|---------------|
| `structure_us` | `score_pre_for_signature` + `score_post_for_signature` (timeline scan) |
| `seam_us` | the seam pair: O(1) FFT-band lookup on lever 1's path, naive Pearson otherwise |
| `repeat_us` | `fill_repeat_correlations` — the repeat window, which lever 1 did **not** band |
| `score_us` | the rest of `unified_fit_score_with_repeat` (`repeat_us` subtracted out) |

The four are disjoint; `Unacct` in the table is the span's exclusive time minus
all four (loop overhead, candidates rejected by the bounds guards, and the clock
reads). A large `Unacct` means the buckets are missing real work.

**Why fields and not sub-spans:** at ~330 µs/candidate over hundreds of
candidates per call, a per-candidate span enter/exit would be a measurable
fraction of what it measures. These are plain `Instant` deltas, ~2 clock reads
per component per candidate, gated on `CLIP_SYNC_SPAN_TIMING` so production pays
one bool load and no clock reads.

**Reading `seam_us` ≈ 0 on `*_end`:** expected, not a bug. That loop hoists both
seam values above it (lever 1), so no candidate pays for them.

### First corpus measurement (2026-07-25, complete — 17 of 17 pairs)

6-channel media, **8449 s instrumented**, 1758 bracket searches.

| Loop | Excl | % root | Cand. | µs/cand | Structure | Seam | Repeat | Score | Unacct |
|------|------|--------|-------|---------|-----------|------|--------|-------|--------|
| `unified_refine_start` | 2598 s | 30.75% | 8.44 M | 308 | 0.3% | 0.0% | **99.6%** | 0.0% | 0.1% |
| `unified_refine_end` | 2595 s | 30.71% | 8.44 M | 307 | 0.2% | 0.0% | **99.7%** | 0.0% | 0.0% |
| `unified_coarse_start` | 332 s | 3.93% | 707 k | 470 | 0.2% | 36.4% | 63.3% | 0.0% | 0.0% |
| `unified_fine_polish` | 218 s | 2.58% | 452 k | 482 | 0.2% | 36.1% | 63.7% | 0.0% | 0.0% |
| `unified_coarse_end` | 78 s | 0.92% | 355 k | 220 | 0.2% | 0.0% | **99.7%** | 0.0% | 0.1% |

Non-loop costs worth naming: `patch_decode_b` 1084 s (12.83%) + `patch_decode_a`
1047 s (12.39%) = **25.2% in decode**, and `local_anchor_xcorr` 358 s (4.23%,
99.3% of `anchor_matchability`).

**Repeat correlation totals ~5601 s = 66% of instrumented time.** The 9-pair
partial read (33.0/32.9%, 290 µs/cand) held up: the full set lands at
30.75/30.71% and 308 µs/cand, and every per-pair split agrees within a point.

What this settles:

- **The repeat correlation is ~66% of instrumented repair time** (5601 s of
  8449 s — the per-loop Repeat shares summed). Not the structure
  scan (0.2–0.3%), not the score arithmetic (0.0%). `Unacct` ≈ 0 everywhere, so
  the four buckets account for the loops completely — no further split needed.
- **Lever 1 worked, and that is exactly why repeat now dominates.** Where seam is
  still naive (`unified_coarse_start`, `unified_fine_polish`) it costs the same
  order as repeat; where it was banded, it is 0.0% and repeat is all that's left.
- **Repeat runs more channels than seam does.** On mono the two are 50/50
  exactly; on this 6-channel media the naive loops sit at Seam 38% / Repeat 62%,
  a ratio consistent with seam using the filtered `score_channels` subset (~4 of
  6) while `fill_repeat_correlations` iterates every channel. Lever 2's channel
  hoist never reached the repeat path.

**Rolling up a sweep still in flight** produces negative exclusive times on
`patch_audio` (and any span whose close is still pending) — children have closed,
the parent has not. The harness warns. It is an artifact of reading early, not a
measurement fault; it cleared when the run finished.

**After lever 1b(c), the profile should be:** `unified_refine_start` ~2598 s
(45%), decode ~2131 s (37%), everything else ~1048 s. Decode becomes the #2 cost
and the start-search repeat window becomes the only remaining lever-1b target.
Re-measure to confirm — this is arithmetic, not a measurement.

## 1b. Lever 1b(c) — end-search repeat hoist (implemented + validated 2026-07-25)

The §1a table says `unified_refine_end` is **30.71%** of root and 99.7% repeat
(the 9-pair partial read 32.9% / 99.8%, which is where that pair of figures came
from in earlier drafts).
**All of it was recomputing one identical `f64`.** In the end search:

- the placement start is `fill_bracket_placement(fill_start, end, ..).start`, so
  it is `fill_start` for every candidate;
- both seam values were already hoisted by lever 1;
- `repeat_penalty_at_placement` takes `gap_frames` / `pre_window` / `post_window`
  from the *context*, not the candidate.

So `end` never reaches the repeat window, and the penalty is loop-invariant. A
comment in `unified_search_best_fill_end` had claimed `end` "moves its window",
which is what kept it in the loop; that comment was wrong and is now corrected.

The fix hoists it above the loop and passes it via `RepeatPenaltySource::Fixed`,
substituting the same `f64` under the same guard — byte-identical by
construction, pinned by
`end_search_repeat_penalty_is_invariant_to_fill_end` (bit-equality across the
whole slack range). Expected saving on the full 17-pair run: **2672 s of 8449 s**
(`unified_refine_end` 2595 + `unified_coarse_end` 78), **31.6% of instrumented
repair time**, with no approximation.

### Media spot check — 4 pairs, pre/post, same recipe (2026-07-25)

Pairs 1, 14, 6, 9 re-run on the hoisted binary and compared line-by-line against
the 17-pair baseline logs. No `-RepairArgs` on either run; the `Gap scan:` /
`Gap fill:` recipe lines diff clean, so the comparison is like-for-like.

**Behavior — byte-equivalent on all four pairs.**

| Pair | Decision lines | Candidate-count entries | End spans w/ nonzero `repeat_us` |
|------|----------------|-------------------------|----------------------------------|
| 1    | 40/40, 0 diffs | 960, 0 diffs            | 0 of 384 |
| 14   | 51/51, 0 diffs¹| 125, 0 diffs            | 0 of 50  |
| 6    | 49/49, 0 diffs | 800, 0 diffs            | 0 of 320 |
| 9    | 34/34, 0 diffs | 750, 0 diffs            | 0 of 300 |

¹ after normalizing the timestamp prefix and the output filename; the `WARN`
text itself (`marginal waveform seam`, same gap, same `pre`/`post`/`min`) is
identical. **2635 candidate-count entries compared, zero divergence** — the
per-candidate search structure is unchanged, which is what byte-identical
substitution predicts.

**Cost — root `patch_audio` instrumented time.**

| Pair | Baseline | Hoisted | Delta |
|------|---------:|--------:|------:|
| 1    | 529.0 s  | 423.0 s | −106.0 s (−20.0%) |
| 14   | 180.0 s  | 153.0 s |  −27.0 s (−15.0%) |
| 6    | 404.0 s  | 284.0 s | −120.0 s (−29.7%) |
| 9    | 719.0 s  | 464.0 s | −255.0 s (−35.5%) |
| **Total** | **1832.0 s** | **1324.0 s** | **−508.0 s (−27.7%)** |

The end-loop busy actually deleted was 576.6 s (146.6 + 42.9 + 123.9 + 263.2),
matched to within 0.3 s per pair. The 69 s shortfall against that is run-to-run
noise, independently pinned on **untouched** spans in the same logs:
`patch_decode_a` +10.8 s / `patch_decode_b` +11.3 s on pair 1 (~+16% on pure
ffmpeg decode) and `unified_refine_start` +1.9 s on pair 14 (~+5% on untouched
search). So the 31.6% projection reads as confirmed at 27.7% observed, with the
difference attributable to machine load rather than to the change. **The span
partition, not wall clock, is the verdict here** — wall clock is a cross-run
comparison on a machine whose load we do not control.

**The post-hoist profile (4-pair aggregate) confirms the predicted shape:**

- `unified_refine_start` **43.14%** of root, **99.4% Repeat** — was ~31% before,
  now unambiguously the top cost. Lever 1b(b) (band the start-search repeat
  window) is the next target.
- `patch_decode_b` 17.59% + `patch_decode_a` 17.47% — decode is now #2, as the
  projection said.
- `local_anchor_xcorr` 9.94% — distant third.
- `unified_refine_end` **0.85 s total, 0.07% of root**, and its remaining time is
  **66% structure** — the loop now pays only for the structural evidence it was
  always actually deciding on. `unified_coarse_end` has fallen below the
  roll-up's display threshold entirely (~54 µs/bracket).

Pair 1 exits 4 on both sides (pre-existing unfillable gap, identical decisions);
the nonzero exit is not introduced by this change.

**There is still no post-hoist 17-pair sweep.** The 4-pair aggregate above is the
best current profile; §1/§1a are both pre-hoist. Land a full sweep before quoting
a post-hoist corpus percentage.

**If the post seam ever becomes end-dependent** (Phase C of
[archive/TEMP-fill-placement-axis-plan.md](archive/TEMP-fill-placement-axis-plan.md)), this hoist
needs revisiting — but only its cheap half. `repeat_penalty_at_placement` takes
the seams as *arguments* and branches on them (`wave_min`,
`asymmetric_post_dup`), so an end-varying `post_seam` makes the penalty
end-varying. The two Pearson correlations do **not** move: they read `start` and
the context windows only. So hoist `(repeat_pre, repeat_post)` and recompute just
the branch arithmetic per candidate — `RepeatPenaltySource::Fixed(f64)` becomes
`Fixed { repeat_pre, repeat_post }`, and ~99.8% of the saving survives.
`end_search_repeat_penalty_is_invariant_to_fill_end` fails if this is missed,
which is the intended tripwire.

**This was never a regression — the end search's waveform terms have been
loop-constant since the loop was written.** `unified_search_best_fill_end` was
introduced in `e849e64` (2026-06-20) already calling
`waveform_min_at_start(waveform, fill_start)` inside `consider`, with
`gap_frames: ctx.gap_frames`. `92b8920` (same day) swapped in
`unified_fit_score_with_repeat`, still keyed on `fill_start`. Both were inert in
that loop from their first commit; lever 1 and lever 1b(c) only stopped paying
for them. There is no earlier end-dependent behavior to restore.

**Where the waveform *does* vote on fill length:** at splice time, not search
time. `fit_fill_length_for_gap` → `pick_fill_length_anchor` scores trim-head vs
trim-tail with `fill_splice_seam_correlations_interleaved` on the actual trimmed
fill, and `score_extend_short_fill_to_gap_frames` extends only while the seam
holds and `repeat_post` does not rise. The origin plan
(`archive/fill-fitting-plan.md`, Phase D) specified `repeat_post =
corr(A_post_border, B_fill_tail)` — the *fill tail*, end-dependent by
definition. Pinning the tail at `start + gap_frames` is exact when
`fill_len == gap_frames` and drifts with `|end − nominal_end|`, which the 5.0 s
`default_fill_length_slack_secs` makes reachable. Whether the search's end sweep
should carry its own seam evidence was the open **behavior** question in
[archive/TEMP-fill-placement-axis-plan.md](archive/TEMP-fill-placement-axis-plan.md);
Phase A armed the placement observable, Phase B's corpus roll-up (slack vs
**bracket span**, not original gap) made Phase C a **NO-GO** — the end sweep
rarely leaves nominal (max excursion 388 ms). Slack over-provisioning is the
perf residue (§5 #3).

## 2. Per-pair characterize baseline, 17 pairs (2026-07-23)

Fingerprint mode, so brackets are enumerated exhaustively — treat `search s` as
an **upper bound** on the production path (§3 measures 2.7–4.1 s/bracket in
production). `search s` sums the per-bracket `search_us` field. Two walls, both
from the log timestamps: **`char s`** is first → last `bracket_stats` line
(characterize only, the like-for-like denominator for `search s`); **`run s`** is
first log line → last `bracket_stats` and additionally carries the Gap scan and
setup that precede the first bracket.

| pair | brackets | search s | char s | run s | s/bracket |
|------|----------|----------|--------|-------|-----------|
| 1  | 287 | 841.8  | 1104 | 1104 | 2.93 |
| 2  | 219 | 1027.5 | 1164 | 1164 | 4.69 |
| 3  | 219 | 954.4  | 1057 | 1436 | 4.36 |
| 4  | 169 | 810.0  | 838  | 838  | 4.79 |
| 5  | 198 | 949.8  | 968  | 1280 | 4.80 |
| 6  | 307 | 778.9  | 940  | 1178 | 2.54 |
| 7  | 148 | 710.6  | 774  | 1087 | 4.80 |
| 8  | 174 | 668.1  | 745  | 1056 | 3.84 |
| 9  | 209 | 1019.6 | 1134 | 1425 | 4.88 |
| 10 | 258 | 1217.0 | 1336 | 1661 | 4.72 |
| 11 | 94  | 474.6  | 486  | 486  | 5.05 |
| 12 | 164 | 818.1  | 904  | 1178 | 4.99 |
| 13 | 701 | 2896.7 | 3352 | 3715 | 4.13 |
| 14 | 542 | 2613.1 | 2977 | 3162 | 4.82 |
| 15 | 80  | 387.5  | 424  | 737  | 4.84 |
| 16 | 538 | 2316.2 | 2645 | 2950 | 4.31 |
| 17 | 757 | 3424.9 | 4170 | 4455 | 4.52 |
| **total** | **5064** | **21 908.8** | **25 018** | **28 942** | **4.33 avg** |

**Anchor search is 88% of characterize wall-clock** (21 908.8 / 25 018),
consistent with the 2026-07-20 finding that `char_gate_search` is 93% of a
production repair and `gate_anchor_search` 88–96% of that. Against `run s` it is
76%, but that denominator includes scan and setup, which the anchor search was
never part of — earlier drafts quoted 74% off that mismatched pair.

**Per-bracket cost is flat** — 4.3–5.0 s on 15 of 17 pairs; only pairs 1 (2.93)
and 6 (2.54) sit outside. Two consequences:

- Bracket **count** (94 → 757, an 8× spread) is what separates an 8-minute
  characterize from a 70-minute one, not per-bracket difficulty.
- Any future per-bracket speedup scales linearly across the corpus, with no
  pair-specific structure to special-case. That is a property of the workload,
  not an endorsement of a candidate.

## 3. No-regression record

| pair | `gate_anchor_search` | brackets | s/bracket | vs §2 |
|------|----------------------|----------|-----------|-------|
| 1  | 520.0 s | 192 | 2.71 | 2.93 → **−7%** |
| 10 | 645.3 s | 156 | 4.14 | 4.72 → **−12%** |

Full repair path, after the `bracket_fill` elimination landed. Per-bracket cost
is flat-to-better despite the added execute pass. Pair 1's full repair (728 s) is
*faster* than its 2026-07-23 characterize-only run (1104 s), because production
short-circuits the enumeration fingerprint mode exhausts.

Gap planning is unchanged across that refactor: pair 1 yields 17 gaps found, 7
`→ drop` equivalence tags, 10 planned regions on **both** dates — a real-media
parity signal independent of the fixture suite.

Two recurring worries, both measured and unfounded:

- **"Pair 1 used to take 5–6 minutes."** Not on any recorded run. Characterize
  alone was 18.4 min on 2026-07-23; the full repair is 12.1 min now. The ~4-min
  **scan** phase is the only figure in that range.
- **"`char_gate_search` dropped from 93.3% to 73.8%, something regressed."** No —
  lever 1/2 cut the numerator (`char_gate_search` 1746 s → 330 s, 5.3×), so decode
  and the rest occupy a larger share of a much smaller total.

## 4. Settled — do not re-propose without new measurement

| Candidate | Verdict | Evidence |
|-----------|---------|----------|
| Shared mono downmix hoist | **REFUTED** 2026-07-20 | 0.1 s of 1872 s = **0.006%** |
| Cut bracket count *k* (anchor pre-gate) | **NO-GO** 2026-07-23 | realizable pre-gate fraction over this 17-pair run = **0%** (0/~4939 brackets), vs a 46% theoretical ceiling |
| Fill-assembly double-derivation (M0) | **immaterial** 2026-07-24 | **0.053%** of wall-clock, worst pair 0.35% |
| "FFT the haystack sweep" | **not a thing** | there is no full haystack sweep; the unified search is already windowed and coarse-stepped |

Already landed and **on by default**: the hoisted placement-invariant channel
selection (lever 2) and the FFT seam band on the dense refine (lever 1,
`RepairConfig.fft_seam_search`; `--no-fft-seam-search` opts out).

**The cheap wins are spent.** Treat §1a + §1b as the reference a new candidate
must argue against, not as motivation for one. (§1 is the pre-hoist, pre-Level-F
tree; do not size a candidate against it.)

## 5. Open candidates

0. **Lever 1b(b) — band the *start*-search repeat window.** Promoted to #1 by the
   lever 1b(c) spot check: with the end loops gone, `unified_refine_start` is
   **43.1% of root and 99.4% of it is `fill_repeat_correlations`**. Unlike the end
   search this is *not* loop-invariant — the repeat window moves with the
   candidate `start` — so it needs the lever-1 FFT banding treatment
   (`fill_seam_correlations_band`), not a hoist. Largest remaining single target
   by a wide margin.
1. **`local_anchor_xcorr` — 358 s (4.23%) on the full corpus, 9.94% post-hoist.**
   This is what the old "`gate_anchor_search` holds 910.8 s (9.9%) of exclusive
   time" entry was actually pointing at. That 910.8 s was an **instrumentation
   gap** in the 2026-07-24 binary, which had no span under the gate; once
   `anchor_matchability` / `local_anchor_xcorr` were added, `gate_anchor_search`'s
   own exclusive time collapsed into the roll-up's "everything else" bucket
   (≤140 s total across 17 pairs on 2026-07-25-2) and the cost resolved to the
   local cross-correlation, 99.3% of `anchor_matchability`. It rises to a
   **distant third at 9.94%** once the end loops are hoisted away (§1b). Now a
   located cost, not a mystery — but still unattacked.
2. **Decode is 25.2% combined (2131 s), the #2 cost.** It has not got slower —
   the gate got ~5× faster, so decode's share grew from the 6.5% of the
   2026-07-20 baseline, and post-hoist it is **~35%** of root (§1b). It is 34
   calls of ~63 s each, not a long tail, and it has **never been investigated**.
   Most plausible next candidate.
3. **Narrow end-search slack (`fill_length_slack_secs` 5.0 → ~1.0 s).** The
   decoupling from B extract is already done (below); only the narrowing is open.
   Fingerprint corpus roll-up (17/17 pairs, 2026-07-26): end-search excursion is
   `|fill − bracket span|` — median **24 ms**, p95 **91 ms** (all 1044 placed
   brackets) / **157 ms** (best-by-min-seam cohort, n=121), **max 388 ms**;
   nothing ≥1 s. The ±5 s window is **~13× the observed max** and ~55× its p95.
   (`archive/TEMP-fill-placement-axis-plan.md` Phase B quotes 77 / 105 ms for
   these two p95s; recomputed from the same 1044 brackets they are 91 / 157 —
   77 ms is the p93. Nothing downstream of it turns on the difference.)

   **Two jobs, now two knobs.** `fill_length_slack_secs` used to size both (1) the
   end sweep (`gap ± slack` in `gap_fill_fit`) and (2) the B haystack tail; the
   split has since landed, so (2) is `fill_extract_tail_slack_secs.max(margin)` →
   `b_extract_end_secs` (`patch_audio/region.rs:1496`) and fingerprint `pad_tail`
   (`gap_fingerprint/measure.rs:1951`, `:2101`). The two carry different risk:
   (1) only drops far end candidates; (2) shortens `total_frames` and can
   invalidate late *start* candidates / move gate outcomes. Full-track decode is
   unchanged either way (extract is a slice) — which is why they had to be
   decoupled: one dial bought (2)'s blast radius without the decode win once
   hoped for.

   **Coarse grid is not the issue.** `search_coarse_step(bin, span) =
   (span / bin / 2_000).max(1) * bin` (`gap_structure.rs:284`) with `span =
   2×slack`: at 50 ms bins @ 48 kHz, slack 5.0 s and 1.0 s both floor to
   `coarse_step == bin_frames`. The narrowed end range is a strict **subset** of
   the wide one.

   **Proposed (see [BACKLOG.md](../../BACKLOG.md)):**
   - **Config split — done:** `fill_extract_tail_slack_secs` (default **5.0**) wires
     `b_extract_end` / `pad_tail`; `fill_length_slack_secs` is end-sweep only.
   - **Phase 1 — done:** `fill_length_slack_secs` default **1.0 s**. Behavior
     identical (goldens + 17-pair fingerprint A/B). Perf vs `perf-gate-hoist`
     (pairs 1/6/9/14): coarse-end **202 → 42**; end-loop repeat already 0; end-search
     busy **−40 ms / 4 pairs** — not a meaningful win post-1b(c). Keep 1.0; skip 0.5 s.
   - **Phase 2:** extract-tail shrink deferred / low priority (no decode win).

   **Exit checks (Phase 1):** (1) curated golden A/B — Tier-1 `fill_*` plus
   patch/skip outcome; (2) spot-listen a few largest-excursion patches. A
   re-rolled `|fill − span|` on a shrunk `pad_tail` is **not** a Phase 1
   before/after (that mixes in Phase 2). Measurement provenance:
   [archive/TEMP-fill-placement-axis-plan.md](archive/TEMP-fill-placement-axis-plan.md)
   Phase B.

## 6. Not covered

- **No pre-2026-07-23 data exists**, so nothing here can detect a regression
  introduced *before* that date. The `unified_*` spans were added by `d172d48`
  and have no earlier counterpart at all.
- `search_us` (§2) and `gate_anchor_search` (§1, §3) are not the same
  instrument. They both bound the anchor bracket search and are compared
  per-bracket only; do not read the small deltas in §3 as precise speedups.
- Splice and mux are not broken out beyond "everything else".

---

## Provenance

Numbers in §1–§3 were first recorded in
`docs/dev/TEMP-anchor-search-perf-baseline.md` (`2d8e4f0`, reframed `ba27c00`),
which this doc replaces — it was deleted rather than archived to avoid two copies
of one baseline. Earlier campaign reasoning, kept for history only:

- [archive/TEMP-production-repair-perf-plan.md](archive/TEMP-production-repair-perf-plan.md)
  — the 2026-07-20 baseline and the measurement that killed the downmix hoist.
- [archive/TEMP-patch-audio-bracket-fill-elimination-plan.md](archive/TEMP-patch-audio-bracket-fill-elimination-plan.md)
  §3.1 — the per-pair M0 fill-assembly table.
- [archive/TEMP-pipeline-perf-redesign-plan.md](archive/TEMP-pipeline-perf-redesign-plan.md)
  — the characterize/execute split this all sits on.
