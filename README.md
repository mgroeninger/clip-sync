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
| `--log-file <FILE>` | — | Write structured logs to file |
| `--try-all-tracks` | — | Try all decodable audio track pairs |
| `--refine-offset-high-rate` | — | Apply native-rate FFT refinement after fingerprint match |
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
| `--dry-run` | `true` | Report gaps only; do not write output |
| `-o, --output <PATH>` | — | Output path (required when `--no-dry-run`) |
| `--format <human\|json>` | `human` | Output format |

Report-only mode exits `0` when analysis completes. No files are written unless `--no-dry-run` and `--output` are both set (requires `ffmpeg` on `PATH`).

---

## Configuration

Settings are merged in this order (later wins): built-in defaults → config file → CLI flags.

The config file is TOML. Pass it with `--config`; if omitted, built-in defaults are used.

### Analyzer config (`clip-sync`)

```toml
[clip]
clip_length = "15m"
num_clips = 1
normalize_loudness = true
trim_silence = true

[alignment]
min_match_score = 0.3
refine_offset_with_pcm = true
refine_offset_high_rate = false   # enable with --refine-offset-high-rate
high_rate_refine_secs = 3
try_all_tracks = false

[output]
format = "human"
show_diagnostics = false

[logging]
level = "warn"
```

### Repair config (`clip-sync-repair`)

```toml
[clip]          # same keys as analyzer
[alignment]     # same keys as analyzer
[logging]       # same keys as analyzer

[repair]
min_gap_ms = 1000
silence_peak_fraction = 0.01
scan_block_ms = 250
decode_chunk_secs = 10
min_fill_correlation = 0.35
crossfade_ms = 10
dry_run = true

[repair.output]
path = "repaired.mp4"
video_codec = "copy"
audio_codec = "aac"
```

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
