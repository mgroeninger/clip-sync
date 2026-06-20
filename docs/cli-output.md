# CLI output contract

Normative behavior for human and progress output of **`clip-sync`** (analyzer) and **`clip-sync-repair`** (repair). For JSON field contracts see [json-output.md](json-output.md); for errors, stderr messages, and exit codes see [error-mapping.md](error-mapping.md). User-facing summary: [README.md](../README.md) § Progress and verbosity.

Historical implementation notes: [archive/cli-output-ux-plan.md](archive/cli-output-ux-plan.md).

---

## Principles

1. **stdout is the report** — one structured document per successful run; safe to pipe (`--format json` or human).
2. **stderr is operational** — progress stages, percentage bars, `tracing` logs, and errors. Never mix report content onto stderr in default mode.
3. **No duplication** — alignment and gap outcomes appear once on stdout at end of run, not again as a mid-run stderr summary (unless `--verbose`).
4. **Three tiers** — default (`Auto`), `--verbose`, and `--quiet` share the same semantics in both binaries.
5. **Library noise stays down** — third-party crates log at `warn` unless the user raises them via `RUST_LOG`.

---

## Streams

| Stream | Success | Failure |
|--------|---------|---------|
| **stdout** | Human report or JSON (exit 0) | Empty — no partial report |
| **stderr** | Progress stages, `%` bars (TTY), `tracing` at configured level | `error: …` line (see [error-mapping.md](error-mapping.md)) |
| **exit code** | `0` | Non-zero per error category |

Scripting: prefer `--format json --quiet` and parse stdout only.

---

## Verbosity tiers

Controlled by `[logging].progress` in TOML (`auto` \| `verbose` \| `quiet`) and overridden by CLI flags. If both `-v` and `-q` are passed, the last flag in the CLI merge order wins.

| Tier | Flags / config | stderr | stdout (human) |
|------|----------------|--------|----------------|
| **Default** | (none) / `progress = "auto"` | Major `phase()` lines; TTY `\r` progress bars for long decode/scan/patch/mux steps; non-TTY sparse `%` lines | Full report once at end |
| **`--verbose`** | `-v` / `progress = "verbose"` | Default **plus** `phase_verbose()` detail (track select, clip plans, per-clip offsets, mid-run alignment summary, per-gap patch lines); labeled `%` off-TTY | Extra diagnostics when `show_diagnostics` is true (see below) |
| **`--quiet`** | `-q` / `progress = "quiet"` | Errors only (no phases, no progress bars) | Unchanged |

### `show_diagnostics`

| Binary | Set by | Effect on stdout |
|--------|--------|------------------|
| `clip-sync` | `-v` → `[output].show_diagnostics = true` | Per-clip window lines, decode-skip counts, repetition lines, high-rate peak, offset-verification skip reason, parallel recheck offset on inconclusive verify |
| `clip-sync-repair` | `-v` → `show_diagnostics` argument to `format_human` | Detailed gap patch status (struct pre/post, slide); high-rate peak; offset-verification skip reason |

Both binaries wire `-v` to `ProgressMode::Verbose` **and** enable stdout diagnostics as above.

---

## `ProgressReporter` (shared library port)

Implementation: `crates/clip-sync/src/infrastructure/logging/progress.rs` (`StderrProgressReporter`).

```rust
trait ProgressReporter {
    fn phase(&self, message: &str);          // major stages — Auto + Verbose + Quiet (suppressed)
    fn phase_verbose(&self, message: &str);   // detail — Verbose only
    fn progress(&self, label: &str, current: u64, total: u64);
}
```

### Call-site rules

| Method | When to use | Auto | Verbose | Quiet |
|--------|-------------|------|---------|-------|
| `phase()` | Coarse stage boundaries users should always see | yes | yes | no |
| `phase_verbose()` | Operational detail (open, track select, clip plan, per-clip offset, mid-run summary) | no | yes | no |
| `progress()` | Long-running sub-steps (extract, scan-a/b, patch-a/b, mux) | TTY bar only | TTY bar + off-TTY `%` | no |

**Before every `phase()` / `phase_verbose()` write**, the reporter finishes any active TTY progress line so stderr lines do not glue to `%` output. Gap-fill skip warnings call `flush_progress()` for the same reason before `tracing::warn`.

**Extraction progress:** `detailed_extraction_progress()` returns true only in Verbose mode. Auto mode uses aggregated extraction progress (no per-clip extraction labels on stderr).

### Major stages (default stderr)

| Stage | `clip-sync` | `clip-sync-repair` |
|-------|-------------|-------------------|
| Startup | `clip-sync: aligning <A> with <B>` | `clip-sync-repair: aligning <A> with <B>` |
| Fingerprint align | — | `Aligning audio fingerprints (video A)...` then `(video B)...` (per-video scopes; one 100% bar each) |
| Match | `Searching for match...` | (via shared align path) |
| Scan | — | `Scanning video A for gaps...`; after scan: `Gap scan: N silent run(s) ≥…ms — …`; when some gaps are not repairable, `Gap fill: R of N repairable (M skipped — …)` |
| Patch | — | When the fill plan omits gaps: `Skipping M gap(s) at fill plan (…); aligning R fill region(s) (structure match + splice)...`; otherwise `Aligning R fill region(s) (structure match + splice)...` |
| Splice | — | `Splicing N fill(s) into timeline...` (when N > 0) |
| Mux | — | `Muxing → <output>` or `Muxing video with patched audio...` (when `--mux`) |

Progress labels for decode buckets: `scan-a`, `scan-b`, `patch-a`, `patch-b`, `patch-gap`, `patch-splice`, `mux`. In Auto mode these appear as TTY bars or sparse `%` only — not as separate phase lines.

### Verbose-only phases (`phase_verbose`)

Examples (both tools where applicable):

- `Opening media`
- `Selected track N (Hz, channels, decodable)`
- `Clip plan for video A/B: …`
- `Extracting clip i/n …` (with `%` when verbose)
- Per-clip offset: `start clip [0:00–15:00]: offset +12.340s (confidence: 0.94)`
- Mid-run alignment summary: `Recommended offset: …`, overlap windows, drift
- `High-rate offset refinement...`
- Per-gap patch: `gap i/n: A [t0 – t1]`
- Mux AAC target (repair, when `--mux`): `Mux AAC bitrate 256k (A 256 kbps, B 384 kbps, policy match_min)` — `phase_verbose` only

`log_alignment_summary()` in `align_videos.rs` must use **`phase_verbose` only** — never `phase()` — so default runs do not duplicate the final stdout report.

---

## `tracing` defaults

When `RUST_LOG` is unset, `init_tracing` uses:

```text
clip_sync=<level>,clip_sync_repair=<level>,symphonia_core=error,warn
```

(`<level>` from `[logging].level` or `--log-level`; default `info`.)

| Level | Typical content |
|-------|-----------------|
| **info** (app crates) | Rare in default UX — prefer phases for user-visible steps |
| **warn** | Skipped gap fills (`gap N/M (range): reason`), correlation below threshold, verification failures worth surfacing |
| **debug** | Structure-match trust, B fill trim/extend, ffmpeg mux command detail, mux AAC bitrate (`Mux AAC bitrate …` with A/B bps and policy), symphonia demuxer |
| **Third-party** | `warn` unless raised in `RUST_LOG`; `symphonia_core` defaults to `error` (suppresses MP4 junk-byte probe noise) |

Precedence: `RUST_LOG` > `--log-level` / `[logging].level` > default filter above.

`--log-file` appends structured logs; stderr logging continues.

---

## Human report conventions (stdout)

Shared helpers live in `clip_sync::application::report` (`format_high_rate_refinement_lines`, `format_offset_verification_lines`, `format_time_range`).

### Time format

- **Human ranges:** `H:MM:SS` or `M:SS` via `format_time_range` (gap table, clip windows in verbose).
- **Overlap line:** decimal seconds with fixed precision (`A [10.97s – 900.00s]`).
- **JSON:** always seconds as `f64`; see [json-output.md](json-output.md).

### High-rate refinement (stdout)

| Mode | Line |
|------|------|
| Applied, default | `High-rate: +0.010s refinement applied` (signed `{:+0.3}s`; no raw correlation peak) |
| Applied, verbose | `High-rate: +0.010s refinement applied (peak …)` |
| Skipped | verbose only: `High-rate: skipped (reason)` |

### Offset verification (stdout)

| Outcome | Default | Verbose |
|---------|---------|---------|
| Not verified | warn line always | — |
| Verified | hidden | `Verify: offset confirmed …` |
| Skipped | hidden | `Verify: skipped (reason)` |
| **Inconclusive** (periodic gating) | `Verify: offset not independently verified (periodic content; hold-out confidence …)` | **plus** `parallel recheck offset …` and Δ vs recommended |

**Inconclusive** is distinct from a normal not-verified outcome: Option A scored a pass on the offset-shifted hold-out, but calendar-parallel PCM recheck disagreed (or parallel PCM failed while `offset_ambiguous_mod_secs` was set). JSON: `verify_inconclusive: true`, optional `independent_offset_secs` / `parallel_recheck_delta_secs` — see [json-output.md](json-output.md).

### Periodic offset ambiguity (stdout)

When `offset_ambiguous_mod_secs` is set (strong start-clip repetition under `check_clip_repetition`):

| Mode | Line |
|------|------|
| Default | `Warning: offset ambiguous (repeats every ~N s) — auto offset and verify may match the wrong period` |

Emitted after high-rate refinement lines and before offset-verification lines. The period **N** is the normalized repeat tile from discovery (not necessarily the raw autocorrelation lag). Setting the flag does **not** imply confidence was halved — see [corpus-validation.md](corpus-validation.md) § Periodic offset ambiguity.

### Query-reference mode (stdout)

When `query_localization` is present (`alignment_mode_used: queryreference`), the human report **replaces** the symmetric headline (`Alignment: offset … confidence …`) with placement-first lines from `format_query_localization_lines` in `application/report.rs`. Both `clip-sync` and `clip-sync-repair` use the same helper for the alignment header.

| Scenario | Default (`show_diagnostics = false`) | Verbose (`-v`) |
|----------|--------------------------------------|----------------|
| **A-long** (query on B, reference is A) | `Match on video A: <span>  (<length>, confidence …)` — span is where the short clip sits on A | **plus** `Clip on B: …`, `Offset: …`, `Search: …` |
| **B-longer donor** (query on A, reference is B) | Same line **plus** `(donor on B: <span>)` suffix on the match line | Same verbose block as A-long |
| **Not located** | `Query clip not located (<reason>)` | — |
| **Ambiguous location** | **plus** `Warning:   clip location ambiguous …` | — |

**Layout rules (query mode):**

- No per-clip offset block or symmetric `Overlap:` line on stdout (overlap is in JSON via `start_overlap` / `query_localization`).
- High-rate refinement, periodic ambiguity, and offset-verification lines follow unchanged (same order as symmetric).
- JSON: `query_localization` object with `anchor_ref_secs` (deserialize alias `anchor_a_secs`); see [json-output.md](json-output.md).

**Examples:**

```text
Match on video A: 45:00 – 53:00  (8m, confidence 0.94)
```

B-longer default (short A on long B):

```text
Match on video A: 0:00 – 1:10  (1m 10s, confidence 0.91)  (donor on B: 1:30 – 2:40)
```

Verbose adds (after the match line):

```text
Clip on B:  0:00 – 1:10
Offset:     +90.000s  (add to A to align with B)
Search:     3 window(s) @ 60s stride
```

### Analyzer (`clip-sync`)

- **Symmetric header:** `Alignment: offset …  confidence …` with optional per-clip offset lines when `clips.len() > 1`.
- **Query-reference header:** see [Query-reference mode](#query-reference-mode-stdout) above (no symmetric headline).
- **Verbose-only:** clip window timestamps, decode-skip annotations, repetition diagnostics; overlap line when not in query mode. With `num_clips ≥ 2`, verbose also prints `End anchor: …` and end-clip absolute windows on A and B (B omitted when identical to A).
- **Implementation:** `crates/clip-sync-cli/src/infrastructure/cli/output.rs`

### Repair (`clip-sync-repair`)

- **Alignment header:** query-reference block or symmetric `Alignment: offset …` (see above); symmetric verbose adds drift, track compatibility, overlap, cross-check when applicable.
- **Alignment instability warning (default):** when clip offsets disagree **and** silence cross-check `MISMATCH`, emit one synthesis line after cross-check: `Warning: alignment unstable — fills used start-clip offset; clip drift and silence cross-check disagree (review gap #N …)` listing skipped gap numbers when a patch summary is available. With the default **shared-timeline** end anchor, large `end − start` drift is more likely to reflect real timeline instability (edits, speed change, tail damage) than a spurious file-tail mismatch on unequal-length pairs; the warning still fires when drift and cross-check disagree.

#### Timeline / duration warnings

Up to three `Warning:` lines may appear after the alignment block and before the gap table. Each is also emitted on stderr via `tracing::warn`. Thresholds are fixed in `crates/clip-sync-repair/src/domain/diagnostics.rs` (not configurable today).

| ID | When | Condition | Example stdout line |
|----|------|-----------|---------------------|
| **A1** | Report (symmetric mode only) | `overlap.video_a_start_secs > 1.0` | `Warning: video A shared overlap starts at 5.0s (not 0:00) — gap times are on the decoded-sample clock and may not match ffmpeg/container timestamps; prefer a clean source file or MKV` |
| **A2** | Report + scan | Gap scan on A measures max \|PTS − sample-clock\| `> 1.0` s | `Warning: audio timeline mismatch on video A (PTS 0.0s vs decoded-sample clock 4.9s, Δ 4.9s) — gap positions may be shifted relative to ffmpeg silencedetect` |
| **A3** | Report (write mode, after patch) | `\|patched_pcm_secs − container_secs\| > 2.0` | `Warning: patched audio length 10210.0s differs from container duration 10205.0s by 5.0s — mux may fail or truncate` |

**Why these fire:** gap scan timestamps audio by **sequential decoded-sample count**; patch extract maps samples by **packet PTS**. Sloppy remuxes (e.g. `ffmpeg -ss … -c copy` MKV→MP4) can desync those clocks. A1 is a cheap proxy (alignment overlap not starting at 0:00); A2 is the direct measurement. A3 catches decode length vs declared container duration before mux.

**What to do:** prefer the original MKV; remux with timestamp cleanup (`-avoid_negative_ts make_zero`); validate with `--wav` before `--mux`; compare gap times against `ffmpeg silencedetect` on the same file.

**JSON:** structured skew is available as `scan.audio_timeline_skew` (see [json-output.md](json-output.md)). Human warning lines are not duplicated as a JSON array; A1 is derived from `scan.overlap`, A3 from patch diagnostics (not in JSON today).

#### Mux failures (`--mux`)

| Stage | Condition | Outcome |
|-------|-----------|---------|
| **B1 — preflight** | Before spawning ffmpeg: `\|patched_pcm_secs − video_secs\| > 5.0` (video duration from ffprobe) | **Error** on stderr; exit non-zero; mux not started. Message: `mux error: patched audio (…) and video (…) differ by …s (>5s); use --wav to inspect audio or fix source timestamps` |
| **B3 — stdin / process** | PCM write to ffmpeg stdin fails, or ffmpeg exits non-zero | **Error** on stderr. Stdin failures append trimmed ffmpeg stderr: `mux error: failed to write replacement audio to ffmpeg stdin: …; ffmpeg: …` |

Implementation: `crates/clip-sync-repair/src/infrastructure/ffmpeg_mux.rs` (`validate_mux_duration`, `run_ffmpeg_mux_with_progress`).

- **Gap section:** single unified table — **not** separate scan + patch sections.

```
Gaps in video A (5 found, 3 repaired, 0 skipped, 2 unfillable):
           repaired 12.5s of audio; skipped 231.7s (gap #12 at 1:42:08)
           (> skipped, - unfillable)

  #   Range                Dur      Status
  >12 1:42:08 – 1:46:00    231.7s!  skipped: boundary alignment failed
  -1  0:00 – 0:16          16.2s    unfillable
  2   19:43 – 19:44        1.0s     patched (struct 0.98→1.00)
  …
```

- **Duration summary:** when patching ran, a sub-line under the header totals repaired/skipped seconds and points at the longest skipped gap (`gap #N at H:MM:SS`).
- **Row emphasis:** `>` prefix on skipped gaps, `-` on unfillable; `!` on duration when skipped/unfillable and ≥ 30s. Rows follow timeline order (gap #1, #2, …).

- **Status column:** merged scan + patch outcome (`unfillable`, `blocked (track layout)`, `repairable` [scan-only], `patched (…)`, `skipped: …`, `not planned: …`).
- **Default patch detail:** `patched (struct pre→post)` when structure-trusted; `patched (pre→post)` otherwise.
- **Verbose patch detail:** includes slide adjustment and full pre/post labels.
- **Footer:** `Output: <path>` when WAV or mux file was written.
- **Optional stderr capstone** (not required for compliance): `Wrote <path> (N gaps patched, offset …)` after mux.
- **Implementation:** `crates/clip-sync-repair/src/infrastructure/cli/output.rs` (`format_human`, `format_unified_gap_report`)

Dry-run / scan-only: header uses repairable counts from scan; status from scan labels only.

#### Gap patch gate and skip reasons

Normative detail for patch outcomes; user-facing summary in [README.md](../README.md) § Gap patching pipeline.

**Per-gap pipeline (write mode):**

1. Map gap on A to B (`fill_offset_mode`: `recommended` or `interpolated` drift).
2. Refine gap edges on A; **structure-match** dropout pattern on B (`min_structure_match_score`) — always runs.
3. **Waveform Pearson** at pre/post borders — see structure trust below.
4. On waveform failure, retry with gap-end extension then gap-start extension (when enabled).

**Structure trust vs waveform gate**

Patching uses two independent checks. `--no-structure-trust` affects only the waveform layer.

| Check | Purpose | `--no-structure-trust` |
|-------|---------|------------------------|
| Structure match (step 2) | Locate dropout on B via active/silent signature | **Unchanged** |
| Waveform Pearson (step 3) | Verify real audio matches at gap seams | **Always runs** |

**Default (`disable_structure_trust = false`):**

- Structure pre **and** post ≥ `strong_structure_trust` (0.90) → **skip** waveform gate; status `patched (struct …)`.
- Structure 0.85–0.90 → waveform runs with threshold softened to `min(min_fill_correlation, 0.12)`.
- Otherwise → waveform at full `min_fill_correlation`.

**With `--no-structure-trust` (`disable_structure_trust = true`):**

- Waveform gate **never** skipped; partial soften **off**.
- **Both** pre and post waveform seams must pass — short-gap mean and one-strong-seam shortcuts disabled.
- Does **not** disable structure match, gap extension, or border standoff.

**Waveform gate (when it runs, structure trust on):** pass if mean(pre, post) ≥ threshold for short gaps (≤ `short_gap_mean_correlation_secs`); else if `short_gap_one_strong_seam_fallback`, pass when either seam ≥ threshold. Longer gaps require both seams individually. Partial structure soften caps threshold at `0.12` when structure scores ≥ `partial_structure_waveform_soften`.

**Boundary extension retries** (shared `gap_end_extend_max_ms` / `gap_end_extend_step_ms`):

| Direction | Config / CLI off | Retry when |
|-----------|------------------|------------|
| Post-end | `gap_end_extend_on_post_seam_fail` / `--no-gap-end-extend` | Post &lt; min and (pre ≥ min, or post within 0.05 of pre, or pre ≥ post + 0.10) |
| Pre-start | `gap_start_extend_on_pre_seam_fail` / `--no-gap-start-extend` | Pre &lt; min and post ≥ min |

**Human skip strings (status column):**

| Pattern | `GapPatchSkipReason` |
|---------|----------------------|
| `skipped: boundary correlation below threshold (pre=… post=… min=…)` | Waveform gate failed after retries |
| `skipped: boundary alignment failed` | Structure bracket failed on B |
| `skipped: structure below threshold` | Structure scores below `min_structure_match_score` |
| `skipped: …` (other) | B extract failed, zero-length gap, out of range, etc. |

**stderr (`tracing::warn`):** `gap N/M (range): waveform seam correlation below threshold` (and similar) when a fill is skipped; `tracing::debug` when a boundary extension succeeds (`gap end extended…` / `gap start extended…`).

**Verbose stdout patch lines:** `patched (struct pre→post)` when structure-trusted; `patched (pre→post slide=+Xs)` otherwise; skipped rows show full skip reason in the status column.

---

## Per-binary wiring

| Concern | `clip-sync-cli` | `clip-sync-repair` |
|---------|-----------------|-------------------|
| CLI entry | `infrastructure/cli/mod.rs` | `infrastructure/cli/mod.rs` |
| `-v` | `ProgressMode::Verbose` + `show_diagnostics` | `ProgressMode::Verbose` + `args.verbose` → `show_diagnostics` |
| `-q` | `ProgressMode::Quiet` | `ProgressMode::Quiet` |
| Startup banner | `clip-sync: aligning …` | `clip-sync-repair: aligning …` |
| JSON | `AlignmentReport` on stdout | `RepairJsonOutput { scan, patch }` on stdout |

---

## Testing expectations

New progress or report output must:

1. Respect tier gating (`phase` vs `phase_verbose` vs `progress`).
2. Keep stdout free of progress noise in all tiers.
3. Update golden / unit tests when human layout changes.

| Area | Location |
|------|----------|
| Progress reporter unit tests | `crates/clip-sync/src/infrastructure/logging/progress.rs` |
| Analyzer human + JSON | `crates/clip-sync-cli/tests/cli_output.rs`, `infrastructure/cli/output.rs` |
| Repair human + JSON | `crates/clip-sync-repair/src/infrastructure/cli/output.rs` (module tests), `tests/fixtures/full_surface_repair.json` |
| Repair integration | `crates/clip-sync-repair/tests/cli_mux_integration.rs`, `cli_wav_integration.rs` |

---

## Known gaps (repair vs this contract)

Tracked here so implementers can close drift without re-reading the archived plan.

| Item | Contract | Current behavior | Severity |
|------|----------|------------------|----------|
| Patch stage label | `Repairing N gap(s)...` (user-facing summary) | `Aligning N fill region(s) (structure match + splice)...` | Cosmetic — more technical than spec |
| Mux stage label | `Muxing → <filename>` | `Muxing video with patched audio...` (filename only on stdout `Output:` line) | Cosmetic |
| stderr capstone | Optional `Wrote <path> (N patched, offset …)` | Not emitted | Optional — never required |
| Decode skips in repair verbose | Show per-clip decode-skip counts like analyzer (`show_diagnostics`) | Data in `AlignmentReport` clips but not rendered in `format_human` | Parity gap |
| B scan phase | Major `Scanning video B for gaps...` when `scan_both` | `scan-b` progress label only (no `phase()` banner) | Minor — easy to miss in verbose checklist |
| Edge-case phases | — | `No gaps planned for patch…`, `No gaps were patched; skipping WAV/mux…` use `phase()` | Acceptable; adds ≤2 lines in edge cases |
| Legacy helper | — | `format_patch_summary()` still exists for tests; not used in live stdout path | None — unified table is live |

If you fix a row, update this table in the same PR.

---

## Related files

| Area | Path |
|------|------|
| Progress reporter | `crates/clip-sync/src/infrastructure/logging/progress.rs` |
| Progress mode / tracing init | `crates/clip-sync/src/infrastructure/logging/mod.rs` |
| `ProgressReporter` trait | `crates/clip-sync/src/application/ports.rs` |
| Align phases + mid-run summary | `crates/clip-sync/src/application/align_videos.rs` |
| Shared report formatters | `crates/clip-sync/src/application/report.rs` |
| Analyzer CLI output | `crates/clip-sync-cli/src/infrastructure/cli/output.rs` |
| Repair CLI output | `crates/clip-sync-repair/src/infrastructure/cli/output.rs` |
| Repair patch phases | `crates/clip-sync-repair/src/application/patch_audio.rs` |
| Repair scan phases | `crates/clip-sync-repair/src/application/scan_gaps.rs` |
| Repair mux phases | `crates/clip-sync-repair/src/infrastructure/ffmpeg_mux.rs` |
