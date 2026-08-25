# TEMP — Capture the A/B program level offset (paired block difference)

**Status:** implemented, 2026-08-25 (rewritten from the floor/mid/roof two-summary draft —
see § 10). **Capture only** — stamp a whole-program A↔B level relationship on the scan
report. No gain policy, no change to `compute_fill_gain` / `normalize_fill`.

**Tracked as:** [BACKLOG.md](../../BACKLOG.md) § *Fill normalize boosts against a
non-local reference* → *Capture A/B program levels*.

**Companions:** [settled.md](settled.md) § A **B-relative / donor-shoulder fill gain**
(refuted — this plan is **not** that idea, see § 1); local-reference defect sizing
[TEMP-fill-normalize-local-reference-2026-08-25.md](TEMP-fill-normalize-local-reference-2026-08-25.md).

**Media hygiene:** pair index only; no filenames / titles / paths in the repo.

---

## 1. Goal / non-goals

**Goal**

Answer one question, once per scan, from data the scan already holds:

> Is B's program systematically louder or quieter than A's — and is a **single scalar**
> enough to describe it?

Measure it as a **paired difference on the same material**: for each A analysis block
that has content, difference it against the B block it maps to, and summarize the
resulting distribution.

- Persist on `GapReport`, emit on scan JSON (`GapScanJson`) so `--format json` and
  patch runs that already hold a report read it without a second decode.
- Report the **spread** alongside the offset — the spread, not the sample size, is what
  says whether one scalar exists at all.
- Unit-test the pure reduce. No corpus media required to ship.

**Non-goals (this plan)**

- Applying `gain₀ = 10^(offset/20)` (or any scalar) to fills. See § 8.
- Changing per-gap `normalize_fill` / `compute_a_border_rms` / `global_a_rms`.
- Matching fills to B's shoulders at the hole (refuted — [settled.md](settled.md) § A).
  This plan never reads B's gap shoulders; it is a **whole-program master
  relationship**, measured over material that is undamaged on both sides.
- Keeping full `BlockLevel` arrays on the report.
- ITU / LUFS loudness.
- Recomputing on patch PCM.

---

## 2. Why paired, not two per-side summaries

The obvious design — summarize A, summarize B, subtract — compares two **independent
distributions**, and is only a level comparison under assumptions this corpus violates:

| Assumption | Why it fails here |
|---|---|
| A and B have similar silence duty cycles | **A is the damaged file.** Every dropout adds mass at the bottom of A's histogram, pushing any all-block percentile down. The subtraction then reports A's damage as a level offset |
| A and B have similar content mix | Any rank statistic is a rank over each file's own speech/music/quiet proportions. B is often a different edit — that is *why* it is the donor |
| Leading/trailing pads can be trimmed away | Requires a head/tail rule, which requires a silence predicate, which (see § 4) is not a level predicate |

Pairing removes all three. Each sample is the **same audio measured twice**, so content
mix cancels, dropouts and pads drop out of the pairing instead of biasing it, and
regions where B holds different material become outliers a median discards.

Cost is nil: A's timeline, B's timeline and the global offset are all live at the same
point in `ScanGaps` (`scan_gaps.rs`, right after `finish_with_levels`), and both
scanners are built from the same `request.recipe` — identical block size, identical
silence knobs, identical dB convention on both sides.

---

## 3. The measurement

Inputs: `a_levels: &[BlockLevel]`, `b_levels: &[BlockLevel]`, `offset_secs: f64`
(A→B map), `b_scanned_end_secs`, `scan_block_secs`, `absolute_silence_rms`.

For each A block `i`:

1. **Active on A.** Keep only `rms_db_A(i) > active_db`, where
   `active_db = absolute_silence_floor_db(absolute_silence_rms)` (≈ −60 dBFS with the
   default recipe; `policies/silence.rs`). This is an **absolute level threshold**, not
   the scanner's `silent` flag — see § 4.
2. **Steady on A.** Require both A neighbours within `STEADY_TOLERANCE_DB` (start at
   3.0 dB). `offset_secs` is not a multiple of `scan_block_ms`, so a paired block is
   misregistered by up to half a block; on transients that turns registration error
   into level error. Rejecting transient-adjacent blocks removes most of it and
   sharpens the spread. Also absorbs slow alignment drift.
3. **Map.** `b_lo = a.start_secs + offset_secs`, `b_hi = a.end_secs + offset_secs`.
   Take the B block with the **largest time overlap** of `[b_lo, b_hi]` (reuse the
   `window_indices` lookup in `domain/gap_equivalence.rs`, or a small shared binary
   search — do **not** derive an index by division; B's timeline can carry
   discontinuities). Max-overlap rather than interpolating the two straddled blocks:
   step 2 has already guaranteed the A neighbourhood is flat to
   `STEADY_TOLERANCE_DB`, so the two candidates differ by little and the simpler rule
   costs nothing.
4. **Inside the reviewed prefix.** Require `b_hi <= b_scanned_end_secs` — the same
   fail-closed rule the donor-occupancy read already uses.
5. **Active on B.** Same `active_db` threshold on the mapped block.
6. Record `d_i = rms_db_B(mapped) − rms_db_A(i)`.

Reject the pair (silently — it is not an error) if any step fails.

**Estimator over the `d` set:**

| Field | Value | Role |
|---|---|---|
| `offset_db` | **median** of `d` | The A↔B program level relationship. Sign convention: **B minus A** — positive means B is louder |
| `spread_db` | **MAD** of `d` (median absolute deviation about `offset_db`) | *Does one scalar exist?* Tight ⇒ a real master offset. Wide or multimodal ⇒ dynamics/compression differ, or the alignment is wrong |
| `p10_db` / `p90_db` | nearest-rank deciles of `d` | Shape check a single MAD hides (asymmetry, bimodality) |
| `n_pairs` | accepted pairs | Trust floor |
| `n_active` / `n_steady` | A blocks that passed step 1, and step 1 + 2 | Two denominators: `n_pairs / n_steady` is B-coverage yield; `n_steady / n_active` is what the steadiness filter cost (the only emitted evidence about the tolerance) |
| `first_pair_secs` / `last_pair_secs` | A-timeline span of accepted pairs | Provenance — which part of the program the number is about |
| `chunks` | per-chunk `{ start_secs, end_secs, offset_db, n_pairs }` over fixed ~10-minute A-timeline buckets (emitted only when a chunk clears `MIN_CHUNK_PAIRS`) | **Is the offset constant?** Catches a differently-normalized reel, per-segment loudness normalization, decoder DRC that varies. Two whole-file numbers average exactly this away |

Nearest-rank for every percentile; document the convention and test `n = 1` / `n = 2`.
Ambiguity in a persisted JSON field is worse than either choice.

Fewer than `MIN_PAIRS` accepted (start at 50) → the whole summary is `None` (abstain).
Never invent an offset from a handful of blocks.

---

## 4. Why an absolute threshold and not the scanner's `silent` flag

`is_silent_interleaved` (`policies/silence.rs`) fires when **either** the peak is below
the absolute floor **or** `RMS < peak × silence_peak_fraction` (default `0.01`). The
second branch is unbounded in level: a block with peak 0.5 and RMS 0.004 reads *silent*
at −48 dBFS. That branch exists to catch codec noise around isolated transients — clock
ticks, drum beats, speech pauses — which is exactly this corpus's awkward material.

Using `silent` as a level gate would therefore admit −48 dB transient blocks into a
"quiet" population and, worse, let a transient-only passage read as a pad. The absolute
threshold sorts such a block into *active*, where it belongs, and step 2 then rejects it
for being transient-adjacent.

`rms_db` note: `block_rms_db` returns `BLOCK_LEVEL_FLOOR_DB` (−120) only when the block
RMS is `<= 1e-9`; a tiny non-zero RMS returns its true dB, which can be *below* −120
(e.g. −160). Compare with `>` / `<=` against thresholds — never `== BLOCK_LEVEL_FLOOR_DB`.
(The doc comment on `BlockLevel` claiming a hard floor is inaccurate; fix it in passing.)

---

## 5. What the number does and does not include

| Source of a non-zero `offset_db` | Counted? | Note |
|---|---|---|
| Different master / mix level | **Yes** | The target |
| Decoder gain — AC-3 dialnorm, DRC, decode-path scaling | **Yes, deliberately** | A fill inherits it. Do not "correct" it away |
| Different channel geometry | **Yes, and it is a confound** | `block_rms_db` is RMS over *interleaved* samples, so a stereo/mono pair carries a correlation-dependent constant indistinguishable from master gain. **Gate:** when `track_compatibility.channels_match` is false, still record the summary but set `channels_comparable: false` so no consumer reads it as a gain |
| A's dropouts | No | Excluded by step 1 (A inactive) |
| Head/tail pads | No | Excluded by step 1, or unpaired |
| Edits where B holds other material | Mostly no | Outlier `d_i`, discarded by the median; visible as elevated `spread_db` |
| Dynamics / compression difference | Not as an offset | Shows as wide `spread_db` — which is the point of reporting it |

---

## 6. Types and plumbing

Sketch (names flexible; keep small):

```rust
/// Whole-program A↔B level relationship, measured as a paired per-block difference.
/// Sign convention: B minus A (positive ⇒ B is louder).
pub struct AbLevelOffset {
    pub offset_db: f64,
    pub spread_db: f64,
    pub p10_db: f64,
    pub p90_db: f64,
    pub n_pairs: usize,
    pub n_active: usize,
    pub n_steady: usize,
    pub first_pair_secs: f64,
    pub last_pair_secs: f64,
    /// False when A/B channel counts differ — the offset is not a pure gain. See § 5.
    pub channels_comparable: bool,
    /// Per-chunk medians; empty when the program is too short to bucket.
    pub chunks: Vec<AbLevelOffsetChunk>,
}

pub struct AbLevelOffsetChunk {
    pub start_secs: f64,
    pub end_secs: f64,
    pub offset_db: f64,
    pub n_pairs: usize,
}

/// Per-side context, kept because it is the only thing available when B cannot be
/// mapped, and because A's own floor is independently useful. **Not** a comparison
/// instrument — never subtract these two (§ 2).
pub struct ProgramLevelContext {
    /// Median dB of active blocks (`rms_db > active_db`).
    pub mid_db: Option<f64>,
    /// Median dB of blocks between digital zero and `active_db`.
    pub floor_db: Option<f64>,
    pub n_active: usize,
}

/// The single `GapReport` field. One container rather than two parallel `Option` fields:
/// halves the struct-literal churn below and keeps the offset next to the context a
/// reader needs to interpret it.
pub struct ProgramLevels {
    pub a: ProgramLevelContext,
    pub b: Option<ProgramLevelContext>,
    pub offset: Option<AbLevelOffset>,
}
```

| Seam | Change |
|---|---|
| Pure reduce | `domain/` helper, e.g. `program_levels::paired_level_offset(a_levels, b_levels, offset_secs, b_scanned_end_secs, params) -> Option<AbLevelOffset>` plus `program_level_context(levels, active_db) -> ProgramLevelContext` |
| Shared stats | `median_db` is private in `domain/gap_equivalence.rs` — extract it (plus a nearest-rank percentile helper) rather than copying |
| `ScanGaps` | After `finish_with_levels` on A and the B scan, before the level timelines are dropped |
| `GapReport` | One new field `program_levels: Option<ProgramLevels>` — `None` on legacy/test constructors |
| `GapScanJson` | Serialize it, omit when `None` |
| Human stdout | One optional line; not required for DoD |
| Patch path | **No read** in v1 |

**Absent-input behaviour.** B levels exist only when a B session opened **and**
`offset_secs` is `Some` — a readable B with no alignment yields no B timeline at all.
Then `program_levels.offset` is `None` and `program_levels.b` is `None`; A's context
still ships. `program_levels` itself is `None` only when A produced no level timeline. A
truncated B scan is not an error source here (unpaired blocks are simply excluded), but
`n_pairs` and `first/last_pair_secs` make the reduced coverage visible.

**Struct-literal churn:** ~44 `GapReport { … }` literals across `src`, `tests` and
`clip-sync-repair-fixtures` each need the new field. Mechanical, one commit.
`full_surface_gap_report` in `infrastructure/cli/output.rs` is the every-field surface
test and *should* change — that is the test doing its job.

---

## 7. Tests (media-free)

Synthetic `Vec<BlockLevel>` in, expectations out.

| Case | Expect |
|---|---|
| B = A + 6 dB on every active block, offset 0 | `offset_db ≈ 6.0`, `spread_db ≈ 0`, `n_pairs` = active count |
| Same, with a fractional-block `offset_secs` | still `≈ 6.0` (mapping is by time, not index division) |
| A has a long dropout B does not | dropout blocks contribute no pairs; `offset_db` unchanged from the no-dropout case |
| Leading + trailing silent pads of *different* lengths on the two sides | `offset_db` unchanged; no head/tail rule needed |
| B's second half is a different edit (random levels) | `offset_db` still tracks the matching half; `spread_db` clearly elevated |
| First half +2 dB, second half +8 dB | whole-file `offset_db ≈ 5`; **`chunks` shows the two levels** |
| Transient-adjacent blocks with a half-block map error | steadiness filter drops them; `spread_db` far below the unfiltered value |
| Fewer than `MIN_PAIRS` overlapping actives | `None` (abstain) |
| `channels_match == false` | summary present, `channels_comparable: false` |
| Empty / all-inactive timelines | `None` |

Goldens are per-gap analyzer rows (`golden_baseline_invariance`), so no golden rewrite.
`GapScanJson` is `Serialize`-only with no `deny_unknown_fields`, and no path reads scan
JSON back into a `GapReport` — the fields are purely additive.

---

## 8. Explicitly out of scope for gain (so it is not smuggled in)

A later plan may use `offset_db` as a coarse master offset (`gain₀`), separate from
hole-local normalize. That plan must:

- require `spread_db` below a threshold **and** `chunks` consistent — *a single scalar
  must be shown to exist* before one is applied;
- require `channels_comparable` and an `n_pairs` floor;
- deadband small offsets;
- decide the interaction with boost-only local normalize — **stacking is a hazard**, and
  the one known audible failure is already a normalize over-boost.

**This TEMP does not ship that.** Capture + JSON only.

---

## 9. Implement checklist

- [x] `AbLevelOffset` / `AbLevelOffsetChunk` / `ProgramLevelContext` + the pure reduce (pairing, steadiness filter, median/MAD/deciles, chunking)
- [x] Extract `median_db` and a nearest-rank percentile helper to a shared domain spot
- [x] Wire in `ScanGaps` after A/B `finish_with_levels`; set the new `GapReport` fields
- [x] Emit on `GapScanJson` / repair JSON `scan` object (omit when `None`)
- [x] Unit tests in § 7
- [x] Fix the inaccurate "floored at −120" doc comment on `BlockLevel`
- [x] Doc: scan-output paragraph in [gap-scan.md](../gap-scan.md) **and** the scan-object field table in [json-output.md](../json-output.md)
- [x] Do **not** touch `compute_fill_gain` / normalize path
- [x] `cargo test` (lib + touched CLI/scan tests); clippy clean

---

## 10. Done when

Scan JSON for a normal A+B run carries `program_levels.offset` with `offset_db`, `spread_db`
and `chunks`; a synthetic pair with a known constant gain recovers it to within 0.1 dB
at near-zero spread; a synthetic pair with a deliberately damaged A recovers the *same*
offset as its undamaged twin (the duty-cycle-immunity claim of § 2); and no patch/gain
behaviour changes (byte-identical fills vs before).

---

## Superseded draft

The first version of this TEMP (2026-08-25) proposed per-side floor/mid/roof summaries
and `delta_mid_db = mid_B − mid_A`. Dropped: subtracting two independent rank statistics
imports A's damage and B's edit differences into the "level offset" (§ 2), and its
populations were split on the scanner's `silent` flag, which is not a level predicate
(§ 4). The per-side statistics survive in reduced form as `ProgramLevelContext` —
context only, never subtracted.

---

## Related

- [gap-scan.md](../gap-scan.md) — scan recipe, block-level timelines
- [json-output.md](../json-output.md) — scan JSON schema (new fields go in the scan object table)
- [pipeline.md](../pipeline.md) — fill normalization (unchanged by this plan)
- [settled.md](settled.md) § A — B-relative / donor-shoulder gain refuted (distinct from this)
