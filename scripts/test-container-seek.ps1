# Container-specific seek, extent, and extract-window regression tests (requires ffmpeg on PATH).
# Run from repo root: .\scripts\test-container-seek.ps1
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
Push-Location (Split-Path $PSScriptRoot -Parent)
try {
    cargo test -p clip-sync --features ffmpeg-tests `
        backward_seek track_decodable_extent extract_after_track_decodable_extent `
        extract_window_regression unequal_mkv_aac mkv_aac_anchored
} finally {
    Pop-Location
}
