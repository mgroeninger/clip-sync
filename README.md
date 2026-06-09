# clip-sync

Synchronize video recordings by comparing audio. Given two recordings of the same event, `clip-sync` fingerprints audio segments from each file, matches them, and reports the time offset needed to align the two timelines.

A companion tool `clip-sync-repair` (in development) uses the same alignment engine to detect silent gaps in one recording and optionally patch them from an aligned partner file.

---

## Workspace layout

```text
clip-sync/
├── Cargo.toml                  # workspace root
├── docs/                       # corpus-validation, error-mapping, corpus-matrix
├── scripts/                    # generate_corpus.ps1 / .sh
├── tests/
│   └── corpus/                 # manifest.toml, committed WAV fixtures
└── crates/
    ├── clip-sync/              # shared alignment library
    ├── clip-sync-cli/          # analyzer binary (clip-sync)
    └── clip-sync-repair/       # repair binary (clip-sync-repair) [in development]
```

| Crate | Binary | Role |
|-------|--------|------|
| `clip-sync` | — | Shared alignment engine: domain, application use cases, and default adapters (Symphonia, Chromaprint) |
| `clip-sync-cli` | `clip-sync` | Analyzer: reports offset between two video files; read-only |
| `clip-sync-repair` | `clip-sync-repair` | Repair: scans for silent gaps in one recording and optionally patches them from the aligned partner |

---

## Installation

**Prerequisites:** Rust toolchain (stable). For `clip-sync-repair` write mode only: `ffmpeg` on `PATH`.

```powershell
cargo build --release
```

Binaries are written to `target/release/`.

---

## Usage

### Analyzer — `clip-sync`

Report the time offset between two video files:

```text
clip-sync [OPTIONS] <VIDEO_A> <VIDEO_B>
```

`VIDEO_A` is the reference timeline. A positive offset means the matching audio event occurs later on `VIDEO_B`'s clock; a negative offset means it occurs earlier.

**Options:**

| Flag | Default | Description |
|------|---------|-------------|
| `-c, --config <FILE>` | — | Config file path |
| `--clip-length <DUR>` | `15m` | Length of each extracted clip window (min: `1m`) |
| `--num-clips <N>` | `1` | Number of clip windows per video |
| `--format <human\|json>` | `human` | Output format |
| `-v, --verbose` | — | Show diagnostics on stderr |
| `-q, --quiet` | — | Errors only; suppress progress |
| `--log-level <LEVEL>` | — | Tracing log level |
| `--log-file <FILE>` | — | Write structured logs to file (also logs to stderr) |
| `--try-all-tracks` | — | Try all decodable audio track pairs |
| `--no-try-all-tracks` | — | Disable try-all-tracks (overrides config) |
| `--refine-offset-high-rate` | — | Apply native-rate FFT refinement after fingerprint match |
| `--no-refine-offset-high-rate` | — | Disable high-rate refinement (overrides config) |
| `-h, --help` | | |
| `-V, --version` | | |

**Examples:**

```powershell
# Basic alignment report
clip-sync camera_a.mp4 camera_b.mp4

# Two clip windows, JSON output
clip-sync --num-clips 2 --format json camera_a.mp4 camera_b.mp4

# Long recording: three 10-minute windows
clip-sync --clip-length 10m --num-clips 3 recording_a.mp4 recording_b.mp4
```

**Sample output:**

```text
Alignment report
  Start clip aligned: yes
  End clip aligned:   yes
  Start clip [0:00–15:00]: aligned, offset +12.340s (confidence 0.94)
  End clip [30:00–45:00]:  aligned, offset +12.355s (confidence 0.91)
  Recommended offset: +12.340s (clip offsets agree)
```

### Repair — `clip-sync-repair` (in development)

Detect silent gaps in `VIDEO_A` and report whether `VIDEO_B` has audio that could fill them:

```text
clip-sync-repair [OPTIONS] <VIDEO_A> <VIDEO_B>
```

`VIDEO_A` is the recording with gaps. `VIDEO_B` is the reference recording used for alignment and gap filling.

**Options:**

| Flag | Default | Description |
|------|---------|-------------|
| `-c, --config <FILE>` | — | Config file path |
| `--format <human\|json>` | `human` | Output format |
| `--clip-length <DUR>` | `15m` | Length of each alignment clip window (min: `1m`) |
| `--num-clips <N>` | `2` | Number of alignment clip windows per video |
| `--min-gap-ms <MS>` | `1000` | Minimum silent gap duration to report |
| `--silence-fraction <F>` | `0.01` | Silence threshold as a fraction of peak amplitude |
| `--decode-chunk-secs <SECS>` | `10` | Decode chunk size for sequential scan (alias: `--scan-window-secs`) |
| `--scan-block-ms <MS>` | `250` | Analysis block size for silence detection |
| `--scan-both` | on | Scan B's timeline for silence (bidirectional agreement) |
| `--no-scan-both` | — | Disable bidirectional silence scan |
| `--wav <PATH>` | — | Write patched multi-channel WAV (implies write mode) |
| `--mux <PATH>` | — | Mux patched audio into video A (implies write mode; requires build with `--features ffmpeg-mux` and `ffmpeg` on `PATH`) |
| `--no-normalize` | — | Disable loudness normalization of fill segments |
| `--crossfade-ms <MS>` | `10` | Crossfade duration at gap boundaries |
| `-v, --verbose` | — | Verbose progress on stderr |
| `-q, --quiet` | — | Suppress progress output |
| `--log-level <LEVEL>` | — | Tracing log level |
| `--log-file <FILE>` | — | Write structured logs to file (also logs to stderr) |
| `--try-all-tracks` | — | Try all decodable audio track pairs |
| `--no-try-all-tracks` | — | Disable try-all-tracks (overrides config) |
| `--refine-offset-high-rate` | — | Apply native-rate FFT refinement (on by default in repair config) |
| `--no-refine-offset-high-rate` | — | Disable high-rate refinement (overrides config) |
| `-h, --help` | | |
| `-V, --version` | | |

Report-only mode exits `0` when analysis completes (default `dry_run = true` in config). No files are written unless `--wav` or `--mux` is set, or config sets `dry_run = false` with output paths.

---

## Configuration

Settings are merged in this order (later wins): built-in defaults → config file → CLI flags.

The config file is TOML. Pass it with `--config` (or `-c`); if omitted, built-in defaults are used.

In TOML, `clip_length` is an integer number of **seconds** (CLI accepts human-friendly values like `15m` or `90s`).

### Logging

| Source | Precedence |
|--------|------------|
| `RUST_LOG` environment variable | Highest — overrides `[logging].level` and `--log-level` when set |
| `--log-level` / `[logging].level` | Used when `RUST_LOG` is unset |
| `--log-file` / `[logging].log_file` | Appends structured logs to a file; stderr logging continues |

### Analyzer config (`clip-sync`)

```toml
[clip]
clip_length = 900          # seconds (900 = 15 minutes)
num_clips = 1
normalize_loudness = true
trim_silence = true

[alignment]
min_match_score = 0.3
refine_offset_with_pcm = true
refine_offset_high_rate = false
high_rate_refine_secs = 3
try_all_tracks = false

[output]
format = "human"
show_diagnostics = false

[logging]
level = "warn"
# log_file = "clip-sync.log"
```

### Repair config (`clip-sync-repair`)

```toml
[clip]
clip_length = 900          # seconds
num_clips = 2              # repair default is 2 (analyzer default is 1)

[alignment]
refine_offset_with_pcm = true
refine_offset_high_rate = true    # repair default; set false to disable
require_consistent_offsets = false
try_all_tracks = false

[logging]
level = "warn"

[repair]
min_gap_ms = 1000
silence_peak_fraction = 0.01
scan_block_ms = 250
decode_chunk_secs = 10
silence_hold_ms = 500
absolute_silence_rms = 33.0
scan_both = true
gap_offset_tolerance_secs = 0.5
min_fill_correlation = 0.35
fill_align_margin_secs = 1.0
max_fill_align_adjustment_secs = 0.5
crossfade_ms = 10
normalize_fill = true
normalize_window_secs = 5.0
max_fill_gain_db = 12.0
dry_run = true

[repair.output]
wav_path = "patched.wav"
# video_path = "repaired.mp4"   # requires `--features ffmpeg-mux`
video_codec = "copy"
audio_codec = "aac"
```

Example fixtures: `crates/clip-sync-cli/tests/fixtures/analyzer.toml`, `crates/clip-sync-repair/tests/fixtures/repair.toml`.

---

## How clip windows work

Each video is split into one or more fixed-length windows before fingerprinting.

| Setting | Default | Minimum |
|---------|---------|---------|
| `clip_length` | 15 min | 1 min |
| `num_clips` | 1 | 1 |

When the video is shorter than `clip_length`, a single window covering the full duration is used regardless of `num_clips`. With two or more windows, the first is always anchored at the start, the last at the end, and any interior windows are centered in equal subdivisions of the timeline.

**Examples** (`clip_length = 15m`, `num_clips = 2`):

| Duration | Windows |
|----------|---------|
| 12 min | `[0:00 – 12:00]` (full file) |
| 45 min | `[0:00 – 15:00]`, `[30:00 – 45:00]` |
| 60 min | `[0:00 – 15:00]`, `[45:00 – 60:00]` |

---

## Exit codes

### Analyzer

| Code | Meaning |
|------|---------|
| 0 | Alignment report printed |
| 2 | Invalid or unreadable configuration |
| 3 | Domain rule violation (e.g. no audio tracks) |
| 4 | Media I/O, probe, or decode failure |
| 5 | Fingerprint generation failure |
| 6 | Alignment engine failure |

Low-confidence alignment exits `0` with a report — it is not a failure.

Repair exit codes are documented in [docs/error-mapping.md](docs/error-mapping.md).

---

## Development

### Build and test

```powershell
# Full workspace
cargo build
cargo test --workspace

# Library only (includes corpus integration tests)
cargo test -p clip-sync

# Corpus tests with HE-AAC support
cargo test -p clip-sync --features he-aac,test-utils corpus_

# Analyzer CLI tests
cargo test -p clip-sync-cli
```

### Corpus fixtures

Audio fixtures used in integration tests live in `tests/corpus/` at the workspace root. They are committed WAV files (~3.4 MB). See [tests/corpus/README.md](tests/corpus/README.md) for regeneration instructions and [docs/corpus-validation.md](docs/corpus-validation.md) for test tiers and expected results.

### Features

| Feature | Crate | Purpose |
|---------|-------|---------|
| `he-aac` | `clip-sync` | Optional HE-AAC decode via `fdk-aac` |
| `test-utils` | `clip-sync` | Exposes fakes, audio fixtures, and corpus helpers for downstream dev-dependencies |

---

## Architecture

`clip-sync` follows a hexagonal (ports and adapters) architecture across three crates. The library crate ships the alignment engine and its default adapters; each application crate is a thin driving hexagon that wires the library to a CLI.

```
clip-sync (library)
  domain        — entities, value objects, pure policies
  application   — AlignVideos use case, port traits, align_with_defaults
  infrastructure — Symphonia decoder, Chromaprint fingerprinter, logging

clip-sync-cli (analyzer)
  application   — run_align orchestration
  infrastructure — clap args, AppConfig TOML, stdout/JSON output, exit codes

clip-sync-repair (repair)
  domain        — Gap, GapReport, silence policies
  application   — ScanGaps, RepairVideos use cases, port traits
  infrastructure — clap args, gap report output, ffmpeg mux adapter
```

The library's public surface is a facade of explicit re-exports in `lib.rs`. Application crates depend only on those exports — never on internal module paths.

---

## Documentation

- [docs/error-mapping.md](docs/error-mapping.md) — exit codes, user messages, error hierarchy
- [docs/corpus-validation.md](docs/corpus-validation.md) — test tiers, CI commands, known offsets
- [docs/corpus-matrix.md](docs/corpus-matrix.md) — case design matrix
- [PLAN.md](PLAN.md) — full architecture reference
- [BACKLOG.md](BACKLOG.md) — deferred work items
