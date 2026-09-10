# Backlog

Open follow-up work for `clip-sync`. Architecture: [PLAN.md](PLAN.md). Repair pipeline:
[docs/pipeline.md](docs/pipeline.md). Test corpus: [docs/dev/corpus-validation.md](docs/dev/corpus-validation.md).
Error handling: [docs/error-mapping.md](docs/error-mapping.md).

Last updated: 2026-09-10.

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

**Next:** 50/g10’s disjoint-offseam experiment is latched
([settled.md](docs/dev/settled.md) § C). Remaining post-join rows below are startable.

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
gap that sits in the end-clip window). Hole 1 stays n=1.

**Disposition tree** — every remaining row lands a category, then a successor.

1. Right when? Licensed landmark vs applied: agree / contradict / no opinion. The check is
   agreement, not magnitude ([json-output.md](docs/json-output.md) `registration_residual_ms`).
2. Contradict + both-live unique → Hole 1. Contradict + bridge ≈ 0 → Hole 2 (skip; blocked).
3. Agree + audible → join, not search.
4. We skipped → donor fail at the true when, or A-side never offered a candidate.

| Item | Next action | Done when |
|------|-------------|-----------|
| **47/g14 — failure-to-move** | Ear-label as its own question. Did a licensed landmark say move, and search stay put? | Labelled. **Yes** → its own fork (not 26's "moved to the wrong tooth"). **No / cannot tell** → drop or re-categorize |
| **31/g8 — Narrow-capture pre miss** | Capture, not the tree above. Write a TEMP. Shipped default `BorderCapture::Narrow` declines the pre at envelope +800 ms / r 0.816; Wide keeps −1146.1 / r 0.974 because the shared capture's 12% trim puts `D` at 1984 ms vs Narrow's 13 ms. Numbers: [archive/TEMP-envelope-adaptive-widen-plan.md](docs/dev/archive/TEMP-envelope-adaptive-widen-plan.md) deciding measurement 2 | A decision exists that is not "flip the default" ([settled.md](docs/dev/settled.md) § B) |
| **α / `border_standoff_secs`** | Not Hole 1's 1.45 s. Fill sits ~25 ms late vs both shoulders; hypothesis is production's 0.35 s standoff. Run existing arms through `scripts/measure/diff-patch-adjustments.ps1`, or drop the item | Confirmed, or dropped |

## Post-join defects (pairs 31 and 50)

Right-when defects at the **join** of technically-successful patches, plus the skip cluster beside
them. Promoted 2026-08-24 from the census and pair-50 notes, both now archived:
[archive/TEMP-collapsed-bridge-census-2026-08-20.md](docs/dev/archive/TEMP-collapsed-bridge-census-2026-08-20.md)
Q7 / Q11 and
[archive/TEMP-pair50-anchor-seam-outcomes-2026-08-21.md](docs/dev/archive/TEMP-pair50-anchor-seam-outcomes-2026-08-21.md)
Q1 (mechanism explained 2026-09-03; disjoint-offseam probe latched — [settled.md](docs/dev/settled.md) § C;
ruled-out approaches in § A) / Q2. Both pairs' steps have opposite signs
(−1.15 s vs +2.789 s) and share nothing else — same tree step 3/4 as above, not a shared class until
a remnant comparison says so.

| Item | Next action | Done when |
|------|-------------|-----------|
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
| **`--replay` re-registers an already-registered window** | Found 2026-09-10 by replaying 51 scan reports. Under `Apply` (default) `donor_span_secs` is the **registered** window; `replay_gap` passes it to `register_donor_window` as the nominal map, so the completeness gate reads `lag L→0` on 624 of 968 gaps (and 8 of 13 on a dump set — not a scan-reader artifact) and exits 1 on arithmetic. Same shift makes `donor@ℓ` a second displacement, so the `apply` FLIP/abstain tallies are read at a window production never used. Recover the nominal span from `envelopes.b_nominal_bin` (recorded for exactly this) rather than from the verdict's span | The gate's mismatch count means "did not reproduce", and the flip count is quotable |
| **33/17 placement-path investigation** | Which path placed 33/17's fill is **unrecorded** — the dump's `brackets` array is the oracle enumeration, not the candidate production selected. Instrument the selected candidate / fit path | The path is recorded. If anything is then tuned, the likely site is the **acceptance floor**, not the comparator |
| **33/17 is a real-but-inaudible dropout whose patch is a defect** | A hits digital zero, B keeps a −68 dB click bed; the patch lands 8 dB above its own surroundings, 36 ms misplaced, writing 380 ms past the gap end. Seam/splice path, independent of the equivalence band | Characterized |

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
