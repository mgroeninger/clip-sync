# clip-sync test corpus (Tier B)

Small committed audio fixtures for manifest-driven integration tests. See [docs/dev/corpus-validation.md](../../docs/dev/corpus-validation.md) and [docs/dev/corpus-matrix.md](../../docs/dev/corpus-matrix.md).

## Size budget

Keep total committed fixtures under **5 MB**. Current clips are 20–30 s mono 16-bit PCM at 11.025 kHz (**~3.4 MB** total as of 2026-06-11).

### Hold-out verification on committed tier

Committed WAVs are **30 s**; default `clip_length` is **60 s**, so `verify_offset` cannot run on Tier-B files (hold-out is skipped). **Accepted coverage:** generated manifest cases `verify_offset_pass` (120 s WAV) and `mkv_tail_decodable_extent_gap` (ffmpeg). A committed ~75 s verify pair would add **~3.2 MB** and exceed the 5 MB cap without removing or extending an existing pair — not added unless the budget is raised.

With default **15 min** `clip_length`, `--verify-offset` can decode up to **~90 minutes** of audio per run (3 retry candidates × 2 files × `clip_length`). See [docs/dev/corpus-validation.md](../../docs/dev/corpus-validation.md) § Hold-out verification cost.

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
- **Third-party sources** (`corpus_source_cases`): `#[ignore]` optional real speech/ambient; run `scripts/fetch_corpus_sources.ps1` then `cargo test -p clip-sync corpus_source_cases -- --ignored --nocapture` to see per-case offset error in ms. High-rate precision cases: `cc_speech_mp3_high_rate_3s`, `cc_ambient_wav_high_rate_3s` (±100 ms). See [THIRD_PARTY_AUDIO.md](THIRD_PARTY_AUDIO.md).

Regenerate committed WAVs:

```bash
./scripts/generate_corpus.sh
```

## Manifest

`manifest.toml` lists each case id, media paths, and expected alignment outcomes. Tests in `corpus_fixtures.rs` load the manifest and run `AlignVideos` against each committed pair.

## Generators and oracles

| Generator | Use for |
|-----------|---------|
| `offset_chirp_pair` | Discovery / alignment offset assertions (+3 s chirp, non-periodic) |
| `looped_chirp_pair` | Hold-out verify probes only (`verify_option_a_false_pass_probe`, `probe_only` in manifest); discovery aliases mod 10 s period |
| `source_offset_pair` | Real audio from `sources.toml`; ffmpeg decode + known `adelay` offset (requires `_sources/` cache) |

See [docs/dev/corpus-validation.md](../../docs/dev/corpus-validation.md) § Option A false-pass evidence for the looped-fixture discovery (+13 s) vs verify (+13 s false-pass) behaviour.

## Licensing

Committed Tier-B fixtures are synthetically generated (chirp / tone). Optional third-party masters are listed in `sources.toml` with attribution in [THIRD_PARTY_AUDIO.md](THIRD_PARTY_AUDIO.md); files are downloaded to `_sources/` (gitignored).
