# Temporary plan: f32 internal PCM + source-driven output bit depth

> **Status:** Draft (2026-06-25). Motivated by an audit showing the entire pipeline represents
> decoded PCM as `Vec<i16>` end-to-end, and the one production WAV writer
> (`clip-sync-repair/src/infrastructure/wav_writer.rs`) hardcodes `bits_per_sample: 16` regardless
> of source. Symphonia decodes every codec to `f32` internally; `float_to_i16()` immediately
> truncates to 16-bit at extraction time, so any 24-bit or float-mastered source loses precision
> before the repair pipeline ever sees it, and there is no way to recover it on write even if we
> wanted to.
>
> Archive to `docs/archive/f32-pcm-bitdepth-plan.md` when shipped.

**Problem:** Two compounding issues:
1. **Precision loss at decode.** `MultiChannelPcm.samples` and `MonoPcmClip.samples` are
   `Vec<i16>`. Every analysis/DSP step (gap energy, RMS silence, crossfade fit, resample,
   correlation) operates on already-quantized 16-bit data, then several of those steps widen to
   `f64` for math anyway (`policies.rs::interleaved_to_mono/channels`,
   `offset_refinement.rs::samples_to_f64`) — paying a quantization cost with no analysis benefit.
2. **No source bit-depth awareness on write.** `AudioTrack` carries `codec`, `channels`,
   `sample_rate`, `duration`, `decodable` — no bit depth or sample format. Symphonia's
   `AudioCodecParameters` already exposes `bits_per_sample: Option<u32>` and
   `sample_format: Option<SampleFormat>` (confirmed in `symphonia-core` 0.6.0,
   `src/codecs/audio.rs:86,92`) but `probe_from_format` (`infrastructure/symphonia/probe.rs:91-99`)
   never reads them. `WavPatchedAudioWriter` then hardcodes 16-bit int output unconditionally.

**Goal:**
- Internal PCM representation becomes `Vec<f32>` (normalized `[-1.0, 1.0]`, matching Symphonia's
  native decode output and `extract.rs::float_to_i16`'s existing scale convention) throughout
  `clip-sync` and `clip-sync-repair` domain/application code.
- Probe captures source bit depth / sample format per track.
- WAV (and, if `ffmpeg-mux` is enabled, the mux pipe) write at a bit depth derived from the
  source: 32-bit float if the source was float (e.g. WAV/FLAC float, some ALAC), 24-bit int if
  source `bits_per_sample > 16`, else 16-bit int. No upsampling claims — output depth is `min`
  of "what the source actually had" and what's useful (we don't manufacture precision a lossy
  codec like AAC/AC-3 never had).

**Non-goals (v1):**
- Per-channel or per-track mixed bit depth in a single output file.
- Dithering when truncating to a lower output depth (e.g. 24-bit float source → 16-bit WAV by
  user choice/flag) — straight truncation/round is acceptable for v1.
- Preserving bit depth through lossy codecs (AAC/MP3/AC-3/Opus) beyond whatever Symphonia reports
  as the decoder's native output format — these have no real "source bit depth" and will fall
  back to a documented default (16-bit), same as today.
- Changing the `--mux` AAC path's encode bit depth (libfdk/AAC encode bit depth is a separate,
  unrelated concern from WAV output depth).

---

## Current codebase baseline

| Area | Path | Current state | Target phase |
|------|------|----------------|--------------|
| PCM struct | `clip-sync/src/domain/multichannel_pcm.rs:8` | `samples: Vec<i16>` | 1 |
| PCM struct | `clip-sync/src/domain/mono_pcm_clip.rs:4` | `samples: Vec<i16>` | 1 |
| Decode conversion | `clip-sync/src/infrastructure/symphonia/extract.rs:1190` | `float_to_i16(f32) -> i16`, called at `:850,882,913,954` | 1 (delete; push `f32` directly) |
| Extract sinks | `clip-sync/src/infrastructure/symphonia/extract_loop.rs:529,798` | `mono_samples: Vec<i16>`, `out: Vec<i16>` | 1 |
| Resample | `clip-sync/src/infrastructure/resample/rubato.rs:45,68-75` | i16→f32 (raw widen, *not* normalized) in, f32→i16 (clamp+round) out around rubato `f32` core | 1 (drop both conversions; note scale-convention mismatch fixed) |
| Repair analysis | `clip-sync-repair/src/domain/policies.rs:136,199,214,243,458` | `is_silent`, `rms_i16`, `rms_interleaved` on `&[i16]`; `interleaved_to_mono/channels` widen to `Vec<f64>` | 1 (operate on `&[f32]`; drop the f64 widen — f32 RMS/correlation precision already exceeds 16-bit quantization noise) |
| Repair analysis | `clip-sync-repair/src/domain/gap_energy.rs:10`, `gap_fill_fit.rs:758` | `energy_bins`, `fit_fill_to_gap_frames` on `&[i16]`/`Vec<i16>` | 1 |
| Offset refinement | `clip-sync/src/application/offset_refinement.rs:526,543` | `first_audio_index`, `samples_to_f64` on `&[i16]` | 1 |
| Raw PCM IO | `clip-sync-repair/src/infrastructure/pcm.rs:45` | `write_pcm_s16le` — i16 slice to ffmpeg stdin | 1 (generalize: emit the bytes for the *chosen output depth*, not always s16le) |
| WAV writer | `clip-sync-repair/src/infrastructure/wav_writer.rs:18-23` | `WavSpec { bits_per_sample: 16, sample_format: Int, .. }` hardcoded | 2 |
| Mux pipe | `clip-sync-repair/src/infrastructure/ffmpeg_mux.rs:30` (`-f s16le`) | Hardcoded s16le pipe format | 2 (match writer's resolved output depth) |
| Track metadata | `clip-sync/src/domain/audio_track.rs:4-13` | No bit depth / sample format field | 1 |
| Probe | `clip-sync/src/infrastructure/symphonia/probe.rs:91-99` | Builds `AudioTrack` from `AudioCodecParameters`; ignores `bits_per_sample`/`sample_format` | 1 |
| hound capability | `hound 3.5.1` (real dep in `clip-sync-repair/Cargo.toml:33`, optional `test-utils` dep in `clip-sync/Cargo.toml:34`) | Already supports `SampleFormat::{Int, Float}` at 16/24/32-bit; only ever invoked with 16-bit Int today | confirmed, no crate change needed |

---

## Decisions

| Topic | Decision |
|-------|----------|
| **Canonical f32 scale** | Normalized `[-1.0, 1.0]`, matching Symphonia's own decoded buffers and the existing `float_to_i16` convention in `extract.rs`. *Rejected:* raw-magnitude f32 (what `rubato.rs` does today, `f32::from(i16)`) — inconsistent with the rest of the codebase and would force a rescale at every boundary. |
| **Where conversion happens now** | Delete `float_to_i16` and its four call sites in `extract.rs`/`extract_loop.rs`; the decode loop pushes the Symphonia-native `f32` sample directly. Decode-layer code becomes *simpler* (one less conversion), not more complex. |
| **Resample boundary** | `rubato.rs` already runs `FftFixedIn<f32>` internally — delete the i16↔f32 conversions at lines 45 and 68-75; pass/return `Vec<f32>` straight through. Fixes the existing raw-vs-normalized scale mismatch as a side effect. |
| **Repair domain math** | `policies.rs::interleaved_to_mono/channels` currently widen `&[i16]` to `Vec<f64>` for downmix/correlation math. With `f32` input already at full mantissa precision relative to any real source, widen to `f64` only where an existing algorithm specifically needs it (e.g. long correlation sums where f32 accumulation error matters) — otherwise keep `f32` throughout to avoid needless alloc/copy. Decide per call site during Phase 1 implementation; default to f32 unless a test regresses. |
| **Bit-depth detection source** | Read `AudioCodecParameters::bits_per_sample` and `sample_format` (Symphonia, confirmed present in `symphonia-core` 0.6.0) at probe time. Add `AudioTrack.bit_depth: Option<BitDepth>` where `BitDepth` is a small enum (`Int16`, `Int24`, `Int32`, `Float32`, `Other(u32)`) derived from `(bits_per_sample, sample_format)`. `None` when Symphonia reports neither (typical for lossy codecs: AAC/MP3/AC-3/Opus/Vorbis) — these fall back to the existing 16-bit default. |
| **Output depth resolution** | New pure function `resolve_output_bit_depth(source: Option<BitDepth>) -> WavBitDepth` (`WavBitDepth::{Int16, Int24, Float32}`): `Float32` source → `Float32` out; `Int24`/`Int32` source → `Int24` out (no need to claim 32-bit int, which `hound`/most players handle poorly); `Int16` or `None` → `Int16` out (today's behavior, unchanged for lossy-codec sources). *Rejected:* a CLI flag to force depth in v1 — detect-and-use is the explicit ask; a future `--bit-depth` override flag is a natural follow-up, not required now. |
| **f32 → output-depth conversion** | New `infrastructure/pcm_depth.rs` (or extend `infrastructure/pcm.rs`) with `f32_to_i16`, `f32_to_i24_bytes` (hound has no native i24 sample type — write as 3 packed bytes per `hound::WavWriter::write_sample` for `i32` with `bits_per_sample: 24`, confirming hound's documented convention), and a no-op passthrough for `Float32`. Centralize so `wav_writer.rs` and (if enabled) `ffmpeg_mux.rs`/`pcm.rs` share one conversion, not two independently-maintained roundings. |
| **`write_pcm_s16le`** | Rename/generalize to `write_pcm_le(writer, samples: &[f32], depth: WavBitDepth)` in `pcm.rs`; mux pipe format string (`-f s16le` / `-f s24le` / `-f f32le`) selected to match. |
| **MultiChannelPcm/MonoPcmClip metadata** | Add `source_bit_depth: Option<BitDepth>` to `MultiChannelPcm` (carried from the `AudioTrack` used to decode it) so the writer doesn't need a second lookup. `MonoPcmClip` (fingerprint-rate, lossy-tolerant use only) does **not** need this field — nothing downstream writes a `MonoPcmClip` to a file. |
| **Existing test fixtures using `hound` at 16-bit** | Untouched — they construct *input* WAVs for tests, independent of the production writer's output decision. No change required unless a test specifically wants to assert 24-bit/float round-tripping (new tests, Phase 3). |

---

## Phases

### Phase 1 — internal f32 conversion (no behavior change in output bit depth)

- [ ] `domain/multichannel_pcm.rs`: `samples: Vec<i16>` → `Vec<f32>`; add `source_bit_depth: Option<BitDepth>`.
- [ ] `domain/mono_pcm_clip.rs`: `samples: Vec<i16>` → `Vec<f32>`.
- [ ] `domain/audio_track.rs`: add `bit_depth: Option<BitDepth>` (new small enum in `domain/`, e.g. `domain/bit_depth.rs`).
- [ ] `infrastructure/symphonia/probe.rs`: populate `AudioTrack.bit_depth` from `AudioCodecParameters::{bits_per_sample, sample_format}` in `probe_from_format` (mirrors how `channel_count()` already derives a field from codec params).
- [ ] `infrastructure/symphonia/extract.rs`: delete `float_to_i16`; the four `append_*` functions push `f32` samples directly (Symphonia buffers are already `f32` via `copy_to_vec_interleaved`/`scratch: &mut Vec<f32>` — no new conversion needed, just stop truncating).
- [ ] `infrastructure/symphonia/extract_loop.rs`: sink buffers `Vec<i16>` → `Vec<f32>`.
- [ ] `infrastructure/resample/rubato.rs`: delete the i16↔f32 boundary conversions (lines ~45, ~68-75); `resample_mono_pcm` takes/returns `Vec<f32>` straight through rubato.
- [ ] `clip-sync-repair/src/domain/policies.rs`: `is_silent`, `rms_i16` → `rms_f32`, `rms_interleaved`, `interleaved_to_mono/channels` operate on `&[f32]`; drop `f64` widen unless a specific test demands it (see Decisions).
- [ ] `clip-sync-repair/src/domain/gap_energy.rs`, `gap_fill_fit.rs`: `&[i16]`/`Vec<i16>` → `&[f32]`/`Vec<f32>`.
- [ ] `clip-sync/src/application/offset_refinement.rs`: `first_audio_index`, `samples_to_f64` → operate on `&[f32]` (rename `samples_to_f64` if it becomes a no-op widen, or drop it if f32 already suffices for the correlation math — check existing test tolerances).
- [ ] `clip-sync-repair/src/infrastructure/pcm.rs`: `write_pcm_s16le` keeps writing s16le for now (still converts `f32` → `i16` at this single boundary) — full generalization deferred to Phase 2 so Phase 1 has zero output-format change to verify against.
- [ ] Update every test fixture/helper that constructs `MultiChannelPcm`/`MonoPcmClip` literals with `Vec<i16>` samples (`policies.rs` tests, `gap_energy.rs` tests, `gap_structure.rs` tests, `offset_refinement.rs` tests, etc.) to `Vec<f32>`.
- [ ] `cargo test --workspace` green with byte-for-byte identical WAV output to pre-refactor (16-bit, same samples) — this phase is a pure internal representation change.

### Phase 2 — source-driven output bit depth

- [ ] `domain/bit_depth.rs`: `BitDepth` enum + `resolve_output_bit_depth(Option<BitDepth>) -> WavBitDepth` pure function (unit-testable in isolation, per Decisions table).
- [ ] `infrastructure/pcm_depth.rs` (new) or extend `infrastructure/pcm.rs`: `f32_to_i16`, `f32_to_i24_packed`, float passthrough; shared by writer and mux.
- [ ] `infrastructure/wav_writer.rs`: resolve `WavSpec` from `audio.source_bit_depth` via `resolve_output_bit_depth`; write via the shared conversion helpers instead of hardcoding `bits_per_sample: 16`.
- [ ] `infrastructure/pcm.rs::write_pcm_s16le` → `write_pcm_le(writer, samples: &[f32], depth: WavBitDepth)`.
- [ ] `infrastructure/ffmpeg_mux.rs` (under `ffmpeg-mux` feature): pipe format string follows resolved depth (`-f s16le` / `-f s24le` / `-f f32le`); confirm ffmpeg's AAC encoder accepts all three as input (it does — encoder input format is independent of AAC's own bit depth).
- [ ] `validate_pcm_for_wav` (`infrastructure/pcm.rs`, referenced from `wav_writer.rs:10`): confirm its multiple-of-channels / non-empty checks are depth-agnostic; add depth to any size/overflow checks if needed (24-bit triples 4 GiB-limit math differently than 16-bit — recheck the existing "exceeds 4 GiB" `--mux` hint in `wav_writer.rs:37-42` against the new byte-per-sample width).
- [ ] CLI/progress text: if `--verbose`, log resolved output depth alongside the existing source/codec info (mirrors `format_description()` pattern in `audio_track.rs`).

### Phase 3 — tests + docs

- [ ] Unit tests: `resolve_output_bit_depth` for every `BitDepth` input including `None`.
- [ ] Integration test: WAV/FLAC 24-bit int source fixture → repaired output is 24-bit WAV; assert via `hound::WavReader` spec on the output.
- [ ] Integration test: 32-bit float WAV source fixture → repaired output is `SampleFormat::Float` 32-bit WAV.
- [ ] Integration test: lossy source (existing AAC/AC-3 fixtures) → output stays 16-bit (no behavior change, regression guard).
- [ ] Existing fixed-16-bit fixtures/tests (`cli_wav_integration.rs`, `patch_audio_integration.rs`, `scan_gaps_integration.rs`, etc.) re-verified green — they construct synthetic 16-bit *input* WAVs, so expected output stays 16-bit; no fixture changes required, but worth a pass to confirm none silently relied on i16 internal representation leaking through an assertion.
- [ ] `docs/pipeline.md`, `docs/gap-repair-guide.md`: document bit-depth detection and the f32 internal representation.
- [ ] `BACKLOG.md`: add completed row.
- [ ] Archive this plan.

---

## Tests

| Concern | Coverage |
|---------|----------|
| f32 internal correctness | Phase 1: full existing test suite re-targeted to `Vec<f32>`, asserting unchanged WAV byte output (round-trip parity with pre-refactor i16 path) |
| Bit-depth resolution logic | Phase 3: `resolve_output_bit_depth` unit tests, all `BitDepth` variants + `None` |
| 24-bit round-trip | Phase 3: 24-bit int source → 24-bit WAV output, sample-value parity check |
| Float round-trip | Phase 3: 32-bit float source → 32-bit float WAV output |
| Lossy-codec fallback | Phase 3: AAC/AC-3 source still produces 16-bit WAV (no regression) |
| 4 GiB WAV-limit hint | Phase 2: recheck `wav_writer.rs`'s existing "use --mux instead" message threshold against 24-bit (3 bytes/sample) and float (4 bytes/sample) — limit is reached sooner than at 16-bit |
| Mux pipe format | Phase 2 (if `ffmpeg-mux` enabled): pipe format string matches resolved depth; existing mux integration tests stay green |

## Exit criteria

- No production code path represents decoded PCM as `Vec<i16>`; `Vec<f32>` end-to-end from decode through analysis, resample, and write.
- `AudioTrack` carries detected `bit_depth` from Symphonia codec params where available.
- WAV output bit depth is resolved from the source track's detected depth, not hardcoded.
- Lossy-codec sources (no detectable bit depth) continue to produce 16-bit WAV — no regression for the common case.
- `ffmpeg-mux` pipe format (if enabled) matches the resolved output depth.
- Phase 1 lands with byte-identical output to today for all-16-bit-equivalent sources (i.e., the f32 conversion alone is behavior-preserving); Phase 2 is the only phase that changes output bytes.

## Open questions

- **24-bit packing via hound:** confirm hound's documented convention for `bits_per_sample: 24` with `SampleFormat::Int` (left-justified in `i32`, or right-justified 3-byte write) before implementing `f32_to_i24_packed` — get this from hound's own source/tests in the registry cache rather than assuming.
- **f32 WAV + downstream players:** some tools handle `IEEE_FLOAT` WAV poorly; consider whether 32-bit-float sources should map to 24-bit int instead of float-out, to maximize compatibility — leaning toward float-out (matches source exactly) but flag as a decision to revisit with the user before Phase 2 lands.
- **`MonoPcmClip` f64 correlation paths:** `offset_refinement.rs` and chromaprint fingerprinting were tuned/thresholded against i16-quantized input; verify existing correlation/offset test tolerances still pass with full f32 precision (should only get *more* accurate, but confirm no test hardcodes an i16-quantization-dependent expected value).

## References

- `crates/clip-sync/src/domain/multichannel_pcm.rs`
- `crates/clip-sync/src/domain/mono_pcm_clip.rs`
- `crates/clip-sync/src/domain/audio_track.rs`
- `crates/clip-sync/src/infrastructure/symphonia/probe.rs`
- `crates/clip-sync/src/infrastructure/symphonia/extract.rs` (`float_to_i16`, lines 850/882/913/954/1190)
- `crates/clip-sync/src/infrastructure/symphonia/extract_loop.rs`
- `crates/clip-sync/src/infrastructure/resample/rubato.rs`
- `crates/clip-sync-repair/src/domain/policies.rs`
- `crates/clip-sync-repair/src/domain/gap_energy.rs`
- `crates/clip-sync-repair/src/domain/gap_fill_fit.rs`
- `crates/clip-sync-repair/src/infrastructure/pcm.rs`
- `crates/clip-sync-repair/src/infrastructure/wav_writer.rs`
- `crates/clip-sync-repair/src/infrastructure/ffmpeg_mux.rs`
- `crates/clip-sync/src/application/offset_refinement.rs`
