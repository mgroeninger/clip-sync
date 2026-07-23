# Archived plan: media session reuse

> **Status:** Completed and archived (2026-06-06). Phases 1–3 and probe dedup are implemented. Sorted-window extraction remains open in [BACKLOG.md](../../../BACKLOG.md).
>
> **Superseded by:** [corpus-validation.md](../corpus-validation.md) (session reuse + multi-track summary), [PLAN.md](../../../PLAN.md) (configuration and CLI).

**Problem:** `SymphoniaMediaSession::extract_mono` re-opened and re-probed the file on every clip window. A typical 2-clip × 2-video run performed 4 redundant probe+open cycles after `open`.

**Goal:** Keep one `FormatReader` (and per-track decoders) alive for the lifetime of a session.

---

## Phases

### Phase 1 — Lazy I/O state on session

- [x] Plan doc
- [x] `MediaIoState`: lazy `FormatReader` on first `extract_mono`
- [x] `HashMap<track_id, CachedTrackDecoder>` — reuse decoder across windows on same track
- [x] Refactor decode loop into `extract_mono_with_state`
- [x] `session_reuses_format_reader_across_extracts` test
- [x] All existing `media_reader` + corpus tests green

### Phase 2 — Multi-track polish

- [x] Document `try_all_tracks` + multi-track behaviour (CLI `--try-all-tracks`, config, [corpus-validation.md](../corpus-validation.md))
- [x] Corpus wall-time regression budget on `two_clip_consistent` (`max_wall_secs` in manifest)

### Phase 3 — Probe dedup

- [x] `open_format_reader` shared by `probe_media` and session `open`
- [x] `probe_media_reusable` retains `FormatReader` at `open` (one probe per file per run)
- [x] `session_open_reuses_probe_format_reader` test
- [ ] Sorted-window extraction — deferred to BACKLOG (not required for session reuse)

---

## Final design

```text
open(path)           → probe_media_reusable → SymphoniaMediaSession { path, tracks, io: Some(MediaIoState) }
                     → format reader rewound to start; no second probe on first extract

extract_mono(...)    → io already present (or lazy reopen if rewind failed)
                     → decoder = cache[track_id] or create
                     → seek + reset + decode loop (unchanged logic)
```

`MediaSession::extract_mono` stays `&self`; interior mutability via `RefCell<Option<MediaIoState>>`.

With `try_all_tracks`, the same session and format reader serve every track pair; only the active track id changes per extract loop.

---

## Tests

| Test | Asserts |
|------|---------|
| `session_reuses_format_reader_across_extracts` | 2 extracts ⇒ 1 `open_format_reader` call |
| `session_open_reuses_probe_format_reader` | `open` retains probe reader; first extract does not re-probe |
| Existing WAV/MKV/MP4 extract tests | PCM identical via session API |
| `two_clip_consistent` corpus case | `max_wall_secs = 30` wall-time budget |

---

## References

- `src/infrastructure/symphonia/media_reader.rs`
- `src/application/align_videos.rs` (`extract_clips` loop)
- [BACKLOG.md](../../../BACKLOG.md) — sorted-window extraction follow-up
