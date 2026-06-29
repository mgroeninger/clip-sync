# Gap fingerprint

A **gap fingerprint** is a licensing-safe numeric characterization of one repair gap: levels, silence
floor, energy contour, editorial anchors, structure/seam scores, and a lag fingerprint — **no audio
samples, no transcripts**. Fingerprints from real (licensed) media can be committed as a
regression/calibration corpus.

Source: `application/gap_fingerprint.rs`. Plan (archived): [TEMP-gap-fingerprint-plan.md](archive/TEMP-gap-fingerprint-plan.md).

> Status: **P1 complete** — schema + builder + oracle `failure_stage` + `--gap-fingerprints`
> bin path + per-gap corpus library, all wired. Validated on real media.

## Producing fingerprints

```
clip-sync-repair A.mkv B.m4v --gap-fingerprints gap-files/ [--fingerprint-gap 3]
```

- `--gap-fingerprints DIR` — after scan, write a corpus directory: `corpus.json` (all characterized
  gaps), one self-contained single-gap JSON **per gap** (the library), and a non-identifying
  `manifest.json`.
- `--fingerprint-gap IDX` (repeatable) — characterize **only** these gaps. Omit it to characterize
  **all** gaps. Each characterized gap gets full detail (per-bracket gate `failure_stage` + lag).

To decide *which* gaps are worth characterizing, use the **normal repair run's gap table** — it lists
every gap's authoritative patch/skip + reason. A summary tier (cheap, no gate detail) still exists in
the API (`characterize_gaps`) but the bin path always characterizes its selected gaps at full detail,
because only the full tier carries the A-vs-B verdict (lag / `failure_stage`) needed to build a fixture.

**Repair takes priority.** This is a repair tool first, so a real repair wins over the diagnostic:
if `--mux` is set, fingerprinting is **skipped** (with a warning if `--gap-fingerprints` was also
passed). `--gap-fingerprints` therefore runs on a scan-only / `--wav` run. *(Note: `--gap-fingerprints`
with `--wav` currently runs both and decodes A/B twice — fine, but not free.)*

## Shape

`GapCorpus { source: SourceMeta, gaps: [GapFingerprint] }`, serialized as JSON. Per gap:

| Field | When | Meaning |
|-------|------|---------|
| `geometry` | always | A reported/refined edges, duration; B mapped edges + fill offset (B present) |
| `levels` | always | `bin_ms`, `profile_db[]` (RMS dBFS across pre→post context), speech-peak/noise-floor/gap-floor dB |
| `silence` | always | collar RMS/peak ratio + whether it clears the **relative** silence test (border walk-off discriminator) |
| `contour` | always | `has_anchor_seam_contour`, pre/post envelope flatness |
| `anchors` | always | pre/post candidates `{ time, source, prominence, rms_db }` |
| `brackets` | full | feasible brackets `{ span, move, structure_*, seam_*, failure_stage }` |
| `structure` / `seams` | B present | baseline scores; seams carry per-channel + selected channels |
| `lag` | full, B present | per pre/post anchor lag fingerprint (see below) |
| `outcome` | B present | plan_kind, tier, seam_shape, fit_path, signature_mode, skip_reason |

## Lag fingerprint

[`lag_correlation_curve`] sweeps the seam's lag-0 Pearson ([`clip_sync::normalized_correlation`])
over integer shifts; [`summarize_lag_curve`] reports the lag-0 value, the integer peak, a
parabolic-interpolated (fractional) peak, and a verdict:

- **`timing_offset`** — `peak ≥ 0.5` and away from lag 0: a shift recovers correlation (read
  `frac_lag_ms`). Fixable by tightening alignment.
- **`decorrelated`** — `peak < 0.3`: no shift recovers correlation; sources genuinely differ.
- **`ambiguous`** — otherwise.

This is what distinguishes a sub-sample/timing offset (recoverable) from genuine A/B decorrelation
(the seam gate is right to refuse) — see [seam-scoring.md](seam-scoring.md) §3–4.

## Source identity & the corpus library

A fingerprint is an **A-vs-B** measurement and is **encoding-sensitive** (the lag/decorrelation verdict
depends on each file's exact decoded waveform — a different B encoding or a partial clip yields a
different result). So identity is **per file**, not per logical source:

- `source.a_source` / `source.b_source` each carry an opaque **`id`** = a stable strided digest of the
  **decoded** PCM (`source_id`). A remux / lossless re-container → *same* id; a different
  codec/bitrate/partial clip → *different* id. The entry's identity is the **pair** `(a_id, b_id)`.
- `source.scan_recipe` echoes the scan params (`min_gap_ms`, `silence_*`, `scan_block_ms`) so two
  entries are known-comparable.
- **No paths or titles** appear anywhere in the committed output.

**Library file names** (from `--fingerprint-corpus-dir`):
`<a8>_<b4>_t<hh-mm-ss>_g<idx>_<tier>_<verdict>.json` — opaque-id prefixed, sorted by A start time,
tagged by tier and lag/outcome verdict (e.g. `…_full_timing_offset.json`). Each file is a complete
single-gap `GapCorpus`. A `manifest.json` indexes them (ids, times, tiers, verdicts).

**Licensing guardrail:** the only place the real `id → title/path` mapping should live is a
**git-ignored** local file (e.g. `corpus/.sources.local.toml`). Keep it out of the committed corpus.
