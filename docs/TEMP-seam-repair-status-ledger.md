# Seam-repair status ledger — proven / open / important (triage index)

**Purpose.** The two working docs
([TEMP-seam-splice-dualfit-plan.md](TEMP-seam-splice-dualfit-plan.md),
[TEMP-gap-vocabulary-redesign-plan.md](TEMP-gap-vocabulary-redesign-plan.md)) hold ~30 claims at every
stage of proof. This ledger is the **index over them**: one row per claim, scored **Confidence × Importance
× Target**, so we can see the critical path and what to incorporate. The two docs stay the detail; this is
the map. Update this when a claim's status changes.

**Legend.** Confidence: `PROVEN` (data) · `SUPP` (strong, small n) · `OPEN` · `REFUTED`.
Importance: `CRIT` (blocks a working repair) · `HIGH` · `MED` · `LOW`.
Target: `VOCAB` · `PIPE` (detect/repair) · `CAP` (fingerprint capture) · `—` (conclusion/park/tombstone).

**Evaluation cohort (do not merge denominators).**
```text
Primary dual-fit cohort: the 6-pair corpus — 19 matched, 6 skipped. The 6 bracket-exhausted skips are the
  dual-fit targets. Counts in B1/B8 refer to this cohort.
Extended scans on disk: dirs 1–7 (dir 1 = 25 gaps; 69 gaps total) are a SUPERSET and are F1-placement
  (not yet outward-anchor). Treat as EXPLORATORY — do not merge their rates with the primary cohort or use
  them to calibrate gates until registration placement (A1/A2/C2) is settled.
```

---

## A. The critical path (do these, in order)

The claims that actually gate a working repair. Everything else is supporting.

| # | Claim / task | Conf | Why it's the blocker |
|---|--------------|------|----------------------|
| A1 | **Registration placement for quiet gaps is unsolved.** `structure_start_frame` (what the F1 fix registers on) *wanders* on flat envelopes → quiet gaps mis-register (6·g6, 7·g3 dead there, clean at `b_mapped`). | SUPP | Everything downstream reads the wrong lag for quiet gaps until this is fixed. **The rescan as currently built registers at `structure_start_frame`, so it will reproduce this.** |
| A2 | **Outward-anchor registration** (align on nearest distinct loud feature per side, carry the lag) is the fix for A1. | SUPP (small n) | Placement artifacts proven on **6·g6 / 7·g3** (dead at `structure_start_frame`, clean at `b_mapped`); outward-anchor uplift shown on **7·g3 pre** (`peak_z` 9→15) and **7·g4 post** (10.6→21.6). Full sweep = C1 (pair-6 running). If it holds → registration method for the detect **and** the capture. |
| A3 | **Dual-fit repair** (independent per-side fit → length reconcile → unchanged gate passes) actually works. | OPEN (unbuilt) | This is the product. Prove offline (`diag_splice_dualfit` simulation) before wiring. |
| A4 | **Donor continuity** — B carries unbroken content across the hole. | OPEN / PARTIAL | The donor half of "fillable." Coded + **partially populated** (55/69 gaps). **But `donor_interior` is measured at `structure_start_frame`, so it inherits A1:** 6·g6 reads `continuous=false, sil_frac=1.00` ("silent hole") only because the span is at the *wandered* placement — it is recoverable at −132.8. **Not decisive until A2/C2.** Prove via C5. |
| A5 | **Threshold calibration** (`peak_z ≥ 12`, prominence, continuity) on the real distribution. | OPEN | Turns the frozen heuristics into gates. Needs **correct-placement** data (depends on A1/A2). |

**Sequencing consequence (important):** the on-disk `gap-files/` scans are already **F1-placement rescans**
(1 s `peak_z`, `donor_interior`, etc. populated) — but they register at `structure_start_frame`, which
wanders on quiet gaps (A1). So **treat them as exploratory:** fine for structure, bracket, and loud-gap
reads; **do not** use them to gate quiet gaps or to calibrate detect thresholds (A5). And do **not** launch
*another* multi-hour rescan until A1/A2 are settled — bake outward-anchor into the capture, register at
`b_mapped`, or keep using the offline harness for quiet gaps — else it re-inflates the same mis-registration.

---

## B. Proven — incorporate now (no more proof needed)

| # | Claim | Conf | Target | Incorporation |
|---|-------|------|--------|---------------|
| B1 | Patch vs skip = **bracket-search success, not step magnitude** (5·g3 vs 1·g19; full step overlap; best-bracket seam 0.62 vs 0.11) | PROVEN | VOCAB + PIPE | Vocab: `bracket_search` axis; W5 = "lag-0/bracket validation failed." Detect: scope dual-fit to `bracket_exhausted`. |
| B2 | **No genuine cross-encoding *type*** — apparent `one-sided-dead` cases are placement/lag-width artifacts (6·g6, 7·g3 proven) | SUPP | — | Refutes cross-codec validator (archived); "same-master" axis constant on this corpus. **Not** "every one-sided-dead is an artifact" — that awaits the exhaustive C1 sweep. The *scare* (a cross-encoding gap type) is resolved; registration for quiet gaps is still open (A1). |
| B3 | Uniqueness needs a **1 s window + `peak_z`** (retire 250 ms `second_peak_r`) | PROVEN | CAP (schema) | Decision frozen §3.6a. **Schema done, not corpus:** trustworthy population waits on A1/A2 + a correct-placement rescan; calibrate thresholds then (A5). |
| B4 | Level/SNR on **energy-weighted downmix** (straight mono `/N` buries 5.1 center 13–15 dB) | PROVEN | CAP (schema) | Frozen; schema done, corpus partial (as B3). |
| B5 | **Correlation on mono** (representation doesn't matter — Pearson scale-invariant) | PROVEN | CAP (schema) | Simplifies: no per-channel correlation. Schema done. |
| B6 | **F1 placement** — register at the gate's own throat, not a divergent `place_on_b` | PROVEN | PIPE (done) | Done via `gate_structure_align`. **F1 ≠ quiet-gap registration solved:** the throat itself wanders on quiet envelopes (A1) — F1 fixed the *divergence* bug, not the *placement* problem. |
| B11 | **Dual-fit ≠ what bracket search already does** — the winning bracket's boundary move is *not* the throat step (5·g3: +72 ms step vs 2600 ms `move_frames`; 0/18 patched gaps have `\|step\|` within 20 ms of a bracket delta) | PROVEN | PIPE | Confirms dual-fit is a distinct operation (interior length edit), not a re-run of anchor/boundary search. Scopes §4. |
| B12 | **Wide-envelope lag concordance** — 100 ms-bin envelope peak lag agrees with the fine-waveform lag | SUPP (pair 1) | CAP (schema) | Secondary registration confirmer; not a gate until `wide_envelope` is populated at the correct placement. |
| B7 | **Content is un-stretched within a side** (both shoulders align at a single lag each) | SUPP | — | The premise that makes reconciliation a **pure trim/pad**, not a warp (A3). |
| B8 | Registration = **offset + step**, not clip drift (per-file slope ≈ 0; 18/19 have `|step|>2 ms`) | PROVEN | VOCAB | Registration axis; drop drift/skew framing. |
| B9 | Residual is the **wrong same-source test** for cross-encoded pairs (`informative=false` expected) | PROVEN | — | Keep as diagnostic; do not gate on it. |
| B10 | **Non-finite/residual-null serialization bug** (silent gaps → `null` → dropped whole pairs) | PROVEN + FIXED | CAP (done) | `finite_db`/`finite_corr`; analyzer tolerant. |

---

## C. Open + important — prove next (ranked)

| # | Question | Conf | Imp | How to prove |
|---|----------|------|-----|--------------|
| C1 | Does outward-anchor **collapse the one-sided-dead bucket** across all gaps (not just n=2)? | OPEN | CRIT | Full `[outward-anchor]` sweep on all one-sided-dead gaps (pair-6 running; needs pair-1/others' media). |
| C2 | Which **placement** should registration use — `structure_start_frame`, `b_mapped`, or outward-anchor? | OPEN | CRIT | Compare all three per gap in the harness; pick the one that recovers quiet gaps. |
| C3 | Does the **dual-fit repair pass the unchanged gate** on the known skips? | OPEN | CRIT | Offline `diag_splice_dualfit` simulation before wiring §4. |
| C4 | Is **±200 ms lag search** sufficient at the *correct* placement, or clip large offsets? | OPEN | HIGH | Falls out of C1/C2 (edge-pinned peaks at correct placement ⇒ widen `lag_max_lag_ms`). |
| C5 | **Donor continuity** true for the skip targets? (= A4, ranked) | OPEN / PARTIAL | HIGH | Validate at the **correct placement** (post C2) — the partial `donor_interior` on disk is at `structure_start_frame` and mis-reads quiet gaps (6·g6 false there, recoverable at −132.8). Do not use pre-A2 continuity as a gate. |
| C6 | **Threshold calibration** — `peak_z`/prominence/continuity floors on the real distribution. | OPEN | HIGH | Corrected-placement corpus (post A1/A2). **Calibrate on BOTH the patched and the skipped distributions** — patched gaps also fail strict uniqueness (5·g3 margin 0.02), so a floor tuned on skips alone would misfire. **Detect gates ≠ patch gates:** dual-fit's detect is a *new* gate on bracket-exhausted gaps, not the production Pearson gate — they need not share thresholds (review C6/S8). |
| C7 | **Trim magnitude ≈ measured `step_ms`** — the length edit the repair applies matches the fingerprinted step | OPEN | HIGH | Falls out of the C3 (`diag_splice_dualfit`) simulation: compare samples trimmed for a gate-passing fill vs `SpliceSummary.step_ms`. A large mismatch ⇒ wrong model. |

---

## D. Open + low / parked (do not spend cycles yet)

| # | Item | Why parked |
|---|------|-----------|
| D1 | **Mechanism of the step** (silence-splice vs resampler vs PTS; sub-frame, not quantized) | The repair *measures* the step; the physical cause doesn't change the fix. Interesting, not blocking. |
| D2 | **Decorrelated / different-content regime** | Untestable — this corpus is all same-master. Revisit only with different-content data. |
| D3 | **Channel-scope / donor-displacement axes** (vocab §2b) | Surface in analyzer later; not decision-relevant for dual-fit. |
| D4 | **Keep vs deprecate W-tiers**; reconcile `gap_tags.rs`/`content_hint`/`seam_shape` | Vocab P3/P4 decision; after the type set is named. |
| D5 | **Perf** (FFT lag, dedup search, decode reuse) | Deliberately deferred until the plan is proven. See **Capture parked** below. |
| D6 | **No regression on existing patches** (dual-fit flag off ⇒ unchanged) | Verify after A3 (repair built) — a run-comparison, not an open question yet. |
| D7 | **Audibility of the trim point** (splice at low-energy interior sounds clean) | After A3; gate-pass is necessary, not sufficient (needs a listen). |
| D8 | **Decoy / wrong-placement safety** (a deliberately wrong B offset still fails the gate) | After A3; corpus has only weak negatives (failed brackets), so this needs a synthetic/shifted-haystack test. |
| D9 | **Fingerprint diagnostic stubs** (F2/F3) | Gate path omits per-bracket `structure_*` and leaves `GateOutcome` vocabulary tags empty. Fine for diagnostics today. See **Capture parked**. |

---

## Capture parked (fingerprint layer hygiene)

Parked **CAP** items from the deleted dualfit plan review — still valid, not on the critical path until
A1/A2 settle registration and a correct-placement rescan is worth running.

**F1 (mostly done).** Decision metrics (`baseline_lag`, `seam_probe`, `donor_interior`, `wide_envelope`,
`splice`, `residual`) register at `oracle_throat_structure_frame` — same throat the gate scores. **Remnant:**
top-level `fp.structure` still comes from the summary pass's `place_on_b` and is not refreshed in the gate
overlay; corpus `structure_min` stats may disagree with the oracle throat. `fp.seams.baseline_*` is updated
from the zero-move oracle bracket.

| # | Item | Status | When to fix |
|---|------|--------|-------------|
| F2 | Gate `brackets[]` write `structure_pre/post = None` (oracle has structure internally) | OPEN | Only if analyzer needs per-bracket structure or schema/docs parity |
| F3 | `GateOutcome.seam_shape` / `fit_path` / `signature_mode` empty in gate path | OPEN | Only if vocabulary tags migrate into fingerprints |
| C-docs | `gap-fingerprint.md` omits `baseline_lag`, `seam_probe`, `splice`, `donor_interior`, `wide_envelope` | OPEN | When capture schema is frozen post A2 |
| C-harness | `uniqueness_z` uses a single-sided `splice.peak_z` when only one side is present (slightly optimistic) | OPEN | Low — tighten when calibrating A5/C6 |

**Perf (before a long rescan).** Dominant cost is still N × oracle bracket scoring (required). Avoidable
overhead today: (1) summary `characterize_gaps` still runs one `place_on_b` before the gate overlay; (2)
diagnostic `fp.lag` at the best-energy bracket adds another `place_on_b` + `lag_at_placement`; (3)
`dump_gap_fingerprints` re-decodes A/B after repair. Likely wins when rescans matter: drop summary
`place_on_b` when gate follows; share one border extract at the throat for lag + probe + wide-envelope;
reuse repair decode; optional coarse-to-fine / FFT lag on the 1 s window.

**Do not optimize first:** `donor_interior` RMS; parallel per-gap loops before deduping search and aligning
placement.

---

## E. Refuted — tombstone (do not revive)

| Hypothesis | Verdict |
|------------|---------|
| Per-seam detect-and-warp rescue | Refuted / archived — step is local, content un-stretched |
| Cross-codec validator-swap (R2/R4 loosen the gate) | Refuted — measurement artifact; plan archived (R2/R4 kept as diagnostics) |
| Clip drift / time-warp | Refuted — offset slope ≈ 0 vs gap time |
| "Skip was right" (uniqueness/residual funnel) | Superseded — wrong timescale (250 ms) + wrong residual test |

---

## Re-orientation — how the proven ideas fold into vocabulary and pipeline

**Vocabulary (descriptive; `gap-vocabulary-redesign` P2/P3).** Re-root on the axes (B1, B8, B2): a gap is
`{geometry, A-presence, donor-presence, shared-source, registration(offset+step), bracket-search,
envelope}`. Name the observed types (constant-offset / stepped-splice / alias-suspect / bracket-rescued /
tail / no-lag) from a clustered rescan — **but** register quiet gaps by outward-anchor (A2) so their
coordinates are trustworthy before clustering. W5 → "same-master, lag-0/bracket validation failed."

**Pipeline (detect → repair; `seam-splice-dualfit` §4).** Order:
1. **Settle registration (A1/A2/C2)** — outward-anchor for quiet gaps; decide the capture placement. *This
   comes before another rescan.*
2. **Detect** = `bracket_exhausted` (B1) ∧ both-sides-recoverable at the *correct* placement (A2, B3) ∧
   donor-continuous (A4). Do not run on already-patched gaps.
3. **Repair** = independent per-side fit at the anchored lags → trim/pad the step at the low-energy interior
   (B7) → **unchanged gate validates** (A3). Prove offline first, then wire behind a flag.
4. **Calibrate** thresholds (A5/C6) on the corrected corpus; **then** consider a rescan and the lag-width
   question (C4).

**Dual-fit addressable set (primary cohort):** **6 bracket-exhausted skips** are the candidates. Under the
old 250 ms metric ~**3** classed as clean splices and 3 as alias-suspect — **but which gaps are which is
placement-sensitive and shifts once A2 lands** (e.g. 6·g6 and 1·g19 read *one-sided-dead* at the F1 throat,
yet 6·g6 is recoverable at `b_mapped`). So the "3 vs 6" split is provisional: expect most of the 6 to become
addressable once quiet-gap registration (A2) + 1 s `peak_z` (A5/C6) are applied. Do not hard-code the
specific gap list from §1b — it predates F1/A2.

**One-line status:** measurement is largely *proven and frozen*; the live blocker is **registration for
quiet gaps (A1/A2)** — which gates both the next rescan and the repair — followed by the **unbuilt dual-fit
repair (A3)**. The one-sided-dead scare is resolved (B2).
