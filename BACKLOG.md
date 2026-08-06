# Backlog

Open follow-up work for `clip-sync`. See [PLAN.md](PLAN.md) for architecture, [docs/pipeline.md](docs/pipeline.md) for the repair pipeline (phase by phase), [docs/dev/corpus-validation.md](docs/dev/corpus-validation.md) for the test corpus, and [docs/error-mapping.md](docs/error-mapping.md) for error handling. Shipped work is recorded in `docs/dev/archive/*` and git history.

Last updated: 2026-08-05.

**How this doc works**

- **Open** — actionable items below (problem / direction kept for open work only).
- **Plans** — active drafts under `docs/dev/TEMP-*.md`; archive when shipped.

**Next:** [Donor registration leftovers](#donor-registration-leftovers); [Repair R6](#repair-r6-follow-ups); [Residual gate](#residual-gate-follow-ups).

---

## Active plans

| Plan | Covers |
|------|--------|
| [TEMP-nway-donor-alignment-plan.md](docs/dev/TEMP-nway-donor-alignment-plan.md) | N-way donor alignment: repair one damaged copy from multiple donors — draft, not started |
| [TEMP-flac-output-plan.md](docs/dev/TEMP-flac-output-plan.md) | In-process `--flac` lossless output (peer of `--wav`, no ffmpeg) — draft, not started |

## Open work

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


### Donor registration leftovers

From the 2026-08 equivalence-band / donor-registration review. Donor Apply and fill-level
check shipped; these remain.

| Item | Direction |
|------|-----------|
| **`equivalence-calibration --replay` reads `GapScanJson`** | Today `--replay` only loads fingerprint `corpus.json` / `GapCorpus`; plain scan JSON already carries the same registration + envelope fields on every gap (`scripts/scan-registration.ps1`). Teach the reader the scan shape so Apply flip/abstain counts come from the production classifier, not a hand reconstruction. Small reader change; no new measurement |
| **Fingerprint `skip_reason` is a placeholder** | Every skip in every dump is `correlation_below_threshold` with zeroed correlations — `measure.rs` invents one variant for the `tier` axis only; `project.rs` can serialize all seven. Thread the real `GapPatchSkipReason` through `compute_region_measurements` (or document the lie until then). Independent of media; same family as residual-abstention reporting |
| **Conditional donor test — investigation only** | Ask “is B non-silent *where A is silent*?” (at the registered lag) instead of independent A-floor + donor-occupancy halves — quiet periodic material can satisfy both in both masters (e.g. 10/12: 4/9 silent on each side, still `repairable_dropout`). **Do not change the gate yet.** First: count A-silent∩donor-silent coincidence on existing 39-pair scan JSON (no re-dump). That rate decides curiosity vs systematic; a wrong threshold drops real dropouts (dangerous direction). Fill-level already catches the observed damage. No TEMP plan until the count says it is worth designing |
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
| `--dry-run` / `--write` | Explicit CLI flags; today write mode is implied by `--wav` / `--mux` or TOML `dry_run = false` |
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
| **Mono/multichannel disagree on "measured"** | med | A side whose probes are **sourced but non-finite** (window found, fit failed) makes the *mono* verdict uninformative — the gate abstains — while the *multichannel* path drops that side from `side_floor_informative` and lets the other side govern, so the veto still applies. Same physical event, opposite gate behaviour; found reviewing residual-abstention reporting (2026-08-05, `uninformative_reason` names the event on both). Multichannel is the stricter reading. Unifying moves the gate either way, so it needs corpus counts (how many gaps have exactly one side sourced-non-finite) before a direction is chosen — **not** a reporting change |

**Explicitly not planned:** `veto_rescue` as default (G5: synthetic-only); F4 pipeline veto (M6).

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
