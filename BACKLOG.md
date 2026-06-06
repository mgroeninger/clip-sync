# Backlog

Open follow-up work for `clip-sync`. See [PLAN.md](PLAN.md) for architecture, [docs/corpus-validation.md](docs/corpus-validation.md) for the test corpus, and [docs/error-mapping.md](docs/error-mapping.md) for error handling.

Last updated: 2026-06-06.

Each item includes **problem**, **impact**, **direction**, and **references**. Temporary implementation plans live under `docs/TEMP-*.md`.

---

## Recommended order of work

Phased sequence based on impact, risk, and dependencies. Items within a phase can overlap; do not start later phases until earlier correctness work is merged.

### Phase 1 — Silent wrong answers ✅

**Done (2026-06-06):** first decodable track selection; decode skip logging + consecutive-error fail-fast.

Next: [Phase 2](#phase-2--maintainability-and-perf-do-next).

### Phase 2 — Maintainability and perf (do next)
|---|------|-----------|
| 3 | [Split `media_reader.rs`](#split-media_readerrs) | ~1,400-line file blocks every Symphonia change |
| 4 | [Sorted-window extraction](#sorted-window-extraction-session-reuse-follow-up) | Natural follow-up to session reuse; measurable multi-clip perf |

Do the split **before** bitrate probe, duration hardening, or decode logging refactors spread further.

### Phase 3 — Algorithm research (parallel spike, not a quick fix)

| # | Item | Rationale |
|---|------|-----------|
| 5 | [Large-offset alignment accuracy](#large-offset-alignment-accuracy) | Needs measurement matrix (offset × clip length × preset); PCM refine ±1 s cannot fix coarse errors |

Run a spike before coding: decide product limit vs longer clips vs multi-pass alignment.

### Phase 4 — Edge cases and semantics

| # | Item | Rationale |
|---|------|-----------|
| 6 | [Duration-less files at open](#duration-less-files-at-open) | Partially done; audit remaining gaps |
| 7 | [Chromaprint no match vs zero-confidence](#chromaprint-no-match-vs-zero-confidence-success) | Freeze user-visible failure contract before JSON changes |
| 8 | [Bitrate for track selection](#bitrate-for-track-selection) | Probe or remove dead tiebreaker |

### Phase 5 — Validation diagnostics (optional flags, off by default)

| # | Item | Rationale |
|---|------|-----------|
| 9 | [Clip self-repetition check](#clip-self-repetition-check) | Surfaces ambiguous internal repeats; see [TEMP-clip-self-repetition-plan.md](docs/TEMP-clip-self-repetition-plan.md) |
| 10 | [Hold-out offset verification](#hold-out-offset-verification) | Independent check for single-clip runs; shares `ValidationConfig`; see [TEMP-offset-verification-plan.md](docs/TEMP-offset-verification-plan.md) |

Implement repetition before or alongside verification (shared config section, same align loop).

### Phase 6 — Architecture cleanup (when feature velocity slows)

| # | Item |
|---|------|
| 11 | [Architecture layer leaks](#architecture-domain-and-application-layer-leaks) |
| 12 | [`MediaSession` interior mutability](#mediasession-interior-mutability) — pair with `session.rs` split |
| 13 | [Documentation drift (PLAN vs code)](#documentation-drift-plan-vs-code) — after policy decisions land |

### Defer / opportunistic

- [Memory use and PCM cloning](#memory-use-and-pcm-cloning-on-long-clips) — document order-of-magnitude first; optimize when users report pain
- [Binary-only crate (no `lib.rs`)](#binary-only-crate-no-librs) — when embedding or black-box tests are needed
- [Log file appender](#log-file-appender), [committed test fixtures](#committed-test-fixtures), [test helper cross-layer coupling](#test-helper-cross-layer-coupling) — as CI/support needs arise
- [Type and dependency polish](#type-and-dependency-polish), [stringly-typed port errors](#stringly-typed-port-errors), [silent resample fallback](#silent-resample-fallback) — incremental when touching those files

---

## High priority

### Large-offset alignment accuracy

**Status:** Open — **research track**, not a quick fix.

**Problem:** Chromaprint degrades when leader offset is large relative to clip length. Corpus `wav_leader_30s` uses **+15 s** proxy; true **+30 s** on 60 s clips measures ~16 s. PCM refinement (`offset_refinement.rs`) only adjusts ±1 s around coarse estimate.

**Impact:** Long-start delays (minutes) may get plausible but wrong offsets.

**Direction:**

- Spike: matrix of leader offset × clip length × Chromaprint preset
- Options: longer windows, preset tuning, multi-pass (coarse → seek → refine), extend PCM refine range when clips allow
- Add corpus cases at 15 / 30 / 60 s with explicit tolerances; restore +30 s expectation when within bounds

**References:** `src/infrastructure/chromaprint/aligner.rs`, `src/application/offset_refinement.rs`, `tests/corpus/manifest.toml`

---

## Medium priority

### Split `media_reader.rs`

**Problem:** ~1,400 lines: probe, open, session cache, seek, decode, duration estimation, FDK-AAC, inline tests. Every codec change touches one file.

**Impact:** Review fatigue, merge conflicts, slow onboarding.

**Direction:** Split into `probe.rs`, `session.rs`, `extract.rs`, `duration.rs`; keep `error_mapping.rs`; move tests with modules. **No behaviour change** in the split PR.

**References:** `src/infrastructure/symphonia/media_reader.rs`

---

### Sorted-window extraction (session reuse follow-up)

**Problem:** Session reuse keeps one format reader, but extracts seek in clip-plan order (start → end → interior). Non-monotonic order causes backward seeks on long files.

**Impact:** Partial perf win from session reuse; redundant I/O on hour-long multi-clip runs.

**Direction:**

- Sort windows by start time before extract; map results back to clip index
- Wall-time assertion on `two_clip_consistent` or new long multi-clip case

**References:** `src/application/align_videos.rs`, [docs/archive/session-reuse-plan.md](docs/archive/session-reuse-plan.md)

---

### Clip self-repetition check

**Status:** Not started. Plan: [TEMP-clip-self-repetition-plan.md](docs/TEMP-clip-self-repetition-plan.md).

**Problem:** Clips with internal repetition (loops, duplicated segments) can produce ambiguous Chromaprint matches — cross-file alignment may latch wrong lag with high confidence.

**Impact:** False positives on looped B-roll, rebroadcast segments, or repeated stings within a window.

**Direction (phased):**

- Phase 0: spike `match_fingerprints(&fp, &fp)` on synthetic repeat
- Phase 1: `ValidationConfig`, `detect_clip_repetition`, wire in align loop, debug logging only
- Phase 2: `repetition_a` / `repetition_b` on `ClipMatch`, JSON + CLI `--check-clip-repetition`
- Phase 3: corpus `repeated_segment_in_clip`; optional confidence downgrade when repeat lag ≈ cross-file offset

Off by default. Diagnostic only (exit 0 in v1).

**References:** `docs/TEMP-clip-self-repetition-plan.md`, `src/infrastructure/chromaprint/aligner.rs`, `src/domain/alignment.rs`

---

### Hold-out offset verification

**Status:** Not started. Plan: [TEMP-offset-verification-plan.md](docs/TEMP-offset-verification-plan.md).

**Problem:** With `num_clips == 1`, one Chromaprint window is the only evidence. Multi-clip runs compare offsets across windows but never verify “at lag Δ, do shifted regions actually match at zero lag?”

**Impact:** Confident wrong Δ on single-clip runs has no independent check.

**Direction:**

- Shared `ValidationConfig` with repetition check (`verify_offset`, `min_verification_confidence`)
- After `recommended_offset_secs`, extract hold-out windows shifted by Δ; score lag-0 similarity
- Report `offset_verified` on `AlignmentResult`; keep offset + warn when unverified (v1)

Implement after or alongside repetition (shared config, same align loop).

**References:** `docs/TEMP-offset-verification-plan.md`, `src/application/align_videos.rs`

---

### Architecture: domain and application layer leaks

**Problem:** PLAN says domain has no external deps; code uses `rubato` in `domain/resample.rs`, `cross_correlate` in `application/offset_refinement.rs`, and `serde::Serialize` on domain report types. Resample/refinement not behind ports.

**Impact:** Harder to test domain in isolation; PLAN misleads contributors.

**Direction:** `Resampler` and `OffsetRefiner` ports; move `Serialize` to application/infrastructure DTOs; update PLAN. Do after `media_reader` split to reduce parallel churn.

**References:** `src/domain/resample.rs`, `src/application/offset_refinement.rs`, `src/application/ports.rs`, `PLAN.md`

---

### `MediaSession` interior mutability

**Problem:** `SymphoniaMediaSession` uses `RefCell<Option<MediaIoState>>`; `extract_mono(&self)` mutates internally. Production code uses `expect("decoder cached")` for invariants.

**Impact:** Surprising API, not `Sync`, brittle refactors.

**Direction:** `extract_mono(&mut self, …)` on port trait (breaking); or explicit mutable session handle. Pair with `session.rs` extraction. Replace `expect()` with `MediaError` returns.

**References:** `src/infrastructure/symphonia/media_reader.rs`, `src/application/ports.rs`

---

### Bitrate for track selection

**Problem:** `AudioTrack.bitrate` always `None` at probe; `select_best_track` bitrate tiebreaker is dead code.

**Impact:** Misleading policy; missed signal for dual-track disambiguation.

**Direction:** Parse codec headers where available; wait for Symphonia; or **remove** tiebreaker until data exists. Do **after** dual-track policy revision.

**References:** `src/infrastructure/symphonia/media_reader.rs`, `src/domain/policies.rs`

---

### Duration-less files at open

**Status:** Partially addressed. `scan_container_audio_duration`, chapter metadata, and corpus case `mp3_no_duration_tag` exist. Some edge cases may still fail at open.

**Problem:** End-clip windows need non-zero duration. Files that decode but lack track duration metadata historically failed early.

**Impact:** Valid-ish inputs rejected instead of degrading gracefully.

**Direction:** Audit remaining failures; relax open when decodable; fail at clip planning if duration still unknown after scan.

**References:** `src/infrastructure/symphonia/media_reader.rs`, `tests/corpus/manifest.toml` (`mp3_no_duration_tag`)

---

### Chromaprint “no match” vs zero-confidence success

**Problem:** Aligner returns `Ok` with zero confidence when no segment matches. `AlignmentError::NoMatch` / `AmbiguousMatch` are documented but unused (`dead_code`). Ambiguity only downgrades confidence.

**Impact:** Failure modes conflated; error-mapping doc over-promises.

**Direction:** Pick one contract (low-confidence `Ok` vs engine errors); wire variants; adapter tests. Do when JSON/failure model is ready to freeze.

**References:** `src/infrastructure/chromaprint/aligner.rs`, `docs/error-mapping.md`

---

### Memory use and PCM cloning on long clips

**Problem:** 15-minute default clips hold full PCM in memory; multiple clones along prepare/align path. No streaming fingerprinting.

**Impact:** Hundreds of MB on long multi-clip runs; structural ceiling.

**Direction:** Document expectations in PLAN; reduce clones (`Cow`, in-place prep); long-term chunked fingerprint if API allows.

**References:** `src/application/align_videos.rs`, `src/domain/pcm_preparation.rs`

---

## Low priority

### Silent resample fallback

**Problem:** `resample_mono_pcm` silently falls back to linear interpolation when `rubato` fails. No log.

**Direction:** `warn` on fallback; natural at infrastructure boundary after resample port move.

**References:** `src/domain/resample.rs`

---

### Stringly-typed port errors

**Problem:** Port errors use free-form `String` details; no `source()` chains for Symphonia context.

**Direction:** Structured sub-enums where categories repeat; keep display strings for stderr.

**References:** `src/application/error.rs`, `src/infrastructure/symphonia/error_mapping.rs`

---

### Type and dependency polish

**Problem:** `Fingerprint` is bare `Vec<u32>`; `PartialEq` on float estimates; unused `anyhow` in `Cargo.toml`; sub-second truncation in subclip selection and config serde.

**Direction:** Newtype `Fingerprint`; remove `anyhow`; document sub-second config limit.

**References:** `src/domain/alignment.rs`, `Cargo.toml`, `src/application/config.rs`

---

### Binary-only crate (no `lib.rs`)

**Problem:** Binary-only crate; composition root not reusable for integration tests or embedding.

**Direction:** Add `lib.rs` with thin `main`; optional `clip_sync::run(config)`.

**References:** `src/main.rs`, `src/infrastructure/cli/mod.rs`

---

### Test helper cross-layer coupling

**Problem:** Infrastructure tests import `application::testing::ffmpeg_util`.

**Direction:** Move shared helpers to `tests/support/` when splitting `media_reader` tests.

**References:** `src/infrastructure/symphonia/media_reader.rs`, `src/application/testing/`

---

### Documentation drift (PLAN vs code)

**Problem:** PLAN `num_clips` default vs code; incomplete domain error list; domain purity claim vs `rubato`.

**Direction:** Audit PLAN after track policy and architecture refactors land.

**References:** `PLAN.md`, `src/application/config.rs`

---

### Log file appender

**Problem:** `--log-file` parsed but not implemented; init warns users.

**Direction:** `tracing-appender` file layer in `src/infrastructure/logging/mod.rs`.

**References:** `src/infrastructure/logging/mod.rs`

---

### Committed test fixtures

**Problem:** Tier B is 3 WAV pairs; encoded formats need ffmpeg at test time.

**Direction:** Optional tiny committed MP3 for CI without ffmpeg; document required features in corpus README.

**References:** `tests/corpus/`, `Cargo.toml` features

---

## Completed

### Phase 1 — Dual-track default track selection

**Done:** `select_best_track` returns the first decodable track in container order (no sample-rate / channel ranking). Unit tests + `mp4_dual_track_wrong_default` corpus case updated. `--try-all-tracks` still available when program is not muxed first.

**References:** `src/domain/policies.rs`, `tests/corpus/manifest.toml`, `PLAN.md`, `docs/corpus-validation.md`

---

### Phase 1 — Silent decode degradation

**Done:** `DecodeError` skips logged at `debug`; aggregate `warn` when extract completes with skips; fail after 64 consecutive decode errors. Complements existing `decode_shortfall_limit`.

**Remaining:** surface skip count in `--verbose` / JSON (optional follow-up).

**References:** `src/infrastructure/symphonia/media_reader.rs`

---

### Session reuse / re-probe

**Done:** One probe per file per run; format reader + per-track decoders reused across extracts. Shared `open_format_reader` + `probe_from_format`. `two_clip_consistent` has `max_wall_secs = 30`.

**Remaining:** sorted-window extraction (medium priority).

**References:** [docs/archive/session-reuse-plan.md](docs/archive/session-reuse-plan.md)

---

### Multi-track documentation (`try_all_tracks`)

**Done:** CLI `--try-all-tracks`, config `alignment.try_all_tracks`, PLAN and [corpus-validation.md](docs/corpus-validation.md) document dual-track behaviour and corpus cases (`mp4_dual_track_decoy`, `mp4_dual_track_wrong_default`).

**Remaining:** default track **policy** (high priority above).

---

### Probe deduplication at open

**Done:** `probe_media_reusable` at `open`; session path shares probe helpers with standalone probe.

---

### Decode shortfall limits

**Done:** Extract fails when decoded sample count falls far below expected window (`decode_shortfall_limit`), with tail-padding tolerance on long clips. Does not replace decode-skip logging.

**References:** `src/infrastructure/symphonia/media_reader.rs`

---

## Explicitly out of scope (initial version)

From [PLAN.md](PLAN.md) — not backlog unless scope changes:

- Video frame / visual sync
- Batch processing (> two files)
- Writing aligned output files (report offset only)
- Network or streaming sources
