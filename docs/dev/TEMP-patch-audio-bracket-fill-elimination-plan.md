# `bracket_fill` elimination — plan

Status: **planned** (not started).

Kill the transitional `bracket_fill: Option<Vec<f32>>` carry on
`RegionCharacterization::Patch`, making the characterize→execute handoff
**decision-only** (a `GapRepairSpec`). Execute re-derives the bracket fill PCM
from the spec's `FillAlignment` + the decode buffers via the already-shadowed
`execute_bracket_fill`. This is behavior-**preserving** (byte-parity gated), not
byte-*text*-preserving — it moves where PCM is assembled and changes buffer
lifetimes, so it is **not** part of the M-MOD module split.

---

## 0. Relationship to other plans (read first)

- **Provenance.** This is the breakout of one deferred row from
  [archive/TEMP-pipeline-perf-redesign-plan.md](archive/TEMP-pipeline-perf-redesign-plan.md)
  — specifically §3 step 8 **Hoists**, note **(b)**: "`execute_bracket_fill` goes
  live (spec self-sufficient); transitional `bracket_fill: Option` dropped; the
  temporary 2× fill/border assembly appears *and* is deduped." That doc is the
  authoritative history for steps 6a–6c and the 6b.3 sub-step ledger; do not
  re-open it — track the flip here.
- **Not the module split.** [TEMP-patch-audio-module-split-plan.md](archive/TEMP-patch-audio-module-split-plan.md)
  (M-MOD, P1–P6 **done**) was verbatim relocation with **no** behavior change.
  This plan *does* change behavior-adjacent structure and must be gated on
  byte-parity, so it is deliberately a **separate** work item — do not fold it
  into the split ledger or land it as split cleanup.
- **`large_enum_variant` is a side effect, not the goal.** The `#[allow(clippy::large_enum_variant)]`
  on `RegionCharacterization` (`region.rs`) is driven by the large `Bracket`
  struct-variant *inside* `GapRepairSpec`, which **both** arms carry — not by the
  24-byte `Option<Vec<f32>>`. Dropping `bracket_fill` does not shrink anything; it
  makes `Patch { spec }` and `Skip(spec)` carry an identical payload, so the
  size disparity goes to zero and the `#[allow]` becomes removable. Expect **no**
  size win, only the lint clearing.

---

## 1. Problem

Today the Bracket path assembles the fill PCM during **characterize**
(`characterize_region`, `region.rs` ~L1873) and carries that `Vec<f32>` across
the two loops inside `RegionCharacterization::Patch { spec, bracket_fill }`.
`execute_region_spec` then reads it back via
`bracket_fill.expect("bracket verdict requires a bracket fill until 6c re-derivation")`
(`region.rs` ~L1293).

Consequences:

- The pass-1 characterization buffer (`Vec<(RegionCharacterization, …)>`) retains
  per-gap fill PCM between the characterize and execute loops — often the largest
  payload on the Bracket path — for no decision reason (all decisions are already
  on the spec).
- The characterize→execute boundary is muddied: characterize is supposed to
  **decide** (geometry, seams, gain, verdict) and execute is supposed to
  **assemble PCM**. The carry is a transitional cheat (the 6b.3e intent was a
  decision-only handoff).
- `RegionCharacterization` can't collapse to `GapRepairSpec` alone (or
  `Patch(spec) | Skip(spec)`) while the side-channel `Vec` exists.

## 2. Why it's safe — the parity contract already exists

`execute_bracket_fill` (`region.rs:902`) already reconstructs the fill from
`FillAlignment` + decode buffers + A geometry **independently of characterize**,
and the `debug_assert_eq!` at `region.rs:1893` asserts it byte-matches the inline
`assemble_bracket_fill`. That shadow is the contract this plan flips to live.

**The shadow survives the flip** (stronger than "gate then delete"): characterize
must *still* assemble the fill for its own decisions —

- report-vs-splice seam reconciliation (`fill_splice_seam_correlations_interleaved`, ~L1954),
- `normalize_gain` from `rms_interleaved(&b_fill)` (~L1924),

— so the inline `assemble_bracket_fill` does not disappear. The
`debug_assert_eq!` therefore remains a **permanent** parity guard, not migration
scaffolding. This is exactly the "assemble twice" design that the Hoists step
then dedupes.

**The decision outputs are already off the fill.** `seam_pre`/`seam_post`/
`used_splice`/`confidence` and `normalize_gain` are computed in characterize and
stored **on the spec** (~L1997–2028); `execute_region_spec` reads them back via
`ExecuteBracketOutputCtx`. The *only* thing execute needs the carried `Vec` for
is the splice PCM — precisely what `execute_bracket_fill` reproduces.

## 3. Why it must land Hoists-gated (perf caveat)

This change makes nothing faster **by itself** and adds a temporary **2×
assembly**: characterize builds the fill for reconciliation/gain, execute builds
it again for the splice. Given the standing perf posture (per-bracket score
already dominates wall-clock — per-bracket score × k brackets),
adding a second border/fill assembly to the hot bracket path before the shared
inputs exist is a regression we don't want to eat.

So the real move is: **land the input-sharing Hoists first** (share the per-side
mono downmix — the byte-preservable, exactly-sliceable hoist per §3.1 of the
redesign doc; **not** one border template/RMS grid for all consumers), **then**
flip execute to re-derive so the second assemble is cheap.

**Hazard (from redesign §H2/H3):** any hoisted shared subexpression must be
**precomputed read-only before the characterize loop**, or memoized with an
**order-independent** key — never lazily populated *during* characterize in gap
order.

## 4. What execute needs threaded in

`execute_region_spec` today receives only `sample_rate`. `execute_bracket_fill`'s
`ExecuteBracketFillCtx` (`region.rs:874`) needs, beyond what's already on the spec
(`alignment`, `refined`, `crossfade_secs`):

- **media buffers:** `b_samples`, `a_samples`, `a_frames`
- **channel/geometry:** `channels`, `gap_frames`, `a_start_secs`
- **fill/border policy knobs:** `fill_mode`, `border_frames`,
  `border_standoff_frames`, `silence_peak_fraction`, `absolute_silence_rms`,
  `seam_gate_frames`

Thread these through the executor entry (grouped in a ctx/spec struct to avoid a
`too_many_arguments` fight — mirror the `DualFitInputSpec` / `ExecuteBracketFillCtx`
pattern already in the file). Several are `PatchAudioRequest`/derived-config
fields; prefer reading from a borrowed request + derived struct over a 15-wide
positional list.

## 5. Preview note

The `preview()` / `PatchRunKind::Preview` path and `outcome_from_characterization`
(`region.rs:1239`) already landed (post-split). Preview already derives outcomes
from the spec without executing, so "scan-only preview" is largely realized —
killing `bracket_fill` removes the last per-gap PCM that preview drags around,
finishing that story rather than starting it.

---

## 6. Phases

Each phase: build `--all-targets` → clippy `--all-targets` →
`.\scripts\test-tier.ps1 -Tier pr-repair` → ledger update → commit. Because this
is behavior-adjacent, **also** run the byte-parity gate (the `debug_assert_eq!`
shadow on a debug build + the golden/differential repair fixtures) before each
commit that touches the fill path.

| Phase | Scope | Gate |
|-------|-------|------|
| **H1** | Share the per-side **mono downmix** (precompute read-only before the characterize loop; exactly-sliceable). Measure first — confirm the hoist is worthwhile and byte-preservable. No `bracket_fill` change yet. | Perf measurement + byte-parity; §H2/H3 ordering rule |
| **H2** | Route the second (execute-side) border/fill assembly through the shared downmix so `execute_bracket_fill` is cheap — dedup the "assemble twice" cost. Shadow still live. | Byte-parity (shadow + fixtures) |
| **F1** | Thread media + fill/border policy knobs into `execute_region_spec` (grouped struct). Switch the Bracket arm from `bracket_fill.expect(...)` to `execute_bracket_fill(...)`. Still set `Some(b_fill)` on the characterization (no removal yet) so the flip is isolated and shadow-comparable. | Byte-parity; execute output unchanged |
| **F2** | Stop putting `Some(b_fill)` on `RegionCharacterization`; drop the `bracket_fill` param from `execute_region_spec`. Characterize keeps its local `assemble_bracket_fill` (for reconciliation/gain) and discards the `Vec` after building the spec. | Byte-parity; pass-1 buffer no longer retains fill |
| **C1** | Collapse `RegionCharacterization::Patch { spec, bracket_fill }` → `Patch(spec)`; remove `#[allow(clippy::large_enum_variant)]`. Evaluate deleting `RegionCharacterization` entirely in favor of returning `GapRepairSpec` (`Skip` is already a full `Skip`-verdict spec). | build/clippy clean; behavior unchanged |

**Do not** insert an intermediate `struct { spec, bracket_fill }` rename — it's a
stepping stone to nowhere; the field is deleted in C1 regardless.

## 7. Ground rules

- **Behavior byte-parity only.** No gate/threshold/dual-fit/anchor-policy retune,
  no string changes. The `debug_assert_eq!` shadow is the contract; keep it after
  the flip.
- **Sequence is load-bearing.** H before F: do not flip execute to re-derive
  before the shared downmix exists, or you add a real 2× to the hot path.
- **Order-independent hoisting.** Precompute shared subexpressions read-only
  before the characterize loop (§H2/H3).
- Stage only phase-relevant files; commit each phase separately with
  `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`.

## 8. Ledger

| Phase | Status | Commit | Notes |
|-------|--------|--------|-------|
| H1 | Planned | — | measure mono-downmix hoist first |
| H2 | Planned | — | dedup execute-side assembly |
| F1 | Planned | — | thread inputs; flip to `execute_bracket_fill` (carry still set) |
| F2 | Planned | — | drop the carry + param |
| C1 | Planned | — | collapse enum; remove `#[allow]`; consider deleting the type |

---

Companions: [archive/TEMP-pipeline-perf-redesign-plan.md](archive/TEMP-pipeline-perf-redesign-plan.md)
(§3 step 8 Hoists / 6b.3 ledger — authoritative history),
[TEMP-patch-audio-module-split-plan.md](archive/TEMP-patch-audio-module-split-plan.md)
(M-MOD, done), [pipeline.md](../pipeline.md), [gap-fill-modes.md](../gap-fill-modes.md).
