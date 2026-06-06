# Corpus validation harness

Manifest-driven integration tests exercise real-world alignment scenarios: multiple containers/codecs, timing leaders, multi-track MP4, and multi-clip consistency.

**Case matrix:** [corpus-matrix.md](corpus-matrix.md)  
**Fixtures & commands:** [tests/corpus/README.md](../tests/corpus/README.md)  
**Archived plans:** [corpus implementation](archive/corpus-implementation-plan.md), [session reuse](archive/session-reuse-plan.md), [high-rate refinement](archive/high-rate-offset-refinement-plan.md)

---

## Quick start

```powershell
cargo test corpus_                    # committed + generated (~60s with ffmpeg)
cargo test -- --ignored               # + external long smoke (CLIP_SYNC_CORPUS)
.\scripts\generate_corpus.ps1         # regenerate committed WAV fixtures
```

- **Committed tier** — 3 cases, 6 WAV files under `tests/corpus/wav/` (~3.4 MB).
- **Generated tier** — 17 cases built at test time; ffmpeg / `he-aac` cases skip when unavailable.
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
| Default `select_best_track` on dual MP4 | **Fixed:** first decodable track in mux order (`mp4_dual_track_wrong_default`); use `try_all_tracks` when program is not first |
| Two-clip offset agreement | `require_consistent_offsets` blocks bad recommendations (`two_clip_inconsistent`, `require_consistent_blocks`) |
| Redundant probe+open per clip window | **Fixed:** one probe per file per run; format reader + decoders reused ([session reuse plan](archive/session-reuse-plan.md)) |
| Sub-50 ms residual after discovery on WAV | **Fixed:** optional high-rate hold-out refine (`wav_high_rate_refine_3s`, ±50 ms) |

---

## Multi-track containers (`try_all_tracks`)

`select_best_track` picks the **first decodable audio track** in container mux order. When the main program is muxed first, dual-track MP4/MKV aligns correctly without extra flags (`mp4_dual_track_wrong_default`). When commentary or a decoy is muxed **before** the program, use `try_all_tracks`.

When `try_all_tracks` is enabled, the aligner decodes every decodable track pair on A and B, scores each alignment, and keeps the highest-confidence result. The same media session and format reader are reused across track pairs and clip windows.

**Enable via CLI:**

```powershell
clip-sync --try-all-tracks video_a.mp4 video_b.mp4
```

**Or in a config file** (`[alignment]` section):

```toml
try_all_tracks = true
```

Default is `false` because track-pair brute force multiplies decode work. Prefer enabling it when you know a container has multiple audio tracks or alignment looks wrong with the default pick.

---

## Follow-up

Tracked in [BACKLOG.md](../BACKLOG.md):

- Tighten `max_wall_secs` on other multi-clip cases if regressions are caught
- Dual-track case when decoy is muxed first (default pick still wrong; needs `try_all_tracks`)
