# `bracket_fill` elimination — plan

Status: **planned** (not started). Revised 2026-07-24 after a code/measurement
audit (see §9 for what changed and why).

Kill the transitional `bracket_fill: Option<Vec<f32>>` carry on
`RegionCharacterization::Patch`, making the characterize→execute handoff
**decision-only** (a `GapRepairSpec`). Execute re-derives the bracket fill PCM
from the spec's `FillAlignment` + `BExtractWindow` + the decode buffers via the
already-shadowed `execute_bracket_fill`. This is behavior-**preserving**
(byte-parity gated), not byte-*text*-preserving — it moves where PCM is
assembled and changes buffer lifetimes, so it is **not** part of the M-MOD
module split.

Paths below are relative to
`crates/clip-sync-repair/src/application/patch_audio/` unless stated.

---

## 0. Relationship to other plans (read first)

- **Provenance.** This is the breakout of one deferred row from
  [archive/TEMP-pipeline-perf-redesign-plan.md](archive/TEMP-pipeline-perf-redesign-plan.md)
  — specifically §3 step 8 **Hoists**, note **(b)**: "`execute_bracket_fill` goes
  live (spec self-sufficient); transitional `bracket_fill: Option` dropped; the
  temporary 2× fill/border assembly appears *and* is deduped." That doc is the
  authoritative history for steps 6a–6c and the 6b.3 sub-step ledger; do not
  re-open it — track the flip here.
- **The Hoists half of that row is dead.** The mono-downmix hoist (redesign §3.1,
  production-perf §2.1) was **REFUTED by measurement 2026-07-20**: 0.1 s of
  1872 s = **0.006%** of runtime, with an explicit "do not re-propose without new
  measurement" ([archive/TEMP-production-repair-perf-plan.md](archive/TEMP-production-repair-perf-plan.md)
  §2.1, §0 table). This plan therefore does **not** gate on it. What survives of
  the "2× assembly" worry is a question to *measure* (§3, phase **M0**), and the
  thing to measure is the fill assembly itself — not the downmix.
- **Not the module split.** [TEMP-patch-audio-module-split-plan.md](archive/TEMP-patch-audio-module-split-plan.md)
  (M-MOD, P1–P6 **done**) was verbatim relocation with **no** behavior change.
  This plan *does* change behavior-adjacent structure and must be gated on
  byte-parity, so it is deliberately a **separate** work item — do not fold it
  into the split ledger or land it as split cleanup.
- **`large_enum_variant` is a side effect, not the goal.** The `#[allow(clippy::large_enum_variant)]`
  on `RegionCharacterization` (`region.rs:1051`) is driven by the large `Bracket`
  struct-variant *inside* `GapRepairSpec`, which **both** arms carry — not by the
  24-byte `Option<Vec<f32>>`. Dropping `bracket_fill` does not shrink anything; it
  makes `Patch { spec }` and `Skip(spec)` carry an identical payload, so the
  size disparity goes to zero and the `#[allow]` becomes removable. Expect **no**
  size win, only the lint clearing.

---

## 1. Problem

Today the Bracket path assembles the fill PCM during **characterize**
(`characterize_region`, `region.rs:1873`) and carries that `Vec<f32>` across
the two loops inside `RegionCharacterization::Patch { spec, bracket_fill }`.
`execute_region_spec` then reads it back via
`bracket_fill.expect("bracket verdict requires a bracket fill until 6c re-derivation")`
(`region.rs:1293`).

Consequences:

- The pass-1 characterization buffer (`Vec<(RegionCharacterization, …)>`) retains
  per-gap fill PCM between the characterize and execute loops — on the Bracket
  path, for no decision reason (all decisions are already on the spec).
- The characterize→execute boundary is muddied: characterize is supposed to
  **decide** (geometry, seams, gain, verdict) and execute is supposed to
  **assemble PCM**. The carry is a transitional cheat (the 6b.3e intent was a
  decision-only handoff).
- `RegionCharacterization` can't collapse to `GapRepairSpec` alone (or
  `Patch(spec) | Skip(spec)`) while the side-channel `Vec` exists.

**Scope note — this does not make pass-1 PCM-free.**
`GapRepairStrategy::SilenceSplice { fill: Vec<f32>, … }`
(`domain/gap_repair_spec.rs:186-188`) carries synthesized PCM **on the spec** by
design (the documented "PCM-ownership asymmetry": Bracket carries indices,
SilenceSplice carries PCM that is not reconstructable from indices). Dual-fit
rescues therefore keep dragging PCM through pass 1 and through preview after
this plan lands. Every retention claim below is **Bracket-path-only**.

## 2. Why it's safe — the parity contract already exists (with one hole)

`execute_bracket_fill` (`region.rs:902`) already reconstructs the fill from
`FillAlignment` + decode buffers + A geometry, and the `debug_assert_eq!` at
`region.rs:1893` asserts it byte-matches the inline `assemble_bracket_fill`.
That shadow is the contract this plan flips to live.

**The hole: the B-extract re-slice is not shadow-covered.** The shadow passes
`execute_bracket_fill` the *already-sliced* `b_samples` that characterize holds.
The executor will only have `b_samples_full` and must re-slice via
`spec.b_extract`. That step is currently unproven, and it has a live mismatch:

- the spec stores `b_extract.end_frame` **unclamped** —
  `(b_extract_end_secs * sample_rate).round()` (`region.rs:2005`);
- characterize's `slice_b_segment` **clamps** `end_frame` to
  `b_samples.len() / channels` (`region.rs:2103`).

Near the end of B, the naive re-slice is longer than characterize's (or out of
range). `b_extension` is `&b_samples[b_fill_end_sample..]` (`region.rs:924`), so
a longer slice changes Gate-mode extension and Fit-mode length fitting — a
byte-parity break in exactly the awkward tail case. **Phase S1 closes this hole
before anything flips.**

**The shadow survives the flip** (stronger than "gate then delete"): characterize
must *still* assemble the fill for its own decisions —

- report-vs-splice seam reconciliation (`fill_splice_seam_correlations_interleaved`, `region.rs:1955`),
- `normalize_gain` from `rms_interleaved(&b_fill)` (`region.rs:1924`),

— so the inline `assemble_bracket_fill` does not disappear. The
`debug_assert_eq!` therefore remains a **permanent** parity guard, not migration
scaffolding.

**The decision outputs are already off the fill.** `seam_pre`/`seam_post`/
`used_splice`/`confidence` and `normalize_gain` are computed in characterize and
stored **on the spec** (`region.rs:1996-2028`); `execute_region_spec` reads them
back via `ExecuteBracketOutputCtx`. `b_fill` is never mutated after assembly. The
*only* thing execute needs the carried `Vec` for is the splice PCM — precisely
what `execute_bracket_fill` reproduces.

## 3. Perf posture: measure, don't assume (M0)

The original draft gated this work on landing the shared mono downmix first, to
pre-pay for a "temporary 2× assembly". That gate is withdrawn: the downmix hoist
is refuted (§0), and the 2× cost was never measured — **no tracing span covers
the fill assembly at all**. `char_gate_search` (93% of characterize) closes at
`region.rs:1676`, well before the assembly at `1873`.

So M0 measures the actual thing. The plausibly-material cost inside
`execute_bracket_fill` is `fit_fill_length_for_gap` (Fit mode, incl. the boundary
grid), *not* the downmix or the border-template rebuild.

- **M0 result < ~1% of wall-clock** → no hoist phase; go straight to S0.
- **M0 result material** → open a hoist phase targeting *what M0 indicts*, with
  its own measurement, before F1. Do not resurrect the downmix hoist without new
  measurement showing the downmix specifically is material.

### 3.1 Measured result (2026-07-24) — immaterial, H? retired

Release profile, real repair path (`--wav`), `CLIP_SYNC_SPAN_TIMING=1`, via
`scripts/measure-repair-perf.ps1`. **Complete 17-pair sweep**, licensed media,
gap-fingerprint corpus pairs (the pair-index → media mapping is deliberately
**not** recorded in-repo; it lives only in the gitignored source map, per the
convention in the archived perf plan).

| Pair | `patch_audio` | `char_fill_assembly` | `exec_fill_assembly` | exec share |
|------|---------------|----------------------|----------------------|-----------|
| 1  | 728 s | 0.004 s (n=1) | 0.006 s (n=1) | 0.0009% |
| 2  | 485 s | 0.031 s (n=1) | 0.041 s (n=2) | 0.0085% |
| 3  | 750 s | 0.629 s (n=2) | 0.598 s (n=4) | 0.0797% |
| 4  | 402 s | 0.025 s (n=4) | 0.050 s (n=6) | 0.0125% |
| 5  | 481 s | 0.891 s (n=7) | 0.880 s (n=8) | 0.1829% |
| 6  | 440 s | 1.474 s (n=6) | 1.553 s (n=6) | **0.3529%** |
| 7  | 522 s | 0.829 s (n=5) | 0.718 s (n=5) | 0.1375% |
| 8  | 399 s | 0.003 s (n=1) | 0.017 s (n=3) | 0.0043% |
| 9  | 775 s | 0.044 s (n=4) | 0.067 s (n=5) | 0.0086% |
| 10 | 846 s | 0.057 s (n=3) | 0.066 s (n=4) | 0.0078% |
| 11 | 345 s | 0.086 s (n=4) | 0.098 s (n=4) | 0.0285% |
| 12 | 693 s | 0.046 s (n=4) | 0.062 s (n=4) | 0.0089% |
| 13 | 534 s | 0.037 s (n=6) | 0.057 s (n=8) | 0.0107% |
| 14 | 195 s | 0.009 s (n=2) | 0.019 s (n=2) | 0.0097% |
| 15 | 334 s | 0.071 s (n=2) | 0.079 s (n=2) | 0.0238% |
| 16 | 462 s | 0.297 s (n=6) | 0.293 s (n=7) | 0.0634% |
| 17 | 824 s | 0.316 s (n=2) | 0.304 s (n=3) | 0.0369% |
| **all** | **9215 s** | **4.85 s (n=60)** | **4.91 s (n=74)** | **0.0533%** |

**Verdict: the F1 re-derivation costs 0.053% of wall-clock across all 17 pairs,
worst pair 0.35%. No hoist. H? is retired, not deferred.** The "2× assembly"
worry that gated the original draft was never a cost. Note the full sweep is
**5× the 7-pair figure** (0.011%) — the early pairs happened to be the cheap
ones, which is exactly why the ledger entry waited for the complete set.

Even the worst pair leaves a 2.8× margin under the 1% bar, and the pessimistic
reading is unavailable: `char_gate_search` is 73.8% of these runs, so there is no
hidden denominator inflating the share.

Three observations worth recording, none actionable:

- **`char_gate_search` is 73.8% here, not the 93.3% of the 2026-07-20 baseline.**
  Not a regression — the lever-1/2 gate optimizations cut the numerator, so decode
  and the rest occupy a larger *relative* share of a much smaller total. The
  harness warns below 50% for exactly this reason (run shape moved ⇒ baseline
  comparison void); 73.8% is comfortably inside the valid band.
- **Call counts differ: 60 char vs 74 exec overall, and exec is never lower.**
  Some brackets reach execute without a characterize-side assembly. Not
  investigated; flagged so a future reader does not read the asymmetry as
  double-assembly. The harness's warning text for this is wrong (it says release
  runs "should match") and has been corrected — see below.
- **The exec > char expectation holds only in aggregate (1.01×), not per pair.**
  Pair 7 came in at 0.87× and pair 5/16 at 0.99×, despite exec doing strictly
  more work (it also wraps the border rebuild and B re-slice). At totals of
  0.02–1.5 s over n≤8 calls, cache warmth and run-to-run variance swamp the
  structural difference — characterize touches these buffers first and pays the
  cold-miss cost that execute then avoids. Do not read per-pair ratios as
  evidence of anything; the aggregate border-rebuild cost is 0.06 s of 9215 s.

**Hazard (from redesign §H2/H3), applicable to any hoist M0 justifies:** a
hoisted shared subexpression must be **precomputed read-only before the
characterize loop**, or memoized with an **order-independent** key — never
lazily populated *during* characterize in gap order.

## 4. What execute needs threaded in

`execute_region_spec` today receives only `sample_rate`. `execute_bracket_fill`'s
`ExecuteBracketFillCtx` (`region.rs:874`) needs, beyond what's already on the spec
(`alignment`, `refined`, `crossfade_secs`, `b_extract`), three distinct classes —
and they are **not** all "config knobs", which the first draft got wrong:

1. **Media (thread a borrow).** `b_samples`, `a_samples`, `a_frames`. Both
   `a_pcm` and `b_samples_full` are already in scope in `mod.rs` at the execute
   loop, so this is `&RegionPatchMedia<'_>` — no new ownership, no re-decode
   (`b_samples` is a borrowed slice of `b_samples_full`, not a per-gap decode).
   Execute re-slices the extract itself via `spec.b_extract` + the S1 helper.
2. **Request/context fields (thread a borrow).** `channels`, `sample_rate`,
   `fill_mode`, `silence_peak_fraction`, `absolute_silence_rms` — read from
   `&PatchAudioRequest` + `&RegionPatchContext`.
3. **Per-gap *derived* values (re-derive, do not thread).** `gap_frames`,
   `correlate_frames`, `seam_gate_frames`, `border_frames`,
   `border_standoff_frames` are computed **inside characterize** from request
   fields *and gap geometry* (`region.rs:1548`, `1560-1608`) — e.g.
   `border_frames = border_frames_from_secs(normalize_window_secs, sample_rate).min(correlate_frames)`,
   where `correlate_frames` depends on `gap_frames`. They are re-derivable from
   `spec.refined` + request, but hand-duplicating those expressions in the
   executor is exactly where byte-parity breaks. **S0 extracts them into one
   shared helper called by both loops.**

Thread (1) and (2) grouped in a ctx struct to avoid a `too_many_arguments`
fight — mirror the `DualFitInputSpec` / `ExecuteBracketFillCtx` pattern already
in the file.

### 4.1 Correction (F1): one derived value is **not** re-derivable

Class (3) above is wrong on one input, discovered while implementing F1. The
window trio (`correlate_frames` / `seam_gate_frames` / `border_frames`) is sized
from the gap length **as it stood before the seam gate ran** (`region.rs:1561`),
and then held fixed. `spec.refined` is the **post**-gate gap. So the executor
cannot reconstruct the windows from `refined`:

* Fit mode: boundary search moves `refined`. The delta *is* recorded, in
  `gap_start_adjust_frames` / `gap_end_adjust_frames`.
* Gate mode: `retry_waveform_seam_extensions` mutates `refined.end_frame` and
  then **re-runs** `evaluate_seam_gate` on the already-extended gap
  (`patch_region.rs`), so the retry's delta is *not* in the adjust fields. Adding
  them back does not recover the pre-gate length.

Fix: the spec carries the pre-gate length explicitly as
`GapRepairStrategy::Bracket { window_gap_frames }`. It is a real decision output
— "the size the fill's windows were cut to" — not a threading shortcut, and it
is the input `FillWindowFrames::for_gap` takes on both sides. The *post*-gate
length (what the fill is assembled **to**) stays underived-from-storage: it is
`refined.end_frame - refined.start_frame`, pinned by a `debug_assert_eq!` in
characterize against the gate's own reported `gap_frames`, so a future gate that
decouples the two fails loudly instead of assembling a differently-sized fill.

## 5. Preview note

The `preview()` / `PatchRunKind::Preview` path and `outcome_from_characterization`
(`region.rs:1239`) already landed (post-split). Preview derives outcomes from the
spec without executing, so killing `bracket_fill` removes the Bracket path's
per-gap PCM from preview. It does **not** make preview PCM-free — SilenceSplice
specs still carry `fill` (§1 scope note). "Scan-only preview" needs that
asymmetry addressed separately; it is out of scope here.

---

## 6. Phases

Each phase: build `--all-targets` → clippy `--all-targets` →
`.\scripts\test-tier.ps1 -Tier pr-repair` → ledger update → commit. Because this
is behavior-adjacent, **also** run the byte-parity gate (the `debug_assert_eq!`
shadow on a debug build + the golden/differential repair fixtures) before each
commit that touches the fill path.

| Phase | Scope | Gate |
|-------|-------|------|
| **M0** | Instrument the fill assembly (span around `assemble_bracket_fill` + `execute_bracket_fill`, Fit vs Gate broken out) and measure on the standing perf corpus. Decides whether any hoist phase exists at all (§3). Instrumentation only — no path change. | Measurement recorded here; spans don't alter output |
| **L0** | Make `assemble_bracket_fill` silent: return `extended_frames: Option<usize>` instead of logging; emit from characterize's call site with identical message/fields (§7). Prerequisite for F1 — a pure primitive can't be called twice while it narrates. | Byte-parity incl. **log output** (same line, same place) |
| **S0** | Extract the per-gap derived-knob computation (`gap_frames`, `correlate_frames`, `seam_gate_frames`, `border_frames`, `border_standoff_frames`) into one helper; call it from characterize in place of the inline expressions. No executor change yet. | Byte-parity (pure refactor; characterize output identical) |
| **S1** | Extract the B-extract re-slice as a helper taking `(b_samples_full, channels, spec.b_extract)` that **replicates `slice_b_segment`'s clamp** (§2). Feed the existing `debug_assert_eq!` shadow through it, so the shadow proves reconstruction from *spec + full buffers* — closing the last un-shadowed step. | Shadow now covers the re-slice; byte-parity incl. end-of-B fixtures |
| **H?** | **Conditional on M0** only. Hoist/dedup whatever M0 indicts, order-independent per §3. Skip entirely if M0 says the assembly is immaterial. | Perf measurement + byte-parity |
| **F1** | Thread media + request/context into `execute_region_spec` (grouped struct); re-derive per-gap knobs via S0's helper and the extract via S1's. Switch the Bracket arm from `bracket_fill.expect(...)` to `execute_bracket_fill(...)`. Still set `Some(b_fill)` on the characterization (no removal yet) so the flip is isolated and shadow-comparable. | Byte-parity; execute output unchanged |
| **F2** | Stop putting `Some(b_fill)` on `RegionCharacterization`; drop the `bracket_fill` param from `execute_region_spec`. Characterize keeps its local `assemble_bracket_fill` (for reconciliation/gain) and discards the `Vec` after building the spec. | Byte-parity; pass-1 buffer no longer retains **Bracket** fill |
| **C1** | Collapse `RegionCharacterization::Patch { spec, bracket_fill }` → `Patch(spec)`; remove `#[allow(clippy::large_enum_variant)]`. Evaluate deleting `RegionCharacterization` entirely in favor of returning `GapRepairSpec` (`Skip` is already a full `Skip`-verdict spec). | build/clippy clean; behavior unchanged |

**Do not** insert an intermediate `struct { spec, bracket_fill }` rename — it's a
stepping stone to nowhere; the field is deleted in C1 regardless.

## 7. Ground rules

- **Behavior byte-parity only.** No gate/threshold/dual-fit/anchor-policy retune,
  no string changes. The `debug_assert_eq!` shadow is the contract; keep it after
  the flip.
- **The one log inside the assembly must move out of it first (phase L0).**
  `assemble_bracket_fill` is a pure PCM primitive that also narrates: the
  Gate-mode `"B bracket shorter than A gap; extended from contiguous B audio (gate)"`
  debug log (`region.rs:861`, fires only when Gate mode + `source_frames <
  gap_frames` + `extend_to > 0`). A pure function that computes *and* reports
  cannot be called twice, so re-deriving the fill in execute would double-emit it
  in release (debug already double-emits via the shadow). Nothing else in the
  assembly tree logs — `fit_fill_length_for_gap`,
  `score_extend_short_fill_to_gap_frames`, and `fit_fill_to_gap_frames` are
  silent; the `unified_*` spans belong to the placement search, which execute
  does not re-run.

  **Fix the layering, don't flag it.** An `emit_logs: bool` parameter would just
  make the wrong owner work twice. Instead have `assemble_bracket_fill` *return*
  the fact (`extended_frames: Option<usize>`) and emit from the caller. That is
  structurally duplication-proof: calling a pure function twice yields the same
  value twice, and only the reporting call site speaks. It also fixes a second
  problem the duplication exposed — after F2 characterize's fill is **discarded**
  (kept only for reconciliation/gain), so a log describing that fill describes a
  throwaway artifact, while the fill actually written is the executor's.

  **Parity-safe form:** move the emission up one frame to characterize's existing
  call site, keeping the message and fields byte-identical. Folding it into the
  per-gap structured reporter (`GapFillResultLog`, `log.rs:41`) is the better
  end state — that struct is the established owner of per-gap fill reporting and
  this log is an orphan that bypassed it — but that changes verbose output, so it
  is a **separate** follow-up, not part of a parity-gated phase.
- **L0 + S0 + S1 before F1.** The flip is only as safe as the re-derivation it
  relies on: the primitive must be side-effect-free (L0) and both derivation
  paths single-sourced (S0) and shadow-covered (S1) first.
- **No refuted hoists.** The mono-downmix hoist stays dead unless new measurement
  says otherwise (§0).
- **Order-independent hoisting** if H? happens: precompute read-only before the
  characterize loop (§H2/H3).
- Stage only phase-relevant files; commit each phase separately with
  `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>`.

## 8. Ledger

| Phase | Status | Commit | Notes |
|-------|--------|--------|-------|
| M0 | Done | `ededf0f` | `char_fill_assembly` + `exec_fill_assembly` spans added. **Measured 2026-07-24, full 17-pair sweep: 0.053% of wall-clock (worst pair 0.35%) — immaterial (§3.1)** |
| L0 | Done | `61fcd78` | `assemble_bracket_fill` returns `BracketFill { pcm, extended_frames }`; caller emits the same line |
| S0 | Done | `7c774f7` | `FillWindowFrames::for_gap` in `geometry.rs`; characterize + `derive_seam_gate_geometry` both call it |
| S1 | Done | `5a00b16` | `slice_b_extract` / `b_extract_frames`; shadow re-slices from the spec's own `BExtractWindow` |
| H? | **Retired** | — | M0 measured 0.053% over 17 pairs (§3.1). Nothing indicted; no hoist will be opened |
| F1 | Done | (this commit) | Executor re-derives the fill. Added `window_gap_frames` to the Bracket verdict — see §4.1. Carry retained as a debug parity check at the handoff |
| F2 | Done | (with C1) | `bracket_fill: None` at the char site; param dropped |
| C1 | Done | (with F2) | `Patch(GapRepairSpec)`; `#[allow(large_enum_variant)]` gone. **Deletion of the type: evaluated, not done** — see below |

**F2 and C1 landed in one commit.** Separating them would have committed a
`field bracket_fill is never read` warning, and CI runs
`cargo clippy --all-targets -- -D warnings` — so the F2-only state is not a
valid commit. Removing the field *is* the completion of F2.

**C1's "evaluate deleting `RegionCharacterization`":** both variants now carry
nothing but a `GapRepairSpec`, so the enum is a restatement of `spec.verdict` —
a second source of truth for a tag that already exists (`skip_outcome_from_spec`
already `unreachable!()`s on the disagreement). Deleting it means
`characterize_region -> (GapRepairSpec, GapTagsPatchContext)` and matching on
`spec.verdict` at the ~5 dispatch sites, all in `region.rs` + `mod.rs`.
Recommended, mechanical, and **out of scope for this plan** — it is a
characterize-boundary cleanup, not `bracket_fill` elimination. Left for the
user to schedule.

## 9. Revision log

**2026-07-24 (audit revision).** Changes from the first draft:

- **Dropped H1/H2 (mono-downmix hoist) as a blocking prerequisite.** It is the
  refuted §2.1 hoist (0.006%, measured 2026-07-20). Replaced by **M0**, which
  measures the fill assembly itself — the cost the "2× assembly" worry was
  actually about, and which no span currently covers.
- **Added S1** — the B-extract re-slice was an unproven reconstruction step with
  a real unclamped-`end_frame` mismatch (`region.rs:2005` vs `2103`). §2 now
  states the hole instead of claiming the shadow already covers everything.
- **Added S0 and rewrote §4** — `border_frames` / `seam_gate_frames` /
  `border_standoff_frames` / `correlate_frames` are per-gap *derived*, not
  request fields; the draft's "thread these knobs" framing invited a
  hand-duplicated derivation, the most likely byte-parity break in the plan.
- **Added L0** — the re-derivation would have double-emitted the one log living
  inside `assemble_bracket_fill`. Rather than suppress the second copy, the
  primitive stops narrating: it returns the fact and the caller reports. The
  duplication was a symptom of a leaf PCM primitive owning diagnostics, and of
  reporting from the pass whose fill is discarded.
- **Corrected §5 and §1** — `bracket_fill` is not the last per-gap PCM in pass 1;
  `SilenceSplice` specs carry `fill` by design. All retention claims are now
  scoped to the Bracket path.
- Refreshed line references to post-split `region.rs`; fixed the commit trailer.

**2026-07-24 (F1).** §4.1 added: the "re-derive, do not thread" rule has one
exception the audit missed — the window trio is sized from the *pre*-gate gap
length, which `spec.refined` does not preserve and the adjust fields cannot
reconstruct in gate mode. The spec now carries `window_gap_frames`. This is a
deviation from §4 as written, deliberate and scoped to that one value.

---

Companions: [archive/TEMP-pipeline-perf-redesign-plan.md](archive/TEMP-pipeline-perf-redesign-plan.md)
(§3 step 8 Hoists / 6b.3 ledger — authoritative history),
[archive/TEMP-production-repair-perf-plan.md](archive/TEMP-production-repair-perf-plan.md)
(§0/§2.1 — the measurement that killed the downmix hoist),
[TEMP-patch-audio-module-split-plan.md](archive/TEMP-patch-audio-module-split-plan.md)
(M-MOD, done), [pipeline.md](../pipeline.md), [gap-fill-modes.md](../gap-fill-modes.md).
