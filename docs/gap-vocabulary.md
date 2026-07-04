# Gap vocabulary

A gap is a point in a small measurement space — how A's kept content meets B's donor content at the
seam(s). Its **type** is the cell that point falls into; patch/skip is a function of the cell, not a
single conflated score. This doc names the cells that actually occur in the corpus
(`gap-files/re-anchor-dual-fit-on-nominal`, 62 matched gaps). Background and derivation:
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

That's the whole vocabulary for this corpus — five named interior types, one edge case, plus tail:

**Bracket patch** (n=23, e.g. 1·g6, 5·g3 +72 ms step) — a bracket search found a placement where both
seams pass at lag 0; today's normal path. Includes **7 donor-BROKEN interior patches** (e.g. 1·g1, 1·g2)
where the bracket already passed — donor state is recorded but does not gate a patch that cleared bracket
search. *Old guide: **W1/W2/W3/W4** (`balanced`, `asymmetric_*` — the fine Pearson shape of the winning
bracket).*

**Silence-splice** (n=9, e.g. 2·g1, 7·g3, 7·g4) — bracket search exhausted (no lag-0 placement), but
each shoulder registers cleanly at its *own* lag, the lags disagree by a real step, and B is occupied
across the hole. This is the dual-fit addressable set (`dualfit_target()`); repair = independent
per-seam fit + length reconciliation ([ledger §4](TEMP-seam-repair-status-ledger.md#4-dual-fit-repair--wire-spec-a3-shipped)).
*Old guide: throat **W5** skip; after rescue the gap becomes **W7** (`patch_tier=high`) — W7 is a
post-rescue outcome, not a separate gap type.*

**Program-quiet** (n=24, e.g. 1·g4, 1·g19, 6·g2) — B has no content to donate across the hole (silent
at the *same program time*, before any lag search). Can look identical to silence-splice at the seam —
`1·g19` scores 0.998 on both seams yet its donor interior is dead — so donor occupancy, not seam score,
is what tells the two apart; skip is permanent here, not a search-radius problem. *Old guide: **W5**,
un-rescued (stayed `symmetric_weak`/dead zone).*

**No-placement** (n=5, e.g. 1·g0, 4·g0, 7·g0) — structure/anchor search found no candidate at all;
never reached seam scoring, so there's nothing to name at a finer grain. *Old guide: **W6**
(`structure fail`).*

**Tail** (n=7) — a geometry mismatch (gap longer/shorter than available donor), filtered out before any
of the above axes apply; not part of the matched-gap denominator (P6). *Old guide: not a W-tier — handled
upstream of seam scoring.*

**Bracket-exhausted, gate unmeasured** (n=1, `5·g0`) — bracket search exhausted and donor is continuous,
but seam viability was not measured (`splice_dualfit` absent); not in the dual-fit addressable set. Counted
among the 32 bracket-exhausted skips in §7g, outside the nine silence-splice targets.

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
| W5 | Symmetric weak | **Was** "weak content" — is actually **silence-splice or program-quiet**, i.e. same-master content that failed lag-0 bracket search, not decorrelated/low-quality content |
| W6 | Structure fail | No-placement |
| W7 | Bracket-exhausted → dual-fit | Post-rescue **patch tier** on a silence-splice gap (default `dual_fit` on) — not a separate cell |

The corrected reading of **W5** is the reason this doc exists: `min(pre, post)` at lag 0 can't
distinguish "B doesn't have this content" (program-quiet) from "B has it at a different lag than the
bracket tried" (silence-splice) — two cells with opposite correct actions (skip forever vs. dual-fit
rescue) that collapsed onto one score.
