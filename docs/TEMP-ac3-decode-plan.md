# Temporary plan: AC-3 / E-AC-3 decode via oxideav-ac3

> **Status:** Shipped and validated (2026-06-09). Archived to [`docs/archive/ac3-decode-plan.md`](archive/ac3-decode-plan.md). This file can be deleted.

**Problem:** Symphonia demuxes AC-3 and E-AC-3 from MP4/MKV (`dac3` / `dec3` atoms → `CODEC_ID_AC3` / `CODEC_ID_EAC3`) but ships **no decoder**. Probe marks those tracks `decodable: false`. `select_best_track` picks the first decodable stream — often 2ch AAC — while video A is 6ch. Repair reports `mismatch (fill blocked)` even when B contains a matching surround program on an undecodable AC-3 track.

**Motivating case:** `media.mkv` (6ch @ 48 kHz) + `media.m4v` (2ch AAC + 6ch AC-3/E-AC-3). Scan finds B energy at gap positions; patch plan is empty due to channel mismatch on the selected B track.

**Goal:** Decode AC-3 and E-AC-3 elementary streams in-process (pure Rust, no ffmpeg decode subprocess) and select a B audio track that **matches A's channel layout** when possible.

**Non-goals (v1):** Up/downmix between layouts; DTS; ffmpeg-as-decoder fallback; changing alignment fingerprint rate/path.

**References:** [oxideav-ac3](https://crates.io/crates/oxideav-ac3) (MIT, v0.0.7), existing `he-aac` + `fdk_aac` Symphonia adapter in `crates/clip-sync/src/infrastructure/symphonia/fdk_aac/`, `select_best_track` in `domain/policies.rs`, repair track gate in `domain/gap_fill.rs` + `domain/track_match.rs`.

---

## Decisions

Locked before implementation. Change only with an explicit plan revision.

| Topic | Decision |
|-------|----------|
| **Decoder library** | [oxideav-ac3](https://github.com/OxideAV/oxideav-ac3) v0.0.8 (`oxideav-core` v0.1.28 comes transitively — no explicit dep needed). Pure Rust, MIT. |
| **Cargo feature** | `ac3` on `clip-sync` (optional, off by default). Mirrors `he-aac`. Repair/CLI enable via `clip-sync = { features = ["ac3"] }` when we want it in release builds. |
| **Demux** | Unchanged — Symphonia `isomp4` / `mkv` already extract packets and set `CodecParameters` for AC-3/E-AC-3. |
| **Integration shape** | Symphonia `AudioDecoder` + `RegisterableAudioDecoder` adapter (same pattern as `fdk_aac/decoder.rs`). Register in `codec_registry()` when `ac3` feature is on. |
| **Codec IDs** | `CODEC_ID_AC3` (`0x1008`) and `CODEC_ID_EAC3` (`0x1009`) from `symphonia_core::codecs::audio::well_known`. |
| **Output format** | S16 interleaved PCM (oxideav default; matches existing `MultiChannelPcm` / Symphonia buffer path). |
| **Track selection** | Add `select_track_for_reference(a: &AudioTrack, tracks: &[AudioTrack])` in lib `domain/policies.rs`: among decodable tracks, prefer **matching `channels`** with A; tie-break first in container order (current behaviour). Fall back to `select_best_track` when no channel match. |
| **Where selection applies** | Lib alignment (`align_videos` when not using `try_all_tracks` scoring), repair `scan_gaps::open_best_track`, repair `patch_audio` B open. Alignment `try_all_tracks` already scores all decodable pairs — once AC-3 is decodable, 6ch×6ch pairs enter the search automatically. |
| **Repair compatibility** | Channel match → existing `Identical` / `Compatible` (rate resample only). No new verdict variant. |
| **ffmpeg** | Not used for decode. Existing `ffmpeg-mux` feature unchanged (mux only). |
| **User-facing hint** | When surround track exists but is undecodable without `ac3` feature, human report lists other audio tracks (index, codec, channels, decodable) under track compatibility — optional polish slice. |

---

## Architecture

```text
MP4/MKV file
    │
    ▼
Symphonia FormatReader (existing)
    │  packets per track_id
    ▼
codec_registry() ──► [AAC, FLAC, …, Ac3Decoder*]   * when feature `ac3`
    │
    ▼
extract_interleaved / extract_mono / scan buckets (unchanged)
    │
    ▼
align + scan + patch (repair)
```

### New code (lib)

| Path | Purpose |
|------|---------|
| `crates/clip-sync/src/infrastructure/symphonia/oxideav_ac3/mod.rs` | Module root |
| `crates/clip-sync/src/infrastructure/symphonia/oxideav_ac3/decoder.rs` | `Ac3Decoder` implementing Symphonia `AudioDecoder` |
| `crates/clip-sync/src/infrastructure/symphonia/codec_registry.rs` | `#[cfg(feature = "ac3")]` register `Ac3Decoder` for AC3 + EAC3 |
| `crates/clip-sync/src/domain/policies.rs` | `select_track_for_reference` + unit tests |
| `crates/clip-sync/Cargo.toml` | Feature `ac3`, optional dep `oxideav-ac3` (brings `oxideav-core` transitively) |

### Adapter responsibilities (`Ac3Decoder`)

1. **`try_new(params)`** — Build oxideav decoder from `AudioCodecParameters` (sample rate, channel layout from `params.channels`; pass `extra_data` from `dac3`/`dec3` if oxideav requires it — validate against oxideav README during spike).
2. **`decode(packet)`** — Feed Symphonia `PacketRef` bytes via oxideav `send_packet` / `receive_frame`; copy S16 interleaved into Symphonia `AudioBuffer<i16>`.
3. **`reset` / `finalize`** — Forward to oxideav decoder reset on seek (reuse existing session seek path in `extract.rs`).
4. **E-AC-3 dispatch** — Route by `params.codec` (`CODEC_ID_EAC3` vs `CODEC_ID_AC3`); oxideav exposes separate codec ids `"eac3"` / `"ac3"`.

### Repair / CLI touch points

| Location | Change |
|----------|--------|
| `scan_gaps.rs` | `open_best_track` uses `select_track_for_reference(&track_a, &tracks_b)` |
| `patch_audio.rs` | Same for B track after A track selected |
| `clip-sync-repair/Cargo.toml` | Optional: `ac3 = ["clip-sync/ac3"]` feature forwarding |
| `clip-sync-repair` CLI | Document `--features ac3` in README; no new flags required for v1 |

---

## Implementation phases

### A0 — Spike (half day)

- [x] Add `ac3` feature + deps; stub registry registration.
- [x] Probe `media.m4v` (or ffmpeg-generated 6ch AC-3 MP4 fixture): confirm track list shows AC-3 6ch `decodable: true` after registration.
- [x] Decode 5–10 s window to `MultiChannelPcm`; compare peak/RMS vs `ffmpeg -c:a pcm_s16le` on same segment (manual or ignored test).
- [x] Note oxideav version pin (`0.0.8`); `extra_data` pass-through implemented in adapter.

**Exit:** One integration test decodes ffmpeg-generated `ac3` + `eac3` 5.1 MP4 snippets when `ac3` feature enabled. ✓

### A1 — Symphonia adapter (lib)

- [x] Implement `Ac3Decoder` (`AudioDecoder` + `RegisterableAudioDecoder`).
- [x] Register for `CODEC_ID_AC3` and `CODEC_ID_EAC3`.
- [x] Extend `probe.rs` `codec_name` mapping: `"ac3"` / `"eac3"` (human-readable).
- [x] Unit test: decodability probe returns true for AC-3 params when feature on.

### A2 — Channel-matching track selection (lib + repair)

- [x] `select_track_for_reference` with tests (6ch A + [2ch, 6ch] B → pick 6ch; mono A unchanged).
- [x] Wire into `scan_gaps`, `patch_audio`.
- [ ] Consider storing selected B `track.index` on `GapReport` for diagnostics (optional; defer if scope creep).

### A3 — Validation & docs

- [x] Ignored-by-default corpus test: dual-track MP4 (2ch AAC + 6ch AC-3), align + repair smoke when `ac3` enabled.
- [x] Manual validation: licensed pair → `repairable_count > 0`, patch allowed, at least one gap patched (listening optional).
- [x] README: feature flag, limitations (oxideav maturity), `ffprobe` tip for stream layout.
- [x] Archive this doc; add one-line entry to `BACKLOG.md` if item existed there.

---

## Testing strategy

| Layer | Test |
|-------|------|
| **Lib unit** | `select_track_for_reference_*`; adapter round-trip on synthetic AC-3 frame bytes (if oxideav exposes test vectors) |
| **Lib integration** | `media_reader_tests`: probe + `extract_interleaved` on ffmpeg lavfi/generated AC-3 MP4 (`#[cfg(all(feature = "ac3", feature = "ffmpeg-tests"))]`) |
| **Repair integration** | Extend or mirror `cli_mux_integration` pattern: 6ch gap fill when B is surround MP4 |
| **Regression** | Default build (no `ac3`) unchanged; `ac3` off → AC-3 still `decodable: false` |

Fixture generation (dev only, requires ffmpeg):

```bash
# 5.1 AC-3 in MP4 (example — tune layout/duration for CI time budget)
ffmpeg -y -f lavfi -i sine=frequency=440:duration=5 -ac 6 \
  -c:a ac3 -b:a 448k -f mp4 tests/fixtures/generated/ac3-51-48k.mp4
```

---

## Risks and mitigations

| Risk | Mitigation |
|------|------------|
| oxideav **v0.0.x** maturity; some E-AC-3 paths still evolving | Feature off by default; spike on target media; ffmpeg PSNR comparison in A0; keep ffmpeg extract workaround documented |
| MP4 **dual-track** order (AAC before AC-3) | Channel-matching selection, not first-decodable only |
| **Seek** after AC-3 decode | Reuse Symphonia decoder `reset` on seek; add seek smoke test in A1 |
| **Channel order** (AC-3 bitstream vs WAV order) | oxideav applies WAVE order for multichannel; verify against ffmpeg in A0 |
| **Dependency weight** | Optional feature; only three small oxideav crates |
| **Alignment on 6ch** | Chromaprint path still mono-downmix for fingerprint — unchanged; only fill path uses full 6ch B |

---

## Config / CLI (v1)

No new TOML keys required. Optional future: `repair.prefer_surround_track = true` (default true when `ac3` enabled) — defer.

Build with AC-3 support:

```bash
cargo build --release -p clip-sync-repair --features ac3,ffmpeg-mux
```

Enable in workspace permanently only after licensed media (or equivalent) validation.

---

## Open questions (resolve in A0 spike)

1. Does oxideav need MP4 `dac3`/`dec3` `extra_data`, or only raw syncframe bytes per packet?
2. Is `media.m4v` surround track **AC-3** or **E-AC-3** (`ffprobe -show_streams`)?
3. Should `clip-sync-cli` / `clip-sync-repair` default features include `ac3` once stable, or stay opt-in?
4. Does `try_all_tracks` need to become default for dual-track containers, or is channel-matching on B sufficient after alignment?

---

## Checklist summary

```text
A0  spike + fixture + ffmpeg cross-check
A1  Ac3Decoder adapter + registry + probe labels
A2  select_track_for_reference + repair wiring
A3  integration tests + licensed media manual pass + doc archive
```
