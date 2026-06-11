# clip-sync test corpus (Tier B)

Small committed audio fixtures for manifest-driven integration tests. See [docs/corpus-validation.md](../../docs/corpus-validation.md) and [docs/corpus-matrix.md](../../docs/corpus-matrix.md).

## Size budget

Keep total committed fixtures under **5 MB**. Current clips are 20–30 s mono 16-bit PCM at 11.025 kHz (**~3.4 MB** total as of 2026-06-11).

### Hold-out verification on committed tier

Committed WAVs are **30 s**; default `clip_length` is **60 s**, so `verify_offset` cannot run on Tier-B files (hold-out is skipped). **Accepted coverage:** generated manifest cases `verify_offset_pass` (120 s WAV) and `mkv_tail_decodable_extent_gap` (ffmpeg). A committed ~75 s verify pair would add **~3.2 MB** and exceed the 5 MB cap without removing or extending an existing pair — not added unless the budget is raised.

## Regenerate WAV fixtures

From the repo root (requires Rust toolchain):

```powershell
cargo test -p clip-sync regenerate_committed_wav_fixtures -- --ignored --nocapture
```

Or:

```powershell
.\scripts\generate_corpus.ps1
```

This overwrites `tests/corpus/wav/*.wav` from the synthetic chirp generators in `crates/clip-sync/src/application/testing/`.

## Run corpus tests

```powershell
cargo test -p clip-sync corpus_
```

- **Committed** (`corpus_committed_cases`): always runs; uses `tests/corpus/wav/`.
- **Generated** (`corpus_generated_cases`): builds chirp pairs at test time; MP3/MP4/MKV/dual-track cases require **ffmpeg** on PATH (skipped when missing). Pure WAV generated cases run without ffmpeg.
- **External** (`corpus_external_cases`): `#[ignore]` long smoke; set `CLIP_SYNC_CORPUS` to a persistent directory and run with `--ignored`.

Regenerate committed WAVs:

```bash
./scripts/generate_corpus.sh
```

## Manifest

`manifest.toml` lists each case id, media paths, and expected alignment outcomes. Tests in `corpus_fixtures.rs` load the manifest and run `AlignVideos` against each committed pair.

## Licensing

All fixtures are synthetically generated (chirp / tone). No third-party audio.
