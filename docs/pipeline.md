# Repair pipeline

The `clip-sync-repair` execution pipeline, phase by phase, with the reference doc for each. This is the **map**; the linked docs are the territory.

`clip-sync-repair` takes two recordings of the same event — **A** (has silent gaps) and **B** (the donor/reference) — and produces A with its gaps filled from B. It runs as a linear pipeline:

```text
1. Align  →  2. Scan gaps  →  3. Fill plan  →  4. Per-gap patch  →  5. Write / mux
```

| # | Phase | What happens | Reference | Key code |
|---|-------|--------------|-----------|----------|
| **1** | **Align** | Fingerprint A and B, match, produce offset + confidence + start/end clip anchors; optional high-rate refinement; silence cross-check | [alignment.md](alignment.md) | `clip-sync` aligner |
| **2** | **Scan gaps** | Detect silent runs in A; check B has energy at the aligned position → `fillable` / `unfillable` | [gap-scan.md](gap-scan.md) | `application/scan_gaps.rs` |
| **3** | **Fill plan** | Per-gap B offset map (`fill_offset_mode`); classify each gap | [gap-fill-modes.md](gap-fill-modes.md), README § Gap patching pipeline | `application/patch_audio.rs` |
| **4** | **Per-gap patch** | Structure match on B → seam scoring → optional A-boundary grid → splice | [gap-repair-guide.md](gap-repair-guide.md), [gap-fill-modes.md](gap-fill-modes.md), [seam-scoring.md](seam-scoring.md) | `application/patch_region.rs`, `domain/gap_fill_fit.rs`, `gap_structure.rs`, `gap_energy.rs` |
| **5** | **Write / mux** | Write patched WAV / mux into A's container via ffmpeg | README § Write output, [cli-output.md](cli-output.md) § Timeline warnings | `application/repair_videos.rs`, `infrastructure/ffmpeg_mux.rs` |

For the architectural view (crates, hexagonal layers, ports), see [PLAN.md](../PLAN.md).

---

## 1. Align

Fingerprints (chromaprint) of clip windows from each file are matched to find the time **offset** that aligns A and B, with a **confidence** and **start/end clip anchors**. Optional native-rate **high-rate FFT refinement** sharpens the offset; a **silence-based cross-check** validates it independently. The phase also handles **offset drift** (start vs end offset differ), **query-reference** mode (a short A against a long B), and **periodic ambiguity** (repetitive content). Output is an `AlignmentResult` (offset, clips, overlap window) — it can also be supplied externally instead of computed.

- **Reference:** [alignment.md](alignment.md) — modes, clip windows, query-reference, refinement, drift, ambiguity.
- **Config:** `[alignment]` — `clip_length`, `num_clips`, `--refine-offset-high-rate`, `--query-reference`, `--symmetric-align`.

## 2. Scan gaps

A is decoded and scanned for **silent runs** ≥ `min_gap_ms` (default 1000) where the level stays below the silence floor (`absolute_silence_rms`, default 33 + `silence_peak_fraction`, default 0.01), measured in `scan_block_ms` blocks (250) with `silence_hold_ms` (500) bridging brief dips. For each detected gap, the aligned position on B is checked for energy (`scan_both`): a gap B can fill is **`fillable`**; one where B is also silent (or out of B's coverage) is **`unfillable`**. Output is a `GapReport` with each gap classified and timestamped on the decoded-sample clock.

- **Reference:** [gap-scan.md](gap-scan.md) — detection, B mapping, bidirectional cross-check, output.
- **Config:** `min_gap_ms`, `absolute_silence_rms`, `silence_peak_fraction`, `scan_block_ms`, `silence_hold_ms`, `scan_both`, `gap_offset_tolerance_secs`.
- **Code:** `application/scan_gaps.rs`. Scan-corpus validation: [corpus-validation.md](corpus-validation.md), [`tests/gap_corpus/README.md`](../crates/clip-sync-repair/tests/gap_corpus/README.md).

## 3. Fill plan

For each fillable gap, the **B offset map** translates A's gap time to a nominal location on B, governed by `fill_offset_mode`:

- **`recommended`** (default) — every gap uses the alignment's `recommended_offset_secs`.
- **`interpolated`** — linearly interpolates between the start- and end-clip offsets by the gap's position on A (use on drift-heavy pairs).
- **`anchored_retry`** — two-pass: pass 1 patches with the clip offset, pass 2 retries seam failures using offset anchors from confident pass-1 successes.

Gaps are also tagged `plan_kind` (`fillable` / `unfillable` / `not_planned`). See [gap-fill-modes.md](gap-fill-modes.md) § Patch anchors and README § Gap patching pipeline (1).

- **Config:** `fill_offset_mode`, `fill_anchor_*`, `limit_fill_to_mapped_region`.
- **Code:** `application/patch_audio.rs`.

## 4. Per-gap patch

For each fillable gap (`application/patch_region.rs`):

1. **Refine A gap edges** — tighten the reported gap boundaries against the actual silence.
2. **Slice B haystack** — extract B around the nominal map: `gap_signature_context_secs` context + `fill_border_search_secs` slide radius + `fill_align_margin_secs` + length slack.
3. **Structure match** — build the gap signature (`bool` / `energy` / `auto`; [gap-repair-guide.md](gap-repair-guide.md) § Layer 4) and search the haystack for the matching dropout. In `fit` mode this is a unified structure + waveform search; it locates *where* on B to splice.
4. **Seam scoring & gates** — build A border templates, select signal channels, compute peak-normalized Pearson `pre`/`post`, and apply the structure gate + waveform tier (High / Marginal / skip). Full mechanics: [seam-scoring.md](seam-scoring.md).
5. **A-boundary extension grid** *(optional)* — when `--full` (or gate retries) is on, jointly search the A gap boundaries for a better seam.
6. **Splice + crossfade + normalize** — write B's fill into A's gap with a crossfade and level match.

- **References:** [gap-fill-modes.md](gap-fill-modes.md) (`fit` vs `gate`, flags, performance), [gap-repair-guide.md](gap-repair-guide.md) (reading/steering, tiers, seam shapes, profiles), [seam-scoring.md](seam-scoring.md) (seam mechanics).
- **Code:** `application/patch_region.rs`, `domain/gap_fill_fit.rs`, `domain/gap_structure.rs`, `domain/gap_energy.rs`.

## 5. Write / mux

In write mode the patched A audio is emitted:

- **`--wav`** — lossless 16-bit PCM (no re-encode).
- **`--mux`** — copies A's video and re-encodes the patched audio track into A's container via ffmpeg. Bitrate defaults to `mux_audio_bitrate = "match_min"` (the lower of A and B's measured rates, so output isn't upsampled). A **mux preflight** checks duration and PTS-vs-sample-clock skew before writing.

Report-only mode (default `dry_run = true`) writes nothing.

- **Config:** `dry_run`, output paths, `audio_codec`, `mux_audio_bitrate`, `normalize_fill`, `crossfade_ms`.
- **References:** README § Write output, [cli-output.md](cli-output.md) § Timeline / duration warnings, [json-output.md](json-output.md).
- **Code:** `application/repair_videos.rs`, `application/mux_bitrate.rs`, `infrastructure/ffmpeg_mux.rs`.

---

## Two lenses, don't confuse them

- **This document** = the **execution** pipeline (what the tool *does*, in order).
- **[gap-repair-guide.md](gap-repair-guide.md) Layers 1–5** = the **operator decision** lens (how to *read and steer* a run): plan-time gap types (P0–P7), tiers & seam shapes (W1–W6), signature mode, repair profiles. These describe how to interpret phase 4's output, not separate pipeline stages.
- **[PLAN.md](../PLAN.md)** = the **architecture** (crates, hexagonal domain/application/infrastructure layers, ports/adapters).

## Related reading

- [gap-repair-guide.md](gap-repair-guide.md) — reading and steering a repair run
- [gap-fill-modes.md](gap-fill-modes.md) — `fit` vs `gate`, flag interactions, performance
- [seam-scoring.md](seam-scoring.md) — how `pre`/`post` seams are identified and scored
- [cli-output.md](cli-output.md) / [json-output.md](json-output.md) — report layout
- [corpus-validation.md](corpus-validation.md) — test corpus and acceptance
- [PLAN.md](../PLAN.md) — architecture and application sketch
