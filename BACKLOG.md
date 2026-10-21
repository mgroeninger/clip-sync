# Backlog

Open follow-up work for `clip-sync`. Architecture: [PLAN.md](PLAN.md). Repair pipeline:
[docs/pipeline.md](docs/pipeline.md). Test corpus: [docs/dev/corpus-validation.md](docs/dev/corpus-validation.md).
Error handling: [docs/error-mapping.md](docs/error-mapping.md).

Last updated: 2026-10-21.

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
gap that sits in the end-clip window). Hole 1 was n=1 until 2026-09-21 (13/g16); its routing repair shipped 2026-10-09 ([archive](docs/dev/archive/TEMP-dual-fit-registration-routing-plan.md), [settled.md](docs/dev/settled.md) § C). 47/g14 ear-pass 2026-09-18: clean, and not a failure-to-move — the written fill starts
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
| **Gap start lands after A's dropout begins (soft leading edge left in the output)** | 50/g7, found 2026-10-05 ([interior-edit plan](docs/dev/archive/TEMP-dualfit-interior-edit-plan.md) § 4): a blip heard at ~2.8 s in every arm across three blind sittings sits in audio the patch never touches. Raw A's dropout begins at 2.974 s in the `--gap-listen` clip (a −20 dB fall in 2 ms on every channel, to −51…−65 dBFS) and only reaches digital silence (−104 dB) by 3.004 s. The write starts at 2.990 s (fill at 3.000 s less the 10 ms crossfade), so ~16 ms of partial dropout survives before the pre seam. That transition is the largest spectral-novelty event in the clip (100th percentile). Likely mechanism, not verified: the leading edge sits around the scan's `absolute_silence_rms` 0.001007 (−60 dBFS), so the detector calls it programme until it falls below the floor. The other suspect is the scan's boundary resolution. `gap_start_extend_on_pre_seam_fail` cannot catch it: it fires only on a pre-seam failure, and this seam passed. Whether it applies on the dual-fit path is also unchecked. The mirror case, a soft trailing edge at gap end, is unmeasured. Next: census the corpus. For every patched gap, locate A's dropout onset from the shoulder (the first ≥ 15 dB fall in 2 ms bins over the ~100 ms before the reported gap start) and from the far side at gap end. Report the onset-to-boundary distance per edge, then ear-check the largest. The `--gap-listen` clip is enough (patched == raw A before the write) | The per-edge onset-to-boundary distribution over the corpus is known. Either boundaries snap to the dropout onset (with a test on a soft-edged fixture), or the residual is shown inaudible by ear on the worst cases |
| **Default-on `--seam-extend-fill` (removes the one-crossfade advance of every default-run fill)** | **Default arm:** `apply_seam_crossfade` consumes `b_fill[..cf]` into A's pre-gap tail and starts the gap interior at `b_fill[cf]` (`domain/policies/seam_splice.rs:116`), so B content index `k` lands at A frame `gap_start + k - cf`. `registration_applied_ms` (`patch_result.rs:614`) is assembled plan-side and carries no crossfade term, so report and WAV differ by exactly `cf` — 480 frames at the 10 ms default, a fixed config value (`effective_seam_crossfade_frames` clamps only below `gap_frames/2`). Pinned by `apply_seam_crossfade_displaces_fill_content_by_the_crossfade_width`; `fill_level.rs`'s module note records the same displacement. **The fix already exists:** `--seam-extend-fill` (`apply_seam_crossfade_extended`, `seam_splice.rs:170`) hands the splice `cf` frames of real B either side of the fill, so the interior starts at `fill[0]` and a reported frame **is** the audio frame. It covers bracket (`region/execute.rs:109`) and dual-fit (`region/dual_fit.rs:632`); dual-fit is ear-confirmed (2026-09-24), bracket-path validation is open ([gap-fill-modes.md](docs/gap-fill-modes.md) § *Seam crossfade*). Do **not** build a second compensation (e.g. cutting `b_fill` `cf` earlier). **Blocker before flipping:** the splice-aware seam scorer (`seam_scoring.rs` `score_splice_pre_seam` / `score_splice_post_seam`, `single_lag_alignment` branch) models the **plain** splice geometry and has no flag-aware branch, so on the flag arm `pick_fill_length_anchor` (`gap_fill_fit.rs:1875`) and Fit reconciliation (`region/characterize.rs:1020`) score a layout the splice does not write. Dual-fit passes `single_lag_alignment: false` and is unaffected. In-process probe (2026-10-21, synthetic harmonic+noise, `cf` = 480): for a fill registered exactly, the plain-geometry splice score reads pre +0.16 / post +0.05; it peaks at 1.0 only with the fill shifted `−cf` (pre) or `+cf` (post), never both — no single registration satisfies it, because the plain post blend rewinds the fill `cf`. The extended geometry (B margin vs raw A) reads 1.0 / 1.0 at the exact registration, agreeing with the placement search's `b[start − w .. start]` seam (seam-scoring.md § 4). Reconciliation adopts the splice score only when `min(splice) ≥ min(report)`, so on the flag arm it can only upgrade, never skip; `fill_anchor_better` takes its min directly, so the trim-head/tail pick runs on the wrong geometry. Design: [TEMP-seam-extend-scorer-plan.md](docs/dev/TEMP-seam-extend-scorer-plan.md). Next: make the scorer's crossfade-window branch follow the splice actually used (ramp-fill test as for the displacement pin), then flip the default — which moves goldens and `registration_applied_ms` semantics | The scorer scores the geometry the splice writes on both arms, `seam_extend_fill` defaults on after a bracket-path ear pass, and report/WAV agree with no frame conversion |

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

## Patch outcome types

Follow-ups from carrying `dual_fit_route` on the outcome (2026-10-21): the tag join now reads placement facts
from `RegionPatchOutcome` through `GapPatchPlacement`, and `GapTagsPatchContext` holds run-level settings only.
No behaviour change is pending in any of these rows.

| Item | Next action | Done when |
|------|-------------|-----------|
| **Bundle the dual-fit placement facts into one `Option`** | `dual_fit_route`, `dual_fit_step_real` and `dual_fit_post_window_ms` are three independent fields on both `RegionPatchOutcome::Patched` (`region/types.rs`) and `GapPatchPlacement` (`domain/gap_tags.rs`), so "the other two are `Some` only when the route is" is a convention, not a type. Replace them with one `Option<DualFitPlacement { route, step_real, post_window_ms }>` on both, built once in `execute_region_spec`'s SilenceSplice arm and `outcome_from_spec`. `GapPatchStatus::Patched` keeps its flat wire fields (JSON v1 is frozen); flatten at the status join | A non-dual-fit outcome cannot carry a step or post-window value, and `full_surface_repair.json` is byte-identical |
| **`full_surface_repair` golden bypasses the real tag path** | `full_surface_patch_summary` (`infrastructure/cli/output.rs`) builds every gap with the `#[cfg(test)]` `GapPatchOutcome::with_tags_from_status`, which is lossy on `Patched`, then hand-sets `outcome.tags.dual_fit_route` (~line 1565). The golden therefore pins the JSON shape but not `derive_gap_tags_from_patch_outcome`. Build the patched gaps from a `GapPatchTierInput::Patched` + `GapPatchPlacement` through `derive_gap_tags_from_patch_outcome` instead, and drop the hand-patch | No golden gap's tags are assigned after construction, and the regenerated golden is byte-identical (or every diff is explained) |
| **`characterize_region` returns a tuple for one per-gap label** | `characterize_region` returns `(GapRepairSpec, GapTagsPatchContext)`, but the context is now request settings plus the per-gap `signature_mode_label`. Move the label onto the spec (or the region outcome), build the context once from the request at the join (`region_outcome_gap_tags`), and drop the tuple from `characterize_region`, `prepare_region_patch` and the `patch_audio/mod.rs` characterization vector | `characterize_region` returns a bare `GapRepairSpec` and `GapTagsPatchContext` is constructed once per run |

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
