# Development guide

Build commands, Cargo feature flags per crate, and how to run the full test matrix (including slow and `#[ignore]` tiers).

**Related:** [cli-output.md](cli-output.md) (CLI progress and human-report contract), [corpus-validation.md](corpus-validation.md) (alignment corpus findings), [tests/corpus/README.md](../tests/corpus/README.md), [gap corpus README](../crates/clip-sync-repair/tests/gap_corpus/README.md).

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

Tests fall into four buckets:

| Bucket | Runs with `cargo test --workspace`? | Notes |
|--------|-------------------------------------|-------|
| Unit + adapter | yes | Domain, application (fakes), infrastructure |
| Corpus committed | yes | Pre-generated WAV fixtures under `tests/corpus/wav/` |
| Corpus generated / external | **no** (`#[ignore]`) | Built at test time; needs `--ignored` |
| FFmpeg / mux / slow PCM | **no** (`#[ignore]` or feature-gated) | Needs features, ffmpeg, and/or `--ignored` |

`cargo test --workspace` is the **default PR gate**. It does **not** run ignored tests.

---

## Default / CI commands

```powershell
# Full workspace (unit + adapter + committed corpus + CLI adapter tests)
cargo test --workspace

# Library only
cargo test -p clip-sync

# Committed alignment corpus only (fast; no ffmpeg required)
cargo test -p clip-sync corpus_committed

# Analyzer CLI adapter tests
cargo test -p clip-sync-cli

# Repair: committed gap corpus + unit tests
cargo test -p clip-sync-repair gap_corpus_committed
```

**Recommended PR check** (alignment committed tier):

```powershell
cargo test -p clip-sync corpus_committed
```

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

# Repair: AC-3 dual-track scan smoke (generates dual-track MP4 via ffmpeg)
cargo test -p clip-sync-repair --features ac3,ffmpeg-tests ac3_dual_track
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
cargo test --workspace
cargo test -p clip-sync --features he-aac,test-utils,ffmpeg-tests,ac3 -- --ignored
cargo test -p clip-sync-repair --features ac3,ffmpeg-mux,ffmpeg-tests -- --ignored
cargo test -p clip-sync-cli --features he-aac,ac3
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
| `corpus_query_reference_45min_anchor` | `clip-sync` | `--ignored`; 60 min query-reference oracle |
| `corpus_query_reference_45min_anchor` | `clip-sync` | `--ignored`; 60 min query-reference oracle (~minutes) |
| `corpus_external_cases` | `clip-sync` | `--ignored`; `CLIP_SYNC_CORPUS` |
| `regenerate_committed_wav_fixtures` | `clip-sync` | `--ignored`; overwrites committed WAVs |
| `gap_corpus_generated` | `clip-sync-repair` | `--ignored` |
| `gap_corpus_external` | `clip-sync-repair` | `--ignored`; `CLIP_SYNC_GAP_CORPUS` |
| `gap_corpus_regenerate` | `clip-sync-repair` | `--ignored`; overwrites gap WAVs |
| `pcm_discover_finds_*`, `refine_recovers_large` | `clip-sync` | `--ignored`; slow |
| `mux_writes_video` | `clip-sync-repair` | `ffmpeg-mux` + `--ignored`; ffmpeg |
| `mux_writes_video` (integration) | `clip-sync-repair` | `ffmpeg-mux` + `--ignored`; ffmpeg |

Feature-gated tests (not ignored, but **not compiled** without features): `media_reader_tests` blocks under `ffmpeg-tests` (includes backward-seek MP4/MKV and MKV padded-duration extent tests — WAV backward-seek runs in default `cargo test -p clip-sync`); `ac3_dual_track_b_scan_detects_gap` under `ac3` + `ffmpeg-tests`.

**Optional CI step** for container-specific seek regressions (see [scripts/test-container-seek.ps1](../scripts/test-container-seek.ps1)):

```powershell
.\scripts\test-container-seek.ps1
# or: cargo test -p clip-sync --features ffmpeg-tests backward_seek track_decodable_extent
```
