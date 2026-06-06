# Regenerate Tier-B committed WAV fixtures under tests/corpus/wav/.
# Requires: Rust toolchain, cargo

$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot\..

Write-Host "Regenerating committed corpus WAV fixtures..."
cargo test regenerate_committed_wav_fixtures -- --ignored --nocapture
if ($LASTEXITCODE -ne 0) {
    throw "fixture generation failed with exit code $LASTEXITCODE"
}

Write-Host "Done. Run: cargo test corpus_"
Write-Host "Generated-tier cases (MP3/MP4/MKV) build at test time; ffmpeg on PATH required for those."
