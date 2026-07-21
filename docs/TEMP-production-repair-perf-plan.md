# Production repair pipeline performance — plan

**Status:** **§0 + Level-C measured (2026-07-20) — plan PIVOTED.** The instrumented production run **refuted the
original candidate (§2.1 mono-downmix hoist: 0.006% of runtime)** and located the real cost: **`char_gate_search`
= 93% of wall-clock**, decomposed (§2.3) into a **flat ~22 s per-bracket score** run once (baseline) + **k times**
(anchor-seam rescue, `gate_anchor_search` = 88% of gate search). Active target: **speed the per-bracket score** —
code review (§2.3) shows it is **not** a full-haystack sweep but a windowed coarse+refine search whose per-candidate
seam correlations are recomputed naively with no cross-candidate reuse. **Lever 2 (hoist placement-invariant
channel selection) LANDED — measured −31% on the unified search** (byte-parity 26/26); **lever 1 (cross-candidate
reuse) is next** —
Level-E (§2.3) confirms it is **FFT** (95% of the search is a dense integer-lag refine): route the refine seam
correlation through `lag_correlation_curve_auto`. FFT is calibration-safe on our corpus and will be scoped to
the production search, leaving the dump/golden untouched (§2.4). See
**[§2.3](#23-level-c--decompose-evaluate_seam_gate-measured-2026-07-20)** / §2.4. Successor
to the archived [archive/TEMP-pipeline-perf-redesign-plan.md](archive/TEMP-pipeline-perf-redesign-plan.md) (D12),
whose dump/fingerprint + characterize→execute + oracle-unification work is **complete**. This doc owns only what
that one didn't: the **production repair path** end-users actually run.

**Scope.** Performance of the production repair pipeline —
`PatchAudio::execute` → `prepare_region_patch` / `characterize_region` → `execute_region_spec` → `splice_into_a`.
**Out of scope:** the diagnostic `--gap-fingerprints` dump (`characterize_gaps_from_decode`) — that path is done
and its perf profile is captured in [gap-fingerprint.md](gap-fingerprint.md) § Performance.

**Structural frame (already landed).** Gap **identification** and **repair** are split and stay split:
**`characterize_region`** runs the gate, dual-fit, and seam reconciliation and emits a typed **`GapRepairSpec`**
(verdict + `GapRepairCell` rooted in [gap-vocabulary.md](gap-vocabulary.md)); **`execute_region_spec`** is
fill-only and produces **`RegionPatch`** PCM from that spec with no re-gating. Production does not consume
fingerprint JSON — the spec *is* the analysis output. This doc optimizes **shared compute inside characterize**
only; it does not merge the phases, add a parallel fast path, or extend the measurement surface.

---

## 0. Prime directive — measure the production path first

**Every perf number we have so far is from the DUMP, not production, and does not transfer.** The dump scores
**every feasible bracket per gap** (for per-bracket `failure_stage`), which measured **~82% of dump wall-clock**
(decode ~12%). Production default (`fit`, `fit_boundary_search = baseline_only`) is designed lean —
**~one unified search per gap**, not a per-bracket enumeration (only `--full`/`full_grid` does the heavy grid).
So the 82%/12% split says nothing about production; production's cost profile is **unmeasured**.

**First task, gating everything below:** instrument `PatchAudio::execute` (decode vs per-gap characterize vs
per-gap execute; within characterize, downmix/RMS/border rebuilds vs the gate search) and run on a licensed
media pair — the same method used for the dump. **Bucket anchored-retry separately:** failed gaps still re-run
`prepare_region_patch` (a second characterize+execute on a subset); tag those gaps so they do not inflate the
per-gap characterize baseline. Only then decide whether any optimization here is worth it. If the measured
production redundancy is negligible, **close this doc** rather than optimizing a non-cost.

**Instrumentation status (2026-07-20): LANDED + RUN COMPLETE.** The production path
already carried `tracing` spans over the coarse buckets; two changes complete the §0 surface:

- **Level A** (`infrastructure/logging/mod.rs`) — setting `CLIP_SYNC_SPAN_TIMING` switches the fmt subscriber
  to `FmtSpan::CLOSE`, so every span emits `time.busy` / `time.idle` on close. Off by default (no change to
  normal output).
- **Level B** (`application/patch_audio.rs::characterize_region`) — a per-gap `characterize` span plus child
  spans `char_b_extract` (B span slice), `char_geometry` (border/RMS/geometry rebuild), `char_dual_fit_input`
  (dual-fit downmix), `char_gate_search` (structure + bracket/waveform + residual). These nest under
  `patch_audio` (pass 1) or `patch_anchored_retry` (retry pass), so anchored-retry stays bucketed separately.

**Running the instrumented pass** (operator, licensed media — no audio lives in-repo):

```powershell
$env:CLIP_SYNC_SPAN_TIMING = "1"
$env:RUST_LOG = "clip_sync_repair=info"
cargo run --release --features clip-sync/default-tracing -p clip-sync-repair -- `
  A.mkv B.m4v --mux out.mkv 2>perf.log
# then aggregate perf.log by span name (sum time.busy per span: decode_a/decode_b, characterize,
# char_gate_search, char_b_extract, char_geometry, char_dual_fit_input, patch_gap, patch_splice)
```

**Non-negotiables:** `--release` (debug timings are meaningless); the real repair path (`--mux`/`--wav`),
**never** `--gap-fingerprints` (that is the dump path §0 rejects); and `CLIP_SYNC_SPAN_TIMING` must be set or
no timings emit. **Reading busy time:** an `.entered()` parent span's `time.busy` *includes* its children, so
a bucket's exclusive cost = its own busy minus its children's busy (e.g. characterize-other = `characterize` −
Σ `char_*`). Anchored-retry buckets appear as `char_*` spans whose parent is `patch_anchored_retry`.

**Measured results (2026-07-20).** Pair: licensed media (equiv-coarse-vs-fine pair 2, `9405b3bf`) — 2 h,
6ch/48 kHz, default profile (`--mux` only ⇒ `fit_boundary_search = baseline_only`, `fill_offset_mode =
Recommended`). The scan-time equivalence gate dropped 17 of 26 gaps before the plan, so **9 gaps reached
`execute`**. Total repair **1872 s**.

| Bucket | Span(s) summed | Time | Share |
|--------|----------------|------|-------|
| **Characterize — gate search** | `char_gate_search` | **1746.2 s** | **93.3%** |
| Decode | `patch_decode_a` + `patch_decode_b` | 120.9 s | 6.5% |
| Characterize — downmix (§2.1 target) | `char_dual_fit_input` | 0.10 s | 0.006% |
| Characterize — b-extract | `char_b_extract` | ~0 | ~0 |
| Characterize — borders/RMS | `char_geometry` | ~0 | ~0 |
| Execute + splice | `patch_gap` + `patch_splice` | 0.008 s | ~0 |

**The cost is per-gap and rescue-path-bound.** Base patch/marginal gaps take ~22–23 s of gate search; the five
**anchor/dual-fit rescue** gaps take far more — 92 s, 207 s, 377 s (anchor), 384 s, **595 s** (dual-fit) — i.e.
**95% of all gate-search time lives in 5 of 9 gaps**, up to **27× the base gate**. `char_dual_fit_input` (the
dual-fit *input* downmix) is cheap; the expense is inside `evaluate_seam_gate` *itself* when a gap doesn't find
an easy winner and falls into the exhaustive search (anchor-seam bracket search / boundary haystack).

**Conclusions:**

1. **§2.1 (downmix hoist) is REFUTED** — its target redundancy is 0.006% of runtime. Killed (see §2.1).
2. **The real cost — `char_gate_search` on rescue gaps — was declared out-of-scope by the old §2.2** ("production
   routes to a winner"). Measurement refutes that for the rescue path. **New active candidate: §2.3** — decompose
   `evaluate_seam_gate` to localize the 595 s.
3. Decode (6.5%), execute, splice: all negligible — do not optimize.

The run recipe above stands for the §2.3 re-run (with the deeper sub-spans added). No `patch_anchored_retry`
spans appeared — default `Recommended` offset mode does not run the anchored-retry pass, so that bucket is empty
on this profile.

---

## 1. Inherited philosophy — one path, shared objects, current vocabulary

The structural frame above is **complete** (D12 step 6); the principles below govern how perf work may change
the code without undoing it. This doc continues the archived plan's design principles verbatim; production perf
is pursued **only** through them (no bespoke fast paths, no scan/prod drift):

- **Compute-once-share** — one decode · one binned-RMS per side · one **mono downmix** · one lag curve; every
  consumer indexes into the shared value. The natural home is `characterize_region`'s per-gap shared context
  (which the D12 step-6 characterize→execute split already established).
- **Shared primitives, no drift** — `domain/{seam_local,donor,dual_fit}` are already single-impl (scan +
  production) ✓; the typed handoff is `GapRepairSpec` / `GapRepairCell` (rooted in
  [gap-vocabulary.md](gap-vocabulary.md) cells, not an ad-hoc score). Any new shared object is expressed in that
  vocabulary and flows through the existing `characterize_region → execute_region_spec` path — not a parallel
  one.
- **FFT lag sweep** — already landed (`lag_correlation_curve_auto`, `domain/seam_local.rs`).

---

## 2. Candidate work

### 2.1 Hoist the shared per-side mono downmix — ~~candidate~~ **REFUTED (2026-07-20)**

**Killed by the §0 measurement.** The redundancy this targeted — `char_dual_fit_input` + `char_b_extract` +
`char_geometry`, the repeated per-gap downmix/extract — measured **~0.1 s of 1872 s (0.006%)**. Per §0's own
rule ("if the measured production redundancy is negligible, close it rather than optimize a non-cost"), this is
not worth doing. The feasibility analysis was sound (the mono downmix *is* byte-identically shareable) — the
opportunity is simply too small. Do **not** re-propose without new measurement showing the downmix is material.
*(This is the second production-perf intuition refuted by measurement; cf. the dump's 8g.5 correlation
pre-filter. Measure before optimizing.)* The §0 caveat foresaw this: "on the production path several of the
dump's downmix consumers do not run … it may be too small to bother." It was.

### 2.2 (obsolete framing) — "production routes to a winner"

The old §2.2 asserted the dump's per-bracket enumeration is *not* a production cost because "production routes to
a winner," and told us not to carry gate-search cost here. **The §0 measurement refutes that for the rescue
path:** a gap that finds no easy winner falls into an exhaustive `evaluate_seam_gate` search costing up to 595 s.
The gate search *is* the production cost. Superseded by §2.3.

### 2.3 Level C — decompose `evaluate_seam_gate` **(MEASURED 2026-07-20)**

Sub-spans landed inside `evaluate_seam_gate` (`application/patch_region.rs`, nested under `char_gate_search`,
gated by `CLIP_SYNC_SPAN_TIMING`): `gate_cache_build` (`FitHaystackCache::build`), `gate_baseline_score`
(baseline bracket), `gate_anchor_search` (`try_anchor_seam_joint_search`), `gate_grid` (boundary grid — empty on
`baseline_only`). Byte-parity held (`patch_audio_integration` 26/26). Re-run on the same pair 2 (`perf_2.log`):

| Sub-span | n | Total | Share of gate search |
|----------|---|-------|----------------------|
| **`gate_anchor_search`** | 5 | **1575.8 s** | **88.5%** |
| `gate_baseline_score` | 9 | 201.8 s | 11.3% |
| `gate_cache_build` | 9 | 0.5 s | 0.03% |
| `gate_grid` | 0 | — | (baseline_only) |

Per gap (`gate total` / `baseline` / `anchor`, seconds): base patch/marginal gaps 3·17·18·24 = ~23 / ~22 / — ;
rescue gaps 19 = 93 / 23 / 70 · 21 = 210 / 21 / 189 · 7 (anchor) = 386 / 20 / 365 · 6 = 397 / 23 / 374 · 22 =
601 / 23 / **578**.

**The unifying cause — a ~22 s per-bracket score.** `gate_baseline_score` is a **flat ~22 s on every gap** (one
baseline bracket), and `gate_anchor_search` is that same primitive run **k times** (one per anchor bracket): gap
22's 578 s ÷ 22 s ≈ **26 anchor brackets**; gap 19's 70 s ≈ 3. So the whole cost is
`(1 baseline + k anchor) × ~22 s/bracket`. `gate_cache_build` (the shared B-downmix prep, i.e. the old §2.1
territory) is confirmed negligible.

**Code review of the ~22 s per-bracket score (2026-07-20) — the "naive `O(n·L)` haystack sweep" framing was
imprecise; corrected here.** Traced `match_gap_fill_unified_in_b_with_timeline` → `unified_search_best_fill_start`/
`_end` → `waveform_min_at_start` → `fill_seam_correlations`:

- **NOT a full-haystack lag sweep.** The search is **windowed** to ±`search_radius_frames` (=`fill_border_search_secs`
  × sr = **10 s × 48 k = ±480 k frames** on the default profile), and **coarse-stepped with a ~2000-candidate cap**
  (`search_coarse_step`) + a bounded integer refine + a ≤128-frame fine polish. `fill_seam_correlations` does no
  internal lag slide — it is one fixed-placement Pearson. So there is already an algorithmic bound; there is **no
  single big sweep to FFT away.**
- **But it IS naive and un-shared per candidate.** At each of the ~thousands of candidates (coarse + integer refine,
  for **both** the start and end searches) it recomputes, from scratch, a fresh `seam_pearson` over the pre/post
  windows **per selected channel** plus the structure `score_pre/post_for_signature`. **No FFT, no prefix-sum /
  running-correlation reuse across adjacent candidates, no memoization.**
- **Concrete redundancy:** `fill_seam_correlations` re-runs `seam_score_channel_indices(a_pre_ch, a_post_ch)`
  (`policies.rs`) inside the per-candidate loop, but that A-side channel selection is **placement-invariant** — it
  does not depend on `start` or B. Pure hot-loop waste.
- **The real multiplier is `k` brackets, not the sweep.** This bounded-but-naive search runs once per baseline
  bracket and once per anchor bracket, so `gate_anchor_search` = `k × (borders + signature + thousands of naive
  correlations)`. The 88 % is bracket-count × per-search cost.

**Levers, re-ranked to the actual code (do NOT "FFT the haystack" — there is no full sweep):**

1. **Cross-candidate reuse in the coarse/refine correlation loop (primary — collapses BOTH buckets).** Prefix-sum
   the seam Pearson numerator/denominator across adjacent candidate starts, or compute the coarse-grid correlations
   in one FFT pass (`lag_correlation_curve_auto` reused as the shared primitive), instead of an independent Pearson
   per candidate.
2. **Hoist `seam_score_channel_indices` out of the per-candidate loop** — **LANDED + MEASURED (2026-07-20).** The
   unified search now computes `seam_score_channels(templates)` once and threads it through
   `fill_seam_correlations_with_channels` (`policies.rs`, `gap_fill_fit.rs`); byte-parity `patch_audio_integration`
   26/26. **Measured `perf_4.log` vs `perf_3.log` (same pair/profile): −31% on `bracket_unified_search`** (20.7 s →
   14.3 s/bracket), `char_gate_search` 1739 s → 1212 s. **Far bigger than the "few %" predicted from code-reading**
   — `seam_score_channel_indices` scans **all** channels' full templates to select the (few) it scores, so the
   per-candidate *selection* was scanning more than the *scoring* it gated. Measure-first: the code estimate
   undershot ~10×.
3. **Bound `k` (anchor-bracket count) and/or the 10 s search window (secondary — rescue gaps only).** `k` reached
   ~26 on gap 22; the 10 s `fill_border_search_secs` sets the ±480 k window width. Only helps rescue gaps.

**Level-D result (`perf_3.log`, 2026-07-20) — CONFIRMED.** Of the per-bracket score (82 brackets = 9 baseline +
73 anchor):

| Sub-span | n | Total | Mean | Share of per-bracket |
|----------|---|-------|------|----------------------|
| **`bracket_unified_search`** (`match_gap_fill_unified_in_b_with_timeline`) | 82 | **1698.9 s** | **20.7 s** | **99.9%** |
| `bracket_signature` (`build_gap_signature`) | 82 | 0.57 s | 6.9 ms | 0.03% |
| `bracket_borders` (template build) | 82 | 0.28 s | 3.4 ms | 0.02% |

The windowed unified search is **97.7% of `char_gate_search`** and **91% of total wall-clock** (82 × 20.7 s =
1699 s). Border build and signature are noise; residual is not in this path. **Measurement chain closed:**
`char_gate_search` (93% of run) → `gate_anchor_search` (88%, k brackets) → `bracket_unified_search` (99.9% of each
bracket). The target is a single function, and the code review (above) already explains the slowness.

**Optimization is now unblocked** — pick from levers 1–3 (cross-candidate reuse; hoist channel selection; bound
`k`/window). Any change: §1 shared-object path, byte-parity via `patch_audio_integration`. **Lever 2 (hoist the
placement-invariant `seam_score_channel_indices` out of `fill_seam_correlations`'s per-candidate loop) is
correct regardless of the others and is the smallest, isolated first PR.** A Level-E split *inside*
`match_gap_fill_unified_in_b_with_timeline` (candidate count vs per-candidate correlation) is only needed if
lever 1's approach (prefix-sum vs FFT coarse grid) needs disambiguating first.

**Cohort note:** the rescue-gap cost only appears when gaps reach the anchor/dual-fit rescue — pick a pair with
rescue targets (pair 2 has 2 dual-fit + 1 anchor; pair 1/7 have 3 dual-fit each). A plain-patch-only pair would
hide the dominant cost.

**Level-E result (`perf_5.log` post-rebuild, 2026-07-20) — lever 1 is FFT, decisively.** Phase spans
`unified_coarse` / `unified_refine` / `unified_fine_polish` inside the unified search (with a `candidates` count
field), byte-parity 26/26. Split of the 1211 s `bracket_unified_search`:

| Phase | Density | Time | Share | Candidates | µs/cand |
|-------|---------|------|-------|------------|---------|
| **`unified_refine`** | **dense (integer step)** | **1121.9 s** | **92.6%** | 787,364 | 1425 |
| `unified_coarse` | sparse (`coarse_step`) | 58.5 s | 4.8% | 49,528 | 1181 |
| `unified_fine_polish` | dense | 30.4 s | 2.5% | 21,074 | 1444 |

Phases sum to 100% of the search (all time is candidate loops). **Dense (refine + polish) = 95.2%**; each refine
pass is **~4,800 contiguous integer candidates** (±`coarse_step`, `coarse_step` ≈ 2,400 frames / 50 ms), each a
full per-channel Pearson (~1.4 ms). **This is the textbook FFT case** — one transform computes the whole
±`coarse_step` band of `Σ aᵢ·bᵢ` in O(M log M) vs ~4,800 independent O(W) Pearsons. Prefix-sum-only leaves the
numerator untouched and is relegated to the 4.8% sparse coarse pass (FFT overkill there).

**Lever 1 implementation shape (confirmed by measurement):** route the **refine + fine-polish** seam correlation
through an FFT lag curve over the ±`coarse_step` band — per channel, pre and post seams — then the candidate
loop does cheap structure-score + penalty lookups against the precomputed correlations. Reuse
`lag_correlation_curve_auto` (already FFT-numerator + prefix-sum-denominator); do **not** hand-roll. The coarse
pass can keep direct dot products (or a light prefix-sum normalization) — it is 4.8%, not worth FFT.

### 2.4 Lever 1 — FFT/prefix-sum calibration-safety + fingerprint-dump scoping

Lever 1 (cross-candidate reuse) will introduce FFT into the correlation path. Two questions decided **before**
any lever-1 code:

**(a) Will FFT skew our findings? — NO (checked against the corpus, 2026-07-20).** f64 FFT carries ~**1e-10**
relative error, so a finding could only flip if a value sat within ~1e-10 of its threshold. Scanning
**equiv-coarse-vs-fine** (all 8 pairs) for the smallest margin of every correlation-derived gate to its
threshold:

| Finding (correlation-derived) | Threshold | Closest value in corpus | Headroom vs 1e-10 |
|-------------------------------|-----------|-------------------------|-------------------|
| `prominence` (uniqueness tiebreaker) | 0.15 | **3.9e-4** | ~4×10⁶ |
| `step_real` (`post_r − post_global_r`) | 0.15 | 1.5e-2 | ~1×10⁸ |
| `peak_z` (uniqueness, primary) | 12 | 2.8e-2 | ~3×10⁸ |
| `gate_pass` (`min(seam_r)`) | 0.35 | 7.9e-2 | ~8×10⁸ |

The tightest value in the entire corpus (`prominence` at 3.9e-4) is **six orders of magnitude** above FFT's
noise. Three independent reasons it is safe: (1) margins dwarf the error (table); (2) the **scan-time
equivalence gate** — the whole point of equiv-coarse-vs-fine — is **silence-character (RMS/donor-silence), not
correlation**, so FFT cannot touch it at all (its margins 0.10 dB / 0.05 are moot); (3) the search already
tie-breaks at `SCORE_TIE_EPSILON = 1e-9` (`gap_fill_fit.rs`) using deterministic **position**, and 1e-9 > 1e-10,
so FFT cannot even flip a near-tie argmax (the discrete-jump case) — the positional rule dominates the wobble
zone. Standard belt-and-suspenders regardless: gate lever 1 behind a `fft_curve ≈ naive_curve` regression test
within tight ε (as the archived FFT-lag-sweep note already prescribes).

**(b) The fingerprint-dump caveat — and our decision: scope FFT to the production search only.** The committed
corpus/golden (`equiv-coarse-vs-fine`, `re-anchor-dual-fit-on-nominal.golden.json`) are **exact-float
snapshots**. If lever 1's FFT lands in a primitive **shared** with the dump/oracle path, re-generating them
would drift the stored numbers at ~1e-10 — **no verdict changes**, but an exact-match golden diff would break on
the last digits, forcing a re-freeze. **Decision (2026-07-20): scope the FFT change to the production unified
search only** (the `match_gap_fill_unified_in_b` candidate loop), leaving the dump's oracle path — and thus the
committed corpus/golden — **byte-untouched**. This is also consistent with §1 (this doc owns the production path,
not the dump) and §0-scope. **Only** gate-and-re-freeze instead if a concrete reason forces the FFT into a
genuinely shared primitive (e.g. the dump path turns out to need the same speedup and single-impl/no-drift
outweighs the re-freeze cost); absent that, prefer the scoped change and no golden churn.

### 2.5 Lever 1 — implementation design (FFT search + exact re-score belt) **(APPROVED 2026-07-20)**

**Goal: byte-identical output, not merely ε-close.** The FFT is used only to *find where to look*; the final
placement, confidence tier, and reported pre/post scores all come from an **exact naive re-score** at the chosen
placement. Identical placement → identical splice → identical PCM, so `patch_audio_integration` (byte-parity)
*validates* lever 1 — the same guarantee that anchored levers 2/3.

**Scope of the FFT (bounds any placement drift to sub-ms, by design):**
- **Coarse pass stays naive.** It is 4.8% of the search (Level E) and sparse (FFT doesn't help a stepped grid).
  Keeping it naive means the coarse winner that *anchors the refine window* is **bit-identical to today** ⇒ no
  gross (±`coarse_step` ≈ 50 ms) relocation is possible. FFT can only ever move a placement *within* the refine
  band.
- **Refine + fine-polish use FFT.** These are the dense contiguous integer bands (95% of cost). Compute the
  pre/post seam cross-correlation over the ±`coarse_step` band once per search, **per selected channel**, via the
  existing `lag_correlation_curve_auto` (FFT numerator + prefix-sum denominator — do not hand-roll). The candidate
  loop then does the cheap structure-score + penalty + a **lookup** into the precomputed correlations instead of
  a fresh `fill_seam_correlations` per candidate.

**Exact re-score belt (always on — NOT a flag):**
- After the FFT search picks a winner, re-score the **near-tie cluster** (every candidate FFT rates within
  ~(FFT-error + `SCORE_TIE_EPSILON`) of the top — almost always 1, occasionally a few adjacent frames) with the
  **exact naive `fill_seam_correlations` over the full seam window** (same window, ~250 ms × ch — the window does
  **not** shrink; only the *count* of naive evaluations drops from ~800k to ≈1), and make the final
  placement/tier/reported-score decision on those exact values with the unchanged tie-break. This is what makes
  the path byte-identical; it is intrinsic, not optional (there is no legitimate "FFT without re-score" in
  production — it would buy sub-ms drift for ~no speed, since the re-score is ≈1 candidate's cost vs the ~4,800
  the FFT replaced).

**Handling a re-score discrepancy (the belt doubles as a free runtime correctness monitor):**
- **Near-tie (≤ ~1e-10, within `SCORE_TIE_EPSILON`):** the belt simply picks the exact-naive winner. Not a
  problem — the belt working as designed; byte-identical result; no logging.
- **Large discrepancy (exact vs FFT value at the winner > a tight threshold, e.g. 1e-6 — far above 1e-10, far
  below any signal):** this can only be an **FFT porting bug** (lag convention / edge mask / normalization — the
  classic traps). Two layers: **(1) test-time (primary):** a `fft_curve ≈ naive_curve` ε test + a
  **placement-diff test** (naive vs FFT `start_frame`/`end_frame` on the corpus) fail the build before ship;
  **(2) runtime (self-healing):** since the winner is re-scored anyway, compare exact-vs-FFT for free — on a
  divergence beyond threshold, **fall back to the full naive search for that one gap** (correct, unaccelerated)
  and emit a `warn`. Per-gap, not an abort. A latent FFT bug therefore degrades a gap to "slow but correct,"
  never "wrong placement."

**CLI / defaulting:**
- **Re-score:** always on, **no flag** (it is the correctness mechanism, and nearly free).
- **FFT:** behind a hidden/advanced opt-out **`--no-fft-seam-search`** (A/B tool + escape hatch for pathological
  media outside the 8-pair corpus). **Default ON iff byte-parity (`patch_audio_integration`) passes** — a
  provably-output-neutral speedup has no downside and a default-off perf feature helps nobody. If only bounded
  sub-ms drift is achievable (not bit-identical), instead default **OFF** (opt-in) until a real-media run confirms
  no audible change — the conservative fallback, mirroring the dual-fit ship-behind-flag precedent.

**Validation order:** (1) `fft_curve ≈ naive_curve` unit ε test; (2) placement-diff test on the corpus;
(3) `patch_audio_integration` byte-parity; (4) re-run §0 recipe for the measured speedup. Only after (3) is green
does the default flip on.

**Implementation progress (2026-07-20):**

- **Part A — end-search hoist: LANDED, byte-identical (byte-parity 26/26).** In `unified_search_best_fill_end`,
  the pre seam and waveform seam are anchored at the FIXED `fill_start`, so they are constant across every `end`
  candidate — hoisted out of the per-candidate loop (`const_pre_score` / `const_wave_min`). Same value ⇒ no
  flag, no re-score, byte-parity-validated. Covers the entire end-search half of the refine as pure
  cross-candidate reuse. (Start search still needs the FFT — its seams genuinely slide with `start`.)
- **Part B foundation 1 — band primitive: LANDED + equivalence-tested.**
  `seam_local.rs::seam_correlation_over_bases(a, b, base_lo, base_hi)` returns the dense per-base seam Pearson
  over a contiguous band in one `lag_correlation_curve_auto` pass (entry `i` ↔ base `base_lo + i`), equal to
  `seam_pearson` per placement within ε ≤ 1e-8 on both auto branches (`seam_correlation_over_bases_matches_naive`).
- **Part B foundation 2 — band evaluator: LANDED + equivalence-tested.**
  `policies.rs::fill_seam_correlations_band(...)` precomputes `(pre, post)` for every start in a band, mirroring
  `fill_seam_correlations_with_channels` exactly (`use_channels` / bounds / per-channel `best_channel_correlation`
  / mono fallback), returning `None` on any non-uniform band-edge case (caller falls back to naive there). Matches
  the per-start naive call within ε on both the multichannel and mono paths, FFT branch exercised
  (`fill_seam_correlations_band_matches_per_start`). Wired into the start-search refine (Part B, below), so the
  `#[allow(dead_code)]` is now removed. **Confirmed prerequisite:** `clip_sync::…::normalized_correlation` (band
  path) and `metrics::normalized_correlation` (`seam_pearson` path) are byte-for-byte identical, so band == naive
  exactly on the naive branch; the evaluator test also now *guards* against future drift between the two copies.
- **Part B — start-search integration: LANDED behind a flag (default OFF), byte-parity 26/26, placement-diff green.**
  `unified_search_best_fill_start` now: (1) `consider` takes the `wave_min` as `Option<f64>` — `None` computes it
  naively in-line (byte-identical to before), `Some` uses a value the refine looked up; the **coarse pass stays
  naive** (its winner anchors the refine window bit-identically, so the FFT can only move a placement *within* the
  ±`coarse_step` band); (2) the refine precomputes `build_wave_min_band` (one `fill_seam_correlations_band` FFT
  pass; `None` ⇒ non-uniform band edge ⇒ naive fallback for the whole refine), and the candidate loop looks up
  `wave_min`, re-applying `waveform_min_at_start`'s `placement_in_bounds` NEG_∞ gate on top of the band value;
  (3) **belt + runtime monitor:** the FFT only *finds where to look* — the winner's band value is checked against
  an exact naive re-score, and a divergence > `FFT_SEAM_DISCREPANCY_TOL` (1e-6, ≫ 1e-10 FFT noise) can only be a
  porting bug, so that **one gap** degrades to the exact naive refine + a `warn` (per-gap, never an abort). No
  separate reported-value re-score is needed — `match_gap_fill_unified_in_b_with_timeline` already re-derives the
  winner's reported seam/structure scores naively downstream. (4) **Flag: `use_fft_seam_search` threaded through
  `_with_timeline`; wired end-to-end and DEFAULT ON.** `RepairConfig.fft_seam_search` (serde `default_true`) →
  `PatchRequestSettings` → `PatchAudioRequest` → `SeamGateConfig::from_repair` → the search; CLI **`--no-fft-seam-search`**
  is the opt-out. The public `match_gap_fill_unified_in_b` (dump/fixtures) always passes `false`, so the corpus/
  golden stay byte-exact (§2.4). (5) **Placement-diff test** `fft_seam_search_matches_naive_placement` (flag on vs
  off → same `alignment.start_frame`). **Fine polish left naive this PR** (Level E: 2.5% of the search) — trivial
  follow-up if wanted.
- **Lever 1b (NEW finding, 2026-07-21; CONFIRMED by `perf_6.log`) — the FFT band alone delivers only ~16%, not
  ~92%; the repeat penalty is active by default and re-incurs the lever-2 cost per candidate.** The band replaces
  only `waveform_min_at_start`.
  - **Measured (`perf_6.log` vs `perf_5.log`, same pair 2, FFT default-ON):** `char_gate_search` 1259.7 s →
    1062.3 s (**−15.7%**); `unified_refine` 1121.9 s → 936.3 s (**−16.5%**, per-candidate 1425 µs → 1189 µs);
    `bracket_unified_search` 14.8 → 12.5 s. **No correctness drift:** 0 `fft seam band diverged` warns (belt never
    fired), the marginal-seam WARNs are digit-identical across runs (gap 17 `pre=0.37 post=0.28`, gap 18 `0.28/0.28`,
    gap 24 `0.29/0.35`), 23/26 repairable + 9 regions in both. **Sharper still:** `perf_5` predates Part A (end
    hoist), so this −16.5% folds in *both* the entire end-search waveform removal (exact) *and* the start FFT band —
    i.e. essentially the whole `waveform_min_at_start` seam correlation is gone from the refine, and it still moved
    only ~16%. So that seam correlation was ~16% of the per-candidate refine; the other **~83% is the repeat
    penalty**. **Verdict: default-ON stays (clean + a real −16%); the residual is 1b's target.** But `unified_fit_score_with_repeat` also calls `repeat_penalty_at_placement`
  **unconditionally** for every finite candidate when `repeat_penalty_weight > 0 ∧ waveform_weight > 0` — and the
  production default is `fill_repeat_penalty_weight = 0.4` (config default; test `…defaults_repeat_penalty_weight`)
  with `fill_fit_waveform_weight = 0.65`, so it *is* on. Each call runs `fill_repeat_correlations` (2 **mono**
  `seam_pearson` over the repeat window — cheap) **+** `fill_seam_correlations` (`gap_fill_fit.rs:315`), and that
  second call is the expensive, redundant one:
    - It is **`fill_seam_correlations_with_channels` under the hood** (`policies.rs:1207`) — the *same*
      multichannel best-channel seam correlation the main loop already computed for this candidate (`wave_min =
      pre_seam.min(post_seam)`), so its `(pre_seam, post_seam)` is **byte-identically already known**.
    - **Lever-2 regression:** the plain `fill_seam_correlations` wrapper re-runs `seam_score_channel_indices`
      **per candidate** — the exact all-channel selection scan **lever 2 hoisted out of the main loop** (which
      lever 2 measured as the dominant per-candidate cost, undershooting ~10×). The main search now threads the
      hoisted `score_channels`; the repeat-penalty path does **not**, so it silently pays the pre-lever-2 cost on
      every candidate. This — not the mono repeat Pearsons — is the likely bulk of the residual in `perf_6`.

  **Next optimization (its own piece), ordered by cost/benefit:** (a) **free + byte-identical, do first:** thread
  the band's already-computed `(pre_seam, post_seam)` **and** the hoisted `score_channels` into the penalty →
  drops the `fill_seam_correlations` call entirely (redundant value) *and* removes the re-incurred lever-2 channel
  scan; (b) `fill_repeat_correlations` is another contiguous-in-`start` band → the same
  `seam_correlation_over_bases` primitive collapses it; (c) partial short-circuit via the **known seam values**
  (not `wave_min` alone): `wave_min` gates only 2 of the 3 nonzero branches (both need `wave_min < 0.45`); the
  third, `asymmetric_post_dup`, is `wave_min`-independent, so a full early-return-0 needs `wave_min ≥ 0.45` **and**
  the asymmetric branch excluded (`post_seam − pre_seam ≤ 0.35`, checkable from the seam values (a) already
  supplies). Do NOT re-scope lever 1 for this — land it, measure, then size 1b against the measured residual.
- **Lever 1c (NEW finding, 2026-07-21) — cheap pre-gates run AFTER expensive work; two more compute-before-guard
  sites found in the same audit as 1b.** Same principle as 1b (do the cheap gate first), at two other cost centers:

  - **#2 — the anchor-bracket search runs a full unified search on EVERY bracket before the viability gate
    (`patch_region.rs`, `evaluate_seam_gate_fit_candidate`). Highest structural upside — it multiplies by `k`**
    (the bracket count that made gap 22 = 578 s / ~26 brackets). Order per anchor bracket: (1) `gate_structure_align`
    (`:1628`) = the **full `bracket_unified_search`** (99.9% of a bracket, Level D); (2) `structure_passes_gate`
    (`:1646`, cheap — but genuinely needs the search's structure scores, so it *can't* precede it); (3)
    `anchor_bracket_both_matchable_at_gate` (`:1666`) — an **FFT-xcorr viability check that can reject the bracket
    outright**, run **after** the full search. So a bracket that fails matchability paid for a complete unified
    search first. Where 1b speeds *each* bracket, this removes *doomed* brackets — it attacks the `k` in
    `gate_anchor_search = k × per-bracket`. **NOT a free reorder (needs validation):** the gate keys on the
    *searched* placement (`alignment.start_frame`), so it can't be hoisted verbatim. Brackets are already
    pre-filtered by `list_anchor_candidates_a` (matchable anchors) → `list_feasible_anchor_brackets` (geometry
    only); the open question is whether a **cheap matchability proxy at the bracket's NOMINAL placement** is a
    provable *superset* filter (rejects only brackets the searched-placement gate would also reject) that can run
    before the search to cut `k`. Design change, gated on that superset argument + placement/byte validation —
    **size it after `perf_6` shows how much `k`-bound cost remains post-lever-1.**
  - **#3 — `try_dual_fit`: the content-existence gate runs dead last, after two FFT seam searches (`dual_fit.rs`).**
    Per-gap (rescue path only, so smaller), but a clean mechanical, byte-identical reorder. Order: `seam_local_peak`
    pre (`:104`) + post (`:114`) = two ±600 ms FFT lag searches **first**; then `gate_pass` (`:134`), `step_real`
    (`:157`), `donor_interior_at` (`:172`), and finally **`program_quiet_at_nominal` (`:188`)** — the "is the nominal
    donor span even non-silent (content to fill)?" check — **last**. `program_quiet_at_nominal` depends only on
    `b_mono` + nominal geometry (`b_mapped_start`, `gap_frames`), **none** of the seam peaks, so hoist it to the top
    to reject a program-quiet gap before paying for the two FFT searches. Secondary nit: `gate_pass` needs both
    peaks, but if the *pre* peak already falls below the floor, `smin` fails regardless of post — a first-peak
    early-out skips the second `seam_local_peak`.

  **Ranking (updated after `perf_6`):** **1b(a) is now the priority** — the measurement shows ~83% of the refine
  is the repeat penalty, whose `fill_seam_correlations` re-incurs the lever-2 channel scan (which lever 2 measured
  at ~31% of a bracket); 1b(a) drops it free/byte-identical and should recover a chunk comparable to or larger than
  the FFT itself. #3 is a mechanical byte-identical reorder (do opportunistically). #2 is the big-but-risky
  `k`-reduction — size it only after 1b lands and re-measures the residual.
- **Part B — (6) licensed-media perf run: DONE (`perf_6.log`, 2026-07-21).** FFT default-ON on pair 2 measured
  **−15.7% on `char_gate_search`** (−16.5% on the refine) with **zero correctness drift** (see the 1b measurement
  block above: no divergence warns, digit-identical seam WARNs, 23/26 + 9 regions). Partial win exactly as 1b
  predicted; the residual is the repeat penalty → **1b(a) next**. **Validation note:** the FFT winner *may* differ
  from naive by a sub-ms near-tie (not
  guaranteed bit-identical), so byte-parity strictly gates only the flag-off path — **but in practice
  `patch_audio_integration` is now GREEN 26/26 with the flag ON by default** (harness mirrors the production
  default), i.e. no fixture surfaced a near-tie divergence, and it ran in 333 s vs 394 s naive (uncontrolled, but
  a real speedup). The flag-ON path is thus gated by the placement-diff test (✓) + integration 26/26 (✓); the
  licensed-media run remains the last confirmation of the *magnitude* + no audible change.

**Follow-up hygiene (not part of lever 1):** `clip_sync::…::offset_refinement::normalized_correlation` and
`crate::domain::metrics::normalized_correlation` are exact duplicates. Consider consolidating to a shared numeric
home (low priority — the band evaluator test now guards drift; `metrics` is intentionally dependency-free, so this
is a `clip_sync`-side move, not a bundle-into-perf change).

---

## 3. Interaction with the policies module split — **resolved: no trigger**

The policies-split phases **P2** (`silence.rs`) / **P3** (`gap_borders.rs`) were only ever going to be *triggered
by* the §2.1 downmix hoist (extract-when-you-touch: land the extraction as a byte-preserving PR adjacent to the
hoist). **§2.1 is refuted (§0), so its §3-step-1 branch fires: "if the hoist isn't worth it → stop; leave the
policies split fully opportunistic."** P2/P3 stay **pending with no perf forcing them**; P1/P4 already fired
independently. If §2.3 eventually optimizes a stage that lives in `silence.rs`/`gap_borders.rs`, the same
extract-when-you-touch rule applies then — but §2.3's likely targets are in `patch_region.rs` /
`seam_local.rs` (the gate/anchor-seam/lag search), which is P4 territory (already extracted), not P2/P3.

---

## 4. Non-goals / deferred (do not re-propose)

- **Per-bracket-oracle gating (dump 8g.5)** — DUMP-only cost, **deferred** after two approaches were refuted by
  measurement (reclassification/short-circuit; correlation pre-filter). See the archived plan's 8g.5 row +
  [gap-fingerprint.md](gap-fingerprint.md) § Performance. Not a production concern (production doesn't enumerate
  all brackets).
- **Dump/fingerprint performance** — complete + archived.
- **Bespoke production fast paths / parallel code** — perf only via the shared-object path (§1).
- **Mono-downmix hoist (old §2.1)** — REFUTED by §0 (0.006%). Do not re-propose without new measurement.

---

## 5. Validation

- **Measure-first (§0):** production timing on a licensed media pair, before and after any change. §2.3 repeats
  this: land the `evaluate_seam_gate` sub-spans (instrumentation-only, byte-neutral), **re-run** the §0 recipe,
  and only then choose a stage to optimize.
- **Byte-parity:** `patch_audio_integration` (production byte-parity) must stay green through any change — a
  perf optimization must be byte-identical, so patched PCM cannot change. (Passed at every step so far: Level-A/B,
  Level-C, Level-D, and lever 2 — all 26/26, 2026-07-20.)
- **Lever 1 FFT is *not* byte-identical** (it changes the numeric path at ~1e-10), so it is the one exception to
  strict byte-parity. Gate it two ways: (1) a `fft_curve ≈ naive_curve` regression test within tight ε; (2)
  **scope it to the production search only so the fingerprint dump / committed corpus / golden are untouched**
  (§2.4) — no golden re-freeze. Calibration-safety on the corpus is verified (§2.4a): tightest threshold margin
  is 3.9e-4, ~4×10⁶ above FFT's error, and the equivalence gate is non-correlation so it is FFT-immune.
- **Policy extractions:** byte-preserving per the policies-split plan's fast gate (compile + clippy +
  `pr-repair`), each as its own PR — *if* §2.3 ever touches a P2/P3 region (see §3).

---

## 6. References

- [archive/TEMP-pipeline-perf-redesign-plan.md](archive/TEMP-pipeline-perf-redesign-plan.md) — predecessor
  (audit §1, cost hierarchy §1.3, characterize→execute §2.5, 8g fingerprint unification, §3.1 hoist
  feasibility). The durable audit history.
- [TEMP-policies-module-split-plan.md](TEMP-policies-module-split-plan.md) — P2/P3 triggers for the hoist.
- [gap-fingerprint.md](gap-fingerprint.md) § Performance — the dump-path profile (why 82%/12% is dump-only).
- [gap-vocabulary.md](gap-vocabulary.md) — the cell vocabulary the shared objects are rooted in.
