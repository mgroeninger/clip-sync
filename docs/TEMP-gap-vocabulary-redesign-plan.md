# Gap vocabulary redesign — measurement-first grouping (DRAFT)

> **Status & next-steps for the whole effort live in the ledger:**
> [TEMP-seam-repair-status-ledger.md](TEMP-seam-repair-status-ledger.md). This doc is the *detail* for the
> vocabulary redesign; the ledger is the authoritative proven/open/important index.

Status: **DRAFT — direction validated; P0 + P1 capture DONE; P2 clustering BLOCKED on registration placement.**
Full 6-pair corpus analyzed (19 matched, 6 skipped). The **registration axis** is confirmed: per-side
lag-resolved Pearson at each shoulder's *own* best lag plus the **step** between them — not the single throat
Pearson@0 (which conflated misalignment, silence-splice, and a ±25 ms-window artifact into one "dead seam").
**Patch vs skip** is **bracket-search success**, not step magnitude. **But clustering the corpus into named
types (P2) must wait until quiet-gap registration is corrected (ledger A1/A2 — `structure_start_frame`
wanders on quiet gaps), or the coordinates it clusters on are wrong.** Mechanism + repair:
[TEMP-seam-splice-dualfit-plan.md](TEMP-seam-splice-dualfit-plan.md).
Bracket-vs-step + proof index:
[TEMP-seam-repair-status-ledger.md](TEMP-seam-repair-status-ledger.md) (B1, B11; C3, C7).

Reading: [gap-repair-guide.md](gap-repair-guide.md) § Vocabulary (the taxonomy this revises),
[gap-fingerprint.md](gap-fingerprint.md) § Lag fingerprint,
[seam-scoring.md](seam-scoring.md) §3–4,
[archive/TEMP-w5-timing-offset-diag-plan.md](archive/TEMP-w5-timing-offset-diag-plan.md) (the class that
exposed the defect). Siblings:
[TEMP-seam-splice-dualfit-plan.md](TEMP-seam-splice-dualfit-plan.md) (mechanism + repair — unbuilt),
[TEMP-seam-repair-status-ledger.md](TEMP-seam-repair-status-ledger.md) (proven/open index),
[archive/TEMP-cross-codec-seam-impl-plan.md](archive/TEMP-cross-codec-seam-impl-plan.md) (superseded —
validator-swap refuted; R2/R4 retained as diagnostics),
[archive/TEMP-w5-timing-offset-rescue-plan.md](archive/TEMP-w5-timing-offset-rescue-plan.md) (archived warp path).

---

## 1. Problem — the vocabulary is rooted in a lossy primitive

The current gap vocabulary's spine is **`min(pre, post)` Pearson at lag 0** (Layer 3 W-tiers
High/marginal/dead_zone/hard_skip and the `seam_shape` geometry). But lag-0 Pearson is a *single scalar
that collapses several independent physical facts*: it is high only when **(A has content at the seam)
AND (B's donor is the same source) AND (they are registered at lag 0) AND (anchor/boundary search found a
bracket where both seams pass @0)**. When it is low the number can't say *which* failed — silence,
different content, mis-registration at lag 0, or **bracket exhaustion** all land in the same
`W5 / symmetric_weak` bucket. The taxonomy inherits the conflation because it is built on that one
measurement.

**Corpus evidence (6 pairs, 40 gaps — `diag_fingerprint_corpus`):** 19 matched · 15 no-lag · 6 tail.
Among matched: **100% `timing_offset`, 0 `decorrelated`** (`peak_r` ≥ 0.91); **13 patched, 6 skipped**.
The shared-source axis is *constant on this corpus* — every donor is the same master (`one-sided-dead = 0`:
every shoulder aligns at *some* lag). The relationship varies on **registration** (offset + step) and on
**whether bracket search finds a lag-0 compromise** — not on content identity.

That makes `W5 / symmetric_weak` an actively *misleading* label: skips are not generically "weak both
sides." They are **same-master, per-side recoverable at own lag, bracket-exhausted** — e.g. 1·g19 and
5·g3 share ~72 ms throat step; 5·g3 patches (**18 / 25** brackets pass) while 1·g19 skips (**0 / 16**).
The Pearson tier reports a **validation + search-space** failure as a content-quality verdict.

So the fix is not "add a `lag_verdict` axis to the existing taxonomy." It is **re-root the taxonomy on
the independent physical axes of the A↔B seam relationship**, and let the Pearson tier become a *derived
readout*, not the primitive.

**Superseded repair directions (do not revive):** per-seam warp
([archive/TEMP-w5-timing-offset-rescue-plan.md](archive/TEMP-w5-timing-offset-rescue-plan.md)),
cross-codec validator-swap ([archive/TEMP-cross-codec-seam-impl-plan.md](archive/TEMP-cross-codec-seam-impl-plan.md)).

---

## 2. Principle — describe a gap by coordinates, derive the decision

> A gap is a **point in a low-dimensional measurement space** describing how A's kept content meets B's
> donor content at the seam(s). Its "type" is the region it occupies; the patch decision (patch / skip /
> which correction) is a **function over the coordinates** — not a threshold on one conflated scalar.

The axes, measured (or derivable) from the fingerprint:

| Axis | Question | Fingerprint field(s) | Values |
|------|----------|----------------------|--------|
| **Role / geometry** | Interior gap vs length-mismatch tail | `geometry.duration_secs` | interior · tail (P6) |
| **A-seam presence** | A has content at the edge, or silence walk-off? | `silence.collar_above_relative_floor`, `collar_rms_peak_ratio`, `levels` | content · silence |
| **B donor presence** | B has energetic content in the hole? | `donor_interior` (`continuous`, `rms_db`, `silence_fraction`); lag present as fallback | bridges · hole · none |
| **Shared source** | B's seam content is the *same master* as A? | `baseline_lag.peak_r`, `splice.pre/post_peak_r`; `one-sided-dead` | same · different · ambiguous |
| **Registration** | If same source, how aligned at the throat? | `baseline_lag` → **offset** `seam_mid_ms` `(pre+post)/2` + **step** `seam_step_ms` `post−pre` | offset (shiftable scatter) · **stepped** (usual: `\|step\| > 2 ms` on 18/19 matched; *not* clip drift) |
| **Bracket search** | Did anchor/grid search find a lag-0 placement? | `brackets[]` pass/fail; analyzer `brackets_passing`, `bracket_exhausted()` | rescued (≥1 pass) · exhausted (0 pass) |
| **Envelope agreement** | Placement confidence, independent of waveform | `structure.baseline_pre/post` | strong · weak |

The legacy Pearson tier ≈ a nonlinear AND of *(A-presence) × (shared-source) × (registration-at-lag-0) ×
(bracket-found)*. It is a *projection* of this space; recovering the axes un-conflates the W-buckets.

**W5 reinterpretation:** "same-master, lag-0 / bracket validation failed" — not "weak content."

---

## 2b. Candidate additional axes — measurement status

Candidate further axes. Keep only if **physically independent**, **robustly measurable**, and
**decision-relevant**.

| Candidate axis | Why it matters | Measured today? |
|----------------|----------------|-----------------|
| **Channel scope / per-channel consistency** | Partial-channel dropout vs full (5.1 center-dominant) | **Yes** — `seams.per_channel`, `selected_channels`; surface in analyzer (TODO) |
| **Donor displacement** | How far fill slid from nominal B map; far slide = decoy risk | **Partly** — derive from `geometry`; compute slide (TODO) |
| **Seam level / SNR** | Measurability + audibility stakes at the seam | **Yes** — `seam_probe.snr_db` on **energy-weighted downmix** (not straight mono `/N` on 5.1) |
| **Match uniqueness / periodicity** | Guards registration/step from periodic false peaks | **Yes (capture DONE)** — `LagSummary.peak_z`, `prominence`, `top2_spacing_ms` at **1 s** window (§3.6a); retire 250 ms `second_peak_r` as primary |
| **Residual cancellation** | Strong same-master test on *identical* encodings | **Yes (capture DONE)** — `GapFingerprint.residual`; **wrong discriminator for cross-encoded pairs** (expected `informative=false`) — diagnostic only here |
| **Wide-envelope concordance** | Segment identity at macro scale; peak lag vs fine lag | **Yes (capture DONE)** — `wide_envelope` (100 ms bin); cross-scale agreement check |

**Re-scan pending:** committed `gap-files/` scans predate several fields (`donor_interior`, `splice`,
`peak_z` @ 1 s, `wide_envelope`). Capture schema is complete in code; one re-scan populates them.

**Registration measurement for quiet gaps — outward-anchor (operator idea; see dualfit §3.7).** For a gap
inside a *long quiet section*, no window *centered* on the seam (250 ms edge **or** 1–2 s wide-envelope) has
distinctive signal — so both the uniqueness (`peak_z`) and the structure placement (`structure_start_frame`)
fail there. The fix is to measure the registration axis by **searching outward to the nearest distinctive
(loud) feature per side, lag-aligning there, and carrying the lag back to the seam** (same-master rigid
content, negligible drift over 1–2 s). Validated: a 500 ms window at a distant loud feature beats a 2 s
window centered on the quiet gap (7·g3 pre `peak_z` 15 vs 11; 7·g4 post 10.6→21.6). This is the registration
measurement for the quiet regime, distinct from the centered wide-envelope of §2b's uniqueness row.

Deliberately *excluded* as non-axes (they re-project the above): the raw `min(pre,post)` Pearson tier
(derived), `seam_shape` (Pearson geometry), `content_hint` flat/contour (≈ envelope axis).

---

## 3. Scope — descriptive first

| In scope | Out of scope (for now) |
|----------|------------------------|
| Decision-placement lag (P0) + axis surfacing (P1) | Changing the gate's patch/skip *decision* (still Pearson-driven) |
| Surfacing per-gap **axis coordinates** in fingerprint + analyzer | Cross-codec validator-swap (archived) |
| **Clustering** the real corpus; naming the types that actually occur | Removing/renaming live `gap_tags.rs` vocabulary |
| A redesigned **vocabulary doc** (axes + observed types) | §4 dual-fit repair implementation |
| | Rewriting `gap-repair-guide.md` Layers 1–5 wholesale |

**This is a descriptive re-vocabulary.** The gate still decides on Pearson; this work describes gaps
better than the gate currently acts on them. Wiring coordinates into decisions/tags is P4 (deferred).
Repair routing for bracket-exhausted skips → [TEMP-seam-splice-dualfit-plan.md](TEMP-seam-splice-dualfit-plan.md).

---

## 4. Phases

### P0 — Decision-placement lag ("#2") — **DONE (2026-06-29)**

`lag` was measured at a **diagnostic** placement (best energy-peak anchor bracket), producing implausible
"drifts." Registration is now measured at the structure-slid **throat** via `baseline_lag`.

**Implemented & validated on `gap-files/1..6`:**
- `GapFingerprint.baseline_lag` at throat in both builders; analyzer prefers it over diagnostic `lag`.
- Offset/step decomposition (`seam_mid_ms` / `seam_step_ms`) reframes registration: **not clip drift**
  (per-file offset slope ≈ 0 on well-sampled pairs); **step is the usual state** (18/19 matched with
  `|step| > 2 ms`).

### P1 — Surface the axes (+ load-bearing measurements) — **capture DONE; harness re-scan pending**

1. **Surface existing fields** in `gap_fingerprint_corpus.rs`: A-presence, donor presence, shared-source,
   registration, envelope, geometry, **bracket pass counts** (`brackets_passing`, `bracket_exhausted`,
   `dual_fit_candidate`). — *mostly done*; channel scope, donor displacement still TODO.
2. **Load-bearing measurements (§2b) — DONE in `gap_fingerprint.rs` (2026-06-30):**
   - **Uniqueness** — 1 s lag window; `peak_z`, `prominence`, `top2_spacing_ms`; analyzer `uniqueness_z`.
     Thresholds frozen by §3.6a (`peak_z ≥ 12`, `prom ≥ 0.45`, calibrate on rescan).
   - **Residual** — `ResidualInfo` at throat; `residual_headroom_db` in analyzer (cross-codec caveat).
   - **Donor interior** — `DonorInterior` over `b_mapped` span.
   - **Wide envelope** — `WideEnvelopeFingerprint` @ 100 ms bin.
   - **Splice summary** — `SpliceSummary` (`step_ms`, per-side peaks / `peak_z`).
   - **Seam level** — energy-weighted downmix SNR on `seam_probe`.

**Remaining:** re-scan to populate new fields; harness projection + threshold calibration.

### P2 — Cluster the corpus; let the data name the types

Group gaps by axis coordinates; report cells that occur (frequency + exemplars). **Do not impose a
taxonomy** — read it off the data.

**Expected cells (to confirm/refine on rescan):**
- `tail` · `no-lag` (no matchable B bracket)
- `patched-bracket-rescued` (≥1 bracket passes — includes high-step gaps like 5·g3 +72 ms)
- `skip-bracket-exhausted` (0 brackets pass; structure often ≥ 0.5)
- `silence-splice` (both shoulders recoverable at ±200 ms `baseline_lag`)
- `alias-suspect` (thin uniqueness at *old* 250 ms metric; may clear at 1 s `peak_z`)

Cluster on **bracket_passing × step × structure × splice_diag** — not skew/drift types.

### P3 — Draft the redesigned vocabulary

Write **axis coordinates → named type**, with:
- observed types from P2;
- mapping from old guide IDs (W5 = "same-master, lag-0/bracket validation failed", not "weak");
- legacy Pearson tier as *derived* readout.

### P4 — Wire coordinates into decisions/tags (DEFERRED)

Only after P3: first-class `gap_tags.rs` facts. Repair routing for `dual_fit_candidate()` gaps → dualfit
plan §4 — **not** cross-codec validator tiers.

---

## 5. Validation

- **P0:** ✓ `baseline_lag` on full corpus; offset/step decomposition; drift framing rejected.
- **P1:** re-scan populates `donor_interior`, `peak_z`, `wide_envelope`, `splice`; harness columns live.
- **P2:** stable cluster table including **bracket-exhausted** (6/19 matched); step is not a cluster divider.
- **P3:** each old W/C/P ID maps to new types; W5 mislabel documented.
- **Regression:** g003 (pair 1 idx 3) — same-master / stepped; may be alias-suspect at 250 ms margin but
  unique at 1 s (§3.6a); do not require old `uniqueness_margin ≥ 0.30`.

---

## 6. Open questions

1. **Axis thresholds.** Calibrate `peak_z` / `prominence` on rescan distribution. `|step| < 2 ms` ("clean")
   is rare (1/19) — do not treat as the default registration bucket.
2. **Registration sub-split.** Keep **offset** vs **step**; drop offset-vs-**skew** (drift refuted). Mechanism
   sub-frame, open (quantization test ≈ 0.84× chance).
3. **Keep or deprecate W-tiers?** Keep as *derived* operator readout; stop treating as the type (decide in P3).
4. **Existing tags.** `donor_relation`, `content_hint`, `seam_shape` — reconcile, don't duplicate (§2b).
5. **Decorrelated absent.** This corpus is all same-master; `different`/`ambiguous` source regions untested.
6. **Bracket search vs dual-fit.** When brackets pass (5·g3), today's path suffices. Dual-fit targets
   `bracket_exhausted` + recoverable shoulders + donor continuous — see dualfit review decision tree.

---

## 7. Corpus findings (6 pairs, `diag_fingerprint_corpus`)

### 7a. Overview

- `plan_kind` always `fillable`; **40 gaps** — 19 matched · 15 no-lag · 6 tail.
- Among matched: **100% `timing_offset`, 0 `decorrelated`**; **13 patched, 6 skipped**.
- `one-sided-dead = 0` — no shoulder fails to recover at any lag in ±200 ms (cross-encoding validator need
  refuted).
- `seam_shape` / `skip_reason` stub in fingerprint outcome; fine Pearson vocabulary lives in patch path
  (`gap_tags.rs`).

### 7b. Registration — offset + step; not drift

Decomposing `baseline_lag` at the throat:

- **Offset** (median ~28 ms): per-gap scatter, **not clip drift** — per-file offset vs gap time has ~0
  slope on well-sampled pairs.
- **Step** (median ~29 ms, max ~122 ms): pre↔post lag disagreement; **18/19 matched** have `|step| > 2 ms`
  — the *normal* registration signature, not a rare "edit" type. Cannot be smooth clock skew.
- **Mechanism OPEN.** Not block-quantized (best fit ≈ 0.84× chance). Leading hypothesis: silence-splice at
  encoder boundaries (dualfit §0); sub-frame / resampler boundary also plausible.

### 7c. Measurement lessons (what §7c funnel got wrong)

An early read of uniqueness + residual concluded "no gap survives → skip was right." **That strategic
conclusion is superseded.** The funnel numbers are real at the *old* metric definitions; the interpretation
was wrong:

| Probe | Lesson |
|-------|--------|
| **`uniqueness_margin` @ 250 ms** | 13/19 "periodicity-suspect" — **wrong timescale**. At **1 s**, alias-suspect gaps (e.g. pair-1 g3-pre) become decisively unique (`peak_z` ≥ 15). Retire single-rival @ 250 ms as primary. |
| **Residual @ throat** | 18/19 `informative=false` on cross-encoded pairs — **expected** (lossy encodings don't cancel sample-for-sample). Wrong same-source test for this corpus; keep as diagnostic, not funnel gate. |
| **Trustworthy funnel** | `matched 19 → unique (margin ≥ 0.30) 6 → +residual-confirmed 0` — **do not use as go/no-go.** Operator ground truth: all six pairs are same soundtrack; gaps are fillable. |

### 7d. Placement works; validation + bracket search decide patch/skip

Operator ground truth: duplicate recordings, different encodings; other sections patch cleanly.

- **Structure/envelope placement largely works** — all 6 skips have `structure_min ≥ 0.5`; skips are not
  wrong-neighborhood failures.
- **Patch vs skip = bracket search**, not step magnitude:
  - Patched: best-bracket seam median **0.62** (throat median **0.38** — often weak at throat, rescued).
  - Skipped: best-bracket seam max **0.11**; all fail `waveform_floor`.
- **Cross-codec validator hypothesis refuted:** R2/R4 high + Pearson dead + low `recovered_r` = post-side
  optimal lag outside `seam_probe`'s **±25 ms** window, not genuine phase-scramble (`one-sided-dead = 0`).

### 7e. Silence-splice + dual-fit (current repair hypothesis)

Per-side `baseline_lag` over ±200 ms: both shoulders recover at own lag (`peak_r` 0.92–1.00) on every
matched gap; they differ by a **step**. Skips share this signature with patches — they skipped because
**no bracket** achieves lag-0 Pearson on both sides simultaneously, not because content differs.

**Repair direction (unbuilt):** independent per-seam fit + length reconciliation at gap interior;
validate with **unchanged** waveform gate — see
[TEMP-seam-splice-dualfit-plan.md](TEMP-seam-splice-dualfit-plan.md). Scope: **bracket-exhausted** skips
only (not high-step patches like 5·g3 where 18/25 brackets already pass).

**Proof sequencing:** P3 from fingerprints now; P1/P2 via offline `diag_splice_dualfit` simulation before
§4 repair — see [TEMP-seam-repair-status-ledger.md](TEMP-seam-repair-status-ledger.md) (C3, C7).

### 7f. Superseded hypotheses (tombstone)

| Hypothesis | Verdict |
|------------|---------|
| Clip drift / time-warp rescue | Refuted — step is local, not monotone vs gap time |
| §7c "skip was right" via uniqueness funnel | Superseded — wrong timescale + wrong residual test |
| Cross-codec validator-swap (§7d redirect) | Refuted — measurement artifact; plan archived |
| Per-seam warp | Archived |

---

## 8. Related reading

| Doc | Contents |
|-----|----------|
| [gap-repair-guide.md](gap-repair-guide.md) | The Pearson-rooted vocabulary being revised |
| [gap-fingerprint.md](gap-fingerprint.md) | Measurements (lag, levels, donor_interior, splice, …) |
| [TEMP-seam-splice-dualfit-plan.md](TEMP-seam-splice-dualfit-plan.md) | Mechanism + repair (unbuilt) |
| [TEMP-seam-repair-status-ledger.md](TEMP-seam-repair-status-ledger.md) | Proven/open index; bracket-vs-step; proof sequencing |
| [archive/TEMP-cross-codec-seam-impl-plan.md](archive/TEMP-cross-codec-seam-impl-plan.md) | Superseded validator-swap |
| [archive/TEMP-w5-timing-offset-rescue-plan.md](archive/TEMP-w5-timing-offset-rescue-plan.md) | Archived warp path |
| [archive/TEMP-w5-timing-offset-diag-plan.md](archive/TEMP-w5-timing-offset-diag-plan.md) | The class that exposed the conflation |
