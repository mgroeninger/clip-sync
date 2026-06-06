# Corpus validation harness

Manifest-driven integration tests exercise real-world alignment scenarios: multiple containers/codecs, timing leaders, multi-track MP4, and multi-clip consistency.

**Case matrix:** [corpus-matrix.md](corpus-matrix.md)  
**Fixtures & commands:** [tests/corpus/README.md](../tests/corpus/README.md)  
**Archived implementation plan:** [archive/corpus-implementation-plan.md](archive/corpus-implementation-plan.md)

---

## Quick start

```powershell
cargo test corpus_                    # committed + generated (~60s with ffmpeg)
cargo test -- --ignored               # + external long smoke (CLIP_SYNC_CORPUS)
.\scripts\generate_corpus.ps1         # regenerate committed WAV fixtures
```

- **Committed tier** — 3 cases, 6 WAV files under `tests/corpus/wav/` (~3.4 MB).
- **Generated tier** — 12 cases built at test time; ffmpeg cases skip when ffmpeg is missing.
- **External tier** — `long_smoke_60m` (3600 s); `#[ignore]` unless `CLIP_SYNC_CORPUS` is set.

Harness code: `src/application/testing/corpus_fixtures.rs`, generators in `audio_fixtures.rs` and `ffmpeg_util.rs`.

---

## What the corpus proved (2026-06-06)

| Finding | Resolution |
|---------|------------|
| MP3 without Xing/duration tag | Opens and aligns (`mp3_no_duration_tag` passes) |
| Stereo AAC downmix | Works (`mp4_stereo_leader_3s`) |
| Dual-track MP4 with `try_all_tracks` | Works when program track is scored (`mp4_dual_track_decoy`) |
| Identical decoy on A/B caused false offset 0 | **Fixed:** distinct decoy tones (220 Hz vs 330 Hz) per file |
| Default `select_best_track` on dual MP4 | Documented failure when decoy has higher sample rate (`mp4_dual_track_wrong_default`); use `try_all_tracks` |
| Two-clip offset agreement | `require_consistent_offsets` blocks bad recommendations (`two_clip_inconsistent`, `require_consistent_blocks`) |

---

## Follow-up (not blocking validation)

Tracked in [BACKLOG.md](../BACKLOG.md):

- `wav_leader_30s`, `reencode_mp3_vs_mp4`, `refine_on_vs_off` — matrix rows not yet in manifest
- `near_silence_window` — needs clip-skip / soft-fail behavior
- `he_aac_mp4_leader_3s` — requires `he-aac` feature + ffmpeg
- Session reuse for perf on multi-clip / long files
- Improve `select_best_track` for dual-track containers (bitrate tiebreaker currently inert)
