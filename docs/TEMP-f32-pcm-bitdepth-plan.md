# Temporary plan: f32 internal PCM + source-driven output bit depth

> **Status:** Phase 1 complete (2026-06-25). Phase 2 (source-driven output bit depth) is next. Motivated by an audit showing the entire pipeline represents
> decoded PCM as `Vec<i16>` end-to-end, and the one production WAV writer
> (`clip-sync-repair/src/infrastructure/wav_writer.rs`) hardcodes `bits_per_sample: 16` regardless
> of source. Symphonia decodes every codec to `f32` internally; `float_to_i16()` immediately
> truncates to 16-bit at extraction time, so any 24-bit or float-mastered source loses precision
> before the repair pipeline ever sees it, and there is no way to recover it on write even if we
> wanted to.
>
> Archive to `docs/archive/f32-pcm-bitdepth-plan.md` when shipped.

**Problem:** Two compounding issues:
1. **Precision loss at decode.** `MultiChannelPcm.samples` is `Vec<i16>`. Every repair-path
   analysis/DSP step (gap energy, RMS silence, crossfade fit, interleaved resample) operates on
   already-quantized 16-bit data, then several steps widen to `f64` for math anyway
   (`policies.rs::interleaved_to_mono/channels`) — paying a quantization cost with no analysis
   benefit. (`MonoPcmClip` also uses `Vec<i16>` but is fingerprint-only and stays that way; see
   Decisions.)
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
  source, **capped at 24-bit int**: source `bits_per_sample > 16` (including 32-bit float
  sources) → 24-bit int output; else 16-bit int (today's behavior, unchanged). We do not write
  32-bit float WAV output in v1 — 24-bit int is the ceiling regardless of source format, since
  it's universally supported by players/editors and float WAV support is inconsistent.

**Non-goals (v1):**
- 32-bit float WAV output — explicitly out of scope; 24-bit int is the max output depth even for
  float sources.
- Per-channel or per-track mixed bit depth in a single output file.
- Dithering when truncating to a lower output depth (e.g. 24-bit/float source → 16-bit WAV by
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
| PCM struct | `clip-sync/src/domain/mono_pcm_clip.rs:4` | `samples: Vec<i16>` | **no change** — stays `Vec<i16>`; chromaprint boundary constraint (see Decisions) |
| Decode conversion — interleaved path | `clip-sync/src/infrastructure/symphonia/extract.rs:1190` | `float_to_i16(f32) -> i16`, called at `:850,882` (interleaved) | 1 (delete call sites `:850,882`; push `f32` directly into `MultiChannelPcm`) |
| Decode conversion — mono path | `clip-sync/src/infrastructure/symphonia/extract.rs:913,954` | `float_to_i16` called at `:913,954` (mono, feeds `MonoPcmClip`) | **no change** — mono path stays `i16` |
| Extract sinks — interleaved | `clip-sync/src/infrastructure/symphonia/extract_loop.rs:798` | `out: Vec<i16>` in `InterleavedCollectContext` | 1 (`Vec<f32>`) |
| Extract sinks — mono | `clip-sync/src/infrastructure/symphonia/extract_loop.rs:529` | `mono_samples: Vec<i16>` in `MonoExtractSink` | **no change** — feeds `MonoPcmClip` |
| Resample — mono | `clip-sync/src/infrastructure/resample/rubato.rs:45,68-75` | i16↔f32 around rubato `f32` core; operates on `MonoPcmClip` | **no change** — `MonoPcmClip` stays `i16` |
| Resample — interleaved | `clip-sync/src/infrastructure/resample/rubato.rs:129` | `resample_interleaved(samples: &[i16], ...) -> Vec<i16>`; deinterleaves into `MonoPcmClip` per-channel, reinterleaves | 1 (signature → `&[f32]`/`Vec<f32>`; internal per-channel wrapper must *not* go through `MonoPcmClip` — deinterleave to `Vec<f32>` per channel, run rubato directly, reinterleave) |
| Repair analysis | `clip-sync-repair/src/domain/policies.rs:136,199,214,243,458` | `is_silent`, `rms_i16`, `rms_interleaved` on `&[i16]`; `interleaved_to_mono/channels` widen to `Vec<f64>` | 1 (operate on `&[f32]`; drop the f64 widen — f32 precision already exceeds 16-bit quantization noise; see scale-threshold audit below) |
| Repair analysis | `clip-sync-repair/src/domain/gap_energy.rs:10`, `gap_fill_fit.rs:758` | `energy_bins`, `fit_fill_to_gap_frames` on `&[i16]`/`Vec<i16>` | 1 |
| Repair analysis | `clip-sync-repair/src/domain/gap_signature.rs:73,124` | `build_gap_signature`, `effective_mode` take `samples: &[i16]` | 1 |
| Repair analysis | `clip-sync-repair/src/domain/gap_structure.rs:39,75,118,519` | `build`, `build_gap_context_signature`, `match_gap_structure_in_b`, `activity_bins` take `samples: &[i16]` | 1 |
| Patch region params | `clip-sync-repair/src/application/patch_region.rs:53` | `SeamGateParams.b_samples: &'a [i16]` — threaded through ~18 functions in the file | 1 (`&'a [f32]`) |
| Patch audio internals | `clip-sync-repair/src/application/patch_audio.rs:268` | `b_samples: Vec<i16>` struct field | 1 (`Vec<f32>`) |
| Patch audio internals | `clip-sync-repair/src/application/patch_audio.rs:538-544` | `b_gained: Vec<i16>` — applies float gain then clamps to i16 range | 1 (becomes `Vec<f32>`, clamp to `[-1.0, 1.0]` instead of `i16` range) |
| Patch audio internals | `clip-sync-repair/src/application/patch_audio.rs:1892` | `slice_b_segment(b_samples: &[i16], ...) -> Option<&[i16]>` | 1 (`&[f32]`/`Option<&[f32]>`) |
| Patch audio internals | `clip-sync-repair/src/application/patch_audio.rs:1952` | `splice_into_a(a_samples: &mut [i16], b_samples: &[i16], ...)` | 1 (`&mut [f32]`, `&[f32]`) |
| Patch audio internals | `clip-sync-repair/src/application/patch_audio.rs:1913` | `compute_a_border_rms` uses `f64::from(s: i16)` scaling off `a_pcm.samples` | 1 (use `s as f64` directly on `f32` samples; scale-agnostic ratio consumer, see audit) |
| Offset refinement | `clip-sync/src/application/offset_refinement.rs:526,543` | `first_audio_index`, `samples_to_f64` on `&[i16]` (operate on `MonoPcmClip`) | **no change** — `MonoPcmClip` stays `i16`; `64.0` threshold calibrated to i16 scale |
| Raw PCM IO | `clip-sync-repair/src/infrastructure/pcm.rs:45` | `write_pcm_s16le` — i16 slice to ffmpeg stdin | 1 (generalize: `write_pcm_le(writer, samples: &[f32], depth: WavBitDepth)`) |
| WAV writer | `clip-sync-repair/src/infrastructure/wav_writer.rs:18-23` | `WavSpec { bits_per_sample: 16, sample_format: Int, .. }` hardcoded | 2 |
| Mux pipe | `clip-sync-repair/src/infrastructure/ffmpeg_mux.rs:30` (`-f s16le`) | Hardcoded s16le pipe format | 2 (match writer's resolved output depth) |
| Track metadata | `clip-sync/src/domain/audio_track.rs:4-13` | No bit depth / sample format field | 1 |
| Probe | `clip-sync/src/infrastructure/symphonia/probe.rs:91-99` | Builds `AudioTrack` from `AudioCodecParameters`; ignores `bits_per_sample`/`sample_format` | 1 |
| hound capability | `hound 3.5.1` (real dep in `clip-sync-repair/Cargo.toml:33`, optional `test-utils` dep in `clip-sync/Cargo.toml:34`) | Already supports `SampleFormat::{Int, Float}` at 16/24/32-bit; only ever invoked with 16-bit Int today | confirmed, no crate change needed |
| Confirmed out of scope | `clip-sync/src/domain/pcm_preparation.rs` | All functions operate on `MonoPcmClip` only (fingerprint prep); i16-scale constants stay valid | no change |
| Confirmed out of scope | `clip-sync/src/infrastructure/symphonia/fdk_aac/decoder.rs`, `oxideav_ac3/decoder.rs` | Internal Symphonia codec adapters; their `AudioBuffer<i16>` is normalized to `f32` in `[-1,1]` by Symphonia's `copy_to_vec_interleaved` before reaching extract.rs — no change needed | no change |
| Confirmed out of scope | `clip-sync/src/infrastructure/correlation.rs:163` | `i16::MAX` reference is in a test-only tone generator (`fn tone_samples` in `#[cfg(test)]`); correlation math is scale-agnostic | no change |

---

## Decisions

| Topic | Decision |
|-------|----------|
| **MonoPcmClip stays `Vec<i16>`** | `rusty-chromaprint 0.3.0`'s `Fingerprinter::consume` signature is `fn consume(&mut self, data: &[i16])` — a hard C-library constraint. `MonoPcmClip.samples` feeds directly into `fingerprinter.consume(&clip.samples)` with no intermediate. Changing `MonoPcmClip` to `f32` would require a `Vec<i16>` allocation on every fingerprint call with no quality benefit (chromaprint is lossy-tolerant at 11 kHz mono). Decision: `MonoPcmClip`, its mono extract path, rubato mono resample, and `offset_refinement.rs` all stay `Vec<i16>` and are **out of scope** for this refactor. The f32 change is strictly `MultiChannelPcm` (multi-channel, native-rate, repair/write path). |
| **Canonical f32 scale** | Normalized `[-1.0, 1.0]`, matching Symphonia's own decoded buffers and the existing `float_to_i16` convention in `extract.rs`. *Rejected:* raw-magnitude f32 (what `rubato.rs` does today, `f32::from(i16)`) — inconsistent with the rest of the codebase and would force a rescale at every boundary. |
| **Where conversion happens now** | Delete the `float_to_i16` call sites at `extract.rs:850,882` (interleaved path only); the decode loop pushes the Symphonia-native `f32` sample directly into `MultiChannelPcm`. The function itself at `:1190` stays — the mono path call sites `:913,954` still use it for `MonoPcmClip`. |
| **Resample boundary — interleaved** | `resample_interleaved` in `rubato.rs:129` currently deinterleaves to per-channel `MonoPcmClip` (i16) and calls `resample_mono_pcm`. After the change the function takes/returns `&[f32]`/`Vec<f32>` and runs rubato directly per channel (own `FftFixedIn<f32>` loop, not through `MonoPcmClip`) to avoid an f32→i16→f32 round-trip. The mono path (`resample_mono_pcm`, `linear_resample_fallback`) is unchanged. |
| **Repair domain math** | `policies.rs::interleaved_to_mono/channels` currently widen `&[i16]` to `Vec<f64>` for downmix/correlation math. With `f32` input already at full mantissa precision relative to any real source, widen to `f64` only where an existing algorithm specifically needs it (e.g. long correlation sums where f32 accumulation error matters) — otherwise keep `f32` throughout to avoid needless alloc/copy. Decide per call site during Phase 1 implementation; default to f32 unless a test regresses. |
| **Bit-depth detection source** | Read `AudioCodecParameters::bits_per_sample` and `sample_format` (Symphonia, confirmed present in `symphonia-core` 0.6.0) at probe time. Add `AudioTrack.bit_depth: Option<BitDepth>` where `BitDepth` is a small enum (`Int16`, `Int24`, `Int32`, `Float32`, `Other(u32)`) derived from `(bits_per_sample, sample_format)`. `None` when Symphonia reports neither (typical for lossy codecs: AAC/MP3/AC-3/Opus/Vorbis) — these fall back to the existing 16-bit default. |
| **Output depth resolution** | New pure function `resolve_output_bit_depth(source: Option<BitDepth>) -> WavBitDepth` (`WavBitDepth::{Int16, Int24}` — no `Float32` variant; output caps at 24-bit int per the Goal/Non-goals above): `Int24`, `Int32`, **or `Float32`** source → `Int24` out; `Int16` or `None` → `Int16` out (today's behavior, unchanged for lossy-codec sources). *Rejected:* 32-bit float output — explicit non-goal. *Rejected:* a CLI flag to force depth in v1 — detect-and-use is the explicit ask; a future `--bit-depth` override flag is a natural follow-up, not required now. |
| **f32 → output-depth conversion** | New `infrastructure/pcm_depth.rs` (or extend `infrastructure/pcm.rs`) with `f32_to_i16` and `f32_to_i24`. **Confirmed via `hound` 3.5.1 source (`hound-3.5.1/src/write.rs`):** for `bits_per_sample: 24`, `WavWriter::write_sample` takes an `i32` in the *true* 24-bit signed range `-8_388_608..=8_388_607` (i.e. scale by `2^23`, not `i32::MAX`/left-justified) — out-of-range values are a hard write error. hound packs each sample as 3 raw bytes on disk (`bytes_per_sample = 3`), no 4-byte padding. So `f32_to_i24(s: f32) -> i32 { (s.clamp(-1.0, 1.0) * 8_388_607.0).round() as i32 }`. Centralize so `wav_writer.rs` and (if enabled) `ffmpeg_mux.rs`/`pcm.rs` share one conversion, not two independently-maintained roundings. |
| **`write_pcm_s16le`** | Rename/generalize to `write_pcm_le(writer, samples: &[f32], depth: WavBitDepth)` in `pcm.rs`; mux pipe format string (`-f s16le` / `-f s24le`) selected to match. |
| **MultiChannelPcm/MonoPcmClip metadata** | Add `source_bit_depth: Option<BitDepth>` to `MultiChannelPcm` (carried from the `AudioTrack` used to decode it) so the writer doesn't need a second lookup. `MonoPcmClip` (fingerprint-rate, lossy-tolerant use only) does **not** need this field — nothing downstream writes a `MonoPcmClip` to a file. |
| **Scale-sensitive thresholds audit** | `policies::is_silent` and `is_silent_channel` take an `absolute_rms_floor: f32` parameter calibrated to i16 scale (e.g. a floor of ~`500.0` is ~1.5% of full scale). After the f32 change, the same callers would be passing i16-scale constants into normalized math, making them ~32767× too large. Phase 1 must audit every call site of `is_silent`/`is_silent_channel` and convert their `absolute_rms_floor` values to normalized scale (`floor / 32767.0`). Similarly, any hardcoded amplitude constant in `gap_energy.rs`, `gap_fill_fit.rs`, or `patch_audio.rs` that assumes i16 range must be rescaled. `compute_fill_gain` takes two RMS values as a ratio — scale-agnostic as long as both sides switch simultaneously, which they will. |
| **Existing test fixtures using `hound` at 16-bit** | Untouched — they construct *input* WAVs for tests, independent of the production writer's output decision. No change required unless a test specifically wants to assert 24-bit/float round-tripping (new tests, Phase 3). |

---

## Phases

### Phase 1 — internal f32 conversion (no behavior change in output bit depth)

**Scope: `MultiChannelPcm` only. `MonoPcmClip`, mono extract path, rubato mono resample, and `offset_refinement.rs` are all out of scope (see Decisions).**

- [x] `domain/multichannel_pcm.rs`: `samples: Vec<i16>` → `Vec<f32>`; add `source_bit_depth: Option<BitDepth>`.
- [x] `domain/audio_track.rs`: add `bit_depth: Option<BitDepth>` (new small enum in `domain/`, e.g. `domain/bit_depth.rs`).
- [x] `infrastructure/symphonia/probe.rs`: populate `AudioTrack.bit_depth` from `AudioCodecParameters::{bits_per_sample, sample_format}` in `probe_from_format`.
- [x] `infrastructure/symphonia/extract.rs`: delete `float_to_i16` call sites at `:850,882` only (interleaved path); push `f32` directly. Leave `:913,954` (mono path feeding `MonoPcmClip`) unchanged.
- [x] `infrastructure/symphonia/extract_loop.rs`: change `InterleavedCollectContext.out: Vec<i16>` → `Vec<f32>` only; leave `MonoExtractSink.mono_samples` as `Vec<i16>`.
- [x] `infrastructure/resample/rubato.rs`: `resample_interleaved(samples: &[i16], ...) -> Vec<i16>` → `(&[f32], ...) -> Vec<f32>`; deinterleave to `Vec<f32>` per channel and run rubato directly (no `MonoPcmClip` intermediate — that would require i16 round-trip). Leave `resample_mono_pcm` and `linear_resample_fallback` unchanged.
- [x] `clip-sync-repair/src/domain/policies.rs`: `is_silent`, `rms_i16` → `rms_f32`, `rms_interleaved`, `interleaved_to_mono/channels` operate on `&[f32]`; drop the `Vec<f64>` widen unless a specific test demands it.
- [x] `clip-sync-repair/src/domain/policies.rs` — **scale audit**: every hardcoded `absolute_rms_floor` value converted from i16-scale to normalized (`old_value / 32767.0`). `compute_fill_gain` confirmed unaffected (ratio consumer).
- [x] `clip-sync-repair/src/domain/gap_energy.rs`, `gap_fill_fit.rs`: `&[i16]`/`Vec<i16>` → `&[f32]`/`Vec<f32>`; amplitude constants rescaled.
- [x] `clip-sync-repair/src/domain/gap_signature.rs`: `build_gap_signature` and `effective_mode` — `samples: &[i16]` → `&[f32]`.
- [x] `clip-sync-repair/src/domain/gap_structure.rs`: `build`, `build_gap_context_signature`, `match_gap_structure_in_b`, `activity_bins` — `samples: &[i16]` → `&[f32]`; `write_frame` test helper likewise.
- [x] `clip-sync-repair/src/application/patch_region.rs`: `SeamGateParams.b_samples: &'a [i16]` → `&'a [f32]`.
- [x] `clip-sync-repair/src/application/patch_audio.rs`:
  - `b_samples: Vec<i16>` → `Vec<f32>`
  - `b_gained` gain application: clamp to `[-1.0, 1.0]` instead of i16 range
  - `slice_b_segment`: `&[i16]` → `&[f32]`
  - `splice_into_a`: `&mut [i16]`, `&[i16]` → `&mut [f32]`, `&[f32]`
  - `compute_a_border_rms`: `f64::from(s: i16)` → `s as f64` on `f32` samples
- [x] `clip-sync-repair/src/infrastructure/pcm.rs`: `write_pcm_s16le` converts `f32` → `i16` at this single boundary; full generalization deferred to Phase 2.
- [x] All tests updated to `Vec<f32>` normalized `[-1.0, 1.0]`; all threshold constants rescaled including `tests/gap_corpus/manifest.toml` (`absolute_silence_floor: 33.0` → `0.001007`) and `gap_corpus_fixtures.rs` default.
- [x] `cargo test --workspace` green (261 lib + 28 patch_audio integration + all other suites).

### Phase 2 — source-driven output bit depth

- [ ] `domain/bit_depth.rs`: `BitDepth` enum + `resolve_output_bit_depth(Option<BitDepth>) -> WavBitDepth` pure function (unit-testable in isolation, per Decisions table).
- [ ] `infrastructure/pcm_depth.rs` (new) or extend `infrastructure/pcm.rs`: `f32_to_i16`, `f32_to_i24`; shared by writer and mux.
- [ ] `infrastructure/wav_writer.rs`: resolve `WavSpec` from `audio.source_bit_depth` via `resolve_output_bit_depth`; write via the shared conversion helpers instead of hardcoding `bits_per_sample: 16`.
- [ ] `infrastructure/pcm.rs::write_pcm_s16le` → `write_pcm_le(writer, samples: &[f32], depth: WavBitDepth)`.
- [ ] `infrastructure/ffmpeg_mux.rs` (under `ffmpeg-mux` feature): pipe format string follows resolved depth (`-f s16le` / `-f s24le` only — no float pipe, consistent with the 24-bit int output cap); confirm ffmpeg's AAC encoder accepts both as input (it does).
- [ ] `validate_pcm_for_wav` (`infrastructure/pcm.rs`, referenced from `wav_writer.rs:10`): confirm its multiple-of-channels / non-empty checks are depth-agnostic; add depth to any size/overflow checks if needed (24-bit uses 3 bytes/sample vs 2, so the 4 GiB WAV limit is reached at ⅔ the frame count — recheck the existing "exceeds 4 GiB" `--mux` hint in `wav_writer.rs:37-42` against this).
- [ ] CLI/progress text: if `--verbose`, log resolved output depth alongside the existing source/codec info (mirrors `format_description()` pattern in `audio_track.rs`).

### Phase 3 — tests + docs

- [ ] Unit tests: `resolve_output_bit_depth` for every `BitDepth` input including `None`.
- [ ] Integration test: WAV/FLAC 24-bit int source fixture → repaired output is 24-bit WAV; assert via `hound::WavReader` spec on the output.
- [ ] Integration test: 32-bit float WAV source fixture → repaired output is **24-bit int** WAV (capped, not float-out).
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
| Float source → 24-bit int out | Phase 3: 32-bit float WAV source → 24-bit int WAV output (output depth capped, not float-out) |
| Lossy-codec fallback | Phase 3: AAC/AC-3 source still produces 16-bit WAV (no regression) |
| 4 GiB WAV-limit hint | Phase 2: recheck `wav_writer.rs`'s existing "use --mux instead" message threshold against 24-bit (3 bytes/sample vs 2) — limit is reached at ⅔ the frame count of a 16-bit file |
| Mux pipe format | Phase 2 (if `ffmpeg-mux` enabled): pipe format string matches resolved depth; existing mux integration tests stay green |

## Exit criteria

- `MultiChannelPcm` (the repair/write path) uses `Vec<f32>` end-to-end from decode through analysis, resample, and write. `MonoPcmClip` (fingerprint path) intentionally remains `Vec<i16>`.
- `AudioTrack` carries detected `bit_depth` from Symphonia codec params where available.
- WAV output bit depth is resolved from the source track's detected depth, not hardcoded.
- Lossy-codec sources (no detectable bit depth) continue to produce 16-bit WAV — no regression for the common case.
- `ffmpeg-mux` pipe format (if enabled) matches the resolved output depth.
- Phase 1 lands with byte-identical output to today for all-16-bit-equivalent sources (i.e., the f32 conversion alone is behavior-preserving); Phase 2 is the only phase that changes output bytes.

## Open questions

None outstanding — all decisions resolved above.

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
- `crates/clip-sync-repair/src/domain/gap_signature.rs`
- `crates/clip-sync-repair/src/domain/gap_structure.rs`
- `crates/clip-sync-repair/src/application/patch_region.rs`
- `crates/clip-sync-repair/src/infrastructure/pcm.rs`
- `crates/clip-sync-repair/src/infrastructure/wav_writer.rs`
- `crates/clip-sync-repair/src/infrastructure/ffmpeg_mux.rs`
- `crates/clip-sync/src/application/offset_refinement.rs` (no change — MonoPcmClip path)
