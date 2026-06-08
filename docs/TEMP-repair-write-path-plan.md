# Temporary plan: repair write path (track match, multi-channel patch, WAV + optional ffmpeg)

> **Status:** Not started. Archive to `docs/archive/repair-write-path-plan.md` when shipped.

**Problem:** `clip-sync-repair` today is report-only (migration Phase 4). It aligns A and B, scans A for silent runs, and flags whether B has energy at the aligned position (`Gap.b_has_energy`). It cannot answer "do the tracks match (surround/stereo)?", cannot patch B's audio into A, has no normalization, scans only one direction (A→B), and the whole media pipeline is **mono-only** (`MediaSession::extract_mono` → `MonoPcmClip`).

**Goal:** Extend the repair hexagon to the full workflow:

1. Compare A/B track topology (channel layout, sample rate) and report compatibility.
2. Surface overlap (already computed as `AlignmentResult.start_overlap`).
3. Scan **both** timelines for silence; use co-occurring (mutual) silence as an independent cross-check of `recommended_offset_secs`.
4. Patch B's audio into A's gaps with crossfade + optional loudness normalization.
5. Emit a **multi-channel WAV** via a Rust-native write path (default deliverable, no external deps).
6. Emit corrected **video** via an ffmpeg `MediaMuxer` adapter behind a **Cargo feature flag** (`ffmpeg-mux`, off by default).

**Workspace split:** Multi-channel/native extraction lives in **`crates/clip-sync`** (new port method + Symphonia adapter + facade re-export — single decode stack, reuses session reuse / seek / decode-skip logic). All repair domain/use cases/adapters (track match, gap fill, normalization, bidirectional scan, WAV writer, ffmpeg mux) live in **`crates/clip-sync-repair`**. Repair never imports `clip_sync::infrastructure::*`.

---

## Decisions

Locked before implementation. Change only with an explicit plan revision.

| Topic | Decision |
|-------|----------|
| **Mono-only blocker** | Add a **separate** native extraction path; do **not** change the alignment path. Alignment keeps `extract_mono` @ `target_sample_rate` (11 kHz). Fill uses full channels at native rate. |
| **Where multi-channel decode lives** | Extend lib port `MediaSession` with `extract_interleaved(...) -> MultiChannelPcm` (default impl returns `MediaError::Unsupported` so existing fakes don't break); implement in `SymphoniaMediaSession`; re-export `MultiChannelPcm` on the facade. **Not** a second symphonia dependency in repair. |
| **PCM sample format** | `i16` interleaved, consistent with `MonoPcmClip`. f32 deferred — revisit only if normalization/crossfade clipping shows up in tests. |
| **Track compatibility verdict** | Pure policy `assess_track_compatibility(&AudioTrack a, &AudioTrack b) -> TrackCompatibility`. Verdict ∈ `Identical | Compatible | Mismatch`. Channel-count mismatch → `Mismatch` (no auto up/downmix in v1); sample-rate mismatch → `Compatible` (resample B to A on fill). Report always; **never** hard-fail in report-only mode. |
| **Fill channel/rate handling** | Patch into A's native track layout. If B's channel count differs → fill is **skipped** for that gap with `reason = "track layout mismatch"` (still reported). If B's rate differs → resample B segment to A's rate (reuse lib `resample` via a facade helper or repair-local linear fallback — see [Open questions](#open-questions)). |
| **Bidirectional scan** | Scan B's own timeline for silence in addition to probing B at A's gap positions. Default **on** when write path runs; configurable `scan_both`. Reuse `is_silent`. |
| **Mutual-silence cross-check** | Independent offset estimate: find the shift that maximizes overlap of A-silence and B-silence intervals; compare to `recommended_offset_secs`. Report `gap_offset_agreement` (delta in seconds + agree bool within tolerance). Diagnostic only in v1 — never overrides alignment offset. |
| **Alignment gate (scan vs fill)** | **Scan** and **fill** are separate concerns. Low-confidence alignment (`recommended_offset_secs: none`) is **not** an error (exit **0**). Still scan A for silence on A's clock (and B on B's clock when `scan_both`). **Do not** map B timeline positions or probe `b_has_energy` without a recommended offset. **Do not** early-return with an empty gap list solely because alignment failed — R3 needs silence intervals for mutual-silence cross-check. **Patch** (R4) gates on `recommended_offset_secs`, shared overlap, `b_has_energy`, `min_fill_correlation`, and channel match. |
| **Normalization** | Match B fill segment loudness to A's audio **bordering** the gap (RMS of `normalize_window_secs` on each side of the gap in A; fall back to global A RMS if borders are silent). Apply scalar gain, clamp to `max_fill_gain_db`. Off → 0 dB. Pure policy `compute_fill_gain`. |
| **Crossfade** | Linear equal-power crossfade of `crossfade_ms` at each gap boundary between A and gain-adjusted B. Pure policy. |
| **Write path default** | **WAV-first.** `PatchAudio` produces an in-memory `MultiChannelPcm`; default `PatchedAudioWriter` writes multi-channel WAV (`hound`, promoted from dev-dep to dep). Exit **0** on successful WAV write. |
| **ffmpeg** | `MediaMuxer` ffmpeg-subprocess adapter compiled only under `--features ffmpeg-mux`. ffmpeg **not** a Cargo dependency (subprocess on PATH). Without the feature, `--mux` is rejected at arg-parse with a clear message. WAV path always available. |
| **dry-run semantics** | `--dry-run` (default **true** until write path ships, then default **false** once `--output`/`--wav` given) gates all file writes. Report-only when no output path set. |
| **Repair errors** | Extend `RepairError` with `Write(io)` (reuse code 4) and `Mux(String)` (new exit code **6**). Keep `Align` boundary wrapping. |
| **No lib AppError changes** | All new failure modes are repair-local. ffmpeg/mux never touches lib. |
| **Phasing** | R0 spike → R1 native extraction (lib) → R2 track match + surface overlap → R3 bidirectional scan + cross-check → R4 gap-fill + WAV → R5 ffmpeg mux (feature). Each phase ships green with tests. |

> **Phase naming:** `R0`–`R5` are *repair feature* phases. They sit on top of completed migration Phase 4 (report-only) and supersede the single deferred "migration Phase 5 (ffmpeg write path)" with a finer breakdown.

---

## Config

### Repair (`RepairConfig` in `crates/clip-sync-repair/src/infrastructure/config.rs`)

Extend the existing struct (current fields: `min_gap_ms`, `silence_peak_fraction`, `scan_window_secs`):

```rust
pub struct RepairConfig {
    // existing
    pub min_gap_ms: u64,
    pub silence_peak_fraction: f32,
    pub scan_window_secs: u64,

    // R3 — bidirectional scan + cross-check
    #[serde(default = "default_true")]
    pub scan_both: bool,
    #[serde(default = "default_gap_offset_tolerance_secs")]
    pub gap_offset_tolerance_secs: f64,   // default 0.5 (matches OFFSET_AGREEMENT_TOLERANCE_SECS)

    // R4 — fill
    #[serde(default = "default_min_fill_correlation")]
    pub min_fill_correlation: f32,        // default 0.35 (gate before splicing B into A)
    #[serde(default = "default_crossfade_ms")]
    pub crossfade_ms: u64,                 // default 10
    #[serde(default = "default_true")]
    pub normalize_fill: bool,
    #[serde(default = "default_normalize_window_secs")]
    pub normalize_window_secs: f64,        // default 5.0 (A border RMS window each side)
    #[serde(default = "default_max_fill_gain_db")]
    pub max_fill_gain_db: f64,             // default 12.0 (clamp)

    // R4/R5 — output
    #[serde(default)]
    pub output: RepairOutputConfig,
    #[serde(default = "default_true")]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RepairOutputConfig {
    pub wav_path: Option<PathBuf>,         // multi-channel WAV (R4)
    pub video_path: Option<PathBuf>,       // ffmpeg mux output (R5, requires feature)
    #[serde(default = "default_video_codec")]
    pub video_codec: String,               // "copy"
    #[serde(default = "default_audio_codec")]
    pub audio_codec: String,               // "aac"
}
```

TOML:

```toml
[repair]
min_gap_ms = 100
silence_peak_fraction = 0.01
scan_window_secs = 60
scan_both = true
min_fill_correlation = 0.35
crossfade_ms = 10
normalize_fill = true
dry_run = true

[repair.output]
wav_path = "patched.wav"
# video_path = "repaired.mkv"   # requires `--features ffmpeg-mux`
video_codec = "copy"
audio_codec = "aac"
```

### CLI (`crates/clip-sync-repair/src/infrastructure/cli/args.rs`)

New flags (override config):

| Flag | Effect |
|------|--------|
| `--wav <PATH>` | Set `output.wav_path`; implies write mode (`dry_run = false`) |
| `--mux <PATH>` | Set `output.video_path`; **requires** `ffmpeg-mux` feature, else arg error; implies write mode |
| `--dry-run` / `--write` | Force report-only / force write |
| `--no-normalize` | `normalize_fill = false` |
| `--crossfade-ms <MS>` | Override crossfade |
| `--scan-both` / `--no-scan-both` | Toggle bidirectional scan |

---

## Types

### Library (`crates/clip-sync/src/domain/multichannel_pcm.rs`, re-export on facade)

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct MultiChannelPcm {
    pub sample_rate: u32,
    pub channels: u16,
    /// Interleaved i16 frames: samples.len() == frames * channels.
    pub samples: Vec<i16>,
    pub decode_error_skips: u32,
    /// Frames decoded before any end-of-window padding.
    pub decoded_frame_count: Option<usize>,
}

impl MultiChannelPcm {
    pub fn frames(&self) -> usize { self.samples.len() / self.channels.max(1) as usize }
    pub fn duration_secs(&self) -> f64 { self.frames() as f64 / self.sample_rate.max(1) as f64 }
}
```

Port addition (`crates/clip-sync/src/application/ports.rs`):

```rust
pub trait MediaSession {
    fn list_tracks(&self) -> Result<Vec<AudioTrack>, MediaError>;
    fn extract_mono(&self, /* unchanged */) -> Result<MonoPcmClip, MediaError>;

    /// Native-rate, all-channels extract for the repair fill path.
    /// Default returns `MediaError::Unsupported` so non-Symphonia fakes opt in only when needed.
    fn extract_interleaved(
        &self,
        track: &AudioTrack,
        window: &ClipWindow,
        progress: &dyn ProgressReporter,
        label: &str,
    ) -> Result<MultiChannelPcm, MediaError> {
        let _ = (track, window, progress, label);
        Err(MediaError::Unsupported("extract_interleaved".into()))
    }

    fn reset_io(&self) -> Result<(), MediaError> { Ok(()) }
}
```

Add `MediaError::Unsupported(String)` — the existing `MediaError::UnsupportedFormat(String)` is format-specific and not a fit for "operation not implemented by this session" (see `application/error.rs`).

### Repair domain (`crates/clip-sync-repair/src/domain/`)

```rust
// track_match.rs
pub enum CompatibilityVerdict { Identical, Compatible, Mismatch }

pub struct TrackCompatibility {
    pub a_channels: u16, pub b_channels: u16,
    pub a_sample_rate: u32, pub b_sample_rate: u32,
    pub channels_match: bool,
    pub rate_match: bool,
    pub verdict: CompatibilityVerdict,
}

pub fn assess_track_compatibility(a: &AudioTrack, b: &AudioTrack) -> TrackCompatibility;

// gap_fill.rs
pub struct FillRegion {
    pub a_start_secs: f64, pub a_end_secs: f64,
    pub b_start_secs: f64, pub b_end_secs: f64,
    pub gain: f32,            // normalization scalar (1.0 = unchanged)
    pub crossfade_secs: f64,
}
pub struct GapFillPlan { pub regions: Vec<FillRegion> }

// policies.rs (extend existing is_silent)
pub fn compute_fill_gain(a_border_rms: f32, b_segment_rms: f32, max_gain_db: f64) -> f32;
pub fn apply_crossfade(into: &mut [i16], fill: &[i16], channels: u16, crossfade_frames: usize);
pub fn rms_interleaved(samples: &[i16]) -> f32;
```

### Repair domain — report extensions (`domain/gap.rs`)

```rust
// Gap — A windows always on A's clock. B fields only when alignment produced an offset.
pub struct Gap {
    pub video_a_start_secs: f64,
    pub video_a_end_secs: f64,
    /// B timeline when `recommended_offset_secs` is known; `None` when alignment failed.
    pub video_b_start_secs: Option<f64>,
    pub video_b_end_secs: Option<f64>,
    /// Probed only when B positions are known and the window lies on B's media.
    pub b_has_energy: bool,
}

impl Gap {
    pub fn is_fillable(&self) -> bool {
        self.video_b_start_secs.is_some() && self.b_has_energy
    }
}

// add to GapReport
pub track_compatibility: TrackCompatibility,
pub overlap: Option<TimelineOverlap>,            // copied from alignment.start_overlap
pub gap_offset_agreement: Option<GapOffsetAgreement>,  // R3 cross-check

pub struct GapOffsetAgreement {
    pub silence_based_offset_secs: f64,
    pub alignment_offset_secs: f64,
    pub delta_secs: f64,
    pub agrees: bool,
}
```

`TimelineOverlap` is already re-exported? **No** — audit facade; add `TimelineOverlap` to the `domain` re-export block in lib `lib.rs` (same fix pattern as `HighRateRefinement`, gap #7 in the archived refactor doc).

---

## Phases

### R0 — Spike: native multi-channel extraction

**Lib (`clip-sync`)**

- [ ] Prototype `extract_interleaved` for one stereo WAV via existing Symphonia session; confirm interleaved i16, correct frame count, native rate
- [ ] Confirm session reuse / seek path works for an arbitrary mid-file window (not just `start == 0`)
- [ ] Record whether resampling B→A rate needs `rubato` (lib) or a repair-local linear fallback suffices

**Repair:** none

### R1 — Native extraction port (lib only)

**Lib (`clip-sync`)**

- [ ] `domain/multichannel_pcm.rs` — `MultiChannelPcm`; facade re-export
- [ ] `MediaError::Unsupported(String)` (if absent)
- [ ] `MediaSession::extract_interleaved` default + `SymphoniaMediaSession` impl (reuse decode-skip + shortfall logic from `extract.rs`)
- [ ] Re-export `TimelineOverlap` on facade `domain` block
- [ ] Lib tests: stereo extract frame count, channel deinterleave round-trip, mid-file window, decode-skip surfaced

**Repair:** none

### R2 — Track compatibility + surface overlap (repair, report-only)

**Repair (`clip-sync-repair`)**

- [ ] `domain/track_match.rs` — `assess_track_compatibility` + unit tests (identical / rate-only / channel mismatch)
- [ ] `application/scan_gaps.rs` — capture `track_a`/`track_b`, build `TrackCompatibility`; copy `alignment.start_overlap` into report
- [ ] `application/scan_gaps.rs` — **alignment gate:** when `recommended_offset_secs` is `None`, still emit A silent windows; set `video_b_*` to `None`, skip B session open and `b_has_energy` probe (never `unwrap_or(0.0)` on B positions)
- [ ] `domain/gap.rs` — `video_b_start_secs` / `video_b_end_secs` → `Option<f64>`; tighten `is_fillable()`; add `track_compatibility`, `overlap` to `GapReport`
- [ ] `infrastructure/cli/output.rs` — human + JSON lines for track match + overlap window; when offset is `none`, note that gaps are A-only (not fillable)
- [ ] Tests: report includes compatibility + overlap; JSON shape; failed alignment → A gaps present, `video_b_*` null, `fillable_count == 0`

**Lib:** none

### R3 — Bidirectional scan + mutual-silence cross-check (repair)

**Repair (`clip-sync-repair`)**

- [ ] `application/scan_gaps.rs` — when `scan_both`, also scan B timeline → `Vec<Gap>` on B's clock
- [ ] `application/cross_check.rs` — `silence_based_offset(a_gaps, b_gaps) -> Option<f64>` (shift maximizing silence-interval overlap); `GapOffsetAgreement` vs `recommended_offset_secs`
- [ ] `domain/gap.rs` — `gap_offset_agreement`
- [ ] `infrastructure/cli/output.rs` — agreement line (warn when `!agrees`)
- [ ] Tests: synthetic A/B with co-located silence → offset recovered; disagreement flagged; no shared silence → `None`

**Lib:** none

### R4 — Gap fill → multi-channel WAV (Rust-native write path)

**Repair (`clip-sync-repair`)**

- [ ] `Cargo.toml` — promote `hound` to dependency; add `[features] default = []; ffmpeg-mux = []`
- [ ] `domain/gap_fill.rs` — `FillRegion`, `GapFillPlan`; build plan from fillable gaps (gate on `min_fill_correlation` + channel match)
- [ ] `domain/policies.rs` — `compute_fill_gain`, `apply_crossfade`, `rms_interleaved` + unit tests
- [ ] `application/ports.rs` — `PatchedAudioWriter { fn write(&self, audio: &MultiChannelPcm, path: &Path) -> Result<(), RepairError>; }`
- [ ] `application/patch_audio.rs` — `PatchAudio`: extract A native full timeline (chunked), for each `FillRegion` extract B via `extract_interleaved`, resample if needed, normalize, crossfade-splice → patched `MultiChannelPcm`
- [ ] `infrastructure/wav_writer.rs` — `WavPatchedAudioWriter` (hound, multi-channel)
- [ ] `application/error.rs` — `RepairError::Write(io)`; map to exit code 4
- [ ] `infrastructure/cli/{args,mod}.rs` — `--wav`, `--no-normalize`, `--crossfade-ms`, write-mode wiring
- [ ] Tests: gap-fill splice on synthetic stereo (gap in A, energy in B) → patched WAV has B audio in gap, A elsewhere; normalization gain bounded; crossfade continuity; integration test writing a real WAV via Symphonia

**Lib:** none (consumes R1 `extract_interleaved`)

### R5 — ffmpeg video mux (behind `ffmpeg-mux` feature)

**Repair (`clip-sync-repair`)**

- [ ] `application/repair_videos.rs` — `RepairVideos`: `PatchAudio` → `PatchedAudioWriter` (temp WAV) → `MediaMuxer`
- [ ] `infrastructure/ffmpeg_mux.rs` (`#[cfg(feature = "ffmpeg-mux")]`) — implement existing `MediaMuxer` port via ffmpeg subprocess (`-i source -i wav -map ... -c:v copy -c:a aac`)
- [ ] Flesh out `MediaMuxer` trait signature (currently an empty stub) per PLAN §Repair application
- [ ] `application/error.rs` — `RepairError::Mux(String)` → exit code 6; `exit_code.rs` table update
- [ ] `infrastructure/cli/{args,mod}.rs` — `--mux <PATH>`; arg error when feature off
- [ ] `docs/error-mapping.md` — repair exit codes 4 (write) / 6 (mux)
- [ ] Tests: `#[ignore]` integration mux (needs ffmpeg on PATH); arg-rejected-without-feature test; unit test of ffmpeg arg construction

**Lib:** none

---

## Design

### Native extraction vs alignment extraction

```text
alignment path (unchanged):  extract_mono   -> MonoPcmClip @ 11 kHz mono   (Chromaprint)
repair fill path (new):      extract_interleaved -> MultiChannelPcm @ native rate, all channels
```

Two distinct extracts on the same `MediaSession` (reuses the open format reader + decoders). The fill path never downsamples A's program audio.

### Alignment gate and gap semantics (R2+)

Repair conflates two jobs if treated as one pipeline:

| Job | Purpose | Requires offset? |
|-----|---------|------------------|
| **Silence audit** | Find silent runs on each file's native timeline | No |
| **Fill assessment / patch** | Map A dropouts to B and splice B audio into A | Yes (+ overlap, correlation, layout) |

**When `recommended_offset_secs` is `None`** (low-confidence alignment, inconsistent clips, etc.):

- Exit **0** — analysis completed; same as analyzer semantics.
- **Continue** scanning A for silence (chunked `extract_mono` on A's clock).
- **Do not** open B for offset-mapped probes; `video_b_start_secs` / `video_b_end_secs` stay `null` in JSON; `b_has_energy` is always `false`; `is_fillable()` is always `false`.
- **Do not** skip the A scan or return an empty `gaps` list — R3 bidirectional scan and mutual-silence cross-check need silence intervals even when Chromaprint alignment failed.
- Human output: alignment block shows `n/a (alignment failed)`; gap section labels all entries **unfillable** (or adds a one-line note that B mapping was skipped).

**When `recommended_offset_secs` is `Some(Δ)`:**

- Map each A gap to B via `b_start = a_start + Δ`, `b_end = a_end + Δ`.
- Probe B energy only when `b_start >= 0` and the window decodes.
- Copy `alignment.start_overlap` into the report; R4 additionally requires the gap to fall inside the shared overlap region before splicing (gap outside overlap → reported, not filled).

**R4 patch gate** (all must pass for a gap to enter `GapFillPlan`):

```text
recommended_offset_secs.is_some()
&& gap.is_fillable()                    # B mapped + b_has_energy
&& gap within shared overlap (if known)
&& boundary correlation >= min_fill_correlation   # when correlation is implemented
&& track_compatibility.verdict != Mismatch        # channel layout
```

Mutual-silence `gap_offset_agreement` (R3) is **diagnostic only** — it never overrides `recommended_offset_secs` or enables fill in v1.

### Patch assembly (R4)

```text
PatchAudio(report, session_a, session_b, cfg):
  a = extract_interleaved(track_a, [0, dur_a))          # chunked to bound memory
  for gap in report.gaps where gap.b_has_energy:
    if !channels_match(track_a, track_b): skip (reason)
    b_seg = extract_interleaved(track_b, [gap.b_start, gap.b_end))
    if b_seg.rate != a.rate: b_seg = resample(b_seg, a.rate)
    gain = normalize_fill ? compute_fill_gain(a_border_rms(gap), rms(b_seg), max_db) : 1.0
    splice b_seg * gain into a at [gap.a_start, gap.a_end) with crossfade(crossfade_ms)
  return a   # patched MultiChannelPcm
```

Memory note: A's full native PCM for a long file is large (stereo 48 kHz ≈ 11 MB/min). Acceptable for v1; chunked streaming write is a Defer item.

### Mutual-silence cross-check (R3)

```text
silence_based_offset(a_gaps, b_gaps):
  # Each gap is an interval. Find Δ maximizing Σ overlap(a_gap, b_gap + Δ).
  candidates = { b.start - a.start for a in a_gaps, b in b_gaps }   # boundary-aligned shifts
  pick Δ maximizing total silence-interval intersection; require >= 1 overlapping pair
agrees = |Δ_silence - recommended_offset_secs| <= gap_offset_tolerance_secs
```

Diagnostic: confirms alignment using a signal (silence structure) independent of Chromaprint/PCM correlation. Never overrides the offset in v1.

### Normalization (R4)

```text
a_border_rms(gap) = rms( A samples in [gap.a_start - W, gap.a_start) ∪ [gap.a_end, gap.a_end + W) )
                    fall back to global A RMS if borders are silent/empty
gain = clamp(a_border_rms / b_segment_rms, +/- max_fill_gain_db)
```

Equal-power linear crossfade over `crossfade_frames` at both seams to avoid clicks.

---

## Tests

| Test | Phase | Crate | Asserts |
|------|-------|-------|---------|
| `extract_interleaved_stereo_frame_count` | R1 | lib | frames == rate × secs; channels == 2 |
| `extract_interleaved_midfile_window` | R1 | lib | mid-file seek returns expected window |
| `assess_compat_identical / rate_only / channel_mismatch` | R2 | repair | verdict mapping |
| `report_includes_compatibility_and_overlap` | R2 | repair | fields populated; JSON shape |
| `failed_alignment_emits_a_gaps_without_b_mapping` | R2 | repair | `recommended_offset_secs: null` → A gaps listed, `video_b_*` null, `fillable_count == 0` |
| `silence_offset_recovered_from_mutual_gaps` | R3 | repair | co-located silence → Δ ≈ true offset |
| `gap_offset_disagreement_flagged` | R3 | repair | wrong silence layout → `agrees = false` |
| `compute_fill_gain_clamps_to_max_db` | R4 | repair | gain bounded |
| `apply_crossfade_is_continuous` | R4 | repair | no discontinuity at seam |
| `patch_inserts_b_audio_into_a_gap` | R4 | repair | gap region energy from B; rest from A |
| `patched_wav_roundtrip` (integration) | R4 | repair | real Symphonia → WAV written, channel count preserved |
| `mux_arg_rejected_without_feature` | R5 | repair | `--mux` errors when feature off |
| `ffmpeg_arg_construction` | R5 | repair | correct ffmpeg argv |
| `mux_writes_video` (`#[ignore]`) | R5 | repair | needs ffmpeg on PATH |

---

## Open questions

- **Resampler reuse:** the lib has `resample_mono_pcm` in `domain/resample.rs` (rubato-backed with linear fallback) but it is **mono-only and not on the facade**. Either add a facade `resample_interleaved` helper (per-channel rubato), or implement a repair-local linear resampler. Prefer the facade helper for quality; accept a repair-local linear fallback for v1 if facade churn is undesirable.
- **Channel up/downmix:** v1 skips fill on channel-count mismatch. A future phase could map stereo→5.1 fronts or downmix. Out of scope until a corpus case needs it.
- **Streaming write:** full-timeline A in memory is the simple v1 choice. Chunked encode is deferred until users hit memory pain (mirrors lib BACKLOG "Memory use and PCM cloning").

---

## References

### Library (`crates/clip-sync`)
- `src/application/ports.rs` — `MediaSession::extract_interleaved`
- `src/domain/multichannel_pcm.rs` — **new** `MultiChannelPcm`
- `src/infrastructure/symphonia/extract.rs` — native extract impl (mirror `extract_mono_with_state`)
- `src/lib.rs` — facade re-exports (`MultiChannelPcm`, `TimelineOverlap`)
- `src/application/error.rs` — `MediaError::Unsupported`

### Repair (`crates/clip-sync-repair`)
- `src/domain/{gap.rs, track_match.rs, gap_fill.rs, policies.rs}`
- `src/application/{scan_gaps.rs, cross_check.rs, patch_audio.rs, repair_videos.rs, ports.rs, error.rs}`
- `src/infrastructure/{wav_writer.rs, ffmpeg_mux.rs, config.rs, cli/}`
- `Cargo.toml` — `hound` dep, `[features] ffmpeg-mux`

### Other
- [PLAN.md](../PLAN.md) — target repair architecture (§Repair workflow); write-path section points here
- [BACKLOG.md](../BACKLOG.md) — repair write path tracked under R0–R5
- [docs/error-mapping.md](error-mapping.md) — repair exit codes (4 write, 6 mux)
- [docs/archive/workspace-refactor-gaps.md](archive/workspace-refactor-gaps.md) — migration Phase 5 origin
