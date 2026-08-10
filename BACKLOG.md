# Backlog

Open follow-up work for `clip-sync`. See [PLAN.md](PLAN.md) for architecture, [docs/pipeline.md](docs/pipeline.md) for the repair pipeline (phase by phase), [docs/dev/corpus-validation.md](docs/dev/corpus-validation.md) for the test corpus, and [docs/error-mapping.md](docs/error-mapping.md) for error handling. Shipped work is recorded in `docs/dev/archive/*` and git history.

Last updated: 2026-08-10.

**How this doc works**

- **Open** — actionable items below (problem / direction kept for open work only).
- **Plans** — active drafts under `docs/dev/TEMP-*.md`; archive when shipped.

**Next:** [Fill placement vs local registration](#fill-placement-is-accepted-without-checking-local-registration-defect); [Donor registration leftovers](#donor-registration-leftovers); [Repair R6](#repair-r6-follow-ups); [Residual gate](#residual-gate-follow-ups).

---

## Active plans

| Plan | Covers |
|------|--------|
| [TEMP-nway-donor-alignment-plan.md](docs/dev/TEMP-nway-donor-alignment-plan.md) | N-way donor alignment: repair one damaged copy from multiple donors — draft, not started |
| [TEMP-flac-output-plan.md](docs/dev/TEMP-flac-output-plan.md) | In-process `--flac` lossless output (peer of `--wav`, no ffmpeg) — draft, not started |

## Open work

### Fill placement is accepted without checking local registration (defect)

**The corpus's only ear-confirmed wrong repairs: pair 26 gap 7 and pair 31 gap 8.** Both are
shoulder duplications — the fill leads with a repeat of the material immediately before the gap —
and both were accepted at `patch_tier: high` / `confidence: high`. Found by ear, not by any gate.
Gap numbers throughout are the repair table's 1-based `#`, as `--fingerprint-gap` takes them
(`resolve_fingerprint_gap_select`, `composition.rs`); corpus dumps index the same gaps 0-based.
Data: `gap-files/2026-08-07-fill-level-shape/` (38 pairs, 227 patched gaps) for the reports,
`gap-files/2026-08-09-drift-listen/` (`listen.csv` + `listen-round2.csv`, 12 patched gaps
rendered and labelled 2026-08-09) for the ear pass.

**The mechanism.** Each fill carries an `align_adjustment_secs`. Measured against the *true* local
registration — envelope Pearson over the 2.6 s of pre-gap context in the rendered `_a_surround` /
`_b_surround` clips, 10 ms bins, r 0.929–0.998 on all 12 — the adjustment is **correct on 10 of
12**, faithfully tracking a real misregistration that ranges from 0 to −1.16 s:

| | \|applied − true registration\| |
|---|---|
| the 10 clean gaps | ≤ 0.056 s (median 0.038) |
| 26/#7 (stutter) | **1.410 s** (applied −1.450, true −0.040) |
| 31/#8 (stutter) | **0.937 s** (applied −0.977, true −0.040) |

No overlap; a 17× margin. Both failures slid **backward** where the shoulders were already
registered. A backward slide with no drift to correct lands the donor window on material A has
already played — a shoulder repeat by construction. A forward slide cannot duplicate. So the
defect is not the size of the adjustment, it is that **a large displacement is accepted without
being checked against what the local shoulders say the offset should be.**

**What this refutes.** The previous reading of this item — "a pair-level drift the per-gap search
re-discovers instead of correcting once" — is wrong, and acting on it would make things worse.
Pair 31's ≈ −1.11 s at gaps 9/11/12 is *genuine*: those gaps really are misregistered by −1.15 s
and are correctly placed and clean by ear. Re-fitting the pair alignment once would break them and
would not help #8, which needs ≈ 0. Within pair 31 the true registration *steps* from −0.04 s at
#8 (28 min) to −1.15 s at #9 (86 min); #8's failure is the post-step value applied one gap early.
Also refuted as discriminators, each by a labelled counterexample: seam correlation and
`splice_dualfit.gate_pass` (pair 31's broken seams sound fine; clean-seam gaps stutter),
`confidence` / `patch_tier` (both `high` on both failures), donor-envelope self-similarity
(26/#7 = 0.366 vs clean median 0.160 — wrong direction) and spectral flux (26/#7 = 2.91, inside
the clean range 1.13–3.50).

| Item | Direction |
|------|-----------|
| **Build the guard around registration, not magnitude** | The discriminating quantity is `align_adjustment_secs` minus the locally measured shoulder registration. Nothing in the pipeline computes the latter today. The pre-gap shoulders are already decoded at placement time, so a coarse envelope correlation there is cheap; the open question is whether it can be made robust enough to *veto* (false positive = an unrepaired hole, the same asymmetry that kept fill-level record-only) or should start record-only |
| **`donor_registration` is a partial proxy, not the answer** | `equivalence_production.donor_registration.lag_ms` already measures shoulder registration at 100 ms bins, and agrees with the true value on 9 of 12. It is **wrong exactly where it matters**: −1000 ms at 31/#8 against a true −40 ms. Its `peak_r` does flag trouble (0.576 / 0.370 / 0.369 vs 0.746–0.998 elsewhere), but a `peak_r ≥ 0.7` abstain catches 26/#7 and misses 31/#8. Understand why it fails at #8 before reusing it — the 10 ms leading-context measurement got r = 0.987 on the same gap |
| **Screen the corpus by signed backward magnitude** | `\|align_adjustment\| / gap_length` was the wrong screen (it is what put the four clean pair-31 gaps at the top). Of 227 patched gaps: 53 backward, 92 forward, 82 zero. Backward ≥ 0.9 s is exactly 5 gaps, with a natural break down to 0.382 s. 14 backward gaps in the 0.20–0.38 s band are unheard, concentrated in pairs 13, 22, 37 — render those to find where duplication starts becoming audible, and to confirm the guard's negative side (24/#91, backward 0.377 s and correctly placed, is already rendered and clean) |
| **Do not pursue correlation gain** | `post − pre` was the previous candidate signal and it does not separate these cases: 31/#8 is flat (+0.0001, 0.4368 → 0.4369) and *bad*, 31/#12 *falls* 0.574 → 0.376 and is correctly placed and clean, 7/#7 goes 0.993 → 0.354 and sounds clean. Corpus-wide 67% of patched gaps move it by less than 0.001. It is not a quality measure |

**Caveat: n = 2.** Two labelled failures against ten labelled clean gaps is enough to identify the
mechanism and to kill the old framing, not enough to fix a threshold. The corpus screen above is
what turns it into a number. Selection lesson from the round-2 render: the fingerprint dump's
`manifest.json` `outcome: patch` is the *fingerprint's own* gate simulation and does not predict
production — 6 of 11 gaps chosen that way produced no patch because the scan-time equivalence gate
declined them. Select on the scan report's `patch.gaps[i].status.patched`.

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
§ 2.8 (R1–R3 + C1 + C2 shipped 2026-08-07/08; all eight § 2.4 groups carry a contract).
**Deferred, not refused** — neither has a demonstrated need. Shipped behaviour:
[gap-fingerprint.md](docs/dev/gap-fingerprint.md) § *`_contract`*.

| Item | Direction |
|------|-----------|
| Share the contract string table with harness `legend_text()` | The stated goal was "dump and analyzer legend cannot drift" (§ 2.5). Cost is real: `legend_text()` is a human CLI roll-up on a **22-char label column** — R3 had to re-pad every label and continuation line for one 22-char field name — while contracts are ≤120-char prose read beside the numbers. Sharing couples the two formats, and the four axes do not fit a legend line. Trigger: an *observed* drift between a legend string and a contract, not the possibility of one |
| `--fingerprint-contracts=once\|always\|off` write flag | § 2.6 chose repeat-per-gap ("clarity beats bytes") for diagnostic dumps and deferred the knob to "if size becomes an issue". Nobody has measured the size cost of the eight contracts on a real corpus. Trigger: a corpus where `_contract` repetition is a measured problem — until then `always` is the only behaviour, and a knob is an untested branch |

### Donor registration leftovers

From the 2026-08 equivalence-band / donor-registration review. Donor Apply and fill-level
check shipped; these remain.

| Item | Direction |
|------|-----------|
| **`equivalence-calibration --replay` reads `GapScanJson`** | Today `--replay` only loads fingerprint `corpus.json` / `GapCorpus`; plain scan JSON already carries the same registration + envelope fields on every gap (`scripts/measure/scan-registration.ps1`). Teach the reader the scan shape so Apply flip/abstain counts come from the production classifier, not a hand reconstruction. Small reader change; no new measurement |
| **Conditional donor test — investigation only** | Ask “is B non-silent *where A is silent*?” (at the registered lag) instead of independent A-floor + donor-occupancy halves — quiet periodic material can satisfy both in both masters (e.g. 10/12: 4/9 silent on each side, still `repairable_dropout`). **Do not change the gate yet.** First: count A-silent∩donor-silent coincidence on existing 39-pair scan JSON (no re-dump). That rate decides curiosity vs systematic; a wrong threshold drops real dropouts (dangerous direction). ~~Fill-level already catches the observed damage.~~ **Retracted 2026-08-06:** fill-level catches *loudness* damage only, and not reliably enough to gate on — see [Fill placement vs local registration](#fill-placement-is-accepted-without-checking-local-registration-defect) for damage it does not see at all. No TEMP plan until the count says it is worth designing |
| **33/17 placement-path investigation** | Which path placed 33/17’s fill is **unrecorded**. The dump’s `brackets` array is the oracle enumeration (`list_feasible_anchor_brackets`), not the candidate production selected; rendered seams (`pre_seam_r` 0.998 / `post_seam_r` 0.973) match **no** bracket row (scores top out ~0.43). Bound-/price-extension proposals are dead (comparator already prices move hard; default profile never runs the grid; smaller moves failed the waveform floor). **Next:** instrument the selected candidate / fit path so a later “overrun” proposal has a target; likely site if anything is tuned is the **acceptance floor**, not the comparator |

### Dual-fit confidence axis

**Fingerprint / analysis only** — not a production dual-fit scope change. Do not wire
into `try_dual_fit` / rescue gating (seam ledger A5/D8: uniqueness stays diagnostic until a
labeled false positive). From [archive/TEMP-fill-placement-axis-plan.md](docs/dev/archive/TEMP-fill-placement-axis-plan.md)
Phase B residue (the `gate_pass` / end-search correlation). Axis semantics:
[gap-fingerprint.md](docs/dev/gap-fingerprint.md).

| Item | Direction |
|------|-----------|
| **Dual-fit `gate_pass` is a production mirror, not a discriminator — add a fingerprint confidence axis** | `gate_pass = min(pre_seam_r, post_seam_r) ≥ max(0.35, 0.12)` passes **263/263** on the 17-pair corpus. That is faithful, not broken: it reproduces the production gate exactly, and production's threshold sits far below the observed distribution (p05 of `smin` = 0.892) because `smin` is a ±600 ms argmax with no uniqueness term. Its value is provenance — "what did production decide" — so it should **not** be tightened. What's missing is a *separate fingerprint read* on whether the seam lag was unambiguous (analyzer / corpus roll-up strata — not a repair gate). The validators that discriminate are already emitted but ungated: `pre/post_seam_z` (p05 3.14, p25 4.91, median 7.86) and `pre/post_seam_prom` (p25 0.123). **Direction:** leave `gate_pass` alone (the goldens and corpus history read it); add derived fingerprint field `dualfit_confident` from min-z + min-prominence. **Do not** consolidate end-search length into dual-fit: on the throat cohort (n=65, where `span == gap`) the two disagree genuinely, not by dilution — `corr(fill−span, bridge−gap)` = +0.06, and tightening the amplitude floor drives it to −0.10; stratifying by min z reaches r = +0.68 only at n=8 / p=0.053, one of ~25 strata swept. In the z ≥ 6 cohort, 8 of 19 gaps show an end excursion of exactly 0 ms while dual-fit trims 4–28 ms. |


### Repair R6 follow-ups

From [archive/repair-write-path-plan.md](docs/dev/archive/repair-write-path-plan.md) post-ship gaps.

| Item | Direction |
|------|-----------|
| `--dry-run` / `--write` | Explicit CLI flags; today write mode is implied by `--wav` / `--mux` or TOML `dry_run = false`. **Still open, and now bigger than it was:** `--patch-only` (shipped) added a fourth run mode — patch, no sink — so the mode is no longer a `dry_run` boolean at all. Four modes (scan-only / `--repair-preview` / `--patch-only` / write) are selected today by three flags plus a TOML boolean, with the mutual exclusions enforced pairwise in `RepairConfig::validate`. If this is picked up, do it as one coherent mode selector rather than bolting `--dry-run` / `--write` onto the side — note `--patch-only` deliberately *keeps* `dry_run = true` (it writes nothing), which an explicit `--write` flag would make incoherent |
| Scratch-buffer regression test | Dedicated unit test for patch PCM path |
| Streaming / chunked WAV encode | Large multi-gap fills without holding full PCM |
| Adaptive gap-signature context (low priority) | Widen `gap_signature_context_secs` per-gap only when the score at the nominal map is below floor, instead of decoding wide B context for every gap. From [energy-signature-plan.md](docs/dev/archive/energy-signature-plan.md) Phase 4; low value since mode-coupled `nominal_bias` already handles drift at the 3 s default |
| Dual-fit oracle: unpin pre-shoulder lag from 0 (optional hardening) | `validate_dual_fit_oracle.rs` (gated `validation-tests`, needs ffmpeg + fetched corpus) only steps the **post**-shoulder (`step_ms`); the pre-shoulder is always sourced from B at lag 0 by construction (`dual_fit_oracle.rs`). The 2026-07-03/05 production bug (dual-fit's re-validation gate wrongly applying the single-rigid-lag crossfade branch, fixed via `SpliceSeamContext::single_lag_alignment`) is now covered end-to-end by a synthetic unit test (`dual_fit_result_passes_the_production_revalidation_gate`, `domain/dual_fit.rs`), so this isn't blocking. Direction if picked up: add an optional `pre_step_ms` field to `DualFitOracleCase` (default `0.0`, mirroring `step_ms`) that shifts the pre-gap portion the same way the post-gap portion is shifted, plus a manifest case with both nonzero — proving the real-codec path too, not just the synthetic gate. Not yet implemented or run (needs ffmpeg + real media, which wasn't fetched here per the licensed-media partition) |

---

### Residual gate follow-ups

From [archive/residual-gate-findings.md](docs/dev/archive/residual-gate-findings.md) and
[archive/residual-gate-wiring-plan.md](docs/dev/archive/residual-gate-wiring-plan.md). Test inventory:
[`residual_gate_catalog/`](crates/clip-sync-repair/tests/residual_gate_catalog/).

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
| **Informative floor + NaN headroom** | low | `informative: true` with non-finite `worst_headroom_db` does **not** abstain: band is `correlates_only`, residual gate pass-through (no veto). Shipped MC behaviour, now shared by mono after measuredness unify. Documented in `abstention_reporting_is_decision_neutral`. **Direction:** leave as-is unless a census shows the cell is common and harmful; if tightening, extend `gate_abstains` to require finite headroom (more abstentions) — count first, like the unify plan |

**Explicitly not planned:** `veto_rescue` as default (G5: synthetic-only); F4 pipeline veto (M6).

**Shipped 2026-08-06:** mono/multichannel “measured” semantics unified toward MC
([archive/TEMP-residual-measured-unify-plan.md](docs/dev/archive/TEMP-residual-measured-unify-plan.md)) —
`ProbeNonFinite` ignored like unmeasured on both constructors via shared `combine_informative`.
Combined `uninformative_reason` prefers governing `FloorAboveOkDb` over coexisting `ProbeNonFinite`.

---

### Defer / opportunistic

| Item | Direction |
|------|-----------|
| [Offset-mapped end placement](#offset-mapped-end-placement) | After start clip, place B end window at `A_end + Δ` when B has a long leader — see [archive/anchored-end-extraction-plan.md](docs/dev/archive/anchored-end-extraction-plan.md) follow-ups |
| [Skip overlapping end fingerprint](#skip-overlapping-end-fingerprint) | Omit end clip when `T_anchor − L` overlaps start window |
| [Weighted drift in repair warning](#weighted-drift-in-repair-warning) | Down-rank end clip in instability synthesis when end confidence is low |
| [Memory / PCM cloning](#memory-use-and-pcm-cloning-on-long-clips) | `Cow` / in-place prep when painful; parallel A/B decode when needed |
| [Committed test fixtures](#committed-test-fixtures) | Optional committed MP3; committed verify deferred — see [tests/corpus/README.md](tests/corpus/README.md) |
| [Verification cost knob](#verification-cost-knob) | `validation.max_verification_secs` — only on demonstrated friction |
| [Test tier follow-ups](docs/dev/test-tier-remainder.md) | `clip-sync` ignore cleanup (~1 h), optional `pr-repair-extended` / SP on PR, Phase 2b binary split — see [test-tier-remainder.md](docs/dev/test-tier-remainder.md) |

#### Memory use and PCM cloning on long clips

15-minute default clips; full PCM in memory per extracted window; no streaming fingerprint API yet. **Decided (2026-06-11):** future streaming should reuse `scan_*_buckets` callbacks; `MediaSession: Send` allows one session per thread when parallel decode lands — see [PLAN.md](PLAN.md) § Media session semantics and [archive/media-session-redesign-plan.md](docs/dev/archive/media-session-redesign-plan.md).

**Refs:** `application/align_videos.rs`, `domain/pcm_preparation.rs`

#### Committed test fixtures

Tier B = 3× 30 s WAV pairs; ffmpeg for encoded formats. Hold-out verify on committed tier deferred — generated-only coverage documented in [tests/corpus/README.md](tests/corpus/README.md).

**Refs:** `tests/corpus/`, `Cargo.toml` features

#### Verification cost knob

Optional `validation.max_verification_secs` — deferred in [archive/verification-hardening-plan.md](docs/dev/archive/verification-hardening-plan.md). Implement only if verify decode cost becomes painful in practice.

**Refs:** [corpus-validation.md](docs/dev/corpus-validation.md) § Hold-out verification cost

#### Offset-mapped end placement

Symmetric `SharedTimeline` places end windows at the same absolute times on A and B; when B has a long leader before shared content, offset-mapped end (`[T_a−L, T_a]` on A, shifted by Δ on B) would align fingerprints to the same story region.

**Refs:** [archive/anchored-end-extraction-plan.md](docs/dev/archive/anchored-end-extraction-plan.md)

#### Skip overlapping end fingerprint

When `T_anchor − clip_length` overlaps the start window (short shared span), skip end fingerprinting to avoid comparing redundant audio.

**Refs:** `domain/policies.rs`, `application/align_videos.rs`

#### Weighted drift in repair warning

Repair instability warning treats end − start drift equally; down-rank or de-emphasize end when end confidence is low or tail decode was unreliable.

**Refs:** `clip-sync-repair/src/infrastructure/cli/output.rs`

---

## Explicitly out of scope (initial version)

From [PLAN.md](PLAN.md) — not backlog unless scope changes:

- Video frame / visual sync
- Batch processing (> two files)
- Writing aligned output files from the **analyzer** (report offset only)
- Patched repair output is **`clip-sync-repair`** only — [archive/repair-write-path-plan.md](docs/dev/archive/repair-write-path-plan.md)
- Network or streaming sources
