# TEMP — A/B constellation lag histogram

**Status:** draft, 2026-09-06. Experiment, not a Change. Fingerprint path
only. Does **not** touch `domain/seam_locks.rs`, `GapSignature`, or unified
fit.

**Tracked as:** [BACKLOG.md](../../BACKLOG.md) § *Fill placement* → *A/B
constellation fingerprint*.

**Predecessor (do not implement):** the 2026-09-06 dump of full-band 10 ms
RMS local-maxima plus pairwise `Δt` coincidence. That measured envelope
spikes and interval-multiset overlap. It did not measure the question
below. This file replaces it in place; nothing was run.

**Companions:** [archive/energy-signature-plan.md](archive/energy-signature-plan.md)
Phase 4 (peak-picked landmarks as a third `GapSignature` variant — won't-do
as a *production* signature; this plan does not reopen that);
[TEMP-disjoint-offseam-lock-plan.md](TEMP-disjoint-offseam-lock-plan.md)
(same Pearson instrument, disjoint *placement*; complementary, not a
rename — 50/g10 lives there);
[archive/TEMP-phat-seam-scoring-plan.md](archive/TEMP-phat-seam-scoring-plan.md)
(refuted: rescoring the same dense seam window).

**Media hygiene:** pair index + gap index only.

---

## 0. What this tests (normative)

**On remaining-skip shoulders, after an event picker that lands on known
onsets in a fixture, do A and B share enough events that a histogram of
`L = t_B − t_A` has a unique peak, at a decoy rate measured on
time-shuffled peaks (or random B windows), not on gaps Pearson already
solved?**

That is the whole plan. Later sections only implement it. If a later
sentence disagrees with this one, this one wins.

“Event” means a time-localized onset the fixture recovers, not a
full-band RMS local-max. “Share enough” means the **diagonal** of a rigid
shift: votes pile at one `L`, not that two point clouds happen to share
an interval. “Unique” means that peak stands out of **that shoulder’s
own** decoy histogram (shuffled B times, same `n`, same span), not that
some bin is nonempty and not that the same vote recovers
`lag_decision.frac_lag_ms` on already-patched gaps. “Remaining-skip
shoulders” are the population in § 1; they are a class, not a named gap.

This plan does **not** test carrying a lag back to the seam, wiring a
gate, or adding `GapSignature::Constellation`. A matcher TEMP is a later
question, and only if this probe unique-locks on the remaining-skip set
above the measured decoy rate.

**Not the customer.** 50/g10 is [TEMP-disjoint-offseam-lock-plan.md](TEMP-disjoint-offseam-lock-plan.md).
Do not read this dump as a 50/g10 result. If that gap is in the
remaining-skip set, its ~2.8 s consensus vs a 2000 ms haystack is a
**geometry miss** — exclude it from the § 0 verdict (rule 8).

---

## 1. Remaining-skip shoulder (the population)

A **remaining-skip shoulder** is a pre or post of a gap where:

1. Production `disposition` is `skipped`.
2. B is present and the donor interior is occupied (not Program-quiet,
   not a dead aligned donor).
3. The skip is not a reasoned non-placement: not `shoulders_abut` /
   `bridge_frames == 0`, not `equivalence_production` skip, not
   **Decorrelated** (different content).
4. That shoulder’s Pearson seam lock is absent or non-unique
   (`*_seam_r < min_fill_correlation`, or prominence essentially flat).
   Dense Pearson already failed *here*; that is the failure mode.

Patched / `gate_pass` / ear-clean gaps may appear in the dump for audit.
They are **not** the customer and **not** the decoy control. Calibrating
uniqueness so it rediscovers lags Pearson already found is a different
experiment; do not run it as this one.

---

## 2. Design rules (normative)

1. **Fixture before corpus.** The picker is whatever passes § 3. No
   remaining-skip dump is a § 0 result until the fixture is green. If
   the first candidate fails, change the picker; do not run the corpus
   on a picker that tracks ripple.

2. **The picker is an onset picker.** First candidate: high-pass (~2 kHz)
   then 10 ms log-RMS, local-max, prominence = peak minus the median of
   a ~200 ms neighborhood excluding a 30 ms core, min-spacing = picker
   bin, cap 12, **no write floor**. This is not
   `energy_peak_candidates` (3-point neighbor prominence on full-band
   bins) and not `list_anchor_candidates_a` (scan fallback, A-only).
   Do not inherit `gap_signature_bin_ms` (50 ms) — that is the I1
   structure-pattern bin
   ([archive/TEMP-equivalence-instrument-convergence.md](archive/TEMP-equivalence-instrument-convergence.md)).
   Reopen bin / high-pass / spacing only from a failed fixture, never
   from corpus taste.

3. **`L` is a lag, not an interval.** For every kept A event and every
   kept B event on that shoulder, vote
   `L = t_B_seam − t_A_seam` (B’s seam origin is nominal `b_mapped`).
   Bin at `vote_bin_ms = DUALFIT_SEAM_UNIQ_LAG_MS` (30). Restrict to the
   searched haystack. Do **not** histogram `|Δt_A − Δt_B|`. Interval
   equality does not produce `L`.

4. **Unique means beats that shoulder’s shuffle.** `S = 100` trials:
   redraw B times uniformly inside the searched B span, same `n_B`.
   Real unique-locks iff all of: `n_A ≥ 2` and `n_B ≥ 2`; the mode bin
   is strictly interior (not within one `vote_bin` of the histogram
   edge); real mode count **> 95th percentile** of the shuffle mode
   counts. That **is** the decoy measurement. A lock that appears equally
   often on shuffled times is density, not a shared set.

5. **Random B window is a fixture negative, not a second corpus
   instrument.** B drawn from a span with no shared onsets must not
   unique-lock. On the corpus, shuffle (rule 4) is the decoy.

6. **A windows are one-sided and longer than the seam template.** Pre:
   `[gap_start − context, gap_start)`; post:
   `[gap_end, gap_end + context)`; `context = gap_signature_context_secs`
   (default 3.0). The 250 ms seam is the window PHAT already rescored.
   Do not walk A into the hole.

7. **B haystack is the A window, shifted by the recipe reach, around
   nominal `b_mapped`.** `max_lag = lag_envelope_max_lag_ms` (default
   2000). Do **not** use `SEAM_LOCAL_SEARCH_MS` (600): that is the
   Pearson band that already failed on this population. Clamp to
   decoded PCM; stamp the searched A/B spans. A peak on the span edge
   is search-exhausted (`edge_pinned`), not a measurement.

8. **Geometry miss ≠ no shared events.** If the pair’s map error is
   larger than `max_lag_ms`, B’s list cannot contain the match. Stamp
   reach; drop those shoulders from the § 0 verdict. Do not silently
   widen past the recipe in v1.

9. **Dump the test, not a later analysis.** Wire fields are the event
   lists (audit) **and** the histogram / shuffle summary (the lock). No
   `suggested_lag_ms` into a gate. Absence: `None` = unmeasured
   (projector / no B); `Some` with empty `points` = measured, none kept
   (same rule as `anchors`).

---

## 3. Fixture (synthetic, before any corpus run)

Three cases, same picker, same `L` histogram, same uniqueness rule.

- **Known onsets + known lag.** Inject ≥ 3 clicks (or equally sharp
  onsets) at known A times in one shoulder window, on a stationary noise
  bed. B is that same waveform delayed by a known `L` inside `max_lag`.
  The picker must recover the injected times within one picker bin
  (injected onsets survive the cap; bed ripple must not displace them).
  The `L` histogram must unique-lock at the injected lag within one
  `vote_bin`. Time-shuffled B times must **not** unique-lock. A random
  B window with no clicks must **not** unique-lock.

- **A-only onsets.** Same A clicks; B is bed without those clicks.
  Must not unique-lock.

- **Geometry.** Assert A windows do not enter the hole; a true lag
  beyond `max_lag` is `edge_pinned` / no interior mode, not a lock.

If the first picker candidate fails the first case by tracking bed
ripple, stop. Raise the high-pass, the neighborhood, or the bin from
that count; re-run the fixture. Corpus is not a picker lab.

---

## 4. Probe (fingerprint path only)

`--gap-fingerprints` from-decode, B present, every gap that already gets
`lag_decision` / `splice_dualfit`. Independent of
`--fingerprint-diagnostics` (not Tier-3). Production `dual_fit.rs`
literals stay `None`. Schema comments: no gate reads these.

Per shoulder: cut the four windows (A one-sided, B haystack, § 2.6–2.7),
pick, vote `L`, shuffle, stamp § 5. Cost is energy bins over ~3 s A +
~7 s B per shoulder plus 100 tiny histograms — cheap vs `lag_decision`.

---

## 5. Dump (minimum)

Group name `constellation`. Recipe-stamp `bin_ms`, `min_spacing_ms`,
`context_secs`, `max_lag_ms`, `vote_bin_ms`, `shuffle_s`,
`shuffle_percentile`.

Per shoulder, both sides (`pre` / `post` × `a` / `b`):

- `span_start_secs` / `span_end_secs` / `edge_pinned` (any kept point
  within one picker bin of that side’s searched edge)
- `n`, `points[]`: `t_seam_ms`, `time_secs`, `prominence` (picker’s
  neighborhood prominence, not 3-point). Prominence-desc, cap 12.

Per shoulder, the test:

- `n_a`, `n_b`
- `hist_peak_lag_ms`, `hist_peak_votes`, `hist_second_votes` (`null` if
  `n_a < 2` or `n_b < 2`)
- `shuffle_p95_votes`
- `reason`: `locked` / `no_train` / `not_unique` / `edge_pinned` /
  `unmeasured`

`hist_peak_lag_ms` is diagnostic. It is not a production placement.

Old corpora deserialize `constellation: None`. Curated goldens stay
field-absent until an intentional harvest.

---

## 6. Corpus comparison

One `--gap-fingerprints --features calibration,he-aac` run **after**
§ 3 is green. Compare:

- **Remaining-skip shoulders (the test):** unique-lock rate (`reason =
  locked`) vs the fraction of those same shoulders whose shuffle would
  have locked if we had treated the 95th percentile as a real peak
  (should be ~5% by construction; report the realized shuffle lock
  fraction as the decoy rate). A hit is a remaining-skip shoulder that
  unique-locks. Geometry-miss shoulders (rule 8) are a separate count,
  not misses.

- **`no_train` rate** on remaining skips: picker found `< 2` A events.
  Those are not constellation customers — same class of failure as
  Pearson, earlier. Do not treat `contour.*_flatness` 1.0 and `no_train`
  as two discoveries.

- **Sanity, not calibration:** on already-confident Pearson seams
  (`*_seam_r ≥ 0.35`), if a constellation lock exists, record whether
  `hist_peak_lag_ms` agrees within `vote_bin_ms`. Agreement is
  interesting; **disagreement or silence on that set does not refute
  § 0**, and agreement does not confirm it. Do not tune the picker or
  the 95th percentile against this set.

---

## 7. Decision

- **Remaining-skip unique-lock rate is clearly above the shuffle decoy
  rate, and geometry misses are not carrying the count** → § 0 is yes.
  Open a **separate** TEMP for a matcher (how to take `hist_peak_lag_ms`
  to the seam). Do not promote into `seam_locks.rs` or `GapSignature`
  from this dump.

- **Lock rate is at decoy, or the mass is `no_train`** → sparse onset
  sets do not pin remaining-skip shoulders. Archive with a settled row
  that refutes **this histogram on this picker**, not “constellation”
  as a slogan. Do not retry with `energy_peak_candidates` or 50 ms
  structure bins.

- **Fixture never greens** → there is no instrument. Same close-out;
  the settled row is “this onset picker does not recover known onsets
  on a noise bed,” not a corpus verdict.

---

## 8. Done when

The § 3 fixture is green, then § 6 has been read against a real dump,
then either a matcher TEMP exists or this plan is archived with a § A
row that refutes **L-histogram unique-lock on remaining-skip shoulders
at the shuffle decoy** — not “dump peak lists” and not 50/g10.

Production patch bytes unchanged (`--no` extra flags, default repair).

**Not** done when: a production gate reads these lists; `anchors` grow a
B side; `GapSignature` gains a third variant.

---

## 9. Placement in code (checklist — execute in order)

Design is § 0–§ 2. This list is the Change.

- [ ] Domain helper: high-pass → 10 ms bins → neighborhood-prominence
      local-max → spacing → cap. Unit: fixture § 3 case 1 picker half
      (injected onsets recovered; bed ripple does not displace them).
- [ ] `L` histogram + shuffle uniqueness. Unit: fixture § 3 all three
      cases (lock at injected `L`; shuffle and random-B and A-only do
      not lock; out-of-reach lag is not a lock).
- [ ] Fingerprint measure: four windows, pick, vote, stamp § 5.
      Restore-after-project if the projector would drop the group
      (same pattern as `anchors`).
- [ ] Schema + `gap-fingerprint.md` table row + field contract
      (`measures` / `placement` / `window` / `not`: not `anchors`, not
      energy structure Pearson, not Chromaprint, not `Δt` coincidence).
- [ ] One from-decode arm; § 6 read; § 7 decision.
