# Backlog

Open follow-up work for `clip-sync`. Architecture: [PLAN.md](PLAN.md). Repair pipeline:
[docs/pipeline.md](docs/pipeline.md). Test corpus: [docs/dev/corpus-validation.md](docs/dev/corpus-validation.md).
Error handling: [docs/error-mapping.md](docs/error-mapping.md).

Last updated: 2026-10-05.

**How this doc works**

- **Everything here is startable.** Each row is a problem, a next action, and a done-when. If a row
  cannot be started, it does not belong here.
- **No history, no rationale, no prohibitions.** Those live in
  [docs/dev/settled.md](docs/dev/settled.md) — the verdict ledger. **Read it before proposing
  anything here**; refuted ideas, latched deferrals, and blocked items are recorded there with the
  evidence that settled them.
- **Plans** — active drafts under `docs/dev/TEMP-*.md`; catalog:
  [docs/dev/README.md](docs/dev/README.md) § Plans. Archiving a plan means *also* adding a
  `settled.md` row and deleting the corresponding row here.
- Shipped work is recorded in `docs/dev/archive/*` and git history.
- Out of scope: [PLAN.md](PLAN.md) § Out of scope.

---

## Fill placement

Two ear-confirmed wrong patches, two distinct defects. Findings:
[gap-vocabulary.md](docs/dev/gap-vocabulary.md) § *Known holes*,
[archive/TEMP-31g7-model-vs-findings.md](docs/dev/archive/TEMP-31g7-model-vs-findings.md) §8.
Durable `lag_decision` behaviour: [gap-fingerprint.md](docs/dev/gap-fingerprint.md) § *`lag_decision`*.
The 31/g7 **pair-sequence** skip classifier is **blocked** — [settled.md](docs/dev/settled.md) § D.
The **per-gap** keep-path refuse (early locks, abut → skip) shipped 2026-08-31; the
post-splice drain of that silence is latched — [settled.md](docs/dev/settled.md) § C.
Seven diagnostic-field and conditional-ear-label leftovers moved to [settled.md](docs/dev/settled.md)
§ B / § C on 2026-08-24.

**Already categorized (no row).** 26/g6 is Hole 1 (well-posed landmark ignored; axis-2 `waveform_min`
won at −1450 ms, 2026-08-27). 17/g34 ear-pass 2026-08-27: patched, heard misaligned / possible pre
echo, **cannot tell** 26-class wrong-when (jazz self-similarity; pair used start-clip offset at a
gap that sits in the end-clip window). Hole 1 was n=1 until 2026-09-21 (13/g16, row below). 47/g14 ear-pass 2026-09-18: clean, and not a failure-to-move — the written fill starts
+214.3 ms vs +213 ms shoulders, `registration_residual_ms` +1.0 ([settled.md](docs/dev/settled.md) § A).

**Disposition tree** — every remaining row lands a category, then a successor.

1. Right when? Licensed landmark vs applied: agree / contradict / no opinion. The check is
   agreement, not magnitude ([json-output.md](docs/json-output.md) `registration_residual_ms`).
   Use dumps from 2026-09-18 or later: before that fix the field read the ~350 ms border standoff
   (median |value| 329 ms over the 209 non-dual-fit patched gaps of the 2026-09-16 corpus run).
2. Contradict + both-live unique → Hole 1. Contradict + bridge ≈ 0 → Hole 2 (skip; blocked).
3. Agree + audible → join, not search.
4. We skipped → donor fail at the true when, or A-side never offered a candidate.

| Item | Next action | Done when |
|------|-------------|-----------|
| **Early fills that contradict licensed shoulders (Hole 1, now n = 2)** | `registration_residual_ms` tracks the audio-measured nearer-seam mismatch at r 0.99 (median \|Δ\| 10 ms; 43-gap census 2026-09-21, settled § B step row), so the report already carries Hole 1's missing *choice vs registration* axis. 2026-09-18 corpus reports: 7 of 209 non-dual-fit patches read ≤ −30 ms — 26/g6 (−901, audible, Hole 1), 17/g34 (−524, ear *cannot tell*), 19/g2 (−120, ear *unsure*: a B-side voice present in every clip masks the seam), 1/g14 (−92, marginal, unheard), **13/g16 (−83, audible repeat at the pre seam, ear 2026-09-21)**, 32/g13 (−36, unheard), 41/g5 (−30, ear clean). 13/g16: shoulders +236.1 / +273.1 ms (r 0.98 / 0.99), fill +163.3 — early of both; anchor seam, bracket −150 / +250 ms, seam r 0.62 / 0.997, `confidence: high`. The fingerprint's dual-fit reproduction passes the gate on 13/g16, 19/g2 and 41/g5 (each seam ≥ 0.94 at its own lag). **13/g16 diagnosed 2026-09-21 — a search defect, not a scoring one (26/g6 was scoring).** Throwaway oracle (every `consider` in `unified_search_best_fill_start`, dump reverted; production `--patch-only` reproduced exactly): inside the applied bracket (start −7200 / end +12000 frames, one of 25) the seam Pearson is a saw-tooth with a 20–30 ms period (+150 → 0.31, +160 → 0.10, +190 → 0.02), which the 50 ms coarse grid samples at arbitrary phase. The coarse points around the shoulder lags (+250, +300 start offset; fill lag = offset + 10 ms trim-head) had the best `structure_combined` (1.82 / 1.86 vs 1.53) but landed on troughs (`waveform_min` ≈ 0.11) and ranked 3 and 2 behind +150 (0.31); refine densifies only ±50 ms of the coarse winner, so 100–200 ms. Scored at 0.5 ms, the objective's own best in that bracket is +284.5 (total 0.854 vs applied 0.783). Counterfactual with refine widened to 100–350 ms: placement +314 ms, bracket −7200 / 0, seams 0.83 / 0.39, `confidence: high`, residual −82.8 → +41.2 (late of both shoulders by ≈ +51 / +88, inside α's clean-late range). Ear 2026-09-21: the counterfactual is *better but not right* — a hard stop inside a spoken word. Verified in the audio (the fill's first 40–60 ms overwrites live A, so it can be correlated against A itself): the original fill opens on A content from 72.8 ms earlier (r 0.99, the heard repeat), the widened one on content 88.2 ms later (r 0.99, the heard cut). **So the grid is not the root cause.** The shoulders disagree by 37 ms, so no single lag is right at both seams (the two seam errors always differ by exactly the step); and the objective's short-window seam Pearson is saw-toothed on voiced speech, so its peaks are not registration — fixing the grid only moved the fill to a different wrong peak. "Inside α's clean-late range" is not evidence: α's late fills were clean on whatever sat at their seams, and an 88 ms skip mid-word is not. The registration-consistent repair here is two lags (dual-fit: pre part at the pre shoulder's lag, post part at the post shoulder's, 37 ms jump inside the fill), which the fingerprint's reproduction passes at 0.994 / 0.984; production never tried it because the single-lag seams cleared the 0.35 floor. Container timestamps do not locate the step (2026-09-21, `ffprobe -show_packets`, both files, whole length): every AAC packet in A and B sits on a gapless 1024-sample clock (max deviation 0.67 ms = Matroska's 1 ms rounding; constant −44 ms from the first packet), so A's 622 ms of silence was encoded as real samples and the 37 ms step is in the audio content, upstream of encoding — which file lost or gained it is not decidable from the containers. Unverified: whether dual-fit puts its jump between syllables. Forced dual-fit render 2026-09-21 (`--min-fill-correlation 0.95`; dual-fit uses the same floor): seams 0.994 / 0.984, `balanced`, `dual_fit_step_real`; audio lag track +246.1 → +283.1 (step 37.0; both 10 ms late of their shoulders), jump at ≈ +240 ms into the gap. The center channel is active across the whole fill (−19 to −35 dBFS, no pause); the jump sits at −24 to −27 dBFS, ~10 dB above the fill's quietest stretch (+90–130 ms, ≈ −35, just before a sharp onset at +140) — the jump point is not chosen for low energy. **Ear 2026-09-22: inaudible — indistinguishable from the B soundtrack**, despite the mid-speech jump (n = 1: a 37 ms drop inside continuous speech was not heard, where a 73 ms repeat and an 88 ms skip at the seams were). So 13/g16's defect is the routing, not dual-fit: the registration-consistent repair exists and is clean, and production never tries it because the single-lag seams cleared 0.35. Next: design the routing rule — when both shoulders are licensed and the single-lag fill's seam errors (fill lag − each shoulder lag) exceed some bound, try dual-fit first; size it on this row's cohort (the 7 negative-residual gaps plus the late-of-both side of the 43-gap step census, settled § B) and ear-pass 1/g14 and 32/g13 | A routing rule is specified with its trigger sized on the corpus, and its first cohort is rendered and ear-passed. The bound below 75 ms waits on the *Dual-fit interior edit* row |
| **Dual-fit interior edit** | Dual-fit's bridge is `gap + step` frames, and `trim_at_lowest_energy_interior` reconciles it hard: a long bridge is cut with no crossfade, and a short one holds **one sample** for \|step\| frames (a 29–35 ms near-DC dropout, positioned by a one-frame energy window). That hold is the dual-fit blip in the routing plan's § 6b listen (routing plan § 7, 2026-10-02). Plan: [TEMP-dualfit-interior-edit-plan.md](docs/dev/TEMP-dualfit-interior-edit-plan.md). Next: § 2 checklist (domain fn + tests, flag, dump fields) | § 3 measurement gate passes and § 4 blind listen returns *Fixed*; default flipped and legacy branch deleted |
| **Gap start lands after A's dropout begins (soft leading edge left in the output)** | 50/g7, found 2026-10-05 (interior-edit plan § 4): a blip heard at ~2.8 s in every arm across three blind sittings sits in audio the patch never touches. Raw A's dropout begins at 2.974 s in the `--gap-listen` clip (a −20 dB fall in 2 ms on every channel, to −51…−65 dBFS) and only reaches digital silence (−104 dB) by 3.004 s. The write starts at 2.990 s (fill at 3.000 s less the 10 ms crossfade), so ~16 ms of partial dropout survives before the pre seam. That transition is the largest spectral-novelty event in the clip (100th percentile). Likely mechanism, not verified: the leading edge sits around the scan's `absolute_silence_rms` 0.001007 (−60 dBFS), so the detector calls it programme until it falls below the floor. The other suspect is the scan's boundary resolution. `gap_start_extend_on_pre_seam_fail` cannot catch it: it fires only on a pre-seam failure, and this seam passed. Whether it applies on the dual-fit path is also unchecked. The mirror case, a soft trailing edge at gap end, is unmeasured. Next: census the corpus. For every patched gap, locate A's dropout onset from the shoulder (the first ≥ 15 dB fall in 2 ms bins over the ~100 ms before the reported gap start) and from the far side at gap end. Report the onset-to-boundary distance per edge, then ear-check the largest. The `--gap-listen` clip is enough (patched == raw A before the write) | The per-edge onset-to-boundary distribution over the corpus is known. Either boundaries snap to the dropout onset (with a test on a soft-edged fixture), or the residual is shown inaudible by ear on the worst cases |
| **The written fill is advanced one crossfade width ahead of the plan, on every patch** | `apply_seam_crossfade` consumes `b_fill[..cf]` into A's pre-gap tail and starts the gap interior at `b_fill[cf]` (`domain/policies/seam_splice.rs:116`), so B content index `k` lands at A frame `gap_start + k - cf` — the written fill carries B material `cf` ahead of where the plan placed it. `registration_applied_ms` is assembled plan-side (`align_adjustment_secs` + Fit's trim head, `patch_result.rs:602`) before the writer runs and carries no crossfade term, so report and WAV differ by exactly `cf`. At the `crossfade_ms` default of 10 that is 480 frames at 48 kHz, and `effective_seam_crossfade_frames` clamps only below `gap_frames/2`, so with a 500 ms silence hold no corpus gap ever gets less — the offset is a fixed config value, not a per-gap derivation. Mechanism and sign pinned in-process by `apply_seam_crossfade_displaces_fill_content_by_the_crossfade_width` (ramp fill; the pre-existing tests use a constant fill and are structurally blind to displacement). This is the constant 10 ms between the patch report's residual and the audio measurement (routing plan § 3), and `fill_level.rs`'s module note records the same displacement independently ("the bin grid sits one crossfade width off A's timeline"). **Open question, not a known defect:** the advance is deliberate — it is how the interior avoids a silence ramp — but it is uncompensated, and against the 50–100 ms candidate routing bounds it is 10–20% of the threshold. The candidate fix is to cut `b_fill` `cf` frames earlier so the interior still starts at the planned B content, which would make report and WAV agree and remove a systematic term from every seam-error census. Scales with `--crossfade-ms`, so a large-crossfade run is affected proportionally. Not started; no ear evidence either way that the advance is audible | Either the fill cut absorbs `cf` and report/WAV agree with no frame conversion, or the advance is measured as deliberate-and-harmless and the conversion is documented as a permanent named term rather than a pending fix |

## Fill normalize boosts against a non-local reference

Ear-confirmed defect, **n = 5** (45/g4, 45/g5, 19/g13, 5/g6, 15/g3; 31/g7 audible but confounded
by its map-step straddle). Sizing, attribution and ear labels:
[TEMP-fill-normalize-local-reference-2026-08-25.md](docs/dev/archive/TEMP-fill-normalize-local-reference-2026-08-25.md)
— **archived 2026-08-26**, all four of its questions promoted or closed. Its Results 1–5 are the
durable measurement; its design half (a cap on the RMS ratio, the ~4.5 dB corridor) is **void**, see
that note's § *What survives*.
Durable reading: [json-output.md](docs/json-output.md) § FillLevelCheck and
[pipeline.md](docs/pipeline.md) § Fill normalization (pair 19/g013, `normalize_gain = 3.88`,
energy-pooled 5 s + 5 s vs −52 dB local shoulders). Boost-only PR archived:
[archive/TEMP-fill-normalize-boost-only-plan.md](docs/dev/archive/TEMP-fill-normalize-boost-only-plan.md).
B-relative gain is **refuted** ([settled.md](docs/dev/settled.md) § A); the unvalidated attenuation
half is latched (§ C).

**Both halves shipped; the estimator this section was written about no longer exists.**
The whole-program half landed 2026-08-27
([archive/TEMP-program-gain-plan.md](docs/dev/archive/TEMP-program-gain-plan.md)) — it applies
`g_program = 10^(-offset_db/20)` to every B-derived fill and defaults `normalize_fill` **off**, which
removed the n = 6 over-boost outright without crushing 24/g46. The **local** half landed 2026-08-31
([archive/TEMP-local-paired-gain-plan.md](docs/dev/archive/TEMP-local-paired-gain-plan.md)):
`compute_fill_gain`'s wide-A-border RMS ratio is **deleted**, replaced by a paired B−A collar
residual around each hole. Its acceptance roll-up is in [pipeline.md](docs/pipeline.md) § Fill gain
— 277 of 308 patched gaps applied, 150 boosts / 127 cuts, median +0.011 dB, zero gaps at any rail,
and all seven former 12 dB clamp pins now inside ±0.16 dB.

What is left below is **not** about the estimator. Do not re-litigate the default here: flipping
`normalize_fill` on wants an ear pass on gaps that move under the *collar*, and the corpus numbers
say it is conservative, which is not the same as saying it is right.

| Item | Next action | Done when |
|------|-------------|-----------|
| **Ear-label the gaps the collar *moves*** | **Retargeted 2026-08-31 — the original seven are answered.** The 2026-08-25 census's seven 12.000 dB pins were `compute_fill_gain` artifacts: under the paired collar all seven read within **±0.16 dB** (5/g6 +0.155, 10/g9 −0.065, 10/g11 +0.066, 19/g2 +0.061, 43/g7 −0.097, 44/g6 −0.070, 45/g4 abstaining), six of them admitted on two-sided collars of n = 9 … 76. Six of the seven demands are gone by measurement, not by refusal, so there is nothing left to label there — 5/g6 stays labelled audible as evidence about the **old** estimator. What now needs ears is the set the collar actually changes, because that is the only missing input to the `normalize_fill` default: the **8 gaps applying more than 0.5 dB** (50/g8 +1.573, 22/g2 −0.988, 32/g5 +0.915, 23/g11 −0.722, 40/g6 +0.587, 48/g4 +0.572, 50/g6 −0.534, 36/g3 −0.522) and the **15 admitted on 8–11 pairs** just over the floor, of which 48/g4 is both — n = 9 at `se_db` 0.859 against a 1.0 bound, applying +0.572 dB. `--gap-listen` each under `--normalize` and label. Note the **cuts are new**: 127 of 277 applied gaps attenuate, a direction no ear label in the corpus covers | The moved-set labels exist and the audible fraction under `--normalize` is known, split by direction |
| **Re-label `2026-08-08-edge-listen`** | Pairs 7/8/12 are swapped. Required before quoting "lowest labelled clean" as a bound | Relabelled |
| Residual seam `g` (`a ≈ g·b`) as the production gain | Candidate replacement — estimates level from the **seam** rather than whole-fill RMS. Needs verdict plumbing plus corpus rules for when the estimate is trustworthy | Evaluated against the sweep |

## Donor registration

From the 2026-08 equivalence-band / donor-registration review. Donor Apply and the fill-level check
shipped. The conditional donor test is **blocked on a count** — [settled.md](docs/dev/settled.md) § D.

| Item | Next action | Done when |
|------|-------------|-----------|
| **`--replay` completeness gate compares correlations at `f64::EPSILON`** | After the Apply-shift fix (2026-09-10) the 51-pair scan set reports 80 registration mismatches. 105 of the 113 field diffs are under 1e-6 — different-summation noise against an absolute `f64::EPSILON` compare — so the gate fails records that reproduce. The other ~6 are real: correlations moving by 1e-3 to 0.24, two with a bin count changing (20→21), one with the lag moving. Understand those six before touching the tolerance; a replay advertised as exact should be exact | The mismatch count means "did not reproduce" and the tool's exit code is actionable |

## Fingerprint contracts (C3)

R1–R3 + C1 + C2 shipped; all eight groups carry a contract —
[archive/TEMP-fingerprint-field-clarity-plan.md](docs/dev/archive/TEMP-fingerprint-field-clarity-plan.md) § 2.8.
Two deferred-on-trigger leftovers moved to [settled.md](docs/dev/settled.md) § C.

| Item | Next action | Done when |
|------|-------------|-----------|
| **Contract `status.patched` on the production report** | Answer two design questions first: (a) `Contracted<T>` needs a `T`, but `GapPatchStatus::Patched` is an enum variant with inline fields, and `contract.rs` keeps reader metadata out of domain types; (b) a dump is one gap per file while a report is all gaps in one — per-gap stamping repeats ~90 KB, and a report-root `_contracts` map is what § 2.6 rejected for dumps. Whether that rejection survives a **report**-shaped file is the open question | Fields that have already misled — `align_adjustment_secs` (signed slide, not a lag), `placement_slide_frames` (unsigned, and ≠ `abs(align_adjustment) × 48000` on 82/227 gaps), `fit_path` (names the boundary grid), `pre/post_correlation` (min-reduced) — carry a contract |

## Repair R6

Post-ship gaps from [archive/repair-write-path-plan.md](docs/dev/archive/repair-write-path-plan.md).
The dual-fit oracle's pre-shoulder unpin moved to [settled.md](docs/dev/settled.md) § C.

| Item | Next action | Done when |
|------|-------------|-----------|
| **Run-mode selector** | Four modes (scan-only / `--repair-preview` / `--patch-only` / write) are selected today by three flags plus a TOML boolean, with mutual exclusions enforced pairwise in `RepairConfig::validate`. If picked up, do it as **one coherent mode selector** — not `--dry-run` / `--write` bolted on the side. Note `--patch-only` deliberately keeps `dry_run = true`, which an explicit `--write` would make incoherent | One selector, exclusions enforced centrally |
| **`--flac` lossless output** | [TEMP-flac-output-plan.md](docs/dev/TEMP-flac-output-plan.md) — draft, not started. In-process FLAC encode as a peer of `--wav`; motivated by the RIFF `u32` data-chunk limit (a long repaired timeline overruns WAV) | `--flac` writes a lossless output alongside `--wav` |
| **Expose `fill_marginal_margin` as a CLI flag** | Only `--min-fill-correlation` is settable; `fill_marginal_margin` (0.08) has no flag, so the real accept floor is `min_fill_correlation - fill_marginal_margin` (`gap_anchor_seam.rs:150`) — every calibration of the flag is off by a margin unless the reader knows to subtract it. Add `--fill-marginal-margin` as a peer of `--min-fill-correlation` (same clamp/validation shape), and correct [gap-fill-modes.md](docs/gap-fill-modes.md) line 115, which says the flag is a "floor on `min(pre, post)`" and names only the High-vs-Marginal split — it does not say accept happens one margin below | `--fill-marginal-margin` sets the value end to end (CLI → TOML → recipe), and the flag table states the `accept = min_fill_correlation - fill_marginal_margin` relationship |
| Scratch-buffer regression test | Dedicated unit test for the patch PCM path | Test exists |
| Streaming / chunked WAV encode | Large multi-gap fills without holding full PCM | Implemented |

## Residual gate

From [archive/residual-gate-findings.md](docs/dev/archive/residual-gate-findings.md) and
[archive/residual-gate-wiring-plan.md](docs/dev/archive/residual-gate-wiring-plan.md). Test inventory:
[`residual_gate_catalog/`](crates/clip-sync-repair/tests/residual_gate_catalog/). Six
deferred-on-trigger items (M3, M4, L6, FD-1, NaN headroom, `veto_rescue`) moved to
[settled.md](docs/dev/settled.md) § C, joined 2026-08-24 by the three optional tests.

| Item | Next action | Done when |
|------|-------------|-----------|
| **G1** — residual on Pearson-only skips | Veto skips are done (`ResidualHeadroomExceeded` + `GapPatchOutcome.residual`). Pearson and structure skips still lack a residual — measure on the last grid candidate when `measure_residual` | Every skip class carries a residual |

## Gap selection

From [archive/TEMP-gap-selection-sequencing-plan.md](docs/dev/archive/TEMP-gap-selection-sequencing-plan.md) §4.
Not required for `--only-gaps` / `--skip-gaps`, both shipped.

| Item | Next action | Done when |
|------|-------------|-----------|
| **`--gaps-from` manifest (v2)** | Reuse a prior gap list + embedded `ScanRecipe`; a mismatch errors on index entries. Prerequisites (ranges, recipe) shipped. Sketch: [archive/TEMP-gap-selection-deferred.md](docs/dev/archive/TEMP-gap-selection-deferred.md) §2 | Implemented |

---

The former *Defer / opportunistic* section moved wholesale to [settled.md](docs/dev/settled.md) § C
on 2026-08-24 — none of its rows was startable. Test-tier follow-ups are catalogued in
[docs/dev/test-tier-remainder.md](docs/dev/test-tier-remainder.md).
