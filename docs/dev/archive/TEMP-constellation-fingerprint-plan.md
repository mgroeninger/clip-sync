# TEMP — A/B constellation lag histogram

> **Archived 2026-09-08. Not pursued to a verdict — superseded by a better-targeted
> successor.** §11's own corpus read is why: of the 51-pair corpus's remaining
> patchable-but-skipped population, all but one gap trace back to pair 31 (gaps 7–8,
> the known −1.15 s map step) or pair 50 (gaps 9–17, a second cluster of 273.8–1040.3 ms
> pre/post lag disagreements, likely a second map-step-adjacent pair). Both are
> step-shaped **injected-time** regions, not a lag any better matcher — constellation
> included — could locate, so the lever this plan was building does not aim at the
> actual failure mode. **Not refuted**: the mechanism itself was never shown wrong, and
> the code (fixture-tested, §9 items 1–4) stays in the tree gated behind
> `--fingerprint-constellation` (off by default) rather than deleted — see
> [../gap-fingerprint.md](../gap-fingerprint.md) § *Constellation fingerprint*.
> Successor: [TEMP-injected-time-detector-plan.md](TEMP-injected-time-detector-plan.md)
> (shipped 2026-09-08 as A4). See [../settled.md](../settled.md) § A.

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

---

## 11. Candidate follow-ups from the § 6 read (not decided, not started)

Found while reading the live corpus dump against `lag_decision`'s trusted
waveform lag. Full-corpus figures below (51 pairs, 2026-09-08; pairs 6
and 16 correctly absent from every corpus run — not a stall) — **906/1164
(77.8%)** of comparable locked shoulders land within 60 ms of the
trusted anchor; the residual 258 (22.2%) is a real mechanism, not the
coordinate bug already fixed upstream of this read (partial 14-pair read
was 72%/28%; the full read doesn't reverse it, only sharpens it).
Recorded here so a later TEMP can pick these up without re-deriving
them. Rule 8 already forbids widening past the recipe *in v1* — this
section is exactly that "later," not a retraction of rule 8.

**Scope caveat, applies to every number below:** these are computed
across *every* constellation shoulder in the dump, not restricted to
§1's "remaining-skip shoulder" population (disposition `skipped`,
Pearson lock absent/non-unique, not a reasoned non-placement). §5
itself warns that patched/`gate_pass` gaps "are not the customer and
not the decoy control." A real §7 verdict needs that filter applied
first — not yet done, so checklist item 5 stays unchecked and no §7
call is made here.

**Status of everything below: data-only analysis against the existing
dump, not implemented changes.** (A)-(D) are candidate directions for a
future production mechanism — how `hist_peak_lag_ms` (or a redesigned
successor to it) might eventually be used — not proposals to edit
`domain/constellation.rs`, the fingerprint measure glue, or the picker
right now. None of rule 2 (picker), rule 3 (vote), or rule 4
(uniqueness/redraw) has changed, and none should change to chase any of
(A)-(D) without a separate, explicit decision to do so. Where a
candidate's own validation used real numbers (B's exposure argument, D's
narrowed-null check), that validation was done by *re-deriving* the
histogram in a throwaway script against already-dumped points — never
by altering the fingerprint path itself. §7's rule stands for all of
these exactly as it does for a matcher: "Do not promote into
`seam_locks.rs` or `GapSignature` from this dump."

**A. One-sided A window is a structural blind spot ("shadow zone").**
Rule 6 makes A one-sided (`[gap_start − context, gap_start)`); rule 7
makes B symmetric-ish (`A span ± max_lag`). Any true B onset whose
lag-adjusted equivalent A-position falls in `(0, +max_lag_ms]` is inside
B's searched span but past A's own window edge — unreachable no matter
how prominent. Quantified corpus-wide, full 51-pair scan (1049 gaps with
both `constellation` and `geometry`): **819/1049 (78.1%)** have
`max_lag_ms` (constant 2000 ms across every gap seen) exceeding the
gap's own `duration_secs` (median gap 1094.7 ms), so the overshoot
commonly reaches past the real hole into genuinely valid, already-
resumed A audio (median overshoot where it exceeds: 1147.0 ms) — this is
the common case, not an edge case; identical rate to the 14-pair partial
read. One concrete
locked-but-wrong shoulder (pair 1, gap 5, pre) has its highest-prominence
B point (ruling out a prominence-floor fix) sitting, once shifted by the
trusted lag, ~1.8 s past A's window edge.

**B. Widening A is not risk-free — do not bundle it with (A) above
without (C).** A's narrow window is *derived from certainty* (we know
exactly which gap-adjacent span is meaningful); B's width is *derived
from uncertainty* (lag unknown, so padded). Mirroring B's number onto A
conflates the two. Concretely:
  - Symmetric widening only closes (A) if B's window is *also* re-derived
    off the new, wider A span (`B = new-A-span ± max_lag`), else the
    shadow zone just moves outward. One re-derivation, not a regress.
  - Extra A content is not inert. An orphan A feature (no true B partner)
    usually scatters its 12 pairwise votes across many bins — mostly
    harmless — *except* when source content is periodic/clustered
    (dialogue rhythm, music beat, repeated foley), where it can clump
    into a false near-tie against the true peak. That is the same
    mechanism already producing today's 28% (gap 5's pre shows exactly
    this: both A and B clustered, real mode 6 votes vs redraw_p95 5).
    Widening A increases exposure to an already-known failure mode; it
    is not a new, unrelated risk, but it is not free either.

**C. Vote weighting (down-weight isolated / far-from-seam onsets) is the
candidate fix for the false-positive half; widening A is the candidate
fix for the false-negative half — they are not substitutes.** Weighting
by something like `prominence × w(distance from seam) × w(isolation
from same-side neighbors)` directly targets clumped-but-spurious votes
(the gap-5 B outlier is both far from the seam and far from B's other
clusters). Two open questions, not yet resolved: the redraw null would
need the same weighting applied to its synthetic draws to stay a fair
95th-percentile comparison (rule 4); and isolation is partly an artifact
of (A)'s blind spot itself (a B point can look "isolated" only because
its true A partner was never searched), so measuring weighting's effect
before touching A's window isolates the two fixes' contributions.

**Recommended order, if this is picked up:** measure weighting alone
first, A's window untouched, and see how much of the 28% it recovers
before deciding whether widening A (with C's exposure in mind, and
B's knock-on re-derivation) is still needed for the remainder. This
keeps the false-positive fix and the false-negative fix separately
attributable instead of shipped as one bundled change.

**D. Cross-side interrogation (converged side informs the other) —
promising, cheap to validate, not yet built.** Same motivation as (A)
without (B)'s contamination risk: instead of blindly widening A's
candidate set, use a confidently-locked side's own lag as a prior to
narrow or re-weight the *other* side's already-searched B span. Pre and
post are separated only by the gap's own (usually short) duration, so
their true local lags should track closely if B is a locally-consistent
mapping near the seam.

Checked against already-existing, independent data (`lag_decision`'s
`pre_anchor`/`post_anchor` `frac_lag_ms`, not the constellation
histogram at all), full 51-pair corpus: **971/1073 gaps have both
anchors; `|pre − post|` is tight** — median 0.02 ms, 90.1% within 10 ms,
93.2% within 30 ms, 98.7% within 60 ms (essentially unchanged from the
14-pair partial read: 90.7%/94.9%/99.4%). The premise holds for the
overwhelming majority of gaps. The known outlier (pair 31 gaps 7 and 8:
deltas 1122 ms and 1688 ms) is still the largest disagreement and is
still the already-known map-step gap (settled: pair 31 straddles a
−1.15 s map step). The full read surfaces a **second cluster worth a
look**: pair 50 gaps 9–17 show repeated large pre/post disagreements
(deltas 273.8–1040.3 ms) — not yet investigated, flagged only, possibly
a second map-step-adjacent pair. Either way this is a good sign, not a
counterexample: "pre and post disagree by a lot" is itself a usable
signal (flag/decline, do not blend) rather than a flaw in the premise —
the failure mode is legible, not silent.

Not yet designed: how to fold this into rule 4's redraw-null test (a
narrowed or re-weighted search changes what "beats the 95th percentile"
means), and how large a disagreement should trigger "decline to use the
prior" vs "use it." Cheaper than (A)/(B)/(C) to prototype since it needs
no new A content and no new B search — just a re-weighting of the
existing per-shoulder histogram using the other shoulder's own already-
computed result.

**Scope, checked against the constellation dump itself (not just the
`lag_decision` anchors above; full 51-pair corpus, 1049 gaps with both
shoulders present):** `both locked` 307, **`exactly one locked` 550
(52.4%, the largest bucket)**, `neither locked` 192 (essentially the
same shape as the 14-pair partial read: 240/376/125). The un-locked
sibling in that 550 is almost always `not_unique` (496; only 54
`unmeasured`) — the picker did find candidate onsets, the vote just
didn't clear the redraw bar — which is exactly the shape a re-weighting
prior helps with (a real candidate pool to nudge, not an empty one).
227/550 (41%) have the locked side beating its own `redraw_p95_votes`
by ≥ 3 votes, not a hairline pass. This makes (D) a
larger-scope candidate than it first looked: a working rescue could
plausibly move a large share of `not_unique` shoulders into "locked,"
not just patch rare disagreements — worth prioritizing ahead of (B)/(C)
for that reason alone, independent of (B)'s contamination-risk
argument.

**Validated entirely from existing dump data, no re-scan; full 51-pair
corpus.** Method: for a one-side-locked gap, trust-filter the locked
side's `hist_peak_lag_ms` against `lag_decision`'s independent waveform
anchor (agreement within 60 ms; drops 125 cases where the "lock" is
itself the wrong peak — see (B)'s own worked example, pair 1 gap 5,
which is exactly one of these), leaving **401 trust-filtered candidates**.
Then, on the `not_unique` other side's own already-dumped A/B points,
recompute the all-pairs vote count in a window near the locked side's
lag, and compare it against a **redraw null restricted to that same
narrow window** (drawn from the already-stamped B span + `min_spacing_ms`
— rule 4's mechanism, just scoped smaller), not the shoulder's full-
haystack `redraw_p95_votes`. That distinction matters: checked against
the full-haystack bar, only 39/401 (9.7%) "win" — but that bar answers
"is anything anywhere real," not "is this one predicted spot real," so
it understates the effect. Checked against the correctly-scoped narrow
null, **207/401 (51.6%)** of trust-filtered `not_unique` sides already
have a real vote cluster at the sibling's lag that would have locked
under a narrowed search — matching the (~52%) scope estimate above, and
essentially unchanged from the 14-pair partial read's 52.9%. This is
proof-of-concept, not a shipped mechanism: it doesn't yet say how narrow
the search window should be in production, or how to fold a narrowed
null into rule 4 in general (not just at gaps where we happen to already
know the answer from the other side).
