# Backlog

Tracked follow-up work for `clip-sync`. See [PLAN.md](PLAN.md) for architecture and [docs/error-mapping.md](docs/error-mapping.md) for error handling.

Last updated: 2026-06-06 (alignment robustness).

---

## Alignment measurement robustness

**Status:** Done (2026-06-06).

| Item | Location |
|------|----------|
| Confidence from segment length + ambiguity penalty | `infrastructure/chromaprint/aligner.rs` |
| Segment clustering by offset | `infrastructure/chromaprint/aligner.rs` |
| Weighted multi-clip offset fusion | `domain/alignment.rs` |
| PCM preparation: trim silence, peak normalize, energy gate | `domain/pcm_preparation.rs` |
| Default `target_sample_rate` = 11025 Hz | `application/config.rs` |
| Rubato FFT resampling | `domain/resample.rs` |
| Chromaprint preset selection (`test2`–`test5`) | `application/config.rs`, `infrastructure/chromaprint/` |
| PCM offset refinement (coarse + short GCC) | `application/offset_refinement.rs` |
| Multi-track fallback (`try_all_tracks`) | `application/align_videos.rs` |
| Adaptive window slide (aligned sub-clip from A energy) | `domain/pcm_preparation.rs` |
| Require consistent offsets (`require_consistent_offsets`) | `domain/alignment.rs`, `AlignmentConfig` |
| End-clip validates start-clip (consistency check) | `domain/alignment.rs` |

**Follow-up:**

- `cross_correlate` dev oracle tests for Chromaprint offset validation

---

## High priority (other)

### Chromaprint adapter

**Status:** Done (2026-06-06).

- `ChromaprintFingerprinter` — mono PCM → fingerprint via `rusty-chromaprint` (`preset_test2`)
- `ChromaprintAligner` — `match_fingerprints`, best segment, offset/confidence
- Unit tests: validation, identical clips, phase-shifted chirp offset

**Follow-up:**

- Wire `NoMatch` / `AmbiguousMatch` if engine-level semantics are needed later

### Application-layer tests

**Status:** Done (2026-06-06).

- Fake ports in `src/application/testing/fakes.rs`
- Eleven `AlignVideos::execute` tests covering happy path, low confidence, config validation, clip mismatch, error propagation, resampling, and offset preference

---

## Medium priority

### Bitrate for track selection

**Status:** `AudioTrack.bitrate` is always `None` at probe.

Symphonia 0.5 `CodecParameters` has no stream bitrate field (`bits_per_coded_sample` is bits per encoded sample, not bps). `select_best_track` uses bitrate as a tiebreaker after sample rate and channels, but it currently has no effect.

**Options:**

- Parse codec-specific headers where available (e.g. MP3 frame headers)
- Wait for upstream Symphonia average-bitrate support
- Remove bitrate from selection until a reliable source exists

### Duration-less files at open

**Status:** Open rejects files when no track reports `n_frames` + `time_base`.

Some formats (notably MP3, streams) may decode fine but fail at open with “could not determine duration.” Clip planning and end-clip windows depend on duration.

**Options:**

- Probe duration via format metadata where available
- Decode-to-EOF fallback for duration estimation (slow; use sparingly)
- Relax open validation when at least one audio track is decodable; fail later if selected track lacks duration

### Re-probe on every extract

**Status:** Each clip calls `File::open` + probe + seek + decode.

For 2 clips × 2 videos that is four full file passes. Correct but slow on large files.

**Options:**

- Cache probe results (tracks, codec params) per path within a session
- Reuse a format reader with seek (careful with decoder reset state)
- Measure before optimizing

---

## Low priority

### Log decode skips

**Status:** Corrupt packets hit `DecodeError` and `continue` silently in the decode loop.

Usually fine, but badly damaged files may spin until EOF with a vague partial/empty error.

- Log at `debug` or `warn` with a skip counter
- Optionally fail after N consecutive decode errors

### Deduplicate probe setup

**Status:** `probe_media` and `extract_mono_window` share hint/open/probe boilerplate.

Extract into one helper to avoid drift between probe and extract paths.

### Log file appender

**Status:** `LoggingConfig.log_file` exists; CLI accepts `--log-file`; init warns “not yet implemented.”

- Add `tracing-appender` or file layer in `src/infrastructure/logging/mod.rs`
- Rotate / flush policy TBD

### Committed test fixtures

**Status:** MP4/MKV tests generate files via ffmpeg at runtime; skipped when ffmpeg is unavailable.

- Add small binary fixtures under `tests/fixtures/` for CI without ffmpeg
- Keep under size budget (PLAN: “kept small”)

---

## Recently resolved

| Item | Resolution |
|------|------------|
| Wrong `bitrate` mapping | Set to `None`; no false tiebreaker |
| Session max duration vs selected track | `AudioTrack.duration`; clip plan uses selected track |
| Post-seek pre-window samples | Sample-index trimming via packet timestamps |
| `target_sample_rate` unwired | Resample in `align_videos` after extract |
| Container format coverage | `isomp4` + `aac` features; MKV/MP4 tests with ffmpeg |
| Strict partial decode on minor gaps | `sample_count_tolerance()` (~20 ms) before failing |
| Higher-quality resampling | Rubato sinc resampler (was linear) |
| Default `target_sample_rate` | Default 11025 Hz to match Chromaprint |

---

## Explicitly out of scope (initial version)

From [PLAN.md](PLAN.md) — not backlog unless scope changes:

- Video frame / visual sync
- Batch processing (> two files)
- Writing aligned output files (report offset only)
- Network or streaming sources

---

## Suggested order of work

1. ~~Finish alignment robustness~~ (done 2026-06-06)
2. **Real-world corpus & validation** — Phase 0–1 done (committed WAV + manifest + `corpus_committed_cases`); Phase 2+ in [docs/TEMP-corpus-implementation-plan.md](docs/TEMP-corpus-implementation-plan.md)
3. Performance (session reuse) and polish (logging, fixtures) as needed
