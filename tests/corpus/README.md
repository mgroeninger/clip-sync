# clip-sync test corpus (Tier B)

Small committed audio fixtures for manifest-driven integration tests. See [docs/corpus-matrix.md](../../docs/corpus-matrix.md) and [docs/TEMP-corpus-implementation-plan.md](../../docs/TEMP-corpus-implementation-plan.md).

## Size budget

Keep total committed fixtures under **5 MB**. Current clips are 20–30 s mono 16-bit PCM at 11.025 kHz (~4 MB total).

## Regenerate WAV fixtures

From the repo root (requires Rust toolchain):

```powershell
cargo test regenerate_committed_wav_fixtures -- --ignored --nocapture
```

Or:

```powershell
.\scripts\generate_corpus.ps1
```

This overwrites `tests/corpus/wav/*.wav` from the synthetic chirp generators in `src/application/testing/`.

## Run corpus tests

```powershell
cargo test corpus_committed
```

Tier-A generated cases (MP3/MP4 via ffmpeg) will be added in a later phase behind `--features ffmpeg-tests`.

## Manifest

`manifest.toml` lists each case id, media paths, and expected alignment outcomes. Tests in `corpus_fixtures.rs` load the manifest and run `AlignVideos` against each committed pair.

## Licensing

All fixtures are synthetically generated (chirp / tone). No third-party audio.
