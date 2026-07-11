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
- `--fingerprint-diagnostics` — also write the **Tier-3 X-set** (`seam_probe`, `wide_envelope`,
  `b_levels`, diagnostic `lag`). Off by default (decision/repair fields only); slower. Needed for the
  analyzer's seam-probe reports.

To decide *which* gaps are worth characterizing, use the **normal repair run's gap table** — it lists
every gap's authoritative patch/skip + reason. A summary tier (cheap, no gate detail) still exists in
the API (`characterize_gaps`) but the bin path always characterizes its selected gaps at full detail,
because only the full tier carries the A-vs-B verdict (lag / `failure_stage`) needed to build a fixture.

**Repair takes priority.** This is a repair tool first, so a real repair wins over the diagnostic:
if `--mux` is set, fingerprinting is **skipped** (with a warning if `--gap-fingerprints` was also
passed). `--gap-fingerprints` therefore runs on a scan-only / `--wav` run. *(Note: `--gap-fingerprints`
with `--wav` currently runs both and decodes A/B twice — fine, but not free.)*

## Performance

The dump characterizes **every selected gap at full detail** — it runs the **full per-bracket oracle**
(`oracle_score_fit_candidate` over all feasible brackets, `gap_fingerprint.rs`), with **no routing or
short-circuit** like the production patch path. That per-bracket structure+seam search over the
`--fill-border-search-secs` haystack is the dominant cost.

Measured on a real 5.1 dump (licensed-pair, HE-AAC, 2026-07-11): **per-bracket oracle ≈ 82 % of wall-clock**
(decode ≈ 12 %), **~8.4 s per bracket** score, 11–22 brackets per short skip gap. Cost scales with the
**bracket count**, not gap duration (a 228 s gap with 0 feasible brackets is ~free).

Why it resisted a cheap speedup: on the licensed corpus, the expensive gaps are **timing-offset skips**
(same audio shifted ~150–200 ms, lag-corr ≥ 0.98) that score every bracket only to fail the lag-0
`waveform_floor` seam. A correlation-based pre-filter can't reject them (correlation is *high*), and the
gate seam sits at a structure-search-chosen placement a fixed probe can't predict — so the fingerprint
perf work (`TEMP-pipeline-perf-redesign-plan.md` §2.5.4 sub-step **8g.5**) was **deferred** after two
approaches were refuted by measurement. Full analysis + cost hierarchy: that plan's §1.3 + 8g.5 row.

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
| `lag` | full, B present | **diagnostic** per pre/post anchor lag fingerprint at the best-energy bracket / structure throat (see below) |
| `baseline_lag` | full, B present | **decision** per-shoulder lag fingerprint registered at **`b_mapped`** (see *Registration & dual-fit*) |
| `splice` | full, B present | first-class registration step derived from `baseline_lag` mono: `step_ms`, per-side `peak_r`/`peak_z`, `edge_pinned` |
| `donor_interior` | full, B present | B occupancy over the **aligned** bridge span (`b_mapped_start+L_pre … b_mapped_end+L_post`): `rms_db`, `silence_fraction`, `longest_silence_ms`, `continuous` |
| `donor_interior_nominal` | full, B present | B occupancy over the **nominal** geometry span (no lag adjustment) — registration-independent; the D11 program-quiet signal |
| `splice_dualfit` | full, B present | dual-fit viability: seams scored at per-shoulder placement + `gate_pass` / `trim_frames` / validators (see below) |
| `wide_envelope` | full, B present | 100 ms-bin RMS-envelope lag peak at `b_mapped` — cross-scale confirmer of `baseline_lag` |
| `seam_probe` | full, B present | **diagnostic** encoding-robust seam metrics (R2/R4/spectrum/env/recovered); not gated on |
| `residual` | full, B present | least-squares same-source cancellation (dB) vs noise floor at the decision seam |
| `b_levels` | full, B present | symmetric B-side `LevelProfile` (validation instrument for the program-quiet hypothesis) |
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

## Registration & dual-fit measurements

The `lag` field above sits at the **diagnostic** placement (best-energy bracket / structure throat) and
can wander on quiet gaps. The **decision** registration lives in `baseline_lag`, and the dual-fit repair
predicate is built from it. **Read each field at the placement it defines — never compare across
placements.**

### `baseline_lag` — decision registration at `b_mapped`

Each shoulder is swept **mono** over ±600 ms centered on the geometry **`b_mapped`** nominal, and the post
shoulder is registered **sequentially** (its search is centered on `S + D_A + round(L_pre)`, not the naive
`S + D_A`) so `L_pre` doesn't stack into the post lag. Per shoulder, `summarize_lag_curve` records:

- `peak_r` / `frac_lag_ms` — best correlation and its (fractional) lag.
- **`peak_z`** — the peak's z-score over the whole lag curve. The periodicity-robust uniqueness metric
  (the primary addressability gate; unique ≈ ≥12 at the 1 s window). Deflates on periodic/leveled content
  where a single-rival `prominence` would not.
- `prominence` / `second_peak_r` / `top2_spacing_ms` — single-rival margin and the spacing to it (a
  recurring spacing *is* the content's period). `prominence` is a low-floor tiebreaker, not primary.
- **`edge_pinned`** — the integer peak sits within ~2 ms of the searched boundary (read from the curve
  extremes, so high-side masking counts). The optimum may lie **beyond** ±600 ms, so `frac_lag_ms` /
  `peak_r` are a window-clipped lower bound → `step_ms` is unreliable. Widen the sweep to clear it.

### `splice` — the registration step

Derived from `baseline_lag` mono: `step_ms = post_frac_lag − pre_frac_lag` (the length discontinuity the
repair reconciles), plus per-side `peak_r`/`peak_z` and a combined **`edge_pinned`** (true if *either*
shoulder was search-exhausted ⇒ `step_ms` is GIGO). A nonzero step is the normal signature of **both**
patched and skipped gaps; what makes a gap skip is bracket-search exhaustion, not the step.

### `donor_interior` / `donor_interior_nominal` — is there anything to fill?

B occupancy over the span it would fill. `donor_interior` uses the **aligned** bridge (shoulders at their
own lags); `donor_interior_nominal` uses the **nominal** program-time span with **no lag adjustment**, so it
is registration-independent — the read used to classify a gap as **program-quiet** (B silent at the same
program time ⇒ nothing to fill, not a dropout; ledger D11). Both carry `rms_db`, `silence_fraction`,
`longest_silence_ms`, and `continuous` (no internal sub-floor run longer than 150 ms ⇒ B bridges the hole).

### `splice_dualfit` — would a length-reconciled fill pass the gate?

The offline dual-fit simulation computed on the scan's own decode. Each shoulder is placed at its own
`baseline_lag`; the pre/post seams are scored at lag 0 against the **unchanged** gate thresholds:

- `pre_seam_r` / `post_seam_r` and **`gate_pass`** — do both clear `min_fill_correlation` and
  `fill_absolute_floor`?
- `gap_frames` / `bridge_frames` / `trim_frames` (`bridge − gap`, = the step in frames; the interior
  trim/pad amount).
- **`post_seam_global_r`** — the post seam scored at the *pre* offset (step forced to 0). If it also passes,
  a single constant shift suffices and the step is a registration artifact; if only the own-lag post passes,
  the step is real.
- `pre_seam_prom` / `post_seam_prom` — per-seam placement-peak prominence (±30 ms); low ⇒ periodic/alias, so
  a PASS is not a trustworthy registration.

### Diagnostic-only fields

`wide_envelope` (100 ms-bin envelope lag peak at `b_mapped`, a cross-scale confirmer of `baseline_lag`),
`seam_probe` (encoding-robust R2/R4/spectrum/env/recovered metrics, retained from the archived cross-codec
plan), and `b_levels` (symmetric B `LevelProfile`) are **not gated on** — they explain decisions and validate
hypotheses. See the analyzer's `legend_text()` for the authoritative placement/window of every field.

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
