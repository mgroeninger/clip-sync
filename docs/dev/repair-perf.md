# Repair performance — measurement reference

**Durable reference, not a plan.** Where repair time goes, how to measure it, and
which candidates are already settled. Last full measurement **2026-07-24**
(17 corpus pairs, production `--wav` path).

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
backing the tables below live in `gap-files/perf-baseline-2026-07-23/`, which
`.gitignore` covers. When recording a result, carry over the derived numbers and
the pair index — nothing else. Grep before committing.

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
  shape (287 vs 192 brackets on pair 1).
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

**Not yet measured on the corpus.** A synthetic mono-chirp pair puts `repeat_us`
at **99%** of both refine loops, which matches the standing hypothesis — lever 1
banded the seam correlations and left the repeat window naive — but a 120 s
synthetic chirp is not the corpus, and this number must not be quoted as one.
Record the real split here after the next 17-pair sweep.

**Reading `seam_us` ≈ 0 on `*_end`:** expected, not a bug. That loop hoists both
seam values above it (lever 1), so no candidate pays for them.

## 2. Per-pair characterize baseline, 17 pairs (2026-07-23)

Fingerprint mode, so brackets are enumerated exhaustively — treat `search s` as
an **upper bound** on the production path (§3 measures 2.7–4.1 s/bracket in
production). `search s` sums the per-bracket `search_us` field; `wall s` spans
first to last `bracket_stats` line, i.e. characterize only.

| pair | brackets | search s | wall s | s/bracket |
|------|----------|----------|--------|-----------|
| 1  | 287 | 841.8  | 1104 | 2.93 |
| 2  | 219 | 1027.5 | 1164 | 4.69 |
| 3  | 219 | 954.4  | 1436 | 4.36 |
| 4  | 169 | 810.0  | 838  | 4.79 |
| 5  | 198 | 949.8  | 1282 | 4.80 |
| 6  | 307 | 778.9  | 1178 | 2.54 |
| 7  | 148 | 710.6  | 1087 | 4.80 |
| 8  | 174 | 668.1  | 1059 | 3.84 |
| 9  | 209 | 1019.6 | 1425 | 4.88 |
| 10 | 258 | 1217.0 | 1666 | 4.72 |
| 11 | 94  | 474.6  | 489  | 5.05 |
| 12 | 164 | 818.1  | 1178 | 4.99 |
| 13 | 701 | 2896.7 | 3715 | 4.13 |
| 14 | 542 | 2613.1 | 3162 | 4.82 |
| 15 | 80  | 387.5  | 745  | 4.84 |
| 16 | 538 | 2316.2 | 2950 | 4.31 |
| 17 | 757 | 3424.9 | 4464 | 4.52 |
| **total** | **5061** | **21 409** | **28 922** | **4.23 avg** |

**Anchor search is 74% of characterize wall-clock**, consistent with the
2026-07-20 finding that `char_gate_search` is 93% of a production repair and
`gate_anchor_search` 88–96% of that.

**Per-bracket cost is flat** — 4.3–5.0 s on 15 of 17 pairs; only pairs 1 (2.93)
and 6 (2.54) sit outside. Two consequences:

- Bracket **count** (94 → 757, an 8× spread) is what separates an 8-minute pair
  from a 74-minute one, not per-bracket difficulty.
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

**The cheap wins are spent.** Treat §1 as the reference a new candidate must
argue against, not as motivation for one.

## 5. Open candidates

1. **`gate_anchor_search` holds 910.8 s (9.9%) of exclusive time.** A tenth of
   all runtime is inside the anchor search but inside *no* child span — anchor
   enumeration, feasibility filtering, or per-bracket setup, between
   `try_anchor_seam_joint_search`'s entry and the per-bracket
   `bracket_unified_search` calls. This is an **instrumentation gap, not a
   located cost**: span it before theorizing about it.
2. **Decode is 25.6% combined (2354.7 s), the #2 cost.** It has not got slower —
   the gate got ~5× faster, so decode's share grew from the 6.5% of the
   2026-07-20 baseline. It is 34 calls of ~69 s each, not a long tail, and it has
   **never been investigated**. Most plausible next candidate.

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
