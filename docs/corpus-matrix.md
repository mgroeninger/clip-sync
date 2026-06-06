# clip-sync corpus case matrix

Authoritative list of validation cases for real-world alignment testing. Each **Case ID** must appear in `tests/corpus/manifest.toml` when implemented.

See [corpus-validation.md](corpus-validation.md) for harness overview and [archive/corpus-implementation-plan.md](archive/corpus-implementation-plan.md) for the archived implementation plan.

---

## Dimensions

| Dimension | Values |
|-----------|--------|
| **Tier** | `B` committed in git; `A` generated at test time (ffmpeg); `C` external via `CLIP_SYNC_CORPUS` |
| **Container** | WAV, MP3, MP4 (AAC), MKV (FLAC) |
| **Codec** | PCM, MP3, AAC-LC, FLAC; optional HE-AAC (`he-aac` feature) |
| **Channels** | mono, stereo |
| **Offset** | seconds to add to A to align with B (domain convention); `+` = B's matching audio later |
| **Tracks** | 1 = single program; 2 = program + decoy |
| **Duration** | short 120s (CI), medium 180–900s, long 3600s (ignored smoke) |
| **Content** | chirp (oracle), tone (negative), near-silence |

---

## Tolerance policy (CI defaults)

| Assertion | Default |
|-----------|---------|
| `recommended_offset_secs` | ±1.0 s of expected |
| `confidence` (positive cases) | ≥ 0.5 |
| `exit_code` | 0 unless case notes hard error |
| `clip_length` (tests) | 60 s |
| `num_clips` | 1 unless noted |

---

## Case table

| Case ID | Tier | Format | Offset | Ch | Trk | Dur | Content | Expected outcome |
|---------|------|--------|--------|----|-----|-----|---------|------------------|
| `wav_baseline_0s` | B | WAV | 0 | 1 | 1 | 120s | chirp | aligned; offset ≈ 0; recommended |
| `wav_leader_3s` | B | WAV | +3s | 1 | 1 | 120s | chirp | aligned; offset ≈ +3; recommended |
| `wav_leader_15s` | A | WAV | +15s | 1 | 1 | 120s | chirp | aligned; offset ≈ +15; recommended |
| `wav_leader_30s` | A | WAV | +30s | 1 | 1 | 120s | chirp | aligned; offset ≈ +30; PCM discover; recommended |
| `wav_leader_60s` | A | WAV | +60s | 1 | 1 | 180s / 120s clip | chirp | aligned; offset ≈ +60; recommended |
| `wav_b_ahead_5s` | A | WAV | −5s | 1 | 1 | 120s | chirp | aligned; offset ≈ −5; recommended |
| `mp3_leader_3s` | A | MP3 | +3s | 1 | 1 | 120s | chirp | aligned; offset ≈ +3; recommended |
| `mp3_no_duration_tag` | A | MP3* | +3s | 1 | 1 | 120s | chirp | open OK; offset ≈ +3 |
| `mp4_aac_leader_3s` | A | MP4/AAC | +3s | 1 | 1 | 120s | chirp | aligned; offset ≈ +3; recommended |
| `mkv_flac_leader_3s` | A | MKV/FLAC | +3s | 1 | 1 | 120s | chirp | aligned; offset ≈ +3; recommended |
| `mp4_stereo_leader_3s` | A | MP4/AAC | +3s | 2 | 1 | 120s | chirp | aligned; offset ≈ +3; downmix OK |
| `mp4_dual_track_decoy` | A | MP4/AAC | +3s | 1 | 2 | 120s | chirp+decoy | offset ≈ +3; program track selected |
| `mp4_dual_track_wrong_default` | A | MP4/AAC | +3s | 1 | 2† | 120s | chirp+decoy | default picks program (muxed first); offset ≈ +3 |
| `no_overlap_tone_vs_chirp` | B | WAV | — | 1 | 1 | 60s | tone vs chirp | not aligned; no recommendation; exit 0 |
| `near_silence_window` | B | WAV | 0 | 1 | 1 | 60s | near-silence | soft fail; see plan ‡ |
| `two_clip_consistent` | A | WAV | +12s | 1 | 1 | 180s | chirp | `num_clips=2`; offsets_consistent; recommended |
| `two_clip_inconsistent` | A | WAV | mixed§ | 1 | 1 | 180s | chirp | `num_clips=2`; offsets_consistent false; no recommended‡ |
| `long_smoke_60m` | C | WAV | +3s | 1 | 1 | 3600s | chirp | offset ≈ +3; `#[ignore]` perf smoke |
| `he_aac_mp4_leader_3s` | A | MP4/HE-AAC | +3s | 1 | 1 | 120s | chirp | requires `he-aac` + ffmpeg |
| `reencode_mp3_vs_mp4` | A | MP3 vs MP4 | +3s | 1 | 1 | 120s | chirp | cross-container pair; offset ≈ +3 |
| `refine_on_vs_off` | A | WAV | +3s | 1 | 1 | 120s | chirp | both configs within tolerance |
| `wav_high_rate_refine_3s` | A | WAV 44.1k | +3s | 1 | 1 | 120s | chirp | `refine_offset_high_rate`; offset ±50 ms |
| `require_consistent_blocks` | A | WAV | +10/+20‖ | 1 | 1 | 180s | chirp | `num_clips=2`; no recommended offset |

### Notes

- `MP3*` — encoded with duration tag stripped (`-write_xing 0`) to stress probe fallback.
- `2†` — program muxed first, decoy second at higher sample rate; default `select_best_track` picks program. Use `try_all_tracks` when program is not first (see [corpus-validation.md](corpus-validation.md)).
- `‡` — `near_silence` / inconsistent cases may need clip-skip behavior; mark `ignore` until implemented.
- `§` — B truncated or different in end window while start matches.
- `‖` — synthetic two-window pair with different true offsets per window.

---

## Coverage checklist

Mark when at least one case exists in the manifest **and** passes in CI:

- [x] WAV positive (0s, +3s, −5s generated)
- [x] MP3 positive
- [x] MP4 AAC positive
- [x] MKV FLAC positive
- [x] Stereo downmix
- [x] Multi-track
- [x] Negative (no overlap)
- [x] Two-clip consistency
- [x] Duration-less MP3 open
- [x] Long smoke (ignored; `long_smoke_60m` in manifest)
- [x] HE-AAC (feature-gated; `he_aac_mp4_leader_3s`, skip without `he-aac`)
- [x] High-rate refinement (±50 ms; `wav_high_rate_refine_3s`)

---

## Status

| Case ID | In manifest | In `tests/corpus/` | Test passing |
|---------|-------------|-------------------|--------------|
| `wav_baseline_0s` | yes | yes (committed WAV) | yes |
| `wav_leader_3s` | yes | yes (committed WAV) | yes |
| `no_overlap_tone_vs_chirp` | yes | yes (committed WAV) | yes |
| `wav_b_ahead_5s` | yes | generated at test time | yes |
| `mp3_leader_3s` | yes | generated (ffmpeg) | yes* |
| `mp3_no_duration_tag` | yes | generated (ffmpeg) | yes* |
| `mp4_aac_leader_3s` | yes | generated (ffmpeg) | yes* |
| `mkv_flac_leader_3s` | yes | generated (ffmpeg) | yes* |
| `mp4_stereo_leader_3s` | yes | generated (ffmpeg) | yes* |
| `mp4_dual_track_decoy` | yes | generated (ffmpeg) | yes* |
| `mp4_dual_track_wrong_default` | yes | generated (ffmpeg) | yes* |
| `two_clip_consistent` | yes | generated at test time | yes |
| `two_clip_inconsistent` | yes | generated at test time | yes |
| `require_consistent_blocks` | yes | generated at test time | yes |
| `wav_leader_15s` | yes | generated at test time | yes |
| `wav_leader_30s` | yes | generated at test time | yes |
| `wav_leader_60s` | yes | generated at test time | yes |
| `reencode_mp3_vs_mp4` | yes | generated (ffmpeg) | yes* |
| `refine_on_vs_off` | yes | generated at test time | yes |
| `wav_high_rate_refine_3s` | yes | generated at test time | yes |
| `near_silence_window` | yes | generated at test time | yes |
| `he_aac_mp4_leader_3s` | yes | generated (ffmpeg) | yes‡ |
| `long_smoke_60m` | yes | external (`#[ignore]`) | manual |

\* Requires `ffmpeg` on PATH; skipped when unavailable.  
‡ Requires `--features he-aac` + ffmpeg; skipped otherwise.

Update the status table as new cases land.
