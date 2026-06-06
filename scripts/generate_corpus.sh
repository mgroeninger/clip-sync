#!/usr/bin/env bash
# Regenerate Tier-B committed WAV fixtures under tests/corpus/wav/.
set -euo pipefail
cd "$(dirname "$0")/.."

echo "Regenerating committed corpus WAV fixtures..."
cargo test regenerate_committed_wav_fixtures -- --ignored --nocapture
echo "Done. Run: cargo test corpus_"
