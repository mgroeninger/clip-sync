# TEMP — Early shoulder locks on the patch path (collapse → skip, before keep)

**Status:** draft, 2026-08-27. One Change. **Startable.**

**Tracked as:** [BACKLOG.md](../../BACKLOG.md) § *Fill placement* → *Early shoulder locks / collapse skip*.

**Companions:** [gap-fingerprint.md](gap-fingerprint.md) § *`lag_decision`* / § *`splice_dualfit`*;
Hole 2 [gap-vocabulary.md](gap-vocabulary.md) § *Known holes*;
shipped rescue ordering
[archive/TEMP-lag-nominal-first-plan.md](archive/TEMP-lag-nominal-first-plan.md);
pair-sequence skip classifier remains [settled.md](settled.md) § D (not this Change).

**Media hygiene:** pair index + gap index only.

---

## 0. Architecture (normative)

This section is the plan. Later sections only implement it. If a later sentence
disagrees with this one, this one wins.

**Once, early** (after B extract, **before** `evaluate_seam_gate` and **before**
`try_dual_fit`): run **nominal-first rescue on the seam-local templates dual-fit
already uses**. Fine ±600 is `seam_local_peak` on `mono(refined ± fill_seam_search_secs)`.
If that shoulder is dead (`peak_r` < 0.5), coarse envelope at the existing
`--lag-envelope-max-lag-ms` (default 2000, 100 ms bins, interior + r ≥ 0.70);
if usable, another `seam_local_peak` ±600 **recentered on that peak**. Envelope
may lazy-fetch a wider A buffer (same as today’s fingerprint rescue); the fine
template stays the seam window. That produces `b_pre_seam` / `b_post_seam` and
thus `bridge_frames`.

**Keep reads that:** if the two locks abut (`b_post_seam <= b_pre_seam`) →
**skip** with reason `shoulders_abut` (payload below), never search a fill.

**`try_dual_fit` no longer calls `seam_local_peak`.** It takes those locks and
does the rest: `gate_pass`, collapse (same numbers, skip path), step-real,
donor, assemble. If the early scan already collapsed, you never call it.

Cheap search lives in **one** place. Dual-fit stays “accept and build a fill
from locks we already have.”

One sentence: **characterize computes the seam locks first; keep may skip on
collapse; dual-fit and `splice_dualfit` consume the same locks and do not
search.**

---

## 1. Sensor (locked)

The early scan is **seam-local**, with the shipped **nominal-first envelope
ordering**. It is not a 1 s `lag_pair` whose `frac_lag_ms` is then treated as a
seam. Dual-fit and `splice_dualfit` place on 250 ms raw templates; a 1 s peak
and a seam peak are different placements (F14). In-reach dual-fit fills stay
byte-identical because the fine window and template are the ones `try_dual_fit`
already searched.

Shoulders are independent around nominal `b_mapped` (production dual-fit today).
Do not add `lag_pair`’s landmark chain to this helper.

Envelope knobs are the existing repair-config fields (`lag_envelope_max_lag_ms`,
`lag_envelope_window_secs`). This Change reads them on the patch path; they
stop being fingerprint-only. Default 2000 / 2.0 is enough for 31/g7’s post
(−1160 ms). `0` disables envelope (fine ±600 only). B extract already includes
`fill_border_search_secs` (default 10 s), so the ±2000 ms haystack is in the
decoded window.

---

## 2. Fingerprint readers

| Field | This Change |
|-------|-------------|
| **`splice_dualfit`** | Calls the same domain helper. Production-mirror: locks, `gate_pass`, `bridge_frames`, step-real inputs. 31/g7 becomes rescued post + `bridge_frames` ≤ 0 ⇒ `dual_fit_rescue: false` (that flag already requires `bridge_frames > 0`). |
| **`lag_decision`** | Stays the 1 s census at `b_mapped`. Calls the **same envelope primitive** once it lives in domain. Not the skip predicate. |
| **`shoulder-sweep`** | Unchanged. Independent check; shares no correlation code with the helper. |

Repair and a fingerprint dump are usually different processes. Each invokes the
helper. Same algorithm, two runs. When both run in one process, stamp
`splice_dualfit` from the locks rather than sweep twice.

---

## 3. Call graph

**Today**

```text
characterize
  B extract
  evaluate_seam_gate  → High keep → Patch(Bracket)   [31/g7 stops here]
                      → skip      → try_dual_fit
                                      seam_local_peak ±600 × 2
                                      gate / collapse / assemble
```

**This Change**

```text
characterize
  B extract
  nominal_first_seam_locks → (b_pre_seam, b_post_seam, bridge_frames)
       │
       ├─ abut?  → Skip { reason: shoulders_abut, cell: SilenceSplice }
       │            No evaluate_seam_gate. No try_dual_fit.
       │
       ├─ evaluate_seam_gate → keep → Patch(Bracket)
       │
       └─ skip → try_dual_fit(locks)   // no seam_local_peak
                    gate_pass / collapse / step-real / donor / assemble
```

`--no-dual-fit` does **not** skip the early scan. Collapse-before-keep is not a
dual-fit feature. Dual-fit input buffers can stay gated on the flag; the lock
helper runs from `a_pcm` + `b_samples` already in characterize.

---

## 4. `shoulders_abut` — skip identity and later A-cut review

Cell: **SilenceSplice** (Hole 2: degenerate splice, zero-length donor bridge).
Not `ProgramQuiet`, not no-placement, not a new cell.

New `GapPatchSkipReason` object:

```json
{
  "shoulders_abut": {
    "pre_lag_ms": -38.0,
    "post_lag_ms": -1160.3,
    "bridge_frames": -48,
    "gap_frames": 53702
  }
}
```

`cell_for_skip_reason` / `reason_admits_cell` / `skip_cell_from_tags`: this
reason is SilenceSplice only (wildcard-free match arms).

Human line (stdout / `-v` / `--gap-listen` manifest) includes the four numbers
so a skip table is the review set:

`shoulders abut (bridge=−48 frames, gap=53702, pre=−38.0 ms, post=−1160.3 ms)`

That is the census key for a **later Change**: given these skips, decide whether
A’s hole is extraneous program (cut A / concatenate PCM across the gap). This
Change does **not** cut A. It must emit a reason you can grep, with the lock
evidence still on the row, so that review does not re-search.

---

## 5. 31/g7 under this plan

A is silent ~1.12 s; B’s match to pre and B’s match to post abut. At the post
seam, ±600 is dead (`peak_r` ~0.12) so envelope consults; the shipped 1 s
census already reports **−1160 ms** with envelope on. Fine retry is the 250 ms
post seam recentered there.

After this Change: early locks → `bridge_frames` ≤ 0 → `shoulders_abut`.
Unified-fit ±10 s never runs. No −977 keep.

---

## 6. Non-goals

- Closing A’s timeline (cut/concatenate). Skip identity only; later Change
  reads `shoulders_abut`.
- Settled § D pair staircase / `--steps`.
- Lowering `SPLICE_MIN_PEAK_R`.
- Changing plan / `b_has_energy`.
- Envelope-first, the widen ladder, or dense `seam_local_peak` over ±2000.
- Using 1 s `lag_pair` positions as dual-fit locks.
- New CLI. Envelope knobs already exist.

---

## 7. Change shape

1. Move the coarse envelope (`envelope_peak_at` / recenter guards) to
   `domain` so fingerprint `lag_pair` and the seam helper share one
   implementation.
2. Domain helper `nominal_first_seam_locks`: per shoulder, `seam_local_peak` at
   nominal → if `peak_r` < 0.5 and envelope on, coarse envelope (lazy wider A)
   → `seam_local_peak` recentered → keep retry only if recoverable (`peak_r`
   ≥ 0.5), else honest nominal. Returns pre/post `(r, lag_frames, z)` plus
   `b_pre_seam` / `b_post_seam` / `bridge_frames`. `None` only when a seam
   window is out of range (same as today’s `seam_local_peak` `None`).
3. `try_dual_fit` takes those locks; delete its in-function `seam_local_peak`.
   Collapse check stays as a guard (caller should not invoke when abut).
4. `characterize_region`: after B extract, call the helper. Abut →
   `shoulders_abut` skip **before** `evaluate_seam_gate`. Otherwise pass locks
   into `skip_or_dual_fit` → `try_dual_fit`.
5. `splice_dualfit_at` calls the helper instead of two bare `seam_local_peak`s.
6. Wire `GapPatchSkipReason::ShouldersAbut { … }`, formatters, JSON contract
   ([json-output.md](../json-output.md), [gap-repair-guide.md](../gap-repair-guide.md)),
   Hole 2 detection sentence in [gap-vocabulary.md](gap-vocabulary.md).

---

## 8. Checklist

- [ ] Domain envelope primitive + `nominal_first_seam_locks`; fingerprint
      `rescue_shoulder` uses the primitive (no second envelope).
- [ ] `try_dual_fit` consumes locks; unit tests: abut never assembles; in-reach
      two-lag fixture matches today’s fill bytes; `peak_r` ≥ 0.5 never consults
      envelope (`NotConsulted` / no envelope call).
- [ ] `characterize_region` skip-before-keep; `--no-dual-fit` still abut-skips.
- [ ] `splice_dualfit_at` + `dual_fit_rescue_flag` (already declines
      `bridge_frames <= 0`) on a rescued-but-abut fixture.
- [ ] `ShouldersAbut` reason: serde, `cell_for_skip_reason` exhaustive tests,
      human format, JSON + repair-guide tables, vocab Hole 2 “detection” line.
- [ ] 31/g7 production: `shoulders_abut`, not High keep (licensed listen WAVs
      already on disk, 2026-08-18 post-sweep stem).

---

## 9. Exit

- Abutting locks → `shoulders_abut` skip, no fill-search; JSON payload carries
  lags + `bridge_frames` + `gap_frames`.
- In-reach two-lag fillable silence-splice still reaches `try_dual_fit` with
  the same seam locks it would have searched; fill bytes unchanged.
- Nominal-recoverable shoulders never consult envelope.
- `splice_dualfit` on an abutting gap reports the rescued locks and
  `dual_fit_rescue: false`.
- `--no-dual-fit` still collapse-skips 31/g7.
- `--lag-envelope-max-lag-ms 0` → 31/g7 post stays dead at ±600 → no abut skip
  from this path (keep may still High, same as today).
