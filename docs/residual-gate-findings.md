# Residual / floor gate — findings ledger

Bugs, gaps, regressions, and smells found while building the residual/floor work (P0 prototype →
P1 plumbing → P4 default `veto`). Companion to
[residual-gate-wiring-plan.md](residual-gate-wiring-plan.md).

Legend: **status** = fixed / open / deferred; **sev** = high / med / low / gap / regression.

**Shipped (production):** default `residual_gate = veto`; unified lag radius; `SeamResidualVerdict`
on patched gaps; `residual_band` tag; `donor_relation` run diagnostic; real-codec calibration
(AAC/Vorbis/music); L3 deferred finalize; L11 zero-alloc lag search; prototype path retired (L12).

---

## Fixed (verified)

| id | sev | what | fix / evidence |
|----|-----|------|----------------|
| **H1** | high | **Reference asymmetry.** Trimmed seam template vs raw floor window → spurious headroom. | `seam_chosen_and_floor` on same raw window; tests `seam_chosen_and_floor_*`. |
| **M1** | med | **Lag radii not unified.** | Single `max_lag_frames` from `residual_lag_secs`. |
| **H2-B** | high | **Broadband Pearson dead zone.** | `veto_rescue` + oracle tests; real-codec validation. |
| **M5** | med | **Real-codec reach false-veto.** | Lag-centered probe + `beyond_lag_reach()` abstention. |
| **M2** | med | **`FLOOR_OK` uncalibrated.** | Calibrated AAC/Vorbis/music + Vorbis dual; `FLOOR_OK = −15`. |
| **G3** | gap | **Real-codec calibration corpus.** | `floor_oracle_integration` matrix green. |
| **L3** | low | **Per-candidate verdict cost under `--full`.** | Deferred finalize on joint grid. |
| **L7** | low | **Empty post-border → 1-frame spurious cancel.** | `seam_post_gate_frames` returns 0. |
| **L11** | low | **Per-lag `Vec` allocation.** | Borrow `b_haystack[lo..hi]` per lag. |
| **L9** | low | **Dual `floor_db` naming** on prototype `SeamResidual`. | Prototype removed (L12); production uses `SeamFloorProbe::residual_db` only. |
| **L10** | low | **`frac_lag` computed, never applied.** | Parabolic refinement removed; integer lag only until fractional-delay work ships. |
| **L12** | low | **Prototype path dead in production.** | Deleted `seam_residual_diagnostics`, `SeamResidual`, `SeamResidualDiagnostics`; tests target `seam_residual_for_side` / `seam_floor_probe`. |
| **L13** | low | **`lsq_residual_ratio` silent-B false ~0 dB.** | `bb < LSQ_B_ENERGY_FLOOR` → abstain; tests `lsq_residual_ratio_abstains_when_b_silent`, `seam_residual_abstains_when_b_silent_at_placement`. |
| **A1–A3** | low | Head-shift artifact, clippy, corpus sha256. | Fixed/subsumed. |

## Open — medium

| id | sev | what | notes |
|----|-----|------|-------|
| **M3** | med | **Floor walk edge case** (no walk past OOB B mapping). | Abstain near haystack edges; restore walk if real media hits it. |

## Deferred / accepted

| id | sev | what | notes |
|----|-----|------|-------|
| **M4** | med | **MP3 unvalidated.** | Codec-agnostic gate; excluded from calibration. |
| **M6** | med | **F4 bool decoy + nominal-floor veto (accepted).** | Signature problem; not residual veto scope. |
| **FD-1** | med | **Fractional-delay cancellation** (was L10 wiring path). | Integer lag caps cancellation (~−16 dB at 0.5 sample); headroom hides within reach (**M5**). See wiring plan §10 + **Fractional-delay review** below. |

## Open — low / smells

| id | sev | what | notes |
|----|-----|------|-------|
| **L1** | low | **NaN `PartialEq` on outcomes.** | `Option<f64>` or sanitize. |
| **L2** | low | **Mono-only residual.** | Must add channel-following before 5.1 (G4). |
| **L4** | low | **Wasted verdict on soon-skipped gaps.** | Minor after L3. |
| **L5** | low | **`SeamFloorSource` CSV vs JSON casing.** | Cosmetic. |
| **L6** | low | **Coarse outward walk step.** | `step_frames = window`. |
| **L8** | low | **Mono peak-only energy gate.** | Per-channel plan may help. |

## Open — gaps in coverage

| id | sev | what | notes |
|----|-----|------|-------|
| **G1** | gap | **No JSON residual on skipped gaps.** | `seam_residual_disagreement_csv` covers analysis. |
| **G2** | gap | **`peak_normalize_f64` no-op in Pearson.** | Doc fixed; code remove when convenient. |
| **G4** | gap | **Channel-following residual.** | Overlaps L2. |

## Regressions

**None found** with `--residual-gate off`. Default **`veto`**.

## Fractional-delay review (FD-1 — deferred)

**Problem:** `seam_residual_for_side` picks an **integer** `best_lag`. Sub-sample misalignment between A and B leaves ~0.5-sample error → cancellation floor ~−16 dB even when content matches. **M5** mitigates for *placement* error via lag-centered search within `residual_lag_secs`; it does not fix *intrinsic* codec/alignment fractional delay at the correct placement.

**What was removed (L10):** Parabolic `frac_lag` from the ratio curve was computed but **never applied** to the B window before LSQ — dead work, not a partial implementation.

**What shipping FD-1 requires:**

| Piece | Work |
|-------|------|
| **Sub-sample lag estimate** | Re-introduce parabolic refinement on the integer lag grid *or* evaluate 2–3 fractional offsets around `best_lag` and pick minimum residual. |
| **Fractional B resample** | Before `lsq_residual_ratio`, resample the B window at `best_lag + frac` (linear interpolation minimum; sinc/polyphase if quality insufficient). |
| **API surface** | Optional `frac_lag` on `SeamFloorProbe` / internal `LagFitResult` for diagnostics only; gate uses improved `residual_db`. |
| **Cost** | 1–3× LSQ per lag side vs today (or one resample + one LSQ if frac fixed after grid). Still behind L3 single-probe-per-gap. |
| **Calibration** | Re-run `source_gap_oracle_floor_csv` + disagreement oracles — `FLOOR_OK` / `HEADROOM_MARGIN` may tighten or stay if headroom already ≈ 0 at truth. |
| **Tests** | Synthetic: known 0.5-sample offset → headroom ≈ 0 after FD-1 (today ~16 dB without). Corpus: placement-offset sweep should not regress. |
| **Interaction** | Lag-centered chosen probe (**M5**) stays; FD-1 improves residual *at* the chosen integer neighbor, not search reach. |

**Not required for FD-1:** Changing Pearson seam scoring, joint grid, or `SeamResidualVerdict` schema (unless reporting `frac_lag`).

## Validation infrastructure

- **`tests/seam_residual_corpus.rs`** — direct-scoring harness.
- **`tests/seam_residual_oracle.rs`** — pipeline oracle + H2-B rescue.
- **`tests/floor_oracle_integration.rs`** — real-codec FLOOR_OK + gate + `veto_rescue` safety.
- **`tests/energy_signature_production.rs`** — F4 bool+veto pipeline.
- **Disagreement table** — `seam_residual_disagreement_oracles` (CI-fast).
