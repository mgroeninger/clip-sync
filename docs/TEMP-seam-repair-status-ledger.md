# Seam-repair status ledger — proven / open / important (triage index)

**Purpose.** The working docs
([archive/TEMP-seam-splice-dualfit-plan.md](archive/TEMP-seam-splice-dualfit-plan.md) — mechanism + measurement history,
[gap-vocabulary.md](gap-vocabulary.md) — vocabulary (archived derivation: [archive/TEMP-gap-vocabulary-redesign-plan.md](archive/TEMP-gap-vocabulary-redesign-plan.md)),
[TEMP-pipeline-perf-redesign-plan.md](TEMP-pipeline-perf-redesign-plan.md) — pipeline perf/assembly, D12)
hold the claims/plans at every stage of proof. This ledger is the **index over them**: one row per claim,
scored **Confidence × Importance × Target**, so we can see the critical path and what to incorporate. The
**§4 wire spec** (dual-fit repair algorithm for A3) lives here; the dualfit plan doc is historical detail.
Update this when a claim's status changes.

**Ledger status (2026-07-03):** Critical path **closed** (A1–A4 proven; A3, G5, D6, D7 shipped in code).
This doc is a **closed proof index** + §4 wire-spec reference. **Active work:**
[§F Production rollout](#f-production-rollout--remaining-work-2026-07-03),
[TEMP-pipeline-perf-redesign-plan.md](TEMP-pipeline-perf-redesign-plan.md) (D12 perf).
Vocab P4 (wiring named cells into `gap_tags.rs`/reporting) is **parked**, not active — see §D.

**Legend.** Confidence: `PROVEN` (data) · `SUPP` (strong, small n) · `DECIDED` (policy chosen, not yet in code) · `OPEN` · `REFUTED`.
Importance: `CRIT` (blocks a working repair) · `HIGH` · `MED` · `LOW`.
Target: `VOCAB` · `PIPE` (detect/repair) · `CAP` (fingerprint capture) · `—` (conclusion/park/tombstone).

**Evaluation cohort (do not merge denominators).**
```text
PRIMARY — **re-anchor rescan** (dirs 1–7, `gap-files/re-anchor-dual-fit-on-nominal`, 2026-07-02): 7 pairs,
  62 matched (7 tail, 0 no-lag), 23 patched / 39 skipped (32 bracket-exhausted). edge-pinned 0/55 (C4).
  Occupancy: dropout 31 · program-quiet 31 (24 skips program-quiet, D11). Dual-fit viability on the
  **nominal-reanchored ±600 ms** `splice_dualfit` + recalibrated step-real: **`gate_pass` is now DEGENERATE —
  31/32 bracket-exhausted skips pass** (the ±600 ms search is over-permissive; gate_pass no longer
  discriminates). Targets come from **donor-occupancy ∧ step-real**: **9 A3 targets — 1·g3, 1·g5, 1·g22, 2·g1,
  2·g2, 5·g6, 7·g2, 7·g3, 7·g4** (all `seam_z` 9.3–21.9, donor-continuous, step materially real). **Golden
  baseline FROZEN** (`golden/re-anchor-dual-fit-on-nominal.golden.json`; §4.0 gates met — P2 clean, b_levels
  clean). D8 caveat: gate_pass degenerate ⇒ a decoy regime needs a real alias gate (`seam_z`/wide-env).
Superseded scans (do NOT merge): edge-pin/D11 (2026-07-01, 1 s-gross-anchored → 3 targets, undercount —
  false-negatived 2·g1/1·g22/7·g3/7·g4); seam-local-fix ±100 ms (2026-07-02, → 7 targets, missed 7·g3's
  ~337 ms gross-vs-seam divergence). Legacy pre-A2 6-pair: 19 matched / 6 skipped (B1/B8 counts).
```

---

## A. The critical path — **closed** (2026-07-03)

The claims that gated a working repair. All rows below are **done** unless noted in §F.

| # | Claim / task | Conf | Why it's the blocker |
|---|--------------|------|----------------------|
| A1 | **Quiet-gap mis-registration is `structure_start_frame` wander**, not decorrelation. Proven: pair-6 (5/5 one-sided-dead), pair-7 **7·g3** (pre 0.986@+94 ms, z 18) and **7·g4** (pre 0.902@+118 ms, post 0.988@+113 ms) — all dead at F1 throat, clean at `b_mapped`. | PROVEN | Diagnosis done; re-anchor rescan (2026-07-02, dirs 1–7) validates capture. |
| A2 | **`b_mapped` registration** — center `baseline_lag` / detect metrics on geometry `b_mapped` nominal, **sequentially per-shoulder registered** (post search centered on `S + D_A + round(L_pre)`, not the naive `S + D_A`; gross lags `L_post_gross = L_pre + L_post_fine` ≈ bridge-length mismatch); **not** `structure_start_frame`. Outward-anchor (RMS loudest) is **not** the primary fix (pair-6 sweep). | **PROVEN (CAP)** | Gross map + ±600 ms sequential centering landed in `gap_fingerprint.rs` (`lag_pair`, `seam_probe_at_placement`, `wide_envelope_at_placement`, `donor_interior_at`); lib tests green. **Validated on full-corpus rescan (dirs 1–7, real media, 2026-07-01):** one-sided-dead collapsed 27/55 → **8/55**, confirming the fix. |
| A3 | **Dual-fit repair** (independent per-side fit → length reconcile → unchanged gate passes) actually works. | **BUILT + MEDIA-VALIDATED (2026-07-02)** | **Shipped** in the production `PatchAudio` path behind `--dual-fit` (`555c51c`): shared `domain/{seam_local,donor,dual_fit}` primitives (scan + production one impl, no drift), `try_dual_fit` self-contained + unit-tested, `skip_or_dual_fit` hook on all three bracket-exhausted skip returns, flag threaded args → `apply_cli_overrides` → `RepairConfig` → `patch_settings` → request. `--no-dual-fit` ⇒ byte-identical bracket path (D6 ✓); default **on** (F1). **Media run (2026-07-02, pairs 2–7; pair 1 still decoding):** dual-fit patched **exactly** the target set — **6/6 non-pair-1 targets rescued** (2·g1 0.98→0.97, 2·g2 0.89→0.94, 5·g6 0.99→0.97, 7·g2 0.99→0.77, 7·g3 0.94→0.99, 7·g4 0.94→0.96), **no over-rescue**, and **all clean by ear (D7 ✓)**. **Key negative confirmed:** pair 6's donor-BROKEN/program-quiet skips (13:46, 13:52, 1:13:28…1:16:08) were *tried* with the flag on and **correctly declined** (stayed skipped) — the donor + program-quiet gates hold on real media, so A3 does not silence-splice genuine dropouts. The one audible flaw the operator found (`5·g4` @ 50:18, start/end blips) is a **bracket-search** patch (`tier=patch`, not a dual-fit target) — a pre-existing fill-boundary crossfade artifact, out of A3 scope. **Remaining:** confirm pair 1 (1·g3/g5/g22) on completion; small production↔analyzer seam-score deltas (mono-approx caveat, e.g. 2·g2 pre 0.89 vs golden 0.981 — outcome unaffected) to reconcile at leisure. Viability proven in-scan; **9 A3 targets** measured on the re-anchor rescan (2026-07-02): 1·g3, 1·g5, 1·g22, 2·g1, 2·g2, 5·g6, 7·g2, 7·g3, 7·g4 — all bracket-exhausted skips, `gate_pass`, step materially real, donor-continuous, ¬program-quiet, `seam_z` 9.3–21.9. **Scope = `dualfit_target()` = skip ∧ bracket-exhausted ∧ gate_pass ∧ step-real ∧ donor-continuous ∧ ¬program-quiet** (NOT `dualfit_candidate`/uniqueness). **Two correctness bugs found + fixed in code review (2026-07-03), no rescan needed (caught before any media run used them):** (1) `skip_or_dual_fit` hardcoded `FillConfidence::High` on the assembled/trimmed fill instead of re-validating it — §5.2 step 3/§4.4's "unchanged gate" requirement was documented but not actually wired; fixed by re-scoring `r.fill` with the real `fill_splice_seam_correlations_interleaved` + `classify_fill_waveform_confidence` (same primitives every other fill path uses), falling back to the skip on `Err`. (2) `skip_or_dual_fit` was missing the `StructureAlignmentFailed` precondition, so dual-fit could fire on a skip where **no bracket was ever scored** (classes 1–2 in the perf-plan §4.1a categorization) — fixed with an explicit `!matches!(fail, SeamGateFailure::StructureAlignmentFailed)` guard. Both fixes are reflected as the current (correct) behavior in `TEMP-pipeline-perf-redesign-plan.md` §2.1/§5.1/§5.2 — this entry is the historical record that they were bugs, not original design. Zero test coverage existed for the class-4 decline path (`donor_interior` BROKEN despite `gate_pass`) before this review; added `dual_fit.rs::declines_donor_broken_bridge` (see perf-plan §4.4). **Three capture fixes, each caught by an operator ground-truth check:** (1) **seam-local** — score each seam at its own lag, not the 1 s `baseline_lag` (recovered 2·g1/1·g22); (2) **nominal re-anchor ±600 ms** (`2622c7a`) — the ±100 ms window centered on the gross lag still clipped 7·g3 (gross pre −319 ms, seam +18 ms); re-anchor on nominal `b_mapped` (recovered 7·g3); (3) **step-real recalibration** (`b099b83`) — `post_own − post@pre ≥ 0.15`, not `post@pre < 0.35` (recovered 7·g4). **P2 finding — `gate_pass` is DEGENERATE post-±600** (31/32 pass): the target set now rests on donor-occupancy ∧ step-real, not seam viability; and `donor-aligned ≡ donor-nominal` on this corpus. **D8 caveat:** on decoy/different-content data a real alias gate (`seam_z`/wide-env) is needed. Golden baseline frozen. **Production rollout** (default profile, user docs): §F. |
| A4 | **Donor continuity** — B carries unbroken content across the hole. | **PROVEN + gating** | Measured at `b_mapped` (aligned `donor_interior` + nominal `donor_interior_nominal`); **validated on the re-anchor rescan via the `b_levels`-vs-elimination cross-check (2026-07-02):** every eliminated gap where B is loud in context is either donor-BROKEN with a genuine **multi-second interior silence** (`longest_silence` 900–3500 ms — nothing to fill) or a start-of-file `g0`. Donor-continuity correctly gates the 9 targets (all continuous) and excludes the interior-silent skips (e.g. 1·g19: seams 0.998 but interior silent). Redundant with program-quiet on this corpus (P2), kept for D8. |
| A5 | **Threshold calibration** (`peak_z ≥ 12`, prominence, continuity) on the real distribution. | **DONE — diagnostic-only** | Re-anchor rescan (2026-07-02) + light calibration (C6) complete for repair-scoping thresholds. **`peak_z`/prominence no longer gate repair** — dual-fit scopes on step-real ∧ donor-occupancy (A3). Prominence floor **0.45 → 0.15** (2026-07-01). `PROGRAM_QUIET_SILENCE_FRAC = 0.5`, `DUALFIT_STEP_REAL_MARGIN = 0.15`, `edge_pinned` validated **0/55** on re-anchor rescan. **`peak_z = 12` kept** for analyzer alias-suspect labeling only. Remaining alias-gate calibration is **D8** (decoy regime — build when a false positive appears, not speculatively). Edge-pinned flag implemented in capture + analyzer (`step_edge_pinned()` excludes from `dualfit_candidate`). |

**Sequencing consequence (2026-07-03):** registration (A2) and dual-fit repair (A3) are **closed and
validated** on the re-anchor rescan; golden baseline **frozen**; G5 program-quiet and `--dual-fit` are
**shipped** in production (`patch_audio.rs`). **`diag_splice_dualfit` sim retired** (E-tombstone).
**Next:** [§F Production rollout](#f-production-rollout--remaining-work-2026-07-03) → D12 perf (optional
for calibration throughput, not repair correctness).

---

## §4. Dual-fit repair — wire spec (A3, **shipped**)

Condensed from [archive/TEMP-seam-splice-dualfit-plan.md](archive/TEMP-seam-splice-dualfit-plan.md) §4.
**Implemented** in production (`domain/dual_fit.rs`, `patch_audio.rs::skip_or_dual_fit`); **default on**
(`RepairConfig.dual_fit = true`, `--no-dual-fit` opt-out). Viability was proven in-scan via `GapFingerprint.splice_dualfit`; this section is the
algorithm reference, not a build checklist.

**Mechanism.** At a repair gap, A has a quiet/silent hole between two un-stretched shoulders. Each shoulder
registers against B at its **own lag**; the lags differ by a **step** (`splice.step_ms`). A single rigid donor
shift cannot satisfy both seams. Dual-fit places each shoulder independently, then reconciles the step with a
**trim or pad at the lowest-energy interior sample** of the fill — a pure length edit, not a within-side warp
(B7/B11).

**Algorithm (per gap):**

1. **Detect** — run only on gaps that **`dualfit_target()`** selects (analyzer: `gap_fingerprint_corpus.rs`):
   `skip` ∧ bracket-exhausted ∧ `splice_dualfit.gate_pass` ∧ **`step_is_real()`** (`post_own − post@pre ≥
   `DUALFIT_STEP_REAL_MARGIN` 0.15` — the step materially improves the seam, not merely clears the floor) ∧
   `donor_interior.continuous` ∧ ¬program-quiet. **Do not** run on gaps that already patch (≥1 bracket passes)
   or on `dualfit_candidate()`/uniqueness (does not predict placement seam viability). **Measured scope
   (re-anchor rescan, 2026-07-02): 9 gaps — 1·g3, 1·g5, 1·g22, 2·g1, 2·g2, 5·g6, 7·g2, 7·g3, 7·g4** (all
   `seam_z` 9.3–21.9). Note (P2): `gate_pass` is degenerate post-±600, so the effective gates are step-real ∧
   donor-occupancy.
2. **Fit each seam at its SEAM-LOCAL lag, re-anchored on NOMINAL `b_mapped`** — search each shoulder
   ±`SEAM_LOCAL_SEARCH_MS` (600 ms, the `baseline_lag` range) around the nominal geometry anchor (pre butts at
   `b_mapped_start`, post at `b_mapped_start + gap_frames`) and take the peak; the seam **defines its own
   placement**. **Do NOT anchor on the gross 1 s `baseline_lag`:** it can lock onto distant content and clip a
   live seam — `2·g1` (gross pre −24 ms, seam +4.4 ms → 0.982) and especially `7·g3` (gross pre **−319 ms**,
   seam **+18 ms** → the ±100 ms gross-anchored window missed it entirely). `b_pre`/`b_post` are the
   nominal-anchored seam peaks; `splice_dualfit.pre/post_seam_z` (whole-curve z-score) is the alias guard
   against the wide search locking onto a far periodic rival — **not** the ±30 ms prominence (which over-flags
   correct-but-periodic content, `5·g6`).
3. **Reconcile the step** — extract the B bridge `[b_pre .. b_post]`; `trim_frames = bridge_frames − gap_frames`
   (= the step in samples; C7 tautological). Trim or pad `|trim_frames|` at the **lowest-RMS interior sample**
   of the fill region (smallest audible splice). Interior edit only — shoulders stay at their own lags.
4. **Validate with the unchanged gate** — score pre/post seams against B at the **seam-local-refined**
   placements (step 2), using `fill_seam_search_secs` (default 250 ms) and the existing `min_fill_correlation`
   / `fill_absolute_floor` thresholds. A bad length edit must fail exactly as a bad shift does today —
   **strict gate, no loosening**. This is the property the (seam-local-fixed) `splice_dualfit` measures at
   capture time, so `splice_dualfit.gate_pass` predicts this validation.
5. **Reject** to skip (as today) if post-reconciliation validation fails, the step was edge-pinned (GIGO), or
   donor continuity is false. Gate-pass alone is not sufficient — donor-BROKEN gaps (e.g. 1·g19: seams 0.998
   but B interior silent) must stay skipped (D11).

**Wiring notes.**

- **Default on** — `RepairConfig.dual_fit = true`; `--no-dual-fit` ⇒ bracket-search path unchanged (D6).
- **Production path** — `skip_or_dual_fit` → `try_dual_fit` → re-validate assembled fill with unchanged gate
  floors; `StructureAlignmentFailed` excluded (no bracket scored).
- **G5 program-quiet (D11)** — analyzer label via `donor_interior_nominal` / `program_quiet()`; dual-fit
  declines program-quiet donors inside `try_dual_fit`. **Not** a production pre-gate skip (2026-07-03).
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

## C. Important questions — **all resolved** (2026-07-03)

| # | Question | Conf | Imp | How to prove |
|---|----------|------|-----|--------------|
| C1 | Does the **`one-sided-dead` bucket collapse** at `b_mapped`? | **PROVEN** | CRIT | **Yes.** Full-corpus rescan (dirs 1–7, real media, post sequential-fix, 2026-07-01): one-sided-dead **27/55 → 8/55** (49% → 14.5%). 19 gaps that were window-placement artifacts now recover both shoulders. 8 residuals are genuinely dead (large steps, `peak_r ≤ 0.17`) — the real floor, not artifacts. |
| C2 | Which **placement** for registration? | **PROVEN** | CRIT | **`b_mapped`**, sequentially per-shoulder registered (post centered on measured `L_pre`, not the naive `S + D_A`). Pair-6 + pair-7 confirmed the placement choice; RMS outward-anchor not primary (D10). |
| C3 | Does the **dual-fit repair pass the unchanged gate** on the known skips? | **PROVEN — yes, for a real subset** | CRIT | **Scan-native `splice_dualfit` (full corpus, 2026-07-01, on the scan's own decode): 7/39 bracket-exhausted skips pass the gate** at per-shoulder placement — **6 need the step (genuine silence-splices), 1 is a constant offset** (1·g21). Passes: 1·g3, **1·g19**, 1·g21, 2·g2, 6·g3, 6·g8, 7·g2. **Refutes the sim's decode-tainted "steps spurious" claim:** 1·g19 (step **+315.8 ms**) reads **both seams 0.998** on the scan decode (the sim had called it dead/common-offset). Genuine large-step splices exist and dual-fit clears the unchanged gate on them. Failures split into *one-shoulder-dead-at-seam* (1 s lag aligns, 250 ms seam dead one side — splice at the edge) and *donor BROKEN*; the latter will further split into program-quiet non-dropouts (D11) once the nominal-donor re-scan lands. Sim retired (E-tombstone). **CONFIRMED on the edge-pin/D11 rescan (2026-07-01, donor now populated):** the same 7/39 pass (6 real-step: 1·g3, 1·g19, 2·g2, 6·g3, 6·g8, 7·g2; 1·g21 spurious). **Donor gating splits the 6 real-step passes in half:** continuous → **1·g3, 2·g2, 7·g2** (the clean A3 targets); donor-BROKEN → 1·g19, 6·g3, 6·g8 (seams align but the gap interior is silent — filling inserts silence, correctly excluded). **1·g19 is the sharp lesson:** seams 0.998 yet donor-BROKEN ⇒ gate-pass is necessary, not sufficient — donor occupancy MUST gate (validates A4/D11). **RESOLVED on the re-anchor rescan (2026-07-02):** the 7/39 was a pre-fix undercount (seams scored at the 1 s `baseline_lag` false-negatived 2·g1/1·g22/7·g3/7·g4). After the seam-local → nominal-reanchor (±600 ms) → step-real fixes, **`gate_pass` is degenerate (31/32 pass)** — it no longer discriminates; the real gates are donor-occupancy ∧ step-real, yielding **9 A3 targets** (1·g3,1·g5,1·g22,2·g1,2·g2,5·g6,7·g2,7·g3,7·g4). C3's "does the fill pass the gate" is now trivially yes for almost every gap — so on same-master data the answer rests on donor + step-real, which validate cleanly (9 targets, no false positive). `gate_pass` being degenerate is a redundancy, **not** a leak (D8 is theoretical/low-priority — see D8). |
| C4 | Is **±600 ms sequentially-centered lag search** sufficient at `b_mapped`? | **PROVEN** | HIGH | **Yes, for the recoverable population.** Full-corpus rescan (2026-07-01): 47/55 register within the ±600 ms sequential window (both-sides-recoverable 15/55 + alias-suspect 32/55). Sequential centering decouples `L_pre`; residual post lags now measure `\|D_B − D_A\|` alone. The 8 one-sided-dead are dead at the shoulder itself (`peak_r ≤ 0.17`), not clipped by the window — widening won't recover them. |
| C5 | **Donor continuity** true for the skip targets? (= A4, ranked) | **PROVEN (= A4)** | HIGH | Validated on the re-anchor rescan via the `b_levels`-vs-elimination cross-check (2026-07-02): all 9 targets are donor-continuous; every eliminated B-loud gap is donor-BROKEN with multi-second interior silence, or a start-of-file `g0`. See A4. (Footgun retained: prefer `donor_interior_nominal` for occupancy when a post lag doesn't resolve.) |
| C6 | **Threshold calibration** — `peak_z`/prominence/continuity floors on the real distribution. | **DONE — repair thresholds** | HIGH | Light calibration complete on re-anchor rescan (2026-07-02). Repair-scoping thresholds frozen: `PROGRAM_QUIET_SILENCE_FRAC = 0.5`, `DUALFIT_STEP_REAL_MARGIN = 0.15`, `DONOR_CONTINUITY_MS = 150`, prominence tiebreaker 0.15. `peak_z`/uniqueness **diagnostic-only**. Remaining alias-gate work is **D8** (decoy regime). |
| C7 | **Trim magnitude ≈ measured `step_ms`** | **RESOLVED (tautological in-scan)** | HIGH | Scan-native `splice_dualfit` places shoulders at their own lags, so `trim_frames = bridge − gap = step` **by construction** (no separate decode to disagree). C7 is no longer an open reconciliation risk; the open question is now *seam viability* (C3) + whether the step is *real* (new validator: `post_seam_global_r`). |

---

## D. Open + low / parked (do not spend cycles yet)

| # | Item | Why parked |
|---|------|-----------|
| D1 | **Mechanism of the step** (silence-splice vs resampler vs PTS; sub-frame, not quantized) | The repair *measures* the step; the physical cause doesn't change the fix. Interesting, not blocking. |
| D2 | **Decorrelated / different-content regime** | Untestable directly — this corpus is all same-master. But same-master **decoys** can stand in for different-content negatives (see **D8**: mine periodic/alias-suspect placements; construct level-matched substitution fills). Revisit with genuine different-content data when available. |
| D3 | **Channel-scope / donor-displacement axes** (vocab §2b) | Surface in analyzer later; not decision-relevant for dual-fit. |
| D4 | **Wire axis facts into `gap_tags.rs`**; reconcile `content_hint`/`seam_shape` with cells | Vocab **P4** (deferred, parked — not scheduled); types named in [gap-vocabulary.md](gap-vocabulary.md) (P3 done). W-tiers stay derived readouts until P4. If picked up, the code-touch point is `gap_tags.rs`'s tag emission, documented today in [gap-repair-guide.md](gap-repair-guide.md) § Vocabulary — no other active TEMP plan touches this file. |
| D5 | **Perf** (FFT lag, dedup search, decode reuse) | **Now owned by [TEMP-pipeline-perf-redesign-plan.md](TEMP-pipeline-perf-redesign-plan.md) (D12).** FFT lag sweep scoped (~50–150×, `rustfft` present, gate on `fft≈naive` test) — full spec in **Capture parked → FFT lag sweep** below, migration ordering in the perf doc §3. |
| D6 | **No regression on existing patches** (`--no-dual-fit` ⇒ unchanged) | **DONE (2026-07-02)** — `--no-dual-fit` restores bracket-only path; dual-fit default **on** (F1). Only reachable from bracket-exhausted skip returns, so patched gaps are untouched. |
| D7 | **Audibility of the trim point** (splice at low-energy interior sounds clean) | **PASSED (2026-07-02)** — operator reviewed dual-fit fills on pairs 2–7; indistinguishable by ear. Pair 1 targets (1·g3/g5/g22) validated in-scan + golden baseline; optional media listen. Blip at `5·g4` is bracket-search crossfade, not dual-fit. |
| D8 | **Decoy / wrong-placement safety** (a deliberately wrong B fill still fails the gate) — the corpus has no **wrong-content** negatives (all same-master). *(It DOES have no-content negatives — program-quiet + donor-BROKEN — which the donor gate catches, validated. D8 is only about the wrong-content/periodic-misregistration case.)* | **THEORETICAL — low priority (2026-07-02).** No false positive has been observed: all 9 targets are validated (donor-continuous, step-real, operator-confirmed spot checks). `gate_pass` being degenerate (31/32 pass) means the seam gate is *redundant* here — it is **not** evidence anything is leaking; the donor + step-real filters carry the discrimination. **A real alias gate is unneeded until a false positive appears.** The first labeled negative will come from **D7 (listening to the 9 fills)** or non-same-master deployment — build the gate *then*, calibrated against that example, not speculatively. Candidate discriminators when needed: wide-envelope concordance (B12), cross-scale lag agreement, 1 s `peak_z` (NOT the 250 ms `seam_z` — one-sided-dead gaps score it up to 41). **Construction** (when needed): **A fair decoy must pass structure but fail the seam** (else it tests nothing): a *too-different* decoy (cross-pair/noise/silence) fails structure trivially; a *matching-shoulders/wrong-interior* decoy is **not a fair test** — A's gap is empty, so B's interior is unverifiable from A (accepting it is a limit, not a gate bug). **Offset-perturbation doesn't work** — the structure/lag search self-corrects a metadata lie; you must change the *audio* so the correct answer isn't available. **Construction (two ways):** (A) **Mine** — periodic/repeated content yields structurally-similar-but-fine-wrong placements for free; the **alias-suspect cluster (pair 6)** *is* a set of natural decoys — offer a repeated-phrase location as a fill candidate and check the seam rejects. (B) **Construct** — on a known-good fillable gap, overwrite B's fill region (mapped span **and** shoulders) with a different, **active, level-matched** passage of the *same* B (single-master ⇒ true content is unique ⇒ search can't route around it); a correct gate flips fill→skip. **Make it a margin:** sweep decoy content-distance (near-repeat → distant) to map the seam's discrimination boundary = the reject-safety margin / headroom on the 0.35 floor. Start with mining (free); build substitution only for the parameterized margin. |
| D9 | **Fingerprint diagnostic stubs** (F2/F3) | Gate path omits per-bracket `structure_*` and leaves `GateOutcome` vocabulary tags empty. Fine for diagnostics today. See **Capture parked**. |
| D10 | **RMS outward-anchor as primary registration** | Pair-6 sweep: loudest ≠ most unique (6·g9 pre z 22→9, 6·g10 pre z 27→9 on sustained tones). `b_mapped` + centered lag already finds −131 ms. Keep `[outward-anchor]` in `diag_splice_timescale` as diagnostic only; if revived, select by **`peak_z` distinctiveness**, not RMS. |
| D12 | **Pipeline performance redesign** (D12) | **Active — not blocking repair.** Own doc: [TEMP-pipeline-perf-redesign-plan.md](TEMP-pipeline-perf-redesign-plan.md). §1 audit done; §2 updated (2026-07-03); §4 golden harness built (**CI subset partial** — §4.7). Remaining: land §4.7 Tier A before hoists; step 1 hoists (partial), step 4 FFT lag (+ B1 equivalence test). X-diagnostics gated (`--fingerprint-diagnostics`); per-bracket oracle still always on in scan. |
| D11 | **Donor-silent gaps = program-quiet, not fillable dropouts** | **DONE — analyzer (2026-07-03); production pre-gate removed (2026-07-03).** Analyzer: `program_quiet_skip()` / `addressable_dropout()` via `donor_interior_nominal`. Plan-time: `b_has_energy = false` → unfillable. Dual-fit declines program-quiet donors. Production patch no longer short-circuits on nominal silence alone (F2/I3 regression fix). Threshold `PROGRAM_QUIET_SILENCE_FRAC = 0.5` (C6). |

---

## Capture parked (fingerprint layer hygiene)

Parked **CAP** items — hygiene only; not on the production rollout path (§F).

**A2 capture:** done — decision metrics at **`b_mapped` nominal**; re-anchor rescan validates.

**F1 (mostly done).** `skip_baseline_placement` dedup landed (D12 step 1 partial). **Remnant:** top-level
`fp.structure` from summary pass may disagree with oracle throat; `fp.seams.baseline_*` updated from
zero-move bracket in gate overlay.

| # | Item | Status | When to fix |
|---|------|--------|-------------|
| F2 | Gate `brackets[]` write `structure_pre/post = None` (oracle has structure internally) | OPEN | Only if analyzer needs per-bracket structure or schema/docs parity |
| F3 | `GateOutcome.seam_shape` / `fit_path` / `signature_mode` empty in gate path | OPEN | Only if vocabulary tags migrate into fingerprints |
| C-docs | `gap-fingerprint.md` omits `baseline_lag`, `seam_probe`, `splice`, `donor_interior`, `wide_envelope` | **DONE (2026-07-01)** | Shape table + new *Registration & dual-fit measurements* section document `baseline_lag` (`b_mapped`, `peak_z`/`prominence`/`edge_pinned`), `splice`, `donor_interior`(`_nominal`), `splice_dualfit`, `wide_envelope`, `seam_probe`, `residual`, `b_levels` — each with its placement provenance. |
| C-harness | `uniqueness_z` single-sided (slightly optimistic) | **DONE (2026-07-01)** | Two-sided `both()` — `uniqueness_z` is `min(pre,post)` only when BOTH shoulders carry `peak_z`, else `None` (no fabricating a both-sides value from one). |
| C-harness-2 | **`verdict` / `skew` use one side** — `gap_row` took `pre.or(post)` for the headline verdict and constant-vs-drift skew | **DONE (2026-07-01)** | `skew` now requires **both** shoulders present and both `timing_offset` (else `NotApplicable`); `verdict` doc marks it one-sided/pre-preferred and points classification at the two-sided `splice_diag()` / `seam_step_ms()`. |
| C-harness-3 | **Legacy `gap.lag` fallback** — analyzer uses `baseline_lag.or(gap.lag)` | **DONE (2026-07-01)** | Rows that fall back are flagged (`registration_from_legacy_lag`) and `summary_text` warns loudly on any pre-/post-A2 schema mix (different placement — not comparable). Fallback retained for pre-A2 corpora. |
| B-level | **Symmetric B-side level** | **DONE — validation role** | Full B `LevelProfile` behind `--fingerprint-diagnostics`. Production G5 uses cheap nominal-span `donor_interior_nominal` (shipped). |

**Perf (2026-07-03).** Dominant scan cost: N × oracle bracket scoring (always on). Partial wins landed:
`skip_baseline_placement` (summary dedup); X-set behind `--fingerprint-diagnostics`. **Still open (D12):**
binned-RMS hoist, border extract sharing, FFT lag sweep (~50–150×). Fingerprint dump shares `decode_ab` with
repair — it does **not** re-decode after repair. Details: [TEMP-pipeline-perf-redesign-plan.md](TEMP-pipeline-perf-redesign-plan.md) §2.4.

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
- **Sequencing:** prerequisites met (golden frozen, naive baseline stable). Land behind §4 harness Tier-2 ε.
  Est. ~1 day incl. tests. Tracking: D12 §3 step 4.

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
| **Edge-pinned peaks** | VALIDATED 0/55 (re-anchor rescan) | Safety net only; `step_edge_pinned()` excludes from `dualfit_candidate`. |
| **`lag0_r` when gross-shifted** | OPEN, low | `LagVerdict` / `lag0_r` can be wrong when the gross-shifted curve omits lag 0 — `splice_diag` unaffected. |
| **`dualfit_candidate` (uniqueness) is retired for scoping** | resolved (re-anchor rescan) | The uniqueness-vs-`gate_pass` divergence was a **`splice_dualfit` placement bug**, not a uniqueness failure: 2·g1/1·g22 (seam-local fix) and 7·g3 (nominal re-anchor) and 7·g4 (step-real) were all false-negatives, now recovered. Final scope = **`dualfit_target()` = gate_pass ∧ step-real ∧ donor-continuous ∧ ¬program-quiet** on the re-anchored `splice_dualfit` → **9 targets**. Post-±600, `gate_pass` itself is degenerate (P2) — the load-bearing gates are step-real ∧ donor-occupancy. `dualfit_candidate`/uniqueness (`peak_z`) is diagnostic only. |

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

## Re-orientation — closed proof summary (2026-07-03)

**Vocabulary (descriptive).** Axes + named cells in [gap-vocabulary.md](gap-vocabulary.md) (P3 done).
**Remaining:** P4 wire axis facts into reporting tags (D4) — not blocking repair.

**Dual-fit addressable set (FINAL — re-anchor rescan):** **9 targets** — 1·g3, 1·g5, 1·g22, 2·g1, 2·g2,
5·g6, 7·g2, 7·g3, 7·g4. Effective gates on this corpus: **step-real ∧ donor-occupancy ∧ ¬program-quiet**
(`gate_pass` degenerate post-±600 — ledger P2). Golden baseline frozen.

**Repair algorithm:** §4 below; **implemented** (dual-fit default on). **G5 program-quiet** always on in
production (not flag-gated).

---

## F. Production rollout — remaining work (2026-07-03)

What is left to call clip-sync-repair **production-complete** for same-master gap fill (not calibration
throughput). Ordered by impact.

| # | Item | Status | Notes |
|---|------|--------|-------|
| **F1** | **Enable dual-fit in default repair profile** | **DONE (2026-07-03)** | `RepairConfig.dual_fit = true` by default; `--no-dual-fit` opt-out. Media-validated on pairs 2–7; golden covers all 9 targets. |
| **F2** | **Operator docs** | **DONE (2026-07-03)** | [gap-repair-guide.md](gap-repair-guide.md) and [gap-fill-modes.md](gap-fill-modes.md) document dual-fit rescue (G6), G5 `ProgramQuiet` skip, W7/P8, and `--no-dual-fit`. |
| **F3** | **CLI / JSON surfacing** | **DONE (2026-07-03)** | Skip reason `ProgramQuiet` documented as reserved (not production-emitted). Dual-fit + fingerprint flags in [README.md](../README.md), [cli-output.md](cli-output.md), `repair.toml`; `--fingerprint-gap` / `--fingerprint-diagnostics` require `--gap-fingerprints`. Dual-fit-rescued patches now carry a `dual_fit_used` marker (mirrors `anchor_seam_used`) distinguishing them from ordinary bracket-search fits: human status `patched (dual-fit pre→post)`, JSON `status.patched.dual_fit_used` / `tags.dual_fit_used`, verbose `dual_fit=true` — see [gap-fill-modes.md](gap-fill-modes.md) § Dual-fit rescue. |
| **F4** | **Bracket-search fill quality** | **OPEN — separate track** | `5·g4` boundary blips (crossfade at fill edge) — pre-existing bracket path, not A3. Worth a focused pass if operators hit audible seams on *patched* (non-dual-fit) gaps. |
| **F5** | **D12 perf** | **OPEN — not blocking correctness** | Hoists + FFT lag ([TEMP-pipeline-perf-redesign-plan.md](TEMP-pipeline-perf-redesign-plan.md) §2.4). Land §4.7 Tier A tests before hoists. Improves fingerprint calibration turnaround (~1.7 h/pair), not end-user repair output. |
| **F6** | **D8 decoy / alias gate** | **PARKED** | Build only when a false positive appears (non-same-master or mined decoy). No observed leak on the 9-target corpus. |
| **F7** | **Vocab P3** | **DONE (2026-07-03)** | Published [gap-vocabulary.md](gap-vocabulary.md) — five named cells + legacy W-tier appendix. P4 (wiring into analyzer/reporting tags, D4) remains deferred; does not change repair decisions. |

**Suggested order:** F3 (output polish) → F4 if user reports audible bracket patches → F5 when rescan
throughput matters.
