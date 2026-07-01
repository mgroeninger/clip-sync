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
Extended scans on disk: dirs 1–7 predate **`b_mapped` capture** (A2) — treat as exploratory until re-scanned
  with the current binary.
```

---

## A. The critical path (do these, in order)

The claims that actually gate a working repair. Everything else is supporting.

| # | Claim / task | Conf | Why it's the blocker |
|---|--------------|------|----------------------|
| A1 | **Quiet-gap mis-registration is `structure_start_frame` wander**, not decorrelation. Proven: pair-6 (5/5 one-sided-dead), pair-7 **7·g3** (pre 0.986@+94 ms, z 18) and **7·g4** (pre 0.902@+118 ms, post 0.988@+113 ms) — all dead at F1 throat, clean at `b_mapped`. | PROVEN | Diagnosis done. Capture fixed (A2); on-disk corpora need rescan. |
| A2 | **`b_mapped` registration** — center `baseline_lag` / detect metrics on geometry `b_mapped` nominal + existing ±200 ms lag sweep; **not** `structure_start_frame`. Outward-anchor (RMS loudest) is **not** the primary fix (pair-6 sweep). | **DONE (CAP)** | Landed in `gap_fingerprint.rs`. Rescan + skip re-classification next. |
| A3 | **Dual-fit repair** (independent per-side fit → length reconcile → unchanged gate passes) actually works. | OPEN (unbuilt) | This is the product. Prove offline (`diag_splice_dualfit` simulation) before wiring. **Re-classify the 6 skips after A2** — some may patch once registration is correct. |
| A4 | **Donor continuity** — B carries unbroken content across the hole. | OPEN / PARTIAL | Coded; now measured at `b_mapped` in capture. On-disk `donor_interior` still from pre-A2 scans — re-measure on rescan (C5). |
| A5 | **Threshold calibration** (`peak_z ≥ 12`, prominence, continuity) on the real distribution. | OPEN | Needs a **`b_mapped` rescan** (post A2). Calibrate on both patched and skipped distributions (C6). |

**Sequencing consequence:** on-disk `gap-files/` predates **`b_mapped` capture** — exploratory until re-scanned.
**Pair-7 spot-check done** (7·g3, 7·g4 — C2). **Next:** rescan primary cohort → **re-classify bracket-exhausted
skips** → **`diag_splice_dualfit`** (C3/A3).

---

## B. Proven — incorporate now (no more proof needed)

| # | Claim | Conf | Target | Incorporation |
|---|-------|------|--------|---------------|
| B1 | Patch vs skip = **bracket-search success, not step magnitude** (5·g3 vs 1·g19; full step overlap; best-bracket seam 0.62 vs 0.11) | PROVEN | VOCAB + PIPE | Vocab: `bracket_search` axis; W5 = "lag-0/bracket validation failed." Detect: scope dual-fit to `bracket_exhausted`. |
| B2 | **No genuine cross-encoding *type*** — `one-sided-dead` is a placement artifact. Pair-6: **5/5** @ `b_mapped` (~−131 ms). Pair-7 spot-check: **7·g3**, **7·g4** both shoulders 0.90+ @ +94 / +118 ms. | PROVEN | — | Refutes cross-codec validator; zero genuine one-sided-dead in all tested pairs. |
| B13 | **`b_mapped` + ±200 ms lag search** resolves quiet-gap registration — pair-6 and pair-7 (7·g3/7·g4) confirmed. | PROVEN | CAP | Policy implemented in `gap_fingerprint.rs`. |
| B3 | Uniqueness needs a **1 s window + `peak_z`** (retire 250 ms `second_peak_r`) | PROVEN | CAP (schema) | Decision frozen §3.6a. **Schema done, not corpus:** trustworthy population waits on A2 (`b_mapped` capture) + rescan; calibrate thresholds then (A5). |
| B4 | Level/SNR on **energy-weighted downmix** (straight mono `/N` buries 5.1 center 13–15 dB) | PROVEN | CAP (schema) | Frozen; schema done, corpus partial (as B3). |
| B5 | **Correlation on mono** (representation doesn't matter — Pearson scale-invariant) | PROVEN | CAP (schema) | Simplifies: no per-channel correlation. Schema done. |
| B6 | **F1 placement** — register at the gate's own throat, not a divergent `place_on_b` | PROVEN | PIPE (done) | Done via `gate_structure_align`. Quiet-gap registration is separate — **`b_mapped`** (B13/A2). |
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
| C1 | Does the **`one-sided-dead` bucket collapse** at `b_mapped`? | **PROVEN** | CRIT | Pair-6: 5/5. Pair-7: 7·g3, 7·g4. No further one-sided-dead pairs known on this corpus. |
| C2 | Which **placement** for registration? | **PROVEN** | CRIT | **`b_mapped`**. Pair-6 + pair-7 confirmed. RMS outward-anchor not primary (D10). |
| C3 | Does the **dual-fit repair pass the unchanged gate** on the known skips? | OPEN | CRIT | Offline `diag_splice_dualfit` after rescan + skip re-classification. |
| C4 | Is **±200 ms lag search** sufficient at `b_mapped`? | **PROVEN** | HIGH | Pair-6 ~−131 ms; pair-7 +94 ms / +118 ms — all inside ±200 ms, not edge-pinned. |
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

**Next CAP change (A2):** done — decision metrics register at **`b_mapped` nominal**; `residual` stays at gate
throat. Re-scan when ready.

**F1 (mostly done).** Registration metrics no longer use `oracle_throat_structure_frame`. **Remnant:** top-level `fp.structure` still comes from the summary pass's `place_on_b` and is not refreshed in the gate
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
1. **Rescan primary cohort** with `b_mapped` capture (dirs 1–6 or full set).
2. **Re-classify skips** via `diag_fingerprint_corpus` — bracket-exhausted set may shrink.
3. **Detect** = `bracket_exhausted` (B1) ∧ both-sides-recoverable at `b_mapped` (B3) ∧ donor-continuous (A4/C5).
4. **Repair** = `diag_splice_dualfit` (C3) → wire §4 behind flag (A3).
5. **Calibrate** thresholds (A5/C6).

**Dual-fit addressable set (primary cohort):** still **6 bracket-exhausted skips** until re-classified on a
`b_mapped` rescan. One-sided-dead is fully refuted (B2/C1) — not a separate rescue path.

**One-line status:** registration **closed** (C1/C2/C4/B13). **Live blocker:** rescan → skip re-classification →
**`diag_splice_dualfit`** (C3/A3).
