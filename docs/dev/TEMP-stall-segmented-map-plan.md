# TEMP — Stall-segmented map

**Status:** implementing, 2026-09-15. Code and synthetic tests are in tree. Remaining
ship-gate: 54/g012 ear-check on licensed media (no `gap-files/` / corpus env in
this workspace).

**Tracked as:** [BACKLOG.md](../../BACKLOG.md) § *Timeline integrity* →
*Stall-segmented map*. Customer: **54/g012** (display `#13`).

**Depends on (do not merge):** [archive/TEMP-b-stall-step-confirmation-plan.md](archive/TEMP-b-stall-step-confirmation-plan.md)
— shipped 2026-09-15 at `stall_min_secs = 2.0`. This plan **consumes**
`shift_secs` + `evidence`; it does not change the detector. § 5.5 census
already gives pair 54 `evidence: shoulder` at `shift* = 65.7 s` (within
±1 s of 65.8). Until a stall reads `shoulder`, Length-only reports are a
no-op for remapping, which is the safety property.

**Companions (do not merge):**

- `fill_offset_mode` interpolated / anchored-retry — slow drift or other-gap
  anchors. A stall is a **step**. Unconstrained pair-54 end clip is already
  +66.47 s; adding the stall on top of interpolated would double-count.
- A6 end-clip override warning
  ([TEMP-end-clip-override-warning-plan.md](TEMP-end-clip-override-warning-plan.md))
  — sees *that* a step exists, not *where*.
- Far-unique interpolate — latched ([settled.md](settled.md) § C); 54/`#13`
  is this map problem, not a locatable lag on a locally-correct map.

**Media hygiene:** pair index + gap index only.

---

## 0. What this ships (normative)

**After a B stall whose shoulders measured a step, every later gap maps to
B at `recommended_offset_secs + Σ shift*` of those stalls, and the
existing ±600 ms seam / dual-fit search runs around that new nominal.**
Dropouts reported only on length (`evidence: length` /
`length_contradicted`) do not move the map. Skip/gate/fill_mode are
unchanged: a post-stall hole that still cannot lock skips honestly.

That is the whole plan. If a later sentence disagrees with this one, this
one wins.

---

## 1. Why the current map misses

A→B is rigid `b = a + offset`. Scan always uses `recommended_offset_secs`
(`scan_gaps.rs`). Patch uses `resolve_gap_offset_secs`. Dual-fit /
`lag_decision` search **±600 ms around that nominal**.

A5 already computes the missing number: `detect_b_stalls` walks B
silences, and on each **reported** stall advances a cumulative offset by
`shift_secs`. Nothing downstream reads it. Pair 54: start +2.06 s, stall
66.6 s at B 6152.7, true post-stall map ≈ +67.9 s. Search cannot reach a
66 s step.

Duration is not good enough as the advance. Pair 54 resumes ~0.8 s early
(`shift* ≈ 65.7`). `2.06 + 66.6 = 68.66` is **0.76 s** off truth —
outside ±600 ms. `2.06 + 65.7 = 67.76` is inside. Step-confirmation rule
7 already stores the measured value on `BStallCandidate.shift_secs` when
`evidence == Shoulder`.

---

## 2. Rule (normative)

**Trusted advance** (pure, next to `attribute_gaps_to_b_stalls` in
`crates/clip-sync-repair/src/domain/b_stall.rs`):

```text
trusted_stall_advance_secs(stalls, gap_a_start) =
    sum of s.shift_secs
    where s.evidence == Shoulder
      and s.a_start_secs <= gap_a_start
```

Same cut as `after_b_stall` (A-gaps do not sit inside a stall’s mapped
span: that span is where A has audio). Do **not** fold in `pre_lag_secs`
(`p*`) — step plan rule 7; `|p*| ≤ 0.5 s` is seam noise. Do **not**
consume `Length` or `LengthContradicted` (chirp A5 fixture is a 20 s
**dropout** reported on length; consuming `Length` would shift later
holes by 20 s). [settled.md](settled.md) § C latches the first
`length_contradicted` advance on real media; this consumer never treats
it as a shift.

**Scan.** Run `detect_b_stalls` **before** the per-gap loop (today it
sits after because the loop consumes `a_runs`; keep a silence-interval
copy). For each run:

`delta = recommended_offset + trusted_stall_advance_secs(reported, run.start)`

Use `delta` for `video_b_*`, `b_has_energy`, and `b_mapped` / donor
registration. Equivalence at the pair offset after a stall is the
collapse the step plan already named; remapping it is a consequence, not
a new classifier.

**Patch.** After `resolve_gap_offset_secs` in `characterize_region` (and
any other caller of that helper that builds `b_mapped`, including the A4
injected-time pre-pass):

```text
if fill_offset_mode == Recommended:
    gap_offset += trusted_stall_advance_secs(report.b_stall.stalls, region.a_start)
```

Do **not** add the advance in interpolated / anchored-retry (v1). Scan is
always recommended-based, so scan JSON `video_b_*` can disagree with an
interpolated extract — same as today, plus the stall step only on the
default path. Piecewise interpolation *inside* stall segments is a later
companion.

Do **not** widen `max_lag_frames` / `LAG_DECISION_MAX_LAG_MS`. Do **not**
add a flag; Shoulder-only is the gate. `limit_fill_to_mapped_region` is
an **A**-overlap test and does not need a B-overlap rewrite; post-stall
B times may sit past the pair-offset overlap (step plan § 2 rule 1) and
extract must still read them if the file has samples.

---

## 3. Output

- **A5 human line:** stop saying later gaps “may be offset … and fail to
  fill” when the stall is `Shoulder`. Those gaps are mapped with `+shift*`
  (e.g. `later gaps (from #13) mapped with +65.8s stall advance`).
  `Length` / `LengthContradicted` keep today’s “may fail” wording — we
  refused to consume them.
- **JSON v1 additive:** optional per-gap `stall_map_advance_secs` (f64,
  skip when 0 / absent). Existing `after_b_stall` stays.
  `scan.gaps[].video_b_*` **values** change for post-Shoulder-stall gaps
  (same keys; changelog line). No new `fill_offset_mode` variant.
- **Docs:** [pipeline.md](../pipeline.md) § 3, [cli-output.md](../cli-output.md)
  A5 row, [json-output.md](../json-output.md) changelog + Gap notes,
  [gap-fill-modes.md](../gap-fill-modes.md) step 1.

A4 is not retargeted. Post-stall gaps measured at the segmented map
should stop looking like a 66 s injected-time island; that is a side
effect of a correct `b_mapped`.

---

## 4. Tests

Domain (pure stalls + gap starts):

1. No stalls / only `Length` / `LengthContradicted` → advance 0.
2. One `Shoulder` 3.0 s → gaps before `a_start` get 0; at/after get 3.0.
3. Two `Shoulder` stalls → later gaps sum both; a `Length` between them
   is omitted from the sum.
4. `p*` present on the candidate does not change the advance.
   `FillOffsetMode::Interpolated` / `AnchoredRetry` do not add it.

Scan integration:

5. Existing `execute_reports_b_stall_and_attributes_later_gap`: 20 s
   zeroed B remains `Length`; the **following** gap’s `video_b_*` stays
   `a + recommended` (no 20 s jump).
6. `execute_confirms_short_b_stall_by_shoulders` plus an A hole after the
   stall → that hole’s `video_b_start = a_start + offset + shift*`
   (±0.2 s).

Patch integration:

7. Same fixture: a post-stall A-silence with matching B audio at the
   segmented map **patches** (or dual-fit r ≥ gate); stripping the stall
   report (pair-offset extract) skips / low boundary r.
8. Covered by domain test 4 (`segmented_fill_offset_secs`).

---

## 5. Ship gate

Not a 51-pair census of new constants (those belong to step
confirmation). This Change is a map consumer:

1. **Pre-check.** Pair 54 stall is `evidence: shoulder` with `shift*`
   within ±1 s of 65.8 — **already true** (2026-09-15 census, 65.7 s).
2. **54/g012** write at default `fill_offset_mode` / `fill_mode`: patches
   **or** honestly skips at the segmented map; ear-check the patch (or
   the skip reason if the donor itself cannot lock).
3. No pair without a `Shoulder` stall changes `video_b_*` or fill
   outcomes (chirp A5 test 5).

---

## 6. Checklist

- [x] Domain helper `trusted_stall_advance_secs` +
      `segmented_fill_offset_secs` + tests 1–4 in
      `crates/clip-sync-repair/src/domain/b_stall.rs`.
- [x] `scan_gaps.rs`: detect stalls before the gap loop; map `video_b_*` /
      energy / equivalence through `delta`; keep stamping
      `GapReport.b_stall`.
- [x] `characterize.rs` (+ A4 pre-pass): add the advance only when
      `FillOffsetMode::Recommended`.
- [x] A5 warning copy; JSON `stall_map_advance_secs`; changelog;
      pipeline / cli-output / gap-fill-modes.
- [x] Integration tests 5–7; regenerate `full_surface_repair.json` if the
      golden’s Shoulder stall now emits `stall_map_advance_secs`.
- [ ] § 5 ship gate (54/g012 ear-check on licensed media); archive this TEMP with a [settled.md](settled.md)
      row; remove the BACKLOG row.

---

## 7. Done when

54/g012 patches (or honestly skips) at the segmented map and is
ear-checked, no non-Shoulder stall moves a gap, and this TEMP is
archived. **Or** § 5.1 parks it because pair 54 never reads `shoulder`.
