# Backlog

Open follow-up work for `clip-sync`. See [PLAN.md](PLAN.md) for architecture, [docs/corpus-validation.md](docs/corpus-validation.md) for the test corpus, and [docs/error-mapping.md](docs/error-mapping.md) for error handling. Shipped work is recorded in `docs/archive/*` and git history.

Last updated: 2026-06-23.

**How this doc works**

- **Open** — actionable items below (problem / direction kept for open work only).
- **Plans** — active drafts under `docs/TEMP-*.md`; archive when shipped.

**Next:** optional [Hexagonal L1/L2](#hexagonal-layer-purity); [Repair R6](#repair-r6-follow-ups).

---

## Active plans

| Plan | Covers |
|------|--------|
| [TEMP-ac3-backend-plan.md](docs/TEMP-ac3-backend-plan.md) | AC-3 capability gate + compile-time `ac3-oxideav` vs `ac3-ffmpeg` decode backends |
| [TEMP-energy-signature-plan.md](docs/TEMP-energy-signature-plan.md) | Gated loudness envelope for gap structure matching (Phases 0–2 shipped; Phase 3 corpus complete; optional Phase 4 open) |
| [fill-fitting-plan.md](docs/archive/fill-fitting-plan.md) | Gap fill gate → fit (shipped; optional Phase D follow-ups in backlog) |

**Recently shipped:** [energy signature production corpus](docs/archive/energy-corpus-plan.md) (2026-06-23) — F1/F2/F3-long + F4-decoy synthetic fixtures, mode matrix, **EC-1–EC-6**, mode-coupled `fill_fit_energy_nominal_bias_scale`, committed scan→patch CI smoke (Phase F profile→synthesize dropped as not decision-relevant). Prior: [patch-anchor offset map](docs/archive/patch-anchor-offset-plan.md) (2026-06-22) — `anchored_retry` two-pass offset anchors, `fill_anchor_*` config, optional marginal pass-2 upgrade.

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
