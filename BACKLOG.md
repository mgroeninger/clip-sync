# Backlog

Open follow-up work for `clip-sync`. Architecture: [PLAN.md](PLAN.md). Repair pipeline:
[docs/pipeline.md](docs/pipeline.md). Test corpus: [docs/dev/corpus-validation.md](docs/dev/corpus-validation.md).
Error handling: [docs/error-mapping.md](docs/error-mapping.md).

Last updated: 2026-08-24.

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

**Next:** [Size the normalize exposure](#fill-normalize-boosts-against-a-non-local-reference) — one
column added to an existing script, against an ear-confirmed defect. Then
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
The 31/g7 step-boundary verdict is **blocked** — [settled.md](docs/dev/settled.md) § D.

| Item | Next action | Done when |
|------|-------------|-----------|
| **26/g6 — registration ignored** | Oracle the axis-2 start sweep: what did the objective score at ≈ −50 ms vs −1450 ms (findings §6.8)? Do **not** retune `fill_fit_early_start_penalty_scale` | The score curve over that axis is recorded and the −1450 ms win is either explained or shown to be a defect |
| **31/g8 — Narrow-capture pre miss** | Write a TEMP. Shipped default `BorderCapture::Narrow` declines the pre at envelope +800 ms / r 0.816; Wide keeps −1146.1 / r 0.974 because the shared capture's 12% trim puts `D` at 1984 ms vs Narrow's 13 ms. Numbers: [archive/TEMP-envelope-adaptive-widen-plan.md](docs/dev/archive/TEMP-envelope-adaptive-widen-plan.md) deciding measurement 2 | A decision exists that is not "flip the default" ([settled.md](docs/dev/settled.md) § B) |
| **Envelope rescue stats** | [TEMP-envelope-rescue-stats-plan.md](docs/dev/TEMP-envelope-rescue-stats-plan.md) — stamp `envelope_peak_z` / `envelope_prominence` / `envelope_curve_n` on rescued and declined shoulders. Diagnostic first; do not retune the 0.70 guard in that Change | The fields land and a dump carries them |
| **Split the `Declined` verdict** | `rescue_shoulder` routes "guard refused" and "no shift needed" to the same `EnvelopeRescue::Declined` — record the reason, or split the variant | A `Declined` **rate** can be read as evidence. Until then, filter on `envelope_edge_pinned` |
| **Unify or stamp the `wide_envelope` reach** | `probes.rs` pins `WIDE_ENV_MAX_LAG_MS = 400.0` while the rescue branch reads `cfg.lag_envelope_max_lag_ms`. Either unify both paths on the same cfg **with recipe-stamping**, or stamp the diagnostic's own reach so the split is legible | A dump taken at a raised `--lag-envelope-max-lag-ms` has a comparable `wide_envelope` |
| **`lag_editorial` confirm** | Piggyback on a `--fingerprint-diagnostics` dump: confirm the diagnostic placement follows the same rescue ordering. Do not treat disagreement with `lag_decision` as a gate signal | The ordering is confirmed or the divergence is filed |
| **Ear-pass 17/g34 and 47/g14** | Cheapest n > 2. 17/g34 is the same well-posed class as 26/g6; 47/g14 is the first failure-to-move. Labels, not a TEMP. Residual grouping 17/g34 with 31/g7 is **not** a shared fingerprint | Both are labelled |
| **α / `border_standoff_secs`** | Fill sits ~25 ms late vs both shoulders; hypothesis is production's 0.35 s standoff. Run existing arms through `scripts/measure/diff-patch-adjustments.ps1`, or drop the item | Confirmed, or dropped |
| **+39 ms fill-start detection bias** | Archived registration plan §2.1. Cosmetic for a discriminator; required before any **absolute** claim about delivered position | The bias is characterized |
| **Two gaps break the frame-672000 identity** | Archived registration plan §3.1 — 589 and 4245 frames. Dump-only | Explained, or the join is fenced |
| **S4 / `applied = 0` cohort** | Conditional ear labels: does a residual guard's domain include gaps that never slid? **Do not run until it would change a decision.** The archived §5 render list is mixed-orientation and stale | A decision depends on it |

## Post-join defects (pairs 31 and 50)

Two ear-confirmed defects at the **post join** of technically-successful patches, plus the two
open failures next to them. Promoted 2026-08-24 from the census and pair-50 notes, both now archived:
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
| **Census completeness** (Low) | Wide-sweep the two remaining `skip`-tier both-dead gaps — 31/g9 and 50/g17 — and settle 25/g11's coordinate-fix status. Needed before any claim that the both-dead population is fully characterized | All three are swept and recorded |
| **Confidence is not tracking audibility on pair 31** | Data point, not yet work: production ranked the audible gap (31/g10) `confidence: high` with both adjust frames 0, and both clean twins lower (g11 `anchor_seam_used` with post correlation 0.376; g8 `marginal`). Fold into the 50/g10 and 31/g10 rows above rather than opening a confidence campaign | A mechanism from the rows above either explains the inversion or shows `confidence` measures something else |

## Fill normalize boosts against a non-local reference

Ear-confirmed defect. Durable reading: [json-output.md](docs/json-output.md) § FillLevelCheck and
[pipeline.md](docs/pipeline.md) § Fill normalization (pair 19/g013, `normalize_gain = 3.88`,
energy-pooled 5 s + 5 s vs −52 dB local shoulders). Boost-only PR archived:
[archive/TEMP-fill-normalize-boost-only-plan.md](docs/dev/archive/TEMP-fill-normalize-boost-only-plan.md).

| Item | Next action | Done when |
|------|-------------|-----------|
| **Size the exposure** (do this first) | Add **one column** — `normalize_gain` — to `scripts/measure/measure-patch-outcomes.ps1`, which today emits `PeakDeltaDb` (`:170`) and `EdgeDeltaDb` (`:183`) but not the gain. Then run `--patch-only` → `fill-level-rollup.csv`. Do **not** design from n=1 | The exposed set is known: large gain **and** quiet immediate shoulders |
| **Make the boost reference local** | Once the sweep lands, pick one: narrow the border window; take the quieter of wide-vs-local; or cap the gain by the local shoulder ratio. `normalize_window_secs` (default 5.0) is already a knob | A gain change ships |
| **Consider defaulting `normalize_fill` to false** | Named because the evidence points at it: **one** ear case where the boost causes an audible defect, **zero** where it demonstrably helps. The honest short-term answer if the sweep shows large boosts are common | Decided either way, from the sweep |
| **Re-run the fill-level roll-up after any gain change** | Already owed once — the boost-only PR changed the gain on every previously-attenuated gap | The roll-up is re-run |
| **Re-label `2026-08-08-edge-listen`** | Pairs 7/8/12 are swapped. Required before quoting "lowest labelled clean" as a bound | Relabelled |
| Residual seam `g` (`a ≈ g·b`) as the production gain | Candidate replacement — estimates level from the **seam** rather than whole-fill RMS. Needs verdict plumbing plus corpus rules for when the estimate is trustworthy | Evaluated against the sweep |
| B-relative / donor-shoulder matching | Candidate replacement — sidesteps the A-reference question, but corrects nothing when the two masters genuinely differ in level, which is the only case normalize exists for | Evaluated against the sweep |
| The attenuation half was never ear-validated | Cheap to close if it ever matters: restore the symmetric clamp, drop the `region.rs` assert, render to a `-before` dir, revert | A decision depends on it |

## Repeat-at-seam prior

Durable reading: [gap-fill-modes.md](docs/gap-fill-modes.md) § Fit unified search and
[json-output.md](docs/json-output.md) § `fit_terms`. `anchor_penalty` is a configuration zero and
needs no work. Do **not** lower `REPEAT_CORR_THRESHOLD` — [settled.md](docs/dev/settled.md) § A.

| Item | Next action | Done when |
|------|-------------|-----------|
| **Delete arm 3** | Unreachable by construction (`REPEAT_SUM_CEILING = 2 × REPEAT_CORR_THRESHOLD`). Delete it; do **not** repair the ceiling | Gone |
| **Split `REPEAT_CORR_THRESHOLD`** | The same 0.55 is both the penalty threshold and the short-bracket extension stop — a veto `fill_repeat_penalty_weight` does not disable. Splitting is the precondition for any retune | Two named constants |
| **Extension-loop instrumentation** | Optional. Record `prev_repeat` / `next_repeat` and the stop reason (`score_ok` / `repeat_ok` / exhaustion) | If the monotone clause always binds first, the threshold is free to move on the penalty side after the split |

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

## Dual-fit confidence axis

**Fingerprint / analysis only.** Do not wire into `try_dual_fit` or rescue gating. Census and "do not
tighten `gate_pass`": [gap-fingerprint.md](docs/dev/gap-fingerprint.md) § *`splice_dualfit`*.

| Item | Next action | Done when |
|------|-------------|-----------|
| **Add fingerprint `dualfit_confident`** | Derive from min-z + min-prominence (`pre/post_seam_z` p05 3.14 / p25 4.91; `pre/post_seam_prom` p25 0.123). Leave `gate_pass` alone — goldens and corpus history read it. Do **not** consolidate end-search length into dual-fit: throat cohort n = 65, `corr(fill−span, bridge−gap)` = +0.06, and in the z ≥ 6 cohort 8 of 19 gaps show a 0 ms end excursion while dual-fit trims 4–28 ms | The field lands on dumps |

## Repair R6

Post-ship gaps from [archive/repair-write-path-plan.md](docs/dev/archive/repair-write-path-plan.md).

| Item | Next action | Done when |
|------|-------------|-----------|
| **Run-mode selector** | Four modes (scan-only / `--repair-preview` / `--patch-only` / write) are selected today by three flags plus a TOML boolean, with mutual exclusions enforced pairwise in `RepairConfig::validate`. If picked up, do it as **one coherent mode selector** — not `--dry-run` / `--write` bolted on the side. Note `--patch-only` deliberately keeps `dry_run = true`, which an explicit `--write` would make incoherent | One selector, exclusions enforced centrally |
| **`region.rs` folder split** | [TEMP-patch-audio-region-module-split-plan.md](docs/dev/TEMP-patch-audio-region-module-split-plan.md) — draft, not started | Split lands |
| **`--flac` lossless output** | [TEMP-flac-output-plan.md](docs/dev/TEMP-flac-output-plan.md) — draft, not started. In-process FLAC encode as a peer of `--wav`; motivated by the RIFF `u32` data-chunk limit (a long repaired timeline overruns WAV) | `--flac` writes a lossless output alongside `--wav` |
| Scratch-buffer regression test | Dedicated unit test for the patch PCM path | Test exists |
| Streaming / chunked WAV encode | Large multi-gap fills without holding full PCM | Implemented |
| Dual-fit oracle: unpin the pre-shoulder lag from 0 | Optional hardening. `validate_dual_fit_oracle.rs` steps only the **post** shoulder; the pre is always sourced from B at lag 0 by construction. Not blocking — the 2026-07-03/05 production bug is covered end-to-end by `dual_fit_result_passes_the_production_revalidation_gate`. Direction: optional `pre_step_ms` on `DualFitOracleCase` plus a manifest case with both nonzero | Both shoulders step |

## Residual gate

From [archive/residual-gate-findings.md](docs/dev/archive/residual-gate-findings.md) and
[archive/residual-gate-wiring-plan.md](docs/dev/archive/residual-gate-wiring-plan.md). Test inventory:
[`residual_gate_catalog/`](crates/clip-sync-repair/tests/residual_gate_catalog/). Six
deferred-on-trigger items (M3, M4, L6, FD-1, NaN headroom, `veto_rescue`) moved to
[settled.md](docs/dev/settled.md) § C.

| Item | Next action | Done when |
|------|-------------|-----------|
| **G1** — residual on Pearson-only skips | Veto skips are done (`ResidualHeadroomExceeded` + `GapPatchOutcome.residual`). Pearson and structure skips still lack a residual — measure on the last grid candidate when `measure_residual` | Every skip class carries a residual |
| `finale_floor_nan_probe` | Optional test. Unit repro of why the Grieg finale floor is NaN (M3-adjacent) | Test exists |
| `c1b_acoustic_echo_pipeline_veto` | Optional test. Pipeline `ResidualHeadroomExceeded` under `production_fit` on a non-F4 echo fixture | Test exists |
| `p2_search_winner_bounds` | Optional test — bound headroom on the search winner vs truth placement. Needs design first | Designed, then written |

## Gap selection

From [archive/TEMP-gap-selection-sequencing-plan.md](docs/dev/archive/TEMP-gap-selection-sequencing-plan.md) §4.
Not required for `--only-gaps` / `--skip-gaps`, both shipped.

| Item | Next action | Done when |
|------|-------------|-----------|
| **`--gaps-from` manifest (v2)** | Reuse a prior gap list + embedded `ScanRecipe`; a mismatch errors on index entries. Prerequisites (ranges, recipe) shipped. Sketch: [archive/TEMP-gap-selection-deferred.md](docs/dev/archive/TEMP-gap-selection-deferred.md) §2 | Implemented |

## Defer / opportunistic

| Item | Direction |
|------|-----------|
| Offset-mapped end placement | After the start clip, place B's end window at `A_end + Δ` when B has a long leader — [archive/anchored-end-extraction-plan.md](docs/dev/archive/anchored-end-extraction-plan.md) |
| Skip overlapping end fingerprint | Omit the end clip when `T_anchor − L` overlaps the start window (`domain/policies.rs`, `application/align_videos.rs`) |
| Weighted drift in repair warning | Down-rank the end clip in instability synthesis when end confidence is low (`infrastructure/cli/output.rs`) |
| Memory / PCM cloning | `Cow` / in-place prep when painful; parallel A/B decode when needed. **Decided 2026-06-11:** future streaming reuses `scan_*_buckets` callbacks; `MediaSession: Send` allows one session per thread — [PLAN.md](PLAN.md) § Media session semantics |
| Committed test fixtures | Optional committed MP3; committed verify deferred — [tests/corpus/README.md](tests/corpus/README.md) |
| Verification cost knob | `validation.max_verification_secs` — only on demonstrated friction |
| Test tier follow-ups | [docs/dev/test-tier-remainder.md](docs/dev/test-tier-remainder.md) |
