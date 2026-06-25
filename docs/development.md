# Development guide

Build commands, Cargo feature flags per crate, and how to run the full test matrix (including slow and `#[ignore]` tiers).

**Related:** [cli-output.md](cli-output.md) (CLI progress and human-report contract), [gap-repair-guide.md](gap-repair-guide.md) (gap types and repair recommendations), [gap-fill-modes.md](gap-fill-modes.md) (`fit` vs `gate`, flags, performance), [README.md](../README.md) § Gap patching pipeline, [corpus-validation.md](corpus-validation.md) (alignment corpus findings), [tests/corpus/README.md](../tests/corpus/README.md), [gap corpus README](../crates/clip-sync-repair/tests/gap_corpus/README.md).

---

## Prerequisites

| Tool | Required for |
|------|----------------|
| Rust (stable) | build and all tests |
| `ffmpeg` on `PATH` | generated alignment corpus cases, `ffmpeg-tests` adapter tests, repair mux integration |
| `ffprobe` on `PATH` | repair `mux_writes_video` unit test |

---

## Build

```powershell
# Workspace (all crates)
cargo build
cargo build --release

# Single crate
cargo build -p clip-sync-cli
cargo build -p clip-sync-repair

# Binaries with optional codecs / mux (see feature tables below)
cargo build --release -p clip-sync-cli --features he-aac,ac3
cargo build --release -p clip-sync-repair --features ac3,ffmpeg-mux,he-aac
```

Application crates (`clip-sync-cli`, `clip-sync-repair`) depend on `clip-sync` with its **default** features (`default-tracing`). Pass feature flags on the application crate to enable passthrough features on the library.

---

## Cargo features by crate

### `clip-sync` (library)

| Feature | Default | Purpose |
|---------|---------|---------|
| `default-tracing` | **yes** | Exposes `init_tracing`; wires `tracing-subscriber` for CLI binaries |
| `he-aac` | no | HE-AAC decode via `fdk-aac` |
| `ac3` | no | AC-3 / E-AC-3 decode via `oxideav-ac3` (pure Rust). When off, AC-3 tracks appear in probe output as `decodable: false` and the tool falls back to the first decodable track |
| `ffmpeg-tests` | no | Compiles ffmpeg-backed test helpers and additional `SymphoniaMediaReader` integration tests (not needed for normal corpus runs) |
| `test-utils` | no | Exposes `clip_sync::testing` (`fakes`, `audio_fixtures`, `corpus_fixtures`) for downstream `dev-dependencies` |

Disable default tracing (embedded / library-only consumers):

```powershell
cargo build -p clip-sync --no-default-features
```

### `clip-sync-cli` (analyzer binary)

| Feature | Default | Purpose |
|---------|---------|---------|
| `he-aac` | no | Passthrough: `clip-sync/he-aac` |
| `ac3` | no | Passthrough: `clip-sync/ac3` |

CLI integration tests enable `clip-sync` with `test-utils` and `he-aac` via `dev-dependencies` automatically.

### `clip-sync-repair` (repair binary)

| Feature | Default | Purpose |
|---------|---------|---------|
| `ffmpeg-mux` | no | `MediaMuxer` ffmpeg subprocess adapter; required for `--mux` and video output |
| `he-aac` | no | Passthrough: `clip-sync/he-aac` |
| `ac3` | no | Passthrough: `clip-sync/ac3` |
| `ffmpeg-tests` | no | Passthrough: `clip-sync/ffmpeg-tests` (AC-3 dual-track scan integration test) |

Without `ffmpeg-mux`, `--mux` is rejected at argument parse with a clear error ([error-mapping.md](error-mapping.md)).

**Typical release build for surround + video repair:**

```powershell
cargo build --release -p clip-sync-repair --features ac3,ffmpeg-mux
```

**Inspect audio tracks in a container:**

```powershell
ffprobe -v error -select_streams a -show_entries stream=index,codec_name,channels,channel_layout -of default=noprint_wrappers=1 your_file.m4v
```

---

## Test overview

Tests are grouped into **execution tiers** (when CI runs them). Acceptance row IDs (SD, SP, EC,
RG, …) describe **what** a test proves — see [test-acceptance-glossary.md](test-acceptance-glossary.md).
Tier machinery is documented in [TEMP-test-tier-plan.md](TEMP-test-tier-plan.md).

There are **four** execution tiers. "**oracle**" is *not* a tier — it is a label (the `oracle_`
file/test-name prefix) for domain-acceptance tests that assert against a computed ground-truth;
those schedule as **integration** (repo-only) or **validation** (external dep / exhaustive
contract). Select oracle tests by name (`cargo test oracle_`) or acceptance ID.

| Tier | Purpose | Default PR (`test-tier.ps1`)? | Typical location |
|------|---------|-------------------------------|------------------|
| **unit** | Pure logic, policies, small fakes | yes (`pr` → repair lib with `oracle_`-label skips) | `src/**` `#[test]` |
| **integration** | Patch/scan/CLI on synthetic WAV; seam behavior; repo-only domain-acceptance (`oracle_` label) | yes (subset via `pr-repair`) | `tests/*_integration.rs`, `tests/oracle_*.rs` |
| **validation** | External dep (real codec / ffmpeg / corpus / env) **or** exhaustive off-PR contract (RG, EC6, floor oracle) | no (`#[ignore]` or manual) | `tests/`, lib ignores |
| **diagnostic** | CSV dumps, sweeps, golden generators (emit data, no assertion) | never | manual only |

### Tier decision rule

To place a new (or moved) test, ask these questions **in order**; the first match wins. This
replaces asking "is this an oracle or a validation test?" — that conflated two questions and
produced ambiguous bins.

1. **Does it emit data for humans** (CSV, sweep, golden generator) with no meaningful pass/fail
   assertion? → **diagnostic**.
2. **Does it need an external resource** (ffmpeg binary, downloaded/external corpus, real codec,
   env var) **or** is it a slow exhaustive acceptance/contract matrix run off-PR (RG catalog,
   EC6 sweeps)? → **validation**.
3. **Does it exercise a single module in isolation**, repo-only, deterministic, fast? → **unit**.
4. **Otherwise** (repo-only, deterministic, asserts pass/fail across modules/binaries) →
   **integration**. This includes **oracle** tests — they keep the `oracle_` name prefix as a
   selection label but schedule as integration.

**Speed is not a tier.** A slow repo-only test (e.g. `patch_audio_integration` SP rows) stays
*integration*; it is kept off the default PR via script selection (`pr-repair-extended`, name
filters), not by relabeling it. Only external-dependency or exhaustive-contract work becomes
*validation*. Full machinery: [TEMP-test-tier-plan.md](TEMP-test-tier-plan.md) § Tier decision rule.

**PR gate:** `.\scripts\test-tier.ps1 -Tier pr` (alignment committed corpus + repair smoke +
CLI adapter tests). Does **not** run full `patch_audio_integration` (~15 min) or ignored
validation/diagnostic rows.

`cargo test --workspace` is a **local convenience** compile check only — not the CI PR gate.

### Wall-time budgets (debug, typical dev machine)

| Profile | Budget |
|---------|--------|
| `pr` | ~4–6 min |
| `pr-align` | ~10–30 s |
| `pr-repair` | ~4–6 min |
| `pr-repair-extended` | +~3–8 min (sine seam grid, skips SP rows) |
| `unit` (repair lib, `oracle_`-label skips) | ~30–60 s |
| `integration` (repair, all `--test` binaries) | ~20+ min (includes full `patch_audio`) |
| `validation` | minutes+; ffmpeg + optional corpus env |

---

## Default / CI commands

```powershell
# PR gate (same as GitHub Actions)
.\scripts\test-tier.ps1 -Tier pr

# Per-crate PR slices
.\scripts\test-tier.ps1 -Tier pr-align
.\scripts\test-tier.ps1 -Tier pr-repair   # includes oracle_energy SD rows (skips slow patch smokes)

# Execution tiers (repair crate)
.\scripts\test-tier.ps1 -Tier unit -Package clip-sync-repair
.\scripts\test-tier.ps1 -Tier integration -Package clip-sync-repair
.\scripts\test-tier.ps1 -Tier oracle -Package clip-sync-repair      # convenience: oracle-label rows (integration tier)
.\scripts\test-tier.ps1 -Tier validation -Package clip-sync-repair   # needs ffmpeg
.\scripts\test-tier.ps1 -Tier diagnostic -Package clip-sync-repair -Nocapture

# Extended repair (pr-repair + patch_audio sine grid, skips i1_/i2_/i3_)
.\scripts\test-tier.ps1 -Tier pr-repair-extended
```

**Integration-only** (repair integration binaries, **no `--lib`**):

```powershell
.\scripts\test-tier.ps1 -Tier integration -Package clip-sync-repair
```

Do **not** use `cargo test --tests` for integration-only — Cargo still runs `--lib` with
`--tests`. Use the script or an explicit `--test <binary>` list (see
[TEMP-test-tier-plan.md](TEMP-test-tier-plan.md) Phase 1).

**Local full workspace compile check** (not CI):

```powershell
cargo test --workspace
```

Legacy filters (still valid for ad-hoc runs):

```powershell
cargo test -p clip-sync corpus_committed
cargo test -p clip-sync-cli
cargo test -p clip-sync-repair gap_corpus_committed
cargo test -p clip-sync-repair --test patch_audio_integration
```

### `#[ignore]` convention (new / edited tests)

```rust
#[ignore = "tier:validation — needs ffmpeg + fetch_corpus_sources"]
#[ignore = "tier:diagnostic — CSV export; test-tier.ps1 -Tier diagnostic"]
```

Phase 1 validation/diagnostic tiers in the script still match many legacy ignore reason strings
(`diagnostic:`, `needs fetch_corpus_sources`, etc.) until prefixes are updated opportunistically.

---

## Full test suite

Run these after significant changes or before a release. Order matters only for wall time; each block is independent.

### 1. Workspace baseline

```powershell
cargo test --workspace
```

### 2. Alignment corpus (all tiers)

```powershell
# Committed + generated (~60s with ffmpeg; HE-AAC cases need feature)
cargo test -p clip-sync --features he-aac,test-utils corpus_ -- --ignored

# External long smoke (3600 s case; needs persistent output dir)
$env:CLIP_SYNC_CORPUS = "D:\clip-sync-corpus-external"   # any writable path
cargo test -p clip-sync corpus_external -- --ignored
```

Generated cases that need MP3/MP4/MKV skip automatically when ffmpeg is missing. HE-AAC cases skip when `he-aac` is not enabled.

See [corpus-validation.md](corpus-validation.md) and [tests/corpus/README.md](../tests/corpus/README.md).

### 3. Repair gap corpus (all tiers)

```powershell
# Committed + generated
cargo test -p clip-sync-repair gap_corpus -- --ignored

# External (real media; ground truth in manifest)
$env:CLIP_SYNC_GAP_CORPUS = "F:\Video"   # directory with your MKV files
cargo test -p clip-sync-repair gap_corpus_external -- --ignored
```

See [gap corpus README](../crates/clip-sync-repair/tests/gap_corpus/README.md).

### 4. FFmpeg-gated adapter tests

```powershell
# Extra SymphoniaMediaReader tests (MP4/MKV round-trips, codec probes)
cargo test -p clip-sync --features ffmpeg-tests,he-aac,ac3

# Extract-window regression matrix (split-tone oracle: start / interior / end windows)
# WAV rows run in default CI; encoded rows need ffmpeg + --features ffmpeg-tests
cargo test -p clip-sync extract_window_regression
cargo test -p clip-sync --features ffmpeg-tests extract_window_regression

# MKV/AAC anchored-end align + long backward-seek end extract
cargo test -p clip-sync --features ffmpeg-tests unequal_mkv_aac mkv_aac_anchored

# Repair: AC-3 dual-track scan smoke (generates dual-track MP4 via ffmpeg)
cargo test -p clip-sync-repair --features ac3,ffmpeg-tests ac3_dual_track

# AC-3 oxideav decode quality (corpus chirp → ffmpeg AC-3 → compare railing vs ffmpeg reference)
cargo test -p clip-sync --features ac3,ffmpeg-tests ac3_corpus_chirp -- --nocapture
```

### 5. Repair mux integration

Mux re-encodes patched audio as AAC. Default `mux_audio_bitrate = "match_min"` in `[repair.output]` sets ffmpeg `-b:a` from measured compressed bitrates of A and B during patch decode (see README § Write output).

```powershell
# Build feature required; test is #[ignore] — needs ffmpeg on PATH
cargo test -p clip-sync-repair --features ffmpeg-mux mux_writes -- --ignored
cargo test -p clip-sync-repair --features ffmpeg-mux --test cli_mux_integration -- --ignored
```

`mux_arg_rejected_without_feature` runs without `ffmpeg-mux` (no `--ignored`).

### 6. Slow PCM refinement (library)

```powershell
cargo test -p clip-sync pcm_discover refine_recovers -- --ignored
```

These exercise 60 s synthetic clips; several minutes wall time.

### One-shot “everything local” script

Requires ffmpeg on `PATH`. External corpus tiers are optional (set env vars or skip those lines).

```powershell
.\scripts\test-tier.ps1 -Tier pr
.\scripts\test-tier.ps1 -Tier validation -Package workspace
.\scripts\test-tier.ps1 -Tier diagnostic -Package workspace -Nocapture
# or legacy workspace blanket:
cargo test --workspace
cargo test -p clip-sync --features he-aac,test-utils,ffmpeg-tests,ac3 -- --ignored
cargo test -p clip-sync-repair --features ac3,ffmpeg-mux,ffmpeg-tests -- --ignored
```

---

## Environment variables

| Variable | Used by | Purpose |
|----------|---------|---------|
| `CLIP_SYNC_CORPUS` | `corpus_external_cases` | Writable directory for the 3600 s external alignment case |
| `CLIP_SYNC_GAP_CORPUS` | `gap_corpus_external` | Root directory containing real media files referenced in gap manifest |
| `CLIP_SYNC_WORKSPACE_ROOT` | `corpus_fixtures` | Override workspace root when resolving `tests/corpus/` (rare) |

---

## Fixture regeneration

Alignment committed WAVs (~3.4 MB under `tests/corpus/wav/`):

```powershell
cargo test -p clip-sync regenerate_committed_wav_fixtures -- --ignored --nocapture
# or
.\scripts\generate_corpus.ps1
```

Repair committed gap WAVs:

```powershell
cargo test -p clip-sync-repair gap_corpus_regenerate -- --ignored --nocapture
```

---

## Ignored tests reference

| Test / filter | Crate | Trigger |
|---------------|-------|---------|
| `corpus_generated_cases` | `clip-sync` | `--ignored`; ffmpeg for container cases |
| `corpus_query_reference_45min_anchor` | `clip-sync` | `--ignored`; 60 min query-reference oracle (~minutes) |
| `corpus_external_cases` | `clip-sync` | `--ignored`; `CLIP_SYNC_CORPUS` |
| `regenerate_committed_wav_fixtures` | `clip-sync` | `--ignored`; overwrites committed WAVs |
| `gap_corpus_generated` | `clip-sync-repair` | `--ignored` |
| `gap_corpus_external` | `clip-sync-repair` | `--ignored`; `CLIP_SYNC_GAP_CORPUS` |
| `gap_corpus_regenerate` | `clip-sync-repair` | `--ignored`; overwrites gap WAVs |
| `pcm_discover_finds_*`, `refine_recovers_large` | `clip-sync` | `--ignored`; slow |
| `mux_writes_video` | `clip-sync-repair` | `ffmpeg-mux` + `--ignored`; ffmpeg |
| `mux_writes_video` (integration) | `clip-sync-repair` | `ffmpeg-mux` + `--ignored`; ffmpeg |

Feature-gated tests (not ignored, but **not compiled** without features): `media_reader_tests` blocks under `ffmpeg-tests` (includes backward-seek MP4/MKV and MKV padded-duration extent tests — WAV backward-seek runs in default `cargo test -p clip-sync`); **`extract_window_regression`** (`extract_window_regression.rs`) — cross-format `extract_loop` matrix: WAV mono + interleaved in default CI; MP4 AAC, MKV FLAC/MKV AAC, MP3, and MKV/AAC anchored-end extract/align behind `ffmpeg-tests`; `ac3_dual_track_b_scan_detects_gap` under `ac3` + `ffmpeg-tests`; `ac3_corpus_chirp` oxideav railing characterization under `ac3` + `ffmpeg-tests` (expects zero full-scale samples).

**Optional local step** for container seek + extract regressions (see [scripts/test-container-seek.ps1](../scripts/test-container-seek.ps1)):

```powershell
.\scripts\test-container-seek.ps1
# or manually:
# cargo test -p clip-sync --features ffmpeg-tests backward_seek track_decodable_extent extract_window_regression
```
