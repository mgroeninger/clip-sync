# Temporary plan: CLI output UX polish

> **Status:** Not started. Archive to `docs/archive/cli-output-ux-plan.md` when shipped.

**Problem:** A full `clip-sync-repair --mux` run emits ~60 lines mixing progress phases, progress percentages, third-party `tracing` INFO logs, and a final structured report. Information is duplicated (alignment mid-run vs final report; gap scan vs patch results), library noise dominates stderr, and non-TTY runs glue progress lines to timestamped log lines (`patch-a: 99%2026-06-09…`).

**Goal:** Three-tier output — **default** (stages on stderr + one report on stdout), **`--verbose`** (today's detail for debugging), **`--quiet`** / **`--format json`** (scripting). No loss of application-specific semantics; relocate internals to debug/warn and JSON.

**Scope:** `crates/clip-sync` (progress + align phases + tracing defaults), `crates/clip-sync-cli` (align-only output), `crates/clip-sync-repair` (scan/patch/mux output). Both CLIs already expose `--verbose`, `--quiet`, `--log-level`, `--log-file`.

---

## Current output architecture

### Four channels (plus stdout report)

| Channel | Mechanism | Destination | Default today |
|---------|-----------|-------------|---------------|
| Phase lines | `ProgressReporter::phase()` | stderr | Always on (unless `--quiet`) |
| Progress % | `ProgressReporter::progress()` | stderr | TTY or `--verbose` (`ProgressMode::Auto`) |
| `tracing` (deps) | symphonia demuxer, etc. | stderr | **INFO** — very noisy |
| `tracing` (app) | patch_audio, ffmpeg_mux | stderr | INFO for operational detail |
| Final report | `print!` / `println!` | stdout | End of run |

### Key files

| Area | Path |
|------|------|
| Progress reporter | `crates/clip-sync/src/infrastructure/logging/progress.rs` |
| Progress mode enum | `crates/clip-sync/src/infrastructure/logging/mod.rs` |
| Tracing init | `crates/clip-sync/src/infrastructure/logging/mod.rs` (`init_tracing`) |
| Align phases + mid-run summary | `crates/clip-sync/src/application/align_videos.rs` (`log_alignment_summary`, `extract_clips`, etc.) |
| High-rate refine phase | `crates/clip-sync/src/application/high_rate_refinement.rs` |
| CLI verbose wiring (align) | `crates/clip-sync-cli/src/infrastructure/cli/mod.rs` |
| Align human report | `crates/clip-sync-cli/src/infrastructure/cli/output.rs` |
| Repair human report | `crates/clip-sync-repair/src/infrastructure/cli/output.rs` |
| Patch phases + tracing | `crates/clip-sync-repair/src/application/patch_audio.rs` |
| Gap scan progress labels | `crates/clip-sync-repair/src/application/scan_gaps.rs` |
| Mux phases + tracing | `crates/clip-sync-repair/src/infrastructure/ffmpeg_mux.rs` |
| Repair CLI verbose wiring | `crates/clip-sync-repair/src/infrastructure/cli/mod.rs` |

### Existing flags (no new flag required)

| Flag | clip-sync-cli | clip-sync-repair |
|------|---------------|------------------|
| `-v` / `--verbose` | `show_diagnostics=true` + `ProgressMode::Verbose` | `ProgressMode::Verbose` only (no diagnostics parity) |
| `-q` / `--quiet` | `ProgressMode::Quiet` | `ProgressMode::Quiet` |
| `--log-level` | overrides `LoggingConfig.level` | same |
| `--format json` | full `AlignmentResult` on stdout | `RepairJsonOutput { scan, patch }` on stdout |

**Gap:** Most chatter is `phase()`, which ignores `Auto` vs `Verbose`. `-v` mainly affects progress bars and clip-sync-cli diagnostics, not repair phase volume.

---

## Target output model

### Tier 1 — Default (no flags)

**stderr — coarse stages only (~5–8 lines for a mux run):**

```text
clip-sync-repair: aligning 1.mkv with 2.mkv
Aligning audio fingerprints...
Scanning video A for gaps...
Repairing 3 gap(s)...
Muxing → Shaun of the Dead.mp4
```

- One progress indicator per long stage (TTY: `\r` bar; non-TTY: sparse % or silent between stages).
- No per-clip extraction lines, symphonia logs, or per-gap patch lines.

**stdout — single structured report:**

```text
Alignment: offset -10.971s  confidence 0.94
  Start clip: -10.956s  (0.94)
  End clip:   -11.005s  (0.94)
Tracks:    A 6ch @ 48000Hz   B 6ch @ 48000Hz   (identical)
Overlap:   A [10.97s – 900.00s]   B [0.00s – 889.03s]   (889.0s shared)

Gaps in video A (5 found, 3 repaired):

  #   Range                Dur      Status
  1   0:00 – 0:16          16.2s    unfillable
  2   19:43 – 19:44        1.0s     patched (struct 0.98→1.00, slide +0.000s)
  3   24:44 – 24:45        1.0s     patched (struct 0.92→1.00, slide +0.000s)
  4   1:21:13 – 1:21:15    2.2s     patched (struct 0.92→1.00, slide +0.000s)
  5   1:39:34 – 1:43:00    205.5s   unfillable

Output: Shaun of the Dead.mp4
```

Optional one-line stderr capstone after mux (or last stdout line):

```text
Wrote Shaun of the Dead.mp4 (3 gaps patched, offset -10.971s)
```

### Tier 2 — `--verbose`

Expose **today's** detail (the sample mux log the user provided). Includes:

- `Opening media`
- `Selected track N (Hz, channels, decodable)` — once per open, not per re-demux
- `Clip plan for video A/B: …`
- `Extracting clip i/n …` progress
- `Searching for match…` + per-clip offset lines
- Mid-run alignment summary (`Start clip aligned`, `Recommended offset`, overlap windows)
- `High-rate refinement` with adjustment; raw correlation peak only here (or still debug)
- `scan-a` / `scan-b`, `patch-a` / `patch-b` labeled progress
- Per-gap lines: `gap 1/3: A [1183.5s – 1184.5s]`
- Decode-skip annotations (clip-sync-cli `show_diagnostics`; add repair parity)
- Repetition diagnostics (`video A: no internal repeat detected`, etc.)

### Tier 3 — `--log-level debug` / `RUST_LOG`

Engineer diagnostics only. Not shown in default or verbose human output.

### Tier — WARN (surfaced in report or stderr, not hidden in debug)

Real problems or degraded results:

- Gap fill skipped (correlation below threshold) — already `tracing::warn!` in `patch_audio.rs`
- Cross-check MISMATCH — already in repair human output
- Clip offsets disagree / significant offset drift — promote in final report; optional warn trace
- Non-zero decode skips — warn or verbose-only depending on frequency in corpus

---

## Per-message disposition table

| Message (representative) | Default stderr | `--verbose` | Log level |
|--------------------------|----------------|-------------|-----------|
| `clip-sync: aligning A with B` | stage | — | — |
| `Opening media` | no | yes | debug |
| symphonia `stream is seekable` | no | no | suppress (warn filter) |
| `Selected track …` | no | yes (dedupe re-opens) | — |
| `Clip plan for video A/B` | no | yes | — |
| `Extracting clip i/n` | progress bar only | yes + label | — |
| `Searching for match…` | stage | — | — |
| Per-clip offset line | no | yes | — |
| `Start/End clip aligned`, `Recommended offset`, overlap (mid-run) | **no** (final report only) | yes | — |
| `High-rate refinement: +Xs adjustment` | one line under alignment in report if applied | full line | peak → debug |
| `scan-a` / `scan-b` / `patch-a` / `patch-b` | TTY bar only | labeled % | — |
| `Aligning N fill region(s)…` | stage | — | — |
| `gap i/n: A […]` | no | yes | — |
| `trusting structure match (skipping waveform seam gate)` | no | no | **debug** |
| `B fill longer/shorter than gap` | no | no | debug |
| `muxing video with patched audio via ffmpeg` | no | no | debug |
| `Muxing video with patched audio…` | stage | — | — |
| `mux: N%` | progress | progress | — |
| Final Alignment / Tracks / Overlap / Gaps | stdout | stdout | — |
| Dual gap + patch tables | **merged single table** | merged (more columns ok) | — |

---

## Structural changes

### 1. Phase gating in `ProgressReporter`

Extend the port or reporter with two entry points:

```rust
// Option A — two methods on trait
fn phase(&self, message: &str);           // major stages only in Auto
fn phase_verbose(&self, message: &str);   // detail; no-op unless Verbose

// Option B — explicit detail flag on phase
fn phase_detail(&self, message: &str, detail: PhaseDetail);
enum PhaseDetail { Major, VerboseOnly }
```

**Auto mode:** `phase()` emits only messages tagged `Major`. **Verbose mode:** both `Major` and `VerboseOnly`.

Migrate existing call sites:

- **Major:** startup banner, `Searching for match…`, scan stage, `Aligning N fill region(s)…`, `Splicing…`, `Muxing…`
- **VerboseOnly:** everything else currently in `phase()`

`--quiet` continues to suppress all phases and progress.

### 2. Remove mid-run duplication

`log_alignment_summary()` in `align_videos.rs` emits alignment, overlap, and high-rate lines via `progress.phase()`. In **Auto** mode, skip this function entirely (or gate all lines to `phase_verbose`). The repair final report (`format_human` in repair `output.rs`) already carries:

- offset + confidence
- per-clip offsets when len > 1
- drift when clips disagree
- tracks, overlap, cross-check

Align-only CLI (`clip-sync-cli/output.rs`) already prints a complete `Alignment report` at end — same rule: no mid-run summary in Auto.

### 3. Merge gap scan + patch results (repair stdout)

Today:

1. `Gaps detected in video A (N total, M repairable):` + per-gap scan status
2. `Patch results (X patched, Y skipped, Z not planned):` + per-gap patch status

**Target:** One section, one row per gap:

| Column | Source |
|--------|--------|
| `#` | index |
| `Range` | `video_a_start_secs` – `video_a_end_secs` (human `H:MM:SS`) |
| `Dur` | duration |
| `Status` | unified: `unfillable`, `blocked (track layout)`, `patched (…)`, `skipped: …`, `not planned: …` |

Implementation:

- Add `format_unified_gap_report(report, patch_summary) -> String` in repair `output.rs`
- Dry-run / scan-only (no patch): status from scan labels only
- After patch: merge `PatchSummary` outcomes; scan-only gaps without patch entry keep scan status

Keep summary header: `Gaps in video A (5 found, 3 repaired, 0 skipped, 2 unfillable)` — counts from merged rows.

JSON: optional `unified_gaps` array on `RepairJsonOutput` for consumers; keep `scan` + `patch` for backward compatibility or version in a follow-up.

### 4. Tracing defaults and log tiering

**Default filter** in `init_tracing` (when `RUST_LOG` unset):

```text
clip_sync=info,clip_sync_repair=info,symphonia=warn,warn
```

Or root `warn` with `clip_sync=info,clip_sync_repair=info` — evaluate which is quieter in practice.

**Downgrade to `debug`:**

| Location | Current message |
|----------|-----------------|
| `patch_audio.rs` ~L554 | `trusting structure match (skipping waveform seam gate)` |
| `patch_audio.rs` ~L644, ~L651 | B fill extend/trim |
| `ffmpeg_mux.rs` ~L211 | `muxing video with patched audio via ffmpeg` |

**Keep `warn`:** skip-gap messages (correlation, out of range, structure below threshold).

### 5. Progress / tracing collision fix

Non-TTY: `progress()` writes `label: N%` without clearing a partial line; symphonia INFO can append on the same line.

Fixes (do both):

1. Suppress symphonia INFO (filter above).
2. In `progress.rs`, before any `phase()` write, call `finish_progress_line()`. Consider emitting progress on non-TTY only on stage boundaries or every 10% to reduce line noise.

### 6. Timestamp format consistency

Human output today mixes:

- `H:MM:SS` in align progress overlap
- `1183.50s` in repair gap tables
- seconds with decimals in overlap line

**Standardize human:** `H:MM:SS` or `M:SS` for ranges; keep raw seconds in JSON only. Reuse `format_timestamp` from `clip-sync-cli/output.rs` or move to a shared `clip_sync::format` helper used by repair output.

### 7. High-rate refinement peak display

`peak 2813101397.00` is raw correlation energy — not meaningful to users.

- **Default report:** `High-rate refinement: +0.010s` (omit peak)
- **Verbose:** optional `peak` as debug field or normalized metric if we add one later
- **JSON:** keep full `HighRateRefinement` struct unchanged

### 8. Repair `--verbose` parity with clip-sync-cli

When `args.verbose` in repair `mod.rs`:

- Set `ProgressMode::Verbose` (already done)
- Add `show_diagnostics: bool` to repair output config or pass flag into `format_human` / `format_patch_summary` for:
  - decode skip counts
  - structure-trust vs waveform-gate distinction in patch detail
  - cross-check always shown; extra diagnostic lines only when verbose

### 9. stdout vs stderr contract (document)

| Stream | Content |
|--------|---------|
| **stdout** | Human report or JSON result — safe to pipe |
| **stderr** | Progress, stages, warnings, tracing |
| **exit code** | Unchanged; errors on stderr |

Update `README.md` when shipped.

---

## Example: default mux run (after polish)

**stderr:**

```text
clip-sync-repair: aligning .\1.mkv with .\2.mkv
Aligning audio fingerprints... ████████████████ 100%
Scanning video A for gaps...   ████████████████ 100%
Repairing 3 gap(s)...          ████████████████ 100%
Muxing → Shaun of the Dead.mp4 ████████████████ 100%
```

**stdout:**

```text
Alignment: offset -10.971s  confidence 0.94
  Start clip: -10.956s  (0.94)
  End clip:   -11.005s  (0.94)
Tracks:    A 6ch @ 48000Hz   B 6ch @ 48000Hz   (identical)
Overlap:   A [10.97s – 900.00s]   B [0.00s – 889.03s]   (889.0s shared)
High-rate: +0.010s refinement applied

Gaps in video A (5 found, 3 repaired):

  #   Range              Dur      Status
  1   0:00 – 0:16        16.2s    unfillable
  2   19:43 – 19:44      1.0s     patched (struct 0.98→1.00)
  3   24:44 – 24:45      1.0s     patched (struct 0.92→1.00)
  4   1:21:13 – 1:21:16  2.2s     patched (struct 0.92→1.00)
  5   1:39:34 – 1:43:00  205.5s   unfillable

Output: Shaun of the Dead.mp4
```

---

## Implementation phases

### Phase 1 — Quick wins (low risk)

1. Default tracing filter: `symphonia=warn` (+ document `RUST_LOG` override).
2. Downgrade patch_audio / ffmpeg_mux operational `info!` → `debug!`.
3. `finish_progress_line()` before every `phase()` write (collision fix).
4. Omit correlation peak from default human high-rate line (align progress + clip-sync-cli output).

**Tests:** unit tests for filter string; existing integration tests should see fewer stderr lines (snapshot tests if any).

### Phase 2 — Phase gating

1. Add `phase_verbose` (or `PhaseDetail`) to `ProgressReporter` trait and `StderrProgressReporter`.
2. Classify all `phase()` call sites in:
   - `align_videos.rs`
   - `high_rate_refinement.rs`
   - `scan_gaps.rs` (if any phases)
   - `patch_audio.rs`
   - `ffmpeg_mux.rs`
3. Skip `log_alignment_summary` in Auto mode.
4. Add major-stage banners: `Aligning audio fingerprints…`, `Scanning video A for gaps…` (repair orchestration in `scan_gaps` / `repair_videos` / CLI `mod.rs`).

**Tests:** `progress.rs` — Auto suppresses verbose phases; Verbose prints them.

### Phase 3 — Unified gap report (repair stdout)

1. Implement `format_unified_gap_report`.
2. Replace separate gap + patch sections in `print_repair_output` / `format_human`.
3. Standardize human timestamps via shared helper.
4. Add `Output: <path>` line when wav/mux written.
5. Wire repair `verbose` → `show_diagnostics` for extra patch columns.

**Tests:** extend `crates/clip-sync-repair/src/infrastructure/cli/output.rs` unit tests; update `cli_mux_integration` expectations if present.

### Phase 4 — Docs and cleanup

1. Update `README.md` and `PLAN.md` logging section to match tier model.
2. Archive this doc to `docs/archive/cli-output-ux-plan.md`.
3. Optional: config TOML `logging.progress = "auto" | "verbose" | "quiet"` already exists — document interaction with `-v`/`-q`.

---

## Non-goals (v1)

- Changing JSON schema breaking fields (additive `unified_gaps` only if needed).
- GUI or structured log formats (JSON logs).
- Progress bars for fingerprinting sub-steps (too fast to matter).
- New `--silent` flag (`--quiet` + `--format json` suffices).

---

## Acceptance criteria

1. Default mux run: ≤10 stderr lines (excluding warn/error), full semantics on stdout.
2. `--verbose` reproduces pre-change phase detail (regression checklist against sample log in this doc's problem statement).
3. No symphonia demuxer lines at default log level.
4. No duplicated alignment block (mid-run + final) in default mode.
5. Single gap table on stdout with correct merged statuses.
6. Non-TTY run: no glued `99%2026-06-09` lines.
7. `cargo test` green; repair + cli output unit tests updated.

---

## Reference: problematic sample log (before)

Captured from user mux run — target for `--verbose` preservation:

```text
clip-sync: aligning .\1.mkv with .\2.mkv
Opening media
2026-06-09T22:17:51.482206Z  INFO symphonia_format_mkv::demuxer: stream is seekable ...
Selected track 2 (48000 Hz, 6 channels, decodable)
Clip plan for video A: 2 clip(s) — [0:00–15:00] start (15:00), [1:27:59–1:42:59] end (15:00)
Extracting clip 2/2 (video A, 15:00): 99%
...
Searching for match...
start clip [0:00–15:00]: offset -10.956s (confidence: 0.94)
...
Recommended offset: -10.971s (clip offsets agree)
Overlap on video A: [0:11–15:00], [1:27:59–1:42:59]
...
scan-a: 100%
patch-a: 99%
Aligning 3 fill region(s) (structure match + splice)...
  gap 1/3: A [1183.5s – 1184.5s]
INFO clip_sync_repair::application::patch_audio: trusting structure match ...
...
Alignment: offset -10.971s  confidence 0.94
Gaps detected in video A (5 total, 3 repairable):
Patch results (3 patched, 0 skipped, 2 not planned):
```
