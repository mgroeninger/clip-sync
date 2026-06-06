# Temporary plan: media session reuse

> **Status:** Phases 1–2 complete. Phase 3 (probe dedup at `open`) done. Archive when sorted-window work lands.

**Problem:** `SymphoniaMediaSession::extract_mono` re-opens and re-probes the file on every clip window. A typical 2-clip × 2-video run performs 4 redundant probe+open cycles after `open`.

**Goal:** Keep one `FormatReader` (and per-track decoders) alive for the lifetime of a session.

---

## Phases

### Phase 1 — Lazy I/O state on session (this PR)

- [x] Plan doc
- [x] `MediaIoState`: lazy `FormatReader` on first `extract_mono`
- [x] `HashMap<track_id, CachedTrackDecoder>` — reuse decoder across windows on same track
- [x] Refactor decode loop into `extract_mono_with_state`
- [x] `session_reuses_format_reader_across_extracts` test
- [x] All existing `media_reader` + corpus tests green

**Not in Phase 1:** sorted window extraction, cross-request cache, probe dedup with `probe_media`.

### Phase 2 — Multi-track polish

- [ ] Document `try_all_tracks` + multi-track behaviour with shared format reader
- [x] Corpus wall-time regression budget on `two_clip_consistent` (`max_wall_secs` in manifest)

### Phase 3 — Probe dedup

- [x] `open_format_reader` shared by `probe_media` and session `open`
- [x] `probe_media_reusable` retains `FormatReader` at `open` (one probe per file per run)
- [ ] Optional sorted-window extraction; archive this doc

---

## Design (Phase 1)

```text
open(path)           → probe only → SymphoniaMediaSession { path, tracks, io: None }

extract_mono(...)    → io.get_or_insert(open_format_reader)
                     → decoder = cache[track_id] or create
                     → seek + reset + decode loop (unchanged logic)
```

`MediaSession::extract_mono` stays `&self`; interior mutability via `RefCell<Option<MediaIoState>>`.

---

## Tests

| Test | Asserts |
|------|---------|
| `session_reuses_format_reader_across_extracts` | 2 extracts ⇒ 1 `open_format_reader` call |
| Existing WAV/MKV/MP4 extract tests | PCM identical via session API |

---

## References

- `src/infrastructure/symphonia/media_reader.rs`
- `src/application/align_videos.rs` (`extract_clips` loop)
- `BACKLOG.md` — session reuse item
