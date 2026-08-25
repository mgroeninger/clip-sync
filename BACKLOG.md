# Backlog

Open follow-up work for `clip-sync`. Architecture: [PLAN.md](PLAN.md). Repair pipeline:
[docs/pipeline.md](docs/pipeline.md). Test corpus: [docs/dev/corpus-validation.md](docs/dev/corpus-validation.md).
Error handling: [docs/error-mapping.md](docs/error-mapping.md).

Last updated: 2026-08-25.

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

**Next:** [Ear-label 5/g6 and 15/g3](#fill-normalize-boosts-against-a-non-local-reference) — two
`--gap-listen` dumps that pick which normalize fix to build; the exposure behind them is sized and
the defect is ear-confirmed on three gaps. Then
[26/g6 start-sweep oracle](#fill-placement) and
[reconcile the donor-window numbers](#donor-registration). The highest-priority *defect* on the
board is [50/g10's audible post join](#post-join-defects-pairs-31-and-50) — confirmed by ear, no
causal model.

---

## Fill placement

Two ear-confirmed wrong patches, two distinct defects. Findings:
[gap-vocabulary.md](docs/dev/gap-vocabulary.md) § *Known holes*,
[archive/TEMP-31g7-model-vs-findings.md](docs/dev/archive/TEMP-31g7-model-vs-findings.md) §8.
Durable `lag_decision` behaviour: [gap-fingerprint.md](docs/dev/gap-fingerprint.md) § *`lag_decision`*.
The 31/g7 step-boundary verdict is **blocked** — [settled.md](docs/dev/settled.md) § D. Seven
diagnostic-field and conditional-ear-label leftovers moved to [settled.md](docs/dev/settled.md)
§ B / § C on 2026-08-24.

| Item | Next action | Done when |
|------|-------------|-----------|
| **26/g6 — registration ignored** | Oracle the axis-2 start sweep: what did the objective score at ≈ −50 ms vs −1450 ms (findings §6.8)? Do **not** retune `fill_fit_early_start_penalty_scale` | The score curve over that axis is recorded and the −1450 ms win is either explained or shown to be a defect |
| **31/g8 — Narrow-capture pre miss** | Write a TEMP. Shipped default `BorderCapture::Narrow` declines the pre at envelope +800 ms / r 0.816; Wide keeps −1146.1 / r 0.974 because the shared capture's 12% trim puts `D` at 1984 ms vs Narrow's 13 ms. Numbers: [archive/TEMP-envelope-adaptive-widen-plan.md](docs/dev/archive/TEMP-envelope-adaptive-widen-plan.md) deciding measurement 2 | A decision exists that is not "flip the default" ([settled.md](docs/dev/settled.md) § B) |
| **Ear-pass 17/g34 and 47/g14** | Cheapest n > 2. 17/g34 is the same well-posed class as 26/g6; 47/g14 is the first failure-to-move. Labels, not a TEMP. Residual grouping 17/g34 with 31/g7 is **not** a shared fingerprint | Both are labelled |
| **α / `border_standoff_secs`** | Fill sits ~25 ms late vs both shoulders; hypothesis is production's 0.35 s standoff. Run existing arms through `scripts/measure/diff-patch-adjustments.ps1`, or drop the item | Confirmed, or dropped |

## Post-join defects (pairs 31 and 50)

Two ear-confirmed defects at the **post join** of technically-successful patches, plus the skip
cluster beside them. Promoted 2026-08-24 from the census and pair-50 notes, both now archived:
[archive/TEMP-collapsed-bridge-census-2026-08-20.md](docs/dev/archive/TEMP-collapsed-bridge-census-2026-08-20.md)
Q7 / Q11 and
[archive/TEMP-pair50-anchor-seam-outcomes-2026-08-21.md](docs/dev/archive/TEMP-pair50-anchor-seam-outcomes-2026-08-21.md)
Q1 / Q2. Both pairs' steps have opposite signs (-1.15 s vs +2.789 s) and the pairs share nothing else,
so treat "both defects are at the post join" as one axis to **test**, not as an established link.

| Item | Next action | Done when |
|------|-------------|-----------|
| **50/g10 — audible post join at the correct offset** (High) | The patch is correct (`anchor_seam_used`, ~2.8 s offset) and the post-onset remnant matches at r = 1.000, yet the join is audible. Only live lead: the overlap-only remnant locks at **13.2 ms**, not 0 — the same size as g10's own plateau split (2.805 vs 2.831 s; 50/g8 splits the same way, 2.767 vs 2.809). Test that seam residual as the echo mechanism. Silent-onset and unreached-region models are both **dead** | The 13.2 ms residual is confirmed or excluded as the mechanism |
| **31/g10 — audible blip where g8 and g11 are clean** (Medium) | Same pair, same map step, same `shared_far_offset` shape, so that shape does not predict the defect. Cheapest first, no media run: (a) re-listen to `…_g010_full_a_patched.wav` and pin whether the blip is at the **post join** or at the fill's own end boundary — "end of the patch" does not distinguish them; (b) correlate the overlap-only post remnant for g10 **and** g11 with the pair-50 technique, compare residual lags; (c) a residual on g10 only makes it the same candidate as 50/g10. Do **not** fold 31/g8's Narrow `lag_decision` miss in — that is the capture item above. n=1 against n=2 clean twins: do not build a detector from it | The blip's locus is pinned and the g10-vs-g11 residual comparison exists |
| **50/g11–g13 — skips between two correctly-resolved flanks** | Live lead from the flat-3000 dump: at the true +2.8 s registration `donor_interior_aligned.continuous` flips `true → false` on exactly g11/g12 (strict count two; g13/g17 gain an aligned donor that also reads discontinuous), while the patching flanks g10/g14 stay continuous and `nominal` stays `true` throughout. Read directly it says **B does not bridge the hole at the correct registration** — which would answer this without any A-side bracket story. Check whether that donor silence and the sub-threshold gate correlations (g11 −0.018/0.393, g12 −0.072/0.567, g13 anchor attempt 0.188/0.226) are the same fact seen twice. Second step if not: g11/g12 record no `best_attempt` at all — distinguish "no A-side anchor candidate found" from "none beat baseline" | Either the donor reading explains the skips, or the missing `best_attempt` is attributed |

## Fill normalize boosts against a non-local reference

Ear-confirmed defect, **n = 3** (45/g4, 45/g5, 19/g13; 31/g7 audible but confounded by its map-step
straddle). Sizing, attribution and ear labels:
[TEMP-fill-normalize-local-reference-2026-08-25.md](docs/dev/TEMP-fill-normalize-local-reference-2026-08-25.md).
Durable reading: [json-output.md](docs/json-output.md) § FillLevelCheck and
[pipeline.md](docs/pipeline.md) § Fill normalization (pair 19/g013, `normalize_gain = 3.88`,
energy-pooled 5 s + 5 s vs −52 dB local shoulders). Boost-only PR archived:
[archive/TEMP-fill-normalize-boost-only-plan.md](docs/dev/archive/TEMP-fill-normalize-boost-only-plan.md).
B-relative gain is **refuted** ([settled.md](docs/dev/settled.md) § A); the unvalidated attenuation
half is latched (§ C).

| Item | Next action | Done when |
|------|-------------|-----------|
| **Validate the A/B program level offset on real media** | The measurement **shipped** capture-only ([TEMP-ab-program-levels-plan.md](docs/dev/TEMP-ab-program-levels-plan.md)): scan JSON now carries `program_levels` (`a` / `b` context + paired `offset` = median `offset_db`, MAD `spread_db`, deciles, per-600 s chunks), unit-tested but never run against the corpus. Remaining work is a media read: scan a handful of pairs, check `spread_db` is small enough for one scalar to exist at all, and see whether `offset_db` correlates with the audible over-boost cohort. Still report-only — no gain derives from it (§ 8; B-relative gain is refuted, [settled.md](docs/dev/settled.md) § A) | A corpus read reports, per pair, `offset_db` / `spread_db` / chunk drift — and says whether a single scalar exists |
| **Make the boost reference local** (do this first for the hole-local defect) | Pick one: narrow the border window; take the quieter of wide-vs-local; or cap the gain by the local shoulder ratio (`normalize_window_secs`, default 5.0, is already a knob). The sweep constrains it: the cap must apply to **every** boost — 45/g5 is audible at +5.61 dB, so scoping to the 12 dB ceiling misses half the cohort — and must separate "+1.14 dB legitimate" from "+5.61 dB audible". Acceptance test is fixed in the note § Q4: the six back out to ≈ unity, **15/g6 and 2/g6 keep their boosts**, 24/g46 stays at 0.00 dB | A gain change ships and passes that test |
| **Consider defaulting `normalize_fill` to false** | Designed, not startable: [TEMP-program-gain-plan.md](docs/dev/TEMP-program-gain-plan.md) pairs default-off local with a whole-program `offset_db` scalar (additive residual if re-enabled). Latched until the validate row's Done-when lands ([settled.md](docs/dev/settled.md) § C). Do not decide Q3 in isolation | The validate row has answered "a scalar exists", then that TEMP ships |
| **Ear-label 5/g6 and 15/g3** | The two unlabelled cohort members; neither pair has ever had a listen directory, so both need a `--gap-listen` dump. **15/g3 is the discriminating one** — +10.74 dB of gain but only +4.05 dB of measured edge, so it probes whether the audible axis is the gain or the resulting edge delta. That answer picks the fix above | Both are labelled |
| **Re-label `2026-08-08-edge-listen`** | Pairs 7/8/12 are swapped. Required before quoting "lowest labelled clean" as a bound | Relabelled |
| Residual seam `g` (`a ≈ g·b`) as the production gain | Candidate replacement — estimates level from the **seam** rather than whole-fill RMS. Needs verdict plumbing plus corpus rules for when the estimate is trustworthy | Evaluated against the sweep |

## Repeat-at-seam prior

Durable reading: [gap-fill-modes.md](docs/gap-fill-modes.md) § Fit unified search and
[json-output.md](docs/json-output.md) § `fit_terms`. `anchor_penalty` is a configuration zero and
needs no work. Do **not** lower `REPEAT_CORR_THRESHOLD` — [settled.md](docs/dev/settled.md) § A;
the constant split that would precondition any retune is recorded in that row's *Reopen if* cell, and
is not work until something asks to raise the threshold.

| Item | Next action | Done when |
|------|-------------|-----------|
| **Delete arm 3** | Unreachable by construction (`REPEAT_SUM_CEILING = 2 × REPEAT_CORR_THRESHOLD`). Delete it; do **not** repair the ceiling | Gone |

## Donor registration

From the 2026-08 equivalence-band / donor-registration review. Donor Apply and the fill-level check
shipped. The conditional donor test is **blocked on a count** — [settled.md](docs/dev/settled.md) § D.

| Item | Next action | Done when |
|------|-------------|-----------|
| **Reconcile the donor-window numbers in `gap-fingerprint.md`** | Its open-differences table still reads "open, small — median Δ 0.008, max 0.067, every gap within ±1 block" from **one pair / ten gaps**. The 39-pair corpus contradicts all three: 100% differ, 42% by more than one block, max **0.333**, **10 / 802 dangerous**. Root cause traced: scan windows on the silent **core** (`scan_gaps.rs:278`), the diagnostic path on the **raw** hold-bridged run (`measure.rs`) | The table states the corpus numbers and names the core-vs-raw cause |
| **`equivalence-calibration --replay` reads `GapScanJson`** | Today `--replay` loads only fingerprint `corpus.json` / `GapCorpus`, but plain scan JSON already carries the same registration + envelope fields on every gap (`scripts/measure/scan-registration.ps1`). Teach the reader the scan shape. Small reader change, no new measurement | Apply flip/abstain counts come from the production classifier, not a hand reconstruction |
| **33/17 placement-path investigation** | Which path placed 33/17's fill is **unrecorded** — the dump's `brackets` array is the oracle enumeration, not the candidate production selected. Instrument the selected candidate / fit path | The path is recorded. If anything is then tuned, the likely site is the **acceptance floor**, not the comparator |
| **33/17 is a real-but-inaudible dropout whose patch is a defect** | A hits digital zero, B keeps a −68 dB click bed; the patch lands 8 dB above its own surroundings, 36 ms misplaced, writing 380 ms past the gap end. Seam/splice path, independent of the equivalence band | Characterized |

## Fingerprint provenance

Tracks A + B shipped 2026-07-31 —
[archive/TEMP-fingerprint-provenance-plan.md](docs/dev/archive/TEMP-fingerprint-provenance-plan.md).
Shipped behaviour: [gap-fingerprint.md](docs/dev/gap-fingerprint.md) § *Source identity & the corpus
library* and § *`measurement`*. Three deferred-on-trigger leftovers moved to
[settled.md](docs/dev/settled.md) § C.

| Item | Next action | Done when |
|------|-------------|-----------|
| **`corpus.json` carries no build stamp** | ~10 lines. `source` identifies the *media*; nothing identifies the *binary*. `source.probe_recipe.border_capture` (shipped 2026-08-22) closes one axis; every other build-time constant is still invisible. **Trigger is met** — this cost real time on 31/g8 | Two arms from different builds can be told apart without inference |

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
| **`region.rs` folder split** | [TEMP-patch-audio-region-module-split-plan.md](docs/dev/TEMP-patch-audio-region-module-split-plan.md) — draft, not started | Split lands |
| **`--flac` lossless output** | [TEMP-flac-output-plan.md](docs/dev/TEMP-flac-output-plan.md) — draft, not started. In-process FLAC encode as a peer of `--wav`; motivated by the RIFF `u32` data-chunk limit (a long repaired timeline overruns WAV) | `--flac` writes a lossless output alongside `--wav` |
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
