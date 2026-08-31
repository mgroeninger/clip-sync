# Backlog

Open follow-up work for `clip-sync`. Architecture: [PLAN.md](PLAN.md). Repair pipeline:
[docs/pipeline.md](docs/pipeline.md). Test corpus: [docs/dev/corpus-validation.md](docs/dev/corpus-validation.md).
Error handling: [docs/error-mapping.md](docs/error-mapping.md).

Last updated: 2026-08-27.

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

**Next:** [Reconcile the donor-window numbers](#donor-registration). The highest-priority *defect*
on the board is [50/g10's audible post join](#post-join-defects-pairs-31-and-50) — confirmed by
ear, no causal model.

---

## Fill placement

Two ear-confirmed wrong patches, two distinct defects. Findings:
[gap-vocabulary.md](docs/dev/gap-vocabulary.md) § *Known holes*,
[archive/TEMP-31g7-model-vs-findings.md](docs/dev/archive/TEMP-31g7-model-vs-findings.md) §8.
Durable `lag_decision` behaviour: [gap-fingerprint.md](docs/dev/gap-fingerprint.md) § *`lag_decision`*.
The 31/g7 **pair-sequence** skip classifier is **blocked** — [settled.md](docs/dev/settled.md) § D.
The **per-gap** keep-path refuse (early locks, abut → skip) is the row below. Seven
diagnostic-field and conditional-ear-label leftovers moved to [settled.md](docs/dev/settled.md)
§ B / § C on 2026-08-24.

**Already categorized (no row).** 26/g6 is Hole 1 (well-posed landmark ignored; axis-2 `waveform_min`
won at −1450 ms, 2026-08-27). 17/g34 ear-pass 2026-08-27: patched, heard misaligned / possible pre
echo, **cannot tell** 26-class wrong-when (jazz self-similarity; pair used start-clip offset at a
gap that sits in the end-clip window). Hole 1 stays n=1.

**Disposition tree** — every remaining row lands a category, then a successor.

1. Right when? Licensed landmark vs applied: agree / contradict / no opinion. The check is
   agreement, not magnitude ([json-output.md](docs/json-output.md) `registration_residual_ms`).
2. Contradict + both-live unique → Hole 1. Contradict + bridge ≈ 0 → Hole 2 (skip; blocked).
3. Agree + audible → join, not search.
4. We skipped → donor fail at the true when, or A-side never offered a candidate.

| Item | Next action | Done when |
|------|-------------|-----------|
| **Early shoulder locks / collapse skip (31/g7)** | [TEMP-early-shoulder-locks-collapse-skip-plan.md](docs/dev/TEMP-early-shoulder-locks-collapse-skip-plan.md) — seam-local nominal-first once before keep; abut → `shoulders_abut`; capture and visit after splice; `try_dual_fit` / `splice_dualfit` take locks | 31/g7 is `shoulders_abut` before unified-fit; post-splice visit runs, A length unchanged; in-reach dual-fit fill bytes unchanged; `--no-dual-fit` still collapse-skips |
| **Post-splice introduced-silence collapse** | [TEMP-post-splice-introduced-silence-collapse-plan.md](docs/dev/TEMP-post-splice-introduced-silence-collapse-plan.md) — fill `visit_shoulders_abut`: concat-confirm, **warn by default**, drain only under `--collapse-introduced-silence`; current mux. Starts the day the skip-gate row ships. **Carries a drain hazard (2026-08-30)**: the captured list is 2 gaps and **11/g9 is a false positive** — no downstream trail, 1 s probe locks both shoulders at nominal — so draining it would delete 599 ms of A that was never introduced silence. It wears no lookalike tag (`clamped` / `edge_pinned` / `program_quiet` are zero corpus-wide), so it arrives looking like 31/g7 | Default 31/g7 is `shoulders_abut` + warning, A length unchanged; flag on is `collapsed` (hole gone); fills never reindexed; **and 11/g9 does not drain**, refused attributably — either `join_dead` from the concat-confirm on an 11/g9-shaped fixture, or an explicit step-agreement admission filter (§1 of the plan has both, with numbers) |
| **47/g14 — failure-to-move** | Ear-label as its own question. Did a licensed landmark say move, and search stay put? | Labelled. **Yes** → its own fork (not 26's "moved to the wrong tooth"). **No / cannot tell** → drop or re-categorize |
| **31/g8 — Narrow-capture pre miss** | Capture, not the tree above. Write a TEMP. Shipped default `BorderCapture::Narrow` declines the pre at envelope +800 ms / r 0.816; Wide keeps −1146.1 / r 0.974 because the shared capture's 12% trim puts `D` at 1984 ms vs Narrow's 13 ms. Numbers: [archive/TEMP-envelope-adaptive-widen-plan.md](docs/dev/archive/TEMP-envelope-adaptive-widen-plan.md) deciding measurement 2 | A decision exists that is not "flip the default" ([settled.md](docs/dev/settled.md) § B) |
| **95 skips at a rigid shoulder offset the bracket sweep never reaches** | New 2026-08-30 from the 51-pair corpus, found via 11/g9. Of **644** skipped gaps, **95** have both 1 s `lag_decision` shoulders at r ≥ 0.94 with `|step| ≤ 25 ms` (rigid, no map step) **and** `donor_interior_nominal.continuous = true, silence_fraction = 0.0` — donor present, both shoulders agreeing on one common offset from −1143 ms to ~0. They skip `waveform_floor`. On 11/g9 all 23 brackets score `seam_pre`/`seam_post` 0.05–0.27 against `lag0_r` 0.046 / −0.018: the sweep varies span and `move_frames` but **never the donor map lag**, so every bracket scores a donor 72 ms misregistered. Candidate fix is register-at-the-shoulder-agreed-lag before the sweep (a plain single-lag fill — **not** dual-fit, there is no step). Remaining 644: 436 donor genuinely silent/discontinuous at nominal (correctly refused), 78 no `lag_decision`, 35 weak or stepped | Sufficiency settled. Necessary is shown; **not** shown sufficient — 38/1 is the counterexample (offset ≈ 0, `lag0_r` pre 0.985, brackets still cap at 0.130), so bracket seam scoring diverges from shoulder correlation for at least one reason beyond placement. **Name that reason**, then either a probe run converts a stated fraction of the 95, or the class is refuted and filed |
| **α / `border_standoff_secs`** | Not Hole 1's 1.45 s. Fill sits ~25 ms late vs both shoulders; hypothesis is production's 0.35 s standoff. Run existing arms through `scripts/measure/diff-patch-adjustments.ps1`, or drop the item | Confirmed, or dropped |

## Post-join defects (pairs 31 and 50)

Right-when defects at the **join** of technically-successful patches, plus the skip cluster beside
them. Promoted 2026-08-24 from the census and pair-50 notes, both now archived:
[archive/TEMP-collapsed-bridge-census-2026-08-20.md](docs/dev/archive/TEMP-collapsed-bridge-census-2026-08-20.md)
Q7 / Q11 and
[archive/TEMP-pair50-anchor-seam-outcomes-2026-08-21.md](docs/dev/archive/TEMP-pair50-anchor-seam-outcomes-2026-08-21.md)
Q1 / Q2. Both pairs' steps have opposite signs (−1.15 s vs +2.789 s) and share nothing else — same
tree step 3/4 as above, not a shared class until a remnant comparison says so.

| Item | Next action | Done when |
|------|-------------|-----------|
| **50/g10 — audible post join at the correct offset** (High) | Category: right when, bad join (`anchor_seam_used`, ~2.8 s). Live lead: overlap-only remnant locks at **13.2 ms**, not 0 — same size as g10's plateau split (2.805 vs 2.831 s; 50/g8 splits the same way, 2.767 vs 2.809). Test that residual as the echo. Silent-onset and unreached-region are dead ([archive/TEMP-pair50-anchor-seam-outcomes-2026-08-21.md](docs/dev/archive/TEMP-pair50-anchor-seam-outcomes-2026-08-21.md)) | **Confirmed** → splice-residual Change in scope. **Excluded** → this lead is dead; name the next mechanism or drop the item |
| **31/g10 — audible blip where g8 and g11 are clean** (Medium) | First: re-listen and pin locus — **post join** vs the fill's own end boundary. Then: overlap-only post remnant for g10 **and** g11 (pair-50 technique), compare residual lags. 31/g8's Narrow miss is the capture row above, not this | Locus pinned. **Same remnant as 50/g10** → one join class. **Different** → keep separate. **Cannot tell** → stop (n=1 vs two clean twins) |
| **50/g11–g13 — skips between two correctly-resolved flanks** | First question only: at the true +2.8 s, `donor_interior_aligned.continuous` is `false` on g11/g12 (strict count two; g13/g17 gain an aligned donor that also reads discontinuous) while flanks g10/g14 stay continuous and `nominal` stays `true`. If that donor reading explains the skip, stop. Only if the donor **does** bridge: g11/g12 have no `best_attempt` — "no A-side candidate" vs "none beat baseline". Gate numbers if needed: g11 −0.018/0.393, g12 −0.072/0.567, g13 anchor attempt 0.188/0.226 | **Donor fails to bridge** → honest skip, close. **Donor bridges** → missing `best_attempt` attributed |

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

**Shipped 2026-08-27** — the whole-program half of the fix is done and accepted:
[archive/TEMP-program-gain-plan.md](docs/dev/archive/TEMP-program-gain-plan.md) applies
`g_program = 10^(-offset_db/20)` to every B-derived fill and defaults `normalize_fill` **off**, which
removed the n = 6 over-boost outright (only `g_program` remains, under 0.3 dB on every corpus pair)
without crushing 24/g46. What is left below is the **local** half: the hole-local gain is still
`compute_fill_gain`'s wide-A-border RMS ratio whenever `--normalize` is passed, and under that flag
it is now a **boost-only** regime — 111 of 308 gaps changed, zero cuts, seven still pinned at the
12 dB clamp. Do not re-litigate the default; the rows below are about making the local gain correct
enough to enable.

| Item | Next action | Done when |
|------|-------------|-----------|
| **Local paired-shoulder gain** | [TEMP-local-paired-gain-plan.md](docs/dev/TEMP-local-paired-gain-plan.md) — replace `compute_fill_gain` (5 s + 5 s A-border RMS vs whole-fill RMS) with the same paired B−A difference as the program scalar, restricted to collar blocks around the hole, applied as a residual against the applied program offset. **Separate Change** from program-gain; do not bundle. Does not flip the `normalize_fill` default. The RMS-window options (narrow / quieter-of / cap) and the findings-note Q4 “keep 15/g6 and 2/g6 boosts” test are withdrawn — those boosts were fill RMS vs a wide A window. **Two halves, measurement first** (plan § 4a): the collar stamps unconditionally at scan and `normalize_fill` stays off, so the corpus census that sets `MIN_LOCAL_PAIRS`, `local_pair_collar_secs` and `MAX_LOCAL_RESIDUAL_DB` costs one ordinary scan and lands before anything applies a local gain. The residual gets its **own** bound — `max_fill_gain_db` (12.0, uncalibrated since 2026-06-08) is not reused | § 4a's census exists and the three constants are stated against it, including the corpus abstention rate; scan JSON carries `collar_level` with per-side medians; `--normalize` does not revive the n=5 over-boost; `g_program * g_local = 10^(-d_local/20)` when both apply |
| **Ear-label the seven clamp-pinned gaps** | The 2026-08-25 census found seven patched gaps pinned at the 12.000 dB `max_fill_gain_db` ceiling — 5@4199.1 s, 10@6479.8 s, 10@6501.0 s, 19@95.7 s, 43@6144.7 s, 44@1714.1 s, 45@1142.2 s. With corpus offsets inside 0.26 dB a 12 dB demand **cannot** be master mismatch, so each is a local-reference failure by construction — the largest available evidence for the default-off row above. `--gap-listen` each and label. Note the clamp censors: these mean *at least* 12 dB ([settled.md](docs/dev/settled.md) § E). 5@4199.1 is 5/g6, labelled audible 2026-08-27 | All seven are labelled and the audible fraction is known |
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
