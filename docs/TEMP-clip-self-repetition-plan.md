# Temporary plan: clip self-repetition check

> **Status:** Phase 0 complete (2026-06-06). Phase 1 not started. Workspace refactor (Phases 1–4) complete — paths below are **`crates/clip-sync`** + **`crates/clip-sync-cli`**. Archive to `docs/archive/clip-self-repetition-plan.md` when shipped.

**Problem:** A clip whose audio repeats internally (loop, rebroadcast, duplicated segment) can produce ambiguous Chromaprint matches — both cross-file alignment and offset verification may latch onto the wrong lag with high confidence.

**Goal:** Optional per-clip diagnostic that fingerprints each **prepared** clip against itself, detects strong non-zero internal repeats, and surfaces findings on `ClipMatch`. Off by default (`align.validation.check_clip_repetition` on `AlignConfig`). Diagnostic only — never changes exit code or clears `recommended_offset_secs` in v1.

**Workspace split:** Engine logic in **`crates/clip-sync`** (library hexagon). Flags, TOML load, and stdout formatting in **`crates/clip-sync-cli`**. **`clip-sync-repair`** consumes `align_with_defaults` / embedded `AlignmentResult` only — no repair-specific repetition UI in v1 (enable via shared `[validation]` in repair TOML if needed). See [Config](#config) and [Phases](#phases).

---

## Current codebase baseline

Audit against the tree **after** workspace migration (2026-06-08). Use this table to pick up work without re-reading the repo.

| Area | Path | Current state | Target phase |
|------|------|---------------|--------------|
| **AlignConfig** | `crates/clip-sync/src/application/config.rs` | `{ clip, alignment }` only — **no `validation` field** | 1 |
| **AppConfig** | `crates/clip-sync-cli/src/infrastructure/config.rs` | Flattens `align: AlignConfig` + `output` + `logging` | 2 (TOML `[validation]`) |
| **Align loop** | `crates/clip-sync/src/application/align_videos.rs` | `align_extracted_pair(extracted_a, extracted_b, config)` → `build_alignment_result`; `align_best_track_pair` → `AlignmentOutcome { result, track_a, track_b, … }` | 1–2 |
| **ClipMatch** | `crates/clip-sync/src/domain/alignment.rs` | No `repetition` field; `AlignmentResult` has `offset_drift_secs`, `start_overlap`, `high_rate_refinement` | 2 |
| **MonoPcmClip** | `crates/clip-sync/src/domain/mono_pcm_clip.rs` | `effective_decoded_sample_count()` only — **no `duration_secs()`** | 1 |
| **Segment helpers** | `aligner.rs` | Private `select_best_segment`, `segment_confidence` | 1 → `matching.rs` |
| **Spike helpers** | `repetition_spike.rs` | `pub(crate)` `select_best_nonzero_lag_segment`, duplicate segment helpers; **`#[cfg(test)]` in `mod.rs`** | 1 → promote to `matching.rs` / `repetition.rs` |
| **Production detect** | — | **Missing** (`repetition.rs`) | 1 |
| **CLI flags** | `clip-sync-cli/.../cli/args.rs` (`Cli`) | No `--check-clip-repetition` | 2 |
| **CLI overrides** | `clip-sync-cli/.../cli/mod.rs` | `apply_cli_overrides` sets clip/alignment/output/logging only | 2 |
| **Human output** | `clip-sync-cli/.../cli/output.rs` | `format_clip_line(clip, show_diagnostics)` — no repetition lines | 2 |
| **JSON output** | same | `serde_json::to_string_pretty(result)` — picks up `ClipMatch.repetition` once on domain types | 2 |
| **Corpus harness** | `crates/clip-sync/src/application/testing/corpus_fixtures.rs` | Workspace `tests/corpus/manifest.toml` | 3 |
| **Repair** | `clip-sync-repair` | `ScanGaps` → `align_with_defaults(AlignConfig)`; gap report embeds full `AlignmentResult` | — (inherits lib JSON when flag on) |

**Phase 0 artifact:** `crates/clip-sync/src/infrastructure/chromaprint/repetition_spike.rs` — spike tests + half-vs-half prototype. Do **not** ship spike API to users; Phase 1 copies proven logic into `repetition.rs` and shared `matching.rs`.

### Phase 0 outcome (2026-06-06)

| Check | Result |
|-------|--------|
| `match_fingerprints(&fp, &fp)` on 10s@0 + 10s@30 synthetic clip | **Rejected** — library returns a single lag-0 segment only (`repetition_spike.rs` tests). |
| Default preset `item_duration_in_seconds()` | **~0.124 s** (Test2). `min_lag_secs` ≈ **4.95 s** (`MIN_LAG_ITEMS = 40` in spike). |
| **Fallback chosen** | **Half-vs-half** fingerprint match at timeline midpoint, with `\|lag_items\| > 1` and `\|lag_secs − duration/2\| ≤ 3 s` guard (`HALF_LAG_MIDPOINT_TOLERANCE_SECS`). |
| Synthetic repeat (silent gaps) | Detects lag **~32.5 s** on 60 s clip (confidence ≥ 0.5). |
| Monotonic chirp control | **No finding** (midpoint guard rejects ~41 s false peak). |
| Chirp-filled repeat (Phase 3 corpus shape) | **Not detected** in spike — tone block drowned by chirp; corpus fixture may need stronger tone or PCM assist in Phase 3. |

Phase 1 detection should implement half-vs-half (not full self-match). Promote spike helpers from `repetition_spike.rs` into `matching.rs` / `repetition.rs`; keep spike tests as regression guard.

---

## Decisions

Locked before implementation. Change only with an explicit plan revision.

| Topic | Decision |
|-------|----------|
| **Phase 0 gate** | **Done (2026-06-06).** Full self-match rejected; half-vs-half + midpoint guard chosen. See spike module. |
| **Input audio** | Same prepared PCM / fingerprints used for cross-file match: after `prepare_clip_for_fingerprint`, and after `select_aligned_subclip_pair` when `window_slide_secs > 0`. Not raw extracts. |
| **Which files** | Check **both** video A and video B for each clip index in the align loop. |
| **Report model** | `repetition: Option<ClipRepetitionReport>` on `ClipMatch` when check ran; `None` on `ClipMatch` when check was off (field omitted from JSON). |
| **`try_all_tracks`** | Run repetition only for the **winning** track pair. `align_best_track_pair` returns `AlignmentOutcome { track_a, track_b, result, … }`; post-selection pass re-extracts that pair via `extract_clips(..., Some(&track))`. Do not check every candidate pair during search. |
| **Lag-zero exclusion** | Always drop segments with internal lag in `[-1, +1]` items before clustering. Lag ≈ 0 is expected self-similarity, not a repeat. |
| **`lag_secs` sign** | Report **positive** duration only: “content near the start of the clip reappears approximately `lag_secs` later.” Normalize `abs(offset2 - offset1) * item_secs`. |
| **Detection threshold** | `validation.min_repetition_confidence` (default `0.5`). Independent knob from `alignment.min_match_score`. |
| **Short clips** | Return `None` when prepared clip duration `< 2 * min_lag_secs` (~10 s with default `min_lag_items`). |
| **Skipped / empty clips** | Skip check when prepare fails (`InsufficientAudio` / `EmptyClip` — same branches as align loop) or fingerprint is empty. |
| **End clip prior path** | When `use_end_prior` skips cross-file fingerprinting, still fingerprint prepared A/B for repetition if `check_clip_repetition` (extra `fingerprinter.fingerprint` calls only). In `align_extracted_pair`, the fingerprint calls must be hoisted **before** the `if use_end_prior` branch so they are available for both cross-file alignment and self-match regardless of path taken. |
| **Clip duration** | Add `MonoPcmClip::duration_secs()` in `crates/clip-sync/src/domain/mono_pcm_clip.rs`. |
| **Shared matching helpers** | Extract helpers to `crates/clip-sync/src/infrastructure/chromaprint/matching.rs` as `pub(crate)`; used by `aligner.rs` and `repetition.rs`. Use `&[Segment]` (owned slice) as the canonical signature — `aligner.rs` already uses this; `repetition_spike.rs` uses `&[&Segment]` (slice of refs) and must be adapted when promoting to `matching.rs`. |
| **Architecture** | Detection in lib `infrastructure/chromaprint/repetition.rs`; report types in lib `domain/alignment.rs`. No new port trait in v1. CLI only formats existing `AlignmentResult`. |
| **Phase 1 scope** | Detection + unit tests + `tracing::debug` only. No stdout/JSON/CLI until Phase 2. |
| **User-visible failure** | Diagnostic only: exit code stays **0**; never clear `recommended_offset_secs` in v1. |
| **JSON (Phase 2)** | Flag **off** → `repetition` key absent on `ClipMatch`. Flag **on** → `repetition` object always present per clip; inner `a` / `b` are `null` when no finding (explicit nulls). |
| **Human (Phase 2)** | Repetition lines only when a finding exists **or** `--verbose` (then show “no internal repeat” per video). |
| **Confidence downgrade (Phase 3)** | If either repetition lag is within ±**1 s** of `offset_secs.abs()`, multiply displayed `ClipMatch.confidence` by `0.5`. Compute `aligned` from **pre-downgrade** confidence so downgrade never flips `aligned` in v1. |
| **Corpus assertions** | See [Tests](#tests). |

---

## Config

Shared with [TEMP-offset-verification-plan.md](TEMP-offset-verification-plan.md). Implement the **full** `ValidationConfig` once when either feature lands.

### Library (`AlignConfig.validation`)

`ValidationConfig` is **not in the tree yet**. Add it on **`AlignConfig`** in `crates/clip-sync/src/application/config.rs` — it drives `AlignVideos` / `align_with_defaults`, not the CLI crate.

**Current `AlignConfig` (today):**

```rust
pub struct AlignConfig {
    pub clip: ClipConfig,
    pub alignment: AlignmentConfig,
    // validation: ValidationConfig,  // Phase 1 — add with #[serde(default)]
}
```

**Target (Phase 1):**

```rust
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AlignConfig {
    #[serde(default)]
    pub clip: ClipConfig,
    #[serde(default)]
    pub alignment: AlignmentConfig,
    #[serde(default)]
    pub validation: ValidationConfig,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ValidationConfig {
    #[serde(default)]
    pub check_clip_repetition: bool,
    #[serde(default = "default_min_repetition_confidence")]
    pub min_repetition_confidence: f32,
    // Placeholder fields for offset-verification plan (TEMP-offset-verification-plan.md).
    // Added here so the shared [validation] TOML section is defined once; these fields
    // are inert (no code reads them) until that plan's Phase 1 lands.
    #[serde(default)]
    pub verify_offset: bool,
    #[serde(default = "default_min_verification_confidence")]
    pub min_verification_confidence: f32,
}

fn default_min_repetition_confidence() -> f32 {
    0.5
}
```

- `load_align_config` in `crates/clip-sync/src/infrastructure/config/file.rs` deserializes top-level `[clip]`, `[alignment]`, `[validation]` into `AlignConfig`.
- `AlignVideosRequest.config` is `AlignConfig`; use `config.validation.check_clip_repetition` in `align_extracted_pair`.
- `AlignConfig::validate()` today only calls `clip.validate()` — no extra validation rules for `[validation]` in v1.

### CLI (`AppConfig` — TOML + flags only)

`AppConfig` in `crates/clip-sync-cli/src/infrastructure/config.rs` already flattens `align: AlignConfig`, so user TOML is **unchanged**:

```toml
[clip]
# …

[alignment]
# …

[validation]
check_clip_repetition = false
min_repetition_confidence = 0.5
verify_offset = false
min_verification_confidence = 0.5

[output]
# …

[logging]
# …
```

- No extra validation rules on `ValidationConfig` for v1.
- **CLI Phase 2:** `--check-clip-repetition` on `Cli` in `args.rs` → `config.align.validation.check_clip_repetition = true` in `apply_cli_overrides` (`cli/mod.rs`).
- Human verbose lines use existing `config.output.show_diagnostics` (set by `--verbose` today).
- When flag is **off**, `ClipMatch.repetition` stays `None` → key omitted via `#[serde(skip_serializing_if = "Option::is_none")]` (lib domain serde; JSON printing is CLI `output.rs`).

When enabled, fingerprint each prepared clip once and reuse that fingerprint for cross-file match and self-match.

**Behaviour when repetition is detected:**

- Exit code **0** always.
- **Lib Phase 2:** populate `ClipMatch.repetition` on `AlignmentResult`.
- **CLI Phase 2:** human lines when finding or verbose.
- **Lib Phase 3:** confidence display downgrade per [Confidence downgrade](#confidence-downgrade-phase-3); `aligned` unchanged.

---

## Types

All report types live in `crates/clip-sync/src/domain/alignment.rs` (library).

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
// crates/clip-sync/src/domain/mono_pcm_clip.rs (Phase 1)
impl MonoPcmClip {
    pub fn duration_secs(&self) -> f64 {
        self.samples.len() as f64 / f64::from(self.sample_rate)
    }
}
```

---

## Phases

### Phase 0 — Spike (blocking) ✅

**Lib**

- [x] `match_fingerprints(&fp, &fp)` on synthetic 10s-block-at-0-and-30s clip — **lag-0 only; not viable**
- [x] Record `item_duration_in_seconds()` for default preset (~0.124 s Test2; `min_lag_secs` ~4.95 s)
- [x] **Fallback:** half-vs-half fingerprint split + midpoint guard (`crates/clip-sync/src/infrastructure/chromaprint/repetition_spike.rs`, compiled under `#[cfg(test)]` in `mod.rs`)
- [x] Outcome recorded at top of this doc

**CLI:** none

### Phase 1 — Core detection (lib only) ✅

**Lib (`crates/clip-sync`)**

- [x] `ValidationConfig` on `AlignConfig` (full struct including offset-verification fields for shared TOML)
- [x] `MonoPcmClip::duration_secs()` on `crates/clip-sync/src/domain/mono_pcm_clip.rs`
- [x] `infrastructure/chromaprint/matching.rs` — **new**; move `select_best_segment`, `segment_confidence`, `select_best_nonzero_lag_segment` from `aligner.rs` + `repetition_spike.rs`
- [x] `infrastructure/chromaprint/repetition.rs` — **new** `detect_clip_repetition(...)` (production API; port logic from `spike_detect_internal_repeat` / half-vs-half path)
- [x] `infrastructure/chromaprint/mod.rs` — `mod matching; mod repetition;` (keep `repetition_spike` under `#[cfg(test)]`)
- [x] `application/align_videos.rs` — inside per-clip loop: optional self-check when `config.validation.check_clip_repetition`; fingerprints computed conditionally (`!use_end_prior || check_clip_repetition`)
- [x] Collect `Vec<(Option<RepetitionFinding>, Option<RepetitionFinding>)>` parallel to `estimates`; `tracing::debug` only — **do not** extend `build_alignment_result` yet
- [x] Unit tests (see [Tests](#tests)); **`ClipMatch.repetition` still unset**

**CLI (`clip-sync-cli`):** none

### Phase 2 — Reporting (lib domain + CLI stdout) ✅

**Lib (`crates/clip-sync`)**

- [x] Add `repetition: Option<ClipRepetitionReport>` to `ClipMatch` in `domain/alignment.rs`
- [x] After `build_alignment_result`, merge diagnostics into `result.clips[i].repetition` when flag was on
- [x] `try_all_tracks`: disable repetition during search (clone config, set flag off); store winning `ExtractedClips`; run `align_extracted_pair` on winner with repetition on; merge `repetition` field into outcome clips
- [x] Lib tests: `align_json_repetition_object_present_when_flag_on`, `align_json_no_repetition_key_when_flag_off`, `try_all_tracks_repetition_on_winner_only`

**CLI (`crates/clip-sync-cli`)**

- [x] `infrastructure/cli/args.rs` — `--check-clip-repetition` on `Cli`
- [x] `infrastructure/cli/mod.rs` — `apply_cli_overrides`: set `config.align.validation.check_clip_repetition = true`
- [x] `infrastructure/cli/output.rs` — `format_repetition_lines` + `format_human_output` (pub, testable); repetition when finding present or `show_diagnostics`
- [x] `tests/fixtures/analyzer.toml` — `[validation]` block added
- [x] `tests/config_roundtrip.rs` — validation fields asserted in fixture test
- [x] `tests/cli_output.rs` — `align_human_shows_repeat_line`, `align_human_verbose_shows_none`, `align_human_no_repetition_lines_when_repetition_field_absent`, JSON repetition tests
- [ ] Document in [PLAN.md](../PLAN.md) and [docs/corpus-validation.md](corpus-validation.md)

### Phase 3 — Corpus + policy (lib only)

**Lib (`clip-sync`)**

- [ ] `application/testing/audio_fixtures.rs` — generator `repeated_segment_clip` (10s tone @ 0s + copy @ 30s in 60s clip)
- [ ] `application/testing/corpus_fixtures.rs` — manifest case `repeated_segment_in_clip` + assertion helper (`config.validation.check_clip_repetition`)
- [ ] Confidence downgrade in `align_extracted_pair` (display confidence only; `aligned` from pre-downgrade)
- [ ] Test `downgrade_lowers_confidence_not_aligned`
- [ ] Archive this doc → `docs/archive/clip-self-repetition-plan.md`

**CLI:** none (JSON/human already handle downgraded `confidence` from lib)

---

## Design

### Module layout

```text
crates/clip-sync/src/infrastructure/chromaprint/
  mod.rs           # pub mod matching; pub mod repetition; #[cfg(test)] mod repetition_spike;
  matching.rs      # pub(crate) segment selection + confidence (shared)
  repetition.rs    # detect_clip_repetition (production)
  repetition_spike.rs  # Phase 0 tests + spike API (test-only module)
  aligner.rs       # uses matching.rs (refactor from private fns)
  fingerprinter.rs
  config.rs
```

### Detection API

```rust
// crates/clip-sync/src/infrastructure/chromaprint/repetition.rs
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

**Current signature (today):**

```rust
fn align_extracted_pair(
    &self,
    extracted_a: &ExtractedClips,
    extracted_b: &ExtractedClips,
    config: &AlignConfig,
) -> Result<AlignmentResult, AppError>
```

Per-clip loop already: `prepare_clip_for_fingerprint` → (optional subclip) → cross-file `fingerprinter.fingerprint` + `aligner.find_offset` → PCM refine → `estimates.push`. Hook repetition **after prepare**, reusing the same prepared `MonoPcmClip` and the cross-file fingerprints when available.

**Key constraint:** fingerprint calls must be **hoisted before** the `if use_end_prior` branch. Today the end-prior path skips `fingerprinter.fingerprint` entirely; repetition detection needs fingerprints on every non-skipped clip regardless of path taken. When `use_end_prior` is true, the cross-file alignment still uses `refine_offset_around_prior` (PCM only), but fingerprints computed above are consumed by the self-match check.

```text
for each clip index in align_extracted_pair:
  prepare clip_a, clip_b (existing)
  if skippable prepare / skip_unreliable_end → push zero estimate; push empty diagnostics; continue

  // Hoist fingerprints before use_end_prior branch so repetition check can use them on all paths
  fingerprint_a = fingerprinter.fingerprint(&clip_a)
  fingerprint_b = fingerprinter.fingerprint(&clip_b)

  if config.validation.check_clip_repetition:
      repetition_a = detect_clip_repetition(&fingerprint_a, clip_a.duration_secs(), preset, min_conf)
      repetition_b = detect_clip_repetition(&fingerprint_b, clip_b.duration_secs(), preset, min_conf)
      debug!(?repetition_a, ?repetition_b, "clip self-repetition")
  else:
      repetition_a, repetition_b = None

  estimate = if use_end_prior:
      refine_offset_around_prior(...)   // PCM only — fingerprints computed above, not used here
  else:
      aligner.find_offset(&fingerprint_a, &fingerprint_b) + optional PCM refine

  Phase 3: track alignment_confidence vs display_confidence for downgrade

  push estimate; push ClipRepetitionDiagnostics { a: repetition_a, b: repetition_b }

build_alignment_result(...)   // Phase 1: no repetition on ClipMatch yet
Phase 2: merge diagnostics → clips[].repetition on returned AlignmentResult
```

> **Note:** `use_end_prior` is computed before the fingerprint block. Fingerprinting is conditional: `need_fingerprints = !use_end_prior || config.validation.check_clip_repetition`. This preserves the existing end-prior performance characteristic (no extra fingerprint per end clip when the flag is off) while still supplying fingerprints for self-match when the flag is on.

**`align_single_track_pair`:** one `align_extracted_pair` call; repetition gated by `config.validation.check_clip_repetition`.

**`try_all_tracks` path (matches `AlignmentOutcome` in code today):**

```text
align_best_track_pair(session_a, session_b, request):
  mut best: Option<(AlignmentOutcome, f32)> = None
  for track_a in decodable_a:
    for track_b in decodable_b:
      extracted_a/b = extract_clips(..., Some(track))
      result = align_extracted_pair(..., collect_repetition: false)  // Phase 1: flag off in search
      if score > best: best = Some(AlignmentOutcome { result, track_a: extracted_a.track, track_b: extracted_b.track, ... })

  if config.validation.check_clip_repetition:
      let outcome = best?;
      re-extract winning pair only
      run repetition-only pass (or align_extracted_pair with repetition flag, discard alignment recompute)
      merge diagnostics into outcome.result.clips[].repetition
  return best.outcome
```

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

**JSON** — serialized from lib `AlignmentResult` / `ClipMatch` (`serde` on domain types). CLI `output.rs` prints JSON when `OutputFormat::Json`; no extra JSON logic required beyond existing formatter.

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

**Human** — **CLI only** (`crates/clip-sync-cli/src/infrastructure/cli/output.rs`, `print_human` / `format_clip_line`):

Uses `show_diagnostics` from `OutputConfig` (CLI `--verbose` sets this today).

**Human** (repetition found):

```text
  Start clip [0:00–1:00]: aligned, offset +3.000s (confidence 0.85)
    video A: internal repeat ~30.0s (confidence 0.72)
```

**Human** (`--verbose` / `show_diagnostics`, no repetition):

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

| Test | Phase | Crate | Asserts |
|------|-------|-------|---------|
| Phase 0 spike | 0 | lib | Half-vs-half on silent-gap repeat → lag ≈ 30–32.5 s; full self-match lag-0 only; chirp control none |
| `detect_clip_repetition_none_on_chirp` | 1 | lib | Monotonic chirp → `None` |
| `detect_clip_repetition_none_on_empty` | 1 | lib | Empty fingerprint → `None` |
| `detect_clip_repetition_none_when_too_short` | 1 | lib | 8s clip → `None` |
| `detect_clip_repetition_finds_copied_block` | 1 | lib | 10s @ 0s + 10s @ 30s → `lag_secs` ∈ [28, 32], `confidence >= 0.5` |
| `select_best_nonzero_lag_ignores_zero_cluster` | 1 | lib | Strong lag-0 + weaker lag-N → picks N |
| `mono_pcm_clip_duration_secs` | 1 | lib | Helper matches sample len / rate |
| `align_repetition_debug_only_phase1` | 1 | lib | Flag on; `ClipMatch.repetition` still `None` |
| `align_json_repetition_object_when_flag_on` | 2 | lib | Flag on → every clip has `repetition` object; null sides when no finding |
| `align_json_no_repetition_key_when_flag_off` | 2 | lib | Flag off → no `repetition` key |
| `try_all_tracks_repetition_on_winner_only` | 2 | lib | `detect` call count == 2 × num_clips (both sides), not × track pairs |
| `align_human_shows_repeat_line` | 2 | CLI | Finding present → `internal repeat` in human output |
| `align_human_verbose_shows_none` | 2 | CLI | Verbose, no finding → `no internal repeat` |
| `config_validation_roundtrip` | 2 | CLI | TOML `[validation]` deserializes into `AppConfig` |
| `downgrade_lowers_confidence_not_aligned` | 3 | lib | Repeat @ 30s, offset +30s → `confidence` halved, `aligned` still true |
| Corpus `repeated_segment_in_clip` | 3 | lib | Per [Corpus case](#corpus-case-repeated_segment_in_clip) |

---

## References

### Library (`crates/clip-sync`)

| File | Status |
|------|--------|
| `src/infrastructure/chromaprint/matching.rs` | **New** — extract from `aligner.rs` + spike |
| `src/infrastructure/chromaprint/repetition.rs` | **New** — `detect_clip_repetition` |
| `src/infrastructure/chromaprint/repetition_spike.rs` | **Exists** — Phase 0 spike + tests (`#[cfg(test)]` in `mod.rs`) |
| `src/infrastructure/chromaprint/aligner.rs` | **Exists** — refactor to use `matching.rs` |
| `src/application/align_videos.rs` | **Exists** — `align_extracted_pair`, `align_best_track_pair`, `AlignmentOutcome` |
| `src/domain/alignment.rs` | **Exists** — extend `ClipMatch`; `build_alignment_result` |
| `src/domain/mono_pcm_clip.rs` | **Exists** — add `duration_secs()` |
| `src/domain/pcm_preparation.rs` | **Exists** — `prepare_clip_for_fingerprint`, `select_aligned_subclip_pair` |
| `src/application/config.rs` | **Exists** — add `ValidationConfig` on `AlignConfig` |
| `src/infrastructure/config/file.rs` | **Exists** — `load_align_config` |
| `src/application/testing/audio_fixtures.rs`, `corpus_fixtures.rs` | **Exists** — Phase 3 generator + `tests/corpus/manifest.toml` |

### CLI (`crates/clip-sync-cli`)

| File | Status |
|------|--------|
| `src/application/run_align.rs` | **Exists** — `align_with_defaults`; no repetition logic here |
| `src/infrastructure/cli/args.rs` | **Exists** — add flag on `Cli` |
| `src/infrastructure/cli/mod.rs` | **Exists** — `apply_cli_overrides`, `run_inner` |
| `src/infrastructure/cli/output.rs` | **Exists** — human + JSON print |
| `src/infrastructure/config.rs` | **Exists** — `AppConfig` flattens `AlignConfig` |
| `tests/cli_output.rs`, `tests/config_roundtrip.rs` | **Exists** — extend for validation / repetition lines |

### Other

- [TEMP-offset-verification-plan.md](TEMP-offset-verification-plan.md) — shared `ValidationConfig`
- [PLAN.md](../PLAN.md) — workspace layout (`crates/clip-sync`, `crates/clip-sync-cli`)
- [BACKLOG.md](../BACKLOG.md) — Phase 5 validation diagnostics
- `crates/clip-sync-repair` — embeds `AlignmentResult` in `GapReport`; repetition appears in nested JSON when enabled via repair TOML `[validation]`
