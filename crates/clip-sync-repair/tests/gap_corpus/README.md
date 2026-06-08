# Gap Detection Corpus

Known-answer tests for the `ScanGaps` silence scanner, kept separate from the
alignment corpus in `tests/corpus/`.

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
cargo test -p clip-sync-repair gap_corpus_committed

# Committed + generated
cargo test -p clip-sync-repair gap_corpus -- --ignored

# External (real MKV files)
$env:CLIP_SYNC_GAP_CORPUS = "F:\Video"
cargo test -p clip-sync-repair gap_corpus_external -- --ignored
```

## Regenerating committed WAV fixtures

```powershell
cargo test -p clip-sync-repair gap_corpus_regenerate -- --ignored --nocapture
```

The fixtures are synthetic chirp WAVs (11 025 Hz, mono, i16) with specific
sample ranges zeroed to create known silence regions. They are small enough
(≈ 2 MB total) to commit directly.

## Adding a case

1. For **committed**: add a row to `manifest.toml` referencing an existing or new
   file in `wav/`, then add a generator call in `write_committed_wav_fixtures()`.
2. For **generated**: add a `[[case]]` with `tier = "generated"` and a `generator`
   name. The `zeroed_chirp` generator accepts `gap_segments`; `quiet_chirp` accepts
   `amplitude_fraction`.
3. For **external**: record the ground truth with ffmpeg:
   ```
   ffmpeg -i FILE.mkv -af silencedetect=noise=-60dB:d=1 -f null -
   ```
   Then add a `[[case]]` with `tier = "external"` and populate `expected_gaps`.

## Size budget

Committed WAVs: < 5 MB total (currently ≈ 2 MB).
