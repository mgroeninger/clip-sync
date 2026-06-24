# Alignment

How `clip-sync` finds the time **offset** that lines up two recordings of the same event. This is **phase 1** of the [repair pipeline](pipeline.md) and the whole job of the analyzer. The repair tool runs the same engine in-process.

Output is an `AlignmentResult`: a recommended **offset** (seconds to shift B onto A), a **confidence**, **start/end clip anchors**, and the shared **overlap** window. Repair maps every gap to B with `b = a + recommended_offset_secs`.

Source: the `clip-sync` library (`domain` alignment + `infrastructure` Chromaprint/Symphonia adapters). Architecture: [PLAN.md](../PLAN.md) § Analyzer workflow. Deep design lives in the archived plans linked below.

---

## Mode selection

`alignment.mode` (default `auto`) picks the strategy from the two files' durations:

| Mode | When | Strategy |
|------|------|----------|
| **`auto`** (default) | — | Query-reference when the shorter file is *much* shorter than the longer (`query_min_duration_ratio`, default 0.5); otherwise symmetric. |
| **`symmetric`** | comparable-length files | Multi-clip fingerprint alignment (the classic path). |
| **`queryreference`** | a short excerpt vs a long recording | Localize the short file within the long one. |

CLI: `--symmetric-align`, `--query-reference`.

## Symmetric path

For comparable-length files (`--num-clips`, default 2 for repair / 1 for analyzer):

1. **Clip windows** — pick `num_clips` windows of `clip_length` (default 15 min) from each file.
2. **Fingerprint** — extract mono PCM per window (resampled to Chromaprint's native 11025 Hz) and compute Chromaprint fingerprints.
3. **Match** — `find_offset` pairwise by clip index; a match must clear `min_match_score` (default 0.5).
4. **Merge** — with `num_clips > 1`, combine per-clip offsets into a recommended offset, and derive **start-clip** and **end-clip** anchors. `require_consistent_offsets` / `prefer_start_clip` govern how disagreement is resolved.

The **end-clip anchor** (`end_clip_anchor`, default `SharedTimeline`) controls how the tail window is chosen; `FileTail` is the legacy per-file-tail behavior. End-clip refinement (`refine_end_clip_around_start_offset`, `end_clip_refine_radius_secs`) and reliability gates (`skip_unreliable_end_clip`, `min_end_clip_decode_fraction`, `max_end_clip_decode_skips`) keep a bad tail window from corrupting the offset.

## Query-reference path

For a short A against a long B (or vice-versa — the short side is the "query"):

1. Fingerprint the full prepared **query** clip.
2. **Coarse sliding-window search** over the reference timeline (`query_search_stride_secs` 60 s stride, `query_decode_bucket_secs` 10 s buckets, up to `query_max_windows_scored` 500 windows, `query_min_match_score` 0.3).
3. **PCM-refine** the top `query_refine_top_k` winning anchor(s).
4. Build a synthetic single-clip result with the **mapped region** — the slice of the long file the query localizes to. In repair, gaps outside this region are reported but not fillable when `limit_fill_to_mapped_region` is set (default).

Design: [archive/query-reference-alignment-plan.md](archive/query-reference-alignment-plan.md); short-A/long-B donor: [archive/query-reference-b-longer-plan.md](archive/query-reference-b-longer-plan.md).

## Offset refinement & verification

- **PCM refinement** (`refine_offset_with_pcm`) — correlates real PCM around the fingerprint discovery point to sharpen the offset below fingerprint resolution.
- **High-rate FFT refinement** (`refine_offset_high_rate`, `--refine-offset-high-rate`, default off) — a native-rate FFT pass over a hold-out window (`high_rate_refine_secs` 3 s, bounded by `high_rate_refine_max_adjustment_secs` 0.1 s). Surfaces as `High-rate: start … end … refinement applied; refined drift …` in the report.
- **Verification / cross-check** (`validation`) — hold-out verification and an independent **silence-based** offset check (`Cross-chk: silence-based … vs alignment … (Δ … — AGREE)` in the report).

## Drift, ambiguity, repetition

- **Offset drift** — when the start-clip and end-clip offsets differ (clocks ran at slightly different rates), the report shows both. Repair's `fill_offset_mode = interpolated` / `anchored_retry` exist to track this across a long file ([gap-fill-modes.md](gap-fill-modes.md)).
- **Periodic ambiguity** — repetitive content (loops, chants) can match at multiple offsets; the engine detects and reports the ambiguity rather than picking blindly. Design: [archive/periodic-ambiguity-plan.md](archive/periodic-ambiguity-plan.md).
- **Clip self-repetition** — `validation.check_clip_repetition` guards against a clip window that repeats within a file. Design: [archive/clip-self-repetition-plan.md](archive/clip-self-repetition-plan.md).
- **Track selection** — `try_all_tracks` / `--try-all-tracks` lets alignment consider tracks beyond the default pick (e.g. when the default-muxed track is the wrong language/layout).

## Reading the report

```text
Alignment: offset -4.778s  confidence 0.83
  Start clip: -4.802s  (confidence 0.83)
  End clip:   -4.754s  (confidence 0.83)
Overlap:   A [4.78s – 900.00s]   B [0.00s – 895.22s]   (895.2s shared)
High-rate: start -0.044s, end -0.005s refinement applied; refined drift +0.048s
Cross-chk: silence-based -4.750s vs alignment -4.778s  (Δ 0.028s — AGREE)
```

`offset` is the recommended value; start/end clips show per-anchor offsets (their spread is drift); `Overlap` is the shared window gaps must fall inside to be fillable.

> **Clock caveat:** offsets and gap times are on the **decoded-sample clock**, which can differ from container/ffmpeg PTS — hence the "shared overlap starts at N.Ns (not 0:00)" warning on files whose audio doesn't start at 0. Prefer a clean source / MKV. See [cli-output.md](cli-output.md) § Timeline warnings.

## Config

| Section / key | Default | Notes |
|---------------|---------|-------|
| `clip.clip_length` | 15 min | Window length (min 1 min) |
| `clip.num_clips` | 1 (repair: 2) | Windows per file |
| `clip.target_sample_rate` | 11025 | Chromaprint native rate |
| `alignment.mode` | `auto` | `auto` / `symmetric` / `queryreference` |
| `alignment.min_match_score` | 0.5 | Symmetric match floor |
| `alignment.refine_offset_high_rate` | off | Native-rate FFT refinement (`--refine-offset-high-rate`) |
| `alignment.end_clip_anchor` | `SharedTimeline` | Tail-window strategy |
| `alignment.query_*` | — | Query-reference search/scoring knobs |
| `validation.*` | — | Offset verification, clip-repetition guard |

Full `AlignConfig` layout: [PLAN.md](../PLAN.md) § `AlignConfig`.

## Related reading

- [pipeline.md](pipeline.md) — where alignment sits (phase 1)
- [PLAN.md](../PLAN.md) § Analyzer workflow / Repair workflow
- [corpus-matrix.md](corpus-matrix.md) — alignment corpus
- `archive/` — `query-reference-alignment-plan.md`, `query-reference-b-longer-plan.md`, `offset-verification-plan.md`, `high-rate-offset-refinement-plan.md`, `periodic-ambiguity-plan.md`, `clip-self-repetition-plan.md` (historical design)
