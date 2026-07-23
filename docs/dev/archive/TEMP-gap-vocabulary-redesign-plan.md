# Gap vocabulary redesign — measurement-first grouping (ARCHIVED)

> **ARCHIVED (2026-07-03) — P0–P3 done; P4 parked, not scheduled.** Do not update this doc for status or
> next steps. **Live vocabulary:** [gap-vocabulary.md](../gap-vocabulary.md) (P3 output — named cells +
> legacy W-tier appendix). **Live index:** [TEMP-seam-repair-status-ledger.md](TEMP-seam-repair-status-ledger.md)
> (§F production rollout; P4 tracked as a parked item, §D). This doc is retained for the axis-derivation
> narrative and the full P2 corpus tables (§7) — the reasoning, not the current state.

Status: **P0–P3 DONE.** P0 + P1 capture done; axis structure settled (§2/§2a); P2 clustering done
(2026-07-02) on the nominal-reanchor rescan (`gap-files/re-anchor-dual-fit-on-nominal`; cluster table
§7g). Golden baseline frozen; orthogonality gate passed (two axes degenerate on this corpus — noted, not
blocking). **P3 published (2026-07-03):** [gap-vocabulary.md](../gap-vocabulary.md) — named cells + legacy
W-tier appendix. **Remaining: P4** (wire coordinates into decisions/tags — deferred, see §4). Mechanism + repair:
[TEMP-seam-repair-status-ledger.md](TEMP-seam-repair-status-ledger.md) §4 (historical:
[TEMP-seam-splice-dualfit-plan.md](TEMP-seam-splice-dualfit-plan.md)).
Bracket-vs-step + proof index:
[TEMP-seam-repair-status-ledger.md](TEMP-seam-repair-status-ledger.md) (B1, B11; C3, C7).

§7a–7e preserve the **historical 6-pair snapshot** (19 matched) that motivated the redesign; **§7f** tombstones; **§7g** is the
authoritative P2 cluster table on the current 7-pair / 62-matched corpus.

Reading: [gap-repair-guide.md](../../gap-repair-guide.md) § Vocabulary (the taxonomy this revises),
[gap-fingerprint.md](../gap-fingerprint.md) § Lag fingerprint,
[seam-scoring.md](../../seam-scoring.md) §3–4,
[TEMP-w5-timing-offset-diag-plan.md](TEMP-w5-timing-offset-diag-plan.md) (the class that
exposed the defect). Siblings:
[TEMP-seam-splice-dualfit-plan.md](TEMP-seam-splice-dualfit-plan.md) (mechanism history — archived),
[TEMP-seam-repair-status-ledger.md](TEMP-seam-repair-status-ledger.md) (proven/open index),
[TEMP-cross-codec-seam-impl-plan.md](TEMP-cross-codec-seam-impl-plan.md) (superseded —
validator-swap refuted; R2/R4 retained as diagnostics),
[TEMP-w5-timing-offset-rescue-plan.md](TEMP-w5-timing-offset-rescue-plan.md) (archived warp path).

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
([TEMP-w5-timing-offset-rescue-plan.md](TEMP-w5-timing-offset-rescue-plan.md)),
cross-codec validator-swap ([TEMP-cross-codec-seam-impl-plan.md](TEMP-cross-codec-seam-impl-plan.md)).

---

## 2. Principle — describe a gap by coordinates, derive the decision

> A gap is a **point in a low-dimensional measurement space** describing how A's kept content meets B's
> donor content at the seam(s). Its "type" is the region it occupies; the patch decision (patch / skip /
> which correction) is a **function over the coordinates** — not a threshold on one conflated scalar.

The axes, measured (or derivable) from the fingerprint. **Role** = **D**ecision (a gate branches on it) ·
**R**epair (builds the fix) · **X** (diagnostic — describes, does not gate); mirrors the perf audit's
measurement→gate labels. **Placement** is part of the axis — the same field at a different placement is a
different measurement (the load-bearing discipline; see the two ⚑ rows):

| Axis | Question | Field(s) · **placement** | Role | Values |
|------|----------|--------------------------|------|--------|
| **Geometry** | Interior gap vs length-mismatch tail | `geometry.duration_secs` | D | interior · tail (P6) |
| **A-seam presence** | A has content at the edge, or silence walk-off? | `silence.*`, `levels` · A | D | content · silence |
| **Donor — nominal** ⚑ | Is B silent at the *same program time* (quiet-in-both)? | `donor_interior_nominal` · **nominal `b_mapped`** (no lag) | D (gate, D11) | occupied · **program-quiet** |
| **Donor — aligned** ⚑ | Does B *bridge* the hole at the registered placement? | `donor_interior.continuous`/`rms_db` · **aligned bridge** | R (gate) | bridges · **BROKEN** (interior silent) |
| **Bracket search** | Did anchor/grid search find a lag-0 placement? | `brackets_passing`, `bracket_exhausted()` | D | rescued (≥1 pass → patch) · exhausted (0 → dual-fit candidate) |
| **Registration — gross** ⚑ | Offset + step over the 1 s window | `baseline_lag` → offset `(pre+post)/2` + **step** `post−pre`; `edge_pinned` validity | R + diag | offset · **stepped** (`\|step\|>2 ms` usual; *not* drift) |
| **Registration — seam-local** ⚑ | The lag the *fill* actually uses at each 250 ms seam | `splice_dualfit`/`seam_local_peak` · **seam-local (±`SEAM_LOCAL_REFINE_MS`)** | R (placement) | agrees with gross · **diverges** (sub-window edit — the `2·g1` case) |
| **Seam viability** | Would a length-reconciled fill pass the *unchanged* gate? | `splice_dualfit.gate_pass`, `post_seam_global_r` · seam-local | **D (the repair gate)** | pass · fail; step real · spurious |
| **Shared source** | B's seam content is the *same master* as A? | `baseline_lag.peak_r`; `one-sided-dead` | (constant here) | **same** (all pairs) · different/ambiguous *(untested — D2/D8)* |
| **Uniqueness** | Periodicity/alias guard on the registration | `peak_z`, `prominence` · gross 1 s | **X** (diagnostic — demoted) | unique · alias-suspect |
| **Envelope agreement** | Macro placement confidence, independent of waveform | `structure.baseline_*`; `wide_envelope` | D (structure) / X (wide-env) | strong · weak |

The legacy Pearson tier ≈ a nonlinear AND of *(A-presence) × (shared-source) × (registration-at-lag-0) ×
(bracket-found)*. It is a *projection* of this space; recovering the axes un-conflates the W-buckets.

**W5 reinterpretation:** "same-master, lag-0 / bracket validation failed" — not "weak content."

### 2a. How the axes shifted as the ledger was worked (structure proven; values provisional)

The original seven axes above refined in five ways as claims were proven — the **structure is now stable**
(safe to build the harness + decisions on), while several **values/thresholds remain provisional** (SUPP/OPEN,
tuned post-rescan). The `#`-marked shifts are what the perf §4 harness keys on:

1. **Registration split into two scales** ⚑ — gross (1 s `baseline_lag`, classification/uniqueness) vs
   **seam-local** (250 ms, the fill placement). `2·g1` proved they diverge; conflating them was a real
   false-negative bug (ledger A3 correction). *New axis row.*
2. **Donor presence split into two placements + promoted to a gate** ⚑ — nominal (registration-independent →
   program-quiet, D11) vs aligned (bridges/BROKEN). `1·g19` proved donor-BROKEN must *gate*, not just
   describe. *Was one descriptive row; now two decision rows.*
3. **Shared source collapsed to a constant** — all same-master (one-sided-dead is a placement artifact,
   B2/C1). `different`/`ambiguous` untested; the real gap is **decoy safety (D8)**, not a live axis.
4. **Uniqueness (`peak_z`) demoted decision → diagnostic** — it stays primary for the *descriptive*
   `splice_diag`, but does **not** predict seam viability (A3), so the repair gates on
   `gate_pass ∧ donor`, not uniqueness.
5. **Registration skew/drift refuted** (B8) — only offset + step survive; drift is a tombstone.
   **Bracket-search** (B1) held up as the patch/skip spine.

**Validation status:** the *structure* (axes, orthogonality-in-principle, the D/R/X partition, the placement
discipline) is proven in the ledger. The *values* (which gaps are targets, seam-local width, donor/step/quiet
thresholds) are provisional and re-derived by the `seam-local-fix` rescan. **P2 orthogonality gate
passed** (2026-07-02) — cluster table §7g; golden baseline frozen (perf §4.0).

---

## 2b. Candidate additional axes — measurement status

Candidate further axes. Keep only if **physically independent**, **robustly measurable**, and
**decision-relevant**.

| Candidate axis | Why it matters | Measured today? |
|----------------|----------------|-----------------|
| **Channel scope / per-channel consistency** | Partial-channel dropout vs full (5.1 center-dominant) | **Yes** — `seams.per_channel`, `selected_channels`; surface in analyzer (TODO) |
| **Donor displacement** | How far fill slid from nominal B map; far slide = decoy risk | **Partly** — derive from `geometry`; compute slide (TODO) |
| **Seam level / SNR** | Measurability + audibility stakes at the seam | **Yes** — `seam_probe.snr_db` on **energy-weighted downmix** (not straight mono `/N` on 5.1) |
| **Match uniqueness / periodicity** | Guards registration/step from periodic false peaks | **Yes (capture DONE)** — `LagSummary.peak_z`, `prominence`, `top2_spacing_ms` at **1 s** window (§3.6a); retire 250 ms `second_peak_r` as primary. **Now diagnostic, not a repair gate (§2a.4)** — does not predict seam viability; the repair gates on `gate_pass ∧ donor`. |
| **Residual cancellation** | Strong same-master test on *identical* encodings | **Yes (capture DONE)** — `GapFingerprint.residual`; **wrong discriminator for cross-encoded pairs** (expected `informative=false`) — diagnostic only here |
| **Wide-envelope concordance** | Segment identity at macro scale; peak lag vs fine lag | **Yes (capture DONE)** — `wide_envelope` (100 ms bin); cross-scale agreement check |

**Re-scan:** nominal-reanchor corpus (`gap-files/re-anchor-dual-fit-on-nominal`, 2026-07-02) populates
`donor_interior`, `splice`, `peak_z` @ 1 s, `wide_envelope`, and golden D/R fields.

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
| **Clustering** + named types — **[gap-vocabulary.md](../gap-vocabulary.md)** (P3 done) | Removing/renaming live `gap_tags.rs` vocabulary (P4) |
| | Rewriting `gap-repair-guide.md` Layers 1–5 wholesale |

**This is a descriptive re-vocabulary.** The gate still decides on Pearson; this work describes gaps
better than the gate currently acts on them. Wiring coordinates into decisions/tags is P4 (deferred).
Repair routing for bracket-exhausted skips → [TEMP-seam-repair-status-ledger.md](TEMP-seam-repair-status-ledger.md) §4
(historical detail: [TEMP-seam-splice-dualfit-plan.md](TEMP-seam-splice-dualfit-plan.md)).

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

### P2 — Cluster the corpus; let the data name the types (+ **orthogonality gate for perf §4**) — **DONE (2026-07-02)**

Group gaps by axis coordinates; report cells that occur (frequency + exemplars). **Do not impose a
taxonomy** — read it off the data. **Dual purpose:** P2 also **validates the axis structure** before the
perf §4 harness freezes its golden record — confirm the D/R axes (§2/§2a) are (a) **independent** (no two
always co-vary → collapse them), (b) **populated** (a cell that never occurs isn't an axis), (c)
**non-redundant**.

**Run corpus:** `gap-files/re-anchor-dual-fit-on-nominal` (nominal-reanchor `splice_dualfit`, commits
`2622c7a` + `b099b83`). Reproduce: `GAP_FP_DIRS=gap-files/re-anchor-dual-fit-on-nominal GAP_FP_CSV=1 cargo
test -p clip-sync-repair --features diagnostic-tests --test diag_fingerprint_corpus -- --nocapture` →
`target/gap_fingerprint_corpus.csv`. Golden snapshot:
`crates/clip-sync-repair-harness/golden/re-anchor-dual-fit-on-nominal.golden.json`.

**Results:** cluster table + orthogonality verdict in **§7g**. Golden baseline frozen; perf §4.0 gates met
([`golden/README.md`](../../../crates/clip-sync-repair-harness/golden/README.md)).

Cluster on **bracket_passing × donor × outcome** (primary); **step is not a cluster divider** (confirmed).
Expected cells from the pre-rescan hypothesis (refined in §7g):

- `tail` · `no-lag` — 7 tail · 0 no-lag on this corpus
- `patched-bracket-rescued` — 23/62 matched (includes high-step gaps like 5·g3 +73 ms)
- `skip-bracket-exhausted` — 32/39 skipped matched
- `silence-splice` — both shoulders recoverable at ±600 ms on all matched gaps with measured registration
- `alias-suspect` — 24/55 at old 250 ms metric; demoted to diagnostic (§2a.4)

### P3 — Draft the redesigned vocabulary — **DONE (2026-07-03)**

Published as [gap-vocabulary.md](../gap-vocabulary.md): five named cells (bracket patch, silence-splice,
program-quiet, no-placement, tail) from the §7g.1 cluster table, each with its old W-tier
correspondence inline + a compact legacy-appendix table. Confirms the W5 mislabel: `symmetric_weak`
conflates silence-splice (rescuable) and program-quiet (permanently unfillable) on one Pearson score.

### P4 — Wire coordinates into decisions/tags (DEFERRED)

Only after P3: first-class `gap_tags.rs` facts. Repair routing for `dual_fit_candidate()` gaps → dualfit
plan §4 — **not** cross-codec validator tiers.

---

## 5. Validation

- **P0:** ✓ `baseline_lag` on full corpus; offset/step decomposition; drift framing rejected.
- **P1:** re-scan populates `donor_interior`, `peak_z`, `wide_envelope`, `splice`; harness columns live.
- **P2:** ✓ cluster table on re-anchor rescan (§7g): 62 matched · 32 bracket-exhausted · step not a divider;
  orthogonality gate passed (two axes degenerate on this corpus — §7g.2).
- **P3:** ✓ [gap-vocabulary.md](../gap-vocabulary.md) — five named cells + legacy W-tier appendix; W5 mislabel documented. C/GK guide IDs deferred (content-shape hints, not gap cells).
- **Regression:** g003 (pair 1 idx 3) — same-master / stepped; may be alias-suspect at 250 ms margin but
  unique at 1 s (§3.6a); do not require old `uniqueness_margin ≥ 0.30`.

---

## 6. Open questions

1. **Axis thresholds.** Calibrate `peak_z` / `prominence` on rescan distribution. `|step| < 2 ms` ("clean")
   is rare (**6/62** matched on re-anchor rescan) — do not treat as the default registration bucket.
2. **Registration sub-split.** Keep **offset** vs **step**; drop offset-vs-**skew** (drift refuted). Mechanism
   sub-frame, open (quantization test ≈ 0.84× chance).
3. **Keep or deprecate W-tiers?** **Decided (P3):** keep as *derived* operator readout in `-v`/JSON ([gap-vocabulary.md](../gap-vocabulary.md) § Derived readouts); gap **type** is the cell, not the W-tier. P4 may wire axis facts into `gap_tags.rs` (D4).
4. **Existing tags.** `donor_relation`, `content_hint`, `seam_shape` — reconcile, don't duplicate (§2b).
5. **Decorrelated absent.** This corpus is all same-master; `different`/`ambiguous` source regions untested.
6. **Bracket search vs dual-fit.** When brackets pass (5·g3), today's path suffices. Dual-fit targets
   `bracket_exhausted` + recoverable shoulders + donor continuous — see dualfit review decision tree.

---

## 7. Corpus findings

§7a–7e: **historical 6-pair snapshot** (`diag_fingerprint_corpus`, 19 matched) — preserved for the narrative
that exposed the W5 mislabel. **§7f:** tombstone. **§7g:** authoritative **P2 cluster table** on the current re-anchor rescan.

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

**Repair (shipped):** independent per-seam fit + length reconciliation at gap interior;
validate with **unchanged** waveform gate — see
[TEMP-seam-repair-status-ledger.md](TEMP-seam-repair-status-ledger.md) §4 (A3 **shipped**; default `dual_fit` on). Scope: **bracket-exhausted** skips
only (not high-step patches like 5·g3 where 18/25 brackets already pass). Historical mechanism detail:
[TEMP-seam-splice-dualfit-plan.md](TEMP-seam-splice-dualfit-plan.md).

**Proof sequencing:** P3 published; C3/C7 via scan-native **`splice_dualfit`** (proven) before §4
repair — see [TEMP-seam-repair-status-ledger.md](TEMP-seam-repair-status-ledger.md) (C3, C7, §4).

### 7f. Superseded hypotheses (tombstone)

| Hypothesis | Verdict |
|------------|---------|
| Clip drift / time-warp rescue | Refuted — step is local, not monotone vs gap time |
| §7c "skip was right" via uniqueness funnel | Superseded — wrong timescale + wrong residual test |
| Cross-codec validator-swap (§7d redirect) | Refuted — measurement artifact; plan archived |
| Per-seam warp | Archived |

### 7g. P2 cluster table — re-anchor rescan (7 pairs, 2026-07-02)

**Corpus:** `gap-files/re-anchor-dual-fit-on-nominal` · nominal-reanchor `splice_dualfit` + corrected
`step_is_real` · golden baseline frozen
(`crates/clip-sync-repair-harness/golden/re-anchor-dual-fit-on-nominal.golden.json`).

**Overview (69 gaps total; matched denominator = 62):**

| Bucket | Count | Notes |
|--------|------:|-------|
| Matched (analysis denominator) | 62 | All carry matchable B seam content |
| Tail (P6) | 7 | Length-mismatch tails; excluded from matched table |
| No-lag | 0 | — |
| Patched | 23 | 37% of matched |
| Skipped | 39 | 63% of matched |
| Bracket-exhausted (0 passing) | 32 | 82% of skipped matched |
| Program-quiet skips (D11) | 24 | Correctly unfillable — drop from addressable denominator |
| Dual-fit targets (`dualfit_target()`) | 9 | Bracket-exhausted · donor-continuous · gate_pass · step-real · ¬program-quiet |

`plan_kind` always `fillable`. Among matched with registration: **100% `timing_offset`** at gross placement
(decorrelated gaps are start-of-file / no-bracket `g0` rows).

#### 7g.1 Primary cells — bracket search × donor × outcome

Read types off the data; do **not** treat step magnitude as a primary divider (patched vs skipped `|step|`
ranges overlap: patched 0.0–588 ms median 29 ms · skipped 0.3–598 ms median 80 ms).

| n | Bracket search | Donor (nominal + aligned) | Outcome | Exemplars |
|--:|----------------|---------------------------|---------|-----------|
| 16 | **bracket-rescued** (≥1 pass) | donor-continuous | **patch** | 1·g6, 1·g20, 2·g3, 3·g2, 5·g3 (+72 ms step), … |
| 7 | **bracket-rescued** | donor-BROKEN (interior silent) | **patch** | 1·g1, 1·g2, 1·g8, 1·g23, … |
| 9 | **bracket-exhausted** | donor-continuous | **skip** → **dual-fit target** | 1·g3, 1·g5, 1·g22, 2·g1, 2·g2, 5·g6, 7·g2, 7·g3, 7·g4 |
| 1 | **bracket-exhausted** | donor-continuous | **skip** (gate unmeasured) | 5·g0 |
| 22 | **bracket-exhausted** | **program-quiet** | **skip** (nothing to fill) | 1·g4, 1·g7, 1·g9, 1·g10, 1·g19, 6·g6, … |
| 2 | **no-brackets** (`g0`) | donor-continuous | skip | 1·g0, 3·g0 |
| 3 | **no-brackets** (`g0`) | donor-BROKEN | skip | 4·g0, 6·g0, 7·g0 |
| 2 | **no-brackets** (`g0`) | program-quiet | skip | 2·g0, 6·g2 |

**Patch vs skip separator:** bracket search success, not step. Among skipped matched with brackets:
best-bracket seam **max 0.26**; among patched: best-bracket seam **median 0.51**. Structure placement is
not the failure mode — **27/39** skipped have `structure_min ≥ 0.5` but fail `waveform_floor` at the throat.

#### 7g.2 Orthogonality gate (perf §4.0 prerequisite)

Run on golden D/R coordinates; verdict **PASS** (2026-07-02) — cells are interpretable; golden baseline
frozen. Two axes **degenerate on this same-master corpus** (noted, not blocking):

| Axis pair | Verdict | Evidence |
|-----------|---------|----------|
| **`gate_pass`** vs bracket-exhausted repair scope | **Degenerate gate** | 31/32 bracket-exhausted gaps pass `gate_pass` (1 unmeasured: 5·g0); 54/55 bracketed gaps with `splice_dualfit` pass. ±600 ms seam search is over-permissive — target set rests on **donor-occupancy ∧ step-real**, not seam viability (D8 caveat). |
| **Donor-aligned** vs **donor-nominal** | **Redundant on corpus** | `aligned_donor_continuous ≡ nominal_donor_continuous` on **62/62** matched. Kept as D8 safety net for decoy regimes. |
| **Step** vs patch/skip | **Not independent** (expected) | `\|step\|` ranges overlap between patched and skipped — step describes registration, not outcome. |
| **Shared source** | **Constant** | All same-master; `different`/`ambiguous` untested (D2/D8). |
| **Bracket search** × **donor** × **outcome** | **Populated + discriminating** | Primary cells in §7g.1 cover all 62 matched gaps; dual-fit targets occupy one clean sub-cell (9 gaps). |

**Load-bearing axes confirmed:** geometry (matched vs tail) · bracket search · donor (continuous / BROKEN /
program-quiet) · registration (offset + step — descriptive, not outcome) · seam-local viability (degenerate
as a *gate* here, still descriptive).

#### 7g.3 Expected-cell checklist (P2 hypothesis → rescan)

| Pre-rescan cell | Rescan result |
|-----------------|---------------|
| `tail` · `no-lag` | 7 tail · 0 no-lag |
| `patched-bracket-rescued` | 23/62 matched |
| `skip-bracket-exhausted` | 32/39 skipped matched |
| `silence-splice` (recoverable shoulders) | All matched gaps with measured registration recover at ±600 ms |
| `alias-suspect` @ 250 ms | 24/55 — diagnostic only (§2a.4); does not predict patch/skip |

**P3 output (published):** [gap-vocabulary.md](../gap-vocabulary.md) — five named cells + legacy W-tier appendix; W5 → silence-splice / program-quiet (not "weak").

---

## 8. Related reading

| Doc | Contents |
|-----|----------|
| [gap-vocabulary.md](../gap-vocabulary.md) | **P3** — named gap cells + legacy W-tier appendix |
| [gap-repair-guide.md](../../gap-repair-guide.md) | The Pearson-rooted vocabulary being revised |
| [gap-fingerprint.md](../gap-fingerprint.md) | Measurements (lag, levels, donor_interior, splice, …) |
| [TEMP-seam-splice-dualfit-plan.md](TEMP-seam-splice-dualfit-plan.md) | Mechanism history (archived); wire spec → ledger §4 |
| [TEMP-seam-repair-status-ledger.md](TEMP-seam-repair-status-ledger.md) | Proven/open index; bracket-vs-step; proof sequencing |
| [TEMP-cross-codec-seam-impl-plan.md](TEMP-cross-codec-seam-impl-plan.md) | Superseded validator-swap |
| [TEMP-w5-timing-offset-rescue-plan.md](TEMP-w5-timing-offset-rescue-plan.md) | Archived warp path |
| [TEMP-w5-timing-offset-diag-plan.md](TEMP-w5-timing-offset-diag-plan.md) | The class that exposed the conflation |
