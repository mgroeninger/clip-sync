# Production repair pipeline performance — plan (DRAFT)

**Status:** not started; **measure-first** (2026-07-11). Successor to the archived
[archive/TEMP-pipeline-perf-redesign-plan.md](archive/TEMP-pipeline-perf-redesign-plan.md) (D12), whose
dump/fingerprint + characterize→execute + oracle-unification work is **complete**. This doc owns only what that
one didn't: the **production repair path** end-users actually run.

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

**Measurement output shape** — fill this table from the first licensed-pair run; use it to rank candidates
(§2) and to decide whether to close the doc:

| Bucket | What to time | Notes |
|--------|--------------|-------|
| **Decode** | `decode_ab` (once per execute) | Already shared; expect small % |
| **Characterize — gate search** | structure + bracket/waveform + residual per gap | Dominant candidate on lean `fit` |
| **Characterize — downmix** | repeated `interleaved_to_mono` / span extracts per gap | §2.1 hoist target |
| **Characterize — borders/RMS** | border rebuilds, binned-RMS, donor interior | Cheap tier; confirm before optimizing |
| **Characterize — anchored-retry** | second-pass `prepare_region_patch` on failed gaps only | Exclude from per-gap baseline |
| **Execute** | `execute_region_spec` + splice per gap | Should be thin post step 6 |
| **Splice / crossfade** | `splice_into_a` | End of pipeline |

Percent columns are TBD until measured; do not import the dump's 82%/12% split.

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

## 2. Candidate work (all gated on §0 measurement)

### 2.1 Hoist the shared per-side mono downmix

Re-homed from the archived plan's §3 step 8 / §3.1 (it was mis-filed there — §3.1 targets
`characterize_region`'s shared context, which is the **production** path). The code-grounded feasibility from
that analysis carries over unchanged:

- **What is NOT shareable (do not attempt):** "one max-radius border template, slice for all consumers" is
  **not byte-preservable** — the peak-relative trim threshold (`trim_low_energy_*`) and the `border_frames`-
  bounded silence walk both couple to the radius, and consumers pass genuinely different `GapBorderSpec`
  (`border_frames`/standoff/floor all differ). Rejected.
- **What IS shareable, byte-identical:** the **mono downmix**. `interleaved_to_mono` is a pure per-frame mean,
  so `mono(samples)[a..b] == mono(samples[a..b])` — every consumer's sub-slice equals the slice of one shared
  wide per-gap downmix. The real redundancy is **A/B being re-downmixed repeatedly per gap** (nominal donor,
  aligned donor, borders, levels each re-downmix their span). One shared downmix in `characterize_region`'s
  context removes that with no threshold/phase hazard. The per-consumer shaping on top (silence walk, peak-trim,
  phase-anchored chunk binning) is **not** shareable but is O(border)/O(span) cheap and does not need to be.
- **Production-specific caveat:** on the production path several of the dump's downmix consumers do **not** run
  (`seam_probe`/`wide_envelope`/diagnostic `lag` are dump-only X-set). So the production redundancy is
  **narrower** than the dump's — which is exactly why §0's measurement matters: it may be too small to bother.

### 2.2 Others (surfaced by measurement, not assumed)

The dump's dominant cost (per-bracket oracle enumeration) is **not** a production cost — production routes to a
winner. So do not carry it here. Any further production candidate must come from the §0 measurement, not from
the dump's §1.3 hierarchy.

---

## 3. Interaction with the policies module split

The §2.1 downmix hoist touches `domain/policies.rs`'s **silence** (binned-RMS) and **gap_borders** (extract)
regions — the exact regions [TEMP-policies-module-split-plan.md](TEMP-policies-module-split-plan.md) phases
**P2** (`silence.rs`) and **P3** (`gap_borders.rs`) cover. The archived pipeline plan's §2.6 trigger table is
**closed**; **P2/P3 live status for this workstream is tracked here** (§3 ordering below), not in the archive.

**Decision — do NOT complete the policies split as a prerequisite.** That plan's own sequencing makes P2/P3
**triggered by** the step-8 hoist ("the hoist *needs* this owner — decomposition = the perf motion"), under its
**extract-when-you-touch** rule: each extraction lands as a **separate byte-preserving PR adjacent to** the
hoist, **never bundled** into the hoist's behavior change (bundling would wreck the "diff proves no behavior
change" guarantee).

So the ordering is **interleaved, gated on §0**:

1. Measure production (§0). If the hoist isn't worth it → **stop**; leave the policies split fully opportunistic
   (P1/P4 triggers already fired independently; P2/P3 stay pending with no perf forcing them).
2. If the hoist is worth it → for each region it touches: **first** land the policies extraction (P2 `silence.rs`
   / P3 `gap_borders.rs`) as a standalone byte-preserving PR (fast gate: `cargo build --all-targets` + clippy +
   `pr-repair` tier), **then** land the downmix hoist against the new single owner.

P4 (`seam_scoring.rs`) trigger was 6b (already passed) — "ready" and independent of this perf work; not required
for the downmix hoist.

---

## 4. Non-goals / deferred (do not re-propose)

- **Per-bracket-oracle gating (dump 8g.5)** — DUMP-only cost, **deferred** after two approaches were refuted by
  measurement (reclassification/short-circuit; correlation pre-filter). See the archived plan's 8g.5 row +
  [gap-fingerprint.md](gap-fingerprint.md) § Performance. Not a production concern (production doesn't enumerate
  all brackets).
- **Dump/fingerprint performance** — complete + archived.
- **Bespoke production fast paths / parallel code** — perf only via the shared-object path (§1).

---

## 5. Validation

- **Measure-first (§0):** production timing on a licensed media pair, before and after any change.
- **Byte-parity:** `patch_audio_integration` (production byte-parity) must stay green through any hoist — a
  shared downmix must be byte-identical, so patched PCM cannot change.
- **Policy extractions:** byte-preserving per the policies-split plan's fast gate (compile + clippy +
  `pr-repair`), each as its own PR.

---

## 6. References

- [archive/TEMP-pipeline-perf-redesign-plan.md](archive/TEMP-pipeline-perf-redesign-plan.md) — predecessor
  (audit §1, cost hierarchy §1.3, characterize→execute §2.5, 8g fingerprint unification, §3.1 hoist
  feasibility). The durable audit history.
- [TEMP-policies-module-split-plan.md](TEMP-policies-module-split-plan.md) — P2/P3 triggers for the hoist.
- [gap-fingerprint.md](gap-fingerprint.md) § Performance — the dump-path profile (why 82%/12% is dump-only).
- [gap-vocabulary.md](gap-vocabulary.md) — the cell vocabulary the shared objects are rooted in.
