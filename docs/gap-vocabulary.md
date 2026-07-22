# Gap vocabulary

A gap is a point in a small measurement space — how A's kept content meets B's donor content at the
seam(s). Its **type** is the cell that point falls into; patch/skip is a function of the cell, not a
single conflated score. This doc names the cells we **see and reconcile to an action** — a patch, or a
*reasoned* skip. The cells are exercised by a committed set of per-gap-**type** fixtures — one representative
each, media-free — under `crates/clip-sync-repair/tests/gap_corpus/fingerprints/curated/`
([TEMP-gap-fixture-corpus-plan.md](TEMP-gap-fixture-corpus-plan.md)), which superseded the original derivation
corpus (`re-anchor-dual-fit-on-nominal`, ephemeral and now retired). Every core cell has a fixture, plus a
hand-built synthetic **Decorrelated** and a real **Tail**; **Residual-veto** and the **Unfillable** family are
**not fingerprint-representable** (the dump sets outcome from seam scoring, never from residual gating or
plan/execution failures) and are covered by other tests. Background and derivation:
[TEMP-gap-vocabulary-redesign-plan.md](archive/TEMP-gap-vocabulary-redesign-plan.md).

## Axes (read before the cells)

Each measurement is tagged with the **placement** it's taken at — the same field at a different
placement is a different fact.

| Axis | Question | Placement |
|------|----------|-----------|
| Geometry | Interior gap, or a length-mismatch tail? | — |
| Bracket search | Did anchor/grid search find a lag-0 placement? | gate throat |
| Donor — nominal | Is B silent at the *same program time* (quiet in both)? | nominal `b_mapped`, no lag |
| Donor — aligned | Does B bridge the hole once registered? | aligned bridge |
| Registration | Offset + step between shoulders | gross (1 s) and seam-local (250 ms) — the two can diverge |
| Seam viability | Would a length-reconciled fill pass the unchanged gate? | seam-local |

Registration and shared-source are **descriptive**, not dividers — patched and skipped gaps have
overlapping `|step|` ranges, and every donor on this corpus is same-master. What actually separates
patch from skip is **bracket search** result crossed with **donor** state. Full axis list (A-presence,
envelope, uniqueness as diagnostic, …): [redesign plan §2](archive/TEMP-gap-vocabulary-redesign-plan.md#2-principle--describe-a-gap-by-coordinates-derive-the-decision).

## The cells

The core cells for this corpus — five named interior types, one edge case, plus tail (the three
wider-production cells follow in their own subsection):

**Bracket patch** (n=23, e.g. 1·g6, 5·g3 +72 ms step) — a bracket search found a placement where both
seams pass at lag 0; today's normal path. Includes **7 donor-BROKEN interior patches** (e.g. 1·g1, 1·g2)
where the bracket already passed — donor state is recorded but does not gate a patch that cleared bracket
search. *Old guide: **W1/W2/W3/W4** (`balanced`, `asymmetric_*` — the fine Pearson shape of the winning
bracket).*

**Silence-splice** (n=9, e.g. 2·g1, 7·g3, 7·g4) — bracket search exhausted (no lag-0 placement), but
each shoulder registers cleanly at its *own* lag, the lags disagree by a real step, and B is occupied
across the hole. This is the dual-fit addressable set (`dualfit_target()`); repair = independent
per-seam fit + length reconciliation ([gap-fill-modes.md](gap-fill-modes.md) § Dual-fit rescue (G6)).
*Old guide: throat **W5** skip; after rescue the gap becomes **W7** (`patch_tier=high`) — W7 is a
post-rescue outcome, not a separate gap type.*

**Program-quiet** (n=24, e.g. 1·g4, 1·g19, 6·g2) — B has no content to donate across the hole (silent
at the *same program time*, before any lag search). Can look identical to silence-splice at the seam —
`1·g19` scores 0.998 on both seams yet its donor interior is dead — so donor occupancy, not seam score,
is what tells the two apart; skip is permanent here, not a search-radius problem. *Old guide: **W5**,
un-rescued (stayed `symmetric_weak`/dead zone).* The scan-time equivalence gate detects this **same
disposition earlier** as `shared_silence` (§ Silence-character pre-gate) — a plan-time drop before decode.

**No-placement** (n=5, e.g. 1·g0, 4·g0, 7·g0) — structure/anchor search found no candidate at all;
never reached seam scoring, so there's nothing to name at a finer grain. *Old guide: **W6**
(`structure fail`).*

**Tail** (n=7) — a geometry mismatch (gap longer/shorter than available donor), filtered out before any
of the above axes apply; not part of the matched-gap denominator (P6). *Old guide: not a W-tier — handled
upstream of seam scoring.*

**Bracket-exhausted, gate unmeasured** (n=1, `5·g0`) — bracket search exhausted and donor is continuous,
but seam viability was not measured (`splice_dualfit` absent); not in the dual-fit addressable set. Counted
among the 32 bracket-exhausted skips in §7g, outside the nine silence-splice targets. *In the
characterize→execute pipeline seams are always measured, so this pre-measurement variant disappears — a gap
here resolves to **Decorrelated** (below) once its seams are scored.*

## Wider-production cells (no real member)

These are real dispositions the source classifier emits (`GapPatchSkipReason`) but that no available corpus
contains (**n=0**). They are named because each **reconciles to an action** — a reasoned skip — and wider
production data does produce them. **Decorrelated** has a **hand-built synthetic** curated fixture (donor
occupied, every peak layer collapsed so no lag recovers). **Residual-veto** and the **Unfillable** family are
**not fingerprint-representable** — the dump path sets `outcome.tier` from seam scoring (`any_ok`), never from
residual gating or plan/execution failures, so neither ever appears as a characterized skip; they are covered
by the residual-gate and `GapPatchSkipReason` tests instead (see below).

**Decorrelated** (n=0 on re-anchor) — bracket search exhausted, donor is **occupied**, but the seams do not
recover at *any* lag: B has genuinely *different* content across the hole (not a registration offset). Distinct
from silence-splice (seams recover at their own lag) and from program-quiet (donor is occupied, not silent).
Source: a bare `CorrelationBelowThreshold` skip with no dual-fit rescue. Action: reasoned skip.

**Residual-veto** — the seams **pass** the waveform gate, but least-squares cancellation at
the throat shows B ≠ A (echo / repeat / a similar-but-different source that merely correlates). The residual
gate (G4, and dual-fit's A6 shoulder check) is the reconciliation; a false same-source is *correctly* rejected.
Source: `ResidualHeadroomExceeded`. Action: reasoned skip. Distinct from every seam-correlation cell — it is
the cell the residual confirm exists to catch. **Not fingerprint-representable:** the veto is a downstream
patch-pipeline action, and the dump sets `outcome.tier` from seam scoring only (a residual-vetoed gap's
fingerprint still reads `tier=patch`), so there is no curated fixture. The gate *decision* is tested at
score/region level (`validate_residual_gate` F4 cases, `seam_residual_oracle`); the **end-to-end pipeline
veto** — a gap surfacing `ResidualHeadroomExceeded` as its skip reason — is the optional, still-unproved
**C1b** item (`tests/residual_gate_catalog/README.md`; residual-gate follow-ups in [BACKLOG.md](../BACKLOG.md)).

**Unfillable** (family) — the gap structurally cannot be filled, so the action is a
definite skip with no judgment: B window empty / segment out of range / zero-length (`BExtractFailed`,
`AlignedSegmentOutOfRange`, `ZeroLengthGap`). These fail at plan/execution time and **never get
characterized** — only gate/correlation skips reach a fingerprint — so this family is **not
fingerprint-representable** and has no curated fixture; it is covered by `GapPatchSkipReason` unit tests
instead. **Tail** (above) is the plan-time arm of the same family
(geometry mismatch, `OutsideReferenceCoverage`), filtered before per-gap scoring. Pair-level aborts
(`TrackLayoutMismatch`, `TrackCompatibilityUnavailable`) are **not** cells — they abort the whole pair, with no
per-gap action to reconcile.

## Silence-character pre-gate (scan-time equivalence)

The cells above classify a gap *after* it enters the seam/donor measurement space. The **equivalence gate**
(`domain/gap_equivalence.rs`; `--skip-equivalent-gaps`, on by default) runs earlier — at **scan time**, on two
cheap per-block signals — and answers a *prior* question: **is this scanned silent run even a dropout worth
repairing?** It reads one **new axis** crossed with an existing one:

| Axis | Question | Placement |
|------|----------|-----------|
| A silence character | Did A's signal **die** (gap RMS ≥ `dropout_margin_db` below A's *own* noise floor), or is it room tone **at** the floor? | A gap interior vs A context (250 ms scan blocks) |
| Donor — nominal | Is B occupied at the same program time? | nominal `b_mapped`, no lag (**reuses** the Donor—nominal axis) |

**Cells (`GapEquivalenceClass`, emitted on the scan report + `--gap-fingerprints`):**

- **`repairable_dropout`** — A died ∧ B occupied → **keep**. *Not a skip cell*: the gap proceeds into the
  seam/donor cells above (Bracket-patch / Silence-splice / Program-quiet / …) exactly as it does today.
- **`shared_silence`** — B silent at nominal → **drop**. This is the **plan-time detection of the
  Program-quiet cell** — same disposition, same Donor—nominal read — surfaced *before decode* as
  `GapFillSkipReason::AlreadyMatchesReference` rather than *after characterize* as
  `GapPatchSkipReason::ProgramQuiet`. Both "A dropped out but B is also dead" and "quiet in both" land here.
- **`ambient_quiet`** — B occupied but A is only room tone (not a dropout) → **drop**. A **new cell** with no
  seam/donor counterpart: the scan false-positived an intentional quiet passage as a gap, so there is nothing
  to repair even though B has content. Decided on A's own character, not B's donor state.
- **`not_evaluated`** — the gate is off or a signal is missing → **keep** (no decision made).

Only `shared_silence` and `ambient_quiet` drop (`GapEquivalenceClass::drops()`), and the drop is applied at
plan time **only** when `--skip-equivalent-gaps` is set, at **lowest precedence** — `NotFillable`, coverage,
and track blocks win (§ Unfillable). The classification is always computed and reported (advisory), so a plain
scan (`--json`, no `--mux`/`--wav`) shows it with the flag off.

## Derived readouts (not primitives)

The legacy Pearson tier (`patch_tier` / `seam_shape`, W1–W7 below) and `peak_z`/`prominence` uniqueness
are **projections** of the cells above, not separate facts — useful as operator-facing shorthand and as
diagnostics, but they don't add a new axis. Don't gate new decisions on them; read the cell instead.

---

## Appendix — legacy W-tier reference

For readers coming from [gap-repair-guide.md](gap-repair-guide.md) § Seam patterns. This is a rough
correspondence for orientation, not a lookup table any code consults — the tiers are still computed by
`gap_tags.rs` from `min(pre, post)` exactly as documented there.

| Legacy ID | Old label | Roughly corresponds to |
|-----------|-----------|-------------------------|
| W1 | Balanced good | Bracket patch, high confidence |
| W2 | Balanced marginal | Bracket patch, marginal confidence |
| W3 | Asymmetric marginal | Bracket patch, one seam much weaker (echo/repeat) |
| W4 | Asymmetric dead zone | Throat skip in the dead-zone band (`patch_tier=dead_zone`) — often bracket-exhausted pre-rescue; not a successful bracket patch |
| W5 | Symmetric weak | **Was** "weak content" — on this corpus is actually **silence-splice or program-quiet** (same-master content that failed lag-0 bracket search). The *genuinely* decorrelated/low-quality content W5 was originally assumed to be is the **Decorrelated** cell — real in wider production, absent from re-anchor |
| W6 | Structure fail | No-placement |
| W7 | Bracket-exhausted → dual-fit | Post-rescue **patch tier** on a silence-splice gap (default `dual_fit` on) — not a separate cell |

No legacy W-tier maps to **Residual-veto** or **Unfillable**: the Pearson tiers are computed from `min(pre,
post)` alone, so they cannot see the residual confirm (B correlates but ≠ A) or a structural non-fill — another
way the legacy score conflates cells the D/R axes separate.

The corrected reading of **W5** is the reason this doc exists: `min(pre, post)` at lag 0 can't
distinguish "B doesn't have this content" (program-quiet) from "B has it at a different lag than the
bracket tried" (silence-splice) — two cells with opposite correct actions (skip forever vs. dual-fit
rescue) that collapsed onto one score. A *third* action-distinct cell hides in the same band once wider
data is admitted — **Decorrelated** (B has *different* content, a reasoned skip) — which is why the cell,
not the score, is the primitive.
