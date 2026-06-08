# Temporary plan: workspace refactor (core library + CLI + repair)

> **Status:** Not started. Archive to `docs/archive/workspace-refactor-plan.md` when shipped.  
> **Location:** Root `TEMP-*.md` for active work; move/archive when complete (see [BACKLOG.md](BACKLOG.md)).

**Problem:** `clip-sync` is a binary-only crate that mixes alignment engine, default adapters, and CLI in one tree. A future **repair** tool (align → scan gaps in video A → patch from aligned video B → write output) needs the same alignment pipeline without duplicating code or turning the analyzer into a file-mutating product.

**Goal:** Restructure into a **Cargo workspace**:

1. **`clip-sync`** — library crate (domain + application + default infrastructure adapters).
2. **`clip-sync-cli`** — thin executable; preserves today’s `clip-sync` command name and behaviour.
3. **`clip-sync-repair`** — new executable; depends on the library; report-only gap scan first, optional patched output later.

No user-visible behaviour change for the analyzer until repair ships. Each phase must keep `cargo test` green.

---

## Decisions

Locked before implementation. Change only with an explicit plan revision.

| Topic | Decision |
|-------|----------|
| **Workspace vs single-crate `lib.rs`** | **Workspace** from the start. Avoids a second large move when `clip-sync-repair` lands. |
| **Library crate name** | **`clip-sync`** (Rust import: `clip_sync`). Binary crate **`clip-sync-cli`** installs binary **`clip-sync`**. Repair crate **`clip-sync-repair`** installs binary **`clip-sync-repair`**. |
| **Analyzer scope** | `clip-sync-cli` remains **read-only** (report offset only). Writing patched files is **repair-only**. |
| **Repair ↔ analyzer coupling** | **Standalone executable.** Repair calls `clip_sync::align` (or `AlignVideos`) internally. **No** requirement to pipe JSON from a prior `clip-sync` run in v1. Optional `--alignment-json` deferred. |
| **Config split** | Library exposes **`AlignConfig`** `{ clip, alignment }`. CLI wraps with **`AppConfig`** `{ align, output, logging }` via `#[serde(flatten)]`. Repair adds local **`RepairConfig`** in its crate only. |
| **Public API surface** | Small stable root re-exports + port traits for injection. Do **not** make every internal module `pub`. |
| **Default adapters in lib** | **Yes** — `SymphoniaMediaReader`, Chromaprint fingerprinter/aligner, TOML config load, logging/progress live in the library so CLI and repair share one implementation. |
| **CLI-only code** | `infrastructure/cli/*` (args, output, exit_code, `run`) stays in **`clip-sync-cli`** only. |
| **ffmpeg in production** | **Repair only** (subprocess, same pattern as `application/testing/ffmpeg_util.rs`). Library and CLI do **not** require ffmpeg on PATH. |
| **Test helpers** | `fakes`, `audio_fixtures` behind lib feature **`test-utils`**. `ffmpeg_util` + `corpus_fixtures` stay dev/test-only (lib `#[cfg(test)]` or workspace integration test). |
| **Corpus manifest** | Stays at **`tests/corpus/`** (workspace root). Corpus runner moves to integration test or lib `#[cfg(test)]` module — pick one in Phase 2 and document in [docs/corpus-validation.md](docs/corpus-validation.md). |
| **Features** | Move `he-aac`, `ffmpeg-tests` to appropriate crates. `he-aac` on lib; `ffmpeg-tests` on dev/integration. |
| **Versioning / release** | Single workspace version for v1 (all crates `0.1.0`). Publish policy TBD; local path deps suffice until then. |
| **PLAN.md / BACKLOG** | Update after Phase 3 (CLI extracted). Repair documented in this plan until repair Phase 2 ships. |

---

## Target layout

```text
clip-sync/                              # workspace root
├── Cargo.toml                          # [workspace] members
├── TEMP-workspace-refactor-plan.md     # this file → archive when done
├── PLAN.md
├── BACKLOG.md
├── tests/
│   └── corpus/                         # manifest.toml, README, fixtures
└── crates/
    ├── clip-sync/                      # LIBRARY
    │   ├── Cargo.toml
    │   └── src/
    │       ├── lib.rs                  # public API root
    │       ├── domain/                 # unchanged modules
    │       ├── application/
    │       │   ├── align_videos.rs
    │       │   ├── config.rs           # AlignConfig (+ shared Clip/Alignment)
    │       │   ├── error.rs
    │       │   ├── high_rate_refinement.rs
    │       │   ├── offset_refinement.rs
    │       │   ├── ports.rs
    │       │   └── testing/            # behind feature "test-utils"
    │       └── infrastructure/
    │           ├── chromaprint/
    │           ├── symphonia/
    │           ├── config/               # TOML load → AlignConfig / AppConfig bridge
    │           └── logging/
    │
    ├── clip-sync-cli/                  # BINARY (name: clip-sync)
    │   ├── Cargo.toml
    │   └── src/
    │       ├── main.rs                 # ExitCode → cli::run()
    │       └── cli/
    │           ├── mod.rs
    │           ├── args.rs
    │           ├── output.rs
    │           └── exit_code.rs
    │
    └── clip-sync-repair/               # BINARY (Phase 4+)
        ├── Cargo.toml
        └── src/
            ├── main.rs
            ├── cli/
            ├── repair_config.rs
            ├── gap_scan.rs             # application
            ├── gap_fill.rs             # PCM splice + verify
            └── ffmpeg_mux.rs           # infrastructure: subprocess writer
```

---

## Library public API (sketch)

`crates/clip-sync/src/lib.rs`:

```rust
pub mod domain;
pub mod application;
pub mod infrastructure;

pub use application::{
    AlignConfig, AlignVideos, AlignVideosRequest, AlignVideosResponse,
    AppError, ConfigError,
    ports::{Aligner, Fingerprinter, MediaReader, MediaSession, ProgressReporter},
};
pub use domain::{
    AlignmentResult, AudioTrack, ClipMatch, ClipMatchEstimate, ClipWindow, DomainError,
    MediaSource, MonoPcmClip, /* … */,
};
pub use infrastructure::{
    chromaprint::{ChromaprintAligner, ChromaprintFingerprinter},
    symphonia::SymphoniaMediaReader,
    logging::{init_tracing, StderrProgressReporter},
};

/// Convenience: default adapters, same wiring as clip-sync CLI.
pub fn align(
    request: AlignVideosRequest,
    progress: &dyn ProgressReporter,
) -> Result<AlignVideosResponse, AppError> { /* … */ }
```

**Stability rule:** external crates (`clip-sync-cli`, `clip-sync-repair`) depend only on `clip_sync::` root re-exports and documented port traits — not `clip_sync::infrastructure::symphonia::extract` internals.

**Repair-specific exposure (Phase 4):** consider making these public on the library if repair needs them without duplicating logic:

- `offset_refinement::aligned_slice_starts`
- `MediaSession::extract_mono` (already on port trait)
- `prepare_clip_for_fingerprint` / silence helpers from `domain::pcm_preparation`
- Optional: `extract_full_track_mono(session, track, progress)` helper if gap scan needs whole-timeline PCM (new application fn, not in v1 lib unless repair Phase 1 requires it)

---

## Config model

### Library: `AlignConfig`

```rust
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AlignConfig {
    #[serde(default)]
    pub clip: ClipConfig,
    #[serde(default)]
    pub alignment: AlignmentConfig,
}

impl AlignConfig {
    pub fn validate(&self) -> Result<(), ConfigError> { /* clip.validate() */ }
}
```

`AlignVideosRequest` uses `AlignConfig` (not full `AppConfig`).

### CLI: `AppConfig`

```rust
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(flatten)]
    pub align: AlignConfig,
    #[serde(default)]
    pub output: OutputConfig,
    #[serde(default)]
    pub logging: LoggingConfig,
}
```

TOML on disk **unchanged** for users — top-level `[clip]`, `[alignment]`, `[output]`, `[logging]` still deserialize into `AppConfig` via flatten.

### Repair: `RepairConfig` (repair crate only)

```toml
[repair]
min_gap_ms = 100
silence_peak_fraction = 0.01
min_fill_correlation = 0.35
crossfade_ms = 10
dry_run = true                      # default true until mux phase

[repair.output]
path = "repaired.mp4"               # required when dry_run = false
video_codec = "copy"
audio_codec = "aac"
```

Repair embeds or flattens `[clip]` + `[alignment]` from the same TOML conventions as the analyzer, or accepts CLI overrides only in v1.

---

## Phases

### Phase 0 — Spike (optional, ≤ half day)

**Goal:** Validate workspace layout without moving all files.

- Add root `Cargo.toml` workspace with **one** member: current package renamed/moved to `crates/clip-sync` **or** add `src/lib.rs` re-export only (rollback if abandoning).
- Confirm `cargo test` and `cargo run -- --help` from workspace root.
- **Exit:** Go/no-go on workspace; no repair work.

### Phase 1 — Workspace + library crate (no behaviour change)

**Goal:** All existing code lives in `crates/clip-sync`; root is workspace-only.

| Step | Action |
|------|--------|
| 1.1 | Create root `[workspace]` and `crates/clip-sync/Cargo.toml`; move dependencies from root (except `clap`). |
| 1.2 | `git mv src/{domain,application,infrastructure}` → `crates/clip-sync/src/` **except** `infrastructure/cli`. |
| 1.3 | Add `crates/clip-sync/src/lib.rs` re-exporting modules. |
| 1.4 | Keep temporary root binary: `src/main.rs` → thin wrapper `clip_sync::…` **or** jump straight to Phase 2 cli crate. Prefer **Phase 2 immediately** if CI allows one PR. |
| 1.5 | Fix all `crate::` paths; run `cargo test`, `cargo clippy`. |

**Deliverable:** Single library crate builds and passes all unit tests. User-facing binary may still be at root until Phase 2.

### Phase 2 — Extract `clip-sync-cli`

**Goal:** Installed binary name `clip-sync`; zero functional diff vs pre-refactor.

| Step | Action |
|------|--------|
| 2.1 | Create `crates/clip-sync-cli` with `clap` dep and `[[bin]] name = "clip-sync"`. |
| 2.2 | Move `infrastructure/cli/*` → `crates/clip-sync-cli/src/cli/`. |
| 2.3 | `main.rs` calls `cli::run()`; map `AppError` → exit codes via `exit_code.rs`. |
| 2.4 | Introduce `AlignConfig` / `AppConfig` split; update TOML loader in lib `infrastructure/config/file.rs` to deserialize CLI wrapper. |
| 2.5 | Remove root `src/` and root `[package]` if fully migrated. |
| 2.6 | Update [PLAN.md](PLAN.md) module layout; [BACKLOG.md](BACKLOG.md) “Binary-only crate” → Done. |

**Deliverable:** `cargo run -p clip-sync-cli -- …` matches previous CLI output (human + JSON). Corpus tests green.

### Phase 3 — `test-utils` feature + corpus hygiene

**Goal:** Clean dependency edges for integration tests and repair development.

| Step | Action |
|------|--------|
| 3.1 | Feature `test-utils` on lib: `fakes`, `audio_fixtures`. |
| 3.2 | Keep `corpus_fixtures` + `ffmpeg_util` in `#[cfg(test)]` **or** new `tests/corpus_integration.rs` at workspace root with `dev-dependencies`. **Decision:** prefer **`tests/corpus_integration.rs`** so lib `rlib` stays lean. |
| 3.3 | Fix cross-layer leak: `symphonia/media_reader_tests` must not import CLI paths; use `clip_sync` + `test-utils` only. |
| 3.4 | Document in [docs/corpus-validation.md](docs/corpus-validation.md): `cargo test -p clip-sync-cli` / workspace test commands. |

**Deliverable:** `cargo test --workspace` green; corpus runnable from CI unchanged.

### Phase 4 — Scaffold `clip-sync-repair` (report-only)

**Goal:** Second binary proves library reuse; **no file writes**.

| Step | Action |
|------|--------|
| 4.1 | Add `crates/clip-sync-repair` with clap skeleton: `clip-sync-repair [OPTIONS] <VIDEO_A> <VIDEO_B>`. |
| 4.2 | Wire `clip_sync::align()` with shared `AlignConfig` from config file / flags. |
| 4.3 | Implement **`gap_scan`**: full-timeline chunked extract from A (reuse `MediaSession::extract_mono` over sliding or whole-file windows); detect internal silent runs using same peak fraction as fingerprint prep; map B timeline via `recommended_offset_secs`. |
| 4.4 | For each candidate gap: report whether B has energy + optional normalized correlation at boundaries (`offset_refinement` helpers). |
| 4.5 | Output: human table + JSON `{ gaps: [{ start_secs, end_secs, fillable, correlation, reason }] }`. Exit **0** always when analysis completes. |
| 4.6 | Unit tests on synthetic PCM (lib test-utils or repair-local fixtures); no ffmpeg required. |

**Deliverable:** `clip-sync-repair --dry-run` (default) lists gaps; does not modify inputs.

### Phase 5 — Repair write path (optional, separate PR)

**Goal:** Patched output file when user opts in.

| Step | Action |
|------|--------|
| 5.1 | `gap_fill`: splice B PCM into A PCM with short crossfade; peak-normalize splice regions if needed. |
| 5.2 | `ffmpeg_mux` adapter: write temp WAV + `ffmpeg -i A -i wav -map 0:v -map 1:a -c:v copy -c:a aac out`. Subprocess only; map errors to repair `AppError`. |
| 5.3 | `--output PATH` + `--dry-run false`; require ffmpeg on PATH; clear stderr when missing. |
| 5.4 | Integration test with ffmpeg (ignored by default like corpus generated cases). |

**Deliverable:** End-to-end repair on chirp fixture with intentional dropout in A.

---

## `clip-sync-repair` behaviour summary

| Mode | Command | Depends on |
|------|---------|------------|
| Gap report (v1) | `clip-sync-repair A B` | lib align + native PCM scan |
| Patched file (v2) | `clip-sync-repair A B -o out.mp4 --no-dry-run` | + ffmpeg subprocess |

**Alignment:** always in-process via library — same offset semantics as analyzer (`offset_secs` = seconds to add to A to match B).

**Not in v1 repair:** batch >2 files, video frame sync, interactive splice review UI, `--alignment-json` import.

---

## Cargo.toml sketches

### Root `Cargo.toml`

```toml
[workspace]
members = [
    "crates/clip-sync",
    "crates/clip-sync-cli",
    # "crates/clip-sync-repair",  # Phase 4
]
resolver = "2"
```

### `crates/clip-sync/Cargo.toml`

```toml
[package]
name = "clip-sync"
version = "0.1.0"
edition = "2021"
description = "Align two videos by comparing audio fingerprints (library)"
license = "Non-commercial"

[features]
default = []
he-aac = ["dep:fdk-aac", "dep:symphonia-core"]
test-utils = []

[dependencies]
# all current deps except clap
```

### `crates/clip-sync-cli/Cargo.toml`

```toml
[package]
name = "clip-sync-cli"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "clip-sync"
path = "src/main.rs"

[dependencies]
clip-sync = { path = "../clip-sync" }
clap = { version = "4", features = ["derive"] }

[dev-dependencies]
clip-sync = { path = "../clip-sync", features = ["test-utils"] }
hound = "3"
tempfile = "3"
```

### `crates/clip-sync-repair/Cargo.toml` (Phase 4)

```toml
[package]
name = "clip-sync-repair"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "clip-sync-repair"
path = "src/main.rs"

[dependencies]
clip-sync = { path = "../clip-sync" }
clap = { version = "4", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

---

## Tests

| Phase | Required checks |
|-------|-----------------|
| 1 | All existing unit tests in lib modules pass. |
| 2 | CLI arg tests; human/JSON snapshot or structural asserts unchanged. |
| 3 | Full corpus via workspace integration test; `he-aac` feature on lib if needed. |
| 4 | Repair gap-scan unit tests (synthetic dropout + aligned chirp pair). |
| 5 | Repair ffmpeg integration (ignored locally without ffmpeg). |

**Regression bar:** Phase 2 completion requires corpus Tier A + B identical pass/fail vs pre-refactor (offsets within existing tolerances).

---

## Risks and mitigations

| Risk | Mitigation |
|------|------------|
| Large mechanical diff (import paths) | Single PR per phase; no logic changes in Phase 1–2. |
| `AppConfig` / TOML breakage | `#[serde(flatten)]` + round-trip deserialize test from existing example config. |
| Duplicate `align` logic in repair | Repair crate forbidden from copying `align_videos.rs`; code review + shared `clip_sync::align`. |
| Memory pressure on full-file gap scan | Chunked scan (e.g. 60 s windows with overlap); document in repair `--help`. |
| Scope creep (repair in analyzer) | Enforce crate boundary: no `ffmpeg` in `clip-sync-cli` deps. |

---

## Documentation updates (checklist)

- [ ] [PLAN.md](PLAN.md) — workspace layout, module paths, “library + CLI” architecture.
- [ ] [BACKLOG.md](BACKLOG.md) — mark “Binary-only crate” done; link this plan; add repair under Phase 6 or new section.
- [ ] [docs/corpus-validation.md](docs/corpus-validation.md) — workspace test commands.
- [ ] [docs/error-mapping.md](docs/error-mapping.md) — repair exit codes when Phase 4 ships (defer until then).
- [ ] Archive this file to `docs/archive/workspace-refactor-plan.md` when Phase 3 complete (repair may still be in progress — note partial completion in archive header).

---

## Completion verification

| Phase | Criterion |
|-------|-----------|
| **1** | `cargo test -p clip-sync` green; no `clap` in lib deps. |
| **2** | `cargo run -p clip-sync-cli -- A B` equivalent to pre-refactor; corpus green. |
| **3** | `test-utils` feature documented; no infra→CLI imports. |
| **4** | `clip-sync-repair A B` prints gap report using in-process align. |
| **5** | Patched MP4/WAV written with ffmpeg; video stream copied. |

**Archive trigger:** Phases **1–3** complete → archive as “core extraction done; repair Phases 4–5 tracked separately” **or** full archive when Phase 5 ships.

---

## Recommended PR sequence

1. **PR A:** Phase 1 + 2 (workspace, lib, CLI extract) — highest priority, unblocks everything.
2. **PR B:** Phase 3 (test-utils, corpus integration test move).
3. **PR C:** Phase 4 (repair scaffold, report-only).
4. **PR D:** Phase 5 (repair write path) — optional, user-facing beta.

Do **not** combine PR A with repair logic.
