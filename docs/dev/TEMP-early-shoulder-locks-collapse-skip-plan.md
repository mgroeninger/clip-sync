# TEMP — Early shoulder locks on the patch path (collapse → skip, before keep)

**Status:** draft, 2026-08-27. One Change. **Startable.**

**Tracked as:** [BACKLOG.md](../../BACKLOG.md) § *Fill placement* → *Early shoulder locks / collapse skip*.

**Companions:** shipped fingerprint rescue
[gap-fingerprint.md](gap-fingerprint.md) § *`lag_decision`* /
[archive/TEMP-lag-nominal-first-plan.md](archive/TEMP-lag-nominal-first-plan.md);
Hole 2
[gap-vocabulary.md](gap-vocabulary.md) § *Known holes*;
pair-sequence skip classifier remains [settled.md](settled.md) § D (not this Change).

**Media hygiene:** pair index + gap index only.

---

## 0. Architecture (normative)

This section is the plan. Later sections only implement it. If a later sentence
disagrees with this one, this one wins.

**Once, early** (after B extract, **before** `evaluate_seam_gate` and **before**
`try_dual_fit`): run the shipped **nominal-first rescue** — fine ±600 at
nominal; if that shoulder is dead (`peak_r` < 0.5), coarse envelope at the
fixed `--lag-envelope-max-lag-ms` (default 2000, 100 ms bins, interior +
r ≥ 0.70); if usable, another fine ±600 **recentered on that peak**. That
produces `b_pre_seam` / `b_post_seam` and thus `bridge_frames`.

**Keep reads that:** if the two locks abut (`b_post_seam <= b_pre_seam`, same
predicate dual-fit already uses) → **skip**, never search a fill.

**`try_dual_fit` no longer calls `seam_local_peak`.** It takes those locks and
does the rest: `gate_pass`, collapse (same numbers, skip path), step-real,
donor, assemble. If the early scan already collapsed, you never call it.

Cheap search lives in **one** place. Dual-fit stays “accept and build a fill
from locks we already have.”

One sentence: **characterize computes the locks first; keep may skip on
collapse; dual-fit consumes the same locks and does not search.**

---

## 1. Call graph

**Today**

```text
characterize
  build dual_fit_input (buffers only)
  evaluate_seam_gate          → High keep  → Patch(Bracket)   [31/g7 stops here]
                            → skip        → try_dual_fit
                                              seam_local_peak ±600 × 2
                                              gate / collapse / assemble
```

**This Change**

```text
characterize
  extract B
  nominal-first rescue → locks (b_pre_seam, b_post_seam, bridge_frames)
       │
       ├─ abut?  → Skip (new reason). No evaluate_seam_gate. No try_dual_fit.
       │
       ├─ evaluate_seam_gate → keep → Patch(Bracket)
       │
       └─ skip → try_dual_fit(locks)   // no seam_local_peak
                    gate_pass / collapse / step-real / donor / assemble
```

`--no-dual-fit` does **not** skip the early scan. Collapse-before-keep is not a
dual-fit feature.

---

## 2. 31/g7 under this plan

A is silent ~1.12 s; B’s match to pre and B’s match to post **abut**. Fingerprint
nominal-first already reports post at **−1160 ms** when envelope is on.

After this Change: early locks see −38-class pre and −1160 post →
`bridge_frames` ≤ 0 → **skip**. Unified-fit ±10 s never runs. No −977 keep.

---

## 3. Non-goals

- Closing A’s timeline (cut/concatenate after other patches). Skip-not-fill
  only.
- Settled § D pair staircase / `--steps`.
- Lowering `SPLICE_MIN_PEAK_R`.
- Changing plan / `b_has_energy`.
- Re-introducing envelope-first or the widen ladder.

---

## 4. Change shape

1. Lift “nominal-first rescue → two seam positions” to a shared helper the
   fingerprint rescue already implements in spirit (`rescue_shoulder` /
   `lag_pair`). Patch characterize calls it once per fillable region. Do not
   fork a second envelope.
2. `try_dual_fit` takes the locks; remove in-function `seam_local_peak`.
3. New skip reason for abut (not `ProgramQuiet`, not no-placement).
4. Keep path: if abut, return that skip **before** `evaluate_seam_gate`.

---

## 5. Exit

- Unit: abutting locks → skip, no fill-search; in-reach two-lag fillable
  silence-splice still reaches `try_dual_fit` with the same locks it would have
  searched; nominal-recoverable shoulders never consult envelope
  (`NotConsulted`).
- 31/g7 production: skip, not High keep (licensed listen WAVs already on disk
  under the 2026-08-18 post-sweep stem).
- Dual-fit skip-path byte-identical on gaps whose both locks were already
  in-reach at ±600 (search result reused, not re-derived with a different
  window).
- `--no-dual-fit` still collapse-skips 31/g7.
