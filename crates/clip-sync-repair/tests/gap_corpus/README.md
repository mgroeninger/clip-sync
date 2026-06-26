# Gap Detection Corpus

Known-answer tests for the `ScanGaps` silence scanner, kept separate from the
alignment corpus in `tests/corpus/`.

Silence is detected on **native multichannel PCM** (all channels must be quiet
per block — ffmpeg `silencedetect` default, `mono=0`).

**Patch seam behavior** (`fill_mode` fit vs gate, fill offset, structure trust, gap extension,
one-strong-seam fallback) is covered by `tests/patch_audio_integration.rs` (integration
tests default to `fill_mode = gate` where gate shortcuts are under test) and unit tests in
`gap_seam_extend.rs` / `patch_region.rs` / `gap_fill_fit.rs` — not this corpus.

## Tiers

- **Committed** (`gap_corpus_committed`): always runs in CI; uses pre-generated WAV
  files committed in `tests/gap_corpus/wav/`.
- **Generated** (`gap_corpus_generated`): builds WAV fixtures at test time using
  pure-Rust generators (no ffmpeg needed). Run with `--ignored`.
- **External** (`gap_corpus_external`): real media files supplied by the user via
  `CLIP_SYNC_GAP_CORPUS`. Ground truth comes from running ffmpeg's `silencedetect`
  filter manually and recording the output in `manifest.toml`.

## Running

```powershell
# Committed only (fast, always green in CI)
cargo test -p clip-sync-repair --test integration_gap_corpus gap_corpus_committed

# Patch timing on committed gap fixtures (scan + fit patch; 5s border, no extension grid)
cargo test -p clip-sync-repair --test integration_gap_corpus gap_corpus_patch_timing_committed -- --ignored --nocapture

# Production-default fit patch on corpus (slow; run before release)
cargo test -p clip-sync-repair --test integration_gap_corpus gap_corpus_patch_timing_production -- --ignored --nocapture

# Committed + generated
cargo test -p clip-sync-repair --test integration_gap_corpus gap_corpus -- --ignored

# External (real MKV files)
$env:CLIP_SYNC_GAP_CORPUS = "F:\Video"
cargo test -p clip-sync-repair --test integration_gap_corpus gap_corpus_external -- --ignored
```

## Regenerating committed WAV fixtures

```powershell
cargo test -p clip-sync-repair --test integration_gap_corpus gap_corpus_regenerate -- --ignored --nocapture
```

The fixtures are synthetic chirp WAVs (11 025 Hz, i16) with specific sample ranges
zeroed to create known silence regions. Mono and stereo committed files are small
enough (≈ 4 MB total) to commit directly.

## Adding a case

1. For **committed**: add a row to `manifest.toml` referencing an existing or new
   file in `wav/`, then add a generator call in `write_committed_wav_fixtures()`.
2. For **generated**: add a `[[case]]` with `tier = "generated"` and a `generator`
   name:
   - `zeroed_chirp` — full chirp; optional `channels` (default 1); `gap_segments`
     zero all channels in each segment
   - `quiet_chirp` — low-amplitude chirp; optional `channels`
   - `asymmetric_channels` — chirp on `hot_channel` (default 0), others silent;
     requires `channels >= 2`
   - `partial_channel_gap` — chirp on all channels, then zero `gap_segments` on
     `gap_channel` (default 1) only; requires `channels >= 2`
3. For **external**: record the ground truth with ffmpeg:
   ```
   ffmpeg -i FILE.mkv -af silencedetect=noise=-60dB:d=1 -f null -
   ```
   Then add a `[[case]]` with `tier = "external"` and populate `expected_gaps`.

## Multichannel cases (committed)

| Case ID | Layout | Expected |
|---------|--------|----------|
| `stereo_mid_gap_2s` | Both channels silent in gap | 1 gap |
| `stereo_hot_left_no_gap` | Chirp L, silent R | No gaps |
| `stereo_partial_gap_no_detect` | Chirp L; R zeroed in gap only | No gaps |

## Size budget

Committed WAVs: < 5 MB total (currently ≈ 4 MB with stereo fixtures).
