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

**PR / integration smokes** (no corpus fetch):

```powershell
cargo test -p clip-sync-repair --test integration_floor_oracle_smoke
```

**Validation tier** (real codec + corpus; requires `validation-tests` feature):

```powershell
.\scripts\test-tier.ps1 -Tier validation -Package clip-sync-repair

# Or explicit binaries:
cargo test -p clip-sync-repair --features validation-tests --test validate_floor_oracle -- --nocapture
```

**Requires:** ffmpeg on `PATH` and corpus sources (`.\scripts\fetch_corpus_sources.ps1`). Tests **fail** if either is missing — they do not soft-skip on the validation tier.

Key validation tests in `tests/validate_floor_oracle.rs`:

- `source_gap_oracle_floor_csv` — calibration matrix CSV
- `floor_oracle_residual_gate_real_codec` — RG gate on real codec
- `floor_oracle_veto_rescue_real_broadband_codec` — veto/rescue safety

## Tiers

- **same_master** — A has an injected silence gap; B is the full master (second encode).
  `nominal == truth` at the gap anchor. Expect `informative` floor at the true fill.
- **two_mic** — A from `source_id`, B from `donor_source_id`. Expect floor **not** informative.

Post-encode validation checks A/B duration match and that the decoded A gap interior is quiet
(codec edges excluded).
