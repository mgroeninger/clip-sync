# clip-sync

Synchronize video recordings by comparing audio. Given two recordings of the same event, `clip-sync` fingerprints audio segments from each file, matches them, and reports the time offset needed to align the two timelines.

Repository: [github.com/mgroeninger/clip-sync](https://github.com/mgroeninger/clip-sync)

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
| `-v, --verbose` | — | Verbose progress on stderr plus extra fields in the human report |
| `-q, --quiet` | — | Suppress progress on stderr (errors still print) |
| `--log-level <LEVEL>` | — | Tracing log level |
| `--log-file <FILE>` | — | Write structured logs to file (also logs to stderr) |
| `--try-all-tracks` | — | Try all decodable audio track pairs |
| `--no-try-all-tracks` | — | Disable try-all-tracks (overrides config) |
| `--refine-offset-high-rate` | — | Apply native-rate FFT refinement after fingerprint match |
| `--no-refine-offset-high-rate` | — | Disable high-rate refinement (overrides config) |
| `--no-constrain-end-clip-to-start-offset` | — | When start/end Chromaprint disagree, keep independent end estimate (default: PCM-search end around start when start confidence is high) |
| `--no-high-rate-recommended-refusion` | — | After dual-anchor high-rate, apply only start-anchor tweak to recommended offset (default: re-fuse from updated clip offsets) |
| `--check-clip-repetition` | — | Diagnostic: detect internal clip repetition |
| `--verify-offset` | — | Diagnostic: hold-out verification |
| `--query-reference` | — | Force query-reference localization |
| `--symmetric-align` | — | Force symmetric multi-clip alignment |
| `--query-stride <SECS>` | — | Coarse search stride in query-reference mode |
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

#### Short clip against a long recording

When one file is much shorter than the other (or symmetric clip windows do not match), **Auto** mode (default) localizes the short clip on the long timeline instead of failing with a clip-count mismatch. The repair tool uses the same engine: gaps **outside** the located clip coverage are still reported but are not filled unless you pass `--no-limit-fill-region`.

```powershell
# Analyzer: 8-minute phone clip vs 60-minute main recording
clip-sync long_recording.mp4 phone_clip.mp4

# Force query mode or tune coarse search
clip-sync --query-reference --query-stride 60 long_recording.mp4 phone_clip.mp4

# Repair (A long, B short): gaps on A, donor clip on B
clip-sync-repair --query-reference long_recording.mp4 phone_clip.mp4

# Repair (A short, B long): short damaged clip as A, full recording as donor B
clip-sync-repair --query-reference short_clip_with_gaps.mp4 full_recording.mp4
```

**Sample output (query mode, default human):**

```text
Match on video A: 45:00 – 53:00  (8m clip, confidence 0.91)

Gaps in video A (2 found, 1 repaired, 0 skipped, 1 unfillable):
  1   45:30 – 46:00          30.0s    patched
  2   10:00 – 10:30          30.0s    outside clip coverage
```

Use `--verbose` to see offset, B-span, and coarse-search stats. JSON always includes `query_localization` and `recommended_offset_secs` for scripting — see [docs/json-output.md](docs/json-output.md).

| Flag | Tool | Effect |
|------|------|--------|
| `--query-reference` | both | Force query-reference localization |
| `--symmetric-align` | both | Force legacy symmetric multi-clip alignment |
| `--query-stride <SECS>` | both | Coarse search stride on the long file |
| `--no-limit-fill-region` | repair only | Allow gap fill outside located clip coverage |

**Sample output (symmetric mode):**

```text
Alignment report
  Start clip aligned: yes
  End clip aligned:   yes
  Start clip [0:00–15:00]: aligned, offset +12.340s (confidence 0.94)
  End clip [30:00–45:00]:  aligned, offset +12.355s (confidence 0.91)
  Recommended offset: +12.340s (clip offsets agree)
```

With `num_clips ≥ 2`, start and end windows are aligned independently. When Chromaprint agrees within **0.5 s**, the end clip is PCM-refined around the start offset. When they disagree but start confidence is high, **constrained end PCM** (default) still searches the end window around the start offset instead of trusting the independent end Chromaprint estimate. The headline `recommended_offset_secs` uses confidence-weighted fusion when both offsets sit near their median (even if drift exceeds 0.5 s), or start preference otherwise — see [docs/cli-output.md](docs/cli-output.md). After dual-anchor high-rate refinement, recommended offset is **re-fused** from the updated per-clip values (disable with `no_high_rate_recommended_refusion`).

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
| `--silence-hold-ms <MS>` | `500` | Non-silent time to absorb before closing a silence run (`hold = hold_ms / block_ms`) |
| `--absolute-silence-rms <N>` | `33` | Absolute RMS floor for silence (0–32767 scale; `0` disables) |
| `--scan-both` | on | Scan B's timeline for silence (bidirectional agreement) |
| `--no-scan-both` | — | Disable bidirectional silence scan |
| `--wav <PATH>` | — | Write patched multi-channel WAV (implies write mode) |
| `--mux <PATH>` | — | Mux patched audio into video A via ffmpeg (implies write mode; requires build with `--features ffmpeg-mux` and `ffmpeg` on `PATH`). AAC is re-encoded; bitrate defaults to the lower measured rate of A and B (see `mux_audio_bitrate` below) |
| `--no-normalize` | — | Disable loudness normalization of fill segments |
| `--no-structure-trust` | — | Stricter seams (gate only): always run waveform Pearson; no structure skip/soften; both seams required |
| `--min-fill-correlation <N>` | `0.35` | Waveform seam floor (`min(pre, post)` in fit; gate threshold when waveform runs) |
| `--max-fill-align-adjust-secs <SECS>` | `0.5` | Legacy structure polish window; **fit** B slide uses `--fill-border-search-secs` |
| `--fill-border-search-secs <SECS>` | `10` | B slide radius for unified gap-fill search; primary patch performance lever |
| `--fill-align-margin-secs <SECS>` | `1` | Extra B audio extracted on each side of the mapped gap |
| `--gap-signature-context-secs <SECS>` | `3` | A audio on each side of the gap used for structure signatures |
| `--fill-length-slack-secs <SECS>` | `5` | How far B fill end may differ from A gap length when locating post-border |
| `--border-standoff-secs <SECS>` | `0.35` | A-side audio excluded adjacent to the dropout when building border templates |
| `--fill-offset <MODE>` | `recommended` | Per-gap B mapping: `recommended` or `interpolated` (drift between start/end clips) |
| `--fill-mode <MODE>` | `fit` | `fit` (unified search + tiering) or `gate` (legacy Pearson gate). See [docs/gap-fill-modes.md](docs/gap-fill-modes.md) |
| `--fill-fit-structure-weight <N>` | `0.35` | Fit only: unified scorer structure term |
| `--fill-fit-waveform-weight <N>` | `0.65` | Fit only: unified scorer waveform term |
| `--no-gap-end-extend` | — | **Fit:** disable joint grid on gap end. **Gate:** disable post-seam extension retries. Does **not** switch to gate mode |
| `--no-gap-start-extend` | — | **Fit:** disable joint grid on gap start. **Gate:** disable pre-seam extension retries |
| `--no-short-gap-one-strong-seam` | — | Disable short-gap one-strong-seam fallback (**gate only**) |
| `--gap-end-extend-max-ms <MS>` | `500` | Max A-boundary shift: fit = joint grid span; gate = retry span |
| `--gap-end-extend-step-ms <MS>` | `20` | Step for boundary search (fit grid) or retries (gate) |
| `--crossfade-ms <MS>` | `10` | Crossfade duration at gap boundaries |
| `-v, --verbose` | — | Verbose progress on stderr plus detailed gap-patch lines in the human report |
| `-q, --quiet` | — | Suppress progress on stderr (errors still print) |
| `--log-level <LEVEL>` | — | Tracing log level |
| `--log-file <FILE>` | — | Write structured logs to file (also logs to stderr) |
| `--try-all-tracks` | — | Try all decodable audio track pairs |
| `--no-try-all-tracks` | — | Disable try-all-tracks (overrides config) |
| `--refine-offset-high-rate` | — | Apply native-rate FFT refinement (on by default in repair config) |
| `--no-refine-offset-high-rate` | — | Disable high-rate refinement (overrides config) |
| `--no-constrain-end-clip-to-start-offset` | — | Keep independent end Chromaprint when it disagrees with start (default: constrain end PCM to start offset) |
| `--no-high-rate-recommended-refusion` | — | Legacy high-rate behavior: start-anchor tweak only on recommended offset |
| `--query-reference` | — | Force query-reference alignment |
| `--symmetric-align` | — | Force symmetric multi-clip alignment |
| `--query-stride <SECS>` | — | Coarse search stride in query-reference mode |
| `--no-limit-fill-region` | — | Allow gap fill outside located clip coverage |
| `-h, --help` | | |
| `-V, --version` | | |

Report-only mode exits `0` when analysis completes (default `dry_run = true` in config). No files are written unless `--wav` or `--mux` is set, or config sets `dry_run = false` with output paths.

**Scan and patch tuning:** gap detection flags (`--scan-block-ms`, `--silence-hold-ms`, `--absolute-silence-rms`) and patch seam flags (`--fill-mode`, `--min-fill-correlation`, `--fill-border-search-secs`, `--fill-align-margin-secs`, `--gap-signature-context-secs`, `--fill-length-slack-secs`, `--border-standoff-secs`, `--fill-offset`, `--gap-end-extend-*`, `--no-gap-start-extend`) can be set on the CLI or in `[repair]` in a config file. Flags marked **gate only** in the table above (`--no-structure-trust`, `--no-short-gap-one-strong-seam`) apply only with `--fill-mode gate`. Other patch settings (structure bin width, seam search window, normalization window, etc.) are config-only — see the example below.

**Timeline / duration warnings and mux preflight** (overlap start, PTS vs sample-clock skew, PCM vs container length, mux duration gate) are documented in [docs/cli-output.md](docs/cli-output.md#timeline--duration-warnings).

**Write output:** `--wav` writes lossless 16-bit PCM (no re-encode). `--mux` copies video from A and re-encodes the patched audio track (default `audio_codec = "aac"`). Mux bitrate is chosen from compressed bytes counted during patch decode — default `mux_audio_bitrate = "match_min"` uses the lower of A and B measured rates so output is not upsampled above either source. Use `"default"` to omit `-b:a` and let ffmpeg pick (~128 kb/s stereo); use `"256k"` (or `match_a`) to override.

**Sample output (repair, after patch):**

```text
Alignment: offset +3.000s  confidence 0.98
  Start clip: +3.000s  (confidence 0.98)
  End clip: +3.000s  (confidence 1.00)
Tracks:    A 1ch @ 44100Hz   B 1ch @ 44100Hz   (identical)
Overlap:   A [0.00s – 60.00s]   B [3.00s – 63.00s]   (60.0s shared)

Gaps in video A (1 found, 1 repaired, 0 skipped, 0 unfillable):

  #   Range                Dur      Status
  1   0:30 – 1:00          30.0s    patched (0.98→0.98)

Output: patched.wav
```

Progress stages (`Aligning audio fingerprints…`, `Scanning video A for gaps…`, etc.) print on **stderr**; the report above is on **stdout** and is safe to pipe or redirect.

#### Gap patching pipeline

When write mode runs (`--wav` / `--mux`), each fillable gap goes through structure matching on B, waveform placement (`fill_mode`), and optional A-boundary extension before splice. **Full pipeline overview (all phases):** [docs/pipeline.md](docs/pipeline.md). **Classifying gaps and choosing profiles:** [docs/gap-repair-guide.md](docs/gap-repair-guide.md). **Flag interactions and performance:** [docs/gap-fill-modes.md](docs/gap-fill-modes.md). **How seams are scored:** [docs/seam-scoring.md](docs/seam-scoring.md). Report layout: [docs/cli-output.md](docs/cli-output.md) § Repair gap outcomes.

**1. Per-gap B timeline (`fill_offset_mode`)**

| Mode | Behavior |
|------|----------|
| `recommended` (default) | Every gap uses `recommended_offset_secs` from alignment (fusion or start preference when clip offsets disagree — not always the raw start-clip offset). |
| `interpolated` | Linearly interpolates between start-clip and end-clip offsets by each gap's position on A. Use when alignment drift is significant (`end − start` offset differs by more than ~50 ms). CLI: `--fill-offset interpolated`. |
| `anchored_retry` | **Two-pass:** pass 1 patches all gaps with clip-based offset (same as `interpolated` when start/end clips exist); pass 2 retries seam failures using offset anchors from high-confidence pass-1 successes. Use on drift-heavy long-form pairs when `interpolated` still leaves hard gaps near the search-window edge. CLI: `--fill-offset anchored-retry`. Orthogonal to `fill_mode` (`fit` / `gate`). See [gap-fill-modes.md](docs/gap-fill-modes.md) § Patch anchors. |
| `anchored` | Config/CLI value reserved for future single-pass sequential mode. Today behaves like clip offset only (no live anchor table). Prefer **`anchored_retry`**. |

**2. Structure match** (always runs)

Active/silent signatures around the gap locate the matching dropout on B. Both structure seam scores must pass `min_structure_match_score` (default `0.55`). Short gaps (≤ `short_gap_mean_correlation_secs`, default `2.0` s) may pass on **mean** structure score instead of both individually.

This step finds *where* to splice B; it is **not** turned off by `--no-structure-trust`.

**3. Waveform seam placement (`fill_mode`)**

| Mode | Behavior |
|------|----------|
| `fit` (default) | Unified structure+waveform search on B; tier **High** / **Marginal** / skip. Optional **joint A-boundary grid** when extension flags are on. |
| `gate` | Score waveform Pearson at the structure winner; structure-trust skip and short-gap shortcuts apply. CLI: `--fill-mode gate`. |

**CLI compatibility with `--fill-mode fit`** (default since Phase A)

All flags are accepted with `fit`; none are rejected. Gate-only options have no effect on waveform placement because `fit` always runs the slide search and requires `min(pre, post) ≥ min_fill_correlation` on the best candidate.

| Flag / config | With `--fill-mode fit` |
|---------------|------------------------|
| `--no-structure-trust` | **No extra effect** — `fit` already disables structure-trust waveform skip and soften. Use with `--fill-mode gate` only. |
| `--no-short-gap-one-strong-seam` | **No effect** — one-strong-seam is a `gate` shortcut only (`--fill-mode gate`). |
| `strong_structure_trust`, `partial_structure_waveform_soften` (config) | **No effect on waveform** — structure match still runs; waveform is never skipped for trust. |
| `--min-fill-correlation` | **Active** — `min(pre, post)` floor; **High** vs **Marginal** tier (`fill_marginal_margin`). |
| `--max-fill-align-adjust-secs` | Legacy polish window only — use **`--fill-border-search-secs`** for B slide radius. |
| `--fill-border-search-secs` | **Active** — B slide radius and part of per-gap extract window. |
| `--fill-align-margin-secs`, `--gap-signature-context-secs`, `--fill-length-slack-secs` | **Active** — B haystack extract padding. |
| `--fill-offset interpolated` | **Active** — per-gap B map before local search. |
| `--fill-offset anchored-retry` | **Active** — two-pass offset map; pass 2 retries failures using patch anchors. |
| `--fill-fit-structure-weight`, `--fill-fit-waveform-weight` | **Active** — unified scorer weights. |
| `--border-standoff-secs` | **Active** |
| `--no-gap-end-extend`, `--no-gap-start-extend` | **Active** — disable **joint grid** (not gate mode). See [gap-fill-modes.md](docs/gap-fill-modes.md). |
| `--gap-end-extend-max-ms`, `--gap-end-extend-step-ms` | **Active** — grid span/step in fit; retry span/step in gate. |
| `--crossfade-ms`, `--no-normalize` | **Active** |
| Align / scan flags (`--clip-length`, `--num-clips`, high-rate, query-reference, etc.) | **Active** — feed alignment and gap scan; orthogonal to `fit`. |

**Example — drift + anchored retry** (when `interpolated` still skips hard gaps):

```powershell
clip-sync-repair recording_with_gaps.mp4 reference.mkv `
  --mux repaired.mp4 `
  --fill-offset anchored-retry `
  --min-fill-correlation 0.35 `
  -v
```

(`--fill-mode fit` is the default; use `--quick` for draft muxes or `--full` when gaps need the boundary grid.)

**Repair profiles** (`default` / `quick` / `full`) bundle haystack size, extension flags, and whether fit mode runs the joint A-boundary grid. The default profile accepts marginal baseline placements and skips the grid (minutes per gap instead of tens of minutes on long-form material).

```powershell
# Interactive default (marginal baseline OK; no boundary grid)
clip-sync-repair recording_with_gaps.mp4 reference.mkv --mux repaired.mp4 -v

# Draft / first listen (smaller haystack)
clip-sync-repair recording_with_gaps.mp4 reference.mkv --mux draft.mp4 --quick -v

# Quality pass when default placement is wrong (full boundary grid)
clip-sync-repair recording_with_gaps.mp4 reference.mkv --mux best.mp4 --full -v
```

Profiles do **not** enable `anchored_retry`. On drift-heavy pairs, add `--fill-offset anchored-retry` when needed (often together with `--full` on remaining hard gaps). See [gap-repair-guide.md](docs/gap-repair-guide.md).

**Example — drift + fit (e.g. long pairs with ~1 s clip drift):**

```powershell
clip-sync-repair recording_with_gaps.mp4 reference.mkv `
  --mux repaired.mp4 `
  --fill-offset interpolated `
  --min-fill-correlation 0.35 `
  --fill-border-search-secs 5 `
  -v
```

(`--fill-mode fit` is the default; use `--fill-mode gate` for legacy behavior.)

**4. Waveform seam gate** (when `fill_mode = gate`, optional shortcut by default)

Two layers — do not confuse them:

| Layer | What it checks | `--no-structure-trust` effect |
|-------|----------------|----------------------------------|
| **Structure match** (step 2) | Active/silent pattern on B | Unchanged — still required |
| **Waveform Pearson** (step 4) | Real audio at pre/post borders after alignment | Always runs in `fit` mode; in `gate` mode may be skipped when structure-trusted |

**Default (structure trust on):** when structure pre **and** post both meet `strong_structure_trust` (default `0.90`), the waveform gate is **skipped** and the gap is patched as `structure_trusted`. When structure is weaker but still passes step 2, the waveform gate runs; scores ≥ `partial_structure_waveform_soften` (default `0.85`) soften the threshold to `min(min_fill_correlation, 0.12)`.

**With `--no-structure-trust`:** waveform Pearson always runs at your full `min_fill_correlation` (no skip, no soften). Short-gap **mean** and **one-strong-seam** shortcuts are also disabled — **both** pre and post waveform seams must pass individually.

Other seam knobs are separate: `--no-gap-end-extend`, `--no-short-gap-one-strong-seam`, `short_gap_mean_correlation_secs`, etc.

| Waveform rule | Applies when |
|------|----------------|
| Both seams ≥ `min_fill_correlation` | Long gaps, or any gap with `--no-structure-trust` |
| Mean(pre, post) ≥ threshold | Short gap only (default ≤ 2 s), structure trust on |
| **One strong seam** (either ≥ threshold) | Short gap, after mean fails; on by default (`short_gap_one_strong_seam_fallback`) |

Set `min_fill_correlation = -1.0` in config to disable the waveform gate entirely (structure gate still applies).

**5. A-boundary extension (`gap_end_extend_*`)**

Extension flags control **A gap edge movement** during patch planning. They do **not** switch `fit` ↔ `gate` — use `--fill-mode gate` for legacy behavior. Full matrix: [docs/gap-fill-modes.md](docs/gap-fill-modes.md).

**`fill_mode = fit` (default):** when `gap_end_extend_on_post_seam_fail` / `gap_start_extend_on_pre_seam_fail` are on (defaults), and the baseline bracket is not **High**, run a **joint grid** over earlier start × later end within `gap_end_extend_max_ms` / `gap_end_extend_step_ms` (~12 steps per axis). Each cell re-runs unified B placement. `--no-gap-end-extend` / `--no-gap-start-extend` disable that grid (baseline only).

**`fill_mode = gate`:** on waveform failure only, **sequential retries** — extend gap end, then shift gap start earlier — using the same ms limits. Order and candidate rules differ from fit; see [docs/cli-output.md](docs/cli-output.md).

| Setting | CLI off switch |
|---------|----------------|
| Post-end | `gap_end_extend_on_post_seam_fail` (default on); `--no-gap-end-extend` |
| Pre-start | `gap_start_extend_on_pre_seam_fail` (default on); `--no-gap-start-extend` |

**6. Skip reasons in the report**

| Status | Meaning |
|--------|---------|
| `skipped: boundary correlation below threshold (pre=… post=… min=…)` | Waveform floor failed after retries (`fit` or `gate`) |
| `skipped: boundary alignment failed` | Structure match could not bracket the gap on B |
| `skipped: structure below threshold` | Active/silent pattern scores too low |
| `unfillable` | No B coverage at mapped position (overlap, track mismatch, outside query region) |

Verbose mode (`-v`) adds per-gap patch lines on stderr and slide/pre/post detail in the gap table.

**Example — legacy gate mode (strict thresholds, no waveform slide search):**

Requires `--fill-mode gate`. Use when reproducing pre-fit behavior or tuning structure-trust / one-strong-seam shortcuts.

```powershell
clip-sync-repair recording_with_gaps.mkv reference.mkv `
  --mux repaired.mp4 `
  --fill-mode gate `
  --fill-offset interpolated `
  --no-structure-trust `
  --min-fill-correlation 0.5 `
  --no-gap-end-extend `
  --no-short-gap-one-strong-seam `
  -v
```

**CLI vs config-only**

| CLI flag | Config key |
|----------|------------|
| `--no-structure-trust` | `disable_structure_trust = true` (gate only) |
| `--min-fill-correlation` | `min_fill_correlation` |
| `--max-fill-align-adjust-secs` | `max_fill_align_adjustment_secs` |
| `--border-standoff-secs` | `border_standoff_secs` |
| `--fill-border-search-secs` | `fill_border_search_secs` |
| `--fill-align-margin-secs` | `fill_align_margin_secs` |
| `--gap-signature-context-secs` | `gap_signature_context_secs` |
| `--gap-signature-mode` | `gap_signature_mode` |
| `--fill-length-slack-secs` | `fill_length_slack_secs` |
| `--fill-offset` | `fill_offset_mode` |
| `--fill-mode` | `fill_mode` |
| `--fill-fit-structure-weight` | `fill_fit_structure_weight` |
| `--fill-fit-waveform-weight` | `fill_fit_waveform_weight` |
| `--no-gap-end-extend` | `gap_end_extend_on_post_seam_fail = false` |
| `--no-gap-start-extend` | `gap_start_extend_on_pre_seam_fail = false` |
| `--no-short-gap-one-strong-seam` | `short_gap_one_strong_seam_fallback = false` (gate only) |
| `--gap-end-extend-max-ms` / `--gap-end-extend-step-ms` | `gap_end_extend_max_ms` / `gap_end_extend_step_ms` |

Config-only (no CLI): `short_gap_mean_correlation_secs`, `fill_seam_search_secs`, `gap_signature_bin_ms`, `min_structure_match_score`, `min_border_discovery_secs`, `fill_marginal_margin`, `fill_absolute_floor`, normalization settings — see repair config example below. Config-only **gate** knobs (ignored when `fill_mode = "fit"`): `strong_structure_trust`, `partial_structure_waveform_soften`, `short_gap_one_strong_seam_fallback`.

---

## Configuration

Settings are merged in this order (later wins): built-in defaults → config file → CLI flags.

The config file is TOML. Pass it with `--config` (or `-c`); if omitted, built-in defaults are used.

In TOML, `clip_length` is an integer number of **seconds** (CLI accepts human-friendly values like `15m` or `90s`).

### Output streams

| Stream | Content |
|--------|---------|
| **stdout** | Human alignment/gap report or JSON — safe to pipe (`--format json`) |
| **stderr** | Progress stages, percentage bars, `tracing` logs, errors |
| **exit code** | `0` on successful analysis; non-zero on config/media/alignment failures |

### Progress and verbosity

Three tiers control how much appears on stderr during a run:

| Tier | Flags / config | stderr | stdout (human) |
|------|----------------|--------|----------------|
| **Default** | (none) | Major stages only (`clip-sync: aligning…`, `Searching for match…`, `Scanning video A for gaps…`, …); `%` bars on a TTY | Full report once at end |
| **`--verbose`** | `-v` / `[logging].progress = "verbose"` | Default stages **plus** track selection, clip plans, per-clip offsets, mid-run alignment summary, per-gap patch lines | Extra diagnostics (e.g. decode skips, detailed patch metrics) |
| **`--quiet`** | `-q` / `[logging].progress = "quiet"` | Errors only | Unchanged |

CLI flags override TOML: `-v` sets `progress = verbose`; `-q` sets `progress = quiet` (and wins if both are passed — last flag in the merge order).

`clip-sync` also sets `[output].show_diagnostics = true` when `-v` is passed. `clip-sync-repair` uses `-v` for progress and for detailed gap status lines in the human report.

For scripting, prefer `--format json --quiet` and parse stdout. The JSON contract is documented field-by-field in [docs/json-output.md](docs/json-output.md) (frozen, v1). Implementer contract for progress and human output: [docs/cli-output.md](docs/cli-output.md).

### Logging (`tracing`)

| Source | Precedence |
|--------|------------|
| `RUST_LOG` environment variable | Highest — overrides `[logging].level` and `--log-level` when set |
| `--log-level` / `[logging].level` | Used when `RUST_LOG` is unset; default filter is `clip_sync=<level>,clip_sync_repair=<level>,warn` so third-party crates (e.g. symphonia) log at `warn` unless you raise them in `RUST_LOG` |
| `--log-file` / `[logging].log_file` | Appends structured logs to a file; stderr logging continues |

Operational detail (structure-match trust, ffmpeg mux args, symphonia demuxer) is at **debug** level. Skipped gap fills and similar problems use **warn**.

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
high_rate_recommended_refusion = true       # re-fuse recommended after dual-anchor high-rate
constrain_end_clip_to_start_offset = true   # PCM-search end around start when Chromaprint disagree
end_clip_refine_radius_secs = 5.0
try_all_tracks = false

[output]
format = "human"
show_diagnostics = false

[logging]
level = "warn"
progress = "auto"   # auto | verbose | quiet — overridden by -v / -q
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
high_rate_recommended_refusion = true
constrain_end_clip_to_start_offset = true
end_clip_refine_radius_secs = 5.0
require_consistent_offsets = false
try_all_tracks = false

[logging]
level = "warn"
progress = "auto"   # auto | verbose | quiet — overridden by -v / -q

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
disable_structure_trust = false   # gate only: true or --no-structure-trust
fill_align_margin_secs = 1.0
max_fill_align_adjustment_secs = 0.5
fill_border_search_secs = 10.0      # fit: B slide radius (default 10; CLI: --fill-border-search-secs)
min_border_discovery_secs = 2.0
border_standoff_secs = 0.35
short_gap_mean_correlation_secs = 2.0
fill_length_slack_secs = 5.0
fill_seam_search_secs = 0.25
gap_signature_context_secs = 3.0
gap_signature_bin_ms = 50
min_structure_match_score = 0.55
strong_structure_trust = 0.90              # gate only: skip waveform when both ≥ this
partial_structure_waveform_soften = 0.85   # gate only: soften waveform floor
fill_offset_mode = "recommended"   # recommended | interpolated | anchored_retry; CLI: --fill-offset
fill_anchor_min_correlation = 0.35           # anchored_retry: min(pre, post) for anchor eligibility
fill_anchor_exclude_structure_trusted = true # anchored_retry: gate patches without waveform
fill_anchor_max_adjustment_frac = 0.9        # anchored_retry: max |slide| / fill_border_search_secs
fill_anchor_search_prior_weight = 0.0        # fit + anchors: unified-search prior (0 = off)
fill_anchor_retry_marginal = false           # anchored_retry pass 2: re-run fit marginal pass-1 gaps; keep pass 2 only when High
gap_signature_mode = "auto"                  # auto (default) | bool | energy (fit path structure tier)
fill_mode = "fit"                  # default; or "gate" for legacy; CLI: --fill-mode
fill_fit_structure_weight = 0.35   # fit only: unified search
fill_fit_waveform_weight = 0.65    # fit only: unified search
fill_fit_energy_nominal_bias_scale = 0.25  # fit only: distance-from-nominal penalty scale for energy-resolved gaps (< base 1.0 lets a confident energy contour override a drifted nominal map)
fill_marginal_margin = 0.08          # fit only: warn-patch band
fill_absolute_floor = 0.12           # fit only: hard skip floor
gap_end_extend_on_post_seam_fail = true
gap_start_extend_on_pre_seam_fail = true
gap_end_extend_max_ms = 500
gap_end_extend_step_ms = 20
short_gap_one_strong_seam_fallback = true   # gate only
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
mux_audio_bitrate = "match_min"   # match_min | match_a | default | 256k (mux only)
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

**Symmetric multi-clip (`num_clips ≥ 2`):** end and interior windows default to **shared timeline** anchoring — both files sample the same clock times up to the shorter file’s effective end (`end_clip_anchor = shared_timeline` in `[alignment]`; legacy per-file tails: `end_clip_anchor = "file_tail"`). JSON reports include `end_clip_anchor` when applicable.

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

See **[docs/development.md](docs/development.md)** for Cargo features per crate, CI vs full test matrix, ignored/slow tiers, and fixture regeneration.

Quick start:

```powershell
cargo build
cargo test --workspace
```

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

## License

Copyright (c) 2026 Matt Groeninger.

This project is licensed under the [PolyForm Noncommercial License 1.0.0](LICENSE). You may use, modify, and distribute the software for **noncommercial purposes** — personal projects, research, education, and use by nonprofit or government organizations. **Commercial use** (including use inside products or services sold for profit) is not permitted without a separate commercial license.

Third-party dependency licenses are summarized in [THIRD_PARTY_LICENSES.txt](THIRD_PARTY_LICENSES.txt). Notable copyleft: the Symphonia audio stack is under [MPL-2.0](https://opensource.org/licenses/MPL-2.0). Optional test audio fixtures are documented separately in [tests/corpus/THIRD_PARTY_AUDIO.md](tests/corpus/THIRD_PARTY_AUDIO.md).

---

## Documentation

- [docs/gap-repair-guide.md](docs/gap-repair-guide.md) — **gap types**, waveform tiers, and repair recommendations
- [docs/gap-fill-modes.md](docs/gap-fill-modes.md) — **`fit` vs `gate`**, flag matrix, extension semantics, performance recipes
- [docs/development.md](docs/development.md) — features, build, full test suite
- [docs/cli-output.md](docs/cli-output.md) — progress tiers, human report layout, gap patch outcomes, timeline/duration warnings, mux failures
- [docs/json-output.md](docs/json-output.md) — JSON output contract (v1) for both CLIs
- [docs/error-mapping.md](docs/error-mapping.md) — exit codes, user messages, error hierarchy
- [docs/corpus-validation.md](docs/corpus-validation.md) — test tiers, CI commands, known offsets
- [docs/corpus-matrix.md](docs/corpus-matrix.md) — case design matrix
- [PLAN.md](PLAN.md) — full architecture reference
- [BACKLOG.md](BACKLOG.md) — deferred work items
