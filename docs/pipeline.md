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
| **4** | **Per-gap patch** | Unified fit (or gate) placement → seam tiers → optional anchor seam / boundary grid → residual gate → splice | [gap-repair-guide.md](gap-repair-guide.md), [gap-fill-modes.md](gap-fill-modes.md), [seam-scoring.md](seam-scoring.md) | `application/patch_region.rs`, `domain/gap_fill_fit.rs`, `gap_structure.rs`, `gap_energy.rs`, `domain/gap_anchor_seam.rs` |
| **5** | **Write / mux** | Write patched WAV / mux into A's container via ffmpeg | README § Write output, [cli-output.md](cli-output.md) § Timeline warnings | `application/repair_videos.rs`, `infrastructure/ffmpeg_mux.rs` |

Phases **1–2** always run. Phases **3–5** run only in **write mode** (see [Orchestration](#orchestration-scan-only-vs-write-mode) below).

For the architectural view (crates, hexagonal layers, ports), see [PLAN.md](../PLAN.md).

---

## Orchestration: scan-only vs write mode

The five phases above are the **logical** execution order. The **binary** wires them in two steps (`application/run_repair.rs`):

```text
ScanGaps (align + scan)  →  GapReport
       │
       └── when --wav / --mux (dry_run = false):
             RepairVideos → PatchAudio (fill plan + per-gap patch + splice) → write WAV / mux
```

| Mode | Trigger | What runs | Output |
|------|---------|-----------|--------|
| **Scan-only** (default) | `dry_run = true`, no `--wav` / `--mux` | Phases **1–2** only | `GapReport` on stdout (alignment + gaps + fillability signal) |
| **Write mode** | `--wav` and/or `--mux` | Phases **1–5** | Patched file(s) + full patch summary in the report |

In scan-only mode, phases **3–5 never execute** — there is no `build_gap_fill_plan`, no seam scoring, no splice. Plan-time labels (`fillable`, `unfillable`, `not planned: …`) are still readable from the scan report via `Gap::is_fillable`, `GapReport::repairable_count`, and `track_compatibility` (same rules as the fill plan, but skip reasons like `track_layout_mismatch` only appear in the patch section after a write run).

Phases **3–4** are nested inside `PatchAudio::execute`, not separate use cases. **`anchored_retry` pass 2** is documented under the fill plan (offset map) but runs at the end of phase 4 after pass 1 completes.

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

**Write mode only.** `build_gap_fill_plan` runs as the first step of `PatchAudio::execute`. Scan-only runs skip this phase; use `GapReport::repairable_count` and per-gap `is_fillable` for the same fillability signal.

For each fillable gap, the **B offset map** translates A's gap time to a nominal location on B, governed by `fill_offset_mode`:

- **`recommended`** (default) — every gap uses the alignment's `recommended_offset_secs`.
- **`interpolated`** — linearly interpolates between the start- and end-clip offsets by the gap's position on A (use on drift-heavy pairs).
- **`anchored_retry`** — pass 1 uses the clip offset; **pass 2** (phase 4, after all gaps in pass 1) retries seam failures using offset anchors from confident pass-1 successes.

Gaps are also tagged `plan_kind` (`fillable` / `unfillable` / `not_planned`). See [gap-fill-modes.md](gap-fill-modes.md) § Patch anchors and README § Gap patching pipeline (1).

- **Config:** `fill_offset_mode`, `fill_anchor_*`, `limit_fill_to_mapped_region`.
- **Code:** `application/patch_audio.rs`.

## 4. Per-gap patch

**Write mode only.** `PatchAudio` decodes full A and B timelines, then for each fillable region calls `prepare_region_patch` → `evaluate_seam_gate` (`application/patch_region.rs`). In **`fit`** mode (default), steps 3–6 below are largely **one joint search** per bracket candidate, not six sequential passes; **`gate`** mode runs structure match then waveform gate, with reactive boundary retries on failure.

1. **Refine A gap edges** — tighten the reported gap boundaries against the actual silence.
2. **Slice B haystack** — extract B around the nominal map: `gap_signature_context_secs` context + `fill_border_search_secs` slide radius + `fill_align_margin_secs` + length slack.
3. **Placement search** — build the gap signature (`bool` / `energy` / `auto`; [gap-repair-guide.md](gap-repair-guide.md) § Layer 4) and locate the B fill. In `fit` mode: unified structure + waveform search over the haystack. In `gate` mode: structure match, then waveform Pearson at the structure winner.
4. **Seam scoring & tiers** — border templates, channel selection, peak-normalized Pearson `pre`/`post`, structure gate + waveform tier (High / Marginal / skip). Mechanics: [seam-scoring.md](seam-scoring.md).
5. **A-boundary extension grid** *(optional, fit)* — when `--full` / `fit_boundary_search = full_grid`, jointly search A gap start/end when baseline is not High. In `gate`, reactive extend-start/end retries after waveform failure instead.
6. **Editorial anchor seam** *(optional, fit)* — when `anchor_seam_mode` triggers, search speech peaks / bool onsets for a better seam bracket when throat-only Pearson is weak ([gap-fill-modes.md](gap-fill-modes.md) § Editorial anchor seam, [gap-repair-guide.md](gap-repair-guide.md) § W5 rescue). Orthogonal to patch anchors (`anchored_retry`).
7. **Residual / floor gate** *(fit, default on)* — after Pearson tiering, measure cancellation headroom vs a per-gap noise floor; default `residual_gate = veto` can skip high-headroom placements (anti-echo) or, with `veto_rescue`, recover Pearson dead-zone skips on same-master broadband seams. Design: [residual-gate-wiring-plan.md](residual-gate-wiring-plan.md); channel policy: [seam-scoring.md](seam-scoring.md) § Residual channel policy.
8. **Splice + crossfade + normalize** — collect winning B segments, then splice into A's timeline with crossfade and level match (`PatchAudio` splice pass).

- **References:** [gap-fill-modes.md](gap-fill-modes.md) (`fit` vs `gate`, flags, performance), [gap-repair-guide.md](gap-repair-guide.md) (reading/steering, tiers, seam shapes, profiles), [seam-scoring.md](seam-scoring.md) (seam mechanics), [residual-gate-wiring-plan.md](residual-gate-wiring-plan.md) (residual gate).
- **Config:** `fill_mode`, `anchor_seam_mode`, `residual_gate`, `residual_*`, `fit_boundary_search`, plus patch knobs in §3.
- **Code:** `application/patch_audio.rs`, `application/patch_region.rs`, `application/fit_routing.rs`, `domain/gap_fill_fit.rs`, `domain/gap_structure.rs`, `domain/gap_energy.rs`, `domain/gap_anchor_seam.rs`.

## 5. Write / mux

In write mode the patched A audio is emitted:

- **`--wav`** — lossless PCM (no re-encode). Output bit depth is **source-driven**: if A's source track reports 24-bit, 32-bit int, or 32-bit float depth, the WAV is written at **24-bit int**; otherwise 16-bit int. Lossy codecs (AAC, AC-3, MP3, Opus) carry no depth, so they output 16-bit.
- **`--mux`** — copies A's video and re-encodes the patched audio track into A's container via ffmpeg. The PCM pipe format matches the resolved output depth (`-f s16le` or `-f s24le`). Bitrate defaults to `mux_audio_bitrate = "match_min"` (the lower of A and B's measured rates, so output isn't upsampled). A **mux preflight** checks duration and PTS-vs-sample-clock skew before writing.

**Internal representation:** all patched audio is held as normalized `f32` samples in `[-1.0, 1.0]` inside `MultiChannelPcm`. Conversion to output integers happens at write time: `f32 × 32767 → i16` (16-bit path) or `f32 × 8_388_607 → i32` (24-bit path). The source bit depth (`AudioTrack.bit_depth`, populated at Symphonia probe time) is carried through `MultiChannelPcm.source_bit_depth` and resolved by `resolve_output_bit_depth()` to one of `WavBitDepth::Int16` or `WavBitDepth::Int24`. `MonoPcmClip` (used by the chromaprint fingerprinter) remains `Vec<i16>` — it is not affected by this representation.

Report-only / scan-only mode (default `dry_run = true`, no output paths) runs phases 1–2 and writes nothing to disk — see [Orchestration](#orchestration-scan-only-vs-write-mode).

- **Config:** `dry_run`, output paths, `audio_codec`, `mux_audio_bitrate`, `normalize_fill`, `crossfade_ms`.
- **References:** README § Write output, [cli-output.md](cli-output.md) § Timeline / duration warnings, [json-output.md](json-output.md).
- **Code:** `application/repair_videos.rs`, `application/mux_bitrate.rs`, `infrastructure/ffmpeg_mux.rs`, `infrastructure/wav_writer.rs`, `infrastructure/pcm.rs`.

---

## Two lenses, don't confuse them

- **This document** = the **execution** pipeline (what the tool *does*, in order).
- **[gap-repair-guide.md](gap-repair-guide.md) Layers 1–5** = the **operator decision** lens (how to *read and steer* a run): plan-time gap types (P0–P7), tiers & seam shapes (W1–W6), signature mode, repair profiles. These describe how to interpret phase 4's output, not separate pipeline stages.
- **[PLAN.md](../PLAN.md)** = the **architecture** (crates, hexagonal domain/application/infrastructure layers, ports/adapters).

## Related reading

- [gap-repair-guide.md](gap-repair-guide.md) — reading and steering a repair run
- [gap-fill-modes.md](gap-fill-modes.md) — `fit` vs `gate`, flag interactions, performance
- [seam-scoring.md](seam-scoring.md) — how `pre`/`post` seams are identified and scored
- [residual-gate-wiring-plan.md](residual-gate-wiring-plan.md) — residual/floor gate (fit mode, default `veto`)
- [cli-output.md](cli-output.md) / [json-output.md](json-output.md) — report layout
- [corpus-validation.md](corpus-validation.md) — test corpus and acceptance
- [PLAN.md](../PLAN.md) — architecture and application sketch
