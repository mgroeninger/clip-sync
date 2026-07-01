# Seam-repair status ledger — proven / open / important (triage index)

**Purpose.** The two working docs
([TEMP-seam-splice-dualfit-plan.md](TEMP-seam-splice-dualfit-plan.md),
[TEMP-gap-vocabulary-redesign-plan.md](TEMP-gap-vocabulary-redesign-plan.md)) hold ~30 claims at every
stage of proof. This ledger is the **index over them**: one row per claim, scored **Confidence × Importance
× Target**, so we can see the critical path and what to incorporate. The two docs stay the detail; this is
the map. Update this when a claim's status changes.

**Legend.** Confidence: `PROVEN` (data) · `SUPP` (strong, small n) · `DECIDED` (policy chosen, not yet in code) · `OPEN` · `REFUTED`.
Importance: `CRIT` (blocks a working repair) · `HIGH` · `MED` · `LOW`.
Target: `VOCAB` · `PIPE` (detect/repair) · `CAP` (fingerprint capture) · `—` (conclusion/park/tombstone).

**Evaluation cohort (do not merge denominators).**
```text
Primary dual-fit cohort: the 6-pair corpus — 19 matched, 6 skipped. The 6 bracket-exhausted skips are the
  dual-fit targets. Counts in B1/B8 refer to this cohort.
Extended scans on disk: dirs 1–7 (dir 1 = 25 gaps; 69 gaps total) are a SUPERSET and are F1-placement
  (not yet `b_mapped` registration). Treat as EXPLORATORY — do not merge their rates with the primary cohort or use
  them to calibrate gates until capture registers at `b_mapped` (A2).
```

---

## A. The critical path (do these, in order)

The claims that actually gate a working repair. Everything else is supporting.

| # | Claim / task | Conf | Why it's the blocker |
|---|--------------|------|----------------------|
| A1 | **Quiet-gap mis-registration is a `structure_start_frame` wander**, not decorrelation. Flat envelopes let the structure search drift; the gross map (`b_mapped`) is stable. Proven on 6·g6, 7·g3, and all five pair-6 one-sided-dead gaps (dead at F1 throat, 0.98+ both sides @ `b_mapped`). | PROVEN | Diagnosis done. Capture still registers at `structure_start_frame` → wrong lags, false one-sided-dead, false `donor_interior`. |
| A2 | **`b_mapped` registration** — center `baseline_lag` / detect metrics on geometry `b_mapped` nominal + existing ±200 ms lag sweep; **not** `structure_start_frame`. Outward-anchor (RMS loudest) is **not** the primary fix (pair-6 sweep). | DECIDED (pair-6) · **CAP unbuilt** | Policy settled on pair-6; must land in `gap_fingerprint.rs` before a trustworthy rescan or detect. See §3.7. |
| A3 | **Dual-fit repair** (independent per-side fit → length reconcile → unchanged gate passes) actually works. | OPEN (unbuilt) | This is the product. Prove offline (`diag_splice_dualfit` simulation) before wiring. **Re-classify the 6 skips after A2** — some may patch once registration is correct. |
| A4 | **Donor continuity** — B carries unbroken content across the hole. | OPEN / PARTIAL | Coded + partially populated. `donor_interior` at `structure_start_frame` inherits A1 (6·g6 false "silent hole"). Re-measure at `b_mapped` post A2 (C5). |
| A5 | **Threshold calibration** (`peak_z ≥ 12`, prominence, continuity) on the real distribution. | OPEN | Needs a **`b_mapped` rescan** (post A2). Calibrate on both patched and skipped distributions (C6). |

**Sequencing consequence (important):** on-disk `gap-files/` scans are **F1-placement rescans** at
`structure_start_frame` — treat as exploratory for quiet gaps. **Next engineering step:** implement **`b_mapped`
registration in capture** (A2), spot-check pair-7 / other pairs if needed (C2), **re-classify bracket-exhausted
skips**, then **`diag_splice_dualfit`** (C3/A3). Rescan only after A2 lands. Do not wire RMS outward-anchor
into production capture (parked — §D10).

---

## B. Proven — incorporate now (no more proof needed)

| # | Claim | Conf | Target | Incorporation |
|---|-------|------|--------|---------------|
| B1 | Patch vs skip = **bracket-search success, not step magnitude** (5·g3 vs 1·g19; full step overlap; best-bracket seam 0.62 vs 0.11) | PROVEN | VOCAB + PIPE | Vocab: `bracket_search` axis; W5 = "lag-0/bracket validation failed." Detect: scope dual-fit to `bracket_exhausted`. |
| B2 | **No genuine cross-encoding *type*** — `one-sided-dead` is a placement artifact, not decorrelation. Pair-6 sweep: **5/5** one-sided-dead gaps recover at `b_mapped` (~−131 ms constant offset, both shoulders 0.98+). Earlier: 6·g6, 7·g3. | PROVEN | — | Refutes cross-codec validator (archived); same-master axis constant on this corpus. Zero genuine one-sided-dead in pair-6. |
| B13 | **`b_mapped` + ±200 ms lag search** resolves quiet-gap registration on pair-6 — ordinary centered lag at the gross map finds the peak; structure search was the failure mode. | PROVEN (pair-6) | CAP | Register detect/fingerprint lags at `b_mapped` nominal, not `structure_start_frame`. |
| B3 | Uniqueness needs a **1 s window + `peak_z`** (retire 250 ms `second_peak_r`) | PROVEN | CAP (schema) | Decision frozen §3.6a. **Schema done, not corpus:** trustworthy population waits on A2 (`b_mapped` capture) + rescan; calibrate thresholds then (A5). |
| B4 | Level/SNR on **energy-weighted downmix** (straight mono `/N` buries 5.1 center 13–15 dB) | PROVEN | CAP (schema) | Frozen; schema done, corpus partial (as B3). |
| B5 | **Correlation on mono** (representation doesn't matter — Pearson scale-invariant) | PROVEN | CAP (schema) | Simplifies: no per-channel correlation. Schema done. |
| B6 | **F1 placement** — register at the gate's own throat, not a divergent `place_on_b` | PROVEN | PIPE (done) | Done via `gate_structure_align`. F1 fixed gate-vs-capture divergence; **quiet-gap wander** is A1/B13 (`b_mapped` policy, CAP pending). |
| B11 | **Dual-fit ≠ what bracket search already does** — the winning bracket's boundary move is *not* the throat step (5·g3: +72 ms step vs 2600 ms `move_frames`; 0/18 patched gaps have `\|step\|` within 20 ms of a bracket delta) | PROVEN | PIPE | Confirms dual-fit is a distinct operation (interior length edit), not a re-run of anchor/boundary search. Scopes §4. |
| B12 | **Wide-envelope lag concordance** — 100 ms-bin envelope peak lag agrees with the fine-waveform lag | SUPP (pair 1) | CAP (schema) | Secondary registration confirmer; populate at `b_mapped` post A2. |
| B7 | **Content is un-stretched within a side** (both shoulders align at a single lag each) | SUPP | — | The premise that makes reconciliation a **pure trim/pad**, not a warp (A3). |
| B8 | Registration = **offset + step**, not clip drift (per-file slope ≈ 0; 18/19 have `|step|>2 ms`) | PROVEN | VOCAB | Registration axis; drop drift/skew framing. |
| B9 | Residual is the **wrong same-source test** for cross-encoded pairs (`informative=false` expected) | PROVEN | — | Keep as diagnostic; do not gate on it. |
| B10 | **Non-finite/residual-null serialization bug** (silent gaps → `null` → dropped whole pairs) | PROVEN + FIXED | CAP (done) | `finite_db`/`finite_corr`; analyzer tolerant. |

---

## C. Open + important — prove next (ranked)

| # | Question | Conf | Imp | How to prove |
|---|----------|------|-----|--------------|
| C1 | Does the **`one-sided-dead` bucket collapse** when registration uses `b_mapped`? | **PROVEN (pair-6)** | CRIT | Done: 5/5 pair-6 gaps (6·g2, 6·g6, 6·g7, 6·g9, 6·g10) recover @ `b_mapped`. Optional spot-check on other pairs if one-sided-dead appears. |
| C2 | Which **placement** for registration — `structure_start_frame`, `b_mapped`, or outward-anchor? | **DECIDED (pair-6)** | CRIT | **`b_mapped`** wins. Outward-anchor RMS loudest is not primary (6·g9/6·g10: z drops when anchor picks sustained tone). Light confirm on pair-7 (7·g3/7·g4 already partial). |
| C3 | Does the **dual-fit repair pass the unchanged gate** on the known skips? | OPEN | CRIT | Offline `diag_splice_dualfit` after A2 + skip re-classification. |
| C4 | Is **±200 ms lag search** sufficient at `b_mapped`? | **PROVEN (pair-6)** | HIGH | Pair-6 cluster ~−131 ms — well inside ±200 ms at correct placement. Revisit only if another pair pins at the edge. |
| C5 | **Donor continuity** true for the skip targets? (= A4, ranked) | OPEN / PARTIAL | HIGH | Re-measure at **`b_mapped`** post A2 capture — on-disk `donor_interior` mis-reads quiet gaps (6·g6). |
| C6 | **Threshold calibration** — `peak_z`/prominence/continuity floors on the real distribution. | OPEN | HIGH | **`b_mapped` rescan** (post A2). Calibrate on BOTH patched and skipped distributions. Detect gates ≠ patch gates. |
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
| D10 | **RMS outward-anchor as primary registration** | Pair-6 sweep: loudest ≠ most unique (6·g9 pre z 22→9, 6·g10 pre z 27→9 on sustained tones). `b_mapped` + centered lag already finds −131 ms. Keep `[outward-anchor]` in `diag_splice_timescale` as diagnostic only; if revived, select by **`peak_z` distinctiveness**, not RMS. |

---

## Capture parked (fingerprint layer hygiene)

Parked **CAP** items — not on the critical path until **`b_mapped` registration** (A2) lands.

**Next CAP change (A2):** move decision metrics (`baseline_lag`, `seam_probe`, `donor_interior`,
`wide_envelope`, `splice`) from `oracle_throat_structure_frame` to **`b_mapped` nominal** + ±200 ms lag
sweep. F1 fixed gate-vs-capture divergence; A2 fixes structure wander on quiet gaps.

**F1 (mostly done).** Today those metrics register at `oracle_throat_structure_frame` (structure-aligned).
**Remnant:** top-level `fp.structure` still comes from the summary pass's `place_on_b` and is not refreshed in the gate
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

**Vocabulary (descriptive; `gap-vocabulary-redesign` P2/P3).** Re-root on the axes (B1, B8, B2, B13): a gap is
`{geometry, A-presence, donor-presence, shared-source, registration(offset+step), bracket-search,
envelope}`. Name types from a **`b_mapped` rescan** (post A2). W5 → "same-master, lag-0/bracket validation failed."

**Pipeline (detect → repair; `seam-splice-dualfit` §4).** Order:
1. **Implement `b_mapped` registration in capture** (A2) — policy proven on pair-6 (B13). *Before another rescan.*
2. **Re-classify skips** — some bracket-exhausted gaps may patch once registration is correct; dual-fit only
   on those that remain exhausted.
3. **Detect** = `bracket_exhausted` (B1) ∧ both-sides-recoverable at `b_mapped` (B3) ∧ donor-continuous (A4).
4. **Repair** = independent per-side fit → trim/pad step (B7) → unchanged gate (A3). Prove via `diag_splice_dualfit` (C3).
5. **Calibrate** thresholds (A5/C6) on the `b_mapped` corpus.

**Dual-fit addressable set (primary cohort):** still **6 bracket-exhausted skips** until re-classified post A2.
Pair-6 one-sided-dead gaps are **not** a separate mechanism — they are **`b_mapped` registration failures**
at the F1 throat (B2). Expect the dual-fit candidate set to shrink once capture uses `b_mapped`.

**One-line status:** registration **policy decided** (`b_mapped`, pair-6 proven); **CAP implementation**
(A2) is the live blocker, then **skip re-classification** + **`diag_splice_dualfit`** (A3/C3). One-sided-dead
is fully refuted (B2).
