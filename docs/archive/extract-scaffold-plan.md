# Symphonia extract shared decode scaffold

> **Status:** Shipped (2026-06-09). All three phases complete. Backlog entry updated.

**Problem:** `extract_mono_with_state` and `extract_interleaved_with_state` in `crates/clip-sync/src/infrastructure/symphonia/extract.rs` duplicated ~300 lines of seek/retry, packet iteration, decode-skip fail-fast, window-boundary filtering, progress callbacks, shortfall/tail-padding, and logging. R1 intentionally mirrored mono so interleaved could ship quickly; fixes on one path did not propagate to the other.

**Goal:** Extract the shared decode loop into one inner driver. Mono and interleaved differ **only** at the append/sink step (and in the final DTO they build). **No behaviour change** — existing `media_reader_tests` and repair integration tests stay green byte-for-byte on PCM output.

**Prerequisite (done):** per-extract `Vec<f32>` scratch buffer passed into `append_*` (lib extract hardening slice 1).

**Out of scope (deferred):** plane-direct Symphonia reads (skip `copy_to_vec_interleaved`); chunked/streaming full-timeline extract for repair memory; changes to public `MediaSession` port signatures.

**References:** [repair-write-path-plan.md](repair-write-path-plan.md) § Lib extract hardening, [BACKLOG.md](../../BACKLOG.md) § Symphonia extract loop hardening, `extract.rs`, `extract_loop.rs`, `media_reader_tests.rs`.

---

## Decisions

| Topic | Decision |
|-------|----------|
| **Behaviour** | Refactor only. PCM output, error paths, decode-skip counts, tail-padding limits, and progress cadence match current code. |
| **Location** | Driver + sinks live in **`extract_loop.rs`** (`pub(super)`), wired from `extract.rs` via `mod extract_loop;`. Append helpers and thin wrappers stay in `extract.rs`. |
| **Abstraction** | Generic **sink trait** (`ExtractSink`) invoked by one `run_extract_decode_loop` driver. |
| **Sink responsibilities** | Buffer allocation/reserve, first-decode metadata discovery (rate/channels), per-packet append, progress unit counting, finalize into `MonoPcmClip` or `MultiChannelPcm` (**each sink owns its truncate/count/progress order — see duplication map**). |
| **Shared driver responsibilities** | Empty-window guard, decoder ensure, `ExtractLoopParams`, seek+retry attempts, packet read/decode/error handling via `read_next_packet` / `packet_window_pos` / `decode_packet_or_skip`, `finished` flag, `allow_tail_padding`, `decode_error_skips`, call sink append, invoke progress when sink reports delta. |
| **Append functions** | Keep `append_frames_in_window` and `append_interleaved_frames_in_window` as separate functions; sinks delegate to them. |
| **Tests** | No new corpus cases. `extract_mono_decode_errors_are_counted` (Phase 1); `mono_and_interleaved_same_skip_count_on_corrupt_fixture` (Phase 2). |

---

## Duplication map

Both entry points follow the same skeleton:

```text
1. window.end <= window.start → error
2. ensure_track_decoder
3. resolve time_base + sample_rate hint from format/track
4. scratch Vec::new(); output buffer; resolved_rate; target count; decode_error_skips; allow_tail_padding
5. for attempt in 0..max_attempts (2 when window.start > 0, else 1):
     seek_to_window_start + decoder.reset
     reset output buffer + target estimate + debug log
     loop packets:
       break if finished / target reached
       next_packet → EOF / ResetRequired / error handling
       skip wrong track_id
       packet vs window bounds (sample path or duration path)
       decode → skip corrupt / fail-fast / EOF / reset
       skip zero-frame buffers
       lazy resolve rate (and channels for interleaved)
       compute packet_start_sample, trim_start_frames
       append_*_frames_in_window → finished flag
       progress every ~0.5 s of units
     break attempts if output non-empty
6. require resolved rate
7. truncate to target
8. empty → error
9. shortfall vs decode_shortfall_limit → pad or error
10. warn on decode_error_skips; log success; build DTO
```

| Concern | Mono | Interleaved |
|---------|------|-------------|
| Output buffer | `mono_samples: Vec<i16>` | `out: Vec<i16>` interleaved |
| Target unit | samples (mono frames) | frames (`out.len() / channels`) |
| Channel discovery | downmix in `append_frames_in_window` | `channels` from track hint or first decode |
| Progress denominator | `target_samples` | `target_frames` |
| DTO | `MonoPcmClip` | `MultiChannelPcm` |
| `decoded_*_count` field | `decoded_sample_count` | `decoded_frame_count` |
| **Finalize count timing** | Capture `decoded_sample_count` **after** `truncate(target)` | Capture `decoded_frame_count` **before** `truncate`; final progress uses `min(target, pre-truncate count)` |

---

## Phases (all complete)

### Phase 0 — Line mapping ✅
- [x] Document duplication map and sink boundaries.

### Phase 1 — Driver + mono sink ✅
- `extract_loop.rs` created with `ExtractLoopParams`, `ExtractSink`, `MonoExtractSink`, `run_extract_decode_loop`.
- `extract_mono_with_state` replaced with 6-line thin wrapper.
- `extract_mono_decode_errors_are_counted` test added.

### Phase 2 — Interleaved sink ✅
- `InterleavedExtractSink` added; interleaved finalize order (pre-truncate count) preserved verbatim.
- `extract_interleaved_with_state` replaced with 6-line thin wrapper.
- `mono_and_interleaved_same_skip_count_on_corrupt_fixture` test added.

### Phase 3 — Optional helpers + docs ✅
- `read_next_packet` — `next_packet` + `ResetRequired` retry loop + error mapping.
- `packet_window_pos` — sample-based then duration-fallback window classification.
- `decode_packet_or_skip` — corrupt skip, consecutive-error cap, EOF, `NeedReset` signal (caller reborrows state for reset).
- BACKLOG.md updated; plan archived here.

---

## Tests delivered

| Test | Crate | Asserts |
|------|-------|---------|
| All existing `media_reader_tests` (mono + interleaved) | lib | No regressions — 130/130 green |
| All existing `append_*` unit tests | lib | Append boundary behaviour unchanged |
| `extract_mono_decode_errors_are_counted` | lib | Mono sink counts skips identically to interleaved path |
| `mono_and_interleaved_same_skip_count_on_corrupt_fixture` | lib | Both sinks report identical `decode_error_skips` on same damaged WAV |
| `patch_audio_fills_gap_in_stereo_wav` | repair | Full-timeline interleaved extract still patches gap |

---

## Non-goals (unchanged)

- Merging `append_frames_in_window` and `append_interleaved_frames_in_window`.
- Reading Symphonia audio planes without `copy_to_vec_interleaved`.
- Changing `MAX_CONSECUTIVE_DECODE_ERRORS`, `decode_shortfall_limit`, or seek-retry policy.
- Exposing the sink trait outside `symphonia` infrastructure.
