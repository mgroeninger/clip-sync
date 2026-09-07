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
`L = t_B − t_A` has a unique peak, at a decoy rate measured by replacing
B's point process (in-span redraw or a random B window), not on gaps
Pearson already solved?**

That is the whole plan. Later sections only implement it. If a later
sentence disagrees with this one, this one wins.

“Event” means a time-localized onset the fixture recovers, not a
full-band RMS local-max. “Share enough” means the **diagonal** of a rigid
shift: votes pile at one `L`, not that two point clouds happen to share
an interval. “Unique” means that peak stands out of **that shoulder’s
own** decoy histogram (a new B process, same `n`, same span — rule 4),
not that some bin is nonempty and not that the same vote recovers
`lag_decision.frac_lag_ms` on already-patched gaps. Permuting the
detected B times is **not** that decoy: under all-pairs voting it is a
no-op (same set, same histogram); a rigid circular shift is a no-op for
peak *height* (the histogram translates). “Remaining-skip shoulders”
are the population in § 1; they are a class, not a named gap.

This plan does **not** test carrying a lag back to the seam, wiring a
gate, or adding `GapSignature::Constellation`. A matcher TEMP is a later
question, and only if this probe unique-locks on the remaining-skip set
above the measured decoy rate.

**Not the customer.** 50/g10 is [TEMP-disjoint-offseam-lock-plan.md](TEMP-disjoint-offseam-lock-plan.md).
Do not read this dump as a 50/g10 result. If that gap is in the
remaining-skip set, its ~2.8 s consensus vs a 2000 ms haystack is an
**independently known** geometry miss — exclude it from the § 0 verdict
(rule 8). That exclusion is the documented offset, not this probe’s
`edge_pinned` flag.

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

4. **Unique means beats a new B point process, not a permutation of
   the detected times.** All-pairs `L` depends only on the two sets:
   permuting kept B times leaves the histogram unchanged; shifting them
   all by one offset only translates it. The null has to mint new times.
   `S = 100` trials: draw `n_B` times uniformly inside the searched B
   span, rejecting draws that violate min-spacing (the null is a legal
   picker output). Real unique-locks iff all of: `n_A ≥ 2` and `n_B ≥ 2`;
   the mode bin is strictly interior (not within one `vote_bin` of the
   histogram edge); real mode count **> 95th percentile** of the redraw
   mode counts. That **is** the corpus decoy. A lock that appears equally
   often on redrawn times is density, not a shared set.

5. **Random B window is a fixture negative, not a second corpus
   instrument.** B drawn from a span with no shared onsets must not
   unique-lock. On the corpus, in-span redraw (rule 4) is the decoy.

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

8. **Geometry miss ≠ no shared events, and `edge_pinned` ≠ geometry
   miss.** Drop a shoulder from the § 0 verdict only when **independent**
   evidence says the pair’s map error exceeds `max_lag_ms` (documented
   consensus offset, fill offset vs `b_mapped` — not this probe’s peak
   location). Stamp reach. `edge_pinned` means a kept point or hist mode
   sits within one bin of the searched edge: search-exhausted, its own
   `reason`, a separate § 6 count. Do not infer map error from it, and
   do not let that count “not carry” a § 7 yes. The fixture may use
   `edge_pinned` as the out-of-reach outcome because the injected lag
   *is* ground truth; the corpus does not get that courtesy. Do not
   silently widen past the recipe in v1.

9. **Dump the test, not a later analysis.** Wire fields are the event
   lists (audit) **and** the histogram / redraw summary (the lock). No
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
  `vote_bin`. Redrawn B times (rule 4) must **not** unique-lock. A
  random B window with no clicks must **not** unique-lock.

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
pick, vote `L`, redraw, stamp § 5. Cost is energy bins over ~3 s A +
~7 s B per shoulder plus 100 tiny histograms — cheap vs `lag_decision`.

---

## 5. Dump (minimum)

Group name `constellation`. Recipe-stamp `bin_ms`, `min_spacing_ms`,
`context_secs`, `max_lag_ms`, `vote_bin_ms`, `redraw_s`,
`redraw_percentile`.

Per shoulder, both sides (`pre` / `post` × `a` / `b`):

- `span_start_secs` / `span_end_secs` / `edge_pinned` (any kept point
  within one picker bin of that side’s searched edge)
- `n`, `points[]`: `t_seam_ms`, `time_secs`, `prominence` (picker’s
  neighborhood prominence, not 3-point). Prominence-desc, cap 12.

Per shoulder, the test:

- `n_a`, `n_b`
- `hist_peak_lag_ms`, `hist_peak_votes`, `hist_second_votes` (`null` if
  `n_a < 2` or `n_b < 2`)
- `redraw_p95_votes`
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
  locked`) vs the fraction of those same shoulders whose redraw would
  have locked if we had treated the 95th percentile as a real peak
  (should be ~5% by construction; report the realized redraw lock
  fraction as the decoy rate). A hit is a remaining-skip shoulder that
  unique-locks. Independently known geometry-miss shoulders (rule 8)
  are a separate count, not misses. `edge_pinned` is a third count —
  search-exhausted, not a confirmed map miss.

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

- **Remaining-skip unique-lock rate is clearly above the redraw decoy
  rate, after dropping only independently known geometry misses** →
  § 0 is yes. `edge_pinned` rows do not get dropped as misses and do
  not get counted as locks. Open a **separate** TEMP for a matcher
  (how to take `hist_peak_lag_ms` to the seam). Do not promote into
  `seam_locks.rs` or `GapSignature` from this dump.

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
at the redraw decoy** — not “dump peak lists” and not 50/g10.

Production patch bytes unchanged (`--no` extra flags, default repair).

**Not** done when: a production gate reads these lists; `anchors` grow a
B side; `GapSignature` gains a third variant.

---

## 9. Placement in code (checklist — execute in order)

Design is § 0–§ 2. This list is the Change.

- [x] Domain helper: high-pass → 10 ms bins → neighborhood-prominence
      local-max → spacing → cap. Unit: fixture § 3 case 1 picker half
      (injected onsets recovered; bed ripple does not displace them).
- [x] `L` histogram + redraw uniqueness. Unit: fixture § 3 all three
      cases (lock at injected `L`; redraw and random-B and A-only do
      not lock; out-of-reach lag is not a lock). Also: permuting the
      detected B times does not change `hist_peak_votes`.
- [x] Fingerprint measure: four windows, pick, vote, stamp § 5.
      Restore-after-project if the projector would drop the group
      (same pattern as `anchors`).
- [x] Schema + `gap-fingerprint.md` table row + field contract
      (`measures` / `placement` / `window` / `not`: not `anchors`, not
      energy structure Pearson, not Chromaprint, not `Δt` coincidence).
- [ ] One from-decode arm; § 6 read; § 7 decision.

---

## 10. Item 3 sketch (struct / function shapes, non-normative)

Scratch for the "fingerprint measure" checklist item. Not a commitment —
adjust freely while implementing; § 5 (dump fields) still governs the
wire shape if this disagrees.

### Schema (`application/gap_fingerprint/schema.rs`)

Mirrors `AnchorSet` / `AnchorPoint` (schema.rs:826-840) one-for-one, plus
a per-side test result:

```rust
/// One kept onset from the domain picker, wire-shaped for a constellation side.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConstellationPoint {
    pub t_seam_ms: f64,
    pub time_secs: f64,
    pub prominence: f32,
}

/// One shoulder side's searched span + kept points (§5 "both sides" block).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConstellationSide {
    pub span_start_secs: f64,
    pub span_end_secs: f64,
    pub edge_pinned: bool,
    pub n: usize,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub points: Vec<ConstellationPoint>, // prominence-desc, cap 12
}

/// `reason` (§5, §0 vocabulary). `Unmeasured` covers two distinct domain
/// situations, both "the test didn't run": a shoulder window that doesn't
/// exist to attempt (file-boundary A/B window, never reaches
/// `evaluate_l_histogram`), and `domain::LockReason::RedrawInfeasible`
/// (the null had no legal arrangement — the uniqueness test in rule 4
/// needs a legal redraw to compare against; without one there's no
/// verdict, which is `unmeasured`, not `not_unique`). Every other
/// `LockReason` variant maps 1:1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConstellationReason {
    Locked,
    NoTrain,
    NotUnique,
    EdgePinned,
    Unmeasured,
}

/// One shoulder's A/B sides plus the `L`-histogram test (§5 "the test" block).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConstellationShoulder {
    pub a: ConstellationSide,
    pub b: ConstellationSide,
    pub n_a: usize,
    pub n_b: usize,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub hist_peak_lag_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub hist_peak_votes: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub hist_second_votes: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub redraw_p95_votes: Option<usize>,
    pub reason: ConstellationReason,
}

/// Group name `constellation` (§5). Recipe-stamp fields live once per gap, not per shoulder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConstellationSet {
    pub bin_ms: f64,
    pub min_spacing_ms: f64,
    pub context_secs: f64,
    pub max_lag_ms: u32,
    pub vote_bin_ms: f64,
    pub redraw_trials: usize,
    pub redraw_percentile: f64,
    /// Not `Option`: every gap that reaches this fn gets both shoulders — a
    /// not-attempted shoulder (file-boundary window, undecodable B) is
    /// `ConstellationReason::Unmeasured`, not a missing struct. See §10 resolution
    /// below for why this differs from the outer `Option<ConstellationSet>`.
    pub pre: ConstellationShoulder,
    pub post: ConstellationShoulder,
}
```

`GapFingerprint` gains (schema.rs, alongside `anchors` at line 568):

```rust
#[serde(skip_serializing_if = "Option::is_none", default)]
pub constellation: Option<ConstellationSet>,
```

### Measure glue (new fn, `application/gap_fingerprint/measure/`)

One function, called from `build_gap_fingerprint` next to
`list_anchor_candidates_a` (build.rs), pairwise-only (`inputs.b_haystack:
Some`) and Full-tier-only (matches `lag_decision`'s tier gate,
build.rs:~215):

```rust
fn measure_constellation(
    inputs: &GapInputs<'_>,
    refined: &RefinedGap,     // same value already used for pre_start/post_end
    ch: u16,
    rate: u32,
    cfg: &RepairProfile,      // gap_signature_context_secs, lag_envelope_max_lag_ms
) -> ConstellationSet
```

Reuses, unchanged, from build.rs's existing block (~lines 40-251):
- `context_frames = (cfg.gap_signature_context_secs * rate).round() as usize`
  and the `pre_start`/`post_end` one-sided A bounds pattern.
- `b_mapped_start(a_start_frame)` closure over `b_mapped_frame_in_haystack`
  for the nominal B anchor (rule 7).
- `b_mono` / `b_ch` (already-decoded B haystack).

New in this fn, not reused:
- `b_search_radius_frames = (cfg.lag_envelope_max_lag_ms as f64 / 1000.0
  * rate).round() as usize` — same one-line pattern as the existing
  `search_radius_frames`, different config knob (rule 7 forbids
  `SEAM_LOCAL_SEARCH_MS`).
- Per shoulder (pre/post): slice A window samples, slice B haystack
  window around `b_mapped_start(...) ± b_search_radius_frames` (clamped
  to decoded PCM per rule 7), call `domain::pick_onsets` on each with
  `PickerParams::first_candidate(rate)`, translate window-relative
  `time_secs` → absolute `t_seam_ms`, call `domain::evaluate_l_histogram`
  on the two kept-point sets.

**`no_train` vs `unmeasured` (resolved).** `evaluate_l_histogram` already
handles `n_a < 2 || n_b < 2` internally and returns `NoTrain` —
confirmed at `constellation.rs:316-326`. So it is always correct to call
`pick_onsets` + `evaluate_l_histogram` whenever both windows are
non-degenerate (positive width, in-range of decoded PCM), even if one
comes back with zero onsets; `NoTrain` is a real, always-reachable
domain outcome, not a case the wire layer pre-empts. `pre`/`post` are
therefore plain `ConstellationShoulder` (not `Option`) in the schema
above — every gap that reaches this fn gets both. `Unmeasured` fires
only when a shoulder's window itself doesn't exist to attempt: gap sits
at a file boundary so the pre- or post-context window has zero width,
or (same as `anchors`) B isn't decodable there. Rule: **attempted but
empty → `NoTrain`; not attempted → `Unmeasured`.** The whole-group
`Option<ConstellationSet>` on `GapFingerprint` stays for the coarser
cases (`b_haystack: None` entirely, non-`Full` tier, or pre-overlay
projector dump) — same two-level absence `anchors` already has
(field `None` vs. `Some` with an empty list), just one level deeper.

`domain::LockReason` has a 5th variant, `RedrawInfeasible` (the null
has no legal arrangement — `constellation.rs:188-191`), which §5's wire
list doesn't name. Map it to `ConstellationReason::Unmeasured`: the
uniqueness test in rule 4 requires a legal redraw to compare against,
and when none exists the test didn't run, which is exactly what
`unmeasured` means elsewhere in this dump — it is not the same claim as
`NotUnique` (test ran, lock didn't clear the bar).

**`redraw_seed` (resolved).** No existing per-gap deterministic-seed
convention exists in the codebase (checked `measure/*.rs` — every
`seed: u64` hit is test-only noise generation). Derive one from
`(gap.index, shoulder)` using the same multiplier `Lcg` already uses,
so it stays in-family and needs no new constant:

```rust
fn constellation_seed(gap_index: usize, side_tag: u64) -> u64 {
    (gap_index as u64)
        .wrapping_mul(6364136223846793005)
        .wrapping_add(side_tag * 2 + 1) // side_tag: 0 = pre, 1 = post
}
```

Deterministic per gap/shoulder, reproducible across runs, distinct
pre vs. post — the only requirements rule 4 actually has (it does not
ask for cryptographic independence, just decoupled trials).

### Overlay wiring (`characterize.rs`, mirrors `anchors` at lines ~290-296)

```rust
let constellation = fp.constellation.take();
...
*fp = spec_to_fingerprint_summary(...);
fp.silence = silence;
fp.contour = contour;
fp.anchors = anchors;
fp.constellation = constellation;
```

`project.rs`'s `spec_to_fingerprint_summary` gains `constellation: None,`
next to `anchors: None,` (~line 253), plus a doc-comment mention next to
the existing "silence/contour/anchors are also omitted" line (~line 39-65).
