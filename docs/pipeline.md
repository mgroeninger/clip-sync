# Repair pipeline

The `clip-sync-repair` execution pipeline, phase by phase, with the reference doc for each. This is the **map**; the linked docs are the territory. For **phase 4 routing and flags**, read together with [gap-fill-modes.md](gap-fill-modes.md).

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

Phases **1–2** always run. Phases **3–5** run in **write mode**; phases **3–4** (pass-1 characterize) also run under **`--repair-preview`**, and phases **3–4 plus the splice** run under **`--patch-only`** with phase 5 skipped (see [Orchestration](#orchestration-scan-only-vs-preview-vs-patch-only-vs-write-mode) below).

For the architectural view (crates, hexagonal layers, ports), see [PLAN.md](../PLAN.md).

---

## Orchestration: scan-only vs preview vs patch-only vs write mode

The five phases above are the **logical** execution order. The **binary** wires them in two steps (`application/run_repair.rs`):

```text
ScanGaps (align + scan)  →  GapReport
       │
       ├── when --repair-preview:
       │     PatchAudio::preview (fill plan + decode + pass-1 characterize) → patch summary, no splice/write
       │
       └── when --wav / --mux (dry_run = false) or --patch-only:
             RepairVideos → PatchAudio (fill plan + per-gap patch + splice) → write WAV / mux
                                                                              (skipped: --patch-only)
```

| Mode | Trigger | What runs | Output |
|------|---------|-----------|--------|
| **Scan-only** (default) | `dry_run = true`, no `--wav` / `--mux` / `--repair-preview` / `--patch-only` | Phases **1–2** only | `GapReport` on stdout (alignment + gaps + fillability signal) |
| **Repair preview** | `--repair-preview` | Phases **1–4** (pass-1 characterize only; no anchored retry, splice, or file write) | Gap report + would-be patch/skip summary (production gate) |
| **Patch-only** | `--patch-only` | Phases **1–4** **plus the splice**; phase 5 skipped | Full patch summary in the report, including splice-time measurements (`fill_level`). No file |
| **Write mode** | `--wav` and/or `--mux` | Phases **1–5** | Patched file(s) + full patch summary in the report |

`--repair-preview` is mutually exclusive with `--wav` / `--mux`. It is **not** `--gap-fingerprints` (that dump uses fingerprint `any_ok` semantics; preview uses the production residual-veto gate). Cost is still a full multichannel decode plus characterize — only splice/encode are skipped.

**`--patch-only` is the write arm with no sink**, not a lighter preview: it costs everything write mode costs except the encode, because the splice is the point. Use it when the run wants the *measurements* a patch produces rather than the audio — `fill_level` above all, which is taken during the splice and so cannot come from preview. It is mutually exclusive with `--wav`, `--mux`, and `--repair-preview` (a run that names a sink is not a patch-only run), and rejected alongside `--gap-listen`, which already runs the full patch itself. Its reason for existing is concrete: forcing write mode with a throwaway `--wav` fails past the 4 GiB classic WAV limit on long surround media, and the write error discards a report whose measurements were already complete.

In scan-only mode, phases **3–5 never execute** — there is no `build_gap_fill_plan`, no seam scoring, no splice. Plan-time labels (`fillable`, `unfillable`, `not planned: …`) are still readable from the scan report via `Gap::is_fillable`, `GapReport::repairable_count`, and `track_compatibility` (same rules as the fill plan, but skip reasons like `track_layout_mismatch` only appear in the patch section after a write or preview run).

Phases **3–4** are nested inside `PatchAudio::execute` / `preview`, not separate use cases. **`anchored_retry` pass 2** is documented under the fill plan (offset map) but runs at the end of phase 4 after pass 1 completes **in write mode only** (preview is pass-1 dispositions).

---

## 1. Align

Fingerprints (chromaprint) of clip windows from each file are matched to find the time **offset** that aligns A and B, with a **confidence** and **start/end clip anchors**. Optional native-rate **high-rate FFT refinement** sharpens the offset; a **silence-based cross-check** validates it independently. The phase also handles **offset drift** (start vs end offset differ), **query-reference** mode (a short A against a long B), and **periodic ambiguity** (repetitive content). Output is an `AlignmentResult` (offset, clips, overlap window) — it can also be supplied externally instead of computed.

- **Reference:** [alignment.md](alignment.md) — modes, clip windows, query-reference, refinement, drift, ambiguity.
- **Config:** `[alignment]` — `clip_length`, `num_clips`, `--refine-offset-high-rate`, `--query-reference`, `--symmetric-align`.

## 2. Scan gaps

A is decoded and scanned for **silent runs** ≥ `min_gap_ms` (default 500) where the level stays below the silence floor (`absolute_silence_rms`, default ≈ 0.001007 normalized / CLI scale 33 + `silence_peak_fraction`, default 0.01), measured in `scan_block_ms` blocks (100) with `silence_hold_ms` (500) bridging brief dips. For each detected gap, the aligned position on B is checked for energy (`scan_both`): a gap B can fill is **`fillable`**; one where B is also silent (or out of B's coverage) is **`unfillable`**. The scan also classifies each gap's **silence character** from its per-block levels (`gap_equivalence`; [gap-vocabulary.md](dev/gap-vocabulary.md) § Silence-character pre-gate) — the input to the fill-plan equivalence drop (§3). Output is a `GapReport` with each gap classified and timestamped on the decoded-sample clock.

- **Reference:** [gap-scan.md](gap-scan.md) — detection, B mapping, bidirectional cross-check, output.
- **Config:** `min_gap_ms`, `absolute_silence_rms`, `silence_peak_fraction`, `scan_block_ms`, `silence_hold_ms`, `scan_both`, `skip_equivalent_gaps`, `gap_offset_tolerance_secs`.
- **Code:** `application/scan_gaps.rs`. Scan-corpus validation: [corpus-validation.md](dev/corpus-validation.md), [`tests/gap_corpus/README.md`](../crates/clip-sync-repair/tests/gap_corpus/README.md).

## 3. Fill plan

**Write mode and `--repair-preview`.** `build_gap_fill_plan` runs as the first step of `PatchAudio::execute` / `preview`. Scan-only runs skip this phase; use `GapReport::repairable_count` and per-gap `is_fillable` for the same fillability signal.

For each fillable gap, the **B offset map** translates A's gap time to a nominal location on B, governed by `fill_offset_mode`:

- **`recommended`** (default) — every gap uses the alignment's `recommended_offset_secs`.
- **`interpolated`** — linearly interpolates between the start- and end-clip offsets by the gap's position on A (use on drift-heavy pairs).
- **`anchored_retry`** — pass 1 uses the clip offset; **pass 2** (phase 4, after all gaps in pass 1) retries seam failures using offset anchors from confident pass-1 successes.

Gaps are also tagged `plan_kind` (`fillable` / `unfillable` / `not_planned`). See [gap-fill-modes.md](gap-fill-modes.md) § Patch anchors and README § Gap patching pipeline (1).

**Gap selection (subset patching).** `--only-gaps` / `--skip-gaps` (and TOML `only_gaps` / `skip_gaps`) filter which detected gaps enter `regions` after fillability, coverage, and equivalence checks. Tokens are 1-based gap **numbers** and/or time ranges (`START-END` identity, `START..END` containment) — see [gap-repair-guide.md](gap-repair-guide.md) § Iterative subset patching. Unselected gaps stay on original A audio and report `not planned: gap not selected` (`plan_skip_reason: gap_not_selected`). Scan-only runs still **validate** tokens (bad `#` / range → exit 2) but do not filter the table.

**Equivalence drop (`skip_equivalent_gaps`, on by default).** After the fillable/coverage gates, a gap whose scan-time silence-character verdict `drops()` (mutual/ambient silence — nothing to repair) is removed from the plan as `already_matches_reference`, so it never reaches decode/patch. Lowest precedence (`not_fillable`, coverage, and track blocks win). Disable with `--no-skip-equivalent-gaps`. See [gap-vocabulary.md](dev/gap-vocabulary.md) § Silence-character pre-gate for the classification and [gap-scan.md](gap-scan.md) for how the signals are measured.

**Donor registration (`apply_donor_registration`, on by default since 2026-08-04).** The donor half of that verdict is measured on B's envelope registered against A's, not at the nominal offset map. A misregistered window reads B's content against the wrong part of A and can synthesise a dropout signature out of ordinary quiet material; registering first removes that class. Registration correlates the scanner's 100 ms dB envelopes on the **shoulders** (gap core excluded), searches ±`max_lag_blocks`, and erodes one bin at each edge for interior levels — details in [gap-scan.md](gap-scan.md) § Donor registration. The registration is always computed and emitted either way — `--no-apply-donor-registration` keeps it inert and classifies at the nominal map (the pre-2026-08-04 behaviour).

When the envelope correlation is below `min_envelope_r` (or there are too few bins to register on), registration **abstains**: the class is `not_evaluated` / `donor_registration_unreliable`, which **keeps** the gap (fail open — a patch attempt, never a hole). It does **not** fall back to measuring at the nominal map; that is the window already known to be wrong.

**Head/tail exclusion (shipped with Apply):** gaps whose A silent **core** touches the scanned A extent classify at the nominal map (Observe semantics) while still recording registration. Mid-extent gaps keep Apply. Predicate is A-span geometry (`a_span_touches_media_edge`), not gap index / `bins`. Details: [gap-scan.md](gap-scan.md) § Donor registration.

- **Config:** `fill_offset_mode`, `fill_anchor_*`, `limit_fill_to_mapped_region`, `skip_equivalent_gaps`, `apply_donor_registration`.
- **Code:** `application/scan_gaps.rs`, `domain/gap_equivalence.rs`.

## 4. Per-gap patch

**Write mode and `--repair-preview`.** Phase 4 detail — bracket routing, per-bracket measurement, and flag matrix — is in [gap-fill-modes.md](gap-fill-modes.md). This section is the **run-level** map; read both documents together for a complete picture. Preview runs the same pass-1 characterize path as write, then stops (no execute / anchored retry / splice).

### `PatchAudio` run (once per write)

```text
1. Fill plan           build_gap_fill_plan
2. Decode A + B        full timelines; B resampled to A rate if needed
3. Pass 1 (per gap)    prepare_region_patch → evaluate_seam_gate
4. Anchored retry      optional pass 2 (fill_offset_mode = anchored_retry)
5. Splice              apply RegionPatches into A PCM
6. Summarize           PatchSummary
```

**Fill-level check (`measure_fill_level`, on by default).** Between steps 4 and 5 — final patches chosen, A not yet mutated — each fill's loudest 100 ms bin is measured against the A shoulders either side of its gap and recorded on the outcome as `fill_level`. It is **report-only**: no threshold, no veto. It exists because a fill placed into quiet A at a level the surrounding program never reaches is audible as damage even when every seam score is clean, and because a veto here would trade that for unrepaired holes — so the number is collected and calibrated first. The fill measured is exactly what the splice writes (`gained_fill` — gain applied, clamped, cut to the destination), and a shoulder with less than half its width of room is declined rather than measured from a sliver. Two rows must not be read naively: `reference_at_floor` means every shoulder was itself digital silence (a neighbouring dropout), and the seam crossfade is ignored. See [json-output.md](json-output.md) § FillLevelCheck. Corpus sweep over a pair manifest: `scripts/measure-fill-level.ps1` (`--patch-only` JSON + `fill-level-rollup.csv`, floored references held out of the listen candidates). The measurement needs the splice, so `--repair-preview` cannot produce it and `--patch-only` is the cheapest run that can.

### Per gap (`prepare_region_patch`)

| Step | What |
|------|------|
| **Offset map** | `resolve_gap_offset_secs` (recommended / interpolated / anchored / anchored-retry pass 2) |
| **A edge refine** | `refine_gap_frames` |
| **B haystack** | Slice from full decoded B (context + border search + margin + slack) |
| **Seam gate** | `evaluate_seam_gate` — **fit** (default) or **gate** (legacy); see below |
| **Extract fill** | Winning B segment + optional normalize gain |
| **Queue patch** | `RegionPatch` or skip + tags (splice happens in step 5 above) |

### Fit mode: bracket routing order

`evaluate_seam_gate_fit_joint` tries **A bracket strategies** in this precedence (E1–E7 in [gap-fill-modes.md](gap-fill-modes.md) § Fit-joint routing):

```text
1. Baseline throat bracket (scan-refined edges)
2. E1 — baseline Pearson High (+ residual confirm) → return
3. E2 — baseline_only profile: accept baseline High/Marginal → return
       (skipped for Marginal when anchor_seam_mode = force — anchor runs first)
4. Editorial anchor seam — if triggered; best anchor High/Marginal may return
5. E5 — baseline_only: best pooled candidate or skip
6. Boundary grid — only when fit_boundary_search = full_grid (--full)
7. E6 — best Pearson High among all grid cells (+ residual confirm) → return
8. E7 — best pooled candidate (ranking + residual walk) or skip
```

**Anchor runs before the boundary grid**, not after. The grid evaluates every cell when reached (no early exit on the first `High`); E6 picks the **best** `High` by ranking score.

### Per-bracket measurement (fit)

Each bracket candidate (baseline, anchor, or grid cell) runs the **same** evaluation in `evaluate_seam_gate_fit_candidate`:

```text
border templates → gap signature → unified B search (structure + waveform jointly)
  → structure hard gate → (anchor matchability, if anchor bracket)
  → Pearson tier (High / Marginal / skip)
  → residual veto/rescue (lazy at pool selection when residual_gate is on)
```

Steps 3–7 in the old pedagogical list are **one joint search + gates**, not separate passes. Residual is not a separate macro stage — it applies when finalizing candidates.

### Gate mode (legacy)

Structure match on B → structure gate → waveform Pearson (optional structure-trust skip) → on failure, reactive extend end then start. No anchor seam, no residual gate, no unified fit search. See [gap-fill-modes.md](gap-fill-modes.md) § `fill_mode = gate`.

- **References:** [gap-fill-modes.md](gap-fill-modes.md) (routing, flags, performance), [gap-repair-guide.md](gap-repair-guide.md) (reading/steering), [seam-scoring.md](seam-scoring.md) (seam mechanics), [archive/residual-gate-wiring-plan.md](dev/archive/residual-gate-wiring-plan.md) (residual gate design record).
- **Config:** `fill_mode`, `anchor_seam_mode`, `residual_gate`, `residual_*`, `fit_boundary_search`, plus §3 fill-plan knobs.
- **Code:** `application/patch_audio.rs`, `application/patch_region.rs`, `application/fit_routing.rs`, `domain/gap_fill_fit.rs`, `domain/gap_structure.rs`, `domain/gap_energy.rs`, `domain/gap_anchor_seam.rs`.

## 5. Write / mux

In write mode the patched A audio is emitted:

- **`--wav`** — lossless PCM (no re-encode). Output bit depth is **source-driven**: if A's source track reports 24-bit, 32-bit int, or 32-bit float depth, the WAV is written at **24-bit int**; otherwise 16-bit int. Lossy codecs (AAC, AC-3, MP3, Opus) carry no depth, so they output 16-bit.
- **`--mux`** — copies A's video and re-encodes the patched audio track into A's container via ffmpeg. The PCM pipe format matches the resolved output depth (`-f s16le` or `-f s24le`). Bitrate defaults to `mux_audio_bitrate = "match_min"` (the lower of A and B's measured rates, so output isn't upsampled). A **mux preflight** checks duration and PTS-vs-sample-clock skew before writing.

**Internal representation:** all patched audio is held as normalized `f32` samples in `[-1.0, 1.0]` inside `MultiChannelPcm`. Conversion to output integers happens at write time: `f32 × 32767 → i16` (16-bit path) or `f32 × 8_388_607 → i32` (24-bit path). The source bit depth (`AudioTrack.bit_depth`, populated at Symphonia probe time) is carried through `MultiChannelPcm.source_bit_depth` and resolved by `resolve_output_bit_depth()` to one of `WavBitDepth::Int16` or `WavBitDepth::Int24`. `MonoPcmClip` (used by the chromaprint fingerprinter) remains `Vec<i16>` — it is not affected by this representation.

Report-only / scan-only mode (default `dry_run = true`, no output paths) runs phases 1–2 and writes nothing to disk — see [Orchestration](#orchestration-scan-only-vs-preview-vs-patch-only-vs-write-mode).

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
- [archive/residual-gate-wiring-plan.md](dev/archive/residual-gate-wiring-plan.md) — residual/floor gate (fit mode, default `veto`; archived design record)
- [cli-output.md](cli-output.md) / [json-output.md](json-output.md) — report layout
- [corpus-validation.md](dev/corpus-validation.md) — test corpus and acceptance
- [PLAN.md](../PLAN.md) — architecture and application sketch
