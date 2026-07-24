# Plan: AC-3 / E-AC-3 decode via oxideav-ac3

> **Status:** Shipped and validated 2026-06-09. All phases complete.

**Problem:** Symphonia demuxes AC-3 and E-AC-3 from MP4/MKV (`dac3` / `dec3` atoms → `CODEC_ID_AC3` / `CODEC_ID_EAC3`) but ships **no decoder**. Probe marks those tracks `decodable: false`. `select_best_track` picks the first decodable stream — often 2ch AAC — while video A is 6ch. Repair reports `mismatch (fill blocked)` even when B contains a matching surround program on an undecodable AC-3 track.

**Motivating case:** `media.mkv` (6ch @ 48 kHz) + `media.m4v` (2ch AAC + 6ch AC-3/E-AC-3). Scan finds B energy at gap positions; patch plan is empty due to channel mismatch on the selected B track.

**Goal:** Decode AC-3 and E-AC-3 elementary streams in-process (pure Rust, no ffmpeg decode subprocess) and select a B audio track that **matches A's channel layout** when possible.

**Non-goals (v1):** Up/downmix between layouts; DTS; ffmpeg-as-decoder fallback; changing alignment fingerprint rate/path.

**References:** [oxideav-ac3](https://crates.io/crates/oxideav-ac3) (MIT, v0.0.8), existing `he-aac` + `fdk_aac` Symphonia adapter in `crates/clip-sync/src/infrastructure/symphonia/fdk_aac/`, `select_best_track` in `domain/policies.rs`, repair track gate in `domain/gap_fill.rs` + `domain/track_match.rs`.

---

## Decisions

| Topic | Decision |
|-------|----------|
| **Decoder library** | [oxideav-ac3](https://github.com/OxideAV/oxideav-ac3) v0.0.8. Pure Rust, MIT. |
| **Cargo feature** | `ac3` on `clip-sync` (optional, off by default). Mirrors `he-aac`. |
| **Demux** | Unchanged — Symphonia `isomp4` / `mkv` already extract packets. |
| **Integration shape** | Symphonia `AudioDecoder` + `RegisterableAudioDecoder` adapter. |
| **Codec IDs** | `CODEC_ID_AC3` + `CODEC_ID_EAC3` from `symphonia_core::codecs::audio::well_known`. |
| **Output format** | S16 interleaved PCM. |
| **Track selection** | `select_track_for_reference(a, tracks)` in `domain/policies.rs`: prefer channel-matching decodable track; fall back to first decodable. |
| **Channel count from dac3** | Symphonia's isomp4 `Dac3Atom` sets `codec_id` and `extra_data` but NOT `channels`. `probe.rs::channel_count` derives channel count from the `dac3`/`dec3` box payload bitfields. |

---

## Implementation phases

### A0 — Spike ✓

- [x] Add `ac3` feature + deps; stub registry registration.
- [x] Probe `media.m4v`: confirm AC-3 6ch `decodable: true`.
- [x] Decode 5–10 s window to `MultiChannelPcm`.
- [x] Note oxideav version pin (`0.0.8`); `extra_data` pass-through implemented.

**Exit:** Integration tests decode ffmpeg-generated `ac3` + `eac3` 5.1 MP4 snippets. ✓

### A1 — Symphonia adapter (lib) ✓

- [x] Implement `Ac3Decoder` (`AudioDecoder` + `RegisterableAudioDecoder`).
- [x] Register for `CODEC_ID_AC3` and `CODEC_ID_EAC3`.
- [x] Extend `probe.rs` `codec_name` mapping: `"ac3"` / `"eac3"`.
- [x] Unit test: decodability probe returns true for AC-3 params when feature on.

### A2 — Channel-matching track selection (lib + repair) ✓

- [x] `select_track_for_reference` with tests (6ch A + [2ch, 6ch] B → pick 6ch; mono A unchanged).
- [x] Wire into `scan_gaps`, `patch_audio`.

### A3 — Validation & docs ✓

- [x] Corpus test: dual-track MP4 (2ch AAC + 6ch AC-3), align + repair smoke when `ac3` enabled.
- [x] Manual validation: media pair → 4 repairable gaps, 3 patched. ✓
- [x] README: feature flag, limitations, `ffprobe` tip.
- [x] Archive this doc.

---

## Key implementation notes

**`dac3` box channel derivation** (`probe.rs`): Symphonia's `fill_audio_sample_entry` for isomp4 AC-3 sets `codec_id` and `extra_data` but not `channels`. The channel count is derived from the `dac3` box payload bitfields:
- `acmod` = `(extra[1] >> 3) & 0x7` — maps to base channel count {0→2, 1→1, 2→2, 3→3, 4→3, 5→4, 6→4, 7→5}
- `lfeon` = `(extra[1] >> 2) & 0x1` — add 1 if LFE present
- `dec3` (E-AC-3): `acmod` = `(extra[3] >> 1) & 0x7`, `lfeon` = `extra[3] & 0x1`

**Lazy buffer init** (`decoder.rs`): `Ac3Decoder::buf` is `Option<AudioBuffer<i16>>`, initialized on the first decoded frame when `params.channels` is None. `render_uninit(Some(audio_frame.samples as usize))` must be used — `None` renders full capacity and causes a length mismatch panic in `copy_from_slice_interleaved`.

**`unsafe impl Sync`**: `oxideav_core::Decoder` is `Send` but not `Sync`. All Symphonia decode methods take `&mut self`, so no shared-reference access is possible; `Sync` is safe to assert.

**Feature forwarding**: `clip-sync-repair` and `clip-sync-cli` both need `ac3 = ["clip-sync/ac3"]` in their `[features]` sections.

---

## Validation result (media, 2026-06-09)

```
Tracks:    A 6ch @ 48000Hz   B 6ch @ 48000Hz   (identical)
Gaps detected in video A (5 total, 4 repairable):
  #2   [197.75s – 200.50s]  (2.8s)  patched  (struct pre=0.97 post=0.90)
  #3   [281.25s – 282.25s]  (1.0s)  patched  (struct pre=0.97 post=1.00)
  #4   [1164.50s – 1165.50s]  (1.0s)  patched  (struct pre=0.98 post=1.00)
  #5   [7260.50s – 7439.98s]  (179.5s)  skipped: boundary alignment failed
Patch results: 3 patched, 1 skipped, 1 not planned
```
