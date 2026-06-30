# Gap vocabulary redesign — measurement-first grouping (DRAFT)

Status: **DRAFT — not started.** P0 (decision-placement lag, "#2") is the prerequisite and is being
built first.

Reading: [gap-repair-guide.md](gap-repair-guide.md) § Vocabulary (the taxonomy this revises),
[gap-fingerprint.md](gap-fingerprint.md) § Lag fingerprint,
[seam-scoring.md](seam-scoring.md) §3–4,
[archive/TEMP-w5-timing-offset-diag-plan.md](archive/TEMP-w5-timing-offset-diag-plan.md) (the class that
exposed the defect). Sibling: [TEMP-w5-timing-offset-rescue-plan.md](TEMP-w5-timing-offset-rescue-plan.md)
(the production correction; shares P0/#2).

---

## 1. Problem — the vocabulary is rooted in a lossy primitive

The current gap vocabulary's spine is **`min(pre, post)` Pearson at lag 0** (Layer 3 W-tiers
High/marginal/dead_zone/hard_skip and the `seam_shape` geometry). But lag-0 Pearson is a *single scalar
that collapses several independent physical facts*: it is high only when **(A has content at the seam)
AND (B's donor is the same source) AND (they are registered at lag 0)**. When it is low the number can't
say *which* failed — silence, different content, or mis-registration all land in the same
`W5 / symmetric_weak` bucket. The taxonomy inherits the conflation because it is built on that one
measurement.

**Corpus evidence (5/6 pairs, 38 gaps — `diag_fingerprint_corpus`):** of the 18 gaps with a donor seam,
**every one is `timing_offset` and zero are `decorrelated`** (`peak_r` 0.97+). The shared-source axis is
*constant* — every donor is the same master. So the relationship varies almost entirely on
**registration** (alignment), not content. That makes `W5 / symmetric_weak` an actively *misleading*
label here: these aren't "weak both sides" content gaps, they're **identical content that is
mis-registered**. The Pearson tier reports an alignment symptom as a content-quality verdict.

So the fix is not "add a `lag_verdict` axis to the existing taxonomy." It is **re-root the taxonomy on
the independent physical axes of the A↔B seam relationship**, and let the Pearson tier become a *derived
readout*, not the primitive.

---

## 2. Principle — describe a gap by coordinates, derive the decision

> A gap is a **point in a low-dimensional measurement space** describing how A's kept content meets B's
> donor content at the seam(s). Its "type" is the region it occupies; the patch decision (patch / skip /
> which correction) is a **function over the coordinates** — not a threshold on one conflated scalar.

The axes, each already measured by the fingerprint:

| Axis | Question | Fingerprint field(s) | Values |
|------|----------|----------------------|--------|
| **Role / geometry** | Interior gap vs length-mismatch tail | `geometry.duration_secs` | interior · tail (P6) |
| **A-seam presence** | A has content at the edge, or silence walk-off? | `silence.collar_above_relative_floor`, `collar_rms_peak_ratio`, `levels` | content · silence |
| **B donor presence** | B has energetic content in the hole? | lag present / `b_has_energy` | donor · none |
| **Shared source** | B's seam content is the *same master* as A? | `lag.peak_r` (+ residual cancel) | same · different · ambiguous |
| **Registration** | If same source, how aligned at the seam? | `lag.frac_lag_pre/post` **at the decision placement (P0)** | clean(~0) · offset(equal≠0) · skew(ramp) · edit(inconsistent) |
| **Envelope agreement** | Placement confidence, independent of waveform | `structure.baseline_pre/post` | strong · weak |

The legacy Pearson tier ≈ a nonlinear AND of *(A-presence) × (shared-source) × (registration-at-lag-0)*.
It is a *projection* of this space; recovering the axes un-conflates the W-buckets.

---

## 2b. Candidate additional axes — and which need a *new* measurement

The six axes in §2 are the obvious decomposition, but they are not obviously complete. Candidate further
axes, with whether the fingerprint already measures them. The selection rule: keep an axis only if it is
**physically independent** of the others, **robustly measurable**, and **decision-relevant**; drop any
that merely re-project an existing axis.

| Candidate axis | Why it matters | Measured today? |
|----------------|----------------|-----------------|
| **Channel scope / per-channel consistency** | Partial-channel dropout vs full; the relationship may hold on only some channels (5.1 center-dominant, one dead channel) — physically a different gap | **Yes** — `seams.per_channel`, `selected_channels`; just surface it |
| **Donor displacement** | How far the fill placement slid from the nominal B map; a far slide = wrong-content / repeat risk (F1 decoy) | **Partly** — derive from `geometry` (`b_mapped_start` vs nominal); compute the slide |
| **Seam level / SNR** | Loud (speech) vs quiet (room tone) seam: changes measurability, audibility, and the stakes of a bad splice | **Yes** — `levels` (speech-peak / noise-floor / gap-floor); summarize *at the decision seam* |
| **Match uniqueness / periodicity** | `peak_r` can be **spuriously high on periodic content** (tones, music) — a periodic false-match reads as "same source". We saw exactly this defeat correlation in the synthetic tone bursts. **Without this axis the shared-source axis has false positives.** | **No — NEW**: summarize the lag curve's *secondary-peak* structure (2nd-peak ratio / count of near-equal peaks) |
| **Residual cancellation** | The *strongest* same-master test: subtract aligned B from A and measure what cancels. Confirms shared-source beyond mere correlation. | **Partial** — `residual_band` / `SeamResidualVerdict` exist in the **patch path**, not recorded in the fingerprint; add residual headroom at the decision seam |

**The two that need new measurement are the load-bearing ones:** *match uniqueness/periodicity* and
*residual cancellation* are what make the **shared-source** and **registration** axes *trustworthy*.
With only `peak_r`/`frac_lag`, a periodic signal can read as a confident, well-registered same-master
match when it is an artifact. So **P1 must add these measurements, not just surface existing fields** —
otherwise the new vocabulary rests on the same fragile correlation primitive the old one did, one level
down. (This is also why P0's decision-placement lag is necessary but not sufficient.)

Deliberately *excluded* as non-axes (they re-project the above): the raw `min(pre,post)` Pearson tier
(derived), `seam_shape` (Pearson geometry), `content_hint` flat/contour (≈ envelope axis).

---

## 3. Scope — descriptive first

| In scope | Out of scope (for now) |
|----------|------------------------|
| Decision-placement lag (P0 / "#2") so the **registration** axis is trustworthy | Changing the gate's patch/skip *decision* (still Pearson-driven) |
| Surfacing per-gap **axis coordinates** in fingerprint + analyzer | The production timing-offset *rescue* (its own plan; shares P0) |
| **Clustering** the real corpus; naming the types that actually occur | Removing/renaming the live `gap_tags.rs` vocabulary in code |
| A redesigned **vocabulary doc** (axes + observed types) | Rewriting `gap-repair-guide.md` Layers 1–5 wholesale |

**This is a descriptive re-vocabulary.** The gate still decides on Pearson; this work describes gaps
better than the gate currently acts on them. Wiring coordinates into decisions/tags is a later, separate
step (P4, deferred). That keeps the redesign honest: we have not changed the pipeline based on these
findings yet.

---

## 4. Phases

### P0 — Decision-placement lag ("#2") — **code DONE (2026-06-29); validation pending**

`lag` was measured at a **diagnostic** placement (the best energy-peak anchor bracket). That made the
registration numbers untrustworthy — the implausible "drifts" (40+ ms over ~1.8 s ⇒ >10,000 ppm) meant
the two diagnostic windows locked onto *different* correlation peaks, not a real skew. The registration
axis is now measured at the seam the gate decides on: the structure-slid **throat** placement.

**Implemented:**
- `GapFingerprint.baseline_lag: Option<LagFingerprint>` — `lag_at_placement` at the throat placement
  (`place_on_b(... nominal_of(refined.start_frame) ...)`), in **both** builders: the structure builder
  (`build_gap_fingerprint`) and the authoritative **gate** path (`characterize_gaps_with_gate`, the one
  `--gap-fingerprints` uses). `lag` (best-bracket) kept for continuity.
- Analyzer reads `baseline_lag` for the registration axis, falling back to `lag` for older files.
- All `GapFingerprint` literals + round-trip test updated; lib + analyzer tests green.

**Validation pending (the P0 deliverable):** the existing `gap-files/1..6` scans predate `baseline_lag`,
so they only carry the diagnostic `lag`. **Re-fingerprint with the rebuilt binary**, then confirm the
drift numbers become physically plausible (tens–hundreds of ppm) and that `baseline_lag` differs from
`lag` where the throat ≠ best bracket. Until that re-run, only the non-registration axes (presence,
shared-source, envelope, geometry) are safe to cluster on.

### P1 — Surface the axes (and add the two missing measurements)

Two parts:

1. **Surface existing fields** in `gap_fingerprint_corpus.rs` (and the fingerprint projection): A-presence
   (`collar_above_relative_floor`, ratio), donor presence (lag present), shared-source (`peak_r`),
   registration (`baseline_lag` frac/verdict), envelope (`structure.baseline_*`), geometry (duration/tail),
   channel scope (`per_channel`/`selected_channels`), donor displacement (slide from nominal), seam level
   (`levels`).
2. **Add the load-bearing measurements (§2b)** to the fingerprint at the decision seam — these are *new*,
   not just surfaced: **match uniqueness / periodicity** (secondary-peak structure of the lag curve, so a
   periodic false-match doesn't read as same-source) and **residual cancellation** (the strong same-master
   confirm). Without them the shared-source / registration axes have false positives.

CSV gains a column per axis; the summary prints the axis distribution.

### P2 — Cluster the corpus; let the data name the types

Group the gaps by axis coordinates and report the cells that actually occur (frequency + exemplars).
**Do not impose a taxonomy** — read it off the data. Expected from the 5-pair preview (to confirm/refine):
tail · no-donor(genuine gap) · clean-fill · same-master-registered · same-master-offset ·
same-master-skewed. Output a cluster table (the empirical type list).

### P3 — Draft the redesigned vocabulary

Write the new vocabulary as **axis coordinates → named type**, with:
- the observed types from P2, each defined by its axis region;
- a mapping to the old guide IDs (what each W/C/P maps to, and where the old labels are *wrong* — e.g.
  W5 = "same-master mis-registered", not "weak");
- the legacy Pearson tier shown as a *derived* readout, not the definition.
Land it as a new doc (or a `gap-repair-guide.md` § rewrite) once P2's clusters are stable.

### P4 — Wire coordinates into decisions/tags (DEFERRED)

Only after P3: add the axes as first-class `gap_tags.rs` facts (`shared_source`, `registration`, …),
and let the gate route on them (ties into the timing-offset rescue's `timing_offset_trusted` tier).
Separate effort; needs sign-off that the descriptive vocabulary holds.

---

## 5. Validation

- **P0:** re-fingerprinted corpus shows physically plausible skew (ppm), and `baseline_lag` differs from
  the diagnostic `lag` where the gate's chosen seam differs from the best bracket.
- **P2:** every gap falls into exactly one named cluster; the cluster table is stable across pairs.
- **P3:** each old guide ID (P/C/W) maps to one or more new types, and every place the old label
  contradicts the measurement (W5 mislabel) is called out.
- **Regression:** the committed g003 exemplar still classifies as same-master / mis-registered (skew),
  via `tests/w5_timing_offset.rs` + the analyzer unit test.

---

## 6. Open questions

1. **Axis thresholds.** Where is "clean" vs "offset" (ms)? "offset" vs "skew" (the `frac_lag_pre/post`
   consistency test)? "same" vs "ambiguous" source (`peak_r`)? Calibrate from the corpus, not a priori.
2. **Registration sub-split.** offset (equal nonzero) vs skew (linear ramp) vs edit (inconsistent) needs
   a linearity test on `baseline_lag` pre/post — only meaningful after P0.
3. **Keep or deprecate W-tiers?** Likely keep as a *derived* operator readout, but stop treating them as
   the type. Decide in P3.
4. **Relationship to existing tags.** `donor_relation` (`same_master`) is the run-level version of the
   shared-source axis; `content_hint` (flat/contour) overlaps envelope; `seam_shape` is a Pearson-geometry
   readout. Reconcile, don't duplicate.
5. **Decorrelated absent.** This corpus is all same-master. Before generalizing the vocabulary, get a
   pair where B is a *different* capture (the shared-source axis actually varies) — else the
   `different`/`ambiguous` source regions are untested on real data.

---

## 7. Corpus findings so far (5/6 pairs, `diag_fingerprint_corpus`)

- `plan_kind` always `fillable`; 18 matched · 15 no-lag · 5 tail.
- Among matched: **100% `timing_offset`, 0 `decorrelated`** → shared-source axis constant.
- 13/18 matched already patched by the gate; **5 (28% of matched) are recoverable-but-skipped** — the
  addressable class (1 "constant", 4 "drift" — but the constant/drift split is **not trustworthy until
  P0**).
- `seam_shape` empty and `skip_reason` generic `"gate skipped"` in the fingerprint outcome — the fine
  Pearson vocabulary is a *stub* there; it's only computed in the live patch path (`gap_tags.rs`).

---

## 8. Related reading

| Doc | Contents |
|-----|----------|
| [gap-repair-guide.md](gap-repair-guide.md) | The Pearson-rooted vocabulary being revised |
| [gap-fingerprint.md](gap-fingerprint.md) | The measurements (lag, levels, silence, structure, seams) |
| [TEMP-w5-timing-offset-rescue-plan.md](TEMP-w5-timing-offset-rescue-plan.md) | Production correction; shares P0 |
| [archive/TEMP-w5-timing-offset-diag-plan.md](archive/TEMP-w5-timing-offset-diag-plan.md) | The class that exposed the conflation |
