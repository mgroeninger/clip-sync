# TEMP — Disjoint off-seam lock

**Status:** draft, 2026-09-06. Experiment, not a Change. Fingerprint path
only (Tier-3). Does **not** touch `domain/seam_locks.rs`.

**Tracked as:** [BACKLOG.md](../../BACKLOG.md) § *Post-join defects (pairs 31
and 50)* → *50/g10*.

**Predecessor (do not re-run):**
[archive/TEMP-seam-anchor-relocation-plan.md](archive/TEMP-seam-anchor-relocation-plan.md).
That funnel’s first candidate was a **10 s** envelope starting 250 ms from
the seam; 967/968 accepted anchors stopped there. It measured “long window
on the seam, keep lags dual-fit already agrees with.” It did not measure
the question below. [settled.md](settled.md) § A records that verdict.

**Companions:** 50/g10 mechanism
[archive/TEMP-pair50-anchor-seam-outcomes-2026-08-21.md](archive/TEMP-pair50-anchor-seam-outcomes-2026-08-21.md)
Result 4; PHAT (same seam window, different scorer — refuted)
[archive/TEMP-phat-seam-scoring-plan.md](archive/TEMP-phat-seam-scoring-plan.md);
grow-post (same seam, wider template, lag ±600 ms)
[archive/TEMP-dual-fit-grow-post-plan.md](archive/TEMP-dual-fit-grow-post-plan.md).

**Media hygiene:** pair index + gap index only.

---

## 0. What this tests (normative)

**At a 1 s slice of A that production’s seam search does not use, is there
a unique A↔B lag under the same Pearson instrument dual-fit already uses?**

That is the whole plan. Later sections only implement it. If a later
sentence disagrees with this one, this one wins.

“Unique” means a peak that stands out of its own lag curve (r and
prominence), not “the 1 s blob correlates at some lag.” “Does not use”
means the slice is **disjoint** from the A span production already
searches at that seam: dual-fit’s ~250 ms template, and grow-post’s
post-side ladder up to 1000 ms, both **centered on the seam**. Lag reach
stays the dual-fit band (±600 ms) around the **same nominal `b_mapped`**
dual-fit uses. One instrument, two placements.

This plan does **not** test carrying a lag back to the seam, wiring a
gate, or scoring the ambiguous seam window with a different function
(PHAT already did that). Carry-back is a later question, and only if this
probe finds a real off-seam lock at 50/g10. Grow-post has already shown
that a lock *at* g10’s post seam, in this lag band, is not there to
re-find.

**Motivating case.** 50/g10’s post seam is undecidable at the seam
(`post_seam_r` 0.248, `post_seam_prom` 0.0049, `post_seam_z` 5.61). The
claim worth measuring: a few seconds into already-resumed A (post) or
further before the hole (pre) there is structured content that *does*
pin a lag, on this map.

**B map (one deliverable).** Every probe in this plan uses dual-fit’s
nominal `b_mapped`. Pair 50’s ~+2.8 s step is outside ±600 ms of that
map. An empty off-seam result at 50/g10 therefore means “no unique lock
on this map, off-seam,” not “no structure exists at the production fill
offset.” Searching the fill offset is a different experiment; do not
bundle it.

---

## 1. Design rules (normative)

These are the rules the predecessor violated. Each is load-bearing.

1. **Window length is 1.0 s** — grow-post’s largest rung, not larger.
   A 10 s envelope that covers the seam and several seconds past it
   cannot say where the matching energy sat.
2. **The off-seam window is disjoint from production’s A-span.** Post:
   production covers `[seam, seam + 1.0 s]`; the first off-seam window
   starts at `seam + 1.5 s` (1.0 s coverage + 0.5 s margin). Pre:
   production covers `[seam − 1.0 s, seam]`; the first off-seam window
   *ends* at `seam − 1.5 s`. One-sided walks as before (post forward,
   pre backward). No candidate whose window overlaps the production
   span is scored, reported, or treated as off-seam.
3. **On-seam control, same window, same instrument.** Score `A[seam,
   seam + 1.0 s]` (post) / `A[seam − 1.0 s, seam]` (pre) with the same
   Pearson peak + prominence + lag. “Found something outside” means the
   off-seam peak is a real lock **and** the on-seam control is not
   already that lock.
4. **Score every stride.** 250 ms steps; post near-edge from 1.5 s
   through 6.5 s (far edge 2.5–7.5 s). Do not stop at the first
   `r ≥ 0.35`. Dump every candidate (or a fixed per-seam summary of
   all of them — see § 3). Take-first is how a walk disappears.
5. **A lock is r and prominence, compared to the control.**
   `r ≥ min_fill_correlation` (0.35) is necessary and not sufficient.
   Prominence must exceed that seam’s **on-seam 1 s control**
   prominence. If the control already locks (`r ≥ 0.35`), an off-seam
   peak within 10 ms of the control’s lag is the **same feature**; a
   lock at a different lag is a **disagreement**.
6. **Do not use dual-fit agreement as pass/fail.** Record
   `offseam_lag − dualfit_lag` (ms). The predecessor’s 2 ms guard
   made a new lag illegal, so the measurement could only re-confirm
   dual-fit. Agreement is a later production question.
7. **Neighbor clamp on the window.** If the 1 s slice would overlap
   another gap’s `[a_start, a_end)`, skip that stride and stamp
   neighbor-bounded. Dump skipped-stride lags the same as scored ones
   (reason: bounded, not “no feature”).

---

## 2. Probe (fingerprint path only)

Reuse `seam_local_peak` / `lag_correlation_curve_auto` (Pearson, ±600 ms)
for both the on-seam control and every off-seam stride. Do not bring back
the 10 s `wide_envelope_side` path; that was the confound.

Cheap A-only flatness (`energy_envelope_is_flat` on the **1 s** slice)
may skip a stride without touching B. Flat is a scored outcome
(`flat`), not silence in the dump.

Pad must cover `1.5 + 5.0 + 1.0 + 0.6 = 8.1 s` from the seam in the
walk direction. Existing fingerprint pads already exceed that on both
sides at the defaults; name both `characterize.rs` pad sites if either
is short.

Synthetic fixture **before** the corpus run (the predecessor’s §4a
intent, aimed at *this* geometry):

- A unique injected lag whose energy lives only in `[seam, seam + 1 s]`
  must appear on the on-seam control and **not** on any off-seam stride.
- A unique injected lag whose energy lives only in `[seam + 3 s,
  seam + 4 s]` must appear on an off-seam stride at ~3 s and **not** on
  the on-seam control.
- Assert every off-seam window is disjoint from `[seam, seam + 1 s]`
  (post) / `[seam − 1.0 s, seam]` (pre).

Diagnostic-only: new fields on `SpliceDualfit` / `SeamLocalTags`, both
copy sites, production `dual_fit.rs` literal stays `None` with the same
annotation the PHAT / predecessor fields carry. Schema comments: no gate
reads these.

---

## 3. Dump (minimum)

Per seam, not 16 unnamed mirrors. Both sides.

- On-seam control: `r`, `prom`, `lag_ms`.
- Off-seam: for each stride (or a compact array): `distance_ms` (near
  edge, always positive), `r`, `prom`, `lag_ms`, `reason`
  (`scored` / `flat` / `neighbor_bounded` / `out_of_range`).
- Summaries a reader can sort without expanding the array: count of
  strides scored; count of locks per rule 5; near-edge of the nearest
  lock (`None` if none); `lag_ms` of that nearest lock; `lag_ms −
  dualfit_lag` for that lock (`None` if none).

No field is an accept/reject against dual-fit. Dual-fit `pre_seam_r` /
`post_seam_r` / `*_lag_ms` already on the dump are the comparison
columns.

---

## 4. Corpus comparison

One `--gap-fingerprints --features calibration,he-aac` run. Compare:

- **50/g10 post:** is there any disjoint stride that locks under rule 5?
  Distance, r, prom, lag, and Δ vs dual-fit’s post lag. An unlocked
  on-seam control plus a real off-seam lock is the §0 hit. An unlocked
  control and no off-seam lock is a miss on this map.
- **Already-confident seams** (on-seam control `r ≥ 0.35`): do off-seam
  locks, when present, match the control’s lag within 10 ms, or do they
  disagree? Broad disagreement means the probe is not trustworthy even
  if g10 lights up.
- **Neighbor-bounded rate** at 1 s / 1.5 s start (the predecessor’s 10 s
  window made this the dominant miss). If 50/g10 is bounded, the
  spacing is wrong for this idea; that is a result, not a retry with a
  shorter window.

---

## 5. Decision

- **50/g10 post has a disjoint lock, and already-confident seams do not
  disagree** → this question is answered yes. Open a **separate** TEMP
  for carry-back (confirm at the seam, centered on that lag). Do not
  promote into `seam_locks.rs` from this dump.
- **No disjoint lock at 50/g10 post** → §0, on this map, does not help
  that gap. Close this plan as refuted for 50/g10; BACKLOG becomes the
  manual-review flag. Do not “try 10 s” or “start closer to the seam.”
- **Disjoint locks disagree with already-good on-seam controls** →
  probe is not a drop-in even if g10 hits; same close-out as the miss.

## 6. Done when

The disjoint-window fixture in § 2 passes, then § 4 has been read against
a real dump, then either a carry-back TEMP exists or this plan is
archived with a § A row that refutes **off-seam lock on the dual-fit
map at 50/g10** — not “relocation” as a slogan.
