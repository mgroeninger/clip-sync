# Development guide

Build commands, Cargo feature flags per crate, and how to run the full test matrix (including slow and `#[ignore]` tiers).

**Related:** [test-tiers.md](test-tiers.md) (**how to run each test tier** — start here for `test-tier.ps1`), [cli-output.md](../cli-output.md) (CLI progress and human-report contract), [json-output.md](../json-output.md) (JSON contract and revision procedure), [gap-repair-guide.md](../gap-repair-guide.md) (gap types and repair recommendations), [gap-fill-modes.md](../gap-fill-modes.md) (`fit` vs `gate`, flags, performance), [README.md](../../README.md) § Gap patching pipeline, [corpus-validation.md](corpus-validation.md) (alignment corpus findings), [tests/corpus/README.md](../../tests/corpus/README.md), [gap corpus README](../../crates/clip-sync-repair/tests/gap_corpus/README.md).

---

## Prerequisites

| Tool | Required for |
|------|----------------|
| Rust (stable) | build and all tests |
| `ffmpeg` on `PATH` | generated alignment corpus cases, `ffmpeg-tests` adapter tests, repair mux integration |
| `ffprobe` on `PATH` | repair `cli_mux_integration` (`mux_writes_video`) |

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
| `validation-tests` | no | Compiles `validate_floor_oracle`, `validate_residual_gate`, `validate_patch_audio` integration binaries |
| `diagnostic-tests` | no | Compiles `diag_energy_matrix`, `diag_seam_residual`, `diag_patch_audio`, `diag_anchor_seam`, `diag_w5_anchor_rescue`, `diag_w5_timing_offset`, `seam_residual_oracle` integration binaries |
| `calibration` | no | Gap-fingerprint calibration workflow: the `--gap-fingerprints` / `--fingerprint-gap` / `--fingerprint-diagnostics` producer flags + corpus writer, the `equivalence-calibration` bin (`clip-sync-repair`), and the `gap-fingerprint-stats` bin (`clip-sync-repair-harness`). Off by default so the diagnostic surface stays out of the production binary |

Without `ffmpeg-mux`, `--mux` is rejected at argument parse with a clear error ([error-mapping.md](../error-mapping.md)).

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

**Run tiers:** [test-tiers.md](test-tiers.md) — `test-tier.ps1` reference, composite profiles (PR, widest regression, pre-release), prerequisites.

Tests are grouped into **execution tiers** (when CI runs them). Acceptance row IDs (SD, SP, EC,
RG, …) describe **what** a test proves — see [test-acceptance-glossary.md](test-acceptance-glossary.md).
Tier machinery: [test-tiers.md](test-tiers.md) (runner guide), this doc (living reference for features/matrix). Migration history:
[archive/test-tier-plan.md](archive/test-tier-plan.md). Open work:
[test-tier-remainder.md](test-tier-remainder.md).

There are **four** execution tiers. "**oracle**" is *not* a tier — it is a label (the `oracle_`
file/test-name prefix) for domain-acceptance tests that assert against a computed ground-truth;
those schedule as **integration** (repo-only) or **validation** (external dep / exhaustive
contract). Select oracle tests by name (`cargo test oracle_`) or acceptance ID.

| Tier | Purpose | Default PR (`test-tier.ps1`)? | Typical location |
|------|---------|-------------------------------|------------------|
| **unit** | Pure logic, policies, small fakes | yes (`pr` → repair lib with `oracle_`-label skips) | `src/**` `#[test]` |
| **integration** | Patch/scan/CLI on synthetic WAV; seam behavior; repo-only domain-acceptance (`oracle_` label) | yes (subset via `pr-repair`) | `tests/*_integration.rs`, `tests/oracle_*.rs` |
| **validation** | External dep (real codec / ffmpeg / corpus / env) **or** exhaustive off-PR contract (RG, EC6, floor oracle) | no (`validation-tests` feature) | `tests/validate_*.rs` |
| **diagnostic** | CSV dumps, sweeps, golden generators (emit data, no assertion) | never (`diagnostic-tests` feature) | `tests/diag_*.rs`, `seam_residual_oracle` |

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

**Speed is not a tier.** A slow repo-only test (e.g. `integration_energy_patch` SP rows) stays
*integration*; it is kept off the default PR via script selection (`pr-repair-extended`, name
filters), not by relabeling it. Only external-dependency or exhaustive-contract work becomes
*validation*. Full machinery: [development.md](development.md) § Tier decision rule.

**PR gate:** `.\scripts\test-tier.ps1 -Tier pr` (alignment committed corpus + repair smoke +
CLI adapter tests). Does **not** run full `patch_audio_integration` (~15 min) or ignored
validation/diagnostic rows. **Validation tier is not CI** — no scheduled runner with ffmpeg /
corpus fetch; run `.\scripts\test-tier.ps1 -Tier validation` locally before releases or large
validation changes.

`cargo test --workspace` is a **local convenience** compile check only — not the CI PR gate.

**Phase 3 (`clip-sync-repair`):** integration test binaries are declared explicitly in
`Cargo.toml` (`autotests = false`). Bare `cargo test -p clip-sync-repair` runs **`--lib` only**
— not integration binaries. Validation and diagnostic binaries require feature flags:

| Feature | Binaries |
|---------|----------|
| *(default)* | lib + integration + `oracle_*` + `integration_gap_corpus` |
| `validation-tests` | `validate_floor_oracle`, `validate_residual_gate`, `validate_patch_audio` |
| `diagnostic-tests` | `diag_energy_matrix`, `diag_seam_residual`, `diag_patch_audio`, `diag_anchor_seam`, `diag_w5_anchor_rescue`, `diag_w5_timing_offset`, `seam_residual_oracle` |
| `calibration` | `equivalence-calibration` + `gap-fingerprint-stats` bins (not tests) |

```powershell
cargo test -p clip-sync-repair --features validation-tests --test validate_floor_oracle
cargo test -p clip-sync-repair --features diagnostic-tests --test diag_energy_matrix -- --nocapture
# Calibration bins (not part of any test tier):
cargo run -p clip-sync-repair --features calibration --bin equivalence-calibration -- gap-files/equiv
cargo run -p clip-sync-repair-harness --features calibration --bin gap-fingerprint-stats -- gap-files
```

**Phase 3.5 (`clip-sync-repair-harness`):** shared integration runners live in the workspace
crate `crates/clip-sync-repair-harness` (`clip_sync_repair_harness`), listed under
`clip-sync-repair` `[dev-dependencies]`. Tier binaries import runners with normal `use` statements;
corpus paths are resolved from `repair_tests_dir!()` in tier binaries (expands to
`env!("CARGO_MANIFEST_DIR")` in the repair `[[test]]` crate), passed into harness helpers such as
`floor_oracle::load_manifest(repair_tests_dir!())`.

| Harness module | Former `tests/common/` file | Used by |
|----------------|----------------------------|---------|
| `floor_oracle` | `floor_oracle_fixtures.rs` | `integration_floor_oracle_smoke`, `validate_floor_oracle` |
| `residual_gate` | `residual_gate_runner.rs` | `validate_floor_oracle` |
| `seam_residual` | `seam_residual_scoring.rs` | `seam_residual_corpus`, `diag_seam_residual` |
| `energy_matrix` | `energy_signature_matrix.rs` | `diag_energy_matrix` |
| `patch_audio` | (new) | `integration_energy_patch`, `diag_patch_audio`, `validate_patch_audio`; `patch_audio_integration` imports `PatchTestOptions` / runners |

To extend a runner: add or edit the module in `clip-sync-repair-harness/src/`, then call it from
the relevant `tests/<tier>_*.rs` binary. Do not reintroduce `include!` or `tests/common/`.

### Repair integration binary matrix

Explicit `[[test]]` entries in `crates/clip-sync-repair/Cargo.toml` (`autotests = false`). **PR**
means included in `.\scripts\test-tier.ps1 -Tier pr-repair` (and therefore `-Tier pr`). See
[test-acceptance-glossary.md](test-acceptance-glossary.md) for SD/SP/EC/RG row IDs.

| Binary | Tier | Feature | PR | Acceptance / role |
|--------|------|---------|----|-------------------|
| `config_roundtrip` | integration | — | yes | TOML fixture deserialize + validate |
| `scan_gaps_integration` | integration | — | yes | Chirp/silence `ScanGaps` |
| `cli_wav_integration` | integration | — | yes | CLI `--wav` scan + patch |
| `query_reference_integration` | integration | — | yes | Query-ref gap inside/outside mapped region |
| `wav_bit_depth_integration` | integration | — | yes | Source-driven WAV output bit depth |
| `integration_gap_corpus` | integration | — | yes (committed scan + `gap_corpus_w5_anchor_seam`) | Gap scan corpus manifest; timing/external/generated `#[ignore]` |
| `integration_energy_smoke` | integration | — | yes | Scan→patch tripwire (`corpus_scan_patch_smoke`, EC01 e2e) |
| `integration_energy_patch` | integration | — | **no** | SP01–SP03 (`i1_`–`i3_`); full `integration` tier only |
| `integration_residual_gate_smoke` | integration | — | yes | RG04 off-regression baseline |
| `integration_floor_oracle_smoke` | integration | — | yes | Floor manifest load + gap-frame geometry (not full codec matrix) |
| `oracle_energy` | integration (oracle label) | — | yes (fast rows) | SD01–SD08 (`u1_`–`u8_`); EC03/EC06 domain; EC01/EC02 `#[ignore]` |
| `seam_residual_corpus` | integration | — | yes | Seam score oracles; F4 headroom placement |
| `anchor_seam_oracle` | integration | — | **no** | Editorial anchor seam A1–A5b + **A6 domain** + F4 regression; **integration** tier (+ A6 pipeline `#[ignore]` in diagnostic) |
| `gap_cell_fixtures` | integration | — | yes | Per-gap-**type** classification contract on committed curated fixtures (footguns; media-free) |
| `golden_baseline_invariance` | integration | — | yes | Curated fixtures vs self-hosting `curated.golden.json` (media-free; `CURATED_GOLDEN_REGEN=1` to rebase) |
| `gap_repair_spec_diff` | integration | — | yes | Projection-fidelity differential over the curated fixtures (media-free) |
| `patch_audio_integration` | integration | — | **extended only** | Sine seam grid (~15 min); SP04 (`i4_f3`); `pr-repair-extended` |
| `cli_mux_integration` | integration | `ffmpeg-mux` | compile on PR† | Mux CLI; e2e mux `#[ignore]` — **validation** tier when ffmpeg on PATH |
| `validate_floor_oracle` | validation | `validation-tests` | no | Floor oracle codec matrix (ffmpeg + `fetch_corpus_sources`) |
| `validate_residual_gate` | validation | `validation-tests` | no | RG catalog rows + EC06 patch discrimination |
| `validate_patch_audio` | validation | `validation-tests` | no | SP05 — production-default fit smoke |
| `diag_energy_matrix` | diagnostic | `diagnostic-tests` | no | Energy mode matrix CSV |
| `diag_seam_residual` | diagnostic | `diagnostic-tests` | no | Seam residual CSV |
| `diag_patch_audio` | diagnostic | `diagnostic-tests` | no | Patch geometry CSV (I1/I3) |
| `diag_anchor_seam` | diagnostic | `diagnostic-tests` | no | Anchor candidate/bracket CSV (`speech_peaks`, C3, flat C1) |
| `diag_w5_anchor_rescue` | diagnostic | `diagnostic-tests` | no | W5 anchor-rescue single-cell scores (nominal/baseline + per-bracket gate CSV) |
| `diag_w5_timing_offset` | diagnostic | `diagnostic-tests` | no | W5 timing-offset recoverability grid (`offset × drift` lag CSV); slow gate-probe row `#[ignore]` |
| `seam_residual_oracle` | diagnostic | `diagnostic-tests` | no | In-memory broadband patch oracle; slow rescue row `#[ignore]` |

The cross-corpus `--gap-fingerprints` analyzer (former `diag_fingerprint_corpus` test, P0 prevalence) is now
the `gap-fingerprint-stats` bin under the `calibration` feature (`clip-sync-repair-harness`), taking corpus
dirs as CLI args instead of the `GAP_FP_DIRS` env var.

† `pr-repair` runs `cli_mux_integration` when `ffmpeg` is on `PATH` (non-ignored rows only). Ignored
mux e2e rows run in `.\scripts\test-tier.ps1 -Tier validation` when ffmpeg is on PATH.

**Formatting (local verification; CI runs `cargo fmt --all -- --check` on every push/PR):**

```powershell
cargo fmt --all
cargo fmt --all -- --check   # same as CI
```

**Clippy (local verification; CI runs the PR-equivalent line on every push/PR):**

```powershell
# PR-equivalent (no validation/diagnostic binaries)
cargo clippy -p clip-sync-repair --all-targets -- -D warnings

# Harness crate (shared runners)
cargo clippy -p clip-sync-repair-harness -- -D warnings

# Full repair harness
cargo clippy -p clip-sync-repair --all-targets --features validation-tests,diagnostic-tests -- -D warnings
```

### Wall-time budgets (debug, typical dev machine)

| Profile | Budget |
|---------|--------|
| `pr` | ~4–6 min |
| `pr-align` | ~10–30 s |
| `pr-repair` | ~4–6 min |
| `pr-repair-extended` | +~3–8 min (sine seam grid, skips SP rows) |
| `unit` (repair lib, `oracle_`-label skips) | ~30–60 s |
| `integration` (repair, integration + oracle `--test` binaries; **not** `validate_*` / `diag_*`) | ~20+ min (includes full `patch_audio`) |
| **`pr` + `integration` + `oracle`** (widest regression, not diagnostic) | ~25–35 min — see [test-tiers.md](test-tiers.md) |
| `validation` | **~5–8 h** workspace / **~4–5 h** repair-only (debug; see [test-tiers.md § Validation tier wall time](test-tiers.md#validation-tier-wall-time)); **local only** (not CI); **ffmpeg on PATH** + `.\scripts\fetch_corpus_sources.ps1` (floor oracle tests fail fast if missing) |

---

## Default / CI commands

See **[test-tiers.md](test-tiers.md)** for the full tier catalog, composite profiles, and parameters.

```powershell
# PR gate (same as GitHub Actions)
.\scripts\check-repair-test-manifest.ps1   # autotests=false [[test]] guard (also in CI)
.\scripts\test-tier.ps1 -Tier pr

# Common local profiles (details in test-tiers.md)
.\scripts\test-tier.ps1 -Tier pr-repair-extended
.\scripts\test-tier.ps1 -Tier integration -Package clip-sync-repair
.\scripts\test-tier.ps1 -Tier oracle -Package clip-sync-repair
.\scripts\test-tier.ps1 -Tier validation -Package workspace
.\scripts\test-tier.ps1 -Tier diagnostic -Package workspace -Nocapture
```

Per-crate slices: `-Tier pr-align`, `-Tier pr-repair`, `-Tier unit -Package clip-sync-repair`, etc. — [test-tiers.md § Script tier reference](test-tiers.md#script-tier-reference).

Do **not** use `cargo test --tests` for integration-only — Cargo still runs `--lib` with
`--tests`. Use the script or an explicit `--test <binary>` list (see
[archive/test-tier-plan.md](archive/test-tier-plan.md) Phase 1).

**Local full workspace compile check** (not CI):

```powershell
cargo test --workspace
```

Legacy filters (still valid for ad-hoc runs):

```powershell
cargo test -p clip-sync corpus_committed
cargo test -p clip-sync-cli
cargo test -p clip-sync-repair --test integration_gap_corpus gap_corpus_committed
cargo test -p clip-sync-repair --test patch_audio_integration
```

### `#[ignore]` convention (integration / lib tests)

Tests in feature-gated `validate_*` / `diag_*` binaries do **not** use `#[ignore]` — the
`validation-tests` / `diagnostic-tests` Cargo features control compilation instead.

For tests still in shared integration binaries or `--lib`, use:

```rust
#[ignore = "tier:oracle — EC01 production geometry; test-tier.ps1 -Tier oracle"]
#[ignore = "tier:validation — needs ffmpeg + fetch_corpus_sources; test-tier.ps1 -Tier validation"]
#[ignore = "tier:diagnostic — golden generator; test-tier.ps1 -Tier diagnostic"]
```

`tier:oracle` / `tier:validation` / `tier:diagnostic` in the reason string is the convention;
`test-tier.ps1` selects ignored rows by binary + `--ignored` (or substring filters for gap
corpus), not by parsing the reason text. Open follow-ups:
[test-tier-remainder.md](test-tier-remainder.md).

### Ignore scheduling

How `test-tier.ps1` runs `#[ignore]` rows (repair crate). Source of truth: `scripts/test-tier.ps1`.

| Tier command | Mechanism | Ignored rows |
|--------------|-----------|--------------|
| **oracle** | `--test oracle_energy` then `-- --ignored` on same binary | All ignored rows in `oracle_energy.rs` (EC01/EC02 production, patch control/smoke, haystack, EC06 search) |
| **validation** | `validate_*` feature binaries (no `#[ignore]`) + gap_corpus **substring** filters + `--ignored` **`--release`** | `gap_corpus_generated`, `gap_corpus_external`, `gap_corpus_patch_timing_*` — **not** `gap_corpus_regenerate_*` (manual only) |
| **validation** | `--test cli_mux_integration -- --ignored` when ffmpeg on PATH | `mux_writes_video`, `mux_24bit_source_pipe_completes_successfully` |
| **diagnostic** | `diag_*` + `seam_residual_oracle` (no `#[ignore]` in those binaries) | — |
| **diagnostic** | Named `--ignored` filters (not blanket `--lib --ignored`) | `broadband_oracle_veto_rescue_patches_marginal`, `write_full_surface_repair_golden`, `mux_reports_progress_for_short_fixture` |

**Manual only** (not in any tier script): `gap_corpus_regenerate_committed_wav_fixtures` (overwrites
committed WAVs).

**`clip-sync`:** validation/diagnostic tiers use substring filters on `--lib`; legacy ignore
strings remain — see [test-tier-remainder.md § Open — clip-sync ignore cleanup](test-tier-remainder.md#open--clip-sync-ignore-cleanup-1-h).

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

See [corpus-validation.md](corpus-validation.md) and [tests/corpus/README.md](../../tests/corpus/README.md).

### 3. Repair gap corpus (all tiers)

```powershell
# Committed + generated
cargo test -p clip-sync-repair --test integration_gap_corpus gap_corpus -- --ignored

# External (real media; ground truth in manifest)
$env:CLIP_SYNC_GAP_CORPUS = "F:\Video"   # directory with your MKV files
cargo test -p clip-sync-repair --test integration_gap_corpus gap_corpus_external -- --ignored
```

See [gap corpus README](../../crates/clip-sync-repair/tests/gap_corpus/README.md).

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
# Build feature required; ffmpeg on PATH (included in pr-repair when ffmpeg is available)
cargo test -p clip-sync-repair --features ffmpeg-mux --test cli_mux_integration
```

`mux_writes_video` soft-skips when ffmpeg is missing.

### 6. Slow PCM refinement (library)

```powershell
cargo test -p clip-sync pcm_discover refine_recovers -- --ignored
```

These exercise 60 s synthetic clips; several minutes wall time.

### One-shot “everything local” script

Requires ffmpeg on `PATH`. External corpus tiers are optional (set env vars or skip those lines). See [test-tiers.md § Composite profiles](test-tiers.md#composite-profiles).

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

## Versioning and release

Clip-sync uses a **single workspace version** defined in the root `Cargo.toml` under `[workspace.package].version`. Every crate under `crates/` inherits it via `version.workspace = true`. The CLI binaries (`clip-sync`, `clip-sync-repair`) expose it via `-V` / `--version` (Clap `version` → `CARGO_PKG_VERSION`).

| Crate | Role |
|-------|------|
| `clip-sync` | Library |
| `clip-sync-cli` | Analyzer binary |
| `clip-sync-repair` | Repair binary |
| `clip-sync-repair-fixtures` | Test fixtures (`publish = false`) |
| `clip-sync-repair-harness` | Integration runners (`publish = false`) |

**Publish policy:** crates are not published to crates.io today. Version bumps are for operator traceability (git tags, `--version`, support questions). Revisit publish policy before any public crate release.

### Semver guidance

Follow [semver](https://semver.org/) at the workspace level:

| Bump | When |
|------|------|
| **PATCH** (`0.1.0` → `0.1.1`) | Bug fixes, performance improvements, refactors with no user-visible behavior change |
| **MINOR** (`0.1.0` → `0.2.0`) | New features, new CLI flags, additive JSON fields, new gap-repair behavior that does not break existing configs |
| **MAJOR** (`0.1.0` → `1.0.0`) | Breaking CLI changes, breaking TOML config keys or semantics, non-additive `--format json` changes, removal of supported behavior |

While all crates remain `0.x.y`, treat **MINOR** as the default bump for meaningful user-facing improvements and **PATCH** for fixes-only releases.

### JSON output contract

JSON versioning is **separate** from crate semver. The analyzer/repair `--format json` shape is documented in [json-output.md](../json-output.md) (currently **v1**). When changing report DTOs, follow its **Revision procedure**:

1. Change application-layer report types — not domain types.
2. Regenerate golden fixtures (`write_full_surface_alignment_golden`, `write_full_surface_repair_golden`).
3. Update [json-output.md](../json-output.md) — bump the contract version marker for breaking changes; additive optional fields may stay on v1.
4. Land doc + fixtures + code in the same commit.

Breaking JSON changes usually warrant a workspace **MINOR** or **MAJOR** bump depending on consumer impact.

### Release checklist

1. **Land pending work** on the release branch (typically `main`).
2. **Run pre-release validation** — at minimum (budget **~5–8 h** for full workspace validation in debug; see [test-tiers.md § Validation tier wall time](test-tiers.md#validation-tier-wall-time)):
   ```powershell
   .\scripts\test-tier.ps1 -Tier pr
   .\scripts\test-tier.ps1 -Tier validation -Package workspace
   ```
   See [Full test suite](#full-test-suite) above and [corpus-validation.md](corpus-validation.md) for operator sign-off when fit/patch defaults or performance tuning changed.
3. **Decide the bump** (patch / minor / major) using the semver table above.
4. **Bump the workspace version** (updates root `Cargo.toml` only; all crates inherit):
   ```powershell
   .\scripts\bump-version.ps1 -Bump patch    # or minor | major
   .\scripts\bump-version.ps1 -Version 0.2.0 # explicit set
   .\scripts\bump-version.ps1                # print current version
   ```
5. **Verify** resolved versions (optional — the script prints these after a bump):
   ```powershell
   cargo metadata --format-version 1 --no-deps | ConvertFrom-Json |
     Select-Object -ExpandProperty packages |
     Where-Object { $_.manifest_path -like '*\crates\*' } |
     Select-Object name, version
   ```
6. **Commit** with a message like `release: v0.1.1` (the version bump only; feature work should already be merged).
7. **Tag** (recommended when operators install or compare builds from git):
   ```powershell
   git tag -a v0.1.1 -m "clip-sync v0.1.1"
   git push origin v0.1.1
   ```
8. **Build release binaries** for local distribution:
   ```powershell
   cargo build --release -p clip-sync-cli --features he-aac,ac3
   cargo build --release -p clip-sync-repair --features ac3,ffmpeg-mux,he-aac
   ```

---

## Environment variables

| Variable | Used by | Purpose |
|----------|---------|---------|
| `CLIP_SYNC_CORPUS` | `corpus_external_cases` | Writable directory for the 3600 s external alignment case |
| `CLIP_SYNC_GAP_CORPUS` | `gap_corpus_external` | Root directory containing real media files referenced in gap manifest |
| `CLIP_SYNC_WORKSPACE_ROOT` | alignment `corpus_fixtures`, floor-oracle harness | Override workspace root when resolving `tests/corpus/` or `tests/floor_oracle/` (rare) |

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
cargo test -p clip-sync-repair --test integration_gap_corpus gap_corpus_regenerate -- --ignored --nocapture
```

---

## Ignored tests reference

| Test / filter | Crate | Trigger |
|---------------|-------|---------|
| `corpus_generated_cases` | `clip-sync` | `--ignored`; ffmpeg for container cases |
| `corpus_query_reference_45min_anchor` | `clip-sync` | `--ignored`; 60 min query-reference oracle (~minutes) |
| `corpus_external_cases` | `clip-sync` | `--ignored`; `CLIP_SYNC_CORPUS` |
| `regenerate_committed_wav_fixtures` | `clip-sync` | `--ignored`; overwrites committed WAVs |
| `gap_corpus_*` | `clip-sync-repair` | `--test integration_gap_corpus`; generated/external/patch_timing `--ignored` |
| `pcm_discover_finds_*`, `refine_recovers_large` | `clip-sync` | `--ignored`; slow |
| `mux_arg_rejected_without_feature` | `clip-sync-repair` | `--test cli_mux_integration` with `ffmpeg-mux`; runs on `pr-repair` when ffmpeg is on PATH |
| `mux_writes_video`, `mux_24bit_source_pipe_completes_successfully` | `clip-sync-repair` | `--test cli_mux_integration` with `ffmpeg-mux`; `#[ignore]` — `test-tier.ps1 -Tier validation` when ffmpeg on PATH |
| `gap_corpus_regenerate_committed_wav_fixtures` | `clip-sync-repair` | manual only (not in validation tier — overwrites committed WAVs) |
| `write_full_surface_repair_golden` | `clip-sync-repair` | `--lib`; `test-tier.ps1 -Tier diagnostic` |
| `mux_reports_progress_for_short_fixture` | `clip-sync-repair` | `--lib` + `ffmpeg-mux`; `test-tier.ps1 -Tier diagnostic` when ffmpeg on PATH |
| `broadband_oracle_veto_rescue_patches_marginal` | `clip-sync-repair` | `seam_residual_oracle`; `test-tier.ps1 -Tier diagnostic` |
| `w5_anchor_rescue_pipeline_engages_anchor_seam_*` | `clip-sync-repair` | `anchor_seam_oracle`; A6 pipeline — `#[ignore]` until anchor bracket reaches High |
| `w5_anchor_rescue_single_cell` | `clip-sync-repair` | `diag_w5_anchor_rescue` (`diagnostic-tests`); single-cell nominal/baseline + per-bracket gate scores |

Feature-gated tests (not ignored, but **not compiled** without features): `media_reader_tests` blocks under `ffmpeg-tests` (includes backward-seek MP4/MKV and MKV padded-duration extent tests — WAV backward-seek runs in default `cargo test -p clip-sync`); **`extract_window_regression`** (`extract_window_regression.rs`) — cross-format `extract_loop` matrix: WAV mono + interleaved in default CI; MP4 AAC, MKV FLAC/MKV AAC, MP3, and MKV/AAC anchored-end extract/align behind `ffmpeg-tests`; `ac3_dual_track_b_scan_detects_gap` under `ac3` + `ffmpeg-tests`; `ac3_corpus_chirp` oxideav railing characterization under `ac3` + `ffmpeg-tests` (expects zero full-scale samples).

**Optional local step** for container seek + extract regressions (see [scripts/test-container-seek.ps1](../../scripts/test-container-seek.ps1)):

```powershell
.\scripts\test-container-seek.ps1
# or manually:
# cargo test -p clip-sync --features ffmpeg-tests backward_seek track_decodable_extent extract_window_regression
```
