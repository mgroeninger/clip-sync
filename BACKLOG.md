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
The 31/g7 step-boundary skip is **blocked** — [settled.md](docs/dev/settled.md) § D. Seven
diagnostic-field and conditional-ear-label leftovers moved to [settled.md](docs/dev/settled.md)
§ B / § C on 2026-08-24.

**Already categorized (no row).** 26/g6 is Hole 1 (well-posed landmark ignored; axis-2 `waveform_min`
won at −1450 ms, 2026-08-27). 17/g34 ear-pass 2026-08-27: patched, heard misaligned / possible pre
echo, **cannot tell** 26-class wrong-when (jazz self-similarity; pair used start-clip offset at a
gap that sits in the end-clip window). Hole 1 stays n=1. 31/g7 is Hole 2 (zero-length donor
bridge; the path is skip, not a better fill).

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

## Contract & tooling hygiene

Uncovered 2026-08-27 while echoing `local_pair_collar_secs` into the scan report
([TEMP-local-paired-gain-plan.md](docs/dev/TEMP-local-paired-gain-plan.md) § 4b). Pre-existing, and
none blocks the collar census. The first three are **one piece of work, in order**: the golden is
what actually guards the JSON contract, its coverage is hand-maintained and has drifted, and the
missing `Default` is why keeping it covered is expensive. Do them in sequence or not at all — row 2
exists to make row 1 cheap, and row 3 only describes what rows 1–2 make true. The binary-stamp axis
is separate and lives in [Fingerprint provenance](#fingerprint-provenance).

| Item | Next action | Done when |
|------|-------------|-----------|
| **The golden does not pin the newest contract fields** | `full_surface_repair_json_golden` is the *only* JSON-shape guard in the crate, and `collar_level` and `map_lag_ms` appear **zero** times in `tests/fixtures/full_surface_repair.json` — the fixture's gaps carry `collar_level: None` and the field is `skip_serializing_if`, so renaming `CollarLevelOffset::spread_db` fails no test today. Both are logged v1 additive fields. Populate at least one gap's `collar_level` (both `map_lag_ms` states: a trusted `Some` and a nominal `null`), then regenerate. The precedent and the reason are already written at `output.rs` `full_surface_gap_report()`: *"Populated (chunks included) so the whole object is pinned by the contract golden, not just its absence"* — it was applied to `program_levels` and never extended to the per-gap collar | Every field named in [json-output.md](docs/json-output.md)'s v1 additive log appears in the golden with a non-absent value |
| **`GapReport` has no `Default`, so covering the golden is expensive** | 43 struct literals across 12 files, nearly all fixtures that do not care about the field being added. Two costs: it biases *placement* (this is exactly why `local_pair_collar_secs` was first put on `ProgramLevels` at 7 sites instead of where it belonged), and it makes richly-populated fixtures — the row above — expensive enough to skip. Not a naive derive: `ScanRecipe` keeps its fields private so effective hold cannot drift, and a zeroed recipe reports 0 hold blocks, which is silently meaningless rather than loud. Add a test-only builder, or `Default` with a deliberately valid recipe | A fixture can pin one field with `..Default::default()` and adding an optional field to `GapReport` touches the struct, the real construction site, and nothing else |
| **Revision-procedure rule 1 does not describe what the code does** | [json-output.md](docs/json-output.md) § Revision procedure step 1 says *"never re-derive `Serialize` on domain types"*, but `domain/program_levels.rs` derives it on 5 types that `GapScanJson` embeds directly, as do `domain::gap::Gap` and `GapEquivalenceVerdict`. Mirror DTOs are the wrong fix for the record types: they have **zero methods** across 38 pub fields, so a 1:1 mirror relocates the coupling into a hand-synced `From` impl whose failure mode — add a domain field, forget the mirror, field silently vanishes from output with no compile error — is worse than the golden diff we get today. Amend the rule to the real policy: behaviourless reporting records may serialize directly and the golden is their guard; types with behaviour (`Gap` has 4 methods and decision logic) or any field whose JSON name must differ from its domain name get a DTO | A reader can tell from the rule alone whether renaming a domain field breaks the contract, without grepping for `derive(Serialize)` |
| **12 of 16 `.ps1` scripts fail to parse under Windows PowerShell 5.1** | They are BOM-less UTF-8 containing non-ASCII (`—`, `−`, `§`, `→`, `±`); 5.1 reads BOM-less files as ANSI and dies at the first one, with parse errors pointing at innocent lines far from the cause — a genuinely misleading failure. `pwsh` 7 is fine, and no doc says to use it. Add a UTF-8 BOM to the affected files, or a `#Requires -Version 7` line so the failure names itself | A script run under 5.1 either works or says why not |

---

The former *Defer / opportunistic* section moved wholesale to [settled.md](docs/dev/settled.md) § C
on 2026-08-24 — none of its rows was startable. Test-tier follow-ups are catalogued in
[docs/dev/test-tier-remainder.md](docs/dev/test-tier-remainder.md).
