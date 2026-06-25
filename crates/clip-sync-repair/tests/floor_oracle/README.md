# Floor oracle corpus (FLOOR_OK calibration)

Known-placement repair oracles built from optional Wikimedia masters in
`tests/corpus/sources.toml`. Separate from the alignment corpus (`tests/corpus/`)
and the gap **scanner** corpus (`tests/gap_corpus/`).

## Setup

```powershell
.\scripts\fetch_corpus_sources.ps1
```

Requires **ffmpeg** on PATH.

## Run

```powershell
cargo test -p clip-sync-repair source_gap_oracle_floor_csv -- --ignored --nocapture
cargo test -p clip-sync-repair floor_oracle_residual_gate_real_codec -- --ignored --nocapture
cargo test -p clip-sync-repair floor_oracle_veto_rescue_real_broadband_codec -- --ignored --nocapture
cargo test -p clip-sync-repair floor_oracle_manifest_loads
```

## Tiers

- **same_master** — A has an injected silence gap; B is the full master (second encode).
  `nominal == truth` at the gap anchor. Expect `informative` floor at the true fill.
- **two_mic** — A from `source_id`, B from `donor_source_id`. Expect floor **not** informative.

Post-encode validation checks A/B duration match and that the decoded A gap interior is quiet
(codec edges excluded).
