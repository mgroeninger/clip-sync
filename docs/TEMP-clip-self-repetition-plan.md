# Temporary plan: clip self-repetition check

> **Status:** Phase 0 complete (2026-06-06). Phase 1 not started. Archive to `docs/archive/clip-self-repetition-plan.md` when shipped.

### Phase 0 outcome (2026-06-06)

| Check | Result |
|-------|--------|
| `match_fingerprints(&fp, &fp)` on 10s@0 + 10s@30 synthetic clip | **Rejected** — library returns a single lag-0 segment only (`repetition_spike.rs` tests). |
| Default preset `item_duration_in_seconds()` | **~0.124 s** (Test2). `min_lag_secs` ≈ **4.95 s** (40 items). |
| **Fallback chosen** | **Half-vs-half** fingerprint match at timeline midpoint, with `\|lag_items\| > 1` and `\|lag_secs − duration/2\| ≤ 3 s` guard. |
| Synthetic repeat (silent gaps) | Detects lag **~32.5 s** on 60 s clip (confidence ≥ 0.5). |
| Monotonic chirp control | **No finding** (midpoint guard rejects ~41 s false peak). |
| Chirp-filled repeat (Phase 3 corpus shape) | **Not detected** in spike — tone block drowned by chirp; corpus fixture may need stronger tone or PCM assist in Phase 3. |

Phase 1 detection should implement half-vs-half (not full self-match). Revise [Detection algorithm](#detection-algorithm) before coding.

**Problem:** A clip whose audio repeats internally (loop, rebroadcast, duplicated segment) can produce ambiguous Chromaprint matches — both cross-file alignment and offset verification may latch onto the wrong lag with high confidence.

**Goal:** Optional per-clip diagnostic that fingerprints each **prepared** clip against itself, detects strong non-zero internal repeats, and surfaces findings on `ClipMatch`. Off by default (`validation.check_clip_repetition`). Diagnostic only — never changes exit code or clears `recommended_offset_secs` in v1.

---

## Decisions

Locked before implementation. Change only with an explicit plan revision.

| Topic | Decision |
|-------|----------|
| **Phase 0 gate** | **Done (2026-06-06).** Full self-match rejected; half-vs-half + midpoint guard chosen. See spike module. |
| **Input audio** | Same prepared PCM / fingerprints used for cross-file match: after `prepare_clip_for_fingerprint`, and after `select_aligned_subclip_pair` when `window_slide_secs > 0`. Not raw extracts. |
| **Which files** | Check **both** video A and video B for each clip index in the align loop. |
| **Report model** | `repetition: Option<ClipRepetitionReport>` on `ClipMatch` when check ran; `None` on `ClipMatch` when check was off (field omitted from JSON). |
| **`try_all_tracks`** | Run repetition only for the **winning** track pair after `align_best_track_pair` selects it. Retain `winning_track_a` / `winning_track_b` in the search loop. Do not check every candidate pair during search. |
| **Lag-zero exclusion** | Always drop segments with internal lag in `[-1, +1]` items before clustering. Lag ≈ 0 is expected self-similarity, not a repeat. |
| **`lag_secs` sign** | Report **positive** duration only: “content near the start of the clip reappears approximately `lag_secs` later.” Normalize `abs(offset2 - offset1) * item_secs`. |
| **Detection threshold** | `validation.min_repetition_confidence` (default `0.5`). Independent knob from `alignment.min_match_score`. |
| **Short clips** | Return `None` when prepared clip duration `< 2 * min_lag_secs` (~10 s with default `min_lag_items`). |
| **Skipped / empty clips** | Skip check when fingerprint is empty (prepare failure, insufficient audio) — same as aligner. |
| **Clip duration** | Add `MonoPcmClip::duration_secs()` in `domain/mono_pcm_clip.rs`: `samples.len() as f64 / sample_rate as f64`. |
| **Shared matching helpers** | Extract `select_best_segment`, `segment_confidence`, and lag clustering to `chromaprint/matching.rs` as `pub(crate)`; used by `aligner.rs` and `repetition.rs`. |
| **Architecture** | Detection in `infrastructure/chromaprint/repetition.rs`; report types in `domain/alignment.rs`. No new port trait in v1. |
| **Phase 1 scope** | Detection + unit tests + `tracing::debug` only. No stdout/JSON/CLI until Phase 2. |
| **User-visible failure** | Diagnostic only: exit code stays **0**; never clear `recommended_offset_secs` in v1. |
| **JSON (Phase 2)** | Flag **off** → `repetition` key absent on `ClipMatch`. Flag **on** → `repetition` object always present per clip; inner `a` / `b` are `null` when no finding (explicit nulls). |
| **Human (Phase 2)** | Repetition lines only when a finding exists **or** `--verbose` (then show “no internal repeat” per video). |
| **Confidence downgrade (Phase 3)** | If either repetition lag is within ±**1 s** of `offset_secs.abs()`, multiply displayed `ClipMatch.confidence` by `0.5`. Compute `aligned` from **pre-downgrade** confidence so downgrade never flips `aligned` in v1. |
| **Corpus assertions** | See [Tests](#tests). |

---

## Config

New `ValidationConfig` section on `AppConfig` (shared with [TEMP-offset-verification-plan.md](TEMP-offset-verification-plan.md)). Implement the **full** struct once when either feature lands:

```toml
[validation]
check_clip_repetition = false
min_repetition_confidence = 0.5
verify_offset = false                    # offset-verification plan
min_verification_confidence = 0.5        # offset-verification plan
```

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidationConfig {
    #[serde(default)]
    pub check_clip_repetition: bool,
    #[serde(default = "default_min_repetition_confidence")]
    pub min_repetition_confidence: f32,
    #[serde(default)]
    pub verify_offset: bool,
    #[serde(default = "default_min_verification_confidence")]
    pub min_verification_confidence: f32,
}

fn default_min_repetition_confidence() -> f32 {
    0.5
}
```

- No extra validation rules on `ValidationConfig` for v1.
- CLI mirror (Phase 2): `--check-clip-repetition` → `config.validation.check_clip_repetition = true` in `apply_cli_overrides`.
- When flag is **off**, `ClipMatch.repetition` stays `None` → key omitted via `#[serde(skip_serializing_if = "Option::is_none")]`.

When enabled, fingerprint each prepared clip once and reuse that fingerprint for cross-file match and self-match.

**Behaviour when repetition is detected:**

- Exit code **0** always.
- Phase 2: `repetition` on each `ClipMatch` when flag was on; human line when finding or verbose.
- Phase 3: confidence display downgrade per [Confidence downgrade](#confidence-downgrade-phase-3); `aligned` unchanged.

---

## Types

All report types live in `src/domain/alignment.rs`.

```rust
/// Internal repeat within a single prepared clip.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RepetitionFinding {
    /// Positive seconds between repeated content (see Decisions).
    pub lag_secs: f64,
    pub confidence: f32,
    pub items_count: usize,
}

/// Per-clip repetition diagnostics when check_clip_repetition was enabled.
/// Always populated per clip; `None` sides serialize as JSON `null` (no skip on a/b).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ClipRepetitionReport {
    pub a: Option<RepetitionFinding>,
    pub b: Option<RepetitionFinding>,
}

pub struct ClipMatch {
    // ... existing fields ...
    /// Present when validation.check_clip_repetition was true for this run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repetition: Option<ClipRepetitionReport>,
}
```

**JSON null semantics:** When flag is on, set `repetition: Some(ClipRepetitionReport { a, b })` for every clip (`a` / `b` use default `Option` serde → `null` when `None`). When flag is off, `ClipMatch.repetition` is `None` and the outer key is omitted via `skip_serializing_if`.

Phase 1 uses `ClipRepetitionDiagnostics` (same shape as `ClipRepetitionReport`) in a side `Vec` parallel to `estimates`; merged in Phase 2.

```rust
// domain/mono_pcm_clip.rs (Phase 1)
impl MonoPcmClip {
    pub fn duration_secs(&self) -> f64 {
        self.samples.len() as f64 / f64::from(self.sample_rate)
    }
}
```

---

## Phases

### Phase 0 — Spike (blocking) ✅

- [x] `match_fingerprints(&fp, &fp)` on synthetic 10s-block-at-0-and-30s clip — **lag-0 only; not viable**
- [x] Record `item_duration_in_seconds()` for default preset (~0.124 s Test2; `min_lag_secs` ~4.95 s)
- [x] **Fallback:** half-vs-half fingerprint split + midpoint guard (`src/infrastructure/chromaprint/repetition_spike.rs`)
- [x] Outcome recorded at top of this doc

### Phase 1 — Core detection

- [ ] `ValidationConfig` on `AppConfig` (full struct including offset-verification fields)
- [ ] `MonoPcmClip::duration_secs()`
- [ ] `chromaprint/matching.rs` — `pub(crate)` `select_best_segment`, `segment_confidence`, `select_best_nonzero_lag_segment`
- [ ] `chromaprint/repetition.rs` — `detect_clip_repetition(...)`
- [ ] `align_extracted_pair(..., collect_repetition: bool)` — run detection when `collect_repetition`
- [ ] Collect `Vec<ClipRepetitionDiagnostics>` parallel to `estimates`; `tracing::debug` only
- [ ] Unit tests (see [Tests](#tests)); no `ClipMatch.repetition` / stdout changes yet

### Phase 2 — Reporting

- [ ] Extend `build_alignment_result` to accept `Option<&[ClipRepetitionDiagnostics]>` when flag was on
- [ ] Set `ClipMatch.repetition = Some(ClipRepetitionReport { a, b })` for every clip when flag was on
- [ ] `output.rs`: human line per clip when finding present or `show_diagnostics`
- [ ] CLI `--check-clip-repetition`
- [ ] `try_all_tracks`: retain winning tracks in loop; post-selection `align_extracted_pair(collect_repetition: true)`; merge `repetition` into `best.clips` only
- [ ] Document in PLAN.md and corpus-validation.md

### Phase 3 — Corpus + policy

- [ ] Generator `repeated_segment_clip` in `audio_fixtures.rs` (10s tone @ 0s + copy @ 30s in 60s clip)
- [ ] Manifest case `repeated_segment_in_clip` + corpus harness assertion helper
- [ ] Confidence downgrade in `align_extracted_pair` (display confidence only; `aligned` from pre-downgrade)
- [ ] Archive this doc

---

## Design

### Module layout

```text
src/infrastructure/chromaprint/
  matching.rs    # pub(crate) segment selection + confidence (shared)
  repetition.rs  # detect_clip_repetition
  aligner.rs     # uses matching.rs
  mod.rs         # mod matching; mod repetition;
```

### Detection API

```rust
// infrastructure/chromaprint/repetition.rs
pub fn detect_clip_repetition(
    fingerprint: &Fingerprint,
    clip_duration_secs: f64,
    preset: ChromaprintPreset,
    min_confidence: f32,
) -> Option<RepetitionFinding>
```

| Guard | Action |
|-------|--------|
| `fingerprint.data.is_empty()` | `None` |
| `clip_duration_secs < 2.0 * min_lag_secs` | `None` |
| Best non-zero cluster confidence `< min_confidence` | `None` |
| Phase 0 fallback path | Half-vs-half match only if full self-match unusable |

### Detection algorithm (Phase 1 — revise from Phase 0 spike)

Phase 0 rejected full self-match. Phase 1 should use **half-vs-half** on the prepared clip fingerprint:

```text
fp = fingerprint(prepared_clip)
mid_item = round((clip_duration_secs / 2) / item_secs)
(left, right) = split fp.data at mid_item
segments = match_fingerprints(left, right, config)

segments' = segments where |offset2 - offset1| > 1 item
(segment, ambiguous) = select_best_nonzero_lag_segment(segments')
lag_secs = (clip_duration_secs / 2) + |offset2 - offset1| * item_secs
if |lag_secs - clip_duration_secs/2| > 3.0 → None   # reject monotonic-chirp false peak
confidence = segment_confidence(...)
if confidence >= min_repetition_confidence:
  → Some(RepetitionFinding { lag_secs, confidence, items_count })
else:
  → None
```

**Limitations (document in Phase 1):** detects repeats aligned near the clip midpoint (corpus fixture shape). General repeats at other lags need a future approach (e.g. PCM template scan). Chirp-heavy clips may miss repeats when the duplicated block is fingerprint-quiet.

| Constant | Value | Rationale |
|----------|-------|-----------|
| `min_lag_items` | 40 | ~5 s at default preset; exclude shorter segments before clustering |
| `min_lag_secs` | `min_lag_items * item_duration` | Short-clip guard |
| `max_lags_reported` | 1 | Primary repeat only in v1 |

### Integration (`align_extracted_pair`)

```text
fn align_extracted_pair(..., collect_repetition: bool) -> (AlignmentResult, Option<Vec<ClipRepetitionDiagnostics>>)
  // Phase 1: diagnostics returned separately or stored; Phase 2: merged into ClipMatch

for each clip index:
  prepare clip_a, clip_b (existing)
  if skippable prepare error → push zero estimate; push empty diagnostics; continue

  fingerprint_a, fingerprint_b (existing)

  if collect_repetition:
      repetition_a = detect_clip_repetition(&fingerprint_a, clip_a.duration_secs(), ...)
      repetition_b = detect_clip_repetition(&fingerprint_b, clip_b.duration_secs(), ...)
      debug!(?repetition_a, ?repetition_b, "clip self-repetition")
  else:
      repetition_a, repetition_b = None

  estimate = find_offset(...) (existing)
  estimate = refine_offset_with_pcm(...) (existing)
  alignment_confidence = estimate.confidence   // pre-downgrade; used for aligned in Phase 3

  Phase 3: display_confidence = estimate.confidence; if should_downgrade(...) { display_confidence *= 0.5 }

  push estimate + display_confidence + alignment_confidence; push ClipRepetitionDiagnostics { a, b }

build_alignment_result(..., repetitions, display_confidences, alignment_confidences)
```

**`align_single_track_pair`:** one call with `collect_repetition: config.validation.check_clip_repetition`.

**`try_all_tracks` path:**

```text
align_best_track_pair:
  mut winning_a, winning_b = first decodable pair
  mut best = None
  for track_a in decodable_a:
    for track_b in decodable_b:
      (result, _) = align_extracted_pair(..., collect_repetition: false)
      if score > best_score:
        best = Some(result)
        winning_a = track_a
        winning_b = track_b
  if config.validation.check_clip_repetition:
      (_, diagnostics) = align_extracted_pair(
          extract(session_a, winning_a), extract(session_b, winning_b),
          collect_repetition: true,
      )
      merge diagnostics into best.clips[].repetition (offsets/confidence unchanged)
  return best
```

The post-selection pass re-decodes and re-fingerprints the winning pair only (acceptable cost; avoids detection during search).

### Confidence downgrade (Phase 3)

```rust
fn should_downgrade(
    repetition_a: &Option<RepetitionFinding>,
    repetition_b: &Option<RepetitionFinding>,
    offset_secs: f64,
) -> bool {
    const TOLERANCE: f64 = 1.0;
    repetition_a.is_some_and(|r| (r.lag_secs - offset_secs.abs()).abs() <= TOLERANCE)
        || repetition_b.is_some_and(|r| (r.lag_secs - offset_secs.abs()).abs() <= TOLERANCE)
}
```

- `aligned` = `alignment_confidence >= min_match_score` (computed **before** downgrade).
- `ClipMatch.confidence` = `display_confidence` (after downgrade when applicable).
- Downgrade never changes `aligned`, `offset_secs`, or `recommended_offset_secs`.

### Output (Phase 2)

**JSON** (`--format json`, flag on):

```json
{
  "clips": [{
    "label": "start",
    "window_start_secs": 0.0,
    "window_end_secs": 60.0,
    "aligned": true,
    "offset_secs": 3.0,
    "confidence": 0.85,
    "repetition": {
      "a": { "lag_secs": 30.0, "confidence": 0.72, "items_count": 48 },
      "b": null
    }
  }]
}
```

**JSON** (flag off): no `repetition` key on clips.

**Human** (repetition found):

```text
  Start clip [0:00–1:00]: aligned, offset +3.000s (confidence 0.85)
    video A: internal repeat ~30.0s (confidence 0.72)
```

**Human** (`--verbose`, no repetition):

```text
  Start clip [0:00–1:00]: aligned, offset +3.000s (confidence 0.85)
    video A: no internal repeat detected
    video B: no internal repeat detected
```

### False positives

Choruses, applause, and steady test tones may trigger repetition. Output is a **warning** only. `validation.fail_on_repetition` is out of scope for v1.

### Corpus case `repeated_segment_in_clip`

| Field | Value |
|-------|-------|
| Generator | `repeated_segment_clip` — 60s WAV, 10s 440 Hz tone at 0s, same 10s at 30s, chirp elsewhere |
| Partner | Standard +3s leader pair (reuse existing chirp leader fixture) |
| `check_clip_repetition` | `true` |
| `num_clips` | `1` |
| Assert | `clips[0].repetition` is `Some`; `a` or `b` finding: `lag_secs` ∈ [28, 32], `confidence >= 0.5` |
| Assert | `recommended_offset_secs` within existing +3s tolerance |
| Assert | Exit 0, `start_aligned` true |

---

## Tests

| Test | Phase | Asserts |
|------|-------|---------|
| Phase 0 spike | 0 | Half-vs-half on silent-gap repeat → lag ≈ 30–32.5 s; full self-match lag-0 only; chirp control none |
| `detect_clip_repetition_none_on_chirp` | 1 | Monotonic chirp → `None` |
| `detect_clip_repetition_none_on_empty` | 1 | Empty fingerprint → `None` |
| `detect_clip_repetition_none_when_too_short` | 1 | 8s clip → `None` |
| `detect_clip_repetition_finds_copied_block` | 1 | 10s @ 0s + 10s @ 30s → `lag_secs` ∈ [28, 32], `confidence >= 0.5` |
| `select_best_nonzero_lag_ignores_zero_cluster` | 1 | Strong lag-0 + weaker lag-N → picks N |
| `mono_pcm_clip_duration_secs` | 1 | Helper matches sample len / rate |
| `align_repetition_debug_only_phase1` | 1 | Flag on; `ClipMatch.repetition` still `None` |
| `align_json_repetition_object_when_flag_on` | 2 | Flag on → every clip has `repetition` object; null sides when no finding |
| `align_json_no_repetition_key_when_flag_off` | 2 | Flag off → no `repetition` key |
| `align_human_shows_repeat_line` | 2 | Finding present → `internal repeat` in human output |
| `align_human_verbose_shows_none` | 2 | Verbose, no finding → `no internal repeat` |
| `downgrade_lowers_confidence_not_aligned` | 3 | Repeat @ 30s, offset +30s, pre-downgrade above threshold → `confidence` halved, `aligned` still true |
| `try_all_tracks_repetition_on_winner_only` | 2 | `detect` call count == 2 × num_clips (both sides), not × track pairs |
| Corpus `repeated_segment_in_clip` | 3 | Per [Corpus case](#corpus-case-repeated_segment_in_clip) |

---

## References

- `src/infrastructure/chromaprint/matching.rs` — **new** shared segment selection
- `src/infrastructure/chromaprint/repetition.rs` — **new** `detect_clip_repetition`
- `src/infrastructure/chromaprint/aligner.rs` — uses `matching.rs`
- `src/application/align_videos.rs` — `align_extracted_pair`, `align_best_track_pair`
- `src/domain/alignment.rs` — `RepetitionFinding`, `ClipRepetitionReport`, `ClipMatch`
- `src/domain/mono_pcm_clip.rs` — `duration_secs()`
- `src/domain/pcm_preparation.rs` — `prepare_clip_for_fingerprint`, `select_aligned_subclip_pair`
- `src/application/config.rs` — `ValidationConfig`
- `src/infrastructure/cli/args.rs`, `mod.rs` — `--check-clip-repetition`
- `src/infrastructure/cli/output.rs` — human repetition lines
- `src/application/testing/audio_fixtures.rs`, `corpus_fixtures.rs` — generator + manifest
- [TEMP-offset-verification-plan.md](TEMP-offset-verification-plan.md) — shared `ValidationConfig`
- [BACKLOG.md](../BACKLOG.md) — add item when work starts
