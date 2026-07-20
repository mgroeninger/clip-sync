# Production repair pipeline performance — plan

**Status:** **§0 measured (2026-07-20) — plan PIVOTED.** The first instrumented production run **refuted the
original candidate (§2.1 mono-downmix hoist: 0.006% of runtime)** and located the real cost: **`char_gate_search`
= 93% of wall-clock**, concentrated in the **anchor/dual-fit rescue gaps** (92–595 s each). Active work is now
**[§2.3 — decompose `evaluate_seam_gate` (Level C)](#23-level-c--decompose-evaluate_seam_gate-active)**. Successor
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

### 2.3 Level C — decompose `evaluate_seam_gate` **(ACTIVE)**

**The measured target.** `char_gate_search` is 93% of wall-clock and 27× more expensive on the 5 rescue gaps
than the 4 base gaps. Before optimizing anything, **localize where a rescue gap spends its 595 s** — the same
measure-first discipline that just killed §2.1. This is one function deeper than the Level-B spans: add
sub-spans *inside* `evaluate_seam_gate` (`application/patch_region.rs`) around its stages so a re-run of the §0
recipe attributes the rescue-gap cost:

- **structure match** — `build_gap_signature` / structure-tier scoring over the signature context.
- **bracket / waveform search** — the feasible-bracket scoring over the `fill_border_search_secs` haystack
  (even lean `baseline_only` sweeps the haystack per placement).
- **anchor-seam bracket search** — the editorial-anchor rescue (`max_anchors_per_side` × `max_anchor_bracket_secs`
  placements, each scored over the haystack). **Prime suspect:** gap 7 (the *anchor* rescue) alone cost 377 s.
- **residual / floor** — least-squares cancellation per selected channel.
- **FFT lag sweep** — already `lag_correlation_curve_auto`; confirm the anchor/bracket search actually routes
  through it and isn't running a naive `O(n·L)` sweep per placement (the likely 595 s culprit — many placements
  × wide haystack × 6 channels).

**Only after** that second measurement points at a specific stage do we pick an optimization — and it must still
obey §1 (shared-object path, no bespoke fast path, byte-parity via `patch_audio_integration`). Likely shapes
(unconfirmed until measured): reuse one FFT-accelerated lag curve across the anchor-seam placements instead of
recomputing per placement; hoist the per-channel border correlation inputs; bound the anchor-seam haystack. Do
not build any of these before the Level-C run.

**Cohort note:** the rescue-gap cost only appears when gaps actually reach the anchor/dual-fit rescue — pick a
§0 pair with rescue targets (pair 2 has 2 dual-fit + 1 anchor; pair 1/7 have 3 dual-fit each). A pair of only
plain patches would hide the dominant cost.

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
  perf optimization must be byte-identical, so patched PCM cannot change. (The Level-A/B instrumentation already
  passed this: 26/26, 2026-07-20.)
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
