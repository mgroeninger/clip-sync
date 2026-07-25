# Anchor bracket search — current-state perf baseline

**Status:** reference data, not a plan. Captured 2026-07-23/24.

**This is a POST-optimization baseline, not a pre-FFT one.** By these runs the
per-bracket levers had already landed and are on by default: the hoisted
placement-invariant channel selection (lever 2) and the FFT seam band on the
dense refine (lever 1, `RepairConfig.fft_seam_search`, `patch_region.rs:1515`;
`--no-fft-seam-search` opts out). `char_gate_search` had already gone 1746 s →
330 s (5.3×) off the 2026-07-20 baseline. Do not read the numbers below as
something an FFT is still going to fix — and note the standing correction that
there is **no full haystack sweep** to FFT: the unified search is already
windowed and coarse-stepped.

The remaining structural lever was **cutting bracket count k**, and that was
measured and **dropped NO-GO on 2026-07-23** (the realizable pre-gate fraction
over this same 17-pair run was 0%). So there is no active optimization this
baseline is feeding. Its value is (1) the no-regression proof in §2, and (2) a
reference point for whatever the next candidate turns out to be.

**Read §2.1 before picking a next candidate** — the exclusive-cost breakdown says
decode is now 25.6% of runtime and that a 9.9% slice inside `gate_anchor_search`
is not instrumented at all. Neither was visible when this doc was first written.

## Media handling

All figures below come from runs over **licensed media**, referenced only by
gap-fingerprint corpus **pair index**. The pair-index → media mapping is
deliberately not recorded in this repository; it exists only in the gitignored
source map (`*.sources.local.toml`) alongside the gitignored `gap-files/`. Raw
logs contain absolute media paths and therefore **must not** be committed — the
copies backing this table live in `gap-files/perf-baseline-2026-07-23/`, which
`.gitignore` covers. This follows the convention already used by the archived
production-repair perf plan.

## 1. Characterize-only baseline, 17 pairs (2026-07-23)

Before the `bracket_fill` elimination phases, after the lever-1/2 gate-search
optimizations. This is the same run whose bracket categories produced the
anchor pre-gate NO-GO. Fingerprint mode, so
brackets are enumerated **exhaustively** rather than short-circuited at the first
winner — treat `search s` as an **upper bound** on the production path. `search s`
is the sum of the per-bracket `search_us` field; `wall s` spans the first to last
`bracket_stats` line, i.e. characterize only.

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

**Per-bracket cost is flat** — 4.3–5.0 s on 15 of 17 pairs, with pairs 1 (2.93)
and 6 (2.54) the only outliers. Two consequences:

- Bracket **count** (94 → 757, an 8× spread) is what separates an 8-minute pair
  from a 74-minute one, not per-bracket difficulty. That is why k was the lever
  worth attacking — and why its NO-GO leaves no obvious successor.
- Any *future* per-bracket speedup would scale linearly across the corpus, with
  no pair-specific structure to special-case. That is a property of the workload,
  not an endorsement of a candidate; the cheap wins here are spent.

Note these are fingerprint-mode figures and so sit above the production
per-bracket cost (§2 measures 2.7–4.1 s on the production path).

## 2. Post-refactor spot check (2026-07-24)

Same pairs, full repair path (`--wav`) rather than characterize-only, with the
`bracket_fill` elimination landed. Used to rule out a refactor-induced
regression.

| pair | `gate_anchor_search` | brackets | s/bracket | vs §1 |
|------|----------------------|----------|-----------|-------|
| 1  | 520.0 s | 192 | 2.71 | 2.93 → **−7%** |
| 10 | 645.3 s | 156 | 4.14 | 4.72 → **−12%** |

**No regression.** Per-bracket cost is flat-to-slightly-better despite the added
execute pass. Pair 1's full repair (728 s) is *faster* than its 2026-07-23
characterize-only run (1104 s), because production short-circuits the bracket
enumeration that fingerprint mode exhausts (192 vs 287 brackets).

Gap planning is unchanged across the refactor: pair 1 yields 17 gaps found with 7
`→ drop` equivalence tags → 10 planned regions on **both** dates. That is a
real-media parity signal for the refactor, independent of the fixture suite.

## 2.1 Exclusive-cost breakdown, all 17 pairs, production path (2026-07-24)

From `scripts/measure-repair-perf.ps1`, which keys spans by their **full parent
chain** and reports **exclusive** time (own busy minus direct children's). Unlike
inclusive `TotalSecs`, exclusive time is a partition — it sums to the root.
Root = `patch_audio`, **9215 s** over 17 pairs.

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

Two things here were invisible in the older flat by-name reporting, and both
change the picture:

1. **`gate_anchor_search` holds 910.8 s (9.9%) of exclusive time** — a tenth of
   all runtime is inside the anchor search but inside *no* child span. That is an
   **instrumentation gap, not a located cost**: something between
   `try_anchor_seam_joint_search`'s entry and the per-bracket
   `bracket_unified_search` calls (anchor enumeration, feasibility filtering,
   per-bracket setup) is unmeasured. Anyone resuming perf work should span that
   region before theorizing about it. The harness now flags any parent holding
   ≥10% exclusive for exactly this reason.
2. **Decode is now 25.6% combined (2354.7 s), the #2 cost after `unified_refine`.**
   The 2026-07-20 baseline put decode at 6.5%; it has not got slower — the gate
   got ~5× faster, so decode's *share* grew. It is now a larger slice than
   anything except the refine loop, and it is 34 calls of ~69 s each rather than a
   long tail. This is the most plausible next perf candidate and, unlike the k-cut,
   it has never been investigated.

`unified_refine` at 56.4% remains the single dominant cost. Its per-call figure
(1.47 ms over 3516 calls) is the thing lever-1's FFT already attacked; the
`ExclSecs` column is what a future attempt should be measured against.

## 3. What this closes

- **"Did the refactor slow production down?"** — No. §2. The fill assembly itself
  is 0.053% of wall-clock (measured; see the `bracket_fill` elimination plan
  §3.1).
- **"Was pair 1 ever 5–6 minutes?"** — Not on any recorded run. Characterize
  alone was 18.4 min on 2026-07-23 and the full repair is 12.1 min now. The
  ~4-minute *scan* phase is the only figure in that range; the recollection most
  likely refers to a scan.

## 4. What this does not cover

- No pre-2026-07-23 data exists, so this cannot detect a regression introduced
  *before* it. The `unified_*` spans were added by `d172d48` and have no earlier
  counterpart at all.
- `search_us` (§1) and `gate_anchor_search` (§2) are not the same instrument.
  They both bound the anchor bracket search and are compared per-bracket only;
  do not read the small deltas in §2 as precise speedups.
- Execute, splice, and decode are covered by the archived production-repair perf
  plan, not here.
