# Temporary plan: AC-3 backend selection (oxideav vs ffmpeg)

> **Status:** Draft (2026-06-11). Motivated by production crackle on real AC-3 program audio when built with `--features ac3` (oxideav-ac3 `0.0.8`). Simple sine fixtures decode cleanly; spectrally complex input produces bursts railed to ±32767 — audible as random clicks/chirps; ffmpeg decodes the same files with zero full-scale samples. WAV and mux outputs are equally affected (decode-layer bug, not patch or AAC re-encode).
>
> Archive to `docs/archive/ac3-backend-plan.md` when shipped.

**Problem:** AC-3 support is a single optional Cargo feature (`ac3`) that always links **oxideav-ac3** `0.0.8` into Symphonia’s codec registry. There is no alternative decode path. Repair and alignment therefore inherit oxideav’s DSP quality for every `ac3` / `eac3` track. Our integration tests only assert frame count and peak on a **sine tone** fixture — they do not catch full-scale railing on complex audio.

**Goal:** Keep **`ac3`** as the user-facing capability gate (“this binary can work with AC-3/E-AC-3 files”), but choose the decode implementation at **compile time** via mutually exclusive backend features:

| Backend | Feature | When to use |
|---------|---------|-------------|
| Pure Rust | `ac3-oxideav` | No ffmpeg dependency; acceptable until oxideav quality is fixed |
| Subprocess | `ac3-ffmpeg` | Production repair on real program audio; reference-quality decode |

**Non-goals (v1):** Runtime backend switching; Symphonia `AudioDecoder` shim around ffmpeg; replacing Symphonia demux/probe for non-AC-3 codecs.

**Workspace split:** Shared ffmpeg subprocess helpers (`infrastructure/ffmpeg/process.rs`), AC-3 decode routing, and PCM extract in **`crates/clip-sync`**. **`clip-sync-repair`** mux refactored to use the shared module. Feature forwarding in **`clip-sync-cli`** and **`clip-sync-repair`**. Repair’s **`ffmpeg-mux`** feature stays independent of `ac3-ffmpeg` but shares **`ffmpeg-subprocess`**.

---

## Current codebase baseline

| Area | Path | Current state | Target phase |
|------|------|---------------|--------------|
| Feature gate | `crates/clip-sync/Cargo.toml` | `ac3 = ["dep:oxideav-ac3", "dep:oxideav-core", "dep:symphonia-core"]`; optional deps always pulled with `ac3` | 1 |
| Codec registry | `infrastructure/symphonia/codec_registry.rs` | `ac3` → register `oxideav_ac3::Ac3Decoder` | 1 (oxideav only) |
| AC-3 adapter | `infrastructure/symphonia/oxideav_ac3/decoder.rs` | Symphonia `AudioDecoder`; `reset()` no-op; `NeedMore` → empty buffer | 2 (wire `reset`; document `NeedMore` risk) |
| Probe | `infrastructure/symphonia/probe.rs` | `is_audio_decodable` = registry `make_audio_decoder` | 1 (ffmpeg backend: explicit ac3/eac3) |
| Extract choke points | `infrastructure/symphonia/extract.rs` | `extract_mono_with_state`, `extract_interleaved_with_state`, `scan_mono_buckets_with_state`, `scan_interleaved_buckets_with_state` → `run_extract_decode_loop` | 1 (early ffmpeg branch) |
| `AudioTrack` | `domain/audio_track.rs` | `index` = Symphonia track id; no ffmpeg stream ordinal | 1 (`ffmpeg_audio_index: Option<u32>`) |
| Tests | `media_reader_tests.rs` | `probe_and_extract_ac3_surround_mp4`: sine tone, peak > 100 | 3 (complex-audio regression) |
| Binaries | `clip-sync-repair/Cargo.toml`, `clip-sync-cli/Cargo.toml` | `ac3 = ["clip-sync/ac3"]`; `default = []` | 1 (forward backend features) |
| FFmpeg subprocess | `clip-sync-repair/.../ffmpeg_mux.rs` | Mux-only; ad-hoc spawn / `NotFound` / stderr trim (~133–191); no shared module; no preflight | 1 (lift to shared `process.rs`; refactor mux) |
| FFmpeg test helper | `test_support/ffmpeg_util.rs` | `ffmpeg_available()` via `-version` (tests/corpus only) | 1 (production check reuses shared module, not test_support) |

---

## Decisions

| Topic | Decision |
|-------|----------|
| **Feature shape** | Split into `ac3` (capability marker), `ac3-oxideav`, `ac3-ffmpeg`. **Exactly one** backend required when `ac3` is enabled. Enforce with `compile_error!` in `infrastructure/symphonia/mod.rs` or `lib.rs`. *Rejected:* a single `ac3` feature with `default = ["ac3-oxideav"]` only — hides the ffmpeg path from docs and CI. |
| **User-facing builds** | Document two recipes: `ac3,ac3-oxideav` (pure Rust) and `ac3,ac3-ffmpeg` (recommended for repair until oxideav is fixed). Do **not** make `ac3-ffmpeg` imply `ffmpeg-mux`; mux remains optional. |
| **Default backend policy** | No change to `default = []` on binaries. README / PLAN recommend **`ac3-ffmpeg`** for repair releases while oxideav `0.0.8` has the complex-audio railing bug. Revisit when oxideav ships a fix. |
| **Where ffmpeg hooks** | **Extract-layer bypass**, not `CodecRegistry`. Symphonia still probes containers and lists tracks; when `ac3-ffmpeg` and `track.codec` is `ac3` or `eac3`, the four `*_with_state` extract entry points delegate to `infrastructure/ffmpeg/audio_extract.rs` and return `MultiChannelPcm` / drive bucket callbacks directly. *Rejected:* `FfmpegAc3Decoder` implementing `AudioDecoder` — fights per-packet symphonia loop and session reuse. |
| **Stream mapping** | Add `ffmpeg_audio_index: Option<u32>` on `AudioTrack`: 0-based ordinal among audio tracks at probe time (`-map 0:a:{n}`). Symphonia `track.id` is **not** always equal to ffmpeg’s audio ordinal — do not reuse `index` for `-map`. Populate for all tracks (Some) so future ffmpeg use is uniform. |
| **Mono extract** | ffmpeg path: decode interleaved native layout (`-ac {channels}`), then apply the same downmix policy as symphonia mono extract (`append_mono` / existing domain helper). Do **not** use ffmpeg `-ac 1` unless we audit parity with alignment downmix — interleaved-then-downmix matches current semantics. |
| **Session / `MediaIoState`** | ffmpeg extract does not use `MediaIoState` for AC-3 tracks. Call sites still call `ensure_io()` for non-AC-3 ops on the same session; AC-3 branch ignores the symphonia reader for that extract only. Document that AC-3 + ffmpeg breaks “single demux cursor” assumptions for mixed-codec sessions (acceptable — no production mixed per-packet session today). |
| **Progress** | Reuse `ProgressReporter` with frame-based estimates from window duration × rate (same pattern as extract finalize). Optional: parse ffmpeg `-progress` later; not required for v1. |
| **Errors** | Map subprocess failures to existing `MediaError` (`open_failed` / `decode_failed` with track index). `ffmpeg not found on PATH` when `ac3-ffmpeg` enabled — mirror repair mux wording. No new error enum variants (contract plan). |
| **`decode_error_skips`** | ffmpeg path sets `0` (subprocess either succeeds for the window or fails the whole extract). |
| **E-AC-3** | Treat `eac3` codec string like `ac3` for backend routing. ffmpeg handles both; oxideav adapter already registers both. |
| **oxideav `reset()`** | Phase 2: forward `Ac3Decoder::reset()` to `inner.reset()` when keeping oxideav — cheap hardening, independent of backend choice. |
| **oxideav version** | Phase 2 (optional): bump `oxideav-ac3` when upstream fixes land; re-run Phase 3 parity tests before demoting `ac3-ffmpeg` as recommended default. |
| **Shared ffmpeg subprocess** | Add **`crates/clip-sync/src/infrastructure/ffmpeg/process.rs`** and centralize the patterns today duplicated only in repair mux: spawn `ffmpeg` / `ffprobe`, map `ErrorKind::NotFound` → `"ffmpeg not found on PATH"` (or `"ffprobe not found on PATH"`), drain stderr, **`trim_ffmpeg_stderr`** (last 5 lines, 500-char cap). Refactor **`clip-sync-repair/.../ffmpeg_mux.rs`** to call the shared helpers instead of owning its own copy. New **`clip-sync` feature `ffmpeg-subprocess`** (empty marker) enabled by `ac3-ffmpeg` and forwarded from repair’s `ffmpeg-mux` (`ffmpeg-mux = [..., "clip-sync/ffmpeg-subprocess"]`). Keeps subprocess code in the library hexagon; repair stays a thin caller for mux. |
| **ffmpeg preflight (`ac3-ffmpeg`)** | **Optional startup check** when `ac3-ffmpeg` is enabled: run `ffmpeg -version` once per process (e.g. `std::sync::OnceLock<Result<(), MediaError>>`) on first AC-3 ffmpeg extract (or first `MediaReader::open` that lists an AC-3 track — pick one site, document it). Success: `tracing::debug!` with the version line. Failure / `NotFound`: return the same friendly PATH error as mux, **before** spawning a decode subprocess. **No version pinning** in v1 — do not reject older/newer ffmpeg builds unless a concrete incompatibility is found later. Mux-only builds (`ffmpeg-mux` without `ac3-ffmpeg`) keep today’s lazy behaviour (check only at mux spawn). *Rejected for v1:* mandatory preflight for every ffmpeg invocation; parsing and enforcing a minimum ffmpeg version. |

---

## Phases

### Phase 0 — characterization (before behaviour change)

- [ ] Document reproduction: user’s 256 kb/s AC-3 @ 48 kHz; pops on WAV and mux; ffmpeg PCM reference clean.
- [ ] Script or test helper: count samples with `abs(s) >= 32767` in extracted PCM (oxideav) vs ffmpeg reference on the same window.
- [ ] Pin oxideav issue / version in this plan when a public URL exists.

### Phase 1 — feature split + ffmpeg extract skeleton

- [ ] `Cargo.toml` (`clip-sync`, `clip-sync-cli`, `clip-sync-repair`):
  ```toml
  ffmpeg-subprocess = []   # shared spawn / stderr / version check (no extra deps)
  ac3 = []
  ac3-oxideav = ["ac3", "dep:oxideav-ac3", "dep:oxideav-core", "dep:symphonia-core"]
  ac3-ffmpeg  = ["ac3", "ffmpeg-subprocess"]
  # repair: ffmpeg-mux = ["dep:tempfile", "clip-sync/ffmpeg-subprocess"]
  ```
- [ ] `compile_error!` guards (both backends / neither backend).
- [ ] **`infrastructure/ffmpeg/mod.rs`** + **`process.rs`** (`#[cfg(feature = "ffmpeg-subprocess")]`):
  - `trim_ffmpeg_stderr(stderr: &str) -> String` — move from `ffmpeg_mux.rs` unchanged.
  - `spawn_ffmpeg(args) -> Result<Child, FfmpegProcessError>` — `NotFound` → `ffmpeg not found on PATH`; other IO → `failed to run ffmpeg: {err}`.
  - `run_ffmpeg_collect_stderr(args) -> Result<(), String>` — wait + non-zero → trimmed stderr (mux and simple decode paths).
  - `ffmpeg_version_check() -> Result<(), FfmpegProcessError>` — `ffmpeg -version`; used by `ac3-ffmpeg` preflight only.
  - Optional: `probe_duration_ms` via `ffprobe` (move from mux; `NotFound` silent `None` for progress, or debug log).
- [ ] `clip-sync` feature: `ffmpeg-subprocess = []`; `ac3-ffmpeg = ["ac3", "ffmpeg-subprocess"]`. Repair: `ffmpeg-mux = [..., "clip-sync/ffmpeg-subprocess"]`.
- [ ] Refactor **`clip-sync-repair/.../ffmpeg_mux.rs`** to import `clip_sync::infrastructure::ffmpeg::process` (or a small public facade `clip_sync::ffmpeg_process` if infrastructure stays private — prefer a documented `pub(crate)` / facade re-export to avoid widening the user API).
- [ ] **`ac3-ffmpeg` preflight:** `ensure_ffmpeg_available()` via `OnceLock` + `ffmpeg_version_check()`; call from AC-3 ffmpeg extract entry (before first `spawn_ffmpeg` for decode).
- [ ] `AudioTrack.ffmpeg_audio_index` populated in `probe_from_format`.
- [ ] `infrastructure/ffmpeg/audio_extract.rs` (new module, `#[cfg(feature = "ac3-ffmpeg")]`):
  - `extract_interleaved_ffmpeg(path, track, window, progress, label) -> Result<MultiChannelPcm, MediaError>`
  - `extract_mono_ffmpeg` → interleaved + downmix
  - `scan_*_ffmpeg` for bucket scans (gap repair): decode sequential windows or full-file pipe with byte offsets — prefer **one subprocess per `extract_*` window** for v1 (repair patch already does full-file extract; gap scan uses buckets — mirror symphonia bucket sizing).
- [ ] `extract.rs`: `if ac3_uses_ffmpeg(track) { ... }` at top of four `*_with_state` functions.
- [ ] `codec_registry.rs`: register `Ac3Decoder` only under `ac3-oxideav`.
- [ ] `probe.rs`: under `ac3-ffmpeg`, `is_audio_decodable` returns true for `CODEC_ID_AC3` / `CODEC_ID_EAC3`.
- [ ] `infrastructure/mod.rs`: `mod ffmpeg` sibling to `symphonia/` (not under symphonia — subprocess code is container-agnostic).
- [ ] `cargo test --workspace` with **no** `ac3` features still green.

### Phase 2 — oxideav hardening (parallel-friendly)

- [ ] Wire `Ac3Decoder::reset()` → `inner.reset()`.
- [ ] Audit `NeedMore` empty-buffer path; add debug log when 0-frame decode returned after `send_packet` (packet consumed, no PCM).
- [ ] Optional: bump `oxideav-ac3` pin; changelog note.

### Phase 3 — tests + parity

- [ ] **Complex-audio fixture** (`ffmpeg-tests` + `ac3`): ffmpeg-generate AC-3 with mixed tones + noise (not sine-only); `#[cfg(all(feature = "ac3-oxideav", feature = "ffmpeg-tests"))]` test asserts railed-sample count below threshold (or skip with message if fixture gen fails).
- [ ] **Parity test** (`ac3-ffmpeg` + `ffmpeg-tests`): same fixture; extract via ffmpeg backend; compare RMS/peak and **zero** full-scale samples vs ffmpeg CLI reference (should match by construction).
- [ ] Keep existing sine AC-3 tests on **both** backends (frame count, channels).
- [ ] Repair integration: `scan_gaps` / `patch_audio` smoke with `ac3-ffmpeg` on dual-track AAC+AC-3 fixture (existing `write_dual_track_aac_ac3_mp4` in `ffmpeg_util.rs`).
- [ ] Document build matrix in `PLAN.md` and repair README.

### Phase 4 — docs + backlog

- [ ] `PLAN.md`: AC-3 feature table, backend choice, `ffmpeg-subprocess` / shared `process.rs`, ffmpeg PATH requirement for `ac3-ffmpeg` (optional `-version` preflight, no version pinning).
- [ ] `docs/error-mapping.md`: ffmpeg-missing message for decode (if distinct from mux).
- [ ] `BACKLOG.md`: add completed row; remove any ad-hoc AC-3 crackle notes if added.
- [ ] Archive this plan.

---

## FFmpeg extract API (v1 sketch)

```text
ffmpeg -nostdin -loglevel error \
  -ss {window.start} -t {window.duration} \
  -i {path} \
  -map 0:a:{ffmpeg_audio_index} \
  -vn -f s16le -acodec pcm_s16le \
  -ar {track.sample_rate} -ac {track.channels} \
  pipe:1
```

- **Seek:** `-ss` before `-i` for coarse seek (acceptable for gap buckets and full-file repair); document ±100 ms tolerance vs symphonia if alignment-sensitive windows regress.
- **Full-file repair patch:** single subprocess for `[0, duration)` is fine (already loads full PCM).
- **Alignment clips:** same window API as symphonia extract.
- **Subprocess:** all ffmpeg spawns go through **`infrastructure/ffmpeg/process.rs`** (shared with mux after refactor).

### Shared `process.rs` (v1 sketch)

```text
infrastructure/ffmpeg/
  mod.rs          # cfg(feature = "ffmpeg-subprocess")
  process.rs      # spawn, stderr trim, version check, optional ffprobe duration
  audio_extract.rs  # cfg(feature = "ac3-ffmpeg") — decode to PCM
```

| Helper | Used by |
|--------|---------|
| `trim_ffmpeg_stderr` | mux, AC-3 decode |
| `spawn_ffmpeg` / `run_ffmpeg_collect_stderr` | mux, AC-3 decode |
| `ffmpeg_version_check` + `OnceLock` preflight | `ac3-ffmpeg` only (first extract) |
| `probe_media_duration_ms` (ffprobe) | mux progress (optional for decode progress later) |

Preflight flow (`ac3-ffmpeg`):

```text
first AC-3 ffmpeg extract:
  ensure_ffmpeg_available()  # OnceLock; ffmpeg -version; debug log version line
  if NotFound → MediaError::decode_failed(..., "ffmpeg not found on PATH")
  else spawn decode subprocess via process.rs
```

No minimum version check — any successful `-version` exit is accepted.

---

## Tests

| Concern | Coverage |
|---------|----------|
| Feature guards | `compile_fail` test crate or doc-test build instructions; CI matrix builds `ac3-oxideav` and `ac3-ffmpeg` separately |
| Probe | AC-3 track `decodable: true` under both backends |
| Oxideav regression | Complex-audio railed-sample test (documents known failure until upstream fix) |
| ffmpeg backend | Parity + repair smoke; `ffmpeg not found` error mapping unit test (preflight + spawn paths) |
| Shared `process.rs` | Unit test `trim_ffmpeg_stderr`; mux integration still green after refactor; `NotFound` maps to same user-facing strings as today |
| No regressions | Default workspace `cargo test` (no ac3); `he-aac` + symphonia paths untouched |

## Exit criteria

- `ac3` requires exactly one of `ac3-oxideav` / `ac3-ffmpeg`; clear `compile_error!` otherwise.
- `ac3-ffmpeg` decode produces clean PCM on complex AC-3 fixture (zero full-scale samples).
- `ac3-oxideav` path unchanged for sine fixtures; complex-audio test documents oxideav debt (fails until upstream fix, or passes after bump).
- `AudioTrack` carries stable `ffmpeg_audio_index` for `-map`.
- **`ffmpeg_mux.rs` delegates spawn / `NotFound` / stderr trim to shared `process.rs`** (no duplicated subprocess logic).
- **`ac3-ffmpeg` runs optional one-time `ffmpeg -version` preflight** (debug log only; no version pinning).
- PLAN + build docs list both backend recipes.

## Cross-plan sequencing

| Plan | Interaction |
|------|-------------|
| [TEMP-media-session-redesign-plan.md](TEMP-media-session-redesign-plan.md) | Extract entry points gain `&mut self` and internal seek recovery. Land AC-3 routing **on the post-redesign extract signatures** if that plan is in flight — otherwise implement on current `*_with_state` and rebase the ffmpeg branch when session redesign merges. |
| [TEMP-query-reference-alignment-plan.md](TEMP-query-reference-alignment-plan.md) | Blocked on media-session; AC-3 backend choice does not block query mode but repair on AC-3 sources benefits from `ac3-ffmpeg` first. |
| Repair `ffmpeg-mux` | Independent feature; typical repair build: `--features "ac3,ac3-ffmpeg,ffmpeg-mux"`. |

**Suggested land order:** Phase 0 characterization (can run immediately) → Phase 1 + 3 (`ac3-ffmpeg` shippable) → Phase 2 oxideav hardening → Phase 4 docs. Phase 2 can parallel Phase 3.

---

## Open questions

- **Bucket scan via ffmpeg:** one subprocess per bucket vs one long pipe with byte-range — start with per-window subprocess (simpler; gap scan is offline).
- **CI:** GitHub runner with ffmpeg + `ac3-ffmpeg` matrix job; `ac3-oxideav` complex-audio test allowed to fail (`#[ignore]` or `should_panic`) until oxideav fix — record decision in Phase 3.
- **Public oxideav issue URL:** link in Phase 0 when available.

## References

- `crates/clip-sync/src/infrastructure/symphonia/oxideav_ac3/decoder.rs`
- `crates/clip-sync/src/infrastructure/symphonia/codec_registry.rs`
- `crates/clip-sync/src/infrastructure/symphonia/extract.rs`
- `crates/clip-sync/src/infrastructure/ffmpeg/process.rs` (new — shared subprocess helpers)
- `crates/clip-sync-repair/src/infrastructure/ffmpeg_mux.rs` (refactor to use `process.rs`)
- `crates/clip-sync/src/test_support/ffmpeg_util.rs` (AC-3 fixture writers; tests may later call shared `ffmpeg_version_check` but not required for v1)
