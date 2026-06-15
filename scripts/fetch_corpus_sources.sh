#!/usr/bin/env bash
# Download optional third-party corpus sources into tests/corpus/_sources/.
set -euo pipefail
cd "$(dirname "$0")/.."

sources_toml="tests/corpus/sources.toml"
sources_dir="tests/corpus/_sources"
mkdir -p "$sources_dir"

python3 - <<'PY'
import hashlib
import pathlib
import re
import sys
import urllib.request

root = pathlib.Path("tests/corpus")
text = (root / "sources.toml").read_text(encoding="utf-8")
entries = []
current = {}
for line in text.splitlines():
    line = line.strip()
    if line == "[[source]]":
        if current:
            entries.append(current)
        current = {}
        continue
    match = re.match(r'^(\w+)\s*=\s*"(.*)"\s*$', line)
    if match:
        current[match.group(1)] = match.group(2)
if current:
    entries.append(current)

failed = False
sources_dir = root / "_sources"
for source in entries:
    dest = sources_dir / source["filename"]
    expected = source["sha256"].lower()
    if dest.is_file():
        digest = hashlib.sha256(dest.read_bytes()).hexdigest()
        if digest == expected:
            print(f"OK (cached): {source['id']}")
            continue
        print(f"Re-downloading {source['id']}: hash mismatch")
    else:
        print(f"Downloading {source['id']} ...")
    urllib.request.urlretrieve(source["url"], dest)
    digest = hashlib.sha256(dest.read_bytes()).hexdigest()
    if digest != expected:
        print(f"SHA-256 mismatch for {source['id']}: got {digest}, expected {expected}", file=sys.stderr)
        failed = True
    else:
        print(f"Verified: {source['id']} ({source['filename']})")

if failed:
    sys.exit(1)
print("Done. Run: cargo test -p clip-sync corpus_source_cases -- --ignored")
PY
