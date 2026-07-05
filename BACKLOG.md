# Backlog

Open follow-up work for `clip-sync`. See [PLAN.md](PLAN.md) for architecture, [docs/pipeline.md](docs/pipeline.md) for the repair pipeline (phase by phase), [docs/corpus-validation.md](docs/corpus-validation.md) for the test corpus, and [docs/error-mapping.md](docs/error-mapping.md) for error handling. Shipped work is recorded in `docs/archive/*` and git history.

Last updated: 2026-07-05.

**How this doc works**

- **Open** — actionable items below (problem / direction kept for open work only).
- **Plans** — active drafts under `docs/TEMP-*.md`; archive when shipped.

**Next:** optional [Hexagonal L1/L2](#hexagonal-layer-purity); [Repair R6](#repair-r6-follow-ups).

---

## Active plans

| Plan | Covers |
|------|--------|
| [TEMP-pipeline-perf-redesign-plan.md](docs/TEMP-pipeline-perf-redesign-plan.md) | Pipeline perf + assembly: `GapRepairSpec` characterize→execute split (§2.5), hoists, golden harness (D12) |
| [TEMP-ac3-backend-plan.md](docs/TEMP-ac3-backend-plan.md) | AC-3 capability gate + compile-time `ac3-oxideav` vs `ac3-ffmpeg` decode backends |
| [fill-fitting-plan.md](docs/archive/fill-fitting-plan.md) | Gap fill gate → fit (shipped; optional Phase D follow-ups in backlog) |

**Recently shipped:** [residual / floor gate](docs/archive/residual-gate-wiring-plan.md) (2026-06-26) — default `residual_gate = veto`; unified lag radius; `apply_residual_to_confidence`; per-channel floor (`seam_chosen_and_floor_multichannel`); lazy residual finalize; `residual_band` / `donor_relation` reporting; AAC/Vorbis calibration; validity contract C1a+C2–C4 (`tests/residual_gate_catalog/`). Prior: [residual channel alignment](docs/archive/residual-channel-alignment-plan.md) (2026-06-26) — residual/floor follows Pearson energy-selected channels (`selected_seam_channels`, `seam_chosen_and_floor_multichannel`, `shared_alignment_lag`); gate inputs (`worst_headroom_db`, `informative`) live on channel-aligned measurements with default `residual_gate = veto`; multichannel corpus/oracle fixtures + Pearson/residual selection parity tests. Prior: [energy-signature gap structure matching](docs/archive/energy-signature-plan.md) (2026-06-25) — gated log-RMS energy envelope as the `fit`-path structure tier (`gap_signature_mode = auto` default), `GapSignature { Bool, Energy }`, flat-envelope fallback, `--gap-signature-mode` / `--gap-signature-context-secs` flags, corpus tuning **EC-1–EC-6** + mode-coupled `nominal_bias`; Phase 4 FFT/landmarks closed won't-do, adaptive context parked (see Repair R6 follow-ups). Prior: f32 internal PCM + source-driven output bit depth (2026-06-25) — `MultiChannelPcm.samples` is now normalized `Vec<f32>` throughout the repair/write path; `WavPatchedAudioWriter` and the ffmpeg mux pipe resolve output depth from `source_bit_depth` (`Int24 | Int32 | Float32 → 24-bit int WAV / s24le pipe`; lossy / 16-bit stays 16-bit); `MonoPcmClip` (chromaprint) remains `Vec<i16>`. Prior: energy-aware seam channel selection (2026-06-23) — `fill_seam_correlations` / splice scorer now follow the channel(s) carrying signal (within ~20 dB of the loudest) instead of hardcoded front L/R, so center-dominant 5.1 mixes are scored on the center channel; previously front-L/R noise produced ~0 pre/post seams and made fit mode skip patchable gaps (`policies.rs`, `seam_score_channel_indices`). Prior: [energy signature production corpus](docs/archive/energy-corpus-plan.md) (2026-06-23) — F1/F2/F3-long + F4-decoy synthetic fixtures, mode matrix, **EC-1–EC-6**, mode-coupled `fill_fit_energy_nominal_bias_scale`, committed scan→patch CI smoke (Phase F profile→synthesize dropped as not decision-relevant). Prior: [patch-anchor offset map](docs/archive/patch-anchor-offset-plan.md) (2026-06-22) — `anchored_retry` two-pass offset anchors, `fill_anchor_*` config, optional marginal pass-2 upgrade.

## Open work

### Hexagonal layer purity

From architecture audit (2026-06-22). Dependency rule: **domain ← application ← infrastructure**; domain/application must not depend on `clap`, Symphonia, Chromaprint, or misplaced infra helpers.

| # | Priority | Item | Status | Direction |
|---|----------|------|--------|-----------|
| H1 | High | Mux bitrate policy in repair `infrastructure/` | **Shipped** | `application/mux_bitrate.rs`; infra keeps ffmpeg subprocess only |
| H2 | High | `clap::ValueEnum` on repair domain enums | **Shipped** | `FromStr` on `FillMode`, `FillOffsetMode`, `GapSignatureMode`; CLI `value_parser!` |
| M1 | Medium | `GapReport` embeds lib report DTOs | **Shipped** (verified 2026-06-22) | `GapReport.alignment` is `clip_sync::AlignmentResult`; top-level `overlap` removed (use `alignment.start_overlap`); `GapScanJson` in infra maps to `AlignmentReport` + `TimelineOverlapReport` for JSON/human output |
| M2 | Medium | Lib application → infrastructure leaks | **Shipped** | `ExtractionProgressScope` → `application/`; `ClipRepetitionDetector` port + `ChromaprintClipRepetitionDetector` adapter |
| L1 | Low | `run_align` typed on `AppConfig` | **Shipped** | `run_align` takes `&AlignConfig` + paths; `infrastructure/cli` maps `AppConfig` at composition root |
| L2 | Low | Composition root in `infrastructure/cli` | **Shipped** | `composition.rs` in each binary crate wires adapters; `run_repair` orchestrates scan/patch; CLI modules are args/overrides/output only |

**Refs:** [PLAN.md](PLAN.md) § Hexagonal architecture; [layer-purity-plan](docs/archive/layer-purity-plan.md) (lib DSP ports, shipped 2026-06-11)

---

### Repair R6 follow-ups

From [archive/repair-write-path-plan.md](docs/archive/repair-write-path-plan.md) post-ship gaps.

| Item | Direction |
|------|-----------|
| `--dry-run` / `--write` | Explicit CLI flags; today write mode is implied by `--wav` / `--mux` or TOML `dry_run = false` |
| Scratch-buffer regression test | Dedicated unit test for patch PCM path |
| Streaming / chunked WAV encode | Large multi-gap fills without holding full PCM |
| Adaptive gap-signature context (low priority) | Widen `gap_signature_context_secs` per-gap only when the score at the nominal map is below floor, instead of decoding wide B context for every gap. From [energy-signature-plan.md](docs/archive/energy-signature-plan.md) Phase 4; low value since mode-coupled `nominal_bias` already handles drift at the 3 s default |
| `fill_repeat_correlations` channel alignment (low priority) | Repeat tier scores the mono downmix **and** every per-channel pair, taking the best — but without the ~20 dB energy **selection** (`seam_score_channel_indices`) that seam Pearson and residual now share, so the three discriminators don't measure the same channel set. Consistency cleanup, not a correctness bug (peak-normalized near-silent channels score ~0). **Fix:** restrict `ch_pre`/`ch_post` to `selected_seam_channels` and **drop the mono term**; replace its defensive role (the per-channel folds start at `NEG_INFINITY`) with an explicit **`0.0` default** for unscoreable cases — empty selection *or* no per-channel window fits. Empty selection needs no mono fallback: it means every channel is digital silence (the loudest channel is always selected when any content exists), so `0.0` ("no repeat") is the honest result. **Caveat:** mono is combined via `max`, so unlike the residual floor it only *adds* sensitivity — a repeat smeared across channels could correlate better in the downmix than in any single channel (rare; for a center-dominant repeat peak-normalization makes `mono ≈ center`). Dropping mono is therefore a deliberate, tiny sensitivity change — add a smeared-multichannel-repeat test to confirm the per-channel path still catches it. From [TEMP-residual-channel-alignment-plan.md](docs/TEMP-residual-channel-alignment-plan.md) §8 |
| Dual-fit oracle: unpin pre-shoulder lag from 0 (optional hardening) | `validate_dual_fit_oracle.rs` (gated `validation-tests`, needs ffmpeg + fetched corpus) only steps the **post**-shoulder (`step_ms`); the pre-shoulder is always sourced from B at lag 0 by construction (`dual_fit_oracle.rs`). The 2026-07-03/05 production bug (dual-fit's re-validation gate wrongly applying the single-rigid-lag crossfade branch, fixed via `SpliceSeamContext::single_lag_alignment`) is now covered end-to-end by a synthetic unit test (`dual_fit_result_passes_the_production_revalidation_gate`, `domain/dual_fit.rs`), so this isn't blocking. Direction if picked up: add an optional `pre_step_ms` field to `DualFitOracleCase` (default `0.0`, mirroring `step_ms`) that shifts the pre-gap portion the same way the post-gap portion is shifted, plus a manifest case with both nonzero — proving the real-codec path too, not just the synthetic gate. Not yet implemented or run (needs ffmpeg + real media, which wasn't fetched here per the licensed-media partition) |

---

### Residual gate follow-ups

From [archive/residual-gate-findings.md](docs/archive/residual-gate-findings.md) and
[archive/residual-gate-wiring-plan.md](docs/archive/residual-gate-wiring-plan.md). **Shipped:**
default `veto`, C1a+C2–C4 contract. Test inventory:
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

**Explicitly not planned:** `veto_rescue` as default (G5: synthetic-only); F4 pipeline veto (M6).

---

### Defer / opportunistic

| Item | Direction |
|------|-----------|
| [Offset-mapped end placement](#offset-mapped-end-placement) | After start clip, place B end window at `A_end + Δ` when B has a long leader — see [archive/anchored-end-extraction-plan.md](docs/archive/anchored-end-extraction-plan.md) follow-ups |
| [Skip overlapping end fingerprint](#skip-overlapping-end-fingerprint) | Omit end clip when `T_anchor − L` overlaps start window |
| [Weighted drift in repair warning](#weighted-drift-in-repair-warning) | Down-rank end clip in instability synthesis when end confidence is low |
| [Memory / PCM cloning](#memory-use-and-pcm-cloning-on-long-clips) | `Cow` / in-place prep when painful; parallel A/B decode when needed |
| [Log file appender](#log-file-appender) | `tracing-appender` in `logging/mod.rs` |
| [Committed test fixtures](#committed-test-fixtures) | Optional committed MP3; committed verify deferred — see [tests/corpus/README.md](tests/corpus/README.md) |
| [Verification cost knob](#verification-cost-knob) | `validation.max_verification_secs` — only on demonstrated friction |
| [Test tier follow-ups](docs/test-tier-remainder.md) | `clip-sync` ignore cleanup (~1 h), optional `pr-repair-extended` / SP on PR, Phase 2b binary split — see [test-tier-remainder.md](docs/test-tier-remainder.md) |

#### Memory use and PCM cloning on long clips

15-minute default clips; full PCM in memory per extracted window; no streaming fingerprint API yet. **Decided (2026-06-11):** future streaming should reuse `scan_*_buckets` callbacks; `MediaSession: Send` allows one session per thread when parallel decode lands — see [PLAN.md](PLAN.md) § Media session semantics and [archive/media-session-redesign-plan.md](docs/archive/media-session-redesign-plan.md).

**Refs:** `application/align_videos.rs`, `domain/pcm_preparation.rs`

#### Log file appender

`--log-file` parsed but not implemented.

**Refs:** `infrastructure/logging/mod.rs`

#### Committed test fixtures

Tier B = 3× 30 s WAV pairs; ffmpeg for encoded formats. Hold-out verify on committed tier deferred — generated-only coverage documented in [tests/corpus/README.md](tests/corpus/README.md).

**Refs:** `tests/corpus/`, `Cargo.toml` features

#### Verification cost knob

Optional `validation.max_verification_secs` — deferred in [archive/verification-hardening-plan.md](docs/archive/verification-hardening-plan.md). Implement only if verify decode cost becomes painful in practice.

**Refs:** [corpus-validation.md](docs/corpus-validation.md) § Hold-out verification cost

#### Offset-mapped end placement

Symmetric `SharedTimeline` places end windows at the same absolute times on A and B; when B has a long leader before shared content, offset-mapped end (`[T_a−L, T_a]` on A, shifted by Δ on B) would align fingerprints to the same story region.

**Refs:** [archive/anchored-end-extraction-plan.md](docs/archive/anchored-end-extraction-plan.md)

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
- Patched repair output is **`clip-sync-repair`** only — [archive/repair-write-path-plan.md](docs/archive/repair-write-path-plan.md)
- Network or streaming sources
