# TEMP — A/B constellation fingerprint capture

**Status:** draft, 2026-09-06. Experiment, not yet a Change. Diagnostic
**capture** on the fingerprint path: dump sparse energy-peak trains on **both**
A and B. Does **not** touch the production gate (`domain/seam_locks.rs`,
`GapSignature`, unified fit) until a later corpus read says a matcher is worth
designing.

**Tracked as:** [BACKLOG.md](../../BACKLOG.md) § *Fill placement* → *A/B
constellation fingerprint*.

**Companions:** [archive/energy-signature-plan.md](archive/energy-signature-plan.md)
Phase 4 ("peak-picked sparse landmarks as a third `GapSignature` variant" —
won't-do as a *production* signature; this plan does not reopen that);
[TEMP-disjoint-offseam-lock-plan.md](TEMP-disjoint-offseam-lock-plan.md)
(1 s Pearson on A disjoint from the seam template; complementary, not a rename);
[archive/TEMP-phat-seam-scoring-plan.md](archive/TEMP-phat-seam-scoring-plan.md)
(refuted: rescoring the same dense window). Conversation that produced this:
constellation matching is "do A's top-N pre-seam peaks have a matching set on
B," not "would pre's 3rd Pearson-lag-curve peak agree with post's 2nd."

**Media hygiene:** pair index + gap index only.

---

## 0. Problem (normative)

Remaining unfilled / undecidable seams (50/g10 is the named customer; the
capture is corpus-wide) fail *dense* Pearson: every sample in the window
votes, so a stationary noise bed correlates with itself at every lag about as
well as the quiet dialogue does. GCC-PHAT tried a different sparsification
(whiten frequency bins) on that same window and found no peak. Sparse
**time-domain events** — keep the onsets, throw away the bed — were never
dumped on B, so we cannot tell whether a matching peak *set* exists.

What we already dump is not this:

| Existing | What it is | Why it is not an A/B constellation |
|----------|------------|-----------------------------------|
| `anchors` | A-only editorial cut candidates (`energy_peak` / `bool_transition` / **scan fallback**), capped by `max_anchors_per_side` | No B list. Scan fallback is a cut, not a peak. One-per-bin at `gap_signature_bin_ms` (50 ms) |
| Energy structure match | Dense Pearson on **all** gated log-RMS bins | Quiet bins vote |
| `energy_peak_candidates` | Local maxima on A's energy bins, then whole-window Pearson vs B | B peaks are never extracted; the set is never matched |
| Chromaprint | Whole-file constellation | Not a gap seam |

**This Change captures the lists.** Matching A-set to B-set is § 3 analysis
on the JSON, not a scorer in this plan. A production `GapSignature::Constellation`
variant stays out of scope — that is exactly the Phase 4 won't-do. Capture
does not reopen it.

---

## 1. Scope fence

**In**

- `--gap-fingerprints` from-decode path, **B present**, every gap that already
  gets `lag_decision` / `splice_dualfit` (unconditional on `gate_pass` — same
  diagnostic-vs-consultation split as the relocation plan § 1: already-clean
  gaps are the control).
- New wire group `constellation` (name locked below). Independent of
  `--fingerprint-diagnostics` (not Tier-3): remaining skips are studied on
  ordinary full dumps.
- Reuse `energy_bins` + the local-max / prominence walk inside
  `energy_peak_candidates`. Do **not** call `list_anchor_candidates_a` (it
  injects scan fallback and is A-only).
- Recipe-stamp the picker bin, min-spacing, A window, and B haystack reach so
  two arms are comparable.

**Out**

- Production `try_dual_fit` / `nominal_first_seam_locks` / unified fit /
  `GapSignature`.
- A matcher, Hough/RANSAC vote, or a dumped `suggested_lag_ms`. Those need
  the lists first.
- Bool-transition constellations (follow-up if energy lists are empty on
  speech-onset gaps).
- Replacing or widening `anchors`.

Absence rule matches `anchors`: `None` = unmeasured (projector / no B);
`Some` with empty `points` = measured, none kept.

---

## 2. Instrument (normative)

### 2.1 One constellation = a spaced peak list, not a fixed N

The dump does **not** pick "the" 4-point set. It emits, per shoulder, the
peaks that survive the spacing rule, ranked by prominence, capped.

A later reader is allowed — and expected — to take **different cardinalities
from the same list**:

| Reader policy (not a dump gate) | When it is usable |
|--------------------------------|-------------------|
| **2-point core** | Two highest-prominence kept peaks whose interval is ≫ picker bin. One interval is one lag constraint. "Well established" means both prominences clear a bar the *reader* chooses after seeing the corpus distribution |
| **3–N uncertain** | More points, weaker prominences, still kept. Usable only if every consecutive interval (time order) is still ≫ picker bin — the dump already enforced that. Extra points are corroboration, not a second instrument |

"Established" is **not** a dump field and **not** a prominence floor on
write. Real editorial energy anchors read prominence ≤ 0.073
([settled.md](settled.md) § A, `anchor_seam_min_prominence`); a high write
floor would empty the lists we are trying to capture. Stamp per-point
`prominence` / `rms_db` and let analysis choose N and the bar.

Cap: **12** kept points per side per track after spacing (enough for a 4-point
uncertain set plus decoys; not an unbounded onset map).

### 2.2 Picker — do not inherit the 50 ms structure bin

`gap_signature_bin_ms` (50 ms) is the structure-signature bin. Finer bins
there discriminate syllable-scale *patterns*; this job is *event location*.
Inheriting it by proximity is the I1 trap
([archive/TEMP-equivalence-instrument-convergence.md](archive/TEMP-equivalence-instrument-convergence.md)).
The coarse envelope bin (`ENVELOPE_BIN_MS` = 100 ms) is coarser still, and
deliberately so.

A constellation interval is only a usable lag prior if its location error is
smaller than the fine confirm we would eventually hand it to
(`DUALFIT_SEAM_UNIQ_LAG_MS` / relocation stage-3 = ±30 ms). **Lock:
`CONSTELLATION_BIN_MS = 10`.** One new constant on the fingerprint measure
path, not a recipe knob in v1 (stamp it; do not thread a CLI flag until a
second arm needs one).

Reopen the bin only if a corpus read shows 10 ms local-maxima tracking noise
ripples rather than onsets — then raise the bin or raise min-spacing to
2× bin, do not silently fall back to 50 ms.

### 2.3 Min-spacing (dump filter, not reader policy)

Two peaks closer than the picker can separate are **one event**. After
ranking by prominence, keep a peak only if it is at least
`CONSTELLATION_MIN_SPACING_MS` from every already-kept peak.
**Lock: min-spacing = picker bin (10 ms).** Adjacent-bin double peaks: keep
the taller.

This is the "too close together" rule. It is **not** a burst-merge heuristic
(rise and peak of the same 80 ms plosive 25 ms apart *are* distinguishable
at 10 ms — whether a matcher should treat them as one event is a reader
question, answered by looking at interval distributions, not by folding them
in the dump).

Stamp `bin_ms` and `min_spacing_ms` on the group even though they are equal
in v1 — a later arm that raises spacing without changing the bin must be
visible.

Time-order consecutive intervals on the kept set are then all ≥ min-spacing
by construction. Stamp `n` and `min_interval_ms` (time-order, or omit when
`n < 2`) so a reader can see "2 points 480 ms apart" vs "4 points packed at
the floor" without recomputing.

### 2.4 Windows

**A, per shoulder, one-sided (do not walk into the hole):**

- Pre: `[gap_start − context, gap_start)`
- Post: `[gap_end, gap_end + context)`
- `context = gap_signature_context_secs` (default 3.0). The 250 ms seam
  template is too short for a 2–4 point train.

**B haystack, same shoulder, wide enough that a shifted copy is visible:**

- Pre: `[b_mapped_start − context − max_lag, b_mapped_start + max_lag)`
- Post: `[b_mapped_end − max_lag, b_mapped_end + context + max_lag)`
- `max_lag = lag_envelope_max_lag_ms` (default 2000), the recipe's already-stamped
  rescue reach. Do **not** use `SEAM_LOCAL_SEARCH_MS` (600): that is the
  instrument that already failed on the named skips.

Clamp to decoded PCM; stamp the **searched** A/B spans in seconds (same
`lag_lo`/`lag_hi` honesty as `SeamPeak`: a peak at the haystack edge is
search-exhausted, not a measurement).

If a pair's true map error is larger than this reach (pair 50's ~2.8 s
consensus vs a 2000 ms haystack is the worked risk), B's list will not
contain the match. That is a **geometry miss**, recorded by
`b_span` / `max_lag_ms` / a B peak sitting on `edge_pinned`, not a matcher
failure. Do not silently widen past the recipe in v1.

### 2.5 Coordinates

Per point:

| Field | Meaning |
|-------|---------|
| `t_seam_ms` | Time from **this shoulder's seam**. Pre: negative (before the hole). Post: positive (after). Intervals are `Δt_seam` without joining geometry |
| `time_secs` | Absolute A-timeline (A points) or B-timeline (B points), same clock as `geometry.*` / `anchors.*.time_secs` |
| `prominence` | Peak − local median on the constellation bins (same shape as `AnchorCandidate.prominence`) |
| `rms_db` | Gated log-RMS of the peak bin |

A matched pair at the same event has `lag_ms ≈ t_B_seam − t_A_seam` in this
frame (B's seam origin is nominal `b_mapped`). Readers compute that; the dump
does not.

### 2.6 Wire shape (locked)

```json
"constellation": {
  "bin_ms": 10,
  "min_spacing_ms": 10,
  "context_secs": 3.0,
  "max_lag_ms": 2000,
  "pre": { "a": { "...side..." }, "b": { "...side..." } },
  "post": { "a": { "...side..." }, "b": { "...side..." } }
}
```

Each side object:

```json
{
  "span_start_secs": 12.34,
  "span_end_secs": 15.34,
  "edge_pinned": false,
  "n": 3,
  "min_interval_ms": 180.0,
  "points": [
    { "t_seam_ms": -420.0, "time_secs": 12.92, "prominence": 0.08, "rms_db": -28.1 }
  ]
}
```

`points` are prominence-desc (the cap ranking), not time order.
`min_interval_ms` is time-order consecutive spacing on that same kept set
(`null` / omitted when `n < 2`).

`edge_pinned` on a **side** is true when any kept point sits within one picker
bin of that side's searched span edge (the train may continue outside).

Old corpora deserialize `constellation: None`. Curated goldens stay
contract-free / field-absent until an intentional harvest — same rule as
other new groups.

---

## 3. What the first dump answers (analysis, not this Change)

After one from-decode corpus (51-pair / remaining-skip subset is enough;
full 51 is better as the control):

1. **Does A even have a train?** Remaining skips with `pre.a.n < 2` (and the
   same on post) are not constellation customers — same failure as Pearson,
   earlier. Join `contour.*_flatness` as a sanity check; do not treat
   flatness 1.0 and `n = 0` as two discoveries.
2. **2-point vs 4-point mix.** Histogram of `n` and of the 2nd / 4th
   prominence. This is the input to any later "established" bar.
3. **Are intervals usable?** `min_interval_ms` vs `bin_ms`. A 2-point core
   with `min_interval_ms` ≈ bin is a location, not a lag constraint.
4. **Does B carry a copy?** Offline: for each A pair of points, histogram
   `Δt_A` against all B pairs' `Δt_B` within `bin_ms` (interval vote). A
   peak in that histogram at lag `L` that also sits inside `max_lag_ms` is
   the first evidence the idea works. **Do not** put that vote on the wire
   in this Change.
5. **Control:** on `gate_pass` / ear-clean patched gaps, the same vote should
   recover a lag within the existing fine window of `lag_decision.frac_lag_ms`
   / dual-fit locks. Disagreement on a clean gap is a decoy rate, not a
   rescue.

50/g10 specifically: if post `a.n < 2`, sparse matching at the seam is the
wrong tool and relocation stays the live experiment. If `a.n ≥ 2` and B
shows no interval vote, the peaks are A-only (noise onsets) and the idea is
refuted for that gap. If B votes, *then* a matcher plan is in order.

50/g10's own reach caveat (§ 2.4): its ~2.8 s consensus offset sits outside
`max_lag_ms` (2000 ms default), so a null read there is a geometry miss, not
a refutation. Do not let 50/g10 carry the corpus-wide verdict — question 4
is answered on the full remaining-skip set, and 50/g10 is read separately,
subject to that caveat.

---

## 3a. Calibrating the vote (normative)

§ 3 question 4 (does B carry a copy?) is not answerable from an eyeballed
histogram. It needs a vote definition and a pass bar **fixed against the
control population before the target set (remaining skips) is read** — the
control has known-correct answers to grade against; the target does not.
This section is that pre-registration. § 3 stays the question list; this is
how question 4 gets scored instead of eyeballed.

### 3a.1 Split

- **Control:** `gate_pass` / already fine-splice-validated gaps carrying a
  trusted `lag_decision.frac_lag_ms` / dual-fit lag (§ 3 item 5). Ground
  truth already exists here — do not re-derive it.
- **Target:** remaining-skip gaps (undecidable at the seam), including
  50/g10 subject to its reach caveat above.

Calibrate on control only. Read target only after the knobs below are
frozen — tuning after seeing the target set is calibrating on the answer.

### 3a.2 Vote definition

Per gap, per shoulder: for every pair of kept A points, compute `Δt_A`; for
every pair of kept B points, compute `Δt_B`. A **vote** at candidate lag `L`
is a `(Δt_A, Δt_B)` pair with `|Δt_A − Δt_B| ≤ vote_bin_ms` and `L` inside
`max_lag_ms`. A gap **locks** at `L` when its vote count at `L` is
`≥ min_votes`.

### 3a.3 Knobs to fix (grid search on control, before reading target)

| Knob | Candidate range | Anchor / starting point |
|------|------------------|--------------------------|
| `vote_bin_ms` | 10, 20, 30, 50 | `DUALFIT_SEAM_UNIQ_LAG_MS` / relocation stage-3 = ±30 ms (§ 2.2's own downstream precision bar) — start here, don't grid-search blind |
| `min_votes` | 1, 2, 3 | 1 = single-interval core (§ 2.1 "2-point core"); ≥ 2 = corroborated (§ 2.1 "3–N uncertain") |
| prominence floor to enter the vote | 0 (dump floor) through the corpus-observed distribution (§ 3 item 2) | none a priori — read the histogram first, do not guess |

### 3a.4 Metric

For each grid point, over the control set:

- **Hit rate** = fraction of control gaps whose lock `L` is within
  `vote_bin_ms` of that gap's known trusted lag.
- **Decoy rate** = fraction of control gaps that lock at a wrong `L`
  (confident and wrong — not "no lock"). This is the quantity § 3 item 5
  names ("a decoy rate, not a rescue") without computing it.

Plot hit rate vs. decoy rate across the grid. Pick the knee: the highest
hit rate at a decoy rate acceptable for a manual-review-flag use case. That
acceptance is a judgment call to record, not compute away — write down the
chosen operating point and the decoy rate it accepts.

### 3a.5 Bin-size sanity check (before the grid, not after)

Before trusting a full-corpus `CONSTELLATION_BIN_MS = 10` pick, confirm on a
handful of gaps with independently known onset locations (existing
`anchors` A-side cuts, or a synthetic fixture with an injected click at a
known time and lag) that the local-max walk lands on the real onset rather
than splitting it into ripple. If it doesn't, § 2.2's reopen clause fires
**before** § 3a.3's grid runs — a contaminated bin size invalidates every
knob choice downstream of it.

### 3a.6 Lock and freeze

Once § 3a.4 picks a grid point, freeze `vote_bin_ms` / `min_votes` /
prominence floor and stamp them alongside the § 3 target-set read. Do not
re-tune after seeing target-set results.

---

## 4. Risks

| Risk | What to do |
|------|------------|
| **Inheriting `anchors` or 50 ms bins** | New group, new bin constant. `anchors` keep their contract |
| **Prominence floor emptying speech lists** | No write floor (`min_prominence = 0` on this picker). Cap + spacing only |
| **10 ms bins tracking ripple** | § 2.2 reopen: raise bin or 2× spacing from a count, not from taste |
| **B haystack miss (lag > `max_lag_ms`)** | Stamp reach; read `edge_pinned`; do not call a miss a failed match |
| **Walking into the hole** | A windows are one-sided, § 2.4. B may include mapped hole interior — that is donor content, not A's dropout, and is correct |
| **Cost** | Energy bins over ~3 s A + ~7 s B per shoulder, no Pearson. Cheap vs `lag_decision`. No `--fingerprint-diagnostics` tax |
| **Re-litigating GapSignature** | Capture ≠ production signature. Phase 4 won't-do stays |

---

## 5. Placement in code (checklist — execute in order)

Design is § 2. This list is the Change.

- [ ] Domain helper: given `energy_bins` output + origin frame + seam frame +
      `bin_ms` / min-spacing / cap, return the kept peak list. Share the
      local-max + local-median prominence walk with `energy_peak_candidates`;
      do not go through `list_anchor_candidates_a`. Unit: spacing keeps the
      taller of two sub-bin neighbors; `n = 0` on a flat envelope; cap 12.
- [ ] Fingerprint measure: for each gap with B PCM, cut the four windows
      (§ 2.4), pick, stamp § 2.6. `build.rs` / from-decode overlay — same
      restore-after-project pattern as `anchors` if the projector would
      drop it.
- [ ] Schema + `gap-fingerprint.md` table row + field contract (`measures` /
      `placement` / `window` / `not`: not `anchors`, not energy structure
      Pearson, not Chromaprint).
- [ ] Unit: A 2-point train, B copy shifted by a known lag inside `max_lag`,
      dumped `t_seam_ms` differ by that lag within one bin. A-only peaks →
      B `n` not a clone. Projector / no-B → `constellation: None`.
- [ ] One `--gap-fingerprints` arm; § 3 questions 1–3 on remaining skips
      plus the clean control. Matching vote (question 4) is a script on that
      dump, not a second schema Change — run § 3a's grid search on the
      control split, freeze the knobs (§ 3a.6), *then* read the target set.

---

## 6. Done when

1. Full from-decode dumps carry `constellation` on every B-present gap, with
   recipe-stamped `bin_ms` / `min_spacing_ms` / `context_secs` / `max_lag_ms`.
2. Production patch bytes are unchanged (`--no` extra flags, default repair).
3. Remaining skips are classifiable from JSON as `a.n < 2` (no train) vs
   `a.n ≥ 2` (train exists; B match is a follow-up read).
4. Calibration knobs (`vote_bin_ms` / `min_votes` / prominence floor) are
   frozen against the control split per § 3a, with a recorded hit-rate /
   decoy-rate operating point, before the target set is read.
5. Either a matcher TEMP is opened from § 3a's calibrated read of the
   target set, or this plan archives with a settled row: sparse lists exist
   and do not vote on the named skips at the calibrated threshold.

**Not** done when: a production gate reads these lists; `anchors` grow a B
side; `GapSignature` gains a third variant.
