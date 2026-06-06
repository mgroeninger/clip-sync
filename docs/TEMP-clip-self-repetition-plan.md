# Temporary plan: clip self-repetition check

> **Status:** Not started. Archive to `docs/archive/clip-self-repetition-plan.md` when shipped.

**Problem:** A clip whose audio repeats internally (loop, rebroadcast, duplicated segment) can produce ambiguous Chromaprint matches — both cross-file alignment and offset verification may latch onto the wrong lag with high confidence.

**Goal:** Optional per-clip diagnostic that fingerprints each **prepared** clip against itself, detects strong non-zero internal repeats, and surfaces findings on `ClipMatch`. Off by default (`validation.check_clip_repetition`). Diagnostic only — never changes exit code or clears `recommended_offset_secs` in v1.

---

## Decisions

Locked before implementation. Change only with an explicit plan revision.

| Topic | Decision |
|-------|----------|
| **Input audio** | Same prepared PCM / fingerprints used for cross-file match: after `prepare_clip_for_fingerprint`, and after `select_aligned_subclip_pair` when `window_slide_secs > 0`. Not raw extracts. |
| **Which files** | Check **both** video A and video B for each clip index in the align loop. |
| **Report model** | `repetition_a: Option<RepetitionFinding>` and `repetition_b: Option<RepetitionFinding>` on `ClipMatch`. |
| **`try_all_tracks`** | Run repetition only for the **winning** track pair after `align_best_track_pair` selects it. Do not check every candidate pair during search. |
| **Lag-zero exclusion** | Always drop clusters with internal lag in `[-1, +1]` items (same tolerance as `select_best_segment`). Lag ≈ 0 is expected self-similarity, not a repeat. |
| **`lag_secs` sign** | Report **positive** duration only: “content near the start of the clip reappears approximately `lag_secs` later.” Normalize `abs(offset2 - offset1) * item_secs`. |
| **Detection threshold** | `validation.min_repetition_confidence` (default `0.5`, same as `alignment.min_match_score` default). Independent knob from alignment. |
| **Short clips** | Return `None` when prepared clip duration `< 2 * min_lag_secs` (~10 s with default `min_lag_items`). |
| **Skipped / empty clips** | Skip check when fingerprint is empty (prepare failure, insufficient audio) — same as aligner. |
| **Architecture** | Private helper in `src/infrastructure/chromaprint/` (no new port trait in v1). |
| **Phase 1 scope** | Detection + unit tests + `tracing::debug` only. No stdout/JSON/CLI until Phase 2. |
| **User-visible failure** | Diagnostic only: exit code stays **0**; never clear `recommended_offset_secs` in v1. |
| **Human / JSON (Phase 2)** | Emit repetition fields on `ClipMatch` in JSON always when flag is on (`null` when none). Human diagnostics only when repetition found **or** `--verbose`. |
| **Confidence downgrade (Phase 3)** | If **either** `repetition_*`.lag is within ±**1 s** of that clip pair’s cross-file `offset_secs`, multiply **pair** confidence by `0.5`. Do not flip `aligned` to false in v1. |
| **Corpus assertions** | See [Tests](#tests) — explicit lag and confidence bounds. |
| **POC risk** | Phase 0 spike: confirm `match_fingerprints(&fp, &fp)` returns usable non-zero segments on synthetic repeat. |

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

- No extra validation rules on `ValidationConfig` for v1 (booleans + f32 in `[0, 1]` optional later).
- CLI mirror (Phase 2): `--check-clip-repetition` → `config.validation.check_clip_repetition = true` in `apply_cli_overrides`.
- When flag is **off**, omit `repetition_a` / `repetition_b` from JSON via `skip_serializing_if` or always omit fields (prefer **`#[serde(skip_serializing_if = "Option::is_none")]`** on optional fields so default-off runs produce unchanged JSON shape).

When enabled, fingerprint each prepared clip once and reuse that fingerprint for cross-file match and self-match.

**Behaviour when repetition is detected:**

- Exit code **0** always.
- Phase 2: `repetition_a` / `repetition_b` on each `ClipMatch` in JSON; human line when found or `--verbose`.
- Phase 3: pair confidence ×0.5 when repetition lag ≈ cross-file offset (see [Confidence downgrade](#confidence-downgrade-phase-3)).

---

## Types

```rust
/// Internal repeat within a single prepared clip (domain or infrastructure; Serialize for JSON).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RepetitionFinding {
    /// Positive seconds between repeated content (see Decisions).
    pub lag_secs: f64,
    pub confidence: f32,
    pub items_count: usize,
}

// ClipMatch — add when check_clip_repetition was used for this run (Phase 2):
pub struct ClipMatch {
    // ... existing fields ...
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repetition_a: Option<RepetitionFinding>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repetition_b: Option<RepetitionFinding>,
}
```

Phase 1 may use a side struct `ClipRepetitionDiagnostics { repetition_a, repetition_b }` per clip index, merged into `ClipMatch` in Phase 2.

---

## Phases

### Phase 0 — Spike

- [ ] `match_fingerprints(&fp, &fp)` on synthetic 10s-block-at-0-and-30s clip returns non-zero-lag segments above threshold
- [ ] Record `item_duration_in_seconds()` for default preset (feeds `min_lag_secs`)
- [ ] If library behaves poorly on identical streams, implement and document **fallback**: fingerprint first half vs second half only when `clip_duration >= 2 * min_lag_secs`

### Phase 1 — Core detection

- [ ] `ValidationConfig` on `AppConfig` (full struct including offset-verification fields)
- [ ] `src/infrastructure/chromaprint/repetition.rs` — `detect_clip_repetition(...)`
- [ ] `select_best_nonzero_lag_segment` — filter lag ∈ `[-1, +1]` items, then reuse `select_best_segment` clustering
- [ ] Wire in `align_extracted_pair` loop when `config.validation.check_clip_repetition`
- [ ] Collect `Vec<ClipRepetitionDiagnostics>` parallel to `estimates`; log via `tracing::debug` only
- [ ] Unit tests (see [Tests](#tests)); no `ClipMatch` / stdout changes yet

### Phase 2 — Reporting

- [ ] Extend `build_alignment_result` (or caller) to accept optional repetition diagnostics per clip index
- [ ] Populate `ClipMatch.repetition_a` / `repetition_b` when flag was on
- [ ] `output.rs`: human line per clip when finding present or `show_diagnostics`
- [ ] CLI `--check-clip-repetition`
- [ ] `try_all_tracks`: post-selection `align_extracted_pair(collect_repetition: true)` on winning tracks; merge into `best.clips`
- [ ] Document in PLAN.md and corpus-validation.md

### Phase 3 — Corpus + policy

- [ ] Generator `repeated_segment_clip` in `audio_fixtures.rs` (10s tone @ 0s + copy @ 30s in 60s clip)
- [ ] Manifest case `repeated_segment_in_clip` + corpus harness assertion helper
- [ ] Confidence downgrade in `align_extracted_pair` after `find_offset` (before pushing estimate)
- [ ] Archive this doc

---

## Design

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

Confidence uses existing `segment_confidence(score, items_count, ambiguous)` from `aligner.rs`. Ambiguous multi-lag clusters apply the same ×0.5 penalty as cross-file matching.

### Detection algorithm

```text
fp = fingerprint(prepared_clip)
segments = match_fingerprints(&fp.data, &fp.data, config)

segments' = segments where |offset2 - offset1| > 1 item
cluster segments' by internal lag (select_best_segment logic)
(segment, ambiguous) = best cluster
confidence = segment_confidence(segment.score, segment.items_count, ambiguous)
if confidence >= min_repetition_confidence:
  lag_secs = abs(offset2 - offset1) * item_secs
  → Some(RepetitionFinding { lag_secs, confidence, items_count })
else:
  → None
```

| Constant | Value | Rationale |
|----------|-------|-----------|
| `min_lag_items` | 40 | ~5 s at default preset; segments shorter than this are excluded before clustering |
| `min_lag_secs` | `min_lag_items * item_duration` | Short-clip guard input |
| `max_lags_reported` | 1 | Primary repeat only; harmonic repeats (30s, 60s) not listed separately in v1 |

### Integration (`align_extracted_pair`)

Single-track and winning `try_all_tracks` pair both use `align_extracted_pair`:

```text
for each clip index:
  prepare clip_a, clip_b (existing)
  if skippable prepare error → push zero estimate; push empty diagnostics; continue

  fingerprint_a, fingerprint_b (existing)

  if collect_repetition:
      dur_a = clip_a.duration_secs()   // from sample_rate + len
      dur_b = clip_b.duration_secs()
      repetition_a = detect_clip_repetition(&fingerprint_a, dur_a, preset, min_rep_conf)
      repetition_b = detect_clip_repetition(&fingerprint_b, dur_b, preset, min_rep_conf)
      debug!(?repetition_a, ?repetition_b, "clip self-repetition")
  else:
      repetition_a, repetition_b = None

  estimate = find_offset(&fingerprint_a, &fingerprint_b) (existing)
  estimate = refine_offset_with_pcm(...) (existing)

  Phase 3: if should_downgrade(repetition_a, repetition_b, estimate.offset_secs):
      estimate.confidence *= 0.5

  push estimate; push ClipRepetitionDiagnostics { repetition_a, repetition_b }

build_alignment_result(windows, estimates, ..., repetitions: Option<&[ClipRepetitionDiagnostics]>)
  Phase 1: repetitions ignored
  Phase 2: zip into ClipMatch.repetition_a / repetition_b
```

**`try_all_tracks` path:** `align_extracted_pair` accepts `collect_repetition: bool`. The track search loop calls it with `collect_repetition: false` (no self-match work during search). After `best` is chosen, record winning `(track_a, track_b)` indices, run **one** more `align_extracted_pair(..., collect_repetition: true)` on that pair, and merge `repetition_a` / `repetition_b` from that pass into `best.clips` (offset/confidence from the search pass unchanged).

**`align_single_track_pair` path:** call `align_extracted_pair(..., collect_repetition: config.validation.check_clip_repetition)` once.

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

- Compare against `offset_secs.abs()` because repetition lag is positive and offset may be negative.
- Downgrade applies to `ClipMatchEstimate.confidence` before `build_alignment_result`; `aligned` still derived from (possibly reduced) confidence vs `min_match_score`.
- v1 does **not** force `aligned = false` when downgrade drops confidence below threshold (document: user sees lower confidence, not a new failure mode).

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
    "repetition_a": { "lag_secs": 30.0, "confidence": 0.72, "items_count": 48 },
    "repetition_b": null
  }]
}
```

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
| Assert | `repetition_a` or `repetition_b` on start clip: `lag_secs` ∈ [28, 32], `confidence >= 0.5` |
| Assert | `recommended_offset_secs` within existing +3s tolerance |
| Assert | Exit / alignment success unchanged (exit 0, `start_aligned` true) |

---

## Tests

| Test | Phase | Asserts |
|------|-------|---------|
| `detect_clip_repetition_none_on_chirp` | 1 | Monotonic chirp → `None` |
| `detect_clip_repetition_none_on_empty` | 1 | Empty fingerprint → `None` |
| `detect_clip_repetition_none_when_too_short` | 1 | 8s clip, default min lag → `None` |
| `detect_clip_repetition_finds_copied_block` | 1 | 10s @ 0s + 10s @ 30s → `lag_secs` ∈ [28, 32], `confidence >= 0.5` |
| `select_best_nonzero_lag_ignores_zero_cluster` | 1 | Synthetic segments with strong lag-0 and weaker lag-N → picks N |
| `align_repetition_debug_only_phase1` | 1 | Flag on; no `repetition_*` on `ClipMatch` in result yet |
| `align_with_check_enabled_json_fields` | 2 | Flag on, JSON → `repetition_a`/`repetition_b` keys on clips |
| `align_human_shows_repeat_line` | 2 | Flag on, finding present → human output contains `internal repeat` |
| `align_human_verbose_shows_none` | 2 | Flag on, verbose, no finding → `no internal repeat` |
| `downgrade_when_lag_matches_offset` | 3 | Repeat @ 30s, offset +30s → confidence ×0.5 |
| `try_all_tracks_repetition_on_winner_only` | 2 | Dual-track case; repetition detected only after winning pair chosen (mock/spy: `detect` call count == num_clips, not × track pairs) |
| Corpus `repeated_segment_in_clip` | 3 | Full harness assertions per [Corpus case](#corpus-case-repeated_segment_in_clip) |

---

## References

- `src/infrastructure/chromaprint/aligner.rs` — `match_fingerprints`, `select_best_segment`, `segment_confidence`
- `src/infrastructure/chromaprint/repetition.rs` — **new** `detect_clip_repetition`
- `src/application/align_videos.rs` — `align_extracted_pair`, `align_best_track_pair`
- `src/domain/alignment.rs` — `ClipMatch`, `build_alignment_result`
- `src/domain/pcm_preparation.rs` — `prepare_clip_for_fingerprint`, `select_aligned_subclip_pair`
- `src/application/config.rs` — `ValidationConfig`
- `src/infrastructure/cli/args.rs`, `mod.rs` — `--check-clip-repetition`
- `src/infrastructure/cli/output.rs` — human repetition lines
- `src/application/testing/audio_fixtures.rs`, `corpus_fixtures.rs` — generator + manifest
- [TEMP-offset-verification-plan.md](TEMP-offset-verification-plan.md) — shared `ValidationConfig`
- [BACKLOG.md](../BACKLOG.md) — add item when work starts
