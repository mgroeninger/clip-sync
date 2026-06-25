# Residual / floor gate — findings ledger

Bugs, gaps, regressions, and smells found while building the residual/floor work (P0 prototype →
P1 plumbing → P4 default `veto`). Companion to
[residual-gate-wiring-plan.md](residual-gate-wiring-plan.md).

Legend: **status** = fixed / open / deferred; **sev** = high / med / low / gap / regression.

**Shipped (production):** default `residual_gate = veto`; unified lag radius; `SeamResidualVerdict`
on patched gaps; `residual_band` tag; `donor_relation` run diagnostic; real-codec calibration
(AAC/Vorbis/music); L3 deferred finalize on joint grid; L11 zero-alloc lag search.

---

## Fixed (verified)

| id | sev | what | fix / evidence |
|----|-----|------|----------------|
| **H1** | high | **Reference asymmetry.** Seam residual used the standoff-/low-energy-*trimmed* border template while the floor used *raw* A windows → spurious ~45 dB headroom at a correct same-master fill. | `seam_chosen_and_floor` measures chosen + floor on the **same raw window**; oracle headroom 44.7 → **0.0**; unit tests `seam_chosen_and_floor_*`. |
| **M1** | med | **Lag radii not unified** (seam ±64 vs floor ±512) → false-reject band 64–512 frames. | Single `max_lag_frames` from `residual_lag_secs` (10 ms default). Placement-offset sweep @16k: headroom **0.0 through offset 100**, ~38 dB at ≥200. |
| **H2-B** | high | **Broadband gaps skip at Pearson before residual matters** (Pearson ~0 at truth, headroom ≈ 0). | `veto_rescue` rescue path + `broadband_oracle_veto_rescue_patches_marginal`. Real-codec: `floor_oracle_residual_gate_real_codec`, `floor_oracle_veto_rescue_real_broadband_codec`. Rescue non-default (`veto_rescue`). |
| **M5** | med | **Real-codec reach false-veto** (search slid > `residual_lag`). | Lag-centered chosen probe + `beyond_lag_reach()` abstention. Tests: `seam_chosen_and_floor_lag_center_*`, `apply_residual_abstains_when_beyond_lag_reach`, `floor_oracle_vorbis_64k_veto_no_false_veto`. |
| **M2** | med | **`FLOOR_OK` uncalibrated on real codecs.** | **Calibrated:** `source_gap_oracle_floor_csv` over CC speech/ambient/music × {wav, aac, vorbis} + **Vorbis dual** (independent-encode floors). **`FLOOR_OK = −15` validated** for AAC and Vorbis incl. music. Low-bitrate content-dependent (AAC 64k uninformative on speech, informative on music). `two_mic` uninformative. Opus excluded (no decoder). See **M4** for MP3. |
| **G3** | gap | **Real-codec FLOOR_OK calibration corpus not built.** | `floor_oracle_integration` + manifest; matrix green. Vorbis dual rows added. |
| **L3** | low | **Per-candidate verdict cost under `--full`** (~13×13 floor probes per gap). | Joint grid defers probe; pearson-ranked `finalize_fit_outcome_residual` (winner first, fallbacks on veto/rescue reject). One probe per accepted gap in the common case. |
| **L7** | low | **Empty post-border forced `post_gate_frames = 1`** → spurious perfect cancellation. | `seam_post_gate_frames` returns 0; residual skips post side. Test `seam_post_gate_frames_zero_when_post_border_empty`. |
| **L11** | low | **Per-lag `Vec` allocation in `seam_residual_for_side`.** ~1025 allocs/side at ±512 default. | `seam_residual_for_side` takes `b_haystack` + bounds callback; borrows `b_haystack[lo..hi]` per lag (zero alloc). Applies to production (`measure_window_at_delta`) and test-only diagnostics. |
| **A1** | low | **Low-energy-prefix head-shift artifact** on trimmed post. | Subsumed by H1 raw-window fix. |
| **A2** | low | Clippy nits (`map_or`, loop index). | Applied. |
| **A3** | low | **Truncated `sha256` in `sources.toml`** broke corpus fetch. | Restored full hashes. |

## Open — medium

| id | sev | what | notes |
|----|-----|------|-------|
| **M3** | med | **`seam_floor_probe` refactor edge case.** `select_reference_window` + `measure_window_at_delta` no longer walks past a window whose B mapping is out of bounds. | Fixtures never hit it; may abstain near haystack edges where old code walked on. Restore walk-on-OOB if it shows up in real media. |

## Deferred / accepted

| id | sev | what | notes |
|----|-----|------|-------|
| **M4** | med | **MP3 excluded from calibration; production-unvalidated.** | Inject-then-encode asymmetry + libmp3lame determinism. MP3 rows `ignore = true` except one speech-128k Pearson-skip evidence row. Production rides codec-agnostic gate; abstains when uninformative. **Not a P4 blocker.** Parked: punch-after-encode oracle. |
| **M6** | med | **F4 bool decoy not caught by nominal-floor veto (accepted).** | Nominal-floor anchor → headroom ≈ 0 at decoy placement → veto abstains. Signature problem (`energy`/`auto` slide to truth), not residual veto scope. Test `f4_decoy_residual_gate_vetoes_bool`: bool+veto patches decoy; energy+veto patches truth. |

## Open — low / smells

| id | sev | what | notes |
|----|-----|------|-------|
| **L1** | low | **NaN handling.** Residual dB fields `NaN` when window doesn't fit; `PartialEq` on outcomes breaks with NaN. | Consider `Option<f64>` per field or sanitize before store. |
| **L2** | low | **Mono-only residual** vs design ("seam-selected channel"). | Fine for mono/stereo P1; **must add channel-following before 5.1** (see G4). |
| **L4** | low | **Wasted verdict compute on soon-skipped gaps.** Verdict before structure/waveform gate on non-deferred paths. | Minor after L3; reorder or accept. |
| **L5** | low | **`SeamFloorSource` casing.** CSV `{:?}` vs JSON snake_case. | Cosmetic. |
| **L6** | low | **Coarse outward walk** (`step_frames = window`). | Unlikely on typical borders; smaller step if walk lands between loud regions. |
| **L8** | low | **Mono peak-only reference energy gate.** Single-sample spike qualifies. | Per-channel plan may improve; mono path unchanged. |
| **L9** | low | **Dual `floor_db` naming.** `SeamResidual::floor_db` (theoretical) vs measured `SeamFloorProbe::residual_db`. | Latent — only in test-only `seam_residual_diagnostics` (L12). Remove field when retiring prototype. |
| **L10** | low | **`frac_lag` computed, never applied.** Integer `best_lag` used for cancellation. | Dead in production (`measure_window_at_delta` drops it). Overlaps wiring plan §10 fractional-delay ceiling. |
| **L12** | low | **Residual prototype path dead in production.** `seam_residual_diagnostics` / `SeamResidual` test-only; pipeline uses `seam_chosen_and_floor`. | Retire prototype (and L9/L10 dead fields) or keep as documented primitive. |
| **L13** | low | **`lsq_residual_ratio` treats silent B as ~0 dB residual.** | Return `None` when `‖b‖² ≤ ε` so lag is skipped. Unlikely when A is energy-gated. |

## Open — gaps in coverage

| id | sev | what | notes |
|----|-----|------|-------|
| **G1** | gap | **Skipped gaps carry no JSON residual** (only patched). | Analysis need met by `seam_residual_disagreement_csv`. Production-reporting gap remains open; low priority. |
| **G2** | gap | **`peak_normalize_f64` in `seam_pearson` is a no-op.** | Doc fixed (`seam-scoring.md`); code unchanged — remove when convenient. |
| **G4** | gap | **Channel-following residual not implemented** (overlaps L2). | Required for 5.1 validity. |

## Regressions

**None found** with `--residual-gate off`. Default is **`veto`** (P4). With gate off and `measure_residual = false`, `GapPatchOutcome.residual` is `None`.

## Findings that aren't defects (worth keeping)

- **Residual rescues broadband false-skips** (Pearson dead zone, headroom ≈ 0) — motivation for `veto_rescue`.
- **Floor-informative check = same-master regime gate** — `donor_relation` derived, not input.
- **Nominal-floor headroom is not an F4 echo detector** (**M6**) — signature mode handles editorial decoys.
- **Harness ↔ pipeline aligned** on unified `seam_chosen_and_floor` model.

## Validation infrastructure (where the evidence lives)

- **`tests/seam_residual_corpus.rs`** — direct-scoring harness (truth/decoy CSV, broadband, placement-offset sweep).
- **`tests/seam_residual_oracle.rs`** — in-memory oracle through `PatchAudio`; H2-B rescue (`broadband_oracle_veto_rescue_patches_marginal`).
- **`tests/floor_oracle_integration.rs`** + manifest — real-codec FLOOR_OK (`source_gap_oracle_floor_csv`); gate (`floor_oracle_residual_gate_real_codec`); **`veto_rescue` safety on real broadband/dual Vorbis** (`floor_oracle_veto_rescue_real_broadband_codec`).
- **`tests/energy_signature_production.rs`** — F4 bool+veto pipeline (`f4_decoy_residual_gate_vetoes_bool`, ignored).
- **Disagreement table** — `seam_residual_disagreement_csv` (ignored, ~96 s); CI-fast `seam_residual_disagreement_oracles` (~24 s).
