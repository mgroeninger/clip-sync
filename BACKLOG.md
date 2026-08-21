# Backlog

Open follow-up work for `clip-sync`. See [PLAN.md](PLAN.md) for architecture, [docs/pipeline.md](docs/pipeline.md) for the repair pipeline (phase by phase), [docs/dev/corpus-validation.md](docs/dev/corpus-validation.md) for the test corpus, and [docs/error-mapping.md](docs/error-mapping.md) for error handling. Shipped work is recorded in `docs/dev/archive/*` and git history. Out of scope: [PLAN.md](PLAN.md) § Out of scope.

Last updated: 2026-08-20.

**How this doc works**

- **Open** — actionable items below (problem / direction kept for open work only).
- **Plans** — active drafts under `docs/dev/TEMP-*.md`; catalog: [docs/dev/README.md](docs/dev/README.md) § Plans. Archive when shipped.

**Next:** [Fill placement](#fill-placement-two-defects--registration-ignored-26g6-class-and-step-gap-with-empty-donor-bridge-31g7) — **Collapsed-bridge skip verdict** (S1 shipped 2026-08-20). 26/g6 axis-2 start-sweep oracle (parallel, not this chain). [Donor registration leftovers](#donor-registration-leftovers); [Repair R6](#repair-r6-follow-ups); [Residual gate](#residual-gate-follow-ups).

---

## Open work

### Fill placement: two defects — registration ignored (26/g6-class) and step gap with empty donor bridge (31/g7)

Findings and closed sensor work: [TEMP-31g7-model-vs-findings.md](docs/dev/archive/TEMP-31g7-model-vs-findings.md) §8, [gap-vocabulary.md](docs/dev/gap-vocabulary.md) § *Known holes*, [archive/TEMP-fill-placement-registration-plan.md](docs/dev/archive/TEMP-fill-placement-registration-plan.md), map-coordinates and nominal-first (both archived). Recenter leftovers **closed 2026-08-20** as a no-op: 31/g12 post is a tail (`a_after = 0`); the 12-pair dump's empty shoulders are all start/tail geometry. Saturated-envelope unit test shipped. **S1 shipped 2026-08-20** — [archive/TEMP-registration-residual-plan.md](docs/dev/archive/TEMP-registration-residual-plan.md); real-media reproduction of 31/g7 / 26/g6's findings-§6.2 numbers via `registration_residual_ms` is still open, not blocking. Do not re-open S2b, the source/gate fork, or envelope-first ordering. Durable `lag_decision` behaviour: [gap-fingerprint.md](docs/dev/gap-fingerprint.md) § *`lag_decision`*.

| Gap | Open |
|-----|------|
| 31/g7 | Collapsed-bridge skip verdict. Spec `\|gap + post_lag − pre_lag\| ≈ 0`; do not treat as one-seam fill. Post at −1160 is in `lag_decision`. n=1 census extended to the full corpus, findings + 7 open follow-ups: [TEMP-collapsed-bridge-census-2026-08-20.md](docs/dev/TEMP-collapsed-bridge-census-2026-08-20.md). |
| 26/g6 | Oracle the axis-2 start sweep — what did the objective score at ≈−50 ms vs −1450 ms (findings §6.8). Do not retune `fill_fit_early_start_penalty_scale`. |

**Order of work** (do not raise the 0.70 envelope guard; do not raise the 2000 ms default until the adaptive-widen ladder dump says to):

1. **Collapsed-bridge skip verdict** — next Change.
2. **Adaptive widen + rescue-branch guard** — [TEMP-envelope-adaptive-widen-plan.md](docs/dev/TEMP-envelope-adaptive-widen-plan.md).
3. **`lag_editorial` confirm** — piggyback on a diagnostics dump.

| Item | Direction |
|------|-----------|
| **Collapsed-bridge skip verdict** | 31/g7 honest action. Spec `\|gap + post_lag − pre_lag\| ≈ 0` |
| **Adaptive widen** | [TEMP-envelope-adaptive-widen-plan.md](docs/dev/TEMP-envelope-adaptive-widen-plan.md) (ladder 2000→3000→4000 in the rescue branch). Keep default 2000 until its ladder dump locks +2.79 s interior |
| **`lag_editorial` shares `lag_at_placement`** | Piggyback on a `--fingerprint-diagnostics` dump. Confirm the diagnostic placement follows the same rescue ordering; do not treat disagreement with `lag_decision` as a gate signal |
| **S4 / `applied = 0` cohort** | Conditional ear labels for whether a residual guard's domain includes gaps that never slid. Do not run until it would change a decision. The archived §5 render list is mixed-orientation and stale |
| **Ear-pass 17/g34 and 47/g14** | Cheapest n>2. 17/g34 is the same well-posed class as 26/g6. Residual also groups it with 31/g7 — do not read that as a shared fingerprint. 47/g14 is the first failure-to-move. Labels, not a TEMP |
| **α / `border_standoff_secs`** | Fill sits ~25 ms late vs both shoulders. Hypothesis: production's 0.35 s standoff. Run existing arms through `scripts/measure/diff-patch-adjustments.ps1` or drop. Not a TEMP |
| **+39 ms fill-start detection bias** | Archived registration plan §2.1. Cosmetic for a discriminator; required before any absolute claim about delivered position |
| **Two gaps break the frame-672000 identity** | Archived registration plan §3.1, 589 and 4245 frames. Dump-only; needed before that join is used unconditionally |

### Repeat-at-seam prior is inert on every winner; one branch is unreachable

Durable reading: [gap-fill-modes.md](docs/gap-fill-modes.md) § Fit unified search and [json-output.md](docs/json-output.md) § `fit_terms` (oriented 2026-08-13 run; arm 3 unreachable; do not lower 0.55 — the live arm's conditions are anti-correlated). `anchor_penalty` is a configuration zero and needs no work.

| Item | Direction |
|------|-----------|
| **Delete arm 3** | Unreachable by construction (`REPEAT_SUM_CEILING = 2 × REPEAT_CORR_THRESHOLD`). Do not repair the ceiling |
| **Leave `REPEAT_CORR_THRESHOLD` (0.55)** | Lowering it would not have fired arm 1 on the 9 high-repeat winners. Split the constant before any retune — the same 0.55 is the short-bracket extension stop, a veto `fill_repeat_penalty_weight` does not disable |
| **Extension-loop instrumentation** | Optional. Record `prev_repeat` / `next_repeat` and stop reason (`score_ok` vs `repeat_ok` vs exhaustion). If the monotone clause always binds first, the threshold is free to move on the penalty side after the split |
| **Winner-only blindness** | `fit_terms` reports the winner, so a working penalty that demoted rivals is invisible. Needs per-candidate capture; **not proposed** |

### Fill normalize boosts against a non-local reference (ear-confirmed defect)

Durable reading: [json-output.md](docs/json-output.md) § FillLevelCheck and [pipeline.md](docs/pipeline.md) § Fill normalization (pair 19/g013, `normalize_gain = 3.88`, energy-pooled 5 s+5 s vs −52 dB local shoulders). Boost-only PR archived: [TEMP-fill-normalize-boost-only-plan.md](docs/dev/archive/TEMP-fill-normalize-boost-only-plan.md). Re-label `2026-08-08-edge-listen` (pairs 7/8/12 swapped) before quoting “lowest labelled clean” as a bound. Oriented roll-up: `gap-files/2026-08-13-repeat-terms/fill-level-rollup.csv`.

| Item | Direction |
|------|-----------|
| **Size the exposure before designing a fix** (do this first) | Do **not** design from n=1. `scripts/measure/measure-patch-outcomes.ps1` is the run (`--patch-only` → `fill-level-rollup.csv`) and needs **one column added** — it emits `PeakDeltaDb`/`EdgeDeltaDb` (`:170`, `:183`) but not `normalize_gain`. With that column: large gain **and** quiet immediate shoulders = the exposed set |
| Make the boost reference local | Candidate directions once the sweep lands: narrow the border window; take the quieter of wide-vs-local; or cap the gain by the local shoulder ratio. `normalize_window_secs` (default 5.0) is already a knob |
| Consider defaulting `normalize_fill` to false | Named because the evidence now points at it: **one** ear case where the boost causes an audible defect, **zero** ear cases where it demonstrably helps. Honest short-term answer if the sweep shows large boosts are common |
| Re-run the fill-level roll-up after any gain change | Any change here moves `peak_delta_db` / `edge_delta_db`. **Already owed once:** the boost-only PR changed the gain on every previously-attenuated gap |
| Residual seam `g` (`a ≈ g·b`) as the production gain | Estimates level from the **seam** rather than whole-fill RMS. Needs verdict plumbing plus corpus rules for when the estimate is trustworthy |
| B-relative / donor-shoulder matching as a full replacement for A-border RMS | Sidesteps the A-reference question; corrects nothing when the two masters genuinely differ in level, which is the only case normalize exists for. Evaluate against the sweep |
| The attenuation half was never ear-validated | The two affected gaps sound clean *now*; nobody has heard them sound bad *before*. Cheap to close if it ever matters: restore the symmetric clamp, drop the `region.rs` assert, render to a `-before` dir, revert |

### Gap-selection parked debt (do not fold into thin v1)

From [archive/TEMP-gap-selection-sequencing-plan.md](docs/dev/archive/TEMP-gap-selection-sequencing-plan.md) §4.
Survive archival of that meta doc. Not required for `--only-gaps` / `--skip-gaps`.

| Item | Direction |
|------|-----------|
| `--gaps-from` manifest (v2) | Reuse a prior gap list + embedded `ScanRecipe`; mismatch → error on index entries. Prerequisites (ranges, recipe) shipped. Sketch: [archive/TEMP-gap-selection-deferred.md](docs/dev/archive/TEMP-gap-selection-deferred.md) §2. `--scan-window` is **refused** (identity): [gap-vocabulary.md](docs/dev/gap-vocabulary.md) § Gap numbering. |
| `limit_fill_to_mapped_region` on scan report | Wrong home; recipe plan explicitly out of scope — separate cleanup if ever moved |
| Absolute B occupancy via `BlockLevel.silent` (not aggregate RMS) | Optional; `silent` is now retained for equivalence (F2). Fillability still uses aggregate `rms_db` vs abs floor — switch if multichannel false-unfillable shows up. |

### Fingerprint provenance follow-ups

Leftovers from [archive/TEMP-fingerprint-provenance-plan.md](docs/dev/archive/TEMP-fingerprint-provenance-plan.md)
(Tracks A + B shipped 2026-07-31). All three were **deliberately deferred**, each with a stated
trigger — none is a known defect. Shipped behaviour: [gap-fingerprint.md](docs/dev/gap-fingerprint.md)
§ *Source identity & the corpus library* and § *`measurement`*.

| Item | Direction |
|------|-----------|
| I1-class bin-divergence warn in `equivalence-calibration` (optional; was out of that plan's DoD) | Emit lives: flag gaps where `a_gap_total_blocks × measurement.bin_ms` disagrees with the geometry span. The check is documented and the fields are on both verdicts, so this is only automating a query a human can already run. Trigger: a second bin-width divergence (I1 was found by reading source, which is what made the fields Derived) |
| Row-level "no provenance" flag on `GapRow` | Deferred: `check.rs`'s health Warn plus the census's `(absent)` bucket already make an unanswerable corpus say so, and the pattern to mirror is `registration_from_legacy_lag`. Trigger: a report that needs to **filter** rows on it — nothing does today |
| `bit_depth` string → `BitDepth` parser | Deferred: the forward pin (`bit_depth_tokens_are_pinned`) is what protects corpora already on disk; a parser is dead code until a consumer reads the token, and none does. `bit_depth` is stored-for-later by design |

### Fingerprint contract follow-ups (C3)

Leftovers from [archive/TEMP-fingerprint-field-clarity-plan.md](docs/dev/archive/TEMP-fingerprint-field-clarity-plan.md)
§ 2.8 (R1–R3 + C1 + C2 shipped; all eight groups carry a contract).
**Deferred, not refused.** Shipped behaviour: [gap-fingerprint.md](docs/dev/gap-fingerprint.md) § *`_contract`*.

| Item | Direction |
|------|-----------|
| Share the contract string table with harness `legend_text()` | Goal was "dump and analyzer legend cannot drift" (§ 2.5). Sharing couples a 22-char CLI label with ≤120-char prose. Trigger: an *observed* drift, not the possibility of one |
| `--fingerprint-contracts=once\|always\|off` write flag | Deferred to "if size becomes an issue". Trigger: a corpus where `_contract` repetition is a measured problem |
| **Contract `status.patched` on the production report** | Raised from the registration plan § 0 S5. Fields that misled: `align_adjustment_secs` (signed slide, **not** a lag), `placement_slide_frames` (unsigned, and **≠ `\|align_adjustment\| × 48000` on 82/227 gaps**), `fit_path` (names the **boundary grid**), `pre/post_correlation` (min-reduced). Two design questions first: (a) `Contracted<T>` needs a `T`; `GapPatchStatus::Patched` is an enum variant with inline fields, and `contract.rs` keeps reader metadata out of domain types (`patch_result.rs`). (b) A dump is one gap per file; a report is all gaps in one — per-gap stamping repeats ~90 KB, and a report-root `_contracts` map is what § 2.6 rejected for dumps. Whether that rejection survives a report-shaped file is the open question. `dual_fit_used` / `anchor_seam_used` already always-serialized (no contract needed for that foot-gun) |

### Donor registration leftovers

From the 2026-08 equivalence-band / donor-registration review. Donor Apply and fill-level
check shipped; these remain.

| Item | Direction |
|------|-----------|
| **`equivalence-calibration --replay` reads `GapScanJson`** | Today `--replay` only loads fingerprint `corpus.json` / `GapCorpus`; plain scan JSON already carries the same registration + envelope fields on every gap (`scripts/measure/scan-registration.ps1`). Teach the reader the scan shape so Apply flip/abstain counts come from the production classifier, not a hand reconstruction. Small reader change; no new measurement |
| **Conditional donor test — investigation only** | Ask “is B non-silent *where A is silent*?” (at the registered lag) instead of independent A-floor + donor-occupancy halves. **Do not change the gate yet.** First: count A-silent∩donor-silent coincidence on existing scan JSON. Fill-level catches *loudness* damage only — see [Fill placement](#fill-placement-two-defects--registration-ignored-26g6-class-and-step-gap-with-empty-donor-bridge-31g7). No TEMP until the count says it is worth designing |
| **33/17 placement-path investigation** | Which path placed 33/17’s fill is **unrecorded**. The dump’s `brackets` array is the oracle enumeration, not the candidate production selected. **Next:** instrument the selected candidate / fit path; likely site if anything is tuned is the **acceptance floor**, not the comparator |

### Dual-fit confidence axis

**Fingerprint / analysis only** — not a production dual-fit scope change. Do not wire
into `try_dual_fit` / rescue gating. Census and “do not tighten `gate_pass`”:
[gap-fingerprint.md](docs/dev/gap-fingerprint.md) § *`splice_dualfit`*.

| Item | Direction |
|------|-----------|
| **Add fingerprint `dualfit_confident`** | Leave `gate_pass` alone (goldens and corpus history read it). Derive from min-z + min-prominence (`pre/post_seam_z` p05 3.14 / p25 4.91; `pre/post_seam_prom` p25 0.123). **Do not** consolidate end-search length into dual-fit: throat cohort n=65, `corr(fill−span, bridge−gap)` = +0.06; in the z ≥ 6 cohort, 8 of 19 gaps show a 0 ms end excursion while dual-fit trims 4–28 ms |

### Repair R6 follow-ups

From [archive/repair-write-path-plan.md](docs/dev/archive/repair-write-path-plan.md) post-ship gaps.

| Item | Direction |
|------|-----------|
| `--dry-run` / `--write` | Explicit CLI flags; today write mode is implied by `--wav` / `--mux` or TOML `dry_run = false`. **Still open, and now bigger than it was:** `--patch-only` (shipped) added a fourth run mode — patch, no sink — so the mode is no longer a `dry_run` boolean at all. Four modes (scan-only / `--repair-preview` / `--patch-only` / write) are selected today by three flags plus a TOML boolean, with the mutual exclusions enforced pairwise in `RepairConfig::validate`. If this is picked up, do it as one coherent mode selector rather than bolting `--dry-run` / `--write` onto the side — note `--patch-only` deliberately *keeps* `dry_run = true` (it writes nothing), which an explicit `--write` flag would make incoherent |
| Scratch-buffer regression test | Dedicated unit test for patch PCM path |
| Streaming / chunked WAV encode | Large multi-gap fills without holding full PCM |
| Adaptive gap-signature context (low priority) | Widen `gap_signature_context_secs` per-gap only when the score at the nominal map is below floor, instead of decoding wide B context for every gap. From [energy-signature-plan.md](docs/dev/archive/energy-signature-plan.md) Phase 4; low value since mode-coupled `nominal_bias` already handles drift at the 3 s default |
| Dual-fit oracle: unpin pre-shoulder lag from 0 (optional hardening) | `validate_dual_fit_oracle.rs` (gated `validation-tests`, needs ffmpeg + fetched corpus) only steps the **post**-shoulder (`step_ms`); the pre-shoulder is always sourced from B at lag 0 by construction (`dual_fit_oracle.rs`). The 2026-07-03/05 production bug is now covered end-to-end by a synthetic unit test (`dual_fit_result_passes_the_production_revalidation_gate`), so this isn't blocking. Direction if picked up: optional `pre_step_ms` on `DualFitOracleCase` plus a manifest case with both nonzero |

### Residual gate follow-ups

From [archive/residual-gate-findings.md](docs/dev/archive/residual-gate-findings.md) and
[archive/residual-gate-wiring-plan.md](docs/dev/archive/residual-gate-wiring-plan.md). Test inventory:
[`residual_gate_catalog/`](crates/clip-sync-repair/tests/residual_gate_catalog/).
**Not planned:** `veto_rescue` as default (G5: synthetic-only); F4 pipeline veto (M6).

| Item | Priority | Direction |
|------|----------|-----------|
| **M3** — floor walk vs B haystack OOB | med | `walk_reference_frames` is A-energy only; B OOB → NaN at measure time today. Tighten walk only if field media hits bad geometry |
| **G1** — residual on Pearson-only skips | gap | Veto skips done (`ResidualHeadroomExceeded` + `GapPatchOutcome.residual`). Pearson/structure skips still lack residual unless we measure on last grid candidate when `measure_residual` |
| **L6** — coarse outward walk step | low | `step_frames = window` in `measure_fit_residual_verdict`; changes floors → recalibrate if touched |
| **M4** — MP3 calibration | defer | Manifest rows marked M4; gate is codec-agnostic |
| **FD-1** — fractional-delay cancellation | defer | Sub-sample lag + B resample; re-run floor calibration — see findings § FD-1 |
| **`finale_floor_nan_probe`** | optional test | Unit repro: why Grieg finale floor is NaN (M3-adjacent); catalog backlog |
| **`c1b_acoustic_echo_pipeline_veto`** | optional test | Pipeline `ResidualHeadroomExceeded` under `production_fit` on non-F4 echo fixture — optional C1b |
| **`p2_search_winner_bounds`** | optional test | Bound headroom on search winner vs truth placement — needs design |
| **Informative floor + NaN headroom** | low | `informative: true` with non-finite `worst_headroom_db` does **not** abstain. **Direction:** leave as-is unless a census shows the cell is common and harmful; if tightening, extend `gate_abstains` to require finite headroom — count first |

### Defer / opportunistic

| Item | Direction |
|------|-----------|
| Offset-mapped end placement | After start clip, place B end window at `A_end + Δ` when B has a long leader — [archive/anchored-end-extraction-plan.md](docs/dev/archive/anchored-end-extraction-plan.md) |
| Skip overlapping end fingerprint | Omit end clip when `T_anchor − L` overlaps start window (`domain/policies.rs`, `application/align_videos.rs`) |
| Weighted drift in repair warning | Down-rank end clip in instability synthesis when end confidence is low (`clip-sync-repair/src/infrastructure/cli/output.rs`) |
| Memory / PCM cloning | `Cow` / in-place prep when painful; parallel A/B decode when needed. **Decided (2026-06-11):** future streaming should reuse `scan_*_buckets` callbacks; `MediaSession: Send` allows one session per thread — [PLAN.md](PLAN.md) § Media session semantics, [archive/media-session-redesign-plan.md](docs/dev/archive/media-session-redesign-plan.md) |
| Committed test fixtures | Optional committed MP3; committed verify deferred — [tests/corpus/README.md](tests/corpus/README.md) |
| Verification cost knob | `validation.max_verification_secs` — only on demonstrated friction ([archive/verification-hardening-plan.md](docs/dev/archive/verification-hardening-plan.md)) |
| Test tier follow-ups | [docs/dev/test-tier-remainder.md](docs/dev/test-tier-remainder.md) |
