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
| `clip-sync` | `-v` → `[output].show_diagnostics = true` | Per-clip window lines, decode-skip counts, repetition lines, high-rate peak, offset-verification skip reason |
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

**Before every `phase()` / `phase_verbose()` write**, the reporter finishes any active TTY progress line so stderr lines do not glue to `%` output.

**Extraction progress:** `detailed_extraction_progress()` returns true only in Verbose mode. Auto mode uses aggregated extraction progress (no per-clip extraction labels on stderr).

### Major stages (default stderr)

| Stage | `clip-sync` | `clip-sync-repair` |
|-------|-------------|-------------------|
| Startup | `clip-sync: aligning <A> with <B>` | `clip-sync-repair: aligning <A> with <B>` |
| Fingerprint align | — | `Aligning audio fingerprints...` |
| Match | `Searching for match...` | (via shared align path) |
| Scan | — | `Scanning video A for gaps...` |
| Patch | — | `Aligning N fill region(s)...` (or `Repairing N gap(s)...` — see gaps below) |
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

`log_alignment_summary()` in `align_videos.rs` must use **`phase_verbose` only** — never `phase()` — so default runs do not duplicate the final stdout report.

---

## `tracing` defaults

When `RUST_LOG` is unset, `init_tracing` uses:

```text
clip_sync=<level>,clip_sync_repair=<level>,warn
```

(`<level>` from `[logging].level` or `--log-level`; default `info`.)

| Level | Typical content |
|-------|-----------------|
| **info** (app crates) | Rare in default UX — prefer phases for user-visible steps |
| **warn** | Skipped gap fills, correlation below threshold, verification failures worth surfacing |
| **debug** | Structure-match trust, B fill trim/extend, ffmpeg mux command detail, symphonia demuxer |
| **Third-party** | `warn` unless raised in `RUST_LOG` |

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
| Applied, default | `High-rate: +0.010s refinement applied` (no raw correlation peak) |
| Applied, verbose | `High-rate: +0.010s refinement applied (peak …)` |
| Skipped | verbose only: `High-rate: skipped (reason)` |

### Offset verification (stdout)

| Outcome | Default | Verbose |
|---------|---------|---------|
| Not verified | warn line always | — |
| Verified | hidden | `Verify: offset confirmed …` |
| Skipped | hidden | `Verify: skipped (reason)` |

### Analyzer (`clip-sync`)

- **Header:** `Alignment report` with per-clip lines, recommended offset, overlap, optional high-rate / verify / repetition.
- **Verbose-only:** clip window timestamps, decode-skip annotations, repetition diagnostics.
- **Implementation:** `crates/clip-sync-cli/src/infrastructure/cli/output.rs`

### Repair (`clip-sync-repair`)

- **Header:** `Alignment: offset … confidence …`, per-clip offsets when len > 1, drift, tracks, overlap, cross-check, optional high-rate / verify.
- **Gap section:** single unified table — **not** separate scan + patch sections.

```
Gaps in video A (5 found, 3 repaired, 0 skipped, 2 unfillable):

  #   Range                Dur      Status
  1   0:00 – 0:16          16.2s    unfillable
  2   19:43 – 19:44        1.0s     patched (struct 0.98→1.00)
  …
```

- **Status column:** merged scan + patch outcome (`unfillable`, `blocked (track layout)`, `repairable` [scan-only], `patched (…)`, `skipped: …`, `not planned: …`).
- **Default patch detail:** `patched (struct pre→post)` when structure-trusted; `patched (pre→post)` otherwise.
- **Verbose patch detail:** includes slide adjustment and full pre/post labels.
- **Footer:** `Output: <path>` when WAV or mux file was written.
- **Optional stderr capstone** (not required for compliance): `Wrote <path> (N gaps patched, offset …)` after mux.
- **Implementation:** `crates/clip-sync-repair/src/infrastructure/cli/output.rs` (`format_human`, `format_unified_gap_report`)

Dry-run / scan-only: header uses repairable counts from scan; status from scan labels only.

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
