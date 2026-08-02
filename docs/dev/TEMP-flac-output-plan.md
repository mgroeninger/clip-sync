# TEMP — In-process `--flac` lossless output

**Status:** draft plan, 2026-08-02 (rev: overwrite / encoder contract / `--bit-depth`).
Working plan for a production lossless audio write path that is a peer of `--wav`,
implemented **in-process** (no ffmpeg spawn).

Companion: [archive/repair-write-path-plan.md](archive/repair-write-path-plan.md),
[archive/f32-pcm-bitdepth-plan.md](archive/f32-pcm-bitdepth-plan.md),
[BACKLOG.md](../../BACKLOG.md) § *Repair R6 follow-ups* (streaming / large PCM).

**Motivation:** Classic RIFF WAV data chunks are `u32`-sized. Our write path already
rejects payloads over `u32::MAX` bytes in `validate_pcm_for_wav`
(`infrastructure/pcm.rs`) with a “use `--mux`” hint. FLAC has no that RIFF limit and
compresses multi-channel masters, so long surround repairs stay inspectable without
re-encoding through a lossy mux codec.

---

## 1. Goal / non-goals

**Goal**
- CLI `--flac <PATH>` (and config `repair.output.flac_path`) writes the patched
  multi-channel PCM as a standalone `.flac` file.
- Bit depth: **default** = source-driven `resolve_output_bit_depth` (same as WAV today);
  **override** via `--bit-depth 16|24` / `repair.output.bit_depth` (applies to WAV, FLAC,
  and mux PCM so dual sinks stay consistent).
- **In-process encode only** — no `ffmpeg` / `ffmpeg-mux` dependency for this path.
- Available in the default repair binary (same as `--wav` / hound).

**Non-goals (v1)**
- ffmpeg-backed FLAC (rejected for this plan; mux may still pass `audio_codec = "flac"`
  independently when `ffmpeg-mux` is enabled).
- 32-bit float FLAC / WAV output.
- Channel-layout / WAVEFORMATEXTENSIBLE-style speaker masks in FLAC metadata.
- Streaming encode that avoids holding full patched PCM in memory (full-file PCM is
  already resident today; chunked *encode* is nice-to-have if the crate API supports it).
- Changing patch / decode / splice behavior.
- Replacing `--wav` or removing the classic-WAV size guard.

---

## 2. Current write-path baseline

```text
PatchAudio::execute → MultiChannelPcm
       │
       ├─ WavPatchedAudioWriter  (--wav)   hound, validate_pcm_for_wav
       └─ FfmpegMediaMuxer       (--mux)   ffmpeg feature, lossy by default (aac)
```

| Seam | Where | Reuse for FLAC |
|------|-------|----------------|
| Output port | `application/ports.rs` `PatchedAudioWriter` | New impl; same trait |
| Bit depth | `clip_sync::resolve_output_bit_depth` / `WavBitDepth` | Shared resolve + new optional override |
| Quantize | `infrastructure/pcm.rs` `f32_to_i24` (+ existing i16 scale in wav writer) | Share; maybe extract `f32_to_i16` helper if not already shared |
| WAV size guard | `validate_pcm_for_wav` | **Do not call** on FLAC path |
| Wiring | `RepairFileOutput` / `PendingRepairWrite` / `composition.rs` | Parallel `flac_path` field |
| CLI | `args.rs` `--wav` / `--mux` | Add `--flac` + `--bit-depth`; preview exclusivity via config validate |
| Decode FLAC (read) | Symphonia (`flac` feature on `clip-sync`) | Roundtrip tests only |

Production WAV writer today:

```15:17:crates/clip-sync-repair/src/infrastructure/wav_writer.rs
    fn write(&self, audio: &MultiChannelPcm, path: &Path) -> Result<(), RepairError> {
        let depth = resolve_output_bit_depth(audio.source_bit_depth);
        validate_pcm_for_wav(audio, depth)?;
```

---

## 3. Encoder choice (locked direction + spike gate)

**Direction: prefer `flac-codec` (encode module), fall back to `flacenc` only if the
spike fails.**

| Crate | Version (as of plan) | License | Why |
|-------|----------------------|---------|-----|
| **`flac-codec`** | 1.3.2 | MIT/Apache-2.0 | Stable 1.x; `encode::FlacSampleWriter` path-based API; RFC 9639 oriented; optional `rayon` |
| `flacenc` | 0.5.x | Apache-2.0 | Pure-Rust; examples use `MemSource` + full `Stream` then `ByteSink` — easier spike, worse peak memory (all `i32` samples + bitstream) and 0.x API churn |

**Phase 0 spike (block implementation on this):** in a throwaway bin or unit test under
`clip-sync-repair`, prove for **6ch / 48 kHz / 16-bit and 24-bit**:

1. Interleaved `i32` samples encode to a standalone `.flac` via `FlacSampleWriter::create`.
2. Symphonia (existing `MediaReader`) decodes it back; RMS / sample equality within
   quantization of the integers we fed the encoder.
3. Peak RSS during encode is acceptable vs WAV write of the same PCM (no accidental
   full-buffer duplication beyond one `i32` staging buffer if required).
4. MSRV: crate declares `rust-version = 1.88`; confirm CI/toolchain is ≥ that (workspace
   currently pins no `rust-version`; CI uses `dtolnay/rust-toolchain@stable`).
5. Confirm overwrite + `total_samples: None` finalize behavior (see §4a).

If (1)–(3) fail on `flac-codec`, document why and switch the plan’s crate pin to
`flacenc` before Phase 1 wiring.

**Rejected:** shelling out to `flac` / `ffmpeg` for `--flac`.

---

## 4. Design

### 4a. Encoder contract (`flac-codec` — locked)

**API to use:** `FlacSampleWriter::create` (standalone file with STREAMINFO / seektable).
Do **not** use `FlacStreamWriter` for `--flac` — that writes raw frames without normal
file metadata and is not a peer of `--wav`.

**Layout:** `MultiChannelPcm.samples` is already interleaved
`[ch0₀, ch1₀, …, ch0₁, ch1₁, …]`. `FlacSampleWriter::write(&[i32])` wants the same
order. Matching that layout is **only** about channel order; it does **not** put us
under the classic-WAV 4 GiB ceiling.

**4 GiB / size:** The RIFF limit lives solely in `validate_pcm_for_wav` (payload bytes
as `u32`). The FLAC path calls `validate_pcm_layout` (+ channel bounds) only — never
the WAV byte guard. FLAC STREAMINFO’s sample-count field is 36-bit (inter-channel
frames); there is no RIFF-style 4 GiB data-chunk cap. A long surround master can exceed
4 GiB of *uncompressed* PCM and still encode.

**Overwrite (locked):** `Options::default()` **refuses** an existing path.
`--wav` (hound) overwrites. Use `Options::default().overwrite()` so re-runs replace
`out.flac` the same way.

**Channels (locked):** crate requires `channels` in `1..=8` (`u8`). Our PCM is `u16`.
Reject with `RepairError::Write` when `channels == 0` or `channels > 8` (clear message).
6ch surround is in range.

**Bit depth — default from source, override allowed (locked):**

| Mode | Behavior |
|------|----------|
| Default (no flag / TOML unset) | `resolve_output_bit_depth(audio.source_bit_depth)` — same as WAV today (lossy / unknown → 16; Int24/Int32/Float32/Other(>16) → 24) |
| Override | `--bit-depth 16\|24` or `repair.output.bit_depth = 16\|24` forces that `WavBitDepth` |

Override applies to **WAV, FLAC, and mux PCM** in the same run (one resolve helper, e.g.
`resolve_write_bit_depth(source, override)`) so `--wav` + `--flac` cannot disagree.
v1 values: **16 and 24 only** (no float32). Invalid values → config/CLI error.

Quantize to signed integers in the chosen depth, widen to `i32` for the encoder
(`f32_to_i16`-style / existing `f32_to_i24`), then `write` chunks of interleaved `i32`.

**`total_samples` (demoted — not a design risk):**
Constructor takes `total_samples: Option<u64>`. Prefer **`None`**: `finalize()` updates
STREAMINFO with the actual count, so we do not have to guess whether the crate counts
frames vs interleaved `i32`s when `Some(...)` is set. Phase 0 confirms `None` +
`finalize` yields a playable file Symphonia accepts. If the crate requires `Some` in
practice, pass `Some(audio.frames() as u64)` (FLAC “total samples” = inter-channel
frames) and verify in the spike — do not pass `samples.len()` for multi-channel.

**Encoder options:** defaults + `.overwrite()`; no compression CLI knob in v1.

### 4a‑bis. Writer sketch

New `infrastructure/flac_writer.rs`:

```rust
pub struct FlacPatchedAudioWriter {
    /// When set, forces output depth; else source-driven resolve.
    pub bit_depth_override: Option<WavBitDepth>,
}

impl PatchedAudioWriter for FlacPatchedAudioWriter {
    fn write(&self, audio: &MultiChannelPcm, path: &Path) -> Result<(), RepairError> {
        let depth = resolve_write_bit_depth(audio.source_bit_depth, self.bit_depth_override);
        validate_pcm_layout(audio)?;
        validate_flac_channels(audio.channels)?; // 1..=8
        // Options::default().overwrite()
        // FlacSampleWriter::create(path, options, rate, bits, channels as u8, None)
        // quantize f32 → i32 @ depth, write interleaved, finalize
    }
}
```

- Mirror WAV’s tracing fields (`bits_per_sample`, rate, channels, frames).
- Map encoder errors through `RepairError::Write` with path prefix (same pattern as
  `wav_writer.rs`).
- Thread the same `bit_depth_override` into `WavPatchedAudioWriter` (and mux resolve)
  when the flag/TOML is set.

### 4b. Config / CLI

| Surface | Change |
|---------|--------|
| `RepairOutputConfig` | `flac_path: Option<PathBuf>`; `bit_depth: Option<u8>` (or enum) — only `16` / `24` |
| `Args` | `--flac <PATH>`; `--bit-depth 16\|24`; `--flac` implies write mode like `--wav` |
| `cli/mod.rs` apply | set `flac_path` **and** `dry_run = false` (same block as wav); apply `bit_depth` |
| `RepairConfig::validate` | preview ⊕ any of `{wav, flac, video}` paths; reject invalid `bit_depth` |

**Combination policy (locked):** `--flac` is independent of `--wav` / `--mux`. Writing
both WAV and FLAC in one run is allowed (same PCM, two sinks). Preview remains
exclusive of all write sinks.

**Note:** `--wav` does **not** use clap `conflicts_with` for preview — rejection is
`RepairConfig::validate` after `apply_cli_overrides`. Mirror that for `--flac` (do not
invent a clap-only conflict that wav lacks).

### 4b‑bis. WAV run map → FLAC parity checklist

How `--wav` actually participates in a run today, and what FLAC must do at each site.
Treat this table as the Phase 2 DoD for orchestration (encoder quality is Phase 0/1).

```text
CLI/TOML → apply_cli_overrides → validate → pending_after_scan
         → ScanGaps → (Write) PatchAudio::execute → gated_pcm → writer(s)
         → optional --gap-fingerprints (unless --mux)
         → print_repair_outcome (Output: <path>)
```

| # | Site | `--wav` behavior today | `--flac` must |
|---|------|------------------------|---------------|
| 1 | **CLI parse** (`args.rs`) | `--wav <PATH>` optional | `--flac <PATH>` optional, same shape |
| 2 | **Apply overrides** (`cli/mod.rs`) | sets `output.wav_path`, forces `dry_run = false` | sets `output.flac_path`, forces `dry_run = false` |
| 3 | **TOML alone** | `wav_path` + `dry_run = false` enters write without CLI | same for `flac_path` |
| 4 | **Preview exclusivity** (`config.rs` validate) | `repair_preview` + `wav_path`/`video_path` → error | include `flac_path` in that OR |
| 5 | **Preview apply order** | `--repair-preview` after outputs sets `dry_run = true` again; validate still fails if paths set | no special case — same apply order |
| 6 | **Mode select** (`pending_after_scan`) | preview wins first; else write if `!dry_run` && path | `wants_flac` joins `wants_wav \|\| wants_mux` |
| 7 | **`pending_repair_write`** | early-out if `dry_run`; else require wav and/or mux path | require wav and/or **flac** and/or mux |
| 8 | **Carry path** | `PendingRepairWrite.wav_path` → `RepairWriteRequest` → `RepairFileOutput` | parallel `flac_path` field on all three |
| 9 | **Patch vs scan-only** | Write arm runs full `PatchAudio::execute` (splice); scan-only only validates `--only-gaps` | same Write arm — flac does not invent a fourth mode |
| 10 | **Selection flags** | `--only-gaps` / `--skip-gaps` / profiles / gates apply via `patch_settings` before write | unchanged; flac is sink-only |
| 11 | **`gated_pcm`** | no patches → skip file write; phase *"skipping WAV/mux output"* | same gate; broaden message to *"WAV/FLAC/mux"* |
| 12 | **`wants_file_output`** | true if wav (or mux) path set — drives the skip message | true if wav **or flac** (or mux) |
| 13 | **Actual encode** | `write_wav_if_requested` after gate | `write_flac_if_requested` beside it (same PCM ref) |
| 14 | **With `--mux`** | wav written first, then mux (both if both paths set) | order: **wav → flac → mux** |
| 15 | **With `--gap-fingerprints`** | **both run** (double decode); only `--mux` suppresses fingerprints | **same as wav** — do **not** suppress fingerprints for `--flac` |
| 16 | **Human `Output:` line** (`print_repair_outcome`) | mux over wav when both | **mux → wav → flac** |
| 17 | **JSON / report** | path not embedded in gap JSON; only human `Output:` | same — no new JSON field required in v1 |
| 18 | **Bit depth** | `resolve_output_bit_depth` inside wav writer | shared `resolve_write_bit_depth` (+ optional override) in wav, flac, mux |
| 19 | **Size guard** | `validate_pcm_for_wav` before hound | **omit** on flac; only layout + channel bounds |
| 20 | **Error type** | `RepairError::Write` | same |
| 21 | **Default build** | always available (hound) | always available (in-process crate) — not behind `ffmpeg-mux` |
| 22 | **Unit/CLI tests** | `cli_wav_integration`, config preview+wav reject, bit-depth writer tests | peer tests for flac + `--bit-depth` override cases |

**Intentional non-parity (document, don’t “fix”):**

| Behavior | WAV | FLAC |
|----------|-----|------|
| Classic ~4 GiB data-chunk ceiling | reject before write | allow |
| Encoder / crate | hound | flac-codec (or fallback) |
| Channel count | hound accepts our `u16` as-is | reject `0` or `> 8` |
| Docs hint when WAV too large | suggest `--flac` / `--mux` | n/a |

**Anti-patterns to avoid while wiring:**

- Treating `--flac` like `--mux` for fingerprint suppression.
- Clap-only preview conflict without updating `RepairConfig::validate` (TOML `flac_path` + `repair_preview` would slip through).
- Entering Write mode on `flac_path` while `dry_run` is still true (forgot to clear dry_run in apply, or forgot `wants_flac` in `pending_repair_write`).
- Writing FLAC when `has_patches()` is false (must share `gated_pcm`).
- Extension sniffing on a shared `--wav` path instead of a dedicated flag/field.
- Calling `validate_pcm_for_wav` from the FLAC writer.
- Using `FlacStreamWriter` for a standalone `.flac` path.

### 4c. Application wiring

`RepairVideos` today takes a single `PW: PatchedAudioWriter` used only for WAV.
Options (pick at implement time; prefer the smallest diff):

1. **Second reference** — `flac_writer: &'r dyn PatchedAudioWriter` (or second type
   param). `write_outputs` calls wav and/or flac when paths are set.
2. **Small dispatcher** — one concrete type that holds both writers and routes by
   which path is requested (still two `write` calls).

Do **not** overload `WavPatchedAudioWriter` by extension sniffing; keep codecs in
separate types.

Touch list (expected):

- `application/repair_videos.rs` — `RepairWriteRequest`, `RepairFileOutput`,
  `wants_file_output`, `write_*_if_requested`
- `application/run_repair.rs` — pending write struct
- `composition.rs` — `pending_repair_write`, inject `FlacPatchedAudioWriter`,
  `output_written` preference **mux → wav → flac**
- `infrastructure/config.rs` — `flac_path`, `bit_depth`, serde defaults, validate
- `infrastructure/wav_writer.rs` / mux resolve — honor `bit_depth` override
- `infrastructure/cli/args.rs` — `--flac`, `--bit-depth`; update `--repair-preview`
  help text that today names only `--wav` / `--mux`

### 4d. WAV size-error copy

Update `validate_pcm_for_wav` message from “use `--mux` …” to mention `--flac` as the
lossless alternative, e.g. `use --flac (or --mux) for long surround outputs instead of
--wav`. Behavior of the guard itself unchanged.

### 4e. Docs to update when shipping (Phase 3)

| Doc | Change |
|-----|--------|
| [`docs/pipeline.md`](../pipeline.md) | Orchestration table: write mode = `--wav` / `--flac` / `--mux`; §5 Write / mux bullet for `--flac`; note `--bit-depth` |
| [`README.md`](../../README.md) | Flag table + write-output blurb; report-only sentence that names only wav/mux today |
| [`docs/gap-repair-guide.md`](../gap-repair-guide.md) | Modes / output bit-depth rows that mention only `--wav` |
| [`docs/cli-output.md`](../cli-output.md) | Only if a human line / warning copy names write sinks |
| [`docs/dev/development.md`](development.md) | Test matrix row if a new `[[test]]` is added |
| `args.rs` help | `--repair-preview` exclusive-with copy includes `--flac` |

---

## 5. Phases / checklist

### Phase 0 — encoder spike (gate)

- [ ] Spike `FlacSampleWriter::create` + `.overwrite()` for 2ch + 6ch, 16- and 24-bit;
      Symphonia roundtrip
- [ ] Confirm `total_samples: None` + `finalize` is enough (else `Some(frames)`)
- [ ] Confirm CI Rust ≥ crate MSRV; license already MIT/Apache-2.0 (OK)
- [ ] If spike fails → switch recommendation to `flacenc` and re-spike memory

### Phase 1 — writer + unit tests

- [ ] `Cargo.toml`: add `flac-codec` (default dep on `clip-sync-repair`)
- [ ] `resolve_write_bit_depth` (+ unit tests for override vs source default)
- [ ] `infrastructure/flac_writer.rs` + `mod` export; channel `1..=8` reject tests
- [ ] Unit/integration: write tone / silence → decode with Symphonia → assert depth +
      sample agreement (mirror `wav_bit_depth_integration.rs` patterns)
- [ ] 16-bit and 24-bit source-driven cases; forced `--bit-depth` cases

### Phase 2 — CLI / config / composition

- [ ] Walk §4b‑bis parity table (#1–#22); each row either implemented or marked N/A
- [ ] `--flac`, `flac_path`, `--bit-depth` / `bit_depth`, validate preview⊕flac,
      dry-run clear on apply, `wants_flac` in pending write
- [ ] `write_outputs` invokes FLAC writer when path set (after `gated_pcm`); order
      wav → flac → mux; `Output:` mux → wav → flac
- [ ] Fingerprints: still run with `--flac` (only `--mux` suppresses)
- [ ] Config roundtrip + preview+flac reject tests; CLI integration peer of
      `cli_wav_integration`

### Phase 3 — docs / polish

- [ ] Walk §4e docs table
- [ ] Update classic-WAV limit error string to mention `--flac`
- [ ] Archive this plan when shipped

---

## 6. Verification (DoD)

1. `clip-sync-repair A B --flac out.flac` writes a playable FLAC; exit 0 on success.
2. Roundtrip decode matches quantized PCM for Int16 and Int24 output depths
   (source-driven and `--bit-depth` override).
3. Payload that would fail `validate_pcm_for_wav` **succeeds** on `--flac` (construct
   via unit test with synthetic frame count if full multi-GiB is impractical — assert
   the FLAC path does not call the WAV guard; optional long-run soak outside CI).
4. `--repair-preview --flac x` is rejected by `RepairConfig::validate` after
   `apply_cli_overrides` (same mechanism as `--wav`; **not** a clap `conflicts_with`).
5. Re-run with the same `--flac` path overwrites the prior file.
6. `channels == 0` or `> 8` → write error; 6ch succeeds.
7. Default build (no `ffmpeg-mux`) includes `--flac`.
8. `cargo test -p clip-sync-repair` green for new tests; no ffmpeg required.

---

## 7. Risks

| Risk | Mitigation |
|------|------------|
| Encoder API forces a full second `i32` buffer | Chunk `write` if API allows; else one staging `Vec<i32>` and document peak memory |
| Surround channel count / order oddities in players | Same as WAV today; document “channel count preserved, no speaker mask”; reject >8 |
| 0.x/`flacenc` churn if fallback used | Pin exact version; wrap behind our writer so call sites stay stable |
| Encode CPU on long 6ch masters | Acceptable for v1; optional `rayon` feature later if measured slow |
| Gap-listen calibration WAVs still hit 4 GiB if someone exports huge windows | Out of scope here; listen plan can reuse `FlacPatchedAudioWriter` later |

---

## 8. Follow-ups (not this plan)

- Gap-listen mode ([TEMP-gap-listen-wav-plan.md](TEMP-gap-listen-wav-plan.md)) switching
  surround exports to FLAC by default or via a flag.
- Chunked patch PCM so neither WAV nor FLAC needs the full timeline resident
  (BACKLOG R6 “Streaming / chunked WAV encode” — broaden to lossless writers).
- Mux-path documentation that `audio_codec = "flac"` is ffmpeg-only and unrelated to
  `--flac`.

---

## 9. Open naming (minor)

- Flag is `--flac <PATH>` (parallel to `--wav`). Rejected: overloading `--wav` by
  extension; rejected: `--lossless` meta-flag in v1.
- Config key: `repair.output.flac_path` (parallel to `wav_path`).
- Bit-depth flag: `--bit-depth 16|24` / `repair.output.bit_depth` (shared across sinks).
