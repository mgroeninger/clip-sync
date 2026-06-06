# Archived plan: real-world corpus & validation harness

> **Status:** Completed and archived (2026-06-06). Superseded by [corpus-matrix.md](../corpus-matrix.md), [corpus-validation.md](../corpus-validation.md), and `tests/corpus/README.md`.
>
> **Goal:** Assemble a manifest-driven test corpus that exercises the wider class of real-world failures (formats, leaders, multi-track, soft alignment failures) and wire it into CI-friendly integration tests.

---

## 1. Outcomes

When this plan is done:

1. **`docs/corpus-matrix.md`** — authoritative case matrix (dimensions × scenarios × expected results).
2. **`tests/corpus/manifest.toml`** — committed Tier-B cases with expected outcomes.
3. **`tests/corpus/`** — small binary fixtures (WAV/MP3/MP4) checked into git.
4. **`src/application/testing/corpus_fixtures.rs`** — Rust generators (Tier A) extending `audio_fixtures.rs`.
5. **`scripts/generate_corpus.sh`** (or `.ps1`) — ffmpeg recipes to regenerate Tier B from a master signal.
6. **Integration tests** — loop manifest + generated cases; assert offset, alignment flags, exit code.
7. **`CLIP_SYNC_CORPUS`** — optional env var for Tier-C local/nightly corpus (documented, not committed).

---

## 2. Case matrix (`docs/corpus-matrix.md`)

Create a markdown doc with the full matrix. Each row is one **case id** used in the manifest.

### 2.1 Dimensions

| Dimension | Values to cover |
|-----------|-----------------|
| Container | `wav`, `mp3`, `mp4`, `mkv` |
| Codec | PCM, MP3, AAC-LC, FLAC (in MKV); optional HE-AAC (`he-aac` feature) |
| Channels | mono, stereo (downmix path) |
| Offset (B vs A) | `0`, `+3s` leader on B, `+30s` leader, `−5s` (B ahead / A delayed content) |
| Duration | `short` (120s), `medium` (900s), `long` (3600s, `#[ignore]`) |
| Tracks | single; dual (program + decoy) |
| Content | `chirp` (primary oracle), `tone` (negative), `near_silence` (soft fail) |
| Clip config | `num_clips=1`, `clip_length=60s` (CI); optional `num_clips=2` for consistency cases |

### 2.2 Case table (initial 20 rows)

Copy this table into `docs/corpus-matrix.md` and keep it in sync with `manifest.toml`.

| Case ID | Tier | Format | Offset | Tracks | Duration | Expected |
|---------|------|--------|--------|--------|----------|----------|
| `wav_baseline_0s` | B | WAV | 0 | 1 | 120s | `offset ≈ 0`, aligned |
| `wav_leader_3s` | B | WAV | +3s | 1 | 120s | `offset ≈ +3`, aligned |
| `wav_leader_30s` | A | WAV | +30s | 1 | 180s | `offset ≈ +30`, aligned |
| `wav_b_ahead_5s` | A | WAV | −5s | 1 | 120s | `offset ≈ −5`, aligned |
| `mp3_leader_3s` | A/B | MP3 | +3s | 1 | 120s | `offset ≈ +3`, aligned |
| `mp3_no_duration_tag` | A | MP3* | +3s | 1 | 120s | open OK, `offset ≈ +3` |
| `mp4_aac_leader_3s` | A/B | MP4/AAC | +3s | 1 | 120s | `offset ≈ +3`, aligned |
| `mkv_flac_leader_3s` | A/B | MKV/FLAC | +3s | 1 | 120s | `offset ≈ +3`, aligned |
| `mp4_stereo_leader_3s` | A/B | MP4/AAC | +3s | 1 stereo | 120s | `offset ≈ +3`, aligned |
| `mp4_dual_track_decoy` | A/B | MP4/AAC | +3s | 2 | 120s | `offset ≈ +3`; correct track |
| `mp4_dual_track_wrong_default` | A/B | MP4/AAC | +3s | 2† | 120s | may fail without `try_all_tracks` |
| `no_overlap_tone_vs_chirp` | B | WAV | n/a | 1 | 60s | `aligned: false`, no recommendation |
| `near_silence_window` | B | WAV | 0 | 1 | 60s | soft fail or `InsufficientAudio`‡ |
| `two_clip_consistent` | A | WAV | +12s | 1 | 180s | start+end agree, `offsets_consistent` |
| `two_clip_inconsistent` | A | WAV | mixed§ | 1 | 180s | `offsets_consistent: false` |
| `long_smoke_60m` | C | WAV | +3s | 1 | 3600s | `offset ≈ +3`; perf smoke, `#[ignore]` |
| `he_aac_mp4_leader_3s` | A | MP4/HE-AAC | +3s | 1 | 120s | feature `he-aac` + ffmpeg |
| `reencode_pair_mp3_vs_mp4` | A | MP3 vs MP4 | +3s | 1 | 120s | same master, cross-container |
| `refine_disabled_vs_enabled` | A | WAV | +3s | 1 | 120s | offset within tolerance both |
| `require_consistent_fail` | A | WAV | +10/+20‖ | 1 | 180s | no `recommended_offset_secs` |

Legend:

- **Tier A** = generated in test via Rust + ffmpeg. **Tier B** = committed under `tests/corpus/`. **Tier C** = local only.
- `MP3*` = encoded with duration tag stripped (stress probe path).
- `2†` = decoy is higher sample rate / louder; tests track-selection policy.
- `‡` = documents desired behavior once clip-skip is implemented; may be `#[ignore]` until then.
- `§` = B has different trim in end window (generate with unlike tail).
- `‖` = two-clip config, different offsets per window (synthetic).

### 2.3 Tolerance policy

Document in the matrix:

| Assertion | CI default |
|-----------|------------|
| `offset_secs` | ±1.0 s (Chromaprint item granularity ~0.12 s; headroom for encode) |
| `confidence` | ≥ 0.5 for positive cases |
| `exit_code` | 0 for soft failures; 4/5 only for cases that expect hard errors |

---

## 3. Repository layout

```text
docs/
  corpus-matrix.md              # case matrix (this plan §2)
  archive/corpus-implementation-plan.md   # archived implementation plan (this file)

tests/
  corpus/
    manifest.toml               # Tier B: paths + expectations
    README.md                   # how to regenerate, size budget, licenses
    wav/
      baseline_0s_a.wav
      baseline_0s_b.wav
      leader_3s_a.wav
      leader_3s_b.wav
      ...
    mp3/
      ...
    mp4/
      ...

src/application/testing/
  audio_fixtures.rs             # existing chirp WAV pair
  corpus_fixtures.rs            # NEW: manifest types + Tier A generators
  mod.rs                        # export corpus_fixtures

scripts/
  generate_corpus.ps1           # Windows-first (primary dev env)
  generate_corpus.sh            # POSIX equivalent

# Integration tests live in existing modules OR:
src/application/
  align_videos.rs               # #[cfg(test)] mod corpus_tests; OR
tests/
  corpus_integration.rs         # if we add [[test]] harness later (optional)
```

**Size budget (Tier B):** total committed fixtures **< 5 MB**. Prefer 10–30 s clips, mono, low-bitrate MP3/AAC.

---

## 4. Manifest schema (`tests/corpus/manifest.toml`)

```toml
# Schema version for future migrations
version = 1

# Default test config overrides (CI-fast)
[defaults]
clip_length_secs = 60
num_clips = 1
tolerance_secs = 1.0
min_confidence = 0.5

[[case]]
id = "wav_leader_3s"
tier = "committed"           # committed | generated | external
video_a = "wav/leader_3s_a.wav"
video_b = "wav/leader_3s_b.wav"
expected_offset_secs = 3.0
expect_aligned = true
expect_recommended = true
expect_exit_code = 0

[[case]]
id = "no_overlap_tone_vs_chirp"
tier = "committed"
video_a = "wav/chirp_a.wav"
video_b = "wav/tone_b.wav"
expect_aligned = false
expect_recommended = false
expect_exit_code = 0

[[case]]
id = "mp3_leader_3s"
tier = "generated"           # produced by corpus_fixtures at test time
generator = "offset_chirp_pair"
format = "mp3"
total_secs = 120
offset_secs = 3
expected_offset_secs = 3.0
expect_aligned = true
requires_ffmpeg = true

[[case]]
id = "long_smoke_60m"
tier = "external"
generator = "offset_chirp_pair"
format = "wav"
total_secs = 3600
offset_secs = 3
expected_offset_secs = 3.0
ignore = true                # cargo test --ignored
```

### 4.1 Rust manifest types (pseudo-code)

```rust
// src/application/testing/corpus_fixtures.rs

#[derive(Debug, Deserialize)]
struct CorpusManifest {
    version: u32,
    defaults: CorpusDefaults,
    case: Vec<CorpusCase>,
}

#[derive(Debug, Deserialize)]
struct CorpusDefaults {
    clip_length_secs: u64,
    num_clips: u32,
    tolerance_secs: f64,
    min_confidence: f32,
}

#[derive(Debug, Deserialize)]
struct CorpusCase {
    id: String,
    tier: Tier,                    // Committed | Generated | External
    video_a: Option<PathBuf>,
    video_b: Option<PathBuf>,
    generator: Option<String>,
    format: Option<String>,
    total_secs: Option<u32>,
    offset_secs: Option<u32>,
    expected_offset_secs: Option<f64>,
    expect_aligned: Option<bool>,
    expect_recommended: Option<bool>,
    expect_exit_code: Option<i32>,
    requires_ffmpeg: bool,
    ignore: bool,
    config_overrides: Option<serde_json::Value>,
}

fn corpus_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/corpus")
}

fn resolve_pair(case: &CorpusCase) -> (PathBuf, PathBuf) {
    match case.tier {
        Tier::Committed => (
            corpus_root().join(case.video_a.as_ref().unwrap()),
            corpus_root().join(case.video_b.as_ref().unwrap()),
        ),
        Tier::Generated => generate_case(case),
        Tier::External => {
            let base = env::var("CLIP_SYNC_CORPUS").expect("CLIP_SYNC_CORPUS not set");
            generate_into(Path::new(&base), case)
        }
    }
}
```

---

## 5. Generator design

### 5.1 Master signal

All synthetic cases derive from one **chirp** (already in `audio_fixtures.rs`):

```rust
// Existing: chirp_sample(sample_rate, index) — 300–700 Hz sweep
// write_mono_wav(path, sample_rate, samples)
```

**Pipeline:**

```text
chirp PCM (Rust)  →  master.wav  →  ffmpeg transforms  →  case pair (a, b)
```

For Tier B regeneration, `scripts/generate_corpus` writes `master.wav` once, then all committed fixtures.

### 5.2 Rust generators (`corpus_fixtures.rs`)

```rust
/// Pseudo-code API

pub fn write_master_chirp_wav(path: &Path, sample_rate: u32, total_secs: u32) { ... }

pub fn write_offset_pair_wav(
    dir: &Path,
    sample_rate: u32,
    total_secs: u32,
    offset_secs: u32,
) -> (PathBuf, PathBuf) {
    // Delegates to existing write_offset_chirp_wav_pair
}

pub fn write_offset_pair_via_ffmpeg(
    dir: &Path,
    format: ContainerFormat,   // Wav | Mp3 | Mp4Aac | MkvFlac
    sample_rate: u32,
    total_secs: u32,
    offset_secs: u32,
    options: &FfmpegGenOptions,
) -> Result<(PathBuf, PathBuf), CorpusError> {
    // 1. write_master_chirp_wav(master.wav)
    // 2. ffmpeg delay → b_master_delayed.wav
    // 3. ffmpeg encode → a.ext, b.ext
}

pub struct FfmpegGenOptions {
    pub strip_mp3_duration: bool,
    pub stereo: bool,
    pub second_track_decoy: bool,   // dual-track MP4
    pub decoy_louder: bool,
}

pub fn ffmpeg_available() -> bool { Command::new("ffmpeg").arg("-version").status().ok()?.success() }
```

### 5.3 ffmpeg pseudo-code (generators)

#### 5.3.1 B delayed by N seconds (leader on B)

```bash
# Inputs: MASTER.wav (chirp), OFFSET_SECS
# Outputs: a.wav (or encoded), b.wav (or encoded)

ffmpeg -y -i MASTER.wav -c copy a.wav

ffmpeg -y -i MASTER.wav \
  -af "adelay=${OFFSET_MS}|${OFFSET_MS}" \
  b_delayed.wav

# For encoded output, replace -c copy with codec args (see below).
```

PowerShell equivalent:

```powershell
$offsetMs = $OffsetSecs * 1000
ffmpeg -y -i master.wav -c copy a.wav
ffmpeg -y -i master.wav -af "adelay=${offsetMs}|${offsetMs}" b_delayed.wav
```

#### 5.3.2 MP3 pair

```bash
ffmpeg -y -i MASTER.wav -c:a libmp3lame -q:a 2 a.mp3
ffmpeg -y -i b_delayed.wav -c:a libmp3lame -q:a 2 b.mp3
```

#### 5.3.3 MP3 without duration tag (probe stress)

```bash
ffmpeg -y -i MASTER.wav \
  -write_xing 0 -id3v2_version 0 \
  -c:a libmp3lame -b:a 128k a_no_duration.mp3
```

#### 5.3.4 MP4 / AAC-LC

```bash
ffmpeg -y -i MASTER.wav -c:a aac -b:a 128k -movflags +faststart a.mp4
ffmpeg -y -i b_delayed.wav -c:a aac -b:a 128k -movflags +faststart b.mp4
```

#### 5.3.5 MKV / FLAC

```bash
ffmpeg -y -i MASTER.wav -c:a flac a.mkv
ffmpeg -y -i b_delayed.wav -c:a flac b.mkv
```

#### 5.3.6 Stereo (downmix path)

```bash
ffmpeg -y -i MASTER.wav -ac 2 -c:a aac -b:a 128k a_stereo.mp4
ffmpeg -y -i b_delayed.wav -ac 2 -c:a aac -b:a 128k b_stereo.mp4
```

#### 5.3.7 Dual audio track (program + decoy)

```bash
# DECOY: low sine, 220 Hz
ffmpeg -y -f lavfi -i "sine=frequency=220:duration=${TOTAL_SECS}" decoy.wav

# Program on track 0, decoy on track 1
ffmpeg -y -i MASTER.wav -i decoy.wav \
  -map 0:a -map 1:a \
  -c:a aac -b:a 128k \
  -shortest dual_a.mp4

# B: delayed program + same decoy (decoy aligned from t=0)
ffmpeg -y -i b_delayed.wav -i decoy.wav \
  -map 0:a -map 1:a \
  -c:a aac -b:a 128k \
  -shortest dual_b.mp4

# “Wrong default” variant: swap loudness so decoy wins sample-rate/channel tie
# → normalize decoy to 0 dBFS, program to −24 dBFS before mux
```

#### 5.3.8 B ahead (negative offset) — content starts later on A

```bash
# A delayed relative to B (A has 5s silence, then chirp)
ffmpeg -y -i MASTER.wav -af "adelay=5000|5000" a_delayed.wav
ffmpeg -y -i MASTER.wav -c copy b.wav
# Expected domain offset ≈ −5s
```

#### 5.3.9 Two-clip inconsistent tail

```bash
# Same first 120s; A full chirp, B chirp only for first 90s then silence
ffmpeg -y -i MASTER.wav -t 90 -c copy b_trunc.wav
# Pad B to total length with silence for end-window mismatch
```

#### 5.3.10 HE-AAC (optional, `he-aac` feature)

```bash
# Reuse existing media_reader attempt order:
ffmpeg -y -i MASTER.wav -c:a libfdk_aac -profile:a aac_he -b:a 64k a_he.mp4
# fallback: -c:a aac -profile:a aac_he -b:a 64k
```

#### 5.3.11 Negative control: tone vs chirp

```bash
ffmpeg -y -f lavfi -i "sine=frequency=440:duration=60" tone.wav
# pair chirp_a.wav (from Rust) with tone.wav
```

### 5.4 `scripts/generate_corpus.ps1` structure

```powershell
# Pseudo-code

param(
  [string]$OutDir = "tests/corpus",
  [int]$SampleRate = 44100,
  [int]$TotalSecs = 120
)

function Require-Ffmpeg { ... }
function Write-MasterChirp { ... }   # call cargo run --bin chirp-gen OR ffmpeg lavfi fallback
function Invoke-Case { param($CaseId, $ScriptBlock) ... }

Require-Ffmpeg
New-Item -ItemType Directory -Force -Path $OutDir/wav, $OutDir/mp3, $OutDir/mp4

Write-MasterChirp "$OutDir/_build/master.wav"

# For each committed case in manifest (Tier B only):
Invoke-Case "wav_leader_3s" {
  ffmpeg ... # §5.3.1, copy to wav/leader_3s_a.wav, wav/leader_3s_b.wav
}
Invoke-Case "mp3_leader_3s" { ... }
# ...

Write-Host "Done. Verify with: cargo test corpus_"
```

---

## 6. Integration test harness

### 6.1 Test entry point

```rust
// Pseudo-code: corpus_integration in align_videos.rs or corpus_fixtures.rs tests

#[test]
fn corpus_committed_cases() {
    let manifest = load_manifest();
    for case in manifest.cases.iter().filter(|c| c.tier == Committed && !c.ignore) {
        run_corpus_case(case, &manifest.defaults);
    }
}

#[test]
#[cfg(feature = "ffmpeg-tests")]
fn corpus_generated_cases() {
    if !ffmpeg_available() { return; }  // or eprintln + skip
    // mp3, mp4, mkv, dual-track, ...
}

#[test]
#[ignore = "long smoke; set CLIP_SYNC_CORPUS"]
fn corpus_external_cases() { ... }

fn run_corpus_case(case: &CorpusCase, defaults: &CorpusDefaults) {
    let (path_a, path_b) = resolve_pair(case);
    let config = build_config(case, defaults);

    let response = AlignVideos::new(...).execute(AlignVideosRequest {
        video_a: path_a,
        video_b: path_b,
        config,
    });

    match case.expect_exit_code {
        0 => {
            let response = response.expect(&format!("case {} should succeed", case.id));
            assert_corpus_expectations(case, defaults, &response.result);
        }
        code => assert!(response.is_err(), "case {}", case.id),
    }
}

fn assert_corpus_expectations(case: &CorpusCase, defaults: &CorpusDefaults, result: &AlignmentResult) {
    if let Some(expect) = case.expect_aligned {
        assert_eq!(result.start_aligned, expect, "case {}", case.id);
    }
    if let Some(offset) = case.expected_offset_secs {
        let rec = result.recommended_offset_secs.expect("recommended offset");
        assert!((rec - offset).abs() <= defaults.tolerance_secs, "case {}", case.id);
    }
    if case.expect_recommended == Some(false) {
        assert_eq!(result.recommended_offset_secs, None);
    }
    // optional: confidence, offsets_consistent, track index (once exposed)
}
```

### 6.2 Cargo features

```toml
# Cargo.toml (additions)
[features]
corpus-ffmpeg = ["ffmpeg-tests"]   # alias / document
```

- Default `cargo test`: Tier B committed only (no ffmpeg).
- `cargo test --features ffmpeg-tests`: Tier A generated cases too.
- `cargo test -- --ignored`: Tier C long / slow.

---

## 7. Implementation phases

### Phase 0 — Docs & scaffolding (½ day)

- [x] Create `docs/corpus-matrix.md` (§2 table + tolerance policy).
- [x] Create `tests/corpus/README.md` (size budget, regenerate instructions).
- [x] Add `tests/corpus/manifest.toml` with 3 committed WAV cases.
- [x] Link this plan from `BACKLOG.md` under “Real-world file validation”.

### Phase 1 — Tier B committed WAV (1 day)

- [x] Generate `wav/baseline_0s_*.wav`, `wav/leader_3s_*.wav`, chirp/tone via script or Rust.
- [x] Implement `load_manifest()` + `run_corpus_case()` for committed tier only.
- [x] Wire `corpus_committed_cases` test; green in CI without ffmpeg.

### Phase 2 — Rust + ffmpeg generators (1–2 days)

- [x] Add `corpus_fixtures.rs` with `generate_case_pair` (WAV + ffmpeg encode).
- [x] Port ffmpeg helpers from `media_reader.rs` tests into shared module (dedupe).
- [x] Add manifest rows: `mp3_leader_3s`, `mp4_aac_leader_3s`, `mkv_flac_leader_3s`, `wav_b_ahead_5s`, `mp3_no_duration_tag`.
- [x] `corpus_generated_cases` test (skips ffmpeg cases when ffmpeg unavailable).

### Phase 3 — Matrix edge cases (1–2 days)

- [x] Dual-track MP4 + `try_all_tracks` assertion.
- [x] Negative: `no_overlap_tone_vs_chirp`.
- [x] Two-clip consistent / inconsistent cases.
- [x] `mp3_no_duration_tag` case.
- [x] Update matrix doc with actual vs expected after first run.

### Phase 4 — Scripts & regeneration (½ day)

- [x] `scripts/generate_corpus.ps1` + `.sh` implementing §5.3.
- [x] Document in `tests/corpus/README.md`.
- [x] Verify committed size < 5 MB.

### Phase 5 — Tier C & perf (optional)

- [x] `CLIP_SYNC_CORPUS` external tier + `#[ignore]` long smoke.
- [ ] Record wall time in test output for session-reuse before/after comparison.

### Phase 6 — Close out

- [x] Fix failures discovered by corpus (track in BACKLOG per issue).
- [x] Delete or archive `docs/TEMP-corpus-implementation-plan.md`.
- [x] Mark “Real-world file validation” done in `BACKLOG.md`.

---

## 8. CI strategy

| Job | Command | Cases |
|-----|---------|-------|
| PR default | `cargo test corpus_committed` | Tier B WAV (+ MP3 if small committed) |
| PR optional | `cargo test --features ffmpeg-tests corpus_generated` | Tier A |
| Nightly | `CLIP_SYNC_CORPUS=... cargo test -- --ignored` | Tier C long |

**Skip policy:** If `requires_ffmpeg` and ffmpeg missing → `eprintln!` + return (same as existing `media_reader` tests).

---

## 9. Dependencies on other backlog work

Corpus will **surface** gaps; fix separately:

| Finding | Backlog / follow-up |
|---------|---------------------|
| MP3 open fails | Duration-less files (may be partially fixed; verify with `mp3_no_duration_tag`) |
| Wrong track on dual MP4 | Enable `try_all_tracks` by default or improve `select_best_track` |
| `InsufficientAudio` aborts run | Clip-skip + continue (not yet implemented) |
| Slow multi-clip | Session reuse |
| Poor stderr on soft fail | Richer diagnostics / log file |

---

## 10. Definition of done

- [x] `docs/corpus-matrix.md` exists and lists ≥ 20 cases with expected outcomes.
- [x] ≥ 3 committed fixture pairs in `tests/corpus/` (~3.4 MB total; under 5 MB budget).
- [x] Manifest drives integration tests; CI runs committed tier without ffmpeg.
- [x] Generated tier passes locally with ffmpeg on PATH (`corpus_generated_cases`).
- [x] `scripts/generate_corpus.ps1` / `.sh` can regenerate Tier B from chirp generators.
- [x] At least one failure from the matrix was found and fixed (dual-track decoy↔decoy false match; see `BACKLOG.md`).

---

## 11. References

- Existing chirp pair: `src/application/testing/audio_fixtures.rs`
- Existing ffmpeg test helpers: `src/infrastructure/symphonia/media_reader.rs` (`#[cfg(feature = "ffmpeg-tests")]`)
- End-to-end alignment test: `execute_detects_known_offset_through_real_wav_pipeline` in `align_videos.rs`
- Architecture: `PLAN.md` § testing (`tests/fixtures/`, kept small)
