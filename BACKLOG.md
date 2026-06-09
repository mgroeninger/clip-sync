# Backlog

Open follow-up work for `clip-sync`. See [PLAN.md](PLAN.md) for architecture, [docs/corpus-validation.md](docs/corpus-validation.md) for the test corpus, and [docs/error-mapping.md](docs/error-mapping.md) for error handling.

Last updated: 2026-06-08.

Each item includes **problem**, **impact**, **direction**, and **references**. Temporary implementation plans live under `docs/TEMP-*.md`.

---

## Recommended order of work

Phased sequence based on impact, risk, and dependencies. Items within a phase can overlap; do not start later phases until earlier correctness work is merged.

### Phase 1 — Silent wrong answers ✅

**Done (2026-06-06):** first decodable track selection; decode skip logging + consecutive-error fail-fast.

Next: [Phase 2](#phase-2--maintainability-and-perf-do-next).

### Phase 2 — Maintainability and perf ✅

**Done (2026-06-06):** split symphonia into `duration`, `probe`, `session`, `extract`, `media_reader_tests`; chronological extract order in `extract_clips`.

Next: [Phase 4](#phase-4--edge-cases-and-semantics).

| # | Item | Status |
|---|------|--------|
| 5 | [Large-offset alignment accuracy](#large-offset-alignment-accuracy) | Done |

### Phase 3 — Large-offset alignment ✅

**Done (2026-06-06):** PCM template discover near coarse Chromaprint estimate (downsampled search + full-rate refine on coarse hint and top peaks); dynamic `pcm_lag_adjustment` range on long clips; corpus cases at 15 / 30 / 60 s leaders.

**Also done (2026-06-06):** [High-rate hold-out refinement](#high-rate-hold-out-refinement) — native-rate FFT pass for sub-50 ms residual correction; corpus `wav_high_rate_refine_3s`. Plan: [archive/high-rate-offset-refinement-plan.md](docs/archive/high-rate-offset-refinement-plan.md).

Next: [Phase 4](#phase-4--edge-cases-and-semantics).

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

### Repair write path (migration Phase 5 → R0–R5) — **shipped**

**Status:** R0–R5 complete (2026-06-09). Archived plan: [docs/archive/repair-write-path-plan.md](docs/archive/repair-write-path-plan.md) (live copy: [docs/TEMP-repair-write-path-plan.md](docs/TEMP-repair-write-path-plan.md)).

| Phase / slice | Scope | Crate | Status |
|---------------|--------|-------|--------|
| **R0–R1** | `MultiChannelPcm`, `extract_interleaved`, `resample_interleaved`, `TimelineOverlap` re-export | lib | ✅ Done |
| **R2** | Track compatibility, overlap, alignment gate (`Option<f64>` B fields), CLI polish | repair | ✅ Done |
| **Lib extract hardening** | Scratch buffer reuse; optional shared mono/interleaved decode scaffold | lib | ✅ Scratch done; scaffold deferred |
| **R3** | Bidirectional silence scan + `gap_offset_agreement` | repair | ✅ Done |
| **R4** | `PatchAudio`, gap fill, multi-channel WAV | repair | ✅ Done |
| **R5** | `RepairVideos` + ffmpeg mux (`ffmpeg-mux` feature) | repair | ✅ Done |

**Open follow-ups:** shared decode scaffold ([TEMP-extract-scaffold-plan.md](docs/TEMP-extract-scaffold-plan.md)), `--dry-run`/`--write` CLI flags, scratch-buffer regression test, streaming WAV encode.

**Prerequisite:** [Workspace repair Phase 4](#workspace-repair-phase-4-report-only) (shipped).

### Phase 6 — Architecture cleanup (when feature velocity slows)

| # | Item |
|---|------|
| 11 | [Architecture layer leaks](#architecture-domain-and-application-layer-leaks) |
| 12 | [`MediaSession` interior mutability](#mediasession-interior-mutability) — pair with `session.rs` split |
| 13 | [Documentation drift (PLAN vs code)](#documentation-drift-plan-vs-code) — after policy decisions land |

### Defer / opportunistic

- [Memory use and PCM cloning](#memory-use-and-pcm-cloning-on-long-clips) — document order-of-magnitude first; optimize when users report pain
- [Log file appender](#log-file-appender), [committed test fixtures](#committed-test-fixtures), [test helper cross-layer coupling](#test-helper-cross-layer-coupling) — as CI/support needs arise
- [Type and dependency polish](#type-and-dependency-polish), [stringly-typed port errors](#stringly-typed-port-errors), [silent resample fallback](#silent-resample-fallback) — incremental when touching those files

---

## High priority

---

### Large-offset alignment accuracy

**Status:** Done (2026-06-06).

**Problem:** Chromaprint degrades when leader offset is large relative to clip length. True **+30 s** on 60 s clips measured ~16 s; PCM refinement only adjusted ±1 s around coarse estimate.

**Resolution:** `pcm_discover_offset` in `offset_refinement.rs` — template match in a window around the coarse estimate (downsampled scan + full-rate refine on coarse hint and top peaks); expanded `pcm_lag_adjustment` cap on long clips. Corpus: `wav_leader_15s`, `wav_leader_30s`, `wav_leader_60s`.

**References:** `crates/clip-sync/src/infrastructure/chromaprint/aligner.rs`, `crates/clip-sync/src/application/offset_refinement.rs`, `tests/corpus/manifest.toml`

---

### High-rate hold-out refinement

**Status:** Done (2026-06-06).

**Problem:** Discovery alignment (Chromaprint + 11 kHz PCM refine) can leave 20–50 ms residual error — audible as echo when tracks are overlaid.

**Resolution:** Optional `apply_high_rate_refinement` after alignment: re-extract a short hold-out at native decode rate, FFT cross-correlate at lag ≈ 0, apply small adjustment capped at 0.1 s. Off by default; enable with `--refine-offset-high-rate` or `refine_offset_high_rate = true`. Corpus: `wav_high_rate_refine_3s` (±50 ms at 44.1 kHz).

**References:** `crates/clip-sync/src/application/high_rate_refinement.rs`, `crates/clip-sync/src/application/offset_refinement.rs`, `crates/clip-sync/src/domain/policies.rs`, [docs/archive/high-rate-offset-refinement-plan.md](docs/archive/high-rate-offset-refinement-plan.md)

---

## Medium priority

### Symphonia extract loop hardening

**Status:** Scratch buffer done (2026-06-08); shared decode scaffold not started. Plan: [TEMP-extract-scaffold-plan.md](docs/TEMP-extract-scaffold-plan.md) (authoritative); summary in [docs/archive/repair-write-path-plan.md](docs/archive/repair-write-path-plan.md) § Lib extract hardening.

**Problem:** `extract_mono_with_state` and `extract_interleaved_with_state` still duplicate ~300 lines of seek/retry/decode-skip logic (R1 intentionally mirrored mono). Per-packet scratch reuse is shipped; duplicated loop bodies remain.

**Impact:** Future mono-path fixes may not propagate to interleaved without manual mirroring.

**Direction:**

1. ~~**Scratch buffer:**~~ done — one `Vec<f32>` per extract loop in `extract.rs`.
2. **Shared decode-loop scaffold (next):** mono vs interleaved differ only at append/sink.
3. **Defer:** plane-direct Symphonia reads instead of `copy_to_vec_interleaved`; dedicated `append_*_reuses_scratch_buffer` test.

**References:** `crates/clip-sync/src/infrastructure/symphonia/extract.rs`, `media_reader_tests.rs`

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

**References:** `docs/TEMP-clip-self-repetition-plan.md`, `crates/clip-sync/src/infrastructure/chromaprint/aligner.rs`, `crates/clip-sync/src/domain/alignment.rs`

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

**References:** `docs/TEMP-offset-verification-plan.md`, `crates/clip-sync/src/application/align_videos.rs`

---

### Architecture: domain and application layer leaks

**Problem:** PLAN says domain has no external deps; code uses `rubato` in `domain/resample.rs`, `cross_correlate` in `application/offset_refinement.rs`, and `serde::Serialize` on domain report types. Resample/refinement not behind ports.

**Impact:** Harder to test domain in isolation; PLAN misleads contributors.

**Direction:** `Resampler` and `OffsetRefiner` ports; move `Serialize` to application/infrastructure DTOs; update PLAN. Do after `media_reader` split to reduce parallel churn.

**References:** `crates/clip-sync/src/domain/resample.rs`, `crates/clip-sync/src/application/offset_refinement.rs`, `crates/clip-sync/src/application/ports.rs`, `PLAN.md`

---

### `MediaSession` interior mutability

**Problem:** `SymphoniaMediaSession` uses `RefCell<Option<MediaIoState>>`; `extract_mono(&self)` mutates internally. Production code uses `expect("decoder cached")` for invariants.

**Impact:** Surprising API, not `Sync`, brittle refactors.

**Direction:** `extract_mono(&mut self, …)` on port trait (breaking); or explicit mutable session handle. Pair with `session.rs` extraction. Replace `expect()` with `MediaError` returns.

**References:** `crates/clip-sync/src/infrastructure/symphonia/session.rs`, `crates/clip-sync/src/application/ports.rs`

---

### Bitrate for track selection

**Problem:** `AudioTrack.bitrate` always `None` at probe; `select_best_track` bitrate tiebreaker is dead code.

**Impact:** Misleading policy; missed signal for dual-track disambiguation.

**Direction:** Parse codec headers where available; wait for Symphonia; or **remove** tiebreaker until data exists. Do **after** dual-track policy revision.

**References:** `crates/clip-sync/src/infrastructure/symphonia/probe.rs`, `crates/clip-sync/src/domain/policies.rs`

---

### Duration-less files at open

**Status:** Partially addressed. `scan_container_audio_duration`, chapter metadata, and corpus case `mp3_no_duration_tag` exist. Some edge cases may still fail at open.

**Problem:** End-clip windows need non-zero duration. Files that decode but lack track duration metadata historically failed early.

**Impact:** Valid-ish inputs rejected instead of degrading gracefully.

**Direction:** Audit remaining failures; relax open when decodable; fail at clip planning if duration still unknown after scan.

**References:** `crates/clip-sync/src/infrastructure/symphonia/session.rs`, `tests/corpus/manifest.toml` (`mp3_no_duration_tag`)

---

### Chromaprint “no match” vs zero-confidence success

**Problem:** Aligner returns `Ok` with zero confidence when no segment matches. `AlignmentError::NoMatch` / `AmbiguousMatch` are documented but unused (`dead_code`). Ambiguity only downgrades confidence.

**Impact:** Failure modes conflated; error-mapping doc over-promises.

**Direction:** Pick one contract (low-confidence `Ok` vs engine errors); wire variants; adapter tests. Do when JSON/failure model is ready to freeze.

**References:** `crates/clip-sync/src/infrastructure/chromaprint/aligner.rs`, `docs/error-mapping.md`

---

### Memory use and PCM cloning on long clips

**Problem:** 15-minute default clips hold full PCM in memory; multiple clones along prepare/align path. No streaming fingerprinting.

**Impact:** Hundreds of MB on long multi-clip runs; structural ceiling.

**Direction:** Document expectations in PLAN; reduce clones (`Cow`, in-place prep); long-term chunked fingerprint if API allows.

**References:** `crates/clip-sync/src/application/align_videos.rs`, `crates/clip-sync/src/domain/pcm_preparation.rs`

---

## Low priority

### Silent resample fallback

**Problem:** `resample_mono_pcm` silently falls back to linear interpolation when `rubato` fails. No log.

**Direction:** `warn` on fallback; natural at infrastructure boundary after resample port move.

**References:** `crates/clip-sync/src/domain/resample.rs`

---

### Stringly-typed port errors

**Problem:** Port errors use free-form `String` details; no `source()` chains for Symphonia context.

**Direction:** Structured sub-enums where categories repeat; keep display strings for stderr.

**References:** `crates/clip-sync/src/application/error.rs`, `crates/clip-sync/src/infrastructure/symphonia/error_mapping.rs`

---

### Type and dependency polish

**Problem:** `Fingerprint` is bare `Vec<u32>`; `PartialEq` on float estimates; unused `anyhow` in `Cargo.toml`; sub-second truncation in subclip selection and config serde.

**Direction:** Newtype `Fingerprint`; remove `anyhow`; document sub-second config limit.

**References:** `crates/clip-sync/src/domain/alignment.rs`, `crates/clip-sync/Cargo.toml`, `crates/clip-sync/src/application/config.rs`

---

### Test helper cross-layer coupling

**Problem:** Infrastructure tests import `application::testing::ffmpeg_util`.

**Direction:** Move shared helpers to `tests/support/` when splitting `media_reader` tests.

**References:** `crates/clip-sync/src/infrastructure/symphonia/`, `crates/clip-sync/src/application/testing/`

---

### Documentation drift (PLAN vs code)

**Problem:** PLAN `num_clips` default vs code; incomplete domain error list; domain purity claim vs `rubato`.

**Direction:** Audit PLAN after track policy and architecture refactors land.

**References:** `PLAN.md`, `crates/clip-sync/src/application/config.rs`

---

### Log file appender

**Problem:** `--log-file` parsed but not implemented; init warns users.

**Direction:** `tracing-appender` file layer in `crates/clip-sync/src/infrastructure/logging/mod.rs`.

**References:** `crates/clip-sync/src/infrastructure/logging/mod.rs`

---

### Committed test fixtures

**Problem:** Tier B is 3 WAV pairs; encoded formats need ffmpeg at test time.

**Direction:** Optional tiny committed MP3 for CI without ffmpeg; document required features in corpus README.

**References:** `tests/corpus/`, `Cargo.toml` features

---

## Completed

### Workspace extraction (Phases 1–2)

**Done (2026-06-07):** Single binary crate restructured into a three-crate Cargo workspace: `crates/clip-sync` (alignment library), `crates/clip-sync-cli` (analyzer binary), workspace root `Cargo.toml`. Facade `lib.rs` is the only public surface of the library. `AlignConfig` split from `AppConfig`; `LoggingConfig` moved to `infrastructure::logging`. Corpus path uses `CLIP_SYNC_WORKSPACE_ROOT` env override + `../..` from `CARGO_MANIFEST_DIR`.

**References:** [docs/archive/workspace-refactor-plan.md](docs/archive/workspace-refactor-plan.md), `crates/clip-sync/src/lib.rs`, `crates/clip-sync-cli/`

---

### Workspace repair Phase 4 (report-only)

**Done (2026-06-08):** `clip-sync-repair` crate shipped — `ScanGaps`, `GapReport`, `GapReporter`, `RepairError`, CLI gap report (human + JSON). Integration test via `crates/clip-sync-repair/tests/scan_gaps_integration.rs`.

**References:** [docs/archive/workspace-refactor-gaps.md](docs/archive/workspace-refactor-gaps.md), `crates/clip-sync-repair/`, [docs/error-mapping.md](docs/error-mapping.md) (repair exit codes)

---

### Repair write path R0–R1 (lib native extraction)

**Done (2026-06-08):** `MultiChannelPcm`, `MediaSession::extract_interleaved`, `SymphoniaMediaSession` impl, `MediaError::Unsupported`, facade re-exports (`MultiChannelPcm`, `TimelineOverlap`, `resample_interleaved`, `resample_mono_pcm`). Lib tests in `media_reader_tests.rs`.

**Known follow-up:** [Symphonia extract loop hardening](#symphonia-extract-loop-hardening) (not part of R1 scope).

**References:** `crates/clip-sync/src/domain/multichannel_pcm.rs`, `crates/clip-sync/src/infrastructure/symphonia/extract.rs`, [docs/archive/repair-write-path-plan.md](docs/archive/repair-write-path-plan.md)

---

### Repair write path R2 (track match + overlap + alignment gate)

**Done (2026-06-08):** `domain/track_match.rs`, `GapReport.track_compatibility` + `overlap`, best-effort B open, `Gap.video_b_*: Option<f64>`, alignment gate in `scan_gaps`, CLI human/JSON (tracks, overlap, B-mapping-skipped note, `is_fillable()` labels), tests for failed-alignment JSON nulls and human note.

**References:** `crates/clip-sync-repair/src/domain/{track_match,gap}.rs`, `scan_gaps.rs`, `infrastructure/cli/output.rs`

---

### Repair write path R3 (bidirectional scan + cross-check)

**Done (2026-06-08):** `scan_both`, `domain/cross_check.rs` (`silence_based_offset`, `GapOffsetAgreement`), CLI human/JSON agreement line, unit tests in `domain/cross_check.rs`.

**References:** `crates/clip-sync-repair/src/domain/cross_check.rs`, `scan_gaps.rs`, `infrastructure/cli/output.rs`

---

### Repair write path R4 (patch audio + WAV)

**Done (2026-06-08):** `PatchAudio`, gap fill policies, `WavPatchedAudioWriter`, `--wav` CLI, integration tests in `patch_audio_integration.rs` and `cli_wav_integration.rs`.

**References:** `crates/clip-sync-repair/src/application/patch_audio.rs`, `infrastructure/wav_writer.rs`, [docs/archive/repair-write-path-plan.md](docs/archive/repair-write-path-plan.md) § R4

---

### Repair write path R5 (ffmpeg mux)

**Done (2026-06-09):** `RepairVideos`, `FfmpegMediaMuxer` behind `ffmpeg-mux` feature, `--mux` CLI, exit code 6, `cli_mux_integration.rs` + `ffmpeg_mux` unit tests.

**References:** `crates/clip-sync-repair/src/application/repair_videos.rs`, `infrastructure/ffmpeg_mux.rs`, [docs/error-mapping.md](docs/error-mapping.md)

---

### Phase 1 — Dual-track default track selection

**Done:** `select_best_track` returns the first decodable track in container order (no sample-rate / channel ranking). Unit tests + `mp4_dual_track_wrong_default` corpus case updated. `--try-all-tracks` still available when program is not muxed first.

**References:** `crates/clip-sync/src/domain/policies.rs`, `tests/corpus/manifest.toml`, `PLAN.md`, `docs/corpus-validation.md`

---

### Phase 1 — Silent decode degradation

**Done:** `DecodeError` skips logged at `debug`; aggregate `warn` when extract completes with skips; fail after 64 consecutive decode errors. Complements existing `decode_shortfall_limit`. Skip counts on `ClipMatch` (`video_a_decode_skips`, `video_b_decode_skips`) in JSON; human lines when `--verbose` / `show_diagnostics`.

**References:** `crates/clip-sync/src/infrastructure/symphonia/extract.rs`, `crates/clip-sync/src/domain/alignment.rs`, `crates/clip-sync-cli/src/infrastructure/cli/output.rs`

---

### Phase 2 — Split `media_reader.rs`

**Done:** `duration.rs`, `probe.rs`, `session.rs`, `extract.rs`, `media_reader_tests.rs`; `mod.rs` re-exports `SymphoniaMediaReader`.

**References:** `crates/clip-sync/src/infrastructure/symphonia/`

---

### Phase 2 — Sorted-window extraction

**Done:** `extract_clips` sorts windows by start time before decode; results mapped back to clip index. Progress message when order differs from plan order.

**References:** `crates/clip-sync/src/application/align_videos.rs`

---

### Session reuse / re-probe

**Done:** One probe per file per run; format reader + per-track decoders reused across extracts. Shared `open_format_reader` + `probe_from_format`. `two_clip_consistent` has `max_wall_secs = 30`.

**References:** [docs/archive/session-reuse-plan.md](docs/archive/session-reuse-plan.md)

---

### Multi-track documentation (`try_all_tracks`)

**Done:** CLI `--try-all-tracks`, config, PLAN, corpus-validation; default track policy fixed (Phase 1).

**References:** `docs/corpus-validation.md`

---

### Probe deduplication at open

**Done:** `probe_media_reusable` at `open`; session path shares probe helpers with standalone probe.

---

### Decode shortfall limits

**Done:** Extract fails when decoded sample count falls far below expected window (`decode_shortfall_limit`), with tail-padding tolerance on long clips. Does not replace decode-skip logging.

**References:** `crates/clip-sync/src/infrastructure/symphonia/session.rs`

---

## Explicitly out of scope (initial version)

From [PLAN.md](PLAN.md) — not backlog unless scope changes:

- Video frame / visual sync
- Batch processing (> two files)
- Writing aligned output files from the **analyzer** (report offset only)
- Patched repair output is **`clip-sync-repair`** only — see [docs/archive/repair-write-path-plan.md](docs/archive/repair-write-path-plan.md)
- Network or streaming sources
