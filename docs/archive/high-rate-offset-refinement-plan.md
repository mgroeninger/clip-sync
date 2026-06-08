# High-rate hold-out PCM refinement (archived)

> **Status:** Completed and archived (2026-06-06). Implementation: `high_rate_refinement.rs`, `offset_refinement.rs`, `policies.rs`.

## Completion verification

| Criterion | Status | Notes |
|-----------|--------|-------|
| Phase 0: slice fix + 11 kHz tests | Done | `aligned_slice_starts_*`, `pcm_lag_fixes_*` |
| Phase 0: Chromaprint residual baseline | Partial | Implicit in cross-layer test (discovery ±1 s → final ±50 ms) |
| Phase 1: core logic + unit tests | Done | `refine_holdout_segment_lag`, `apply_high_rate_refinement`, window picker |
| Phase 2: wire + cross-layer test | Done | `cross_layer_high_rate_refine_tightens_wav_leader_3s` |
| Phase 2: skip / max-adjustment tests | Done | `high_rate_refine_skips_when_window_infeasible`, `refine_high_rate_respects_max_adjustment` |
| Phase 3: reporting + CLI | Done | `HighRateRefinement`, `--refine-offset-high-rate` |
| Phase 4: corpus | Done | `wav_high_rate_refine_3s` |
| Optional MP4 AAC case / tighter discovery tolerance | Done | `mp4_aac_high_rate_refine_3s`; default ±150 ms |

**Problem:** Discovery alignment (Chromaprint + 11.025 kHz PCM refine on **prepared** clips) lands within ~±1 s on corpus cases but can leave **20–50 ms** residual error — Chromaprint item quantization (~124 ms bins) minus partial correction. That is audible as a faint echo when tracks are overlaid. Current `pcm_lag_adjustment_secs` runs on normalized 11 kHz PCM and uses incorrect slice alignment for positive offsets (`left_start` / `right_start` swapped relative to domain convention `t_B = t_A + offset`).

**Goal:** Optional **correction** pass after discovery alignment: re-extract a **short hold-out segment** at **native decode rate** (no fingerprint prep, no 11 kHz downsample), run FFT cross-correlation at lag ≈ 0 using the current recommended Δ, and apply a **small residual adjustment** to `recommended_offset_secs`. Ship a **cross-layer coupling test** that exercises the full stack (Symphonia → AlignVideos → high-rate refine) on a 44.1 kHz WAV oracle pair with **±50 ms** tolerance.

**Not in scope for v1:** Sub-sample parabolic peak interpolation (defer unless ±50 ms corpus still fails on WAV chirp). Native-rate pass on every run without a flag (off by default).

---

## Relationship to other plans

| Plan | Relationship |
|------|--------------|
| [TEMP-offset-verification-plan.md](TEMP-offset-verification-plan.md) | **Verification** scores lag-0 match; does **not** change Δ. **This plan** adjusts Δ. Share `pick_holdout_window` placement logic and hold-out extract shape; verification may call the same window picker with a longer `holdout_length`. |
| [TEMP-clip-self-repetition-plan.md](TEMP-clip-self-repetition-plan.md) | Orthogonal. Repetition runs on prepared discovery clips; high-rate refine uses raw native extracts. |
| `pcm_lag_adjustment_secs` bug | **Prerequisite (Phase 0).** Fix slice alignment in existing 11 kHz refine before or alongside Phase 1 — otherwise discovery-stage refine remains broken for positive leaders. |

---

## Decisions

Locked before implementation. Change only with an explicit plan revision.

| Topic | Decision |
|-------|----------|
| **Purpose** | **Correct** `recommended_offset_secs`, not merely diagnose. When skipped or refinement fails, keep the discovery-stage Δ unchanged. |
| **When it runs** | After `align_single_track_pair` / `align_best_track_pair` returns a `Some(recommended_offset_secs)` and sessions are still open. **Not** inside `align_extracted_pair`. |
| **Default** | **Off** (`alignment.refine_offset_high_rate = false`). Enable in config / CLI when shipped (Phase 3). |
| **Input PCM** | **Native decode rate** from `extract_mono` with **`target_sample_rate = None`** (no 11 kHz resample). **No** `prepare_clip_for_fingerprint`, **no** `select_aligned_subclip_pair`, **no** `expand_window_for_slide`. |
| **Segment length** | `alignment.high_rate_refine_secs` (default **3**). Independent from discovery `clip_length`. |
| **Sample rate policy** | Use each clip at its decoded rate. When A and B differ (e.g. 44.1 vs 48 kHz), resample **both** hold-out segments to `max(rate_a, rate_b)` via existing `resample_mono_pcm` **once** before correlation — do not resample discovery clips. |
| **Max adjustment** | `alignment.high_rate_refine_max_adjustment_secs` (default **0.1**). Discard refinement if `\|adjustment\| > max` or correlation peak is weak. |
| **Window placement** | Reuse shared `pick_holdout_window(duration, discovery_windows, segment_length)` in `src/domain/policies.rs` (same heuristic as verification plan: middle-third for one clip, gap for two+, midpoint fallback). **v1:** skip when `duration < segment_length` (no shorter slice). |
| **Timeline extract** | Given start `T` on A and recommended Δ: `A[T, T+L)`, `B[T + Δ, T + L + Δ)` (domain sign convention). Skip when either file cannot supply full `L` after shift. |
| **`try_all_tracks`** | Re-extract hold-out on **winning** `(track_a, track_b)` only. Record winning track indices during search (same pattern as verification / repetition plans). |
| **Correlation method** | Reuse `cross_correlate` FFT (`CrossCorrelationMode::Full`) on `f64` samples — same crate as `pcm_lag_adjustment_secs`. Normalized time-domain correlation optional guard before applying adjustment. |
| **Slice alignment** | Shared helper `aligned_slice_starts(offset_samples) -> (left_start, right_start)` with `left_start = max(0, -offset_samples)`, `right_start = max(0, offset_samples)`. Used by **both** fixed `pcm_lag_adjustment_secs` and high-rate pass. |
| **Sub-sample peak** | v1: integer peak only. Revisit parabolic interpolation if WAV oracle still exceeds ±50 ms after slice fix. |
| **Hook location** | `AlignVideos::execute()` after discovery alignment, **before** optional offset verification (when that plan lands). Order: align → **high-rate refine** → verify → summary. |
| **Failure behaviour** | Keep discovery Δ; log `tracing::debug` reason. Exit code **0**. No user-visible error in Phase 1–2. |
| **Report model (Phase 3)** | `high_rate_refinement: Option<HighRateRefinement>` on `AlignmentResult`: `{ adjustment_secs, segment_start_secs, correlation_peak, applied: bool, skipped, skip_reason }`. Omit field when flag off. |
| **Architecture** | Window picker in `domain/policies.rs`. Refinement logic in `application/offset_refinement.rs` (extend existing module). Hold-out extract helper in `application/` (e.g. `holdout_extract.rs` or private fns on `align_videos.rs`). No new port trait in v1. |
| **Phase 1 scope** | Core logic + unit tests + cross-layer coupling test + `tracing::debug`. No CLI / JSON / `AlignmentResult` field until Phase 3. |
| **Corpus** | Phase 2: cross-layer test only (generated 44.1 kHz `+3 s` chirp). Phase 4: manifest case `wav_high_rate_refine_3s` with `expect_offset_within_ms = 50` (generated at 44.1 kHz; discovery still at 11 kHz). |

---

## Config

New fields on existing `AlignmentConfig` (correction pass, not validation):

```toml
[alignment]
refine_offset_with_pcm = true          # existing 11 kHz discovery refine
refine_offset_high_rate = false        # new: native-rate hold-out FFT pass
high_rate_refine_secs = 3
high_rate_refine_max_adjustment_secs = 0.1
```

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlignmentConfig {
    // ... existing fields ...
    /// After discovery alignment, re-extract a short native-rate hold-out and FFT-refine Δ.
    #[serde(default)]
    pub refine_offset_high_rate: bool,
    /// Hold-out segment length for high-rate refinement (seconds).
    #[serde(default = "default_high_rate_refine_secs")]
    pub high_rate_refine_secs: u32,
    /// Maximum \|adjustment\| applied from high-rate refinement.
    #[serde(default = "default_high_rate_refine_max_adjustment_secs")]
    pub high_rate_refine_max_adjustment_secs: f64,
}

fn default_high_rate_refine_secs() -> u32 {
    3
}

fn default_high_rate_refine_max_adjustment_secs() -> f64 {
    0.1
}
```

Extract path for hold-out: pass `target_sample_rate: None` into a dedicated extract helper (do not mutate user's `ClipConfig` for discovery).

CLI mirror (Phase 3): `--refine-offset-high-rate` → `config.alignment.refine_offset_high_rate = true`.

---

## Types

```rust
/// Native-rate hold-out FFT correction (Phase 3 on AlignmentResult; Phase 1 internal).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HighRateRefinement {
    pub segment_start_secs: f64,
    pub segment_length_secs: f64,
    pub adjustment_secs: f64,
    pub correlation_peak: f64,
    pub applied: bool,
    #[serde(default)]
    pub skipped: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_reason: Option<String>,
}

// AlignmentResult — add in Phase 3:
#[serde(skip_serializing_if = "Option::is_none")]
pub high_rate_refinement: Option<HighRateRefinement>,
```

Internal Phase 1 result may mirror these fields without `Serialize`.

---

## Phases

### Phase 0 — Prerequisite spike

- [x] Fix `pcm_lag_adjustment_secs` slice alignment via shared `aligned_slice_starts`
- [x] Unit test: +3 s leader at 11 kHz prepared pair — `\|refined − true\| < 50 ms` after fix (today ~29 ms error with refine on/off identical)
- [x] Unit test: −5 s leader (B ahead) — same ±50 ms target
- [ ] Document Chromaprint residual on oracle pair without high-rate pass (baseline for cross-layer test) — *implicit in cross-layer test*

### Phase 1 — Core refinement + unit tests

- [x] Plan doc
- [x] `pick_holdout_window(duration, discovery_windows, segment_length) -> Option<ClipWindow>` in `policies.rs` (implement once; verification plan imports same helper)
- [x] `aligned_slice_starts(offset_samples: i64) -> (usize, usize)` in `offset_refinement.rs`
- [x] `pcm_lag_adjustment_secs(left, right, offset_secs, window_secs)` — use shared starts; keep 20 s window for discovery path
- [x] `refine_holdout_segment_lag` — returns **adjustment only** (planned name: `refine_offset_high_rate_segment`)
- [x] `apply_high_rate_refinement` in application layer (planned name: `refine_recommended_offset_high_rate`)
- [x] Unit tests: known Δ + synthetic native-rate chirp segments → adjustment within 1 sample at 44.1 kHz
- [x] Unit tests: window picker (same cases as verification plan, shorter `segment_length`)

### Phase 2 — Wire + cross-layer coupling test

- [x] Call high-rate refine from `AlignVideos::execute()` when flag on (Phase 1: hard-code `true` in test only; production wire can stay behind flag default false)
- [x] Record winning track indices in `align_best_track_pair` — *via `AlignOutcome { track_a, track_b }`*
- [x] **`cross_layer_high_rate_refine_tightens_wav_leader_3s`** in `align_videos.rs` tests
- [x] `tracing::debug` for adjustment, peak, skip reasons

### Phase 3 — Reporting + CLI

- [x] `high_rate_refinement` on `AlignmentResult`; update `recommended_offset_secs` when `applied`
- [x] Human + JSON output; progress phase: `High-rate offset refinement...`
- [x] CLI `--refine-offset-high-rate`
- [x] Document in PLAN.md; note interaction with `refine_offset_with_pcm` (sequential, not replacement)

### Phase 4 — Corpus + tighter policy

- [x] Manifest case `wav_high_rate_refine_3s` (generated 44.1 kHz, flag on, ±50 ms)
- [ ] Optional: `mp4_aac_high_rate_refine_3s` — **done** (±100 ms tolerance)
- [ ] Tighten default discovery corpus tolerance — **done** (default ±150 ms; per-case overrides for leaders and encoded formats)
- [x] Archive this doc

---

## Design

### Execution flow

```text
execute():
  open sessions
  result = align_single_track_pair(...) or align_best_track_pair(...)
  if alignment.refine_offset_high_rate && result.recommended_offset_secs.is_some():
      progress: "High-rate offset refinement..."
      T = pick_holdout_window(min(dur_a, dur_b), discovery_windows, L)
      if T is None or !feasible(T, Δ, dur_a, dur_b, L):
          high_rate_refinement = { applied: false, skipped: true, ... }
      else:
          extract A[T, T+L), B[T+Δ, T+L+Δ) at native rate (winning tracks)
          adj = refine_offset_high_rate_segment(...)
          if adj is Some and |adj| <= max:
              recommended_offset_secs += adj
              applied = true
          else:
              applied = false
  else if refine_offset_high_rate:
      high_rate_refinement = None   // flag off — omit from JSON
  // optional: offset verification (separate plan)
  log_alignment_summary
  return
```

### Hold-out window placement

Same heuristic as [TEMP-offset-verification-plan.md](TEMP-offset-verification-plan.md) but `segment_length = high_rate_refine_secs` (default 3 s, not `clip_length`).

```text
pick_holdout_window(duration, windows, segment_length):
  if duration < segment_length:
    return None
  if windows.len() == 1:
    T = duration / 3
    return [T, min(T + segment_length, duration))
  if windows.len() >= 2:
    gap_start = windows[0].end
    gap_end = windows.last().start
    if gap_end - gap_start >= segment_length:
      T = gap_start + (gap_end - gap_start - segment_length) / 2
      return [T, T + segment_length)
    else:
      T = (duration - segment_length) / 2
      return [T, T + segment_length)
```

Feasibility (per-file durations `dur_a`, `dur_b`, recommended Δ):

```text
A needs: 0 <= T           and T + L <= dur_a
B needs: 0 <= T + Δ       and T + L + Δ <= dur_b
If either fails → skip (skipped: true)
```

### High-rate FFT refine (lag ≈ 0)

```text
refine_offset_high_rate_segment(left, right, offset_secs, segment_secs, max_adj):
  rate = max(left.sample_rate, right.rate)  // resample both to rate if needed
  offset_samples = round(offset_secs * rate)
  (left_start, right_start) = aligned_slice_starts(offset_samples)
  window = segment_secs * rate samples
  if slices out of bounds or near-silent → None
  corr = FFT cross_correlate(left_slice, right_slice)   // Valid mode
  lag_samples = peak_index_to_lag(corr)                   // integer v1
  adjustment = -lag_samples / rate
  if |adjustment| > max_adj → None
  else Some(adjustment)
```

Sign matches existing `pcm_lag_adjustment_secs`: positive adjustment increases Δ.

### Why not reuse discovery clips?

| Discovery clip | Hold-out extract |
|----------------|------------------|
| 11.025 kHz resampled | Native decode rate |
| Peak-normalized | Raw PCM |
| Trailing silence trimmed | Untrimmed |
| Optional window_slide (index-aligned, not Δ-aware) | Timeline `[T, T+L)` / `[T+Δ, T+L+Δ)` |
| 60 s (typical) | 3 s |

---

## Tests

| Test | Phase | Asserts |
|------|-------|---------|
| `aligned_slice_starts_positive_offset` | 0 | +Δ → `(0, offset_samples)` |
| `aligned_slice_starts_negative_offset` | 0 | −Δ → `(-offset_samples, 0)` |
| `pcm_lag_fixes_three_second_leader_at_11k` | 0 | Prepared 11 kHz +3 s chirp, coarse ≈ Chromaprint → refined within ±50 ms |
| `pick_holdout_window_middle_for_single_clip` | 1 | 60 s media, 3 s segment → hold-out in middle third |
| `refine_high_rate_segment_known_lag` | 1 | Synthetic 44.1 kHz segments, injected 20 ms error → adjustment ≈ −20 ms |
| **`cross_layer_high_rate_refine_tightens_wav_leader_3s`** | **2** | Full `AlignVideos` + native hold-out → **±50 ms** on 44.1 kHz +3 s chirp |
| `high_rate_refine_skips_when_window_infeasible` | 2 | Δ pushes B window past `dur_b` → `skipped: true` |
| `high_rate_refine_respects_max_adjustment` | 2 | Inject 500 ms error → `applied: false` |
| Corpus `wav_high_rate_refine_3s` | 4 | End-to-end, flag on, ±50 ms |

### Cross-layer coupling test (Phase 2 detail)

Lives in `src/application/testing/` — intentionally couples:

- `SymphoniaMediaReader` (infrastructure)
- `ChromaprintFingerprinter` / `ChromaprintAligner` (infrastructure)
- `AlignVideos` (application)
- `refine_offset_high_rate_segment` + hold-out extract (application)
- `pick_holdout_window` (domain)
- `write_offset_chirp_wav_pair` at **44_100** Hz (test fixtures)

```rust
// Pseudocode — not shipped verbatim
#[test]
fn cross_layer_high_rate_refine_tightens_wav_leader_3s() {
    let (a, b) = write_44100_chirp_pair(+3s);
    let config = AppConfig {
        clip: ClipConfig {
            target_sample_rate: Some(11_025),
            window_slide_secs: 0,
            normalize_loudness: false,
            trim_silence: false,
            ..
        },
        alignment: AlignmentConfig {
            refine_offset_with_pcm: true,
            refine_offset_high_rate: true,  // or call refine fn directly in Phase 1
            ..
        },
        ..
    };
    let result = AlignVideos::new(...).execute(...)?;
    let discovery = result.recommended_offset_secs.unwrap();
    assert!((discovery - 3.0).abs() <= 1.0, "discovery baseline");

    let final_delta = result.recommended_offset_secs.unwrap(); // after high-rate
    assert!(
        (final_delta - 3.0).abs() <= 0.050,
        "discovery={discovery}, final={final_delta}"
    );
}
```

---

## Skip conditions

Skip high-rate refinement (`skipped: true`, `applied: false`, discovery Δ unchanged) when:

- `refine_offset_high_rate` is false (omit `high_rate_refinement` from JSON entirely).
- No `recommended_offset_secs`.
- `pick_holdout_window` returns `None`.
- Hold-out window infeasible after applying Δ (full segment required on both sides).
- Either native hold-out extract fails (`InsufficientAudio` / decode error).
- Either slice near-silent or correlation peak below internal threshold.
- `|adjustment| > high_rate_refine_max_adjustment_secs`.

---

## Output (Phase 3)

**JSON** (flag on, refinement applied):

```json
{
  "recommended_offset_secs": 3.000,
  "high_rate_refinement": {
    "segment_start_secs": 20.0,
    "segment_length_secs": 3.0,
    "adjustment_secs": 0.0286,
    "correlation_peak": 0.98,
    "applied": true,
    "skipped": false
  }
}
```

**Human** (`--verbose`, skipped):

```text
  High-rate refinement skipped: hold-out window unavailable
```

---

## References

- `src/application/align_videos.rs` — `execute()` post-alignment hook; hold-out extract
- `src/application/offset_refinement.rs` — `pcm_lag_adjustment_secs`, new high-rate refine
- `src/domain/policies.rs` — `pick_holdout_window` (shared with verification plan)
- `src/domain/resample.rs` — rate mismatch on hold-out pair only
- `src/application/testing/audio_fixtures.rs` — 44.1 kHz chirp pair for cross-layer test
- [TEMP-offset-verification-plan.md](TEMP-offset-verification-plan.md) — shared window picker; diagnostic lag-0 (no Δ change)
- [BACKLOG.md](../BACKLOG.md) — high-rate refinement done (2026-06-06)
