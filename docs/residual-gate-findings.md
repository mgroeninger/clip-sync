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
| **H2-B** | high | **Broadband Pearson dead zone.** | `veto_rescue` + oracle tests (synthetic). **Real-media rescue value still unproven — see G5.** |
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
| **G5** | gap | **`veto_rescue` real-media value unproven on a genuine codec-noise floor.** Run B (transient-anchored real-codec probe) shows that on genuinely-independent real encodes the floor goes *uninformative* exactly where Pearson dies, so rescue does not fire. | See **Rescue real-media reality (Run B)** below. Punch-after-encode AAC-dual is the open confirmation. |

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

## Rescue real-media reality (Run B — G5)

**Question:** does the `veto_rescue` trigger (Pearson in the dead zone *at truth* **and** an informative,
low-headroom floor) ever occur on real media — i.e. does rescue recover gaps that real production would
otherwise skip? Prior tests (`broadband_oracle_veto_rescue_patches_marginal`) proved the *mechanism* only
on a synthetic chirp+LCG-noise oracle. Run B (`source_gap_oracle_transient_csv`) anchors the gap on the
**Grieg fff finale** — the corpus's loudest broadband orchestral tutti (~148.5 s) — and runs the real
encodes through the **production fit-mode gate** (`min_fill 0.35`, real floor, waveform weight 0.65), not
the relaxed calibration config.

**Result (production fit-mode):**

| case (Grieg finale, truth placement) | off / veto | veto_rescue | seam min | floor | note |
|--------------------------------------|------------|-------------|----------|-------|------|
| AAC **same** 128k | skip | **skip** | 0.021 | uninformative (NaN) | rescue inert — no floor |
| AAC **dual** 128k/192k (independent) | skip | **skip** | 0.021 | uninformative (NaN) | the production-realistic case; rescue inert |
| Vorbis **same** 128k | skip | **patch** | 0.021 | informative −120/−120 | rescue *fires*, but floor is the **deterministic bit-identical-border** artifact (M2 caveat), not codec noise |
| AAC 102 s dual | skip | skip | 0.181 | uninformative (NaN) | — |
| benign control (14 s) | patch | patch | 0.300 | — (slid 33 ms) | escapes dead zone by sliding past the seam; slide > reach → residual **abstains** (M5), so 118 dB headroom does not veto |

**Findings:**
1. **Real same-master music seam Pearson at truth is in the dead zone** (0.02 at the finale, 0.30 at the
   benign anchor — both below `min_fill 0.35`). This **corrects** the earlier framing (H2-B /
   `floor_oracle_veto_rescue_real_broadband_codec`) that "real masters pass Pearson at truth." They do not;
   prior calibration runs *looked* like they passed only because the relaxed calibration config
   (`min_fill 0.0`, `fill_absolute_floor −0.05`) patches everything regardless of the seam.
2. **On genuinely-independent real encodes (AAC same + dual), rescue does not fire** — the floor goes
   *uninformative* (unmeasurable / NaN) at the dead-Pearson placement, so all three modes skip. Rescue is
   correctly inert; this is the safe outcome but means **no recovery**.
3. **The one real rescue flip (Vorbis same-bitrate) rides a −120 dB deterministic-encoder floor**
   (libvorbis → bit-identical borders, the M2 caveat), not lossy codec-noise cancellation — so it is not
   evidence that rescue recovers a *genuine* lossy seam.
4. **Net:** rescue's real-world *value* remains unproven on a genuine codec-noise floor; `veto`'s real-world
   inertness here is reassuring (it never false-vetoed a truth gap). Rescue stays correctly **non-default**.

**Open confirmation (in progress):** **punch-after-encode** AAC-dual at the finale — encode first, *then*
remove the gap so A's borders are *native* (no inject-then-encode MDCT corruption of the floor). This
removes the NaN-floor confound and would show whether a real *independent-encode* floor can ever be
informative at a dead-Pearson seam. If it still cannot, rescue is empirically a synthetic-only path and
should be documented as such.

## Validation infrastructure

- **`tests/seam_residual_corpus.rs`** — direct-scoring harness.
- **`tests/seam_residual_oracle.rs`** — pipeline oracle + H2-B rescue.
- **`tests/floor_oracle_integration.rs`** — real-codec FLOOR_OK + gate + `veto_rescue` safety; `source_gap_oracle_transient_csv` (Run B, G5: transient-anchored dead-zone probe under production fit-mode gate).
- **`tests/energy_signature_production.rs`** — F4 bool+veto pipeline.
- **Disagreement table** — `seam_residual_disagreement_oracles` (CI-fast).
