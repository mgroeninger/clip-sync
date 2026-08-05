# TEMP — Head/tail exclusion for donor Apply

**Status:** draft plan, 2026-08-05 (revised after implementation review). Implements the
**Head/tail exclusion for donor Apply** row in [BACKLOG.md](../../BACKLOG.md) § *Donor registration
leftovers*.

**Source:** [TEMP-equivalence-band/07-corpus-rates.md](TEMP-equivalence-band/07-corpus-rates.md)
§6.10.3 (clipped regs are head/tail; prefer explicit edge check over a `bins` floor) and
[TEMP-equivalence-band/08-production-recommendations.md](TEMP-equivalence-band/08-production-recommendations.md)
§7.4a (shipped Apply without the exclusion).

**Deliverable:** when `apply_donor_registration` is on, gaps whose **A silent core touches the scanned
A extent** classify at the **nominal** map (Observe semantics) while still recording registration.
Mid-extent gaps keep Apply unchanged.

**Media hygiene:** unchanged. No filenames, titles, or paths; corpus pairs by index only.

---

## 0. Problem

Under `DonorRegistrationMode::Apply`, registration that cannot fit both ±`EQUIVALENCE_CONTEXT_SECS`
shoulders is single-sided. On the 39-pair scan (§6.10.3):

- all **31** clipped registrations (`bins == 20`) were head or tail of their pair — **none** mid-media;
- clipped regs abstain at roughly **2×** the mid-media rate (9.7 % vs 4.1 %), while median `peak_r`
  is *higher* (bimodal: very good or fail outright);
- abstain ⇒ `NotEvaluated` / `donor_registration_unreliable` ⇒ **keep** (fail open) ⇒ extra patch
  attempts, never holes.

§7.4a shipped Apply without pairing this exclusion. Cost of leaving it out is noise, not defects.
A `bins` floor would be equivalent on that corpus but worse as a rule (it reads a registration
outcome instead of the media geometry that causes the clip).

### 0.1 What §6.10.3 / the backlog said — and what this plan changes

The backlog direction preferred **gap index 0 / n−1**. That was a corpus proxy for “edge of media”,
not the load-bearing condition. Index fails when:

- the first silence run does **not** start at media head (index 0, not an edge);
- an edge-touching run is not first/last in the scanned list (rare with contiguous A scan, but the
  wrong abstraction).

`--only-gaps` / `--skip-gaps` are **plan-time** selectors; they do not reorder or subset scan
classification. Index is still the wrong production predicate — geometry is.

**This plan uses A-span geometry instead.** Same intent as §6.10.3; better predicate.

### 0.2 What this does *not* change

- Apply’s abstain contract mid-media: `peak_r < min_envelope_r` ⇒ `NotEvaluated`, **never** a
  fallback to the nominal window.
- No new gap class, no `min_envelope_r` retune, no `bins` floor.
- `--no-apply-donor-registration` still forces Observe everywhere.
- Leaving the feature out remains safe; this plan is optional noise reduction.
- No new JSON / provenance field for “edge forced Observe” (v1). Scripts that need the split
  re-derive the predicate from `a_span_secs` + scanned A extent.

### 0.3 Policy tradeoff — Observe for *all* edge cores

The override is **mode = Observe whenever the core touches the A extent**, not “Observe only when
Apply would abstain.”

| Effect on edge gaps | Notes |
|---------------------|--------|
| Removes clipped abstain noise | The cited win: 3 of 31 clipped regs abstained (§6.10.3). |
| Also downgrades trusted clipped regs | The other ~28 had *higher* median `peak_r` (0.995). Apply would have classified at the registered lag; this plan classifies at nominal instead. |

That second row can suppress beneficial Apply flips on edges, not only fail-open keeps. §6.10.4 does
**not** break down how many of the 16 Apply flips were head/tail. The three listened flips (12/8,
14/20, 38/4) look mid-list, which is encouraging but not a proof for the full keep→drop set.

**Decision for v1:** accept losing edge Apply flips for a simple geometry rule. Do **not** implement
“Apply, then fall back to nominal on abstain” (violates Apply’s abstain contract — §1.3). Optional
pre-ship check: if the 39-pair dump is still to hand, count `flips ∩ edge-touch`; not a DoD gate.

---

## 1. Design

### 1.1 Edge predicate (A timeline only)

A gap is an **Apply edge** when its silent core touches the scanned A extent:

| Edge | Predicate |
|------|-----------|
| Head | `core_start ≤ a_extent.0 + ε` |
| Tail | `core_end ≥ a_extent.1 − ε` |

Where (frozen — no “materially after 0” ambiguity):

- **`core_*`** — the block-confirmed silent core (`SilentRun::core_*`) already passed into
  `derive_gap_equivalence` and stored as `GapEquivalenceVerdict::a_span_secs`. **Not** the refined
  `Gap` A bounds (`video_a_start/end`), which sub-block edge refine can widen; operators must not
  eyeball gap start/end for this rule.
- **`a_extent`** — `(a_levels.first().start_secs, a_levels.last().end_secs)`. Always the level-stream
  span, including when the first block starts after timeline 0. **Not**
  `GapReport::b_scanned_end_secs` (wrong timeline).
- **`ε`** — width of the **first** `a_levels` block (`end_secs − start_secs`, typically ~0.1 s). Do
  not use the last block (it can be short). Fallback: recipe `scan_block_ms` if levels are somehow
  empty of usable width. If `a_levels` is empty, treat as **non-edge** (no Apply decision to
  special-case; the gate already fails closed on missing signal).
- **Truncated A** — if the A scan ends early, `a_extent.1` is the last fed block end. A core at that
  tip counts as tail even when the container is longer. That matches clipping: context cannot fit
  past scanned material.

```rust
fn a_span_touches_media_edge(
    core_start_secs: f64,
    core_end_secs: f64,
    a_extent: (f64, f64),
    eps: f64,
) -> bool {
    core_start_secs <= a_extent.0 + eps || core_end_secs >= a_extent.1 - eps
}
```

Pure domain helper next to registration / equivalence (no I/O). Unit-testable without a scan.
Call site computes `a_extent` / `ε` once from `a_levels`, then per-gap `mode` from `run.core_*`.

### 1.2 Why not context-fit (yet)

The mechanical cause of clipping is “context cannot fit”:

`core_start − EQUIVALENCE_CONTEXT_SECS < a_extent.0`
or `core_end + EQUIVALENCE_CONTEXT_SECS > a_extent.1`.

The 39-pair clipped set was bimodal at 20 vs 40 bins only (a full shoulder gone), so **edge-touch ≈
context-clip** on that evidence. Edge-touch is the smaller policy and matches the backlog’s
“explicit head/tail check”.

**Known residual:** cores in `(ε, EQUIVALENCE_CONTEXT_SECS)` from an extent bound can still lose part
of a shoulder and stay on Apply. That band produced no `bins ∈ (20, 40)` on the 39-pair scan; treat
as accepted v1 residual. Widen to context-fit only if later media shows partial clips worth
excluding; do not ship both.

### 1.3 Mode override — Observe classification, Apply recording

For each gap at the scan call site (`scan_gaps.rs`):

```text
mode = if request.apply_donor_registration && !a_span_touches_media_edge(...) {
    Apply
} else {
    Observe
}
```

Still construct `DonorRegistrationParams { mode, ..Default::default() }` and pass
`donor_registration: Some(...)`. Registration is **always computed and emitted** when levels allow;
only `mode` changes. That is exactly Observe’s contract: provenance on the verdict, classification
at the nominal map, no `peak_r` abstain path.

**Do not** invent a third `DonorRegistrationMode`. Edge gaps under production Apply behave like
Observe for classification.

**Do not** implement this as “Apply, then if abstain re-measure at nominal”. That would violate
Apply’s documented abstain contract (§7.4a) and re-use the window already known to be wrong on
non-edge gaps if the branch were ever widened.

### 1.4 Where the decision lives

| Layer | Change |
|-------|--------|
| `domain/gap_equivalence.rs` (or small sibling) | `a_span_touches_media_edge` + unit tests |
| `application/scan_gaps.rs` | Per-gap `mode` from the helper; drop the “§6.10.3 not implemented” comment |
| Config / CLI | **No new flag for v1.** Optional later escape hatch only if operators need Apply on edges |
| Docs | [gap-scan.md](../gap-scan.md), [pipeline.md](../pipeline.md) when shipped |

Params are today shared across the gap loop; make them **per gap** (cheap `Copy` struct). The shared
`GapEquivalenceParams` shell can stay; only `donor_registration.mode` varies. Production wiring is
small; the harder piece is the scan-pin fixture (§2).

### 1.5 Decision neutrality mid-media

Any gap whose core does **not** touch the A extent keeps today’s Apply path bit-for-bit relative to
the existing registration-flip fixture. Edge gaps under Apply become class-identical to the same
gaps under `--no-apply-donor-registration`, with `donor_registration` still present when envelopes
exist.

---

## 2. Tests

Split so a multi-hole harness is **not** on the critical path. `SessionKind::Program` today supports
a single hole; extending it is optional polish, not DoD.

1. **Helper unit tests** (required) — cores at `(extent.0, extent.0 + g)`,
   `(extent.1 − g, extent.1)`, and a mid span; `ε` boundary cases (just inside / just outside);
   empty-levels → non-edge if exercised at the call-site wrapper.
2. **Scan pin — mid Apply unchanged** (required) — existing registration-flip fixture with
   `apply_donor_registration: true` still Apply-flips (bit-for-bit vs today).
3. **Scan pin — edge under Apply ≡ Observe class** (required) — synthetic A with a **head-only**
   (and/or tail-only) hole is enough. With `apply_donor_registration: true`:
   - edge gap class matches the same fixture under `--no-apply…` / Observe;
   - `donor_registration` still `Some` when envelopes exist.
4. **Combined head + mid in one scan** (optional) — only if `Program` (or equivalent) gains
   multi-hole / leading-silence support. Not required to ship.
5. **Index is not the predicate** (optional) — unit-level on the helper: a mid-extent core must not
   count as edge even if it would be “gap index 0” in some filtered list.

No corpus re-scan required for DoD; §6.10.3 is evidence, not a gate. Optional flips∩edge count
(§0.3) is informational only.

---

## 3. Docs when shipped

- [gap-scan.md](../gap-scan.md) — `apply_donor_registration` row: edge **cores** classify at nominal;
  registration still recorded; predicate is A-extent geometry (not index, not `bins`); note core vs
  refined gap bounds.
- [pipeline.md](../pipeline.md) — replace the “Not shipped with Apply” note with the shipped rule.
- BACKLOG row → done / remove; archive this TEMP.

---

## 4. Checklist

1. [ ] Add `a_span_touches_media_edge` (+ rustdoc citing §6.10.3 / this plan).
2. [ ] Wire per-gap `DonorRegistrationMode` in `scan_gaps.rs` from `run.core_*` +
   `(first.start, last.end)` / first-block `ε`.
3. [ ] Helper unit tests; mid Apply-flip regression; edge Observe-class scan pin (head- or
   tail-only fixture).
4. [ ] Update `gap-scan.md` / `pipeline.md`; clear BACKLOG row; archive this plan.

---

## 5. Explicitly out of scope

- `bins`-floor abstain / exclude rule.
- Gap-index 0 / n−1 as the production predicate.
- Context-fit widening (`± EQUIVALENCE_CONTEXT_SECS`) unless edge-touch proves too narrow on new media.
- New CLI flag (v1).
- New verdict / JSON field marking edge-forced Observe (v1).
- Changing Apply abstain → nominal fallback.
- Multi-hole test harness (optional; §2.4).
- `interior_delta` recall widener (killed / demoted; separate from this exclusion).
