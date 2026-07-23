# TEMP — Anchor-bracket matchability pre-gate (perf lever #2, "cut k")

> ## ⚠️ BLOCKING CAVEAT (2026-07-22, discovered during wiring) — the greenlight measured the *ceiling*, not the realizable gain
>
> The 44.8%/49.1% "doomed" numbers below are measured at the **searched** placement
> (`anchor_bracket_both_matchable_at_gate(placement = alignment.start_frame)`, `patch_region.rs:1723`): the
> fraction of brackets whose *chosen* placement fails matchability. That is the **ceiling** — the most *any*
> matchability pre-gate could skip. The **byte-safe** pre-gate this plan builds skips only when
> `windowed_max(seam_pearson) < floor` over the whole reachable window, and that realizable set is a
> potentially **much smaller subset**, for two independent reasons:
>
> 1. **The unified search optimizes a JOINT structure+wave score** (`unified_fit_score_with_repeat`), not seam
>    Pearson. The chosen lag can have low seam Pearson while another lag in the same window has high seam
>    Pearson. Such a bracket is "matchability-rejected" (counted in the 44.8%) yet its **windowed-max is high**,
>    so the pre-gate does **not** skip it.
> 2. **xcorr rescue is ACTIVE in production** (`anchor_bracket_both_matchable_at_gate` wires a `FftCorrelator`
>    whenever `residual_max_lag_frames > 0`, which config-validation forces `> 0`). Rescue extends "matchable"
>    down to `min_pearson − xcorr_ambiguous_band = 0.12 − 0.15 = **−0.03**`. So the byte-safe floor is **−0.03**:
>    a seam must be **anti-correlated below −0.03 across the entire reachable window** to be skipped. For the
>    decorrelated / missing-donor content this lever targets, windowed-max Pearson is typically **positive**
>    (~+0.1–0.3), which clears −0.03 ⇒ **not skipped**.
>
> **Consequence:** the realizable skip rate is unmeasured and may be a small fraction of 44.8%. Do **not** treat
> the greenlight as validating the *build* until the **realizable** rate (fraction of brackets where
> `anchor_bracket_matchability_doomed` fires over the reachable window) is measured. Instrumentation is a small,
> byte-identical, emission-only add to the existing `bracket_stats` path (predicate already implemented). See the
> new **§7 — realizable-rate re-measurement** before wiring any behavior change. All wiring is PAUSED pending it.

**Status:** green-lit 2026-07-22 on interim 9-pair evidence (below), then **PAUSED 2026-07-22** pending
realizable-rate re-measurement (see blocking caveat). Scope is deliberately narrow.
**Parent:** `docs/archive/TEMP-production-repair-perf-plan.md` — this doc does NOT restate the perf ranking,
the `bracket_stats` instrumentation, or the phase-2 harness spec; it references them. Read the parent's
lever-#2 entry (~line 430) and its **Superset proof (2026-07-22)** first — that proof is the load-bearing
correctness argument this whole change rests on.

---

## 1. What we're building and why

Every feasible anchor bracket currently pays for a **full `bracket_unified_search`** (`gate_structure_align`,
`patch_region.rs:1678`) before the matchability arm (`anchor_bracket_both_matchable_at_gate`,
`patch_region.rs:813`) can reject it. Post-lever-1, `gate_anchor_search` is 89% of the residual gate cost and
is `k`-bound (`k` brackets × ~3.56 s each). Lever #2 removes **matchability-doomed** brackets *before* the
search, attacking the `k`.

The measurement harness (parent plan, IMPLEMENTED) sized the ceiling. Full roll-up over **17 licensed pairs**
(`scripts/measure-anchor-brackets.ps1`, `CLIP_SYNC_BRACKET_STATS`):

| pair | brackets | doomed | count% | time% |
|---|---|---|---|---|
| 1 | 287 | 205 | 71.4 | 84.3 |
| 4 | 169 | 14 | **8.3** | 9.9 |
| 5 | 198 | 6 | **3.0** | 3.4 |
| 11 | 94 | 3 | **3.2** | 4.1 |
| 13 | 676 | 361 | 53.4 | 55.4 |
| 14 | 444 | 211 | 47.5 | 55.1 |
| 16 | 490 | 245 | 50.0 | 52.4 |
| 17 | 732 | 533 | 72.8 | 75.8 |
| … (2,3,6–10,12,15) | | | 27–43 | 29–52 |
| **pooled (17)** | **4820** | **2157** | **44.8** | **49.1** |

median count% 34.8, range 3.0–72.8.

**Read:** 14/17 pairs clear the 10% "worth building" bar; the three below (4, 5, 11) are clean,
well-registered pairs where the pre-gate is a no-op *and* a no-cost. Signal is strongest on the largest pairs
(13/16/17/14), so the pooled 45% is weighted toward the pairs that dominate wall-clock. `time% ≥ count%` on
**every** pair (doomed brackets are individually ~2× more expensive), so the deterministic **count fraction is
a conservative floor** on realized time saving. (The optimistic first-9-pair slice read 53%/57%; the full 17
regressed to a realistic 45%/49% — plan against the 45%.)

**Caveat carried into the build:** payoff is content-dependent (3%–73%). Correct for a skip-cheap-when-doomed
optimization (free on clean pairs), but phase-2 speedup must be reported **per-pair**, never as one headline.

---

## 2. The predictor (design unknown is already solved — this is the whole trick)

The matchability verdict currently *comes from* the expensive search: a bracket is doomed when the best
placement the search finds still fails `anchor_bracket_both_matchable_at_gate`. To skip before searching we
need a signal that is (a) cheap and (b) a **provable superset filter** — it may reject only brackets the real
searched-placement gate would also reject, so output stays **byte-identical**.

The parent plan's superset proof supplies it. `matchability_at_anchor` (`gap_anchor_seam.rs:560`) is a
seam-Pearson threshold at the *searched* placement, with GCC-PHAT xcorr rescue **only** inside the narrow
band `[min_pearson − xcorr_ambiguous_band, min_pearson)` (`gap_anchor_seam.rs:585-597`). Below
`min_pearson − xcorr_ambiguous_band` there is no rescue and Pearson alone fails. Therefore:

> `matchable-at-placement p ⟹ pre_pearson(p) ≥ min_pearson − xcorr_ambiguous_band` (and the post analog).

The searched placement lies inside the search window, so `max_over_window(pre_pearson) ≥ pre_pearson(p)`.
Contrapositive gives the pre-gate rule:

> **Skip the bracket iff `windowed_max(pre_pearson) < (min_pearson − xcorr_ambiguous_band)`
> OR `windowed_max(post_pearson) < (min_pearson − xcorr_ambiguous_band)`.**

Each disjunct independently proves *that side* is non-matchable at every placement in the window ⇒ non-matchable
at the searched placement ⇒ the real gate (which needs BOTH sides matchable) rejects. So the skip set is a
subset of the real reject set — pool identical, output byte-identical.

**Cheap because** `windowed_max(pearson)` over the search window is exactly what lever-1's
`fill_seam_correlations_band` (one FFT band per side per bracket) already computes — vs a full unified search.
`{pre-gate skip} ⊆ {searched-matchability reject}`, so the harness's `reject_matchability_only + reject_both`
time is the pre-gate's exact ceiling **upper** bound.

---

## 3. Implementation sketch

1. **Compute windowed max-Pearson per side per bracket** using the lever-1 band FFT
   (`fill_seam_correlations_band`) over the same search window and the same `pre_window`/`post_window`
   (`waveform_gate_frames`/`post_gate_frames`) the unified search uses. Reuse, do not reimplement.
2. **Insert the skip test before `gate_structure_align`/`bracket_unified_search`** in the bracket loop
   (`try_anchor_seam_joint_search`, `gap_anchor_seam.rs:880`). On a doomed bracket, **return
   `Err(SeamGateFailure::WaveformBelowThreshold { pre, post, min, best_attempt: None })` — the same failure
   variant the matchability arm returns** (`patch_region.rs:1760`). Do **NOT** "contribute nothing / skip the
   bracket entirely": that is *not* byte-identical (see §3a). Returning the matchability variant makes a
   pre-gated bracket indistinguishable, to `record_fit_joint_candidate_to_pool`, from a searched-then-rejected
   bracket — same `recorded_failure` variant, same first-wins ordering, same (empty) pool contribution.
3. **Threshold source:** `anchor_matchability.min_pearson` and `.xcorr_ambiguous_band` from config
   (`config.rs:246`, defaults `DEFAULT_ANCHOR_MATCH_MIN_PEARSON` / `_XCORR_AMBIGUOUS_BAND`). No new tunable —
   the pre-gate reads the SAME thresholds the real gate uses, or the superset property breaks.
4. **Non-finite guard:** treat a non-finite Pearson as `-inf` in the windowed max (a non-finite placement is
   non-matchable — `matchability_at_anchor` requires `pearson.is_finite()`), so an all-non-finite window skips.
5. **Feature/flag:** on by default in production; an off switch is needed for the phase-2 A/B (see §5).

**Interlock with lever-1:** the band FFT must cover the search window at the resolution the unified search
samples placements — if the search can land on a lag the band didn't evaluate, the "searched placement lies in
window" premise fails. Confirm window/stride parity before trusting byte-identity (see §4 risk R1).

---

## 3a. Byte-identity side-channel audit (2026-07-22, resolves the open pre-wiring question)

The superset proof (§2) covers only the **candidate pool**. A rejected bracket also feeds two side channels in
`record_fit_joint_candidate_to_pool` (`patch_region.rs:444`) that the proof did not touch. Both were traced:

- **`best_waveform` → `best_attempt` (`consider_waveform_attempt`, `enrich_waveform_failure`):** the best
  seam-Pearson attempt seen across brackets. Consumed **only** by (a) the skip-reason *display* string
  (`format_correlation_below_threshold`, `patch_result.rs:92`) and (b) `dual_fit_attempt`'s display. **Never a
  decision, never audio, never a serialized golden field** (`skip_reason_tag` emits only the coarse variant
  label `"correlation_below_threshold"`; pre/post/best are not serialized). Diagnostic-only.

- **`recorded_failure` (first-failure-wins):** `WaveformBelowThreshold`/`ResidualHeadroomExceeded` set-if-none;
  `StructureAlignmentFailed` (an "other") sets **only if `pool.is_empty() && recorded_failure.is_none()`**. This
  is the one that can flip audio. `dual_fit_eligible` (`patch_audio.rs:1566`) =
  `request_dual_fit && !matches!(fail, StructureAlignmentFailed)` — it keys on the failure **variant**. If the
  pre-gate merely *dropped* a doomed bracket (which would have produced `WaveformBelowThreshold`), a later
  `StructureAlignmentFailed` bracket could become `recorded_failure`, flipping `dual_fit_eligible` false and
  **silently losing a dual-fit rescue** (a real skip↔patch flip → audio divergence).

  **Fix = §3.2's synthetic-Waveform return.** By returning `WaveformBelowThreshold` the pre-gate preserves the
  `recorded_failure` variant and ordering, so `dual_fit_eligible` and the whole patch/skip decision are
  **provably byte-identical in audio**. (Dual-fit's rescue inputs come from `df` region mono, never from
  `best_attempt`, so nothing else in that path depends on the skipped bracket.)

**Residual risk — `seam_shape` (R4, empirical).** The gap's *reported* `CorrelationBelowThreshold { pre, post }`
is `recorded_failure`'s pre/post = the **first-failing bracket's** values. On the skip path these feed
`patch_tier_from_correlation_skip` **and** `classify_seam_shape` (`gap_tags.rs:398-417`), and `seam_shape` **is
serialized in the goldens**. The synthetic return must supply pre/post values:

- `patch_tier`: `min(pre,post) < hard_floor ⇒ HardSkip`. A doomed side is `< (min_pearson − band)`; **iff
  `(min_pearson − band) ≤ hard_floor`** (config check — verify before trusting) this is HardSkip on *either*
  valuation → golden-stable.
- `seam_shape`: reads the **non-doomed** side too. Per-side windowed-max on that side can cross the `0.85`/`0.27`
  `classify_seam_shape` boundaries differently than the true searched placement (which the pre-gate skips). So
  windowed-max values *can*, in a narrow case (first-failing bracket is pre-gated, gap is all-fail/skipped,
  non-doomed side crosses a bucket), perturb the serialized `seam_shape`.

There is no cheap way to reproduce the exact searched-placement pair without searching. Decision: **supply
per-side windowed-max as the synthetic pre/post (honest cheap upper bound) and let the golden corpus adjudicate
`seam_shape`** — the plan already mandates golden byte-identity as the primary gate, so a divergence surfaces
the exact gap and we refine (prove bucket-stability, or narrow the skip). Audio is unaffected regardless.

---

## 4. Correctness bar and risks

This is a **quality-affecting change disguised as a perf knob**: a false skip = a bracket that would have
passed gets dropped = a silently lost patch. Acceptance is **byte-identical output**, not "close".

- **Primary gate:** golden corpus regression — `crates/clip-sync-repair/tests/gap_corpus/` — must be
  byte-identical with the pre-gate ON vs the current build. Any divergence is a bug in the superset argument,
  not an acceptable trade.
- **Harness cross-check:** re-run `measure-anchor-brackets.ps1` with the pre-gate ON; the set of brackets that
  never reach the search must equal exactly the `reject_matchability_only + reject_both` set from the OFF run
  (same `a_start_secs`). A skip that ISN'T in that set is a false skip → correctness bug.
- **R1 — window/stride parity (highest risk):** if `fill_seam_correlations_band` and the unified search
  disagree on which lags are candidate placements, `windowed_max` can under- or over-count. Verify parity
  explicitly; add a debug assertion (searched placement's Pearson ≤ recorded windowed max) behind a validation
  feature during bring-up.
- **R2 — xcorr rescue band:** the proof depends on rescue being confined to `[min_pearson − band, min_pearson)`.
  If any code path lowers the effective floor (e.g. a different `min_xcorr_peak` interaction), the threshold is
  wrong. Re-audit `matchability_at_anchor:585-597` if the golden corpus diverges.
- **R3 — "both matchable" asymmetry:** confirm the real gate requires BOTH sides (it does — `anchor_bracket_both_matchable`);
  the OR-of-per-side-skip is only valid because either side failing dooms the bracket.
- **R4 — `seam_shape` side channel (empirical, see §3a):** synthetic windowed-max values feed
  `classify_seam_shape` via `recorded_failure` when a pre-gated bracket is a skipped gap's first failure. Audio
  is unaffected (variant/ordering preserved); the golden corpus is the arbiter for the serialized `seam_shape`.
  Verify `(min_pearson − band) ≤ hard_floor` for `patch_tier` stability. If a golden's `seam_shape` diverges,
  that is the case to inspect — not necessarily a correctness bug (it's diagnostic tier metadata, not audio),
  but it must be understood and signed off, not silently accepted.

---

## 5. Phase-2 realized-speedup measurement

Specced in the parent plan ("Phase 2 — realized-speedup harness", ~line 477); **do not duplicate here**. The
pre-gate PR is the first moment it becomes usable (a speedup is ON-vs-OFF wall-clock on the *same* build). It
needs, on the SAME binary: `--gate-perf-only` (gate loop + `bracket_stats`, skip Tier-3 diagnostics + corpus
write), the pre-gate on/off flag (§3.5), and a persisted `bracket-stats-summary.json` stamped with
`(a_source.id, b_source.id, ScanRecipe)` + the flag for hash-based comparability. Report per-pair (§1 caveat).

---

## 7. Realizable-rate re-measurement (DO THIS BEFORE ANY WIRING — see blocking caveat)

The greenlight measured the ceiling (searched-placement matchability reject). Measure the **realizable** rate —
brackets `anchor_bracket_matchability_doomed` would actually skip over the reachable window — before committing.

**Instrumentation (byte-identical, emission-only, gated on `CLIP_SYNC_BRACKET_STATS`):**
- Inside `gate_structure_align` the reachable window is available: `start_lo/hi = offset_nominal_start ±
  (params.cfg.search_radius_frames + gap_structure::structure_fine_polish_frames(params.cfg.bin_frames))`,
  clamped to `[0, cache.b_mono.len()]`. (Wider is safe; this is a superset of the search's own
  `[nominal ± search_radius]` + fine polish.)
- When stats are enabled AND `anchor_seam_bracket`, call `anchor_bracket_matchability_doomed(templates,
  gap_frames, waveform_gate_frames, post_gate_frames, start_lo, start_hi, &params.cfg.anchor_matchability)` and
  thread the bool out to the emit site (add a `pregate_doomed` field to the `bracket_stats` event). Pure
  side-effect — production behavior unchanged.
- Roll up in `measure-anchor-brackets.ps1`: report `pregate_doomed` count/time **against** the existing
  `reject_matchability_only + reject_both` ceiling, per pair. `pregate_doomed ⊆ reject_matchability` must hold
  (superset proof); the ratio `pregate_doomed / reject_matchability` is the realizable-vs-ceiling efficiency.

**Decision gate:** if realizable count% clears a worth-it bar (say ≥10–15% pooled, per-pair on the big pairs),
un-pause and wire §3. If it's near-zero (the −0.03-floor prediction), the byte-safe mechanism does not pay off —
record the negative result, drop the lever, and redirect to the parent plan's other levers (e.g. FFT the
per-bracket score sweep, `[[production-perf-gate-search-dominates]]`).

**How to run it (2026-07-23 — the equivalence-gate finding; do NOT use a production/`-PerfOnly` run):**
`pregate_doomed` is emitted inside `evaluate_seam_gate_fit_candidate`, which only fires for a gap that actually
*enters* the anchor-seam search. In a plain production repair the **equivalence gate** (`skip_equivalent_gaps`,
ON by default) drops equivalent gaps from the fill plan *before decode/patch* (`patch_audio.rs:337-339`), so on
equiv-heavy content the anchor search — and therefore `bracket_stats` — **never runs** (empirically: pair 1's 17
gaps are all `[equiv:]` → a production run emitted zero bracket_stats). The `--gap-fingerprints` oracle path
(`compute_region_measurements` → `oracle_score_fit_candidate(..., anchor=true)`, `gap_fingerprint.rs:2703`)
force-scores every bracket of every gap regardless of the equivalence gate — that force-scoring IS the ~82%
wall-clock this lever optimizes (`[[8g5-fingerprint-perf-deferred]]`, `[[production-perf-gate-search-dominates]]`
`char_gate_search`) and IS the population the 44.8% ceiling was measured on. So the realizable re-measurement
**must be the full fingerprint run** (default mode of `measure-anchor-brackets.ps1`, reusing the greenlight
manifest + recipe): `./scripts/measure-anchor-brackets.ps1 -Manifest <greenlight-pairs.csv> -ScanArgs "--min-gap-ms 500"`.
No shortcut avoids the ~10-12h — the measurement is a byproduct of the scoring that dominates it. (For a fast
preliminary read, run just the wall-clock-dominant pairs 13/16/17; the realizable/ceiling ratio is per-bracket so
a subset is internally valid, but do the full 17 for the final go/no-go.)

> **Strategic note this surfaces:** because the equivalence gate already skips the anchor search in production,
> lever #2's *production* payoff on equiv-heavy content is ≈0; its real beneficiary is the **characterization/
> fingerprint tooling** (the force-scoring path). "Byte-identical production audio" remains the *safety* bar on
> the shared predicate (it must not perturb the rare non-equivalent gaps that DO reach the production anchor
> search), but the *speedup* accrues to the fingerprint/oracle workload. Weigh the go/no-go accordingly.

> **Tooling cleanup (do this):** a `-PerfOnly` switch was briefly added to `measure-anchor-brackets.ps1` to run
> the production decision pass without the oracle dump. The equivalence-gate finding makes it the wrong tool for
> this measurement (it forces `--no-skip-equivalent-gaps`, measuring a population production normally skips).
> **Remove `-PerfOnly`** — the default fingerprint mode is the sanctioned re-measurement. *(Removed 2026-07-23.)*

---

## 6. Definition of done

- [ ] **Realizable-rate re-measurement (§7) clears the worth-it bar** — GATES everything below.
- [ ] Pre-gate implemented before the unified search in the bracket loop; reads existing matchability thresholds.
- [ ] Golden corpus byte-identical, pre-gate ON vs current build.
- [ ] Harness ON-run skip set == OFF-run `reject_matchability_only + reject_both` set (zero false skips).
- [ ] Window/stride parity verified (R1); debug assertion added during bring-up.
- [ ] Phase-2 `--gate-perf-only` + stamped summary + on/off flag landed in the same binary.
- [ ] Per-pair realized speedup measured on the licensed corpus and recorded (expect ≥ count% floor, ~53%+ pooled).
- [ ] Parent perf-plan lever-#2 entry updated to "LANDED + measured"; memory note added.
