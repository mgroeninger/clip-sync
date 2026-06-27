# Residual / floor gate — findings ledger

> **Status:** **Shipped** (2026-06-26). Build ledger for P0→P4 residual/floor gate work; validity
> contract **C1a + C2–C4** complete. **Archived** (2026-06-26). Living test contract:
> [`crates/clip-sync-repair/tests/residual_gate_catalog/`](../../crates/clip-sync-repair/tests/residual_gate_catalog/).
> Open follow-ups: [BACKLOG.md](../../BACKLOG.md) § Residual gate follow-ups. Outbound doc links are
> relative to `docs/archive/`.

Bugs, gaps, regressions, and smells found while building the residual/floor work (P0 prototype →
P1 plumbing → P4 default `veto`). Companion to
[residual-gate-wiring-plan.md](residual-gate-wiring-plan.md).

Legend: **status** = fixed / open / deferred; **sev** = high / med / low / gap / regression.

**Shipped (production):** default `residual_gate = veto`; unified lag radius; `SeamResidualVerdict`
on patched gaps; per-channel residual/floor (`seam_chosen_and_floor_multichannel`); lazy residual
finalize on joint grid (`fit_routing` + `try_finalize_*`); `residual_band` tag; `donor_relation` run
diagnostic; real-codec calibration (AAC/Vorbis/music); L11 zero-alloc lag search; prototype path
retired (L12).

---

## Fixed (verified)

| id | sev | what | fix / evidence |
|----|-----|------|----------------|
| **H1** | high | **Reference asymmetry.** Trimmed seam template vs raw floor window → spurious headroom. | `seam_chosen_and_floor` on same raw window; tests `seam_chosen_and_floor_*`. |
| **M1** | med | **Lag radii not unified.** | Single `max_lag_frames` from `residual_lag_secs`. |
| **H2-B** | high | **Broadband Pearson dead zone.** | `veto_rescue` + oracle tests (synthetic). Real-media rescue value **unsupported** — see **G5**. |
| **M5** | med | **Real-codec reach false-veto.** | Lag-centered probe + `beyond_lag_reach()` abstention. |
| **M2** | med | **`FLOOR_OK` uncalibrated.** | Calibrated AAC/Vorbis/music + Vorbis dual; `FLOOR_OK = −15`. |
| **G3** | gap | **Real-codec calibration corpus.** | `validate_floor_oracle` matrix green. |
| **G5** | gap | **`veto_rescue` real-media value.** | Run B + `deadzone_punch_assert`: rescue trigger does not occur on real codec noise → synthetic-only; correctly non-default. See **Rescue real-media reality (Run B)** below. Sub-question (*why* finale floor is NaN) open — M3-adjacent; see backlog `finale_floor_nan_probe`. |
| **L2** | low | **Mono-only residual.** | `measure_fit_residual_verdict` uses `seam_chosen_and_floor_multichannel`; tests in `policies.rs`. |
| **L3** | low | **Per-candidate verdict cost under `--full`.** | Lazy residual: `fit_routing` + `try_finalize_high_joint_candidate` / `select_joint_fit_winner_with_residual` — probe only on candidates the router finalizes. |
| **L4** | low | **Wasted verdict on soon-skipped gaps.** | Subsumed by **L3** lazy finalize. |
| **L5** | low | **`SeamFloorSource` CSV vs JSON casing.** | `#[serde(rename_all = "snake_case")]` on `SeamFloorSource`; `source_label()` matches (`border` / `walked` / `none`). |
| **L7** | low | **Empty post-border → 1-frame spurious cancel.** | `seam_post_gate_frames` returns 0. |
| **L8** | low | **Mono peak-only energy gate.** | Per-channel `walk_reference_frames` with `energetic` over selected channels in `seam_chosen_and_floor_multichannel`. |
| **L11** | low | **Per-lag `Vec` allocation.** | Borrow `b_haystack[lo..hi]` per lag. |
| **L9** | low | **Dual `floor_db` naming** on prototype `SeamResidual`. | Prototype removed (L12); production uses `SeamFloorProbe::residual_db` only. |
| **L10** | low | **`frac_lag` computed, never applied.** | Parabolic refinement removed; integer lag only until fractional-delay work ships. |
| **L12** | low | **Prototype path dead in production.** | Deleted `seam_residual_diagnostics`, `SeamResidual`, `SeamResidualDiagnostics`; tests target `seam_residual_for_side` / `seam_floor_probe`. |
| **L13** | low | **`lsq_residual_ratio` silent-B false ~0 dB.** | `bb < LSQ_B_ENERGY_FLOOR` → abstain; tests `lsq_residual_ratio_abstains_when_b_silent`, `seam_residual_abstains_when_b_silent_at_placement`. |
| **G4** | gap | **Channel-following residual.** | Same as **L2** — closed. |
| **G2** | gap | **`peak_normalize_f64` redundant before Pearson.** | Removed; `seam_pearson` calls `normalized_correlation` directly; `seam_pearson_invariant_under_positive_scale`. |
| **L1** | low | **NaN `PartialEq` on outcomes.** | `SeamResidualVerdict`: manual `PartialEq` with `residual_scalar_eq` (`NaN == NaN`); test `seam_residual_verdict_partial_eq_treats_nan_as_equal`. |
| **A1–A3** | low | Head-shift artifact, clippy, corpus sha256. | Fixed/subsumed. |

## Open — medium

| id | sev | what | notes |
|----|-----|------|-------|
| **M3** | med | **Floor walk edge case** (no walk past OOB B mapping). | `walk_reference_frames` gates on A-side energy only; B OOB is caught at measure time (`measure_a_win_at_delta` → NaN when `lo < 0` or `hi > b_len`), not by skipping bad walks. Abstain near haystack edges; tighten walk if real media hits it. |

## Deferred / accepted

| id | sev | what | notes |
|----|-----|------|-------|
| **M4** | med | **MP3 unvalidated.** | Codec-agnostic gate; manifest MP3 rows marked M4 / `ignore`; no FLOOR_OK calibration. |
| **M6** | med | **F4 bool decoy + nominal-floor veto (accepted).** | Signature problem; not residual veto scope. Production anchors the floor at **nominal**; bool lands on the decoy (nominal ≡ decoy) → headroom ≈ 0 → **abstain**, not veto. Energy at truth slides beyond lag reach (**M5**) → abstain. Score harness C1 uses a **truth-anchored** floor at the decoy frame — that proves **C1a** composition, not pipeline veto on F4. See § C1 contract. |
| **FD-1** | med | **Fractional-delay cancellation** (was L10 wiring path). | Integer lag caps cancellation (~−16 dB at 0.5 sample); headroom hides within reach (**M5**). See wiring plan §10 + **Fractional-delay review** below. |

## Open — low / smells

| id | sev | what | notes |
|----|-----|------|-------|
| **L6** | low | **Coarse outward walk step.** | Still `step_frames: window.max(1)` in `measure_fit_residual_verdict` (`patch_region.rs`). |

## Open — gaps in coverage

| id | sev | what | notes |
|----|-----|------|-------|
| **G1** | gap | **Residual on Pearson-only skipped gaps.** | **Veto skips (partial):** `GapPatchSkipReason::ResidualHeadroomExceeded` carries `headroom_db`, `floor_pre_db`, `floor_post_db`, `margin_db`; full `SeamResidualVerdict` on `GapPatchOutcome.residual` via `skipped_patch_with_residual`. **Still open:** Pearson/structure skips — `GapPatchStatus::Skipped` has only `reason`; no residual scalars. Analysis CSV (`diag_seam_residual`) covers offline cases. |

## Regressions

**None found** with `--residual-gate off`. Covered by `off_no_regression_baseline` (fast F1 in-memory).

## Fractional-delay review (FD-1 — deferred)

**Problem:** `seam_residual_for_side` picks an **integer** `best_lag`. Sub-sample misalignment between A and B leaves ~0.5-sample error → cancellation floor ~−16 dB even when content matches. **M5** mitigates for *placement* error via lag-centered search within `residual_lag_secs`; it does not fix *intrinsic* codec/alignment fractional delay at the correct placement.

**What was removed (L10):** Parabolic `frac_lag` from the ratio curve was computed but **never applied** to the B window before LSQ — dead work, not a partial implementation.

**What shipping FD-1 requires:**

| Piece | Work |
|-------|------|
| **Sub-sample lag estimate** | Re-introduce parabolic refinement on the integer lag grid *or* evaluate 2–3 fractional offsets around `best_lag` and pick minimum residual. |
| **Fractional B resample** | Before `lsq_residual_ratio`, resample the B window at `best_lag + frac` (linear interpolation minimum; sinc/polyphase if quality insufficient). |
| **API surface** | Optional `frac_lag` on `SeamFloorProbe` / internal `LagFitResult` for diagnostics only; gate uses improved `residual_db`. |
| **Cost** | 1–3× LSQ per lag side vs today (or one resample + one LSQ if frac fixed after grid). Still behind L3 lazy finalize (one probe per accepted gap). |
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
the relaxed calibration config. **`deadzone_punch_assert`** hard-locks punch-after-encode rows.

**Result (production fit-mode):**

| case (Grieg finale, truth placement) | encode geometry | off / veto | veto_rescue | seam min | floor | note |
|--------------------------------------|-----------------|------------|-------------|----------|-------|------|
| AAC **same** 128k | inject+encode | skip | **skip** | 0.021 | uninformative (NaN) | rescue inert — no floor |
| AAC **dual** 128k/192k (independent) | inject+encode | skip | **skip** | 0.021 | uninformative (NaN) | the production-realistic case; rescue inert |
| AAC **same** 128k | **punch-after-encode** | skip | **skip** | 0.022 | uninformative (NaN) | native borders; floor *still* uninformative |
| AAC **dual** 128k/192k | **punch-after-encode** | skip | **skip** | 0.022 | uninformative (NaN) | **decisive case** — native borders + independent encodes; floor *still* uninformative |
| Vorbis **same** 128k | inject+encode | skip | **patch** | 0.021 | informative −120/−120 | rescue *fires*, but floor is the **deterministic bit-identical-border** artifact (M2 caveat), not codec noise |
| AAC 102 s dual | inject+encode | skip | skip | 0.181 | uninformative (NaN) | — |
| benign control (14 s) | inject+encode | patch | patch | 0.300 | — (slid 33 ms) | escapes dead zone by sliding past the seam; slide > reach → residual **abstains** (M5), so 118 dB headroom does not veto |

**Findings:**
1. **Real same-master music seam Pearson at truth is in the dead zone** (0.02 at the finale, 0.30 at the
   benign anchor — both below `min_fill 0.35`). This **corrects** the earlier framing (H2-B /
   `floor_oracle_veto_rescue_real_broadband_codec`) that "real masters pass Pearson at truth." They do not;
   prior calibration runs *looked* like they passed only because the relaxed calibration config
   (`min_fill 0.0`, `fill_absolute_floor −0.05`) patches everything regardless of the seam.
2. **On genuinely-independent real encodes (AAC same + dual), rescue does not fire** — the floor goes
   *uninformative* (unmeasurable / NaN) at the dead-Pearson placement, so all three modes skip. Rescue is
   correctly inert; this is the safe outcome but means **no recovery**.
3. **The NaN floor is genuine, not an inject-then-encode artifact (punch-after-encode confirms).** The
   confound hypothesis was that injecting silence *before* encode corrupts A's borders (MDCT spread) and
   poisons the floor. The **punch-after-encode** geometry removes that — A is encoded from the *full*
   master, decoded, then the gap is zeroed in PCM, so A's borders are *native* and the gap interior is
   clean. Result: the floor is **still uninformative (NaN)** at the dead-Pearson finale seam for both
   same- and dual-bitrate AAC. So the uninformative floor is a *real* property of the broadband seam, not
   an oracle artifact — the confound hypothesis is **refuted**.
4. **The one real rescue flip (Vorbis same-bitrate) rides a −120 dB deterministic-encoder floor**
   (libvorbis → bit-identical borders, the M2 caveat), not lossy codec-noise cancellation — so it is not
   evidence that rescue recovers a *genuine* lossy seam.
5. **Net (G5 resolved):** across both inject-then-encode *and* punch-after-encode, on genuine real codec
   noise the rescue-trigger condition (dead Pearson **and** informative low-headroom floor) **does not
   occur** — the floor is uninformative exactly where Pearson dies. `veto_rescue` is, on current evidence,
   a **synthetic-only path**: keeping it non-default is correct, and its real-media value is now *actively
   unsupported*, not merely unproven. `veto`'s inertness (never false-vetoed a truth gap) remains
   reassuring.

**Caveat / follow-up:** the floor came back **NaN** (unmeasurable), not merely "high." Whether that is
genuine cancellation failure on broadband content or the floor probe *abstaining* at this geometry
(M3-adjacent: reference-window selection / B-mapping at the loud finale) is unresolved. It does not change
the product conclusion (no informative floor → no rescue), but a targeted probe of *why* the floor is NaN
at the finale would close it. **Scope:** one corpus master (Grieg) + mid-content control; other content or
lower bitrates (higher codec noise) could differ, but the strongest available test — native borders,
independent encodes, loudest broadband region — shows no rescue trigger.

## C1 validation contract (split)

The validity catalog splits **C1** into two claims. Only **C1a** is required to ship default
`veto`; **C1b** is optional end-to-end proof.

| ID | Claim | Status | Evidence |
|----|-------|--------|----------|
| **C1a** | **Gate composition** — when `informative ∧ headroom > margin ∧ Pearson passes`, `apply_residual_to_confidence` vetoes | **Proved** | `gap_fill_fit.rs` unit tests; `seam_residual_corpus.rs` (`f4_decoy_placement_informative_with_high_headroom`, `seam_residual_disagreement_oracles`). Score harness uses truth-anchored floor at fixed placement for F4 — valid for composition, **not** production pipeline geometry. |
| **C1b** | **Pipeline veto fires** — `PatchAudio` under `production_fit` skips with `ResidualHeadroomExceeded` on a realistic acoustic echo | **Not proved; optional** | No test asserts this skip reason today (`integration_residual_gate_smoke` explicitly excludes it in off-regression arms). **F4 is out of scope (M6).** Real codec (Run B): Pearson dead zone + uninformative floor — no veto trigger. If pursued: new acoustic-echo fixture (sine distortion / cancellation failure within lag reach), likely `validate_residual_gate.rs`. |

**Shipped safety contract** (more important than C1b for default `veto`): **C3** (never false-veto
truth under `production_fit`) + **C4** (`off` no-op) + **C2** (two-mic abstain).

## What remains (summary)

| Priority | ID | Action |
|----------|-----|--------|
| med | **M3** | Floor-walk vs B haystack bounds — only if field evidence |
| low | **L6** | Finer outward-walk step (optional) |
| gap | **G1** | Residual on Pearson-only skips (veto skips done) |
| deferred | **M4**, **FD-1** | MP3 calibration; fractional-delay feature |
| optional test | `finale_floor_nan_probe`, `c1b_acoustic_echo_pipeline_veto`, `p2_search_winner_bounds` | See catalog README backlog |

## Validation infrastructure

- **`tests/seam_residual_corpus.rs`** — score-level disagreement (C1a); CI-fast.
- **`tests/validate_residual_gate.rs`** — F4 pipeline abstain (`f4_decoy_residual_gate_vetoes_bool`); EC-6 discrimination.
- **`tests/validate_floor_oracle.rs`** — real-codec FLOOR_OK + gate + Run B (G5); `gate_real_codec_production_fit` (C2/C3); `deadzone_punch_assert`.
- **`tests/integration_residual_gate_smoke.rs`** — `off` no-op (C4).
- **`tests/residual_gate_catalog/`** — validity contract inventory (`matrix.toml`, README).
- **`crates/clip-sync-repair-harness`** — `residual_gate.rs`, `seam_residual.rs`, `floor_oracle.rs` shared runners.
