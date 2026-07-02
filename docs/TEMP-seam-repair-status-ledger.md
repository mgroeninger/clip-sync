# Seam-repair status ledger — proven / open / important (triage index)

**Purpose.** The working docs
([archive/TEMP-seam-splice-dualfit-plan.md](archive/TEMP-seam-splice-dualfit-plan.md) — mechanism + measurement history,
[TEMP-gap-vocabulary-redesign-plan.md](TEMP-gap-vocabulary-redesign-plan.md) — vocabulary,
[TEMP-pipeline-perf-redesign-plan.md](TEMP-pipeline-perf-redesign-plan.md) — pipeline perf/assembly, D12)
hold the claims/plans at every stage of proof. This ledger is the **index over them**: one row per claim,
scored **Confidence × Importance × Target**, so we can see the critical path and what to incorporate. The
**§4 wire spec** (dual-fit repair algorithm for A3) lives here; the dualfit plan doc is historical detail.
Update this when a claim's status changes.

**Legend.** Confidence: `PROVEN` (data) · `SUPP` (strong, small n) · `DECIDED` (policy chosen, not yet in code) · `OPEN` · `REFUTED`.
Importance: `CRIT` (blocks a working repair) · `HIGH` · `MED` · `LOW`.
Target: `VOCAB` · `PIPE` (detect/repair) · `CAP` (fingerprint capture) · `—` (conclusion/park/tombstone).

**Evaluation cohort (do not merge denominators).**
```text
PRIMARY — edge-pin/D11 rescan (dirs 1–7, real media, `gap-files/edge-pin-D11-added`, 2026-07-01): 7 pairs,
  62 matched (7 tail, 0 no-lag), 23 patched / 39 skipped (32 bracket-exhausted). Silence-splice view:
  alias-suspect 24 · one-sided-dead 8 · splice 23; both-sides-recoverable 23/55. **edge-pinned 0/55**
  (C4 confirmed — no peak clipped at ±600 ms; step_ms trustworthy corpus-wide). Occupancy: dropout 31 ·
  program-quiet 31; **24 skips reclassified program-quiet (D11)**. Dual-fit viability: **7/39 bracket-exhausted
  skips pass the gate** (6 real-step + 1 spurious 1·g21). **Clean A3 targets (gate_pass ∧ real-step ∧
  donor-continuous ∧ ¬program-quiet): 3 — 1·g3, 2·g2, 7·g2.**
  ⚠ These 7/39 and 3-target counts are from the **pre-seam-local-fix** `splice_dualfit` and are an
  **UNDERCOUNT**: that scan scored each seam at the 1 s-window `baseline_lag`, false-negativing seams whose
  seam-local lag diverges (e.g. 2·g1/11:50 pre recovers 0.982 @ +4.4 ms, 1·g22 pre 0.997 — both true targets).
  `splice_dualfit_at` now searches ±100 ms per shoulder (`SEAM_LOCAL_REFINE_MS`); **re-scan to re-derive.**
Prior (pre-analyzer-reclass, same dirs 1–7, 2026-07-01): alias-suspect 32 · splice 15; both-sides-recoverable
  15/55 — superseded by the peak_z-primary reclass (A5/B3), not a different scan.
Legacy: the pre-A2 6-pair corpus was 19 matched / 6 skipped. Counts in B1/B8 refer to that legacy cohort.
```

---

## A. The critical path (do these, in order)

The claims that actually gate a working repair. Everything else is supporting.

| # | Claim / task | Conf | Why it's the blocker |
|---|--------------|------|----------------------|
| A1 | **Quiet-gap mis-registration is `structure_start_frame` wander**, not decorrelation. Proven: pair-6 (5/5 one-sided-dead), pair-7 **7·g3** (pre 0.986@+94 ms, z 18) and **7·g4** (pre 0.902@+118 ms, post 0.988@+113 ms) — all dead at F1 throat, clean at `b_mapped`. | PROVEN | Diagnosis done. Capture fixed (A2); on-disk corpora need rescan. |
| A2 | **`b_mapped` registration** — center `baseline_lag` / detect metrics on geometry `b_mapped` nominal, **sequentially per-shoulder registered** (post search centered on `S + D_A + round(L_pre)`, not the naive `S + D_A`; gross lags `L_post_gross = L_pre + L_post_fine` ≈ bridge-length mismatch); **not** `structure_start_frame`. Outward-anchor (RMS loudest) is **not** the primary fix (pair-6 sweep). | **PROVEN (CAP)** | Gross map + ±600 ms sequential centering landed in `gap_fingerprint.rs` (`lag_pair`, `seam_probe_at_placement`, `wide_envelope_at_placement`, `donor_interior_at`); lib tests green. **Validated on full-corpus rescan (dirs 1–7, real media, 2026-07-01):** one-sided-dead collapsed 27/55 → **8/55**, confirming the fix. |
| A3 | **Dual-fit repair** (independent per-side fit → length reconcile → unchanged gate passes) actually works. | **PROVEN in principle (unbuilt) — safe to wire** | §4 repair not wired, but the offline proof is in: scan-native `splice_dualfit` shows **7/39 skips pass the unchanged gate** (6 genuine splices; C3). The gate-pass precondition is met ⇒ **wiring §4 is now justified.** Scope the repair to the passing cohort (`splice_dualfit.gate_pass ∧ post_seam_global_r < floor` for real-step; exclude donor-BROKEN / program-quiet). **Measured target set (edge-pin/D11 rescan, 2026-07-01): 3 gaps — 1·g3, 2·g2, 7·g2** (small-step, continuous-donor splices). **DECISION — scope on `gate_pass ∧ donor-continuous`** (measured seam viability), with the **seam registered locally** (below). Uniqueness (`both_sides_recoverable`) is a diagnostic proxy, not the gate. **CORRECTION (2026-07-01, ground-truth check on 2·g1/11:50):** the earlier "5 false positives of `dualfit_candidate`" claim was **overstated** — 2 of those 5 (**2·g1, 1·g22**) are *true* targets that `splice_dualfit` **false-negatived** by scoring the 250 ms seam at the shoulder's 1 s-window `baseline_lag` instead of the seam-local lag (2·g1 pre: 1 s lag −24 ms → seam dead −0.008, but seam-local +4.4 ms → 0.982). Only **1·g5, 5·g6, 7·g4** are genuine uniqueness false-positives (pre shoulder weak even seam-locally, ≤0.37). Neither existing field was a faithful seam-local gate (`seam_probe` pre is ±25 ms → misses 1·g3's +49 ms; `baseline_lag` is 1 s → misses 2·g1's divergence). **FIX landed:** `splice_dualfit_at` now searches ±`SEAM_LOCAL_REFINE_MS` (100 ms) per shoulder and scores at the peak — a **capture change, needs a rescan** to re-derive C3 / the target set (expected to grow past 3). |
| A4 | **Donor continuity** — B carries unbroken content across the hole. | OPEN / PARTIAL | Coded; now measured at `b_mapped` in capture. On-disk `donor_interior` still from pre-A2 scans — re-measure on rescan (C5). |
| A5 | **Threshold calibration** (`peak_z ≥ 12`, prominence, continuity) on the real distribution. | OPEN — first calibration landed | Needs a **`b_mapped` rescan** (post A2). Calibrate on both patched and skipped distributions (C6). **2026-07-01 finding (dirs 1–7):** of 32 alias-suspect, only **21 fail `peak_z`** (genuinely ambiguous); **9 fail prominence-only** while `peak_z` says unique (z 12.6–26) — and **4 of those 9 were gate-*patched*** (1·g6, 1·g20, 5·g2, 6·g4), proving the flag false. The 0.45 prominence floor undercut B3's `peak_z`-primary decision. **Action taken:** `splice_diag` now makes `peak_z` primary and demotes prominence to a low-floor tiebreaker (**0.45 → 0.15**), catching only true near-duplicate rivals (6·g9 `prom 0.11`). Re-classify (analyzer-only, no rescan): both-sides-unique **15 → 23**. **`peak_z = 12` — keep for now:** gate patch/skip is an *invalid* label for it (peak_z-by-outcome is **inverted** — patched median z 6.8, skipped 21.7 — because patches don't need unique per-shoulder registration while confident splices get skipped). The earlier "5·g1/g3 patched at z 9.3 ⇒ floor too strict" note was wrong (patches aren't dual-fit targets). For the **skip** cohort, z p25 = 11.1, so 12 sits in the discrimination band. Tune only against `splice_dualfit` seam viability on skips (post-rescan). **Edge-pinned flag — IMPLEMENTED (2026-07-01, no-production):** `LagSummary.edge_pinned` (peak within [`LAG_EDGE_TOL_MS`] = 2 ms of the searched boundary, read from the curve extremes so high-side masking is honored) + `SpliceSummary.edge_pinned` (either shoulder) in `gap_fingerprint.rs`; analyzer surfaces `GapRow.splice_edge_pinned` / `step_edge_pinned()` and **excludes edge-pinned steps from `dualfit_candidate`** (GIGO guard) + a count in `splice_text`. Backward-compatible (old corpora → `None` → "re-scan to populate"). **VALIDATED on the edge-pin/D11 rescan (2026-07-01): 0/55 edge-pinned** — no peak clipped at ±600 ms, so `step_ms` is trustworthy corpus-wide and the guard excluded nothing (positive confirmation of C4; the flag is a safety net, not currently active). **New calibration finding (edge-pin/D11 rescan):** 1 s-window `both_sides_recoverable` does **not** predict 250 ms-placement seam viability (5 false positives / 1 false negative vs `splice_dualfit.gate_pass`; see A3). ⇒ **calibrate the repair predicate against `gate_pass ∧ donor-continuous` directly, not uniqueness floors.** |

**Sequencing consequence:** the 2026-07-01 rescan validated registration (A2/C1/C2/C4), but predates the
scan-native **`splice_dualfit`** field (added 2026-07-01, after the rescan) — so C3/C7 need one more re-scan.
**`diag_splice_dualfit` sim retired** (E-tombstone: decode ≠ scan). **Next:** re-scan one pair → read the
`dual-fit viability` section (C3/C7) → full re-scan → calibrate thresholds (A5/C6) → wire §4 repair (A3).

---

## §4. Dual-fit repair — wire spec (A3, unbuilt)

Condensed from [archive/TEMP-seam-splice-dualfit-plan.md](archive/TEMP-seam-splice-dualfit-plan.md) §4 — the production
algorithm to wire behind a flag once perf work (D12) is ready. Viability is **already proven in-scan** via
`GapFingerprint.splice_dualfit` (`splice_dualfit_at` in `gap_fingerprint.rs`); this section is what the repair
path must *do*, not re-prove.

**Mechanism.** At a repair gap, A has a quiet/silent hole between two un-stretched shoulders. Each shoulder
registers against B at its **own lag**; the lags differ by a **step** (`splice.step_ms`). A single rigid donor
shift cannot satisfy both seams. Dual-fit places each shoulder independently, then reconciles the step with a
**trim or pad at the lowest-energy interior sample** of the fill — a pure length edit, not a within-side warp
(B7/B11).

**Algorithm (per gap):**

1. **Detect** — run only on gaps that **`dualfit_target()`** selects (analyzer: `gap_fingerprint_corpus.rs`):
   `skip` ∧ bracket-exhausted ∧ `splice_dualfit.gate_pass` ∧ `post_seam_global_r < 0.35` (step is real, not a
   constant-offset artifact) ∧ `donor_interior.continuous` ∧ ¬program-quiet. **Do not** run on gaps that
   already patch (≥1 bracket passes) or on `dualfit_candidate()` alone — uniqueness does not predict 250 ms
   seam viability (A3). Measured scope on the edge-pin/D11 rescan: **1·g3, 2·g2, 7·g2**.
2. **Fit each seam independently** — read per-side lags `L_pre`, `L_post` from `baseline_lag` mono at
   **`b_mapped`** (sequential post centering: post search on `S + D_A + round(L_pre)`, not naive `S + D_A`).
   Align B shoulders: `b_pre = b_mapped_start + round(L_pre)`, `b_post = b_mapped_end + round(L_post_gross)`.
3. **Reconcile the step** — extract the B bridge `[b_pre .. b_post]`; `trim_frames = bridge_frames − gap_frames`
   (= the step in samples; C7 tautological). Trim or pad `|trim_frames|` at the **lowest-RMS interior sample**
   of the fill region (smallest audible splice). Interior edit only — shoulders stay at their own lags.
4. **Validate with the unchanged gate** — score pre/post seams @ lag 0 against B at the lag-aligned positions,
   using `fill_seam_search_secs` (default 250 ms) and the existing `min_fill_correlation` / `fill_absolute_floor`
   thresholds. A bad length edit must fail exactly as a bad shift does today — **strict gate, no loosening**.
   This is the property `splice_dualfit` already measures at capture time.
5. **Reject** to skip (as today) if post-reconciliation validation fails, the step was edge-pinned (GIGO), or
   donor continuity is false. Gate-pass alone is not sufficient — donor-BROKEN gaps (e.g. 1·g19: seams 0.998
   but B interior silent) must stay skipped (D11).

**Wiring notes.**

- **Flag-gated** — dual-fit off ⇒ existing bracket-search path unchanged (D6).
- **Pre-wire proof** — read `dualfit_viability_text()` / `splice_dualfit` in corpus JSON; offline
  `diag_splice_dualfit` is **retired** (E-tombstone — decode drifted from scan).
- **Schema reference** — field semantics in [gap-fingerprint.md](gap-fingerprint.md) § Registration & dual-fit.
- **Historical detail** — corpus tables, measurement rationale, and the retired offline sim live in
  [archive/TEMP-seam-splice-dualfit-plan.md](archive/TEMP-seam-splice-dualfit-plan.md).

---

## B. Proven — incorporate now (no more proof needed)

| # | Claim | Conf | Target | Incorporation |
|---|-------|------|--------|---------------|
| B1 | Patch vs skip = **bracket-search success, not step magnitude** (5·g3 vs 1·g19; full step overlap; best-bracket seam 0.62 vs 0.11) | PROVEN | VOCAB + PIPE | Vocab: `bracket_search` axis; W5 = "lag-0/bracket validation failed." Detect: scope dual-fit to `bracket_exhausted`. |
| B2 | **No genuine cross-encoding *type*** — `one-sided-dead` is (mostly) a placement artifact. Pair-6: **5/5** @ `b_mapped` (~−131 ms). Pair-7 spot-check: **7·g3**, **7·g4** both shoulders 0.90+ @ +94 / +118 ms. | **PROVEN** | — | Full-corpus rescan (dirs 1–7, real media, post sequential-fix, 2026-07-01): one-sided-dead collapsed **27/55 → 8/55 (49% → 14.5%)**. The 8 residual cases have genuinely dead shoulders (`peak_r ≤ 0.17`) at large steps (±300–600 ms) even under ±600 ms sequential search — a real floor, not a placement artifact. Corpus-wide PROVEN restored for the collapse; ~15% is an unrecoverable residual, not a cross-encoding *type*. |
| B13 | **`b_mapped` + sequential ±600 ms lag search** resolves quiet-gap registration — pair-6 and pair-7 (7·g3/7·g4) confirmed. | **PROVEN** | CAP | `b_mapped` pre anchor + sequential post centering implemented (A2); ±200 ms → ±600 ms widened. Post centering bug (naive `S + D_A`, stacking `L_pre` into the post search) fixed and **validated on the full-corpus rescan** (2026-07-01): registration resolves for 47/55 (one-sided-dead 8/55). |
| B3 | Uniqueness needs a **1 s window + `peak_z`** (retire 250 ms `second_peak_r`) | PROVEN | CAP (schema) | Decision frozen §3.6a. **Schema done + honored in the classifier (2026-07-01):** `splice_diag` had been OR-ing a high prominence floor (0.45) back in, over-flagging `peak_z`-unique gaps as alias-suspect (9/32; 4 gate-patched — see A5). Now `peak_z`-primary, prominence a low-floor (0.15) tiebreaker. `peak_z` confirmed periodicity-robust on leveled content (the whole-curve z-score deflates on periodic signals; prominence, a single-rival term, did not). |
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
| C1 | Does the **`one-sided-dead` bucket collapse** at `b_mapped`? | **PROVEN** | CRIT | **Yes.** Full-corpus rescan (dirs 1–7, real media, post sequential-fix, 2026-07-01): one-sided-dead **27/55 → 8/55** (49% → 14.5%). 19 gaps that were window-placement artifacts now recover both shoulders. 8 residuals are genuinely dead (large steps, `peak_r ≤ 0.17`) — the real floor, not artifacts. |
| C2 | Which **placement** for registration? | **PROVEN** | CRIT | **`b_mapped`**, sequentially per-shoulder registered (post centered on measured `L_pre`, not the naive `S + D_A`). Pair-6 + pair-7 confirmed the placement choice; RMS outward-anchor not primary (D10). |
| C3 | Does the **dual-fit repair pass the unchanged gate** on the known skips? | **PROVEN — yes, for a real subset** | CRIT | **Scan-native `splice_dualfit` (full corpus, 2026-07-01, on the scan's own decode): 7/39 bracket-exhausted skips pass the gate** at per-shoulder placement — **6 need the step (genuine silence-splices), 1 is a constant offset** (1·g21). Passes: 1·g3, **1·g19**, 1·g21, 2·g2, 6·g3, 6·g8, 7·g2. **Refutes the sim's decode-tainted "steps spurious" claim:** 1·g19 (step **+315.8 ms**) reads **both seams 0.998** on the scan decode (the sim had called it dead/common-offset). Genuine large-step splices exist and dual-fit clears the unchanged gate on them. Failures split into *one-shoulder-dead-at-seam* (1 s lag aligns, 250 ms seam dead one side — splice at the edge) and *donor BROKEN*; the latter will further split into program-quiet non-dropouts (D11) once the nominal-donor re-scan lands. Sim retired (E-tombstone). **CONFIRMED on the edge-pin/D11 rescan (2026-07-01, donor now populated):** the same 7/39 pass (6 real-step: 1·g3, 1·g19, 2·g2, 6·g3, 6·g8, 7·g2; 1·g21 spurious). **Donor gating splits the 6 real-step passes in half:** continuous → **1·g3, 2·g2, 7·g2** (the clean A3 targets); donor-BROKEN → 1·g19, 6·g3, 6·g8 (seams align but the gap interior is silent — filling inserts silence, correctly excluded). **1·g19 is the sharp lesson:** seams 0.998 yet donor-BROKEN ⇒ gate-pass is necessary, not sufficient — donor occupancy MUST gate (validates A4/D11). **⚠ 7/39 is a pre-seam-local-fix UNDERCOUNT (2026-07-01):** that `splice_dualfit` scored each seam at the 1 s-window `baseline_lag`, so it false-negatived seams whose seam-local lag diverges (ground-truth: 2·g1/11:50 is fixable — pre recovers 0.982 @ +4.4 ms — as is 1·g22 @ 0.997). `splice_dualfit_at` now does a ±100 ms per-shoulder seam search (`SEAM_LOCAL_REFINE_MS`); **re-scan to re-derive C3** (pass count expected to rise). |
| C4 | Is **±600 ms sequentially-centered lag search** sufficient at `b_mapped`? | **PROVEN** | HIGH | **Yes, for the recoverable population.** Full-corpus rescan (2026-07-01): 47/55 register within the ±600 ms sequential window (both-sides-recoverable 15/55 + alias-suspect 32/55). Sequential centering decouples `L_pre`; residual post lags now measure `\|D_B − D_A\|` alone. The 8 one-sided-dead are dead at the shoulder itself (`peak_r ≤ 0.17`), not clipped by the window — widening won't recover them. |
| C5 | **Donor continuity** true for the skip targets? (= A4, ranked) | OPEN / PARTIAL | HIGH | Re-measure at **`b_mapped`** post A2 capture — on-disk `donor_interior` mis-reads quiet gaps (6·g6). **Footgun:** when the post lag does not resolve, capture falls back to the naive A-length span (`b_mapped_start + gap_frames`) for `donor_interior` — prefer `donor_interior_nominal` (registration-independent) for occupancy reads (D11). |
| C6 | **Threshold calibration** — `peak_z`/prominence/continuity floors on the real distribution. | OPEN — prominence floor calibrated | HIGH | Calibrate on BOTH patched and skipped distributions. **First pass (2026-07-01, analyzer-only):** prominence floor 0.45 → 0.15 (was flagging `peak_z`-unique, gate-patched gaps as alias-suspect; see A5/B3). **Remaining:** `peak_z` floor (keep 12) / `SPLICE_MIN_PEAK_R` (0.85) are now **diagnostic-only** — the repair scopes on `gate_pass ∧ donor-continuous`, not uniqueness (A3), so these no longer gate the fix. **Light calibration DONE (edge-pin/D11 rescan, 2026-07-01):** all repair-scoping thresholds sit in wide bimodal gaps → keep current values. `PROGRAM_QUIET_SILENCE_FRAC = 0.5` (dropouts ≈0 vs program-quiet cluster ≥0.83; any value in ~[0.1,0.8] works). `DUALFIT_STEP_SPURIOUS_R = 0.35` (6 real-step passes ≤0.284 `post@pre-off`, spurious 1·g21 = 0.759). `edge_pinned` **VALIDATED 0/55** (no GIGO). Donor-continuity (`DONOR_CONTINUITY_MS = 150`) cleanly splits the 6 gate-passes 3 cont / 3 BROKEN, agreeing with the nominal-silence read. **`dualfit_target()` implemented** — encodes `skip ∧ bracket-exhausted ∧ gate_pass ∧ step-real ∧ donor-continuous ∧ ¬program-quiet`; yields the 3-gap A3 scope (1·g3, 2·g2, 7·g2). |
| C7 | **Trim magnitude ≈ measured `step_ms`** | **RESOLVED (tautological in-scan)** | HIGH | Scan-native `splice_dualfit` places shoulders at their own lags, so `trim_frames = bridge − gap = step` **by construction** (no separate decode to disagree). C7 is no longer an open reconciliation risk; the open question is now *seam viability* (C3) + whether the step is *real* (new validator: `post_seam_global_r`). |

---

## D. Open + low / parked (do not spend cycles yet)

| # | Item | Why parked |
|---|------|-----------|
| D1 | **Mechanism of the step** (silence-splice vs resampler vs PTS; sub-frame, not quantized) | The repair *measures* the step; the physical cause doesn't change the fix. Interesting, not blocking. |
| D2 | **Decorrelated / different-content regime** | Untestable directly — this corpus is all same-master. But same-master **decoys** can stand in for different-content negatives (see **D8**: mine periodic/alias-suspect placements; construct level-matched substitution fills). Revisit with genuine different-content data when available. |
| D3 | **Channel-scope / donor-displacement axes** (vocab §2b) | Surface in analyzer later; not decision-relevant for dual-fit. |
| D4 | **Keep vs deprecate W-tiers**; reconcile `gap_tags.rs`/`content_hint`/`seam_shape` | Vocab P3/P4 decision; after the type set is named. |
| D5 | **Perf** (FFT lag, dedup search, decode reuse) | **Now owned by [TEMP-pipeline-perf-redesign-plan.md](TEMP-pipeline-perf-redesign-plan.md) (D12).** FFT lag sweep scoped (~50–150×, `rustfft` present, gate on `fft≈naive` test) — full spec in **Capture parked → FFT lag sweep** below, migration ordering in the perf doc §3. |
| D6 | **No regression on existing patches** (dual-fit flag off ⇒ unchanged) | Verify after A3 (repair built) — a run-comparison, not an open question yet. |
| D7 | **Audibility of the trim point** (splice at low-energy interior sounds clean) | After A3; gate-pass is necessary, not sufficient (needs a listen). |
| D8 | **Decoy / wrong-placement safety** (a deliberately wrong B fill still fails the gate) — the corpus has **no genuine negatives** (all same-master), the biggest blind spot the re-scan can't fix. | After A3. **A fair decoy must pass structure but fail the seam** (else it tests nothing): a *too-different* decoy (cross-pair/noise/silence) fails structure trivially; a *matching-shoulders/wrong-interior* decoy is **not a fair test** — A's gap is empty, so B's interior is unverifiable from A (accepting it is a limit, not a gate bug). **Offset-perturbation doesn't work** — the structure/lag search self-corrects a metadata lie; you must change the *audio* so the correct answer isn't available. **Construction (two ways):** (A) **Mine** — periodic/repeated content yields structurally-similar-but-fine-wrong placements for free; the **alias-suspect cluster (pair 6)** *is* a set of natural decoys — offer a repeated-phrase location as a fill candidate and check the seam rejects. (B) **Construct** — on a known-good fillable gap, overwrite B's fill region (mapped span **and** shoulders) with a different, **active, level-matched** passage of the *same* B (single-master ⇒ true content is unique ⇒ search can't route around it); a correct gate flips fill→skip. **Make it a margin:** sweep decoy content-distance (near-repeat → distant) to map the seam's discrimination boundary = the reject-safety margin / headroom on the 0.35 floor. Start with mining (free); build substitution only for the parameterized margin. |
| D9 | **Fingerprint diagnostic stubs** (F2/F3) | Gate path omits per-bracket `structure_*` and leaves `GateOutcome` vocabulary tags empty. Fine for diagnostics today. See **Capture parked**. |
| D10 | **RMS outward-anchor as primary registration** | Pair-6 sweep: loudest ≠ most unique (6·g9 pre z 22→9, 6·g10 pre z 27→9 on sustained tones). `b_mapped` + centered lag already finds −131 ms. Keep `[outward-anchor]` in `diag_splice_timescale` as diagnostic only; if revived, select by **`peak_z` distinctiveness**, not RMS. |
| D12 | **Pipeline performance redesign** — the detect→gate→fingerprint path grew for *exploration*, never reviewed for throughput. | **Own doc: [TEMP-pipeline-perf-redesign-plan.md](TEMP-pipeline-perf-redesign-plan.md).** §1 audit done (2026-07-01): gate inventory, measurement→gate map labeling each field **decision/repair/diagnostic**, cost hierarchy, overlaps. Key finding — two paths (lean production `PatchAudio` vs diagnostic `characterize_gaps_with_gate` that runs *every* measurement per gap); the diagnostic-only set (`seam_probe`, `wide_envelope`, diag `lag`, `b_levels`) is computed unconditionally and belongs behind a flag. Absorbs D5 + the "Perf" capture-parked block. §2–§4 (target/migration/validation) scaffolded, not decided. |
| D11 | **Donor-silent gaps = program-quiet, not fillable dropouts** (classify, don't count as fill misses) | **2026-07-01 finding (pair 6, archived corpus).** The gaps split cleanly by B-side occupancy: *real dropouts* (g4/g5/g12/g13 — A deep-silent `gap_floor −83…−99`, **B occupied** `silence 0%`, continuous, small step) **patch**; the *skip cluster* (g1/g2/g3/g6/g8/g9/g10/g11 — A quiet-at-noise-floor `gap_floor ≈ −77`, **B also silent** `silence 89–100%`, discontinuous, large step +50…+419) is **program-quiet present in both masters** → nothing to fill → correctly skipped. **Not a repair failure.** `donor_interior.silence_fraction`/`continuous` already carries the signal. **Classifier IMPLEMENTED (2026-07-01, no-production):** analyzer `program_quiet_skip()` / `addressable_dropout()` (registration-independent `donor_interior_nominal.silence_fraction ≥ PROGRAM_QUIET_SILENCE_FRAC = 0.5`); `dualfit_candidate` now **excludes** program-quiet, and `dualfit_scope_text` drops them from the addressable-skip denominator (tags rows `program-quiet`). Live b-levels rescan (partial): **24 skips reclassified out**, incl. *recoverable* ones (1·g9 step 262 ms, 1·g10/g11/g12) that the pre-D11 predicate would have mis-targeted — the exact value D11 predicted. Threshold `0.5` is a const, calibrate at C6. Caveat: "B silent" has two readings — genuine program-quiet vs aliased registration landing on a silent B spot (large step + low `peak_z` is the alias signature) — same skip outcome either way; `splice_dualfit` on rescan disambiguates. Refutes my earlier "high gap floor = occupied busy ambience" — B is *silent*, so it's quiet-in-both, not occupied-in-both. |

---

## Capture parked (fingerprint layer hygiene)

Parked **CAP** items — not on the critical path until a **`b_mapped` rescan** is worth running.

**Next CAP change (A2):** done — decision metrics register at **`b_mapped` nominal**; `residual` stays at gate
throat. Re-scan when ready.

**F1 (mostly done).** Registration metrics no longer use `oracle_throat_structure_frame`. **Remnant:** top-level `fp.structure` still comes from the summary pass's `place_on_b` and is not refreshed in the gate
overlay; corpus `structure_min` stats may disagree with the oracle throat. `fp.seams.baseline_*` is updated
from the zero-move oracle bracket.

| # | Item | Status | When to fix |
|---|------|--------|-------------|
| F2 | Gate `brackets[]` write `structure_pre/post = None` (oracle has structure internally) | OPEN | Only if analyzer needs per-bracket structure or schema/docs parity |
| F3 | `GateOutcome.seam_shape` / `fit_path` / `signature_mode` empty in gate path | OPEN | Only if vocabulary tags migrate into fingerprints |
| C-docs | `gap-fingerprint.md` omits `baseline_lag`, `seam_probe`, `splice`, `donor_interior`, `wide_envelope` | **DONE (2026-07-01)** | Shape table + new *Registration & dual-fit measurements* section document `baseline_lag` (`b_mapped`, `peak_z`/`prominence`/`edge_pinned`), `splice`, `donor_interior`(`_nominal`), `splice_dualfit`, `wide_envelope`, `seam_probe`, `residual`, `b_levels` — each with its placement provenance. |
| C-harness | `uniqueness_z` single-sided (slightly optimistic) | **DONE (2026-07-01)** | Two-sided `both()` — `uniqueness_z` is `min(pre,post)` only when BOTH shoulders carry `peak_z`, else `None` (no fabricating a both-sides value from one). |
| C-harness-2 | **`verdict` / `skew` use one side** — `gap_row` took `pre.or(post)` for the headline verdict and constant-vs-drift skew | **DONE (2026-07-01)** | `skew` now requires **both** shoulders present and both `timing_offset` (else `NotApplicable`); `verdict` doc marks it one-sided/pre-preferred and points classification at the two-sided `splice_diag()` / `seam_step_ms()`. |
| C-harness-3 | **Legacy `gap.lag` fallback** — analyzer uses `baseline_lag.or(gap.lag)` | **DONE (2026-07-01)** | Rows that fall back are flagged (`registration_from_legacy_lag`) and `summary_text` warns loudly on any pre-/post-A2 schema mix (different placement — not comparable). Fallback retained for pre-A2 corpora. |
| B-level | **Symmetric B-side level** — capture is asymmetric: full `LevelProfile` on A (speech_peak/noise_floor/contour) but only `donor_interior` (RMS/silence/continuity) on B. | OPEN (D11) | **Validation instrument, not a required production cost.** Use a full B `LevelProfile` to *confirm* the donor-silent ⇒ program-quiet hypothesis (is B quiet relative to its own noise floor, at the correct registration?). **Cheap — does NOT double gap-exam cost:** B is already decoded (for the lag sweep/donor); a level profile is an `O(N)` RMS-bin pass, negligible next to the lag-correlation sweep (the dominant cost). **Production test likely needs neither the sweep nor the full profile:** a **B RMS/silence check at the *nominal* geometry `b_mapped` span (no lag adjustment)** is registration-independent (dodges the alias confound), `O(N)`, and directly answers "is B quiet at the same program time" — pair with A `gap_floor` vs A `noise_floor`. Symmetric profile = calibrate/validate; ship the cheap nominal-span silence test. |

**Perf (before a long rescan).** Dominant cost is still N × oracle bracket scoring (required). Avoidable
overhead today: (1) summary `characterize_gaps` still runs one `place_on_b` before the gate overlay; (2)
diagnostic `fp.lag` at the best-energy bracket adds another `place_on_b` + `lag_at_placement`; (3)
`dump_gap_fingerprints` re-decodes A/B after repair. Likely wins when rescans matter: drop summary
`place_on_b` when gate follows; share one border extract at the throat for lag + probe + wide-envelope;
reuse repair decode; **FFT lag sweep** (below).

**FFT lag sweep (`lag_correlation_curve`) — the biggest single win (~50–150×).** `lag_correlation_curve`
(`gap_fingerprint.rs`) is naive `O(n·L)`: one 1 s Pearson (`n ≈ 48k`) at every integer lag over ±600 ms
(`L ≈ 57.6k`) → ~2.8·10⁹ ops **per shoulder**. FFT drops it to `O(M log M)`, `M = n+L`. This is the
dominant scan cost (registration sweep); `peak_z`/`prominence` piggyback on the same curve for free, so
speeding the sweep speeds them too.
- **Primitive already present:** `rustfft` (`FftPlanner`) is used in `domain/seam_robust.rs` — reuse that.
- **The catch — it's *normalized* (Pearson), not raw cross-correlation.** FFT only accelerates the
  numerator `c(lag) = Σ a[i]·b[i+lag]` (= `ifft(conj(FFT(a_pad))·FFT(b_pad))`, zero-pad to `n+L`). The
  **denominator (sliding b-window mean/var) is prefix sums, NOT FFT** — precompute `cumsum(b)`/`cumsum(b²)`,
  window stats O(1)/lag; `a` stats fixed. Assemble `Pearson(lag) = (c − n·mean_a·mean_b) / (n·std_a·std_b)`.
  Forgetting the prefix-sum normalization is the classic bug.
- **Must match the naive path exactly:** the lag convention (`base = max_lag + lag`, `b_ctx[base..base+n]`)
  **and** the edge-lag mask (naive *skips* lags where `base+n > len` — test asserts `curve.len() < 2L+1`).
  `peak_z` is a whole-curve mean/std, so an off-by-one in the included-lag set shifts it.
- **Calibration-neutral IF gated:** f64 rustfft is ~1e-10 relative — negligible for `r`/`peak_z`/`prom`/
  `frac_lag`, and the peak lag is robust. The *only* way it drifts the z=12 / prom=0.15 floors (A5/C6) is a
  porting bug → gate behind a **regression test: `fft_curve ≈ naive_curve` within tight ε** (assert
  `peak_z`/`prominence`/`frac_lag` specifically).
- **Keep a naive fallback for small curves:** the same fn runs the ±25 ms seam probe and 100 ms-bin envelope
  (tiny `L`) where FFT overhead loses — auto-select by `n·L`.
- **Sequencing:** land it *after* the dual-fit rescan + A5/C6 threshold calibration — behavior-preserving,
  but you want a stable naive baseline to write the equivalence test against, not to change the engine
  under a metric mid-calibration. Est. ~1 day incl. tests.

**Do not optimize first:** `donor_interior` RMS; parallel per-gap loops before deduping search and aligning
placement.

### Capture footguns (measurement / analyzer hygiene)

Read measurements at the placement each field defines — `legend_text()` in `diag_fingerprint_corpus` is the
authoritative map. Key traps:

| Trap | Status | What to do |
|------|--------|------------|
| **Mixed lag widths** | by design | `baseline_lag` ±600 ms (classification); `seam_probe` pre ±25 ms / post ±600 ms; `wide_envelope` pre ±400 ms / post ±600 ms. Prefer **`splice_diag` / `splice_text`** over `seam_diag` / `seam_probe_text` for skip triage. |
| **`fp.lag` vs `baseline_lag`** | by design | Diagnostic `fp.lag` sits at the structure throat; decision registration is at **`b_mapped`**. Never compare interchangeably (F1). |
| **`oracle_throat_structure_frame` vs `b_mapped`** | by design | Gate throat is for bracket/seam scoring; `baseline_lag` / `splice` / `donor_interior` register at **`b_mapped`**. |
| **Edge-pinned peaks** | IMPLEMENTED (needs rescan) | Flag lands: `LagSummary`/`SpliceSummary.edge_pinned` (peak within 2 ms of the searched boundary); analyzer `step_edge_pinned()` excludes it from `dualfit_candidate` and `splice_text` counts it. `splice.step_ms` is now labeled GIGO when set. Coarse post placement only if the rescan shows systematic edge-pins (C4: not needed for recoverable population). |
| **`lag0_r` when gross-shifted** | OPEN, low | `LagVerdict` / `lag0_r` can be wrong when the gross-shifted curve omits lag 0 — `splice_diag` unaffected. |
| **`dualfit_candidate` (uniqueness) is a diagnostic proxy, not the gate** | measured (edge-pin/D11 rescan) | It diverges from `splice_dualfit.gate_pass`, but the divergence was **mostly a `splice_dualfit` placement bug, not a uniqueness failure** (A3 CORRECTION): of the apparent 5 mismatches, 2 (**2·g1, 1·g22**) were `splice_dualfit` false-negatives (seam scored at the 1 s lag), now fixed by the ±100 ms seam-local search; only 3 (1·g5, 5·g6, 7·g4) are genuine uniqueness over-flags. **Scope A3 on `gate_pass ∧ donor-continuous`** (via `dualfit_target()`), computed on the **seam-local-fixed** `splice_dualfit` (re-scan pending). |

---

## E. Refuted — tombstone (do not revive)

| Hypothesis | Verdict |
|------------|---------|
| Per-seam detect-and-warp rescue | Refuted / archived — step is local, content un-stretched |
| Cross-codec validator-swap (R2/R4 loosen the gate) | Refuted — measurement artifact; plan archived (R2/R4 kept as diagnostics) |
| Clip drift / time-warp | Refuted — offset slope ≈ 0 vs gap time |
| "Skip was right" (uniqueness/residual funnel) | Superseded — wrong timescale (250 ms) + wrong residual test |
| **`diag_splice_dualfit` sim (offline gate simulation)** | **Deleted** (was retired — decode unreliable). Replaced by scan-native `splice_dualfit` (C3/C7). |

---

## Re-orientation — how the proven ideas fold into vocabulary and pipeline

**Vocabulary (descriptive; `gap-vocabulary-redesign` P2/P3).** Re-root on the axes (B1, B8, B2, B13): a gap is
`{geometry, A-presence, donor-presence, shared-source, registration(offset+step), bracket-search,
envelope}`. Name types from a **`b_mapped` rescan** (post A2). W5 → "same-master, lag-0/bracket validation failed."

**Pipeline (detect → repair; ledger §4).** Order:
1. **Rescan primary cohort** with `b_mapped` capture (dirs 1–6 or full set).
2. **Re-classify skips** via `diag_fingerprint_corpus` — bracket-exhausted set may shrink.
3. **Detect** = **`dualfit_target()`** (ledger §4 step 1) — not `dualfit_candidate()` / uniqueness.
4. **Repair proof** = scan-native **`splice_dualfit`** already answers C3/C7; wire §4 behind flag (A3).
5. **Calibrate** thresholds (A5/C6) — light calibration done; uniqueness floors are diagnostic only.

**Dual-fit addressable set (provisional — pre-seam-local-fix):** scan-native `splice_dualfit` on the
edge-pin/D11 rescan (2026-07-01) — **7/39 bracket-exhausted skips pass the unchanged gate** (1·g3/g19/g21,
2·g2, 6·g3/g8, 7·g2), and donor gating narrows to 3 continuous-donor targets (1·g3, 2·g2, 7·g2). **⚠ This is
an UNDERCOUNT:** that scan scored each seam at the 1 s-window `baseline_lag`, so it missed true targets whose
seam-local lag diverges — ground-truth confirms **2·g1 (11:50)** and **1·g22** are fixable (pre seams recover
0.982 / 0.997 at their seam-local lags). `splice_dualfit_at` now does a ±100 ms per-shoulder seam search;
**a rescan will re-derive the real set (> 3).** One-sided-dead collapsed to 8/55 (B2/C1) — not a rescue path.

**One-line status:** registration **closed and validated** (A2/B2/B13/C1/C2/C4 PROVEN); **dual-fit
viability PROVEN in-scan (C3): ≥7/39 skips pass the gate** (a pre-fix undercount). **Open finding
(2026-07-01):** `splice_dualfit` scored each seam at the 1 s-window `baseline_lag`, false-negativing seams
whose seam-local lag diverges — ground-truth caught **2·g1 (11:50)** and **1·g22** as fixable targets it
wrongly failed. **FIX landed (unbuilt→built, capture):** `splice_dualfit_at` now does a ±100 ms per-shoulder
seam-local search (`SEAM_LOCAL_REFINE_MS`), unit-tested; **needs a rescan to re-derive C3 / the A3 target set
(> 3).** `dualfit_target()` + A5/C6 light calibration done; edge-pinned 0/55 (C4 reconfirmed). **Next:**
**rescan with the seam-local fix** → re-derive targets (+2·g1/1·g22 expected) → perf-redesign (D12 §2–§4) →
**wire §4 repair (A3)**.
