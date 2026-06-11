#!/usr/bin/env bash
# Container-specific seek and extent tests (requires ffmpeg on PATH).
# Run from repo root: ./scripts/test-container-seek.sh
set -euo pipefail
cd "$(dirname "$0")/.."
cargo test -p clip-sync --features ffmpeg-tests \
  backward_seek track_decodable_extent extract_after_track_decodable_extent
