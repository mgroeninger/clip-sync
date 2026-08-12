# Backlog

Open follow-up work for `clip-sync`. See [PLAN.md](PLAN.md) for architecture, [docs/pipeline.md](docs/pipeline.md) for the repair pipeline (phase by phase), [docs/dev/corpus-validation.md](docs/dev/corpus-validation.md) for the test corpus, and [docs/error-mapping.md](docs/error-mapping.md) for error handling. Shipped work is recorded in `docs/dev/archive/*` and git history.

Last updated: 2026-08-11.

**How this doc works**

- **Open** — actionable items below (problem / direction kept for open work only).
- **Plans** — active drafts under `docs/dev/TEMP-*.md`; archive when shipped.

**Next:** [Fill placement vs local registration](#fill-placement-is-accepted-without-checking-local-registration-defect); [Donor registration leftovers](#donor-registration-leftovers); [Repair R6](#repair-r6-follow-ups); [Residual gate](#residual-gate-follow-ups).

---

## Active plans

| Plan | Covers |
|------|--------|
| [TEMP-fill-placement-registration-plan.md](docs/dev/TEMP-fill-placement-registration-plan.md) | Fill placement accepted without checking local registration — defect characterized, candidate guard measured; Phase A (source vs gate) not started |
| [TEMP-nway-donor-alignment-plan.md](docs/dev/TEMP-nway-donor-alignment-plan.md) | N-way donor alignment: repair one damaged copy from multiple donors — draft, not started |
| [TEMP-flac-output-plan.md](docs/dev/TEMP-flac-output-plan.md) | In-process `--flac` lossless output (peer of `--wav`, no ffmpeg) — draft, not started |

## Open work

### Fill placement is accepted without checking local registration (defect)

**Plan: [TEMP-fill-placement-registration-plan.md](docs/dev/TEMP-fill-placement-registration-plan.md).**

The corpus's only ear-confirmed wrong repairs — pair 26 gap 7 and pair 31 gap 8 — are shoulder
duplications, both accepted at `patch_tier: high` / `confidence: high`, found by ear and by no gate.
Each fill carries an `align_adjustment_secs` that is correct on 10 of 12 labelled gaps; on these two
it slides **backward** by 0.94–1.45 s where the local shoulders were already registered, landing the
donor window on material A has already played. The defect is not the size of the adjustment — it is
that a large displacement is accepted without being checked against what the shoulders say the
offset should be, because **no component owns the question "is this placement plausible?"**

A candidate discriminator (`applied − frac_lag_ms`, signed backward, gated on shoulder confidence)
fires on exactly those two gaps and nothing else in 227. Before it can ship, the plan has to settle
whether the fix belongs in the placement search or in a new guard (Phase A), and whether the
`applied = 0` cohort is in the guard's domain (Phase B). Several readings have already been measured
and **withdrawn** — see the plan before re-proposing pair-level drift correction, correlation gain,
a prominence floor, or a level threshold.

### Fill normalize boosts against a non-local reference (ear-confirmed defect)

**Ear-confirmed 2026-08-12.** Pair 19 gap 14 (`g013`) is the corpus's **only** "audible — too loud"
label, and the cause is normalize's own boost, not donor content: it carries
`normalize_gain = 3.88` (+11.78 dB, **unclamped** — `max_fill_gain_db` sits at 3.98), and the same
gap rendered with `--no-normalize` sits inline with its surroundings and matches the donor. Same
placement, same seam, gain the only variable. This **refutes** the shoulder-continuity explanation
previously attached to that clip (`docs/dev/archive/` fill-level notes and the 2026-08-06 labels).

**Mechanism, traced not guessed.** `compute_a_border_rms` (`region.rs:2312`) pools 5 s before + 5 s
after the gap into a single energy-weighted RMS. Energy pooling means any loud program inside those
10 s dominates it. Here the *immediate* shoulders are −52.1 / −52.0 dB, so matching that wide
reference put the fill roughly 12 dB above the audio directly either side of the seam — at a 10 ms
crossfade, an audible step. The gain was not clamped: it faithfully matched a reference that is not
the local context. The boost-only PR (2026-08-12) removed the *attenuating* half of this same
statistic and does not touch this half.

**Contaminates the fill-level corpus.** `measure_fill_level` measures `gained_fill` but takes
shoulders from ungained A, so a boost inflates `peak_delta_db` ≈1:1. Backing out this gap's gain
gives 15.93 − 11.78 = **4.15 dB** — below the lowest clip ever labelled *clean* (6.37), rather than
second-highest in the labelled set. All 227 rows of `gap-files/2026-08-07-fill-level-shape/`
are inflated by their own gap's unknown gain (0–12 dB). Cross-gap `peak_delta_db` comparisons are
invalid; do not rank or shortlist on that column.

| Item | Direction |
|------|-----------|
| **Size the exposure before designing a fix** (do this first) | Do **not** design from n=1. `--patch-only --format json` now emits `normalize_gain` on every patched gap and runs the full patch path with **no** fingerprint/oracle cost, so a multi-pair sweep is cheap. The tooling already exists: `scripts/measure/measure-patch-outcomes.ps1` is exactly this run (`--patch-only` → `fill-level-rollup.csv`) and needs **one column added** — it emits `PeakDeltaDb`/`EdgeDeltaDb` (`:170`, `:183`) but not `normalize_gain`. With that column: large gain **and** quiet immediate shoulders = the exposed set, and every existing delta becomes divisible by its own gain (see the contamination note above). First honest count of how often this fires, and the input that decides between a reference-window fix and the blunt option below |
| Make the boost reference local | Candidate directions once the sweep lands: narrow the border window; take the quieter of wide-vs-local; or cap the gain by the local shoulder ratio. All are guesses today. Note `normalize_window_secs` (default 5.0) is already a knob, so a calibration sweep is possible without new code |
| Consider defaulting `normalize_fill` to false | Named explicitly because the evidence now points at it: **one** ear case where the boost causes an audible defect, **zero** ear cases where it demonstrably helps — the only support for boosting is a synthetic fixture (`patch_audio_normalizes_fill_loudness_to_a_border`), never a listen. The boost-only plan lists this as a non-goal, but that was written before the boost was implicated. If the sweep shows large boosts are common, this is the honest short-term answer while a better reference is designed |
| Re-run the fill-level roll-up after any gain change | Any change here moves `peak_delta_db` / `edge_delta_db` for the affected gaps, so `gap-files/2026-08-07-fill-level-shape/` becomes a different-policy baseline again. Same caveat the boost-only plan § 5.5 recorded. **Already owed once:** the boost-only PR itself changed the gain on every previously-attenuated gap, so that roll-up is a mixed-policy baseline *today*, before any further work here |
| Residual seam `g` (`a ≈ g·b`) as the production gain | Carried over from the boost-only plan § 1 follow-ups, which has no other home now that plan is archived. The attraction is that it estimates level from the **seam** rather than from whole-fill RMS, so neither the mid-gap-event failure (attenuation, fixed) nor the wide-window failure (boost, open above) applies. Deferred there because it needs verdict plumbing plus corpus rules for when the estimate is trustworthy — not because it was refused |
| B-relative / donor-shoulder matching as a full replacement for A-border RMS | Also from § 1 follow-ups. Matching the fill to **B's own surrounding level** sidesteps the A-reference question entirely; the open issue is that it corrects nothing when the two masters genuinely differ in level, which is the only case normalize exists for. Worth evaluating against the sweep data above rather than in the abstract |
| The attenuation half was never ear-validated | Stated so it is not mistaken for a proven win. The boost-only PR removed attenuation on evidence of mechanism (whole-fill RMS is the wrong statistic) plus earlier "loud event is correct content" labels — but no before/after A/B was ever rendered, because pair 19's only pre-change render turned out to be a boost gap. The two affected gaps sound clean *now*; nobody has heard them sound bad *before*. Cheap to close if it ever matters: restore the symmetric clamp, drop the `region.rs` assert, render to a `-before` dir, revert |

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
| **Contract `status.patched` on the production report** | Raised 2026-08-12 from [TEMP-fill-placement-registration-plan.md](docs/dev/TEMP-fill-placement-registration-plan.md) § 0 S5, where misread patch-record fields produced two withdrawn conclusions. The production `--json` surface carries **no `_contract` anywhere**; the fields that misled want one — `align_adjustment_secs` (signed slide of the chosen fill start off nominal, **not** a lag), `placement_slide_frames` (unsigned, and **≠ `\|align_adjustment\| × 48000` on 82/227 gaps**), `fit_path` (names the **boundary grid**, not the fit path — `gap_tags.rs`'s `FitPathTag::from_boundary_grid_used`), `pre/post_correlation` (min-reduced into the placement objective). **Two design questions come first, which is why this is deferred and not just unwritten.** (a) `Contracted<T>` needs a `T` to flatten; `GapPatchStatus::Patched` is an enum variant with inline fields, so stamping it means extracting them into a named struct — and `contract.rs`'s own rationale deliberately keeps reader metadata *out of* domain types, which `patch_result.rs` is. (b) Per-gap stamping does not transfer: a dump is one gap per file, a report is all gaps in one, so a 227-gap report repeats the same four strings 227 times (~90 KB against files that run 78–574 KB today) — the same measurement the row above is waiting on, and the cheap alternative (one `_contracts` map at the report root) is exactly what § 2.6 rejected as "too easy to skip and too far from the number it explains". That rejection was reasoned about a per-gap file; whether it survives a report-shaped one is the open question. **Already landed separately** (no contract needed): `dual_fit_used` / `anchor_seam_used` are now always serialized, so absence no longer reads as "disabled" |

### Fingerprint dump naming says "skip" for gaps production patches

Raised 2026-08-12 while ear-checking the boost-only normalize PR: two pair-19 gaps whose files are
named `..._full_skip` are **patched in a normal run**, and the name caused a wrong inference about
production behaviour before the schema was re-read. Not a repair defect — a diagnostics-ergonomics
one — but it cost real time, so it is written down rather than remembered.

The trailing token comes from `entry_verdict` (`measure.rs:2962`), which resolves to
`gap.outcome.tier`: the bracket-gate `any_ok` result. That is *correct for what the fingerprint
measures* and is contractually documented as not-the-production-gate
(`schema.rs:1158-1169`: "a `tier: \"skip\"` fingerprint can correspond to a *patched* production
gap"). Production runs `skip_or_dual_fit` after the same failure class and often patches anyway.

| Item | Direction |
|------|-----------|
| **Drop the verdict token from `entry_stem`** | Remove the final `_{}` from the format string (`measure.rs:2986`), delete `entry_verdict`, and check whether `lag_verdict_str` keeps a caller. Safe: `{a8}_{b4}_t{hms}_g{idx}` is already unique and sortable, `entry_stem` has only two callers and both *generate* names, nothing parses the stem, and the rollup CSVs store whole paths. The ears↔JSON join is unaffected and stays guarded by `gap_listen_integration.rs:278`. Nothing is lost: the verdict is already structured data in the gap JSON (`outcome.tier`) and in `manifest.json` (`outcome`). Keep the `full`/`summary` detail-tier token — accurate, flag-stable, and it tells you how much is in the file before you open it. Cost: new dumps stop being name-comparable with existing `gap-files/` dirs (cheap — gitignored, regenerable, self-consistent within a dir) |
| Record production's disposition in the listen manifest | Still worth doing after the token is gone: a listen dir gives no way to see whether production patched a gap without opening the JSON. `gap_listen` already holds the real `GapPatchStatus` when it decides whether to write `_a_patched.wav` (`gap_listen/mod.rs:391-437`), so the manifest entry can carry it. Do **not** encode it in the stem — `entry_stem`'s own doc calls itself the single naming authority for the ears↔JSON join, and a second source of truth is how the two namings drift. Today's only reliable read is *`_a_patched.wav` exists ⇒ production patched it*, since the exporter refuses that file on `Skipped` / `NotPlanned` / `NotApplied` |
| (Same defect, separate axis) The verdict token is not flag-stable | `entry_verdict` prefers `lag_editorial`'s verdict and only falls back to `outcome.tier`, so the same gap gets a different suffix with vs without `--fingerprint-diagnostics`. Dropping the token resolves this too; noted in case the token is kept instead of removed. Related: the stem says "tier" twice for unrelated axes — `detail_tier_str(gap.tier)` (detail) then `gap.outcome.tier` (gate) |

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
