# TEMP — Anchor-bracket matchability pre-gate (perf lever #2, "cut k")

**Status:** green-lit 2026-07-22 on interim 9-pair evidence (below). Scope is deliberately narrow.
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
   (`try_anchor_seam_joint_search`, `gap_anchor_seam.rs:880`). On skip, the bracket contributes nothing to the
   pool — identical to the search running and the matchability arm rejecting.
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

---

## 5. Phase-2 realized-speedup measurement

Specced in the parent plan ("Phase 2 — realized-speedup harness", ~line 477); **do not duplicate here**. The
pre-gate PR is the first moment it becomes usable (a speedup is ON-vs-OFF wall-clock on the *same* build). It
needs, on the SAME binary: `--gate-perf-only` (gate loop + `bracket_stats`, skip Tier-3 diagnostics + corpus
write), the pre-gate on/off flag (§3.5), and a persisted `bracket-stats-summary.json` stamped with
`(a_source.id, b_source.id, ScanRecipe)` + the flag for hash-based comparability. Report per-pair (§1 caveat).

---

## 6. Definition of done

- [ ] Pre-gate implemented before the unified search in the bracket loop; reads existing matchability thresholds.
- [ ] Golden corpus byte-identical, pre-gate ON vs current build.
- [ ] Harness ON-run skip set == OFF-run `reject_matchability_only + reject_both` set (zero false skips).
- [ ] Window/stride parity verified (R1); debug assertion added during bring-up.
- [ ] Phase-2 `--gate-perf-only` + stamped summary + on/off flag landed in the same binary.
- [ ] Per-pair realized speedup measured on the licensed corpus and recorded (expect ≥ count% floor, ~53%+ pooled).
- [ ] Parent perf-plan lever-#2 entry updated to "LANDED + measured"; memory note added.
