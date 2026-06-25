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

### Phase 1 — implementation notes / plan gaps

Items not anticipated in the original checklist that were discovered during implementation. Recorded so the Phase 2 plan can be written with fuller scope coverage.

1. **Integration test helper signatures not covered by the "update every test that constructs `MultiChannelPcm` literals" item.** Three test-local helpers carried `i16` through their API independently of struct literals and each needed its own update:
   - `rms_region(&[i16], ...) -> f32` in `tests/patch_audio_integration.rs` — changed to `&[f32]`
   - `mono_region(&[i16], ...) -> Vec<i16>` in `tests/query_reference_integration.rs` — changed to `&[f32]`/`Vec<f32>`
   - `patch_to_samples(...) -> (Vec<i16>, ...)` in `tests/patch_audio_integration.rs` — reads the written WAV back via hound; changed to divide by 32767.0 and return `Vec<f32>`

2. **Roundtrip assertion thresholds.** `patch_to_samples` reads the output WAV via hound then passes samples to `rms_region`. After both helpers switched to f32, all downstream `> 100.0` / `pre_last > 100.0` / `post_first > 100.0` assertions compared a normalized f32 RMS value against an i16-scale constant, always evaluating false. Each needed `/ 32767.0`. The plan's scale audit was explicitly scoped to `policies.rs` call sites and did not anticipate this category of assertion.

3. **TOML manifest threshold not covered by code-level scale audit.** `tests/gap_corpus/manifest.toml` has `[defaults] absolute_silence_floor = 33.0` which is loaded by serde and overrides the Rust default function entirely. The Rust-side `default_absolute_silence_floor()` function is only used when the field is absent from TOML. Both needed updating: manifest to `0.001007` (≈ 33/32767) and the Rust default to `33.0_f32 / 32767.0` for consistency. Neither was called out in the plan.

4. **Inline sample builders in seam residual test files.** `tests/seam_residual_corpus.rs` and `tests/seam_residual_oracle.rs` build samples via hand-rolled loops assigning `i16` values (`a[f] = val.clamp(-32768, 32767) as i16`), then pass them into `EnergySignatureFixture { a_samples, b_samples, ... }`. These are not `MultiChannelPcm` struct literals so the plan's literal-update item did not cover them. Both needed: loop body changed to f32 (`/ 32767.0`), `absolute_silence_rms` literals inside the same function scaled.

5. **`residual_gate_runner.rs` roundtrip via `read_mono_wav`.** `read_mono_wav()` returns `(u32, Vec<i16>)` from a hound reader. The plan covered updating `gap_report_from_floor_oracle`'s signature to `&[f32]` but not the call site in `tests/common/residual_gate_runner.rs` that feeds the `Vec<i16>` directly into it. Required an inline conversion at the call site: `decoded_a_mono_i16.iter().map(|&s| s as f32 / 32767.0).collect()`.

6. **`extract_window_regression.rs` dual-type `peak_abs` usage.** `peak_abs` was updated to `&[f32]` to handle `MultiChannelPcm.samples`. Line 264 in the same file calls it on `clip.samples` where `clip` is a `MonoPcmClip` (`Vec<i16>`). Required a separate `peak_abs_i16(&[i16]) -> f32` helper alongside the existing one. Not anticipated because the plan treated `MonoPcmClip` usage as "no change" without checking whether any shared test helpers were used for both types.

7. **`repair_videos.rs` test `MultiChannelPcm` literal missed.** The plan enumerated `policies.rs`, `gap_energy.rs`, `gap_structure.rs`, `patch_audio.rs`, and `scan_gaps.rs` inline test data as targets for the literal update, but `src/application/repair_videos.rs` also contained a `MultiChannelPcm { samples: vec![1_000; 100], .. }` in a test helper that was not listed.

**Lesson for Phase 2 scope:** The scale audit must explicitly cover (a) any TOML/config file with hardcoded amplitude or threshold values, (b) test helpers that pass PCM data through a function boundary (not just struct constructors), and (c) assertion thresholds derived from reading back written output (which now goes through a 32767 scale at the WAV boundary).

### Phase 2 — source-driven output bit depth

**Prerequisite state (confirmed after Phase 1):**
- `BitDepth` enum already exists at `clip-sync/src/domain/bit_depth.rs` (Int16, Int24, Int32, Float32, Other(u32))
- `AudioTrack.bit_depth: Option<BitDepth>` already populated from Symphonia at probe time
- `MultiChannelPcm.source_bit_depth: Option<BitDepth>` already carried from decode
- `wav_writer.rs` still hardcodes `bits_per_sample: 16`; f32→i16 conversion already correct at line 30
- `write_pcm_s16le` in `pcm.rs` still s16le-only; used by the `ffmpeg-mux` pipe

**Tasks:**
- [x] `domain/bit_depth.rs`: add `resolve_output_bit_depth(source: Option<BitDepth>) -> WavBitDepth` pure function. `WavBitDepth` is a local enum `{ Int16, Int24 }` (no Float32 output per Non-goals). Rule: `Int24 | Int32 | Float32 | Other(>16)` → `Int24`; `Int16 | None | Other(≤16)` → `Int16`.
- [x] `infrastructure/pcm.rs`: add `f32_to_i24(s: f32) -> i32` helper (scale by `8_388_607.0`, clamp to true 24-bit range); generalize `write_pcm_s16le` → `write_pcm_le(writer, samples: &[f32], depth: WavBitDepth)` (dispatches to s16le or s24le branch). Both writer and mux pipe share this.
- [x] `infrastructure/wav_writer.rs`: resolve `WavBitDepth` from `audio.source_bit_depth`; set `WavSpec.bits_per_sample` and `sample_format` from it; write samples via `f32_to_i16` (existing) or `f32_to_i24` (new). Update the 4 GiB hint at line 37–42: 24-bit uses 3 bytes/sample so the limit is hit at ⅔ the frame count of a 16-bit file.
- [x] `infrastructure/ffmpeg_mux.rs` (under `ffmpeg-mux` feature): replace hardcoded `-f s16le` with depth-resolved format string (`-f s16le` or `-f s24le`) derived from `audio.source_bit_depth`. Confirm ffmpeg accepts s24le PCM as AAC encoder input (it does — `-f s24le` is standard).
- [x] CLI/progress text: log resolved output depth in `format_description()` / verbose output alongside codec info.

**Existing tests safe to leave unchanged in Phase 2:**
- `cli_wav_integration.rs`, `scan_gaps_integration.rs`, `patch_audio_integration.rs`, `query_reference_integration.rs` all write 16-bit input WAVs (`WavSpec { bits_per_sample: 16 }`). Output will remain 16-bit under Phase 2's resolution logic (`Int16 | None → Int16`). Tests that read back via `reader.samples::<i16>()` and compare against i16-scale thresholds (`> 100.0`) remain correct. No changes needed here; Phase 3 adds new tests for 24-bit source paths.
- [x] `pcm_data_bytes` in `pcm.rs:13` hardcodes `* 2` (2 bytes per 16-bit sample). This function is used by `validate_pcm_for_wav` to enforce the 4 GiB classic WAV limit. For 24-bit output the multiplier is `3`, so the limit is hit at ⅔ the frame count. Change signature to `pcm_data_bytes(audio: &MultiChannelPcm, depth: WavBitDepth) -> u64`; update `validate_pcm_for_wav` to take and pass `depth`; update the `validate_pcm_for_wav_rejects_payload_over_limit` test to pass `WavBitDepth::Int16` (existing behavior) and add a companion assertion at `/ 3 + 1` for `Int24`.

### Phase 2 — implementation notes / plan gaps

Items not anticipated in the original checklist that were discovered during implementation.

1. **`build_ffmpeg_mux_args` signature change cascaded to test call sites.** The plan said "replace hardcoded `-f s16le` with depth-resolved format string" but framed it as an in-body substitution. The actual change required adding a `pcm_format: &str` parameter to `build_ffmpeg_mux_args` (since the depth isn't available at the arg-building site — it's computed from PCM metadata), which cascaded to updating all three existing test call sites in the same file. Not anticipated.

2. **`run_ffmpeg_mux_with_progress` needed `depth` threaded through as a parameter.** The plan identified that `write_pcm_s16le` → `write_pcm_le` needed to happen in the mux pipe, but did not note that `run_ffmpeg_mux_with_progress` is a separate private function that takes `args` and `pcm` but not the resolved depth — so depth had to be added as a parameter and threaded from the `MediaMuxer` impl through to the write call. Two sites changed: the function definition and its sole call site.

3. **`write_pcm_s16le` going from `pub` to private required an import update in `ffmpeg_mux.rs`.** The plan described `write_pcm_le` as a generalization of `write_pcm_s16le`, but once `write_pcm_le` became the public interface and `write_pcm_s16le` became a private helper, the existing `use crate::infrastructure::pcm::{..., write_pcm_s16le}` import in `ffmpeg_mux.rs` had to be updated to `write_pcm_le`. Small but easy to overlook.

4. **Existing `format_description` test assertion broke on format string change.** The plan said "log resolved output depth in `format_description()`" but did not flag that this changes an existing output format and breaks the existing test that asserts `"ac3 @ 48000 Hz, 5.1 (decodable)"`. The test needed to be renamed and updated to `"ac3 @ 48000 Hz, 5.1 (decodable, 16-bit out)"`, and a second test added for the 24-bit source case. **Lesson for Phase 3:** any plan item that changes a human-readable string used in an existing assertion should explicitly note that the test will break.

**Lesson for Phase 3 scope:** When a function that is called in multiple places changes its signature (even by adding one parameter), enumerate call sites explicitly in the plan rather than relying on "update call sites" as implied. The compiler will catch them, but enumerating them in the plan prevents mid-implementation surprise.

### Phase 3 — tests + docs

- [x] Unit tests: `resolve_output_bit_depth` for every `BitDepth` input including `None`. *(Done in Phase 2 — `bit_depth.rs` tests cover all variants.)*
- [x] Existing fixed-16-bit fixtures/tests re-verified green. *(Done — `cargo test --workspace` passes as of Phase 2 completion.)*
- [ ] Integration test: WAV/FLAC 24-bit int source fixture → repaired output is 24-bit WAV; assert via `hound::WavReader` spec on the output.
  - Synthesize the fixture programmatically inside the test using `hound::WavWriter` with `WavSpec { bits_per_sample: 24, sample_format: SampleFormat::Int, .. }` — no committed binary fixture needed.
  - Set `source_bit_depth: Some(BitDepth::Int24)` on the `MultiChannelPcm` after decode (or wire a real 24-bit decode path if the corpus has one).
- [ ] Integration test: 32-bit float WAV source fixture → repaired output is **24-bit int** WAV (capped, not float-out).
  - Synthesize with `WavSpec { bits_per_sample: 32, sample_format: SampleFormat::Float, .. }`. Symphonia will report `SampleFormat::F32` → `BitDepth::Float32` → `WavBitDepth::Int24`.
- [ ] Integration test: lossy source (existing AAC/AC-3 fixtures) → output stays 16-bit (no behavior change, regression guard).
- [ ] Mux path: the unit test `ffmpeg_arg_construction_s24le_uses_correct_format` covers arg construction; a full end-to-end mux integration test for 24-bit source → s24le pipe requires ffmpeg present at test time. **Decision needed:** gate behind `#[cfg(feature = "integration")]` and mark `#[ignore]` by default, or treat the unit test + WAV integration test as sufficient coverage and skip the mux end-to-end. Calling it out explicitly so this doesn't get silently deferred.
- [ ] `format_description` now outputs `"(decodable, 16-bit out)"` / `"(decodable, 24-bit out)"`. If any CLI integration test or log capture test asserts on this string verbatim, it will break. Grep for `"decodable"` in test files before writing new Phase 3 integration tests.
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
