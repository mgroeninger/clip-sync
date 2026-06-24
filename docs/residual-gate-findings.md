# Residual / floor gate — findings ledger

Bugs, gaps, regressions, and smells found while building the residual/floor work (P0 prototype → P1
plumbing → H1/M1 refactor → H2 experiment). Companion to
[residual-gate-wiring-plan.md](residual-gate-wiring-plan.md). Status as of the H2 experiment.

Legend: **status** = fixed / open; **sev** = high / med / low / gap / regression.

---

## Fixed (verified)

| id | sev | what | fix / evidence |
|----|-----|------|----------------|
| **H1** | high | **Reference asymmetry.** Seam residual used the standoff-/low-energy-*trimmed* border template while the floor used *raw* A windows → at any non-sample-accurate placement the trimmed template was offset by the standoff (≫ ±64 lag) → seam read ~0 cancellation while floor cancelled → spurious ~45 dB headroom at a correct same-master fill. | `seam_chosen_and_floor` measures chosen + floor on the **same raw window**; oracle headroom 44.7 → **0.0**; unit tests `seam_chosen_and_floor_*`. |
| **M1** | med | **Lag radii not unified** (seam ±64 vs floor ±512) → a true fill offset 64–512 frames showed a ~40 dB false-reject band. | Unified lag (single `max_lag_frames` from `residual_lag_secs`, both seam + floor). Placement-offset sweep @16k under the **10 ms default** (reach ≈160 frames): headroom **0.0 through offset 100, ~38 dB at ≥200** (was 34–41 across 100–512 with the old ±64 seam). No seam↔floor mismatch band. **Note:** reach now = configured lag, so a correct fill offset beyond it false-rejects (intended; tunable via `residual_lag_secs`, smaller at low sample rates). |
| **A1** | low | **Low-energy-prefix head-shift artifact**: trimmed `a_post` was offset a few dozen samples → spurious post-side headroom at the *true* fill (F1 92.3, F2 96.7). | Subsumed by H1 raw-window fix; both now **0.0**. |
| **A2** | low | Clippy: `map_or(true, …)` → `is_none_or`; `let…else { return None }` → `?`; loop index → `iter().enumerate()`. | Applied; clippy clean all-targets. |

## Open — high

| id | sev | what | notes / proposed fix |
|----|-----|------|----------------------|
| **H2-B** | high | **Broadband gaps skip at the Pearson gate before residual matters.** `fill_seam_correlations` (trimmed template, no lag) cannot score broadband seams — at the *correct* placement it reads Pearson ~0 (−0.023/0.047) while the residual reads headroom 0.0. Under default fit weights the gap skips (0.099/0.100). **`--full` does not help** (skips identically at ~33 min/gap — A-boundary grid is the wrong axis). | The residual **rescue** path is the fix: accept on `informative floor && headroom ≤ margin` even when `classify_fill_waveform_confidence` returns `Err`. Blocks P2 viability on broadband/real media until shipped. Verify default-weight placement lands near truth (structure-driven). |

## Open — medium

| id | sev | what | notes |
|----|-----|------|-------|
| **M2** | med | **`FLOOR_OK` uncalibrated on real codecs.** Synthetic floor (~−44 dB) is optimistic; real lossy codecs sit higher. The `informative` gate threshold is a guess until measured. | Needs the injected-gap **real-codec corpus** (reuse `tests/corpus/sources.toml` + ffmpeg re-encode for B; inject known gaps). If real floors routinely exceed `FLOOR_OK`, switch to a relative floor. |
| **M3** | med | **`seam_floor_probe` refactor edge case.** `select_reference_window` (energy gate only) + `measure_window_at_delta` no longer *walks past* a window whose B mapping is out of bounds — it returns `NaN` at the first energetic window instead. Old code walked on. | Our fixtures never hit it, but near haystack-coverage edges the new code may abstain where the old found a usable window. Low likelihood; document or restore walk-on-out-of-bounds if it shows up. |

## Open — low / smells

| id | sev | what | notes |
|----|-----|------|-------|
| **L1** | low | **NaN handling.** Residual dB fields are `NaN` when a window doesn't fit. `serde_json` emits `null` (OK), but `GapPatchOutcome` derives `PartialEq` and `NaN != NaN`, so an outcome with NaN residual won't equal itself — latent surprise for any equality/dedup. | Consider `Option<f64>` per field (None = didn't fit), or sanitize NaN before store. |
| **L2** | low | **Mono-only residual** vs design ("seam-selected channel"). Doc/impl mismatch. | Fine for mono/stereo P1 sources; **must add channel-following before trusting 5.1 numbers.** |
| **L3** | low | **Per-candidate verdict cost under `--full`.** With `measure_residual` on, the floor probe (±lag + walk) runs per joint-grid cell (~13×13) → the ~33 min run. | Compute the verdict only for the **winning** candidate, not every grid cell. |
| **L4** | low | **Wasted verdict compute on soon-skipped gaps.** Verdict is computed before the structure/waveform gate; gaps that then fail discard it. | Minor; reorder or accept. |
| **L5** | low | **`SeamFloorSource` casing.** Oracle CSV prints `{:?}` (`Border`) while JSON serializes snake_case (`border`). | Cosmetic. |
| **L6** | low | **Coarse outward walk in `select_reference_window`.** `step_frames` is set to the full reference `window` (`patch_region.rs` / harness), so each outward step jumps an entire seam window. Energetic audio in the gaps between steps is never considered. | Unlikely on typical borders; if walk lands on a quiet slice between two loud regions, floor may abstain or anchor poorly. Consider a smaller step (e.g. `window / 4`) if it shows up in real media. |
| **L7** | low | **Empty post-border forces `post_gate_frames = 1`.** `post_gate_frames = seam_gate_frames.min(a_post_border.len()).max(1)` (`patch_region.rs:505`) — when `a_post_border` is empty, post-side residual/floor still runs with a 1-frame window. | **Verified, and worse than first written:** with `window = 1`, `lsq_residual_ratio` fits the single sample exactly → residual is always 0 → **spurious −120 dB (perfect cancellation)** *if* that 1-sample window clears the energy gate (a click); only genuinely-silent post sides walk → `None` → NaN. The spurious-perfect case is the dangerous one. `worst_headroom_db` ignores a NaN side (`f64::max` drops NaN) but not a spurious-finite one. Fix: skip post measurement when the post template is empty. |
| **L8** | low | **Mono peak-only reference energy gate.** `select_reference_window` accepts a window when `peak ≥ absolute_silence_rms × 4` on the downmixed mono slice — not RMS. A single-sample spike qualifies as “energetic.” | [TEMP-residual-channel-alignment-plan.md](TEMP-residual-channel-alignment-plan.md) improves this for per-channel selection; mono path unchanged. Low risk on real borders; note if floor anchors on click/pop noise. |
| **L9** | low | **Dual `floor_db` naming.** `SeamResidual::floor_db` is the theoretical i16 requantization lower bound (`policies.rs:1371`); `SeamFloorProbe::residual_db` / `SeamResidualVerdict::floor_*_db` are measured nominal cancellation. Same word, different quantities. | **Verified, but mostly latent:** `SeamResidual::floor_db` is only surfaced via `seam_residual_diagnostics`, which is now **test-only** (see L12). The production path (`measure_window_at_delta`) discards it. Cleaner fix is to **remove the field** rather than rename. |
| **L10** | low | **`frac_lag` computed, never applied.** `seam_residual_for_side` parabolically refines sub-sample lag into `SeamResidual::frac_lag` (`policies.rs:1376`) but cancellation uses the integer `best_lag`. | **Verified dead in the hot path:** computed on *every* production residual call yet `measure_window_at_delta` drops it (`SeamFloorProbe` has no `frac_lag`). Overlaps wiring plan §10 (fractional-delay ceiling). Drop it or wire fractional resample. |
| **L11** | low | **Per-lag `Vec` allocation in `seam_residual_for_side`.** Each lag's `b_at_lag(lag)` `to_vec()`'s a B slice (`policies.rs`). Cost is `O((2·max_lag+1) × window)` allocations per side per probe — ~1025 small `Vec`s/side at the ±512 default. | **Verified.** Overlaps L3. Profile under `measure_residual` / `residual_gate != off` on `--full`. Mitigation: reused scratch buffer or borrow the haystack slice without alloc. |
| **L12** | low | **Residual prototype path is dead in production.** `seam_residual_diagnostics` / `SeamResidual` is now called only from policies unit tests (`policies.rs:2516/2550/2801`); the pipeline + harness use `seam_chosen_and_floor` → `measure_window_at_delta`. So `seam_residual_for_side` computes `floor_db` (L9) and `frac_lag` (L10) that nothing in production reads. | Either retire the prototype `seam_residual_diagnostics` (and its dead fields) or keep it as a documented direct-scoring primitive. Root cause behind L9/L10. |
| **L13** | low | **`lsq_residual_ratio` treats silent B as ~0 dB residual.** When `‖b‖² ≤ ε`, the fit returns `(gain=0.0, ratio=1.0)` → `residual_db ≈ 0 dB` instead of abstaining (`policies.rs:1297–1299`). A silent or near-silent B window at a lag can read as “no cancellation” rather than unmeasurable — can mask a silent B slice (overlaps L7’s 1-frame spurious-perfect case). | Unlikely when A reference windows are energy-gated, but B-side silence at the mapped offset is still possible. Consider returning `None` when `bb` is negligible so the lag is skipped, or propagate `NaN` rather than 0 dB. |

## Open — gaps in coverage

| id | sev | what | notes |
|----|-----|------|-------|
| **G1** | gap | **Skipped gaps carry no JSON residual** (only patched). The echo/veto (skip) disagreement data must come from the debug log, not JSON. | Acceptable for P1; note for the disagreement-table analysis. |
| **G2** | gap | **Pre-existing: `peak_normalize_f64` in `seam_pearson` is a no-op** (Pearson is scale-invariant). Harmless dead work; docs misattribute its purpose ("reduces level mismatch"). | Remove the call + fix the doc, or leave and note. |
| **G3** | gap | **Real-codec FLOOR_OK calibration corpus not built** (the injected-gap oracle's realistic tier). Overlaps M2. | Required to set thresholds with confidence before flipping `residual_gate` default to `veto`. |
| **G4** | gap | **Channel-following residual not implemented** (overlaps L2). | Required for 5.1 validity. |

## Regressions

**None found.** With `residual_gate = off` / `measure_residual = false` (defaults), the verdict is not computed and `GapPatchOutcome.residual` is `None` (serde-skipped) → CLI/JSON byte-identical to pre-change. Verified: 246 lib + 28 integration + smoke green; clippy clean all-targets.

## Findings that aren't defects (worth keeping)

- **Residual is a better gate signal than Pearson for broadband.** At a correct same-master placement, residual headroom = 0 while Pearson ≈ 0 — so residual *rescues* fills Pearson falsely skips, not only *vetoes* echoes. This is the empirical motivation for the rescue path (H2-B fix).
- **The floor-informative check is the same-master regime gate for free** — two-mic pairs can't cancel → floor uninformative → gate abstains, so `donor_relation` is derived, not an input.
- **Harness ↔ pipeline representativeness restored.** The direct harness was updated to the unified `seam_chosen_and_floor` model so its numbers predict pipeline behavior (the old ±64/±512 false-reject band was a harness artifact, now gone).
