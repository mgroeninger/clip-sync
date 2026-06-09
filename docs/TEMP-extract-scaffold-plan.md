# Temporary plan: Symphonia extract shared decode scaffold

> **Status:** Not started (2026-06-08). Scratch-buffer reuse shipped; scaffold open. Archive to `docs/archive/extract-scaffold-plan.md` when shipped.

**Problem:** `extract_mono_with_state` and `extract_interleaved_with_state` in `crates/clip-sync/src/infrastructure/symphonia/extract.rs` duplicate ~300 lines of seek/retry, packet iteration, decode-skip fail-fast, window-boundary filtering, progress callbacks, shortfall/tail-padding, and logging. R1 intentionally mirrored mono so interleaved could ship quickly; fixes on one path do not propagate to the other.

**Goal:** Extract the shared decode loop into one inner driver. Mono and interleaved differ **only** at the append/sink step (and in the final DTO they build). **No behaviour change** — existing `media_reader_tests` and repair integration tests must stay green byte-for-byte on PCM output where feasible.

**Prerequisite (done):** per-extract `Vec<f32>` scratch buffer passed into `append_*` (lib extract hardening slice 1).

**Out of scope (defer):** plane-direct Symphonia reads (skip `copy_to_vec_interleaved`); chunked/streaming full-timeline extract for repair memory; changes to public `MediaSession` port signatures.

**References:** [docs/archive/repair-write-path-plan.md](archive/repair-write-path-plan.md) § Lib extract hardening, [BACKLOG.md](../BACKLOG.md) § Symphonia extract loop hardening, `extract.rs`, `media_reader_tests.rs`.

---

## Decisions

Locked before implementation. Change only with an explicit plan revision.

| Topic | Decision |
|-------|----------|
| **Behaviour** | Refactor only. PCM output, error paths, decode-skip counts, tail-padding limits, and progress cadence must match current code. |
| **Location** | New driver + sinks live in **`extract_loop.rs`** (`pub(crate)`), wired from `extract.rs` via `mod extract_loop;`. `extract.rs` is already ~1850 lines — do not grow it further with the scaffold. Append helpers and thin wrappers stay in `extract.rs`. |
| **Abstraction** | Generic **sink trait** (`ExtractSink`) invoked by one `run_extract_decode_loop` driver. Avoid duplicating the packet `loop` again. |
| **Sink responsibilities** | Buffer allocation/reserve, first-decode metadata discovery (rate/channels), per-packet append, progress unit counting, finalize into `MonoPcmClip` or `MultiChannelPcm` (**each sink owns its truncate/count/progress order — see duplication map**). |
| **Shared driver responsibilities** | Empty-window guard, decoder ensure, populate `ExtractLoopParams` (`track_id`, `time_base`, sample-rate hint, `max_attempts`), seek+retry attempts, packet read/decode/error handling, window skip/break, `finished` flag, `allow_tail_padding`, `decode_error_skips`, call sink append, invoke progress when sink reports delta. |
| **Append functions** | Keep `append_frames_in_window` and `append_interleaved_frames_in_window` as separate functions; sinks delegate to them. Do not merge append paths in v1. |
| **Tests** | No new corpus cases. Rely on existing `media_reader_tests` + `append_*` unit tests. **Phase 1:** add mono decode-skip coverage (interleaved already has `extract_interleaved_decode_errors_are_counted`; mono has none). **Phase 2:** add `mono_and_interleaved_same_skip_count_on_corrupt_fixture` so both sinks prove identical skip counts on the same damaged file. |
| **Rollout** | Land behind incremental PRs: introduce driver + mono sink first (delete mono duplicate), then interleaved sink (delete interleaved duplicate). One PR acceptable if diff is reviewable. |

---

## Current duplication map

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

**Do not unify finalize order** — sinks must copy each path's truncate/count/progress sequence verbatim.

**Already shared (keep):** `seek_to_window_start`, `window_sample_bounds`, `decode_shortfall_limit`, `sample_count_tolerance`, `float_to_i16`, `append_*` helpers, scratch buffer plumbing.

---

## Proposed shape

### Shared loop state

```rust
/// Inputs and mutable driver state for one extract attempt sequence.
struct ExtractLoopParams<'a> {
    path: &'a Path,
    state: &'a mut MediaIoState,
    track: &'a AudioTrack,
    track_id: u32,              // track.index — fixed for the whole extract
    time_base: Option<TimeBase>, // from cached decoder after ensure_track_decoder
    window: &'a ClipWindow,
    progress: &'a dyn ProgressReporter,
    label: &'a str,
    sample_rate_hint: Option<u32>,
    max_attempts: usize,
}

struct ExtractAttemptState {
    seek_start: Duration,
    allow_tail_padding: bool,
    decode_error_skips: u32,
    consecutive_decode_errors: u32,
    finished: bool,
    last_reported: u64,
}
```

### Sink trait

```rust
/// Per-output-mode hooks called by `run_extract_decode_loop`.
trait ExtractSink {
    /// Clear output and re-estimate capacity for a new seek attempt.
    fn reset_attempt(&mut self, window: &ClipWindow, rate_hint: Option<u32>);

    /// After first non-empty decode: set rate (and channels if needed). Returns target unit count.
    fn on_first_decode(
        &mut self,
        decoded: symphonia::core::audio::GenericAudioBufferRef<'_>,
        window: &ClipWindow,
    ) -> Result<usize, MediaError>;

    /// True when collected units >= target (driver may break packet loop).
    fn target_reached(&self) -> bool;

    /// Append in-window frames from `decoded`. Returns `true` when window end reached.
    fn append_packet(
        &mut self,
        decoded: symphonia::core::audio::GenericAudioBufferRef<'_>,
        packet_start_unit: u64,
        trim_start_frames: u32,
        window: &ClipWindow,
        scratch: &mut Vec<f32>,
    ) -> bool;

    /// Units collected so far (mono samples or interleaved frames).
    fn collected_units(&self) -> usize;

    /// Target units after `on_first_decode`; `None` until first decode.
    fn target_units(&self) -> Option<usize>;

    /// Resolved sample rate after first decode.
    fn resolved_rate(&self) -> Option<u32>;

    /// Build final clip or error after loop completes.
    fn finalize(
        self,
        params: &ExtractLoopParams<'_>,
        allow_tail_padding: bool,
        decode_error_skips: u32,
    ) -> Result</* MonoPcmClip or MultiChannelPcm via enum */, MediaError>;
}
```

**Implementation note:** Rust will not allow `finalize(self) -> MonoPcmClip` and `-> MultiChannelPcm` on the same trait without an enum wrapper or generic associated type. Preferred v1 approach:

```rust
trait ExtractSink {
    type Output;
    fn finalize(...) -> Result<Self::Output, MediaError>;
}

fn run_extract_decode_loop<S: ExtractSink>(
    params: ExtractLoopParams<'_>,
    sink: &mut S,
    scratch: &mut Vec<f32>,
) -> Result<S::Output, MediaError>;
```

`run_extract_decode_loop` owns the full shared skeleton (duplication map steps 1–10): empty-window guard through DTO build is either in the driver or delegated to `sink.finalize` — not split across the public wrapper.

Thin public wrappers in `extract.rs` remain:

```rust
pub(crate) fn extract_mono_with_state(...) -> Result<MonoPcmClip, MediaError> {
    let mut sink = MonoExtractSink::new(...);
    let mut scratch = Vec::new();
    extract_loop::run_extract_decode_loop(params, &mut sink, &mut scratch)
}

pub(crate) fn extract_interleaved_with_state(...) -> Result<MultiChannelPcm, MediaError> {
    let mut sink = InterleavedExtractSink::new(...);
    let mut scratch = Vec::new();
    extract_loop::run_extract_decode_loop(params, &mut sink, &mut scratch)
}
```

### Optional micro-extracts (low risk, can land before trait)

These shrink the duplicated `loop` body even if the full trait is phased:

| Helper | Responsibility |
|--------|----------------|
| `read_next_packet(state, path, track_id) -> Result<PacketOutcome, MediaError>` | `next_packet` + `ResetRequired` + map errors |
| `packet_before_window(...) -> bool` | sample-rate and duration fallback skip |
| `packet_past_window(...) -> bool` | sets `allow_tail_padding` semantics |
| `decode_packet_or_skip(...) -> Result<DecodeOutcome, MediaError>` | corrupt skip, consecutive cap, EOF, reset |

Land helpers only if they do not change control flow ordering.

---

## Phases

### Phase 0 — Line mapping (½ day, this document)

- [x] Document duplication map and sink boundaries.
- [ ] Annotate `extract.rs` with `// SHARED:` / `// MONO:` / `// INTERLEAVED:` comments on the two functions (optional, helps review).

### Phase 1 — Introduce driver + mono sink (1 day)

**Goal:** `extract_mono_with_state` becomes a thin wrapper; interleaved unchanged.

1. Add `extract_loop.rs`; `mod extract_loop;` in `symphonia/mod.rs`.
2. Add `ExtractLoopParams`, `ExtractAttemptState`, `MonoExtractSink`, `run_extract_decode_loop`.
3. Move **all** mono shared logic into the driver + sink — not only the inner `loop packets` block:
   - steps 1–3 (empty window, `ensure_track_decoder`, rate hint → populate `ExtractLoopParams` including `track_id` / `time_base`)
   - step 5 (seek/retry attempts, per-attempt reset, packet loop, in-loop progress)
   - steps 6–10 via `MonoExtractSink::finalize` (truncate order, shortfall, tail-padding, logging, `MonoPcmClip`)
4. Delete duplicated mono body from `extract.rs` (wrapper + scratch allocation only).
5. Add mono decode-skip test mirroring `extract_interleaved_decode_errors_are_counted`.
6. `cargo test -p clip-sync` — all green.

**Exit:** Mono path uses scaffold in `extract_loop.rs`; interleaved still duplicated in `extract.rs` but obviously parallel.

### Phase 2 — Interleaved sink + delete duplicate (½ day)

1. Add `InterleavedExtractSink` in `extract_loop.rs` (channels hint from `track.channels`).
2. Wire `extract_interleaved_with_state` through driver; preserve interleaved finalize order (pre-truncate count).
3. Remove duplicated interleaved body from `extract.rs`.
4. Add `mono_and_interleaved_same_skip_count_on_corrupt_fixture`.
5. `cargo test -p clip-sync` + `cargo test -p clip-sync-repair` (patch integration uses full-timeline interleaved extract).

**Exit:** Single packet loop in `run_extract_decode_loop`; duplicated lines removed from `extract.rs`.

### Phase 3 — Optional helpers + docs (½ day)

1. Extract `decode_packet_or_skip` / window bound helpers in `extract_loop.rs` if they improve readability without semantic drift.
2. Update [BACKLOG.md](../BACKLOG.md) and [docs/archive/repair-write-path-plan.md](archive/repair-write-path-plan.md) status to ✅ scaffold shipped.
3. Archive this plan to `docs/archive/extract-scaffold-plan.md`.

---

## Tests

| Test | Crate | Asserts |
|------|-------|---------|
| Existing `media_reader_tests` (mono + interleaved) | lib | No regressions on frame counts, seek windows, HE-AAC tolerance |
| Existing `append_*` unit tests | lib | Append boundary behaviour unchanged |
| `extract_mono_decode_errors_are_counted` *(new, Phase 1)* | lib | Mono sink counts skips like interleaved test today |
| `mono_and_interleaved_same_skip_count_on_corrupt_fixture` *(new, Phase 2)* | lib | Both sinks report identical `decode_error_skips` on same damaged file |
| `patch_audio_fills_gap_in_stereo_wav` | repair | Full-timeline interleaved extract still patches gap |

**Not required:** allocation-count test for scaffold (scratch-buffer test was optional and never added; defer unless profiling demands it).

---

## Risks and mitigations

| Risk | Mitigation |
|------|------------|
| Subtle behaviour drift (padding, shortfall, progress) | Side-by-side diff before/after on fixed WAV fixtures; keep finalize logic copied verbatim into sinks first, then simplify |
| **Mono vs interleaved finalize order** | Mono captures decoded count **after** `truncate`; interleaved **before**. Do not refactor these into a shared helper in v1 — duplicate intentionally in each sink's `finalize`. |
| Trait abstraction obscures flow | Keep `run_extract_decode_loop` linear; sinks are thin; comment shared vs sink sections |
| Large single PR | Phase 1 mono-only merge first |
| `channels` resolved late on interleaved | Preserve current order: first decode sets rate, then channels, then reserve |
| Mono decode-skip regressions undetected | Only interleaved has a skip-count test today; Phase 1 adds mono parity before deleting duplicated loop |

---

## Non-goals

- Merging `append_frames_in_window` and `append_interleaved_frames_in_window` into one function.
- Reading Symphonia audio planes without `copy_to_vec_interleaved`.
- Changing `MAX_CONSECUTIVE_DECODE_ERRORS`, `decode_shortfall_limit`, or seek-retry policy.
- Exposing the sink trait outside `symphonia` infrastructure.

---

## Cross-links

- Parent slice: [docs/archive/repair-write-path-plan.md](archive/repair-write-path-plan.md) § Lib extract hardening
- Backlog: [BACKLOG.md](../BACKLOG.md) § Symphonia extract loop hardening
- Consumer of full-timeline extract: `clip-sync-repair` `PatchAudio::execute`
