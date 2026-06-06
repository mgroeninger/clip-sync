# Backlog

Open follow-up work for `clip-sync`. See [PLAN.md](PLAN.md) for architecture, [docs/corpus-validation.md](docs/corpus-validation.md) for the test corpus, and [docs/error-mapping.md](docs/error-mapping.md) for error handling.

Last updated: 2026-06-06.

Each item includes a **problem** (what is wrong today), **impact** (why it matters), and **direction** (concrete follow-up).

---

## High priority

### Dual-track default track selection

**Problem:** `select_best_track` (`src/domain/policies.rs`) ranks tracks by sample rate, then **fewer** channels, then bitrate. Higher sample rate wins outright, so a secondary commentary or effects track with 48 kHz is chosen over the main 44.1 kHz program audio. The channel tiebreaker also prefers stereo over surround — the opposite of “highest quality” for many containers. Bitrate never breaks ties because it is always `None` at probe (see medium-priority item below).

Corpus case `mp4_dual_track_wrong_default` demonstrates the failure. `try_all_tracks` brute-forces all decodable pairs and picks the best alignment score, but it is **off by default** and multiplies decode work.

**Impact:** Wrong track → wrong offset with high confidence. Users with multi-track MP4/MKV get silently bad results unless they discover the hidden flag.

**Direction:**

- Document `try_all_tracks` in CLI help and user-facing docs (multi-track containers)
- Revise domain policy: prefer first audio / program / default track on ties; do not assume higher sample rate means “main” mix
- Consider enabling `try_all_tracks` by default when `list_tracks()` returns more than one decodable track (with perf guard)
- Wire bitrate probe (below) or remove dead tiebreaker until data exists

**References:** `src/domain/policies.rs`, `src/application/align_videos.rs` (`align_best_track_pair`), `tests/corpus/manifest.toml` (`mp4_dual_track_wrong_default`)

---

### Large-offset alignment accuracy

**Problem:** Chromaprint matching is reliable for modest inter-file delays but degrades on large leaders relative to clip length. Corpus case `wav_leader_30s` uses a **+15 s** proxy; the true **+30 s** leader on 60 s clips measures ~16 s — roughly half the true offset. Clip length, fingerprint item duration, and coarse segment clustering all cap how far a match can be resolved in one window.

PCM refinement (`offset_refinement.rs`) only adjusts within ±1 s of the coarse Chromaprint estimate, so it cannot recover a 14 s coarse error.

**Impact:** Event recordings where one camera started much later (minutes, not seconds) may report a plausible-looking but wrong offset.

**Direction:**

- Investigate longer effective windows, Chromaprint preset tuning, or multi-pass alignment (coarse seek + refine)
- Extend PCM refinement range when coarse confidence is high but clips are long enough to cross-correlate at larger lags
- Restore +30 s expectation in manifest once within tolerance
- Add corpus cases bracketing 15 s / 30 s / 60 s leaders with explicit tolerance notes

**References:** `src/infrastructure/chromaprint/aligner.rs`, `src/application/offset_refinement.rs`, `tests/corpus/manifest.toml` (`wav_leader_30s`)

---

### Silent decode degradation

**Problem:** In the extract decode loop (`media_reader.rs`), `SymphoniaError::DecodeError` is caught and **`continue`** — corrupt or partial packets are dropped with no log, counter, or user-visible signal. Extraction can finish with truncated PCM that still fingerprints and may yield a confident false match.

**Impact:** Damaged files, bad rips, and certain container edge cases produce garbage-in-garbage-out alignment instead of a clear media error.

**Direction:**

- Log each skip at `debug`; emit `warn` with aggregate count when extraction completes
- Optionally fail after N consecutive decode errors or when sample count falls far below expected window size
- Surface “partial extract” in diagnostics (`--verbose` / JSON output)

**References:** `src/infrastructure/symphonia/media_reader.rs` (decode loop), `docs/error-mapping.md`

---

## Medium priority

### Architecture: domain and application layer leaks

**Problem:** [PLAN.md](PLAN.md) states the domain depends on nothing external and application orchestrates via ports. In practice:

| Location | External dependency | Stated layer |
|----------|---------------------|--------------|
| `src/domain/resample.rs` | `rubato` | Domain (should be pure) |
| `src/application/offset_refinement.rs` | `cross_correlate` | Application (should use a port) |
| `src/domain/alignment.rs` | `serde::Serialize` on report types | Domain (serialization is presentation) |

Resampling, PCM refinement, and JSON shape are not behind port traits; they are hard-wired in `AlignVideos`. Ports exist for decode, fingerprint, and align only.

**Impact:** Harder to swap algorithms, test domain in isolation, or reuse the core as a library. Architecture docs diverge from code, which misleads future contributors.

**Direction:**

- Move `resample_mono_pcm` behind a `Resampler` port (adapter uses `rubato`); domain keeps `MonoPcmClip` and policy about target rate
- Move PCM refinement behind an `OffsetRefiner` port (adapter uses `cross_correlate`)
- Move `Serialize` to application/infrastructure DTOs; domain types stay plain structs
- Update PLAN.md dependency table when done

**References:** `src/domain/resample.rs`, `src/application/offset_refinement.rs`, `src/application/ports.rs`, `PLAN.md` § Architecture

---

### Split `media_reader.rs`

**Problem:** `src/infrastructure/symphonia/media_reader.rs` is ~1,400 lines and owns probe, open, session cache, seek, decode loop, duration estimation, FDK-AAC integration, and a large inline test suite. Every codec or seek change touches one file.

**Impact:** Review fatigue, merge conflicts, and fear of refactoring the decode path. Onboarding cost for Symphonia work is high.

**Direction:** Split along natural seams, e.g.:

- `probe.rs` — hint, open, track list, duration
- `session.rs` — `SymphoniaMediaSession`, `MediaIoState`, decoder cache
- `extract.rs` — seek-to-window, decode loop, mono downmix
- `duration.rs` — container scan / fallback duration estimation
- Keep `error_mapping.rs` as-is; re-export from `mod.rs`

**References:** `src/infrastructure/symphonia/media_reader.rs`

---

### `MediaSession` interior mutability

**Problem:** `SymphoniaMediaSession` stores I/O in `RefCell<Option<MediaIoState>>`. `MediaSession::extract_mono` takes `&self` but mutates format reader and decoder cache. Production code relies on `expect("decoder cached")` and `expect("session io initialized")` for invariants the type system does not enforce.

**Impact:** Surprising API (looks immutable), `RefCell` panic on re-entrant borrow, not `Sync`, and brittle refactors. Acceptable for single-threaded CLI today; awkward if the tool grows threads or embeds as a library.

**Direction:**

- Prefer `extract_mono(&mut self, …)` on the port trait (breaking change — update fakes and use case)
- Or expose an explicit session handle with mutable extract API
- Replace `expect()` with internal helpers that return `MediaError` if cache invariants break

**References:** `src/infrastructure/symphonia/media_reader.rs`, `src/application/ports.rs`, `docs/TEMP-session-reuse-plan.md`

---

### Bitrate for track selection

**Problem:** `AudioTrack.bitrate` is always `None` at probe. `select_best_track` compares bitrates as a final tiebreaker, but that branch is dead code. Policy looks more sophisticated than it is.

**Impact:** Misleading domain policy; one less signal for dual-track disambiguation.

**Direction:**

- Parse codec-specific headers where available (e.g. MP3 frame headers, AAC ASC)
- Wait for upstream Symphonia average-bitrate support
- Or **remove** bitrate from `select_best_track` until a reliable source exists (prefer honest policy over noop tiebreaker)

**References:** `src/infrastructure/symphonia/media_reader.rs` (`probe_from_format`), `src/domain/audio_track.rs`, `src/domain/policies.rs`

---

### Duration-less files at open

**Problem:** `open` rejects files when no track reports decodable duration (`n_frames` + `time_base`, or format metadata). Some streams and odd MP3 layouts decode successfully but fail at open. End-clip windows and `clip_windows()` require a non-zero duration.

**Impact:** Valid-ish inputs fail early with “could not determine duration” instead of degrading to a single start clip or estimate-on-first-read.

**Direction:**

- Probe duration from format metadata where Symphonia exposes it
- Decode-to-EOF fallback for duration estimation (slow; log and use sparingly)
- Relax open validation when at least one track is decodable; fail at clip planning if duration still unknown

**References:** `src/infrastructure/symphonia/media_reader.rs`, `src/domain/policies.rs` (`clip_windows`)

---

### Chromaprint “no match” vs zero-confidence success

**Problem:** `ChromaprintAligner` returns `Ok(ClipMatchEstimate { offset_secs: 0.0, confidence: 0.0 })` when no segment matches or fingerprints are empty. That conflates “engine found nothing” with “offset is zero.” `AlignmentError::NoMatch` and `AmbiguousMatch` are defined and documented in [docs/error-mapping.md](docs/error-mapping.md) but marked `#[allow(dead_code)]` — unused.

Aligner also detects ambiguous clusters (`select_best_segment`) but only downgrades confidence; it never surfaces `AmbiguousMatch`.

**Impact:** Downstream fusion and logging cannot distinguish failure modes. Reserved error taxonomy docs promise semantics the code does not deliver.

**Direction:**

- Map empty segments → low-confidence `Ok` **or** `AlignmentError::NoMatch` consistently with error-mapping doc (pick one contract and test it)
- Map high ambiguity → `AmbiguousMatch { candidates }` when engine-level failure is appropriate
- Remove `dead_code` allows once wired; add adapter unit tests

**References:** `src/infrastructure/chromaprint/aligner.rs`, `src/application/error.rs`, `docs/error-mapping.md`

---

### Memory use and PCM cloning on long clips

**Problem:** Default `clip_length` is 15 minutes. Each extract holds full window PCM in `Vec<i16>`. The align path clones clips for preparation (`prepare_clip_for_fingerprint` clones internally; `align_extracted_pair` clones when `window_slide_secs == 0`). Multi-clip × two videos × optional slide padding multiplies resident memory. There is no streaming or chunked fingerprinting.

**Impact:** Long event recordings can use hundreds of MB RAM per run on modest hardware. Not a functional bug for v1, but a structural ceiling.

**Direction:**

- Document memory expectations in PLAN or README (order-of-magnitude for default config)
- Reduce clones: in-place preparation where possible, `Cow` or slice views for fingerprint input
- Longer term: stream PCM through fingerprinter in fixed-size chunks if `rusty-chromaprint` API allows

**References:** `src/application/align_videos.rs`, `src/domain/pcm_preparation.rs`, `src/domain/mono_pcm_clip.rs`

---

### Sorted-window extraction (session reuse follow-up)

**Problem:** Session reuse keeps one format reader, but extracts still seek in clip-plan order. When windows are non-monotonic (start clip, end clip, interior), the reader may seek backward repeatedly — wasted I/O on long files compared to chronological decode order.

**Impact:** Partial perf win from session reuse; multi-clip runs on hour-long files still do redundant seeking.

**Direction:**

- Sort windows by start time before extract loop; map results back to clip index
- Add corpus wall-time assertion or benchmark on multi-clip long-media case
- Archive [TEMP-session-reuse-plan.md](docs/TEMP-session-reuse-plan.md) when done

**References:** `src/application/align_videos.rs` (`extract_clips`), `docs/TEMP-session-reuse-plan.md`

---

## Low priority

### Silent resample fallback

**Problem:** `resample_mono_pcm` (`domain/resample.rs`) silently falls back to linear interpolation when `rubato` construction or `process_into_buffer` fails. No log, metric, or user-visible diagnostic.

**Impact:** Poor resample quality can degrade fingerprint accuracy; failures are invisible in traces.

**Direction:** Log at `warn` on fallback; consider surfacing in verbose diagnostics. Prefer moving resample to infrastructure (see architecture item) so logging is natural at the adapter boundary.

**References:** `src/domain/resample.rs`

---

### Stringly-typed port errors

**Problem:** `MediaError`, `FingerprintError`, and `AlignmentError` carry free-form `String` details (`OpenFailed`, `DecodeFailed { detail }`, etc.). User messages work via `thiserror`, but structured matching, `source()` chains, and stable adapter tests are harder. Symphonia context is flattened at mapping time.

**Impact:** Debugging production failures relies on string parsing; error taxonomy drifts as adapters evolve.

**Direction:**

- Keep display strings for stderr; add optional structured fields or nested enums where Symphonia categories repeat
- Preserve `#[source]` for I/O errors where appropriate
- Document stable error codes in `docs/error-mapping.md` if JSON output adds machine-readable errors later

**References:** `src/application/error.rs`, `src/infrastructure/symphonia/error_mapping.rs`

---

### Type and dependency polish

**Problem:** Several small inconsistencies between stated design and Rust types:

- `Fingerprint` is a public `Vec<u32>`; PLAN describes an opaque blob — no newtype encapsulation
- `ClipMatchEstimate` derives `PartialEq` on `f64`/`f32` — fragile for tests and domain comparisons
- `anyhow` is listed in `Cargo.toml` but unused (project standardized on `thiserror`)
- `select_aligned_subclip_pair` uses `target_duration.as_secs()` — sub-second targets truncate
- Config serde stores `Duration` as whole seconds only (`duration_secs` module)

**Impact:** Low severity individually; collectively signals “draft” type discipline.

**Direction:**

- Newtype `Fingerprint` with private field; accessors for adapter crates
- Remove `PartialEq` from float-bearing estimates or compare with epsilon in tests only
- Remove `anyhow` from dependencies
- Document sub-second config limitation; use `as_secs_f64()` or sample-based counts in subclip selection if sub-second windows matter

**References:** `src/domain/alignment.rs`, `Cargo.toml`, `src/domain/pcm_preparation.rs`, `src/application/config.rs`

---

### Binary-only crate (no `lib.rs`)

**Problem:** The crate is a single binary (`main.rs` → `infrastructure::cli::run`). Integration tests and external tools cannot depend on a stable library surface; wiring adapters to ports is duplicated wherever the full pipeline is needed outside unit tests with fakes.

**Impact:** Harder to embed `clip-sync` in other Rust tools or run black-box integration tests against the real composition root.

**Direction:**

- Add `src/lib.rs` with `pub mod application`, `domain`, `infrastructure` (or re-export a narrow `clip_sync::run(config) -> Result<…>`)
- Thin `main` calls lib; corpus/integration tests use lib API

**References:** `src/main.rs`, `src/infrastructure/cli/mod.rs`

---

### Test helper cross-layer coupling

**Problem:** Infrastructure unit tests in `media_reader.rs` import `application::testing::ffmpeg_util` (behind `ffmpeg-tests` feature). Application fakes live under `application::testing` but infrastructure tests depend upward on application test modules.

**Impact:** Layer diagram is violated in test code; moving or splitting test helpers breaks adapter tests.

**Direction:**

- Move shared ffmpeg/WAV helpers to `tests/support/` or a `testing` module at crate root available to all `#[cfg(test)]` code
- Keep fakes in application for use-case tests only

**References:** `src/infrastructure/symphonia/media_reader.rs`, `src/application/testing/`

---

### Documentation drift (PLAN vs code)

**Problem:** [PLAN.md](PLAN.md) and code disagree in places:

- PLAN table: `num_clips` default **2**; `ClipConfig` default is **1**
- PLAN domain error list omits `NoDecodableAudioTracks`, `InsufficientAudio`
- PLAN claims domain has no external deps; `rubato` is in domain

**Impact:** PLAN is treated as architecture contract; drift causes wrong assumptions in reviews and backlog prioritization.

**Direction:** Audit PLAN against current defaults and module layout; update or add “implemented differences” footnotes until refactors land.

**References:** `PLAN.md`, `src/application/config.rs`, `src/domain/`

---

### Log file appender

**Problem:** `LoggingConfig.log_file` and `--log-file` are parsed into config; `init_tracing` warns that file logging is **not yet implemented**. Users may believe logs are being written to disk.

**Impact:** Missing observability for long batch runs (when batch exists) or support scenarios.

**Direction:**

- Add `tracing-appender` or file layer in `src/infrastructure/logging/mod.rs`
- Rotate / flush policy TBD; document in PLAN configuration section

**References:** `src/infrastructure/logging/mod.rs`, `src/application/config.rs`

---

### Committed test fixtures

**Problem:** Corpus Tier B ships 3 WAV pairs (~3.4 MB). Encoded-format cases generate at test time via ffmpeg; CI without ffmpeg skips Tier A generated cases. No tiny committed MP3/AAC for codec smoke tests in minimal CI.

**Impact:** Codec regressions may not run on every CI job unless ffmpeg + features are enabled.

**Direction:**

- Optional: tiny committed MP3 (or similar) under size budget for CI without ffmpeg
- Keep total fixture size within PLAN “kept small” budget
- Document required CI flags (`ffmpeg-tests`, `he-aac`) in corpus README

**References:** `tests/corpus/`, `docs/corpus-validation.md`, `Cargo.toml` features

---

## Completed

### Session reuse / re-probe (Phases 1–3 core)

**Was:** `extract_mono` re-opened and re-probed on every clip window; typical 2-clip × 2-video run did redundant probe+open cycles after `open`.

**Done:** One probe per file per alignment run (`probe_media_reusable` at `open`); format reader + per-track decoders reused across extracts. Shared `open_format_reader` + `probe_from_format` for probe and session paths. `two_clip_consistent` has `max_wall_secs = 30` in manifest.

**Remaining:** sorted-window extraction (see medium priority above).

**References:** [TEMP-session-reuse-plan.md](docs/TEMP-session-reuse-plan.md), `src/infrastructure/symphonia/media_reader.rs`

---

## Explicitly out of scope (initial version)

From [PLAN.md](PLAN.md) — not backlog unless scope changes:

- Video frame / visual sync
- Batch processing (> two files)
- Writing aligned output files (report offset only)
- Network or streaming sources

---

## Suggested order of work

1. Dual-track selection policy + `try_all_tracks` documentation (user-visible correctness)
2. Silent decode degradation logging / fail-fast thresholds (trust in PCM input)
3. Large-offset accuracy (`wav_leader_30s` and related corpus cases)
4. Split `media_reader.rs` (maintainability before more codec work)
5. Architecture layer leaks (resample/refinement ports, serde out of domain)
6. Duration-less open edge cases
7. `MediaSession` mutability API cleanup
8. Chromaprint `NoMatch` / `AmbiguousMatch` wiring
9. Documentation sync (PLAN.md), type polish, log file appender, fixture/CI hardening
